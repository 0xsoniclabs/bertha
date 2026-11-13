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
	"errors"
	"fmt"
	"math/big"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/core/vm/program"
	"github.com/ethereum/go-ethereum/crypto"
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

	cases := map[string]struct {
		execute           func(*sonicExecutor, *State, *types.Block) (types.Receipts, error)
		tx                *blockdb.Transaction
		expectCommitRules bool
	}{
		"ExecuteBlock with EpochSealingTx": {
			execute:           executeLiveBlock,
			tx:                epochSealingTx,
			expectCommitRules: true,
		},
		"ExecuteBlock with NonEpochSealingTx": {
			execute: executeLiveBlock,
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
			upgradeHeights := metadataStore.EXPECT().GetUpgradeHeights().Return([]opera.UpgradeHeight{{Upgrades: opera.GetSonicUpgrades()}})
			rules := metadataStore.EXPECT().GetRulesAtBlock(uint64(1)).Return(opera.FakeNetRules(opera.GetSonicUpgrades()))
			metadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
			if tc.expectCommitRules {
				// The epoch sealing block runs with the committed rules.
				gomock.InOrder(metadataStore.EXPECT().CommitRules(uint64(1)).Return(nil), upgradeHeights, rules)
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

func TestSonicExecutor_ExecuteBlock_ForwardsCommitRulesError(t *testing.T) {
	injected := errors.New("injected")
	metadataStore := NewMockMetadataStore(gomock.NewController(t))
	metadataStore.EXPECT().CommitRules(uint64(1)).Return(injected)

	epochSealingTx := types.NewTx(&types.LegacyTx{To: &driver.ContractAddress, Data: drivercall.SealEpoch(nil), V: big.NewInt(0), R: big.NewInt(0)})
	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)}).WithBody(types.Body{Transactions: types.Transactions{epochSealingTx}})

	_, _, err := (&sonicExecutor{metadataStore: metadataStore}).ExecuteBlock(nil, block, &blockHashHistory{})
	require.ErrorIs(t, err, injected)
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

	tests := map[string]struct {
		execute          func(*sonicExecutor, *State, *types.Block) (types.Receipts, error)
		expectPatchRules bool
	}{
		"ExecuteBlock":          {execute: executeLiveBlock, expectPatchRules: true},
		"ExecuteBlockOnArchive": {execute: executeArchiveBlock},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			metadataStore := NewMockMetadataStore(ctrl)
			metadataStore.EXPECT().GetUpgradeHeights().Return([]opera.UpgradeHeight{{Upgrades: opera.GetSonicUpgrades()}})
			metadataStore.EXPECT().GetRulesAtBlock(uint64(1)).Return(opera.FakeNetRules(opera.GetSonicUpgrades()))
			metadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
			if tt.expectPatchRules {
				metadataStore.EXPECT().PatchRules(uint64(1), diff)
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
	require.Equal(t, block.TxHash(), header.TxHash)
	require.Equal(t, receipts[0].GasUsed, header.GasUsed)
	require.Equal(t, types.MergeBloom(receipts), header.Bloom)
	require.Equal(t, types.DeriveSha(receipts, trie.NewStackTrie(nil)), header.ReceiptHash)
	root, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.NotEqual(t, root, block.Root())
	require.Equal(t, block.Root(), header.Root)
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
			for _, block := range blocks[1:3] {
				_, err := executeLiveBlock(executor, state, withTransactions(t, block))
				require.NoError(t, err)
			}
			require.NoError(t, state.db.Flush())

			// Init code that reverts if BLOCKHASH(1) is zero. It runs in block 3,
			// since the hash of the parent block comes from the block's ParentHash.
			block := withTransactions(t, blocks[3], sign(&types.LegacyTx{
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

// sender is the key of the account the genesis of newTestSonicExecutor funds.
var sender = crypto.ToECDSAUnsafe(common.Hash{1}.Bytes())

// newTestSonicExecutor returns a sonicExecutor of the fake network, the state
// it executes blocks on, whose genesis funds sender and holds the given
// accounts, and the network's first blocks with the Sonic upgrades.
func newTestSonicExecutor(t *testing.T, metadataStore MetadataStore, accounts ...makefakegenesis.Account) (*sonicExecutor, *State, []*blockdb.Block) {
	t.Helper()
	state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5, Variant: "go-file"})
	require.NoError(t, err)
	t.Cleanup(func() { require.NoError(t, state.Close()) })
	require.NoError(t, state.ApplyGenesis(&makefakegenesis.GenesisJson{Accounts: append([]makefakegenesis.Account{
		{Address: crypto.PubkeyToAddress(sender.PublicKey), Balance: uint256.NewInt(1e18)},
	}, accounts...)}))
	executor := &sonicExecutor{chainID: opera.FakeNetworkID, metadataStore: metadataStore, interpreter: testInterpreter(t)}
	return executor, state, createSonicBlocks(t, opera.FakeNetRules(opera.GetSonicUpgrades()), newGenesisState(t, nil), 21)
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
