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
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

// executeLiveBlock executes the block with ExecuteBlock and an empty hash
// history, and drops the gas used.
func executeLiveBlock(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error) {
	receipts, _, err := e.ExecuteBlock(state, block, &blockHashHistory{})
	return receipts, err
}

// executeArchiveBlock executes the block with ExecuteBlockOnArchive and an
// empty hash history.
func executeArchiveBlock(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error) {
	return e.ExecuteBlockOnArchive(state, block, &blockHashHistory{})
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_CommitRulesUpdateOnlyLiveWhenEncounteringAnEpochSealingTx(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	address := crypto.PubkeyToAddress(key.PublicKey)
	chainID := uint64(2)
	signer := types.LatestSignerForChainID(big.NewInt(int64(chainID)))

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
	nonEpochSealingTx := convert.ToBerthaTransaction(types.MustSignNewTx(key, signer,
		&types.LegacyTx{
			Gas:      21000,
			To:       &common.Address{1},
			GasPrice: big.NewInt(100000),
		},
	))

	cases := map[string]struct {
		execute           func(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error)
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
			mockMetadataStore := NewMockMetadataStore(ctrl)

			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() {
				require.NoError(t, state.Close())
			}()
			require.NoError(t, state.ApplyGenesis(&makefakegenesis.GenesisJson{Accounts: []makefakegenesis.Account{
				{Address: address, Balance: uint256.NewInt(1e18)},
			}}))
			require.NoError(t, state.db.Flush())

			executor := &sonicExecutor{chainID: chainID, metadataStore: mockMetadataStore}

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				GasLimit:     1e18,
				Number:       1,
				ExtraData:    make([]byte, 12),
				Transactions: []*blockdb.Transaction{tc.tx},
			})
			require.NoError(t, err)

			upgradeHeights := mockMetadataStore.EXPECT().GetUpgradeHeights()
			rules := mockMetadataStore.EXPECT().GetRulesAtBlock(uint64(1))
			mockMetadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			mockMetadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))

			if tc.expectCommitRules {
				// The epoch sealing block runs with the committed rules.
				gomock.InOrder(mockMetadataStore.EXPECT().CommitRules(uint64(1)).Return(nil), upgradeHeights, rules)
			}

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

	tests := map[string]struct {
		execute          func(e *sonicExecutor, state *State, block *types.Block) (types.Receipts, error)
		expectPatchRules bool
	}{
		"ExecuteBlock":          {execute: executeLiveBlock, expectPatchRules: true},
		"ExecuteBlockOnArchive": {execute: executeArchiveBlock},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			metadataStore := NewMockMetadataStore(ctrl)
			metadataStore.EXPECT().GetUpgradeHeights()
			metadataStore.EXPECT().GetRulesAtBlock(uint64(1))
			metadataStore.EXPECT().GetCorrectionsAtBlock(uint64(1))
			metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(1))
			if tt.expectPatchRules {
				metadataStore.EXPECT().PatchRules(uint64(1), diff)
			}

			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()
			require.NoError(t, state.ApplyGenesis(&makefakegenesis.GenesisJson{Accounts: []makefakegenesis.Account{{Address: driver.ContractAddress, Code: driverCode}}}))
			require.NoError(t, state.db.Flush())

			executor := &sonicExecutor{chainID: 123, metadataStore: metadataStore, interpreter: testInterpreter(t)}

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(123)), &types.LegacyTx{Gas: 100_000, To: &driver.ContractAddress, Data: makeUpdateNetworkRulesLog(diff).Data})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1), GasLimit: 100_000, Extra: make([]byte, 12)}).
				WithBody(types.Body{Transactions: types.Transactions{tx}})

			receipts, err := tt.execute(executor, state, block)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Len(t, receipts[0].Logs, 1)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_ApplyMetadata(t *testing.T) {
	signer := types.LatestSignerForChainID(big.NewInt(146))
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	transfer := func(nonce uint64) *types.Transaction {
		return types.MustSignNewTx(key, signer, &types.LegacyTx{Nonce: nonce, Gas: 50_000, GasPrice: big.NewInt(1), To: &common.Address{1}})
	}
	newBlock := func(number int64, txs ...*types.Transaction) *types.Block {
		return types.NewBlockWithHeader(&types.Header{Number: big.NewInt(number), GasLimit: 1_000_000, BaseFee: big.NewInt(1), Extra: make([]byte, 12)}).
			WithBody(types.Body{Transactions: txs})
	}

	withExcessGasCharge := opera.GetBrioUpgrades()
	withExcessGasCharge.TransactionBundles = true
	noExcessGasCharge := withExcessGasCharge
	noExcessGasCharge.SingleProposerBlockFormation = true
	envelope := bundle.NewBuilder().
		WithSigner(signer).
		With(bundle.Step(key, &types.AccessListTx{Nonce: 1, Gas: 100_000, GasPrice: big.NewInt(1), Data: []byte{byte(vm.INVALID)}})).
		Build()
	executor := &sonicExecutor{chainID: 146, metadataStore: &BlockDBMetadataStore{metadata: Metadata{
		RulesUpdateHeights: []RulesUpdateHeight{
			{Block: 0, Rules: opera.FakeNetRules(withExcessGasCharge)},
			{Block: 1, Rules: opera.FakeNetRules(noExcessGasCharge)},
		},
		Corrections:   Corrections{2: {common.Address{2}: {Balance: uint256.NewInt(1000)}}},
		FailedBundles: FailedBundles{3: {{Pos: 0, Tx: envelope}}},
	}}, interpreter: testInterpreter(t)}

	state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()
	require.NoError(t, state.ApplyGenesis(&makefakegenesis.GenesisJson{Accounts: []makefakegenesis.Account{
		{Address: crypto.PubkeyToAddress(key.PublicKey), Balance: uint256.NewInt(1e18)},
	}}))

	// The rules of block 1 waive the excess gas charge.
	rulesBlock := newBlock(1, transfer(0))
	receipts, err := executeLiveBlock(executor, state, rulesBlock)
	require.NoError(t, err)
	require.Equal(t, uint64(21_000), receipts[0].GasUsed, "rules not applied")

	// Block 2 corrects the balance of an account.
	require.Zero(t, state.db.GetBalance(cc.Address{2}).Uint64(), "corrections applied before their block")
	_, err = executeLiveBlock(executor, state, newBlock(2))
	require.NoError(t, err)
	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{2}).Uint64(), "corrections not applied")

	// The failed bundle of block 3 uses nonce 1, which makes its tx valid.
	failedBundleBlock := newBlock(3, transfer(2))
	_, err = executeLiveBlock(executor, state, failedBundleBlock)
	require.NoError(t, err, "failed bundles not applied")

	require.NoError(t, state.db.Flush())

	// The archive applies the rules of block 1.
	receipts, err = executeArchiveBlock(executor, state, rulesBlock)
	require.NoError(t, err)
	require.Equal(t, uint64(21_000), receipts[0].GasUsed, "rules not applied on archive")

	// Corrections are not checked on the archive: they are applied after the
	// transactions to a state the archive discards, so they have no visible effect.

	// The archive applies the failed bundle of block 3.
	_, err = executeArchiveBlock(executor, state, failedBundleBlock)
	require.NoError(t, err, "failed bundles not applied on archive")
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
			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			executor := &sonicExecutor{chainID: 146, metadataStore: &BlockDBMetadataStore{}, interpreter: testInterpreter(t)}
			for number := range int64(2) {
				_, _, err = executor.ExecuteBlock(state, types.NewBlockWithHeader(&types.Header{Number: big.NewInt(number + 1), Extra: make([]byte, 12)}), &blockHashHistory{})
				require.NoError(t, err)
			}
			require.NoError(t, state.db.Flush())

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			// Init code that reverts if BLOCKHASH(1) is zero. It runs in block 3,
			// since the hash of the parent block comes from the block's ParentHash.
			tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(146)), &types.LegacyTx{
				Gas:  100_000,
				Data: common.FromHex("0x600140600a57600080fd5b00"),
			})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(3), GasLimit: 100_000, Extra: make([]byte, 12)}).
				WithBody(types.Body{Transactions: types.Transactions{tx}})
			hashHistory := &blockHashHistory{}
			hashHistory.SetBlockHash(1, tt.hashOfBlock1)

			receipts, err := tt.execute(executor, state, block, hashHistory)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Equal(t, tt.wantStatus, receipts[0].Status)
		})
	}
}

