// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

package replay

import (
	"bytes"
	"fmt"
	"log"
	"log/slog"
	"math/big"
	"testing"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	cc "github.com/0xsoniclabs/carmen/go/common"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
	"github.com/Fantom-foundation/lachesis-base/hash"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/core/vm/program"
	"github.com/ethereum/go-ethereum/crypto"
	gethlog "github.com/ethereum/go-ethereum/log"
	"github.com/ethereum/go-ethereum/trie"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
	"google.golang.org/protobuf/proto"
)

// executeLiveBlock executes the block with ExecuteBlock and an empty hash
// history, and drops the header.
func executeLiveBlock(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error) {
	receipts, _, err := e.ExecuteBlock(state, block, &blockHashHistory{})
	return receipts, err
}

// executeArchiveBlock executes the block with ExecuteBlockOnArchive and an
// empty hash history.
func executeArchiveBlock(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error) {
	return e.ExecuteBlockOnArchive(state, block, &blockHashHistory{})
}

func TestNewSonicExecutor_KeepsDefaultLoggersAndSilencesLedger(t *testing.T) {
	gethlogDefault := gethlog.Root()
	slogDefault := slog.Default()
	logWriter, logFlags := log.Writer(), log.Flags()
	t.Cleanup(func() {
		gethlog.SetDefault(gethlogDefault)
		slog.SetDefault(slogDefault)
		log.SetOutput(logWriter)
		log.SetFlags(logFlags)
	})
	var out bytes.Buffer
	logger := gethlog.NewLogger(slog.NewTextHandler(&out, nil))
	gethlog.SetDefault(logger)
	slogger := slog.New(slog.NewTextHandler(&out, nil))
	slog.SetDefault(slogger)
	log.SetOutput(&out)
	log.SetFlags(log.Lmsgprefix)

	executor, state, blocks := newTestSonicExecutor(t, sonicMetadataStore())
	require.Same(t, logger, gethlog.Root())
	require.Same(t, slogger, slog.Default())
	require.Same(t, &out, log.Writer())
	require.Equal(t, log.Lmsgprefix, log.Flags())

	_, err := executeLiveBlock(executor, state, withTransactions(t, blocks[1]))
	require.NoError(t, err)
	require.Empty(t, out.String())
}