func TestSonicExecutor_ExecuteBlock_ReturnsGasUsedOfFailedBundles(t *testing.T) {
	upgrades := opera.GetBrioUpgrades()
	upgrades.TransactionBundles = true
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	// A failed bundle uses gas without adding a receipt. INVALID uses all gas
	// of the bundle's step.
	signer := types.LatestSignerForChainID(big.NewInt(146))
	envelope := bundle.NewBuilder().
		WithSigner(signer).
		With(bundle.Step(key, &types.AccessListTx{Gas: 100_000, GasPrice: big.NewInt(1), Data: []byte{byte(vm.INVALID)}})).
		Build()
	txBundle, err := bundle.OpenEnvelope(signer, envelope)
	require.NoError(t, err)

	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()
	setBalance(state.db, crypto.PubkeyToAddress(key.PublicKey), big.NewInt(1e18))

	executor := &sonicExecutor{chainID: 146, metadataStore: &BlockDBMetadataStore{metadata: Metadata{
		RulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: opera.FakeNetRules(upgrades)}},
		FailedBundles:      FailedBundles{1: {{Pos: 0, Tx: envelope}}},
	}}, interpreter: testInterpreter(t)}

	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1), GasLimit: 1_000_000, BaseFee: big.NewInt(1), Extra: make([]byte, 12)})
	receipts, gasUsed, err := executor.ExecuteBlock(state, block, &blockHashHistory{})
	require.NoError(t, err)
	require.Empty(t, receipts)
	require.Equal(t, txBundle.GetTransactionsInReferencedOrder()[0].Gas(), gasUsed)
}