func TestSonicExecutor_ExecuteBlock_AppliesUpgrades(t *testing.T) {
	// The upgrades change the gas used by a transaction with excess gas: with
	// the single proposer block formation enabled, no excess gas is charged,
	// with it disabled, 10% of the excess gas is charged.
	noExcessGasCharges := opera.GetAllegroUpgrades()
	noExcessGasCharges.SingleProposerBlockFormation = true
	withExcessGasCharges := opera.GetAllegroUpgrades()
	withExcessGasCharges.SingleProposerBlockFormation = false

	metadataStore := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 0, Rules: opera.FakeNetRules(withExcessGasCharges)},
				{Block: 5, Rules: opera.FakeNetRules(noExcessGasCharges)},
				{Block: 10, Rules: opera.FakeNetRules(withExcessGasCharges)},
				{Block: 15, Rules: opera.FakeNetRules(noExcessGasCharges)},
			},
		},
	}
	executor, state, blocks := newTestSonicExecutor(t, metadataStore)

	for number := uint64(1); number < uint64(len(blocks)); number++ {
		receipts, err := executeLiveBlock(executor, state, withTransactions(t, blocks[number], sign(&types.LegacyTx{Nonce: number - 1, Gas: 50_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}})))
		require.NoError(t, err)
		require.Len(t, receipts, 1)
		require.Equal(t, types.ReceiptStatusSuccessful, receipts[0].Status)

		if metadataStore.GetRulesAtBlock(number).Upgrades.SingleProposerBlockFormation {
			require.Equal(t, uint64(21_000), receipts[0].GasUsed)
		} else {
			require.Greater(t, receipts[0].GasUsed, uint64(21_000))
		}
	}
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_CommitRulesUpdateOnlyLiveWhenEncounteringAnEpochSealingTx(t *testing.T) {
	// An epoch sealing transaction must be an internal transaction to the
	// driver contract with the correct data.
	// Internal transitions must have YParity=0 and R=0 which produces V=0,
	// R=0, which satisfies internaltx.IsInternal.
	epochSealingTx := convert.ToBerthaTransaction(types.NewTx(
		&types.LegacyTx{
			Gas:      25000,
			To:       &driver.ContractAddress,
			Data:     drivercall.SealEpoch(nil),
			GasPrice: big.NewInt(0),
			V:        big.NewInt(0),
			R:        big.NewInt(0),
		},
	))
	nonEpochSealingTx := sign(&types.LegacyTx{Gas: 21_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}})
	fakeNetRules := opera.FakeNetRules(opera.GetSonicUpgrades())

	cases := map[string]struct {
		execute           func(*sonicExecutor, *State, *types.Block) (types.Receipts, error)
		isLive            bool
		tx                *blockdb.Transaction
		expectCommitRules bool
	}{
		"ExecuteBlock with EpochSealingTx": {
			execute:           executeLiveBlock,
			isLive:            true,
			tx:                epochSealingTx,
			expectCommitRules: true,
		},
		"ExecuteBlock with NonEpochSealingTx": {
			execute: executeLiveBlock,
			isLive:  true,
			tx:      nonEpochSealingTx,
		},
		"ExecuteBlockOnArchive with EpochSealingTx": {
			execute: executeArchiveBlock,
			tx:      epochSealingTx,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			metadataStore := NewMockMetadataStore(ctrl)
			rules := metadataStore.EXPECT().GetRulesAtBlock(uint64(1)).Return(fakeNetRules)
			if tc.isLive {
				// The ledger reads the upgrades of block 1 and the rules of its head, block 0.
				rules.Times(2)
				metadataStore.EXPECT().GetRulesAtBlock(uint64(0)).Return(fakeNetRules)
			} else {
				metadataStore.EXPECT().GetUpgradeHeights().Return([]opera.UpgradeHeight{{Upgrades: fakeNetRules.Upgrades}})
			}
			metadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
			if tc.expectCommitRules {
				// The epoch sealing block runs with the committed rules.
				gomock.InOrder(metadataStore.EXPECT().CommitRules(uint64(1)).Return(nil), rules)
			}
			executor, state, blocks := newTestSonicExecutor(t, metadataStore)
			require.NoError(t, state.db.Flush())

			block := withTransactions(t, blocks[1], tc.tx)
			receipts, err := tc.execute(executor, state, block)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_OnlyLiveStatePatchesRules(t *testing.T) {
	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	// The driver emits its calldata as an UpdateNetworkRules event.
	driverCode := program.New().
		Op(vm.CALLDATASIZE).Push(0).Push(0).Op(vm.CALLDATACOPY).
		Push(driverpos.Topics.UpdateNetworkRules).Op(vm.CALLDATASIZE).Push(0).Op(vm.LOG1).
		Bytes()
	tx := sign(&types.LegacyTx{
		Gas:      100_000,
		GasPrice: big.NewInt(1e11),
		To:       &driver.ContractAddress,
		Data:     makeUpdateNetworkRulesLog(diff).Data,
	})
	fakeNetRules := opera.FakeNetRules(opera.GetSonicUpgrades())

	tests := map[string]struct {
		execute func(*sonicExecutor, *State, *types.Block) (types.Receipts, error)
		isLive  bool
	}{
		"ExecuteBlock":          {execute: executeLiveBlock, isLive: true},
		"ExecuteBlockOnArchive": {execute: executeArchiveBlock},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			metadataStore := NewMockMetadataStore(ctrl)
			metadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
			if tt.isLive {
				// The ledger reads the upgrades of block 1 and the rules of its head, block 0.
				metadataStore.EXPECT().GetRulesAtBlock(uint64(1)).Return(fakeNetRules).Times(2)
				metadataStore.EXPECT().GetRulesAtBlock(uint64(0)).Return(fakeNetRules)
				metadataStore.EXPECT().PatchRules(uint64(1), diff)
			} else {
				metadataStore.EXPECT().GetRulesAtBlock(uint64(1)).Return(fakeNetRules)
				metadataStore.EXPECT().GetUpgradeHeights().Return([]opera.UpgradeHeight{{Upgrades: fakeNetRules.Upgrades}})
			}

			executor, state, blocks := newTestSonicExecutor(t, metadataStore, makefakegenesis.Account{Address: driver.ContractAddress, Code: driverCode})
			require.NoError(t, state.db.Flush())

			receipts, err := tt.execute(executor, state, withTransactions(t, blocks[1], tx))
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Len(t, receipts[0].Logs, 1)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_AppliesCorrections(t *testing.T) {
	metadataStore := sonicMetadataStore()
	metadataStore.metadata.Corrections = Corrections{
		1: {{1}: {Balance: uint256.NewInt(1000), Storage: map[cc.Key]cc.Value{{3}: {4}}}},
	}
	executor, state, blocks := newTestSonicExecutor(t, metadataStore)

	_, err := executeLiveBlock(executor, state, withTransactions(t, blocks[1]))
	require.NoError(t, err)
	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
	require.Equal(t, cc.Value{4}, state.db.GetState(cc.Address{1}, cc.Key{3}))
}

func TestSonicExecutor_ExecuteBlock_ReturnsHeaderOfExecutedBlockWithRecordedStateRoot(t *testing.T) {
	executor, state, blocks := newTestSonicExecutor(t, sonicMetadataStore())
	// Block 1 follows block 0 by 1s and 500ns.
	blocks[1].ExtraData = inter.EncodeExtraData(time.Unix(int64(blocks[1].Timestamp), 500), time.Second+500)
	// Init code that emits a log.
	block := withTransactions(t, blocks[1], sign(&types.LegacyTx{
		Gas:      100_000,
		GasPrice: big.NewInt(1e11),
		Data:     []byte{byte(vm.PUSH1), 0, byte(vm.PUSH1), 0, byte(vm.LOG0)},
	}))

	receipts, header, err := executor.ExecuteBlock(state, block, &blockHashHistory{})
	require.NoError(t, err)
	require.Len(t, receipts, 1)
	require.Len(t, receipts[0].Logs, 1)
	require.Equal(t, block.ParentHash(), header.ParentHash)
	require.Equal(t, block.Extra(), header.Extra)
	require.Equal(t, block.TxHash(), header.TxHash)
	require.Equal(t, receipts[0].GasUsed, header.GasUsed)
	require.Equal(t, types.MergeBloom(receipts), header.Bloom)
	require.Equal(t, types.DeriveSha(receipts, trie.NewStackTrie(nil)), header.ReceiptHash)
	root, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.NotEqual(t, root, block.Root())
	require.Equal(t, block.Root(), header.Root)
}

func TestSonicExecutor_ExecuteBlock_ReportsFailures(t *testing.T) {
	epochSealingTx := convert.ToBerthaTransaction(types.NewTx(&types.LegacyTx{To: &driver.ContractAddress, Data: drivercall.SealEpoch(nil), V: big.NewInt(0), R: big.NewInt(0)}))
	tests := map[string]struct {
		setup       func(*testing.T, *sonicExecutor, *State)
		modifyBlock func(*blockdb.Block)
		wantErr     string
	}{
		"ledger head fails": {
			setup: func(t *testing.T, e *sonicExecutor, _ *State) {
				ledger := gossip.NewMockLedger(gomock.NewController(t))
				ledger.EXPECT().Head().Return(nil, fmt.Errorf("injected"))
				e.ledger = ledger
			},
			wantErr: "failed to get the ledger's head for block 1",
		},
		"block not following head": {
			modifyBlock: func(b *blockdb.Block) { b.Number = 2 },
			wantErr:     "block 2 does not follow the ledger's head 0",
		},
		"invalid extra data": {
			modifyBlock: func(b *blockdb.Block) { b.ExtraData = nil },
			wantErr:     "failed to decode extra data of block 1",
		},
		"bundle insertion fails": {
			setup: func(_ *testing.T, e *sonicExecutor, _ *State) {
				e.metadataStore.(*BlockDBMetadataStore).metadata.FailedBundles = FailedBundles{1: {{Pos: 1, Tx: types.NewTx(&types.LegacyTx{})}}}
			},
			wantErr: "failed to insert failed bundles into block 1",
		},
		"commit rules fails": {
			setup: func(t *testing.T, e *sonicExecutor, _ *State) {
				metadataStore := NewMockMetadataStore(gomock.NewController(t))
				metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
				metadataStore.EXPECT().CommitRules(uint64(1)).Return(fmt.Errorf("injected"))
				e.metadataStore = metadataStore
			},
			modifyBlock: func(b *blockdb.Block) { b.Transactions = []*blockdb.Transaction{epochSealingTx} },
			wantErr:     "failed to commit rules at block 1",
		},
		"block time not after head": {
			modifyBlock: func(b *blockdb.Block) { b.Timestamp-- },
			wantErr:     "failed to begin block 1",
		},
		"skipped transaction": {
			modifyBlock: func(b *blockdb.Block) {
				b.Transactions = []*blockdb.Transaction{sign(&types.LegacyTx{Nonce: 123, Gas: 21_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}})}
			},
			wantErr: "was skipped",
		},
		"end block fails": {
			setup: func(t *testing.T, _ *sonicExecutor, s *State) {
				db := carmen.NewMockStateDB(gomock.NewController(t))
				db.EXPECT().BeginBlock()
				db.EXPECT().EndBlock(uint64(1)).Return(nil, fmt.Errorf("injected"))
				live := s.db
				t.Cleanup(func() { s.db = live })
				s.db = db
			},
			wantErr: "failed to finalize block 1",
		},
		"check fails": {
			setup: func(t *testing.T, _ *sonicExecutor, s *State) {
				ctrl := gomock.NewController(t)
				staged := carmen.NewMockStagedBlock(ctrl)
				staged.EXPECT().Commit()
				db := carmen.NewMockStateDB(ctrl)
				db.EXPECT().BeginBlock()
				db.EXPECT().EndBlock(uint64(1)).Return(staged, nil)
				db.EXPECT().Check().Return(fmt.Errorf("injected"))
				live := s.db
				t.Cleanup(func() { s.db = live })
				s.db = db
			},
			wantErr: "injected",
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			executor, state, blocks := newTestSonicExecutor(t, sonicMetadataStore())
			if tt.setup != nil {
				tt.setup(t, executor, state)
			}
			block := proto.Clone(blocks[1]).(*blockdb.Block)
			if tt.modifyBlock != nil {
				tt.modifyBlock(block)
			}
			converted, err := convert.ConvertToGethBlock(block)
			require.NoError(t, err)

			_, err = executeLiveBlock(executor, state, converted)
			require.ErrorContains(t, err, tt.wantErr)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_ServeBlockHashFromHashHistory(t *testing.T) {
	executeBlock := func(e *sonicExecutor, state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
		receipts, _, err := e.ExecuteBlock(state, block, hashHistory)
		return receipts, err
	}
	executeBlockOnArchive := func(e *sonicExecutor, state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
		return e.ExecuteBlockOnArchive(state, block, hashHistory)
	}
	tests := map[string]struct {
		execute      func(*sonicExecutor, *State, *types.Block, *blockHashHistory) (types.Receipts, error)
		hashOfBlock1 common.Hash
		wantStatus   uint64
	}{
		"ExecuteBlock with hash":             {execute: executeBlock, hashOfBlock1: common.Hash{1}, wantStatus: types.ReceiptStatusSuccessful},
		"ExecuteBlock without hash":          {execute: executeBlock, wantStatus: types.ReceiptStatusFailed},
		"ExecuteBlockOnArchive with hash":    {execute: executeBlockOnArchive, hashOfBlock1: common.Hash{1}, wantStatus: types.ReceiptStatusSuccessful},
		"ExecuteBlockOnArchive without hash": {execute: executeBlockOnArchive, wantStatus: types.ReceiptStatusFailed},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			executor, state, blocks := newTestSonicExecutor(t, sonicMetadataStore())
			for _, block := range blocks[1:4] {
				_, err := executeLiveBlock(executor, state, withTransactions(t, block))
				require.NoError(t, err)
			}
			require.NoError(t, state.db.Flush())

			// Init code that reverts if BLOCKHASH(1) is zero. It runs in block 4,
			// since the ledger serves the hashes of its head block and its parent.
			block := withTransactions(t, blocks[4], sign(&types.LegacyTx{
				Gas:      100_000,
				GasPrice: big.NewInt(1e11),
				Data:     common.FromHex("0x600140600a57600080fd5b00"),
			}))
			hashHistory := &blockHashHistory{}
			hashHistory.SetBlockHash(1, tt.hashOfBlock1)

			receipts, err := tt.execute(executor, state, block, hashHistory)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Equal(t, tt.wantStatus, receipts[0].Status)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_ProcessFailedBundles(t *testing.T) {
	upgrades := opera.GetBrioUpgrades()
	upgrades.TransactionBundles = true
	signer := types.LatestSignerForChainID(new(big.Int).SetUint64(opera.FakeNetworkID))
	// The bundle's single root step runs init code that reverts, so the bundle
	// fails without being reverted before Canto.
	reverting := func(nonce uint64) *types.Transaction {
		return bundle.NewBuilder().WithSigner(signer).With(bundle.Step(sender, &types.AccessListTx{Nonce: nonce, Gas: 100_000, GasPrice: big.NewInt(1e11), Data: []byte{byte(vm.PUSH1), 0, byte(vm.PUSH1), 0, byte(vm.REVERT)}})).Build()
	}
	executor, state, blocks := newTestSonicExecutor(t, &BlockDBMetadataStore{metadata: Metadata{
		RulesUpdateHeights: []RulesUpdateHeight{{Rules: opera.FakeNetRules(upgrades)}},
		FailedBundles:      FailedBundles{1: {{Pos: 0, Tx: reverting(0)}, {Pos: 1, Tx: reverting(2)}}},
	}})
	block := withTransactions(t, blocks[1], sign(&types.LegacyTx{Nonce: 1, Gas: 21_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}}))

	receipts, header, err := executor.ExecuteBlock(state, block, &blockHashHistory{})
	require.NoError(t, err)
	require.Len(t, receipts, 1)
	require.Greater(t, receipts[0].CumulativeGasUsed, receipts[0].GasUsed)
	require.Greater(t, header.GasUsed, receipts[0].CumulativeGasUsed)
	require.Equal(t, block.TxHash(), header.TxHash)
	require.Equal(t, types.DeriveSha(receipts, trie.NewStackTrie(nil)), header.ReceiptHash)
	require.Equal(t, types.MergeBloom(receipts), header.Bloom)

	require.NoError(t, state.db.Flush())
	archived, err := executeArchiveBlock(executor, state, block)
	require.NoError(t, err)
	require.Len(t, archived, 1)
	require.Equal(t, receipts[0].CumulativeGasUsed, archived[0].CumulativeGasUsed)
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_FailOnFailedBundleThatDidNotFail(t *testing.T) {
	upgrades := opera.GetBrioUpgrades()
	upgrades.TransactionBundles = true
	signer := types.LatestSignerForChainID(new(big.Int).SetUint64(opera.FakeNetworkID))
	// Its transfer succeeds and adds a receipt.
	envelope := bundle.NewBuilder().WithSigner(signer).With(bundle.Step(sender, &types.AccessListTx{Gas: 100_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}})).Build()

	tests := map[string]func(*sonicExecutor, *State, *types.Block) (types.Receipts, error){
		"ExecuteBlock":          executeLiveBlock,
		"ExecuteBlockOnArchive": executeArchiveBlock,
	}

	for name, execute := range tests {
		t.Run(name, func(t *testing.T) {
			executor, state, blocks := newTestSonicExecutor(t, &BlockDBMetadataStore{metadata: Metadata{
				RulesUpdateHeights: []RulesUpdateHeight{{Rules: opera.FakeNetRules(upgrades)}},
				FailedBundles:      FailedBundles{1: {{Pos: 0, Tx: envelope}}},
			}})
			require.NoError(t, state.db.Flush())

			_, err := execute(executor, state, withTransactions(t, blocks[1]))
			require.ErrorContains(t, err, fmt.Sprintf("failed bundle %v added a receipt", envelope.Hash()))
		})
	}
}

func TestSonicExecutor_ExecuteBlockOnArchive_ReproducesReceiptsOfExecuteBlock(t *testing.T) {
	// Without the single proposer block formation, Sonic charges 10% of the
	// excess gas, so the receipts reveal whether Sonic's VM config is used.
	executor, state, blocks := newTestSonicExecutor(t, sonicMetadataStore())
	tx := sign(&types.LegacyTx{Gas: 50_000, GasPrice: big.NewInt(1e11), To: &common.Address{1}})
	receipts, err := executeLiveBlock(executor, state, withTransactions(t, blocks[1], tx))
	require.NoError(t, err)
	require.NoError(t, state.db.Flush())

	archived, err := executeArchiveBlock(executor, state, withTransactions(t, blocks[1], tx))
	require.NoError(t, err)
	require.Len(t, archived, 1)
	require.Greater(t, receipts[0].GasUsed, uint64(21_000))
	require.Equal(t, receipts[0].GasUsed, archived[0].GasUsed)
	require.Equal(t, receipts[0].Status, archived[0].Status)
}

func TestNewLedgerStore_RebuildsRecordedHead(t *testing.T) {
	recorded := createSonicBlocks(t, opera.FakeNetRules(opera.Upgrades{}), newGenesisState(t, nil), 2)[1] // first non-genesis block
	recorded.ExtraData = inter.EncodeExtraData(time.Unix(int64(recorded.Timestamp), 500), time.Second+500)
	recorded.Difficulty = 1
	recorded.Receipts = []*blockdb.TransactionReceipt{{CumulativeGasUsed: 21_000, Logs: []*blockdb.Log{{Address: common.Address{1}.Bytes()}}}}
	head, err := convert.ConvertToGethBlock(recorded)
	require.NoError(t, err)

	ledgerStore, err := newLedgerStore(head.Header(), nil)
	require.NoError(t, err)
	require.Equal(t, head.Hash(), ledgerStore.head.Hash())
}

func TestNewLedgerStore_ReportsInvalidExtraData(t *testing.T) {
	_, err := newLedgerStore(&types.Header{Number: big.NewInt(4)}, nil)
	require.ErrorContains(t, err, "failed to decode extra data of block 4")
}

func TestLedgerStore_GetLatestBlockIndex_ReturnsNumberOfHead(t *testing.T) {
	store := &ledgerStore{head: &inter.Block{Number: 4}}
	require.Equal(t, idx.Block(4), store.GetLatestBlockIndex())
}

func TestLedgerStore_GetLatestBlock_ReturnsHead(t *testing.T) {
	store := &ledgerStore{head: &inter.Block{}}
	require.Same(t, store.head, store.GetLatestBlock())
}

func TestLedgerStore_GetBlock_ReturnsOnlyHead(t *testing.T) {
	store := &ledgerStore{head: &inter.Block{Number: 4}}
	require.Same(t, store.head, store.GetBlock(4))
	require.Nil(t, store.GetBlock(3))
}

func TestLedgerStore_SetBlock_ReplacesHead(t *testing.T) {
	store := &ledgerStore{head: &inter.Block{}}
	block := &inter.Block{}
	store.SetBlock(1, block)
	require.Same(t, block, store.head)
}

func TestLedgerStore_GetUpgradeHeights_ReturnsUpgradesOfBlockFollowingHead(t *testing.T) {
	store := &ledgerStore{
		metadataStore: &BlockDBMetadataStore{metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{
			{Block: 1, Rules: opera.FakeNetRules(opera.GetSonicUpgrades())},
			{Block: 2, Rules: opera.FakeNetRules(opera.GetAllegroUpgrades())},
			{Block: 3, Rules: opera.FakeNetRules(opera.GetBrioUpgrades())},
			{Block: 4, Rules: opera.FakeNetRules(opera.GetCantoUpgrades())},
		}}},
		head: &inter.Block{Number: 2},
	}
	require.Equal(t, []opera.UpgradeHeight{{Upgrades: opera.GetBrioUpgrades(), Height: 3}}, store.GetUpgradeHeights())
}

func TestLedgerStore_GetLiveStateDb_ReturnsStateOfBlockBeingApplied(t *testing.T) {
	stateDB := recordedRootStateDB{root: common.Hash{1}}
	got, err := (&ledgerStore{stateDB: stateDB}).GetLiveStateDb(hash.Hash{2})
	require.NoError(t, err)
	require.Equal(t, stateDB, got)
}

func TestLedgerStore_Header_ReturnsHeadWithItsRulesAndOlderBlocksFromHashHistory(t *testing.T) {
	// The rules of the head, block 4, have no London upgrade, so its header has no base fee.
	headRules := opera.FakeNetRules(opera.Upgrades{})
	head := &inter.Block{Number: 4, ParentHash: common.Hash{3}, BaseFee: big.NewInt(7)}
	hashHistory := &blockHashHistory{}
	hashHistory.SetBlockHash(1, common.Hash{1})
	hashHistory.SetBlockHash(2, common.Hash{2})
	store := &ledgerStore{metadataStore: &BlockDBMetadataStore{metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{
		{Block: 0, Rules: headRules},
		{Block: 5, Rules: opera.FakeNetRules(opera.GetSonicUpgrades())},
	}}}, head: head, hashHistory: hashHistory}

	require.Equal(t, evmcore.ToEvmHeader(head, common.Hash{3}, headRules), store.Header(common.Hash{}, 4))
	require.Equal(t, hashHistory.Header(common.Hash{}, 2), store.Header(common.Hash{}, 2))
}

func TestRecordedRootStateDB_GetStateHash_ReturnsRecordedRoot(t *testing.T) {
	require.Equal(t, common.Hash{1}, recordedRootStateDB{root: common.Hash{1}}.GetStateHash())
}

// sender is the key of the account the genesis of newTestSonicExecutor funds.
var sender = crypto.ToECDSAUnsafe(common.Hash{1}.Bytes())

// newTestSonicExecutor returns a sonicExecutor of the fake network with its
// ledger at block 0, the state it executes blocks on, whose genesis funds
// sender and holds the given accounts, and the network's first blocks.
func newTestSonicExecutor(t *testing.T, metadataStore MetadataStore, accounts ...makefakegenesis.Account) (*sonicExecutor, *State, []*blockdb.Block) {
	t.Helper()
	state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5, Variant: "go-file"})
	require.NoError(t, err)
	t.Cleanup(func() { require.NoError(t, state.Close()) })
	require.NoError(t, state.ApplyGenesis(&makefakegenesis.GenesisJson{Accounts: append([]makefakegenesis.Account{
		{Address: crypto.PubkeyToAddress(sender.PublicKey), Balance: uint256.NewInt(1e18)},
	}, accounts...)}))
	blocks := createSonicBlocks(t, opera.FakeNetRules(opera.GetSonicUpgrades()), newGenesisState(t, nil), 21)

	head, err := convert.ConvertToGethBlock(blocks[0])
	require.NoError(t, err)
	executor, err := newSonicExecutor(opera.FakeNetworkID, metadataStore, testInterpreter(t), head.Header())
	require.NoError(t, err)
	return executor, state, blocks
}

// sonicMetadataStore returns a metadata store with the rules of the fake
// network with the Sonic upgrades.
func sonicMetadataStore() *BlockDBMetadataStore {
	return &BlockDBMetadataStore{metadata: Metadata{
		RulesUpdateHeights: []RulesUpdateHeight{{Rules: opera.FakeNetRules(opera.GetSonicUpgrades())}},
	}}
}

// sign returns the given transaction signed by sender for the fake network.
func sign(tx types.TxData) *blockdb.Transaction {
	signer := types.LatestSignerForChainID(new(big.Int).SetUint64(opera.FakeNetworkID))
	return convert.ToBerthaTransaction(types.MustSignNewTx(sender, signer, tx))
}

// withTransactions returns the given block with the given transactions.
func withTransactions(t *testing.T, block *blockdb.Block, txs ...*blockdb.Transaction) *types.Block {
	t.Helper()
	block = proto.Clone(block).(*blockdb.Block)
	block.Transactions = txs
	converted, err := convert.ConvertToGethBlock(block)
	require.NoError(t, err)
	return converted
}
