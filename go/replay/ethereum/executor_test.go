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

package ethereum

import (
	"encoding/binary"
	"fmt"
	"math/big"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/replay/state"
	cc "github.com/0xsoniclabs/carmen/go/common"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/trie"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestExecutor_ExecuteBlock_CompletesHeader(t *testing.T) {
	state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	// Init code that emits a log.
	tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(11155111)), &types.LegacyTx{
		Gas:  100_000,
		Data: []byte{byte(vm.PUSH1), 0, byte(vm.PUSH1), 0, byte(vm.LOG0)},
	})
	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1), GasLimit: 100_000}).
		WithBody(types.Body{Transactions: types.Transactions{tx}})

	executor := NewExecutor(11155111, testInterpreter(t))
	receipts, header, err := executor.ExecuteBlock(state, block, blockHashes{})
	require.NoError(t, err)
	require.Len(t, receipts, 1)
	require.Len(t, receipts[0].Logs, 1)
	require.Equal(t, receipts[0].CumulativeGasUsed, header.GasUsed)
	require.Equal(t, types.MergeBloom(receipts), header.Bloom)
	require.Equal(t, types.DeriveSha(receipts, trie.NewStackTrie(nil)), header.ReceiptHash)
}

func TestExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_ServeBlockHashFromHashHistory(t *testing.T) {
	executeBlock := func(e *Executor, state *state.State, block *types.Block, hashHistory blockHashes) (types.Receipts, error) {
		receipts, _, err := e.ExecuteBlock(state, block, hashHistory)
		return receipts, err
	}
	executeBlockOnArchive := func(e *Executor, state *state.State, block *types.Block, hashHistory blockHashes) (types.Receipts, error) {
		return e.ExecuteBlockOnArchive(state, block, nil, hashHistory)
	}
	tests := map[string]struct {
		execute      func(*Executor, *state.State, *types.Block, blockHashes) (types.Receipts, error)
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
			state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			executor := NewExecutor(11155111, testInterpreter(t))
			for number := range int64(2) {
				_, _, err = executor.ExecuteBlock(state, types.NewBlockWithHeader(&types.Header{Number: big.NewInt(number + 1)}), blockHashes{})
				require.NoError(t, err)
			}
			require.NoError(t, state.DB.Flush())

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			// Init code that reverts if BLOCKHASH(1) is zero. It runs in block 3,
			// since the hash of the parent block comes from the block's ParentHash.
			tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(11155111)), &types.LegacyTx{
				Gas:  100_000,
				Data: common.FromHex("0x600140600a57600080fd5b00"),
			})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(3), GasLimit: 100_000}).
				WithBody(types.Body{Transactions: types.Transactions{tx}})
			hashHistory := blockHashes{1: tt.hashOfBlock1}

			receipts, err := tt.execute(executor, state, block, hashHistory)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Equal(t, tt.wantStatus, receipts[0].Status)
		})
	}
}

func TestApplyBlockWithProcessor_ForwardsDifficultyToProcessor(t *testing.T) {
	tests := map[string]struct {
		difficulty *big.Int
	}{
		"pre-merge":  {difficulty: big.NewInt(1000)},
		"post-merge": {difficulty: big.NewInt(0)},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			processor := NewMockProcessor(ctrl)

			state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block := types.NewBlockWithHeader(&types.Header{
				Difficulty: tt.difficulty,
				GasLimit:   8_000_000,
			})

			chainConfig := params.MainnetChainConfig

			processor.EXPECT().ProcessWithDifficulty(
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(),
			).DoAndReturn(func(
				_ *evmcore.EvmBlock, _ interface{}, _ vm.Config,
				_ uint64, _ *uint64, _ int, _ interface{},
				difficulty *big.Int, _ uint64,
			) evmcore.ProcessSummary {
				require.Equal(t, tt.difficulty, difficulty)
				return evmcore.ProcessSummary{}
			})

			_, _, err = applyBlockWithProcessor(state, block, testInterpreter(t), processor, chainConfig, false)
			require.NoError(t, err)
		})
	}
}

func TestApplyBlockWithProcessor_CanApplyAnEmptyBlock(t *testing.T) {
	state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{})
	require.NoError(t, err)

	chainConfig := params.MainnetChainConfig

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		blockHashes{},
		opera.Upgrades{},
	)

	receipts, _, err := applyBlockWithProcessor(state, block, testInterpreter(t), processor, chainConfig, false)
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestApplyBlockWithProcessor_RunsSystemCallsOfActiveUpgradesAndReportsTheirFailures(t *testing.T) {
	chainConfig := params.MainnetChainConfig
	beaconRoot := common.Hash{1, 2, 3}
	revert := []byte{byte(vm.PUSH0), byte(vm.PUSH0), byte(vm.REVERT)}
	// EIP-4788 stores the beacon root at slot timestamp % 8191 + 8191.
	rootSlot := func(time uint64) (slot cc.Key) {
		binary.BigEndian.PutUint64(slot[24:], time%8191+8191)
		return slot
	}
	// The request contracts of EIP-7002 and EIP-7251 keep the excess of the
	// requests over their target in slot 0 and the number of queued requests in
	// slot 1.
	excessSlot, countSlot := cc.Key{}, cc.Key{31: 1}
	tests := map[string]struct {
		time       uint64
		beaconRoot []byte
		address    common.Address
		code       []byte
		storage    map[cc.Key]cc.Value
		want       map[cc.Key]cc.Value
		wantErr    string
	}{
		"EIP-4788 stores the beacon root in Cancun": {
			time:       *chainConfig.CancunTime,
			beaconRoot: beaconRoot.Bytes(),
			address:    params.BeaconRootsAddress,
			code:       params.BeaconRootsCode,
			want:       map[cc.Key]cc.Value{rootSlot(*chainConfig.CancunTime): cc.Value(beaconRoot)},
		},
		"EIP-4788 does not store the beacon root before Cancun": {
			time:       *chainConfig.CancunTime - 1,
			beaconRoot: beaconRoot.Bytes(),
			address:    params.BeaconRootsAddress,
			code:       params.BeaconRootsCode,
			want:       map[cc.Key]cc.Value{rootSlot(*chainConfig.CancunTime - 1): {}},
		},
		"EIP-7002 dequeues withdrawal requests in Prague": {
			time:    *chainConfig.PragueTime,
			address: params.WithdrawalQueueAddress,
			code:    params.WithdrawalQueueCode,
			storage: map[cc.Key]cc.Value{countSlot: {31: 3}},
			// The target is 2 requests per block.
			want: map[cc.Key]cc.Value{excessSlot: {31: 1}, countSlot: {}},
		},
		"EIP-7002 does not dequeue withdrawal requests before Prague": {
			time:    *chainConfig.PragueTime - 1,
			address: params.WithdrawalQueueAddress,
			code:    params.WithdrawalQueueCode,
			storage: map[cc.Key]cc.Value{countSlot: {31: 3}},
			want:    map[cc.Key]cc.Value{excessSlot: {}, countSlot: {31: 3}},
		},
		"EIP-7002 reports a failing withdrawal request contract": {
			time:    *chainConfig.PragueTime,
			address: params.WithdrawalQueueAddress,
			code:    revert,
			wantErr: "failed to process EIP-7002 system call",
		},
		"EIP-7251 dequeues consolidation requests in Prague": {
			time:    *chainConfig.PragueTime,
			address: params.ConsolidationQueueAddress,
			code:    params.ConsolidationQueueCode,
			storage: map[cc.Key]cc.Value{countSlot: {31: 3}},
			// The target is 1 request per block.
			want: map[cc.Key]cc.Value{excessSlot: {31: 2}, countSlot: {}},
		},
		"EIP-7251 does not dequeue consolidation requests before Prague": {
			time:    *chainConfig.PragueTime - 1,
			address: params.ConsolidationQueueAddress,
			code:    params.ConsolidationQueueCode,
			storage: map[cc.Key]cc.Value{countSlot: {31: 3}},
			want:    map[cc.Key]cc.Value{excessSlot: {}, countSlot: {31: 3}},
		},
		"EIP-7251 reports a failing consolidation request contract": {
			time:    *chainConfig.PragueTime,
			address: params.ConsolidationQueueAddress,
			code:    revert,
			wantErr: "failed to process EIP-7251 system call",
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			st, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, st.Close()) }()

			st.DB.BeginBlock()
			st.DB.BeginTransaction()
			st.DB.SetCode(cc.Address(tt.address), tt.code)
			for key, value := range tt.storage {
				st.DB.SetState(cc.Address(tt.address), key, value)
			}
			st.DB.EndTransaction()
			handle, err := state.EndBlockAndCommit(st, 0)
			require.NoError(t, err)
			require.NoError(t, handle.Wait())

			zero := uint64(0)
			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				// All forks up to London are active at this block.
				Number:                20_000_000,
				Timestamp:             tt.time,
				GasLimit:              30_000_000,
				ExcessBlobGas:         &zero,
				BlobGasUsed:           &zero,
				ParentBeaconBlockRoot: tt.beaconRoot,
				// A non-zero PrevRandao activates the post-merge EVM rules, which
				// enable PUSH0 of Shanghai the system contracts use.
				PrevRandao: []byte{1},
			})
			require.NoError(t, err)
			processor := evmcore.NewStateProcessorForReplay(chainConfig, blockHashes{}, opera.Upgrades{})

			_, _, err = applyBlockWithProcessor(st, block, testInterpreter(t), processor, chainConfig, false)
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			for key, value := range tt.want {
				require.Equal(t, value, st.DB.GetState(cc.Address(tt.address), key))
			}
		})
	}
}

func TestApplyBlockWithProcessor_CreditsWithdrawalsOnlyPostMerge(t *testing.T) {
	const amountGwei = uint64(1000)
	withdrawalAddr := common.Address{0xAB}

	tests := map[string]struct {
		difficulty     uint64
		wantBalanceWei uint64
	}{
		"post-merge": {
			difficulty:     0,
			wantBalanceWei: amountGwei * params.GWei,
		},
		"pre-merge": {
			difficulty:     1,
			wantBalanceWei: 0,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				GasLimit:   8_000_000,
				Difficulty: tt.difficulty,
				Withdrawals: []*blockdb.Withdrawal{
					{Address: withdrawalAddr.Bytes(), Amount: amountGwei},
				},
			})
			require.NoError(t, err)

			processor := evmcore.NewStateProcessorForReplay(
				params.MainnetChainConfig,
				blockHashes{},
				opera.Upgrades{},
			)

			_, _, err = applyBlockWithProcessor(state, block, testInterpreter(t), processor, params.MainnetChainConfig, false)
			require.NoError(t, err)

			require.Equal(t, tt.wantBalanceWei, state.DB.GetBalance(cc.Address(withdrawalAddr)).Uint64())
		})
	}
}

func TestApplyBlockWithProcessor_AccumulatesRewardsOnlyPreMerge(t *testing.T) {
	coinbase := common.Address{0x01}
	uncleCoinbase := common.Address{0x02}
	blockNumber := uint64(10_000_000) // post-Constantinople
	uncleNumber := blockNumber - 1

	// Constantinople block reward is 2 ETH.
	// Uncle reward = (uncleNumber + 8 - blockNumber) / 8 * blockReward
	// Miner reward = blockReward + blockReward/32 per uncle
	blockReward := ethash.ConstantinopleBlockReward
	uncleReward := new(uint256.Int).Set(blockReward)
	uncleReward.Mul(uncleReward, uint256.NewInt(7))
	uncleReward.Div(uncleReward, uint256.NewInt(8))

	minerReward := new(uint256.Int).Set(blockReward)
	minerReward.Add(minerReward, new(uint256.Int).Div(blockReward, uint256.NewInt(32)))

	tests := map[string]struct {
		difficulty       uint64
		wantMinerBalance *uint256.Int
		wantUncleBalance *uint256.Int
	}{
		"pre-merge": {
			difficulty:       1,
			wantMinerBalance: minerReward,
			wantUncleBalance: uncleReward,
		},
		"post-merge": {
			difficulty:       0,
			wantMinerBalance: uint256.NewInt(0),
			wantUncleBalance: uint256.NewInt(0),
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				Number:      blockNumber,
				GasLimit:    8_000_000,
				Difficulty:  tt.difficulty,
				Beneficiary: coinbase.Bytes(),
				OmmerHeaders: []*blockdb.OmmerHeader{
					{Beneficiary: uncleCoinbase.Bytes(), Number: uncleNumber},
				},
			})
			require.NoError(t, err)

			processor := evmcore.NewStateProcessorForReplay(
				params.MainnetChainConfig,
				blockHashes{},
				opera.Upgrades{},
			)

			_, _, err = applyBlockWithProcessor(state, block, testInterpreter(t), processor, params.MainnetChainConfig, false)
			require.NoError(t, err)

			require.Equal(t, tt.wantMinerBalance.ToBig(), state.DB.GetBalance(cc.Address(coinbase)).ToBig())
			require.Equal(t, tt.wantUncleBalance.ToBig(), state.DB.GetBalance(cc.Address(uncleCoinbase)).ToBig())
		})
	}
}

func TestApplyBlockWithProcessor_CanApplyBlockToArchiveState(t *testing.T) {
	state, err := state.NewState(state.StateParameters{
		Directory:   t.TempDir(),
		WithArchive: true,
		Schema:      5,
	})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	chainConfig := params.MainnetChainConfig

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		blockHashes{},
		opera.Upgrades{},
	)

	// Apply blocks 0, 1 and 2 normally to populate the archive.
	for i := range 3 {
		block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: uint64(i)})
		require.NoError(t, err)
		_, _, err = applyBlockWithProcessor(state, block, testInterpreter(t), processor, chainConfig, false)
		require.NoError(t, err)
	}
	require.NoError(t, state.DB.Flush())

	// Apply block 1 using archive mode.
	block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: 1})
	require.NoError(t, err)

	receipts, _, err := applyBlockWithProcessor(state, block, testInterpreter(t), processor, chainConfig, true)
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestApplyBlockWithProcessor_CommitsBlockToTheArchiveState(t *testing.T) {
	require := require.New(t)
	state, err := state.NewState(state.StateParameters{
		Directory:   t.TempDir(),
		WithArchive: true,
		Schema:      5,
	})
	require.NoError(err)
	defer func() { require.NoError(state.Close()) }()

	chainConfig := params.MainnetChainConfig

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		blockHashes{},
		opera.Upgrades{},
	)

	withdrawals := map[uint64][]*blockdb.Withdrawal{
		1: {{Address: common.Address{1}.Bytes(), Amount: 1000}},
	}

	for i := range 3 {
		block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: uint64(i), Withdrawals: withdrawals[uint64(i)]})
		require.NoError(err)
		_, _, err = applyBlockWithProcessor(state, block, testInterpreter(t), processor, chainConfig, false)
		require.NoError(err)
	}
	require.NoError(state.DB.Flush())

	height, empty, err := state.GetArchiveBlockHeight()
	require.NoError(err)
	require.False(empty)
	require.Equal(uint64(2), height)

	before, err := state.DB.GetArchiveStateDB(0)
	require.NoError(err)
	defer before.Release()
	require.Zero(before.GetBalance(cc.Address{1}).Uint64())

	after, err := state.DB.GetArchiveStateDB(1)
	require.NoError(err)
	defer after.Release()
	require.Equal(uint64(1000*params.GWei), after.GetBalance(cc.Address{1}).Uint64())
}

func TestApplyBlockWithProcessor_ReportsFailures(t *testing.T) {
	injected := fmt.Errorf("injected")
	skipped := types.NewTx(&types.LegacyTx{})
	anyArg := gomock.Any()
	tests := map[string]struct {
		number    int64
		isArchive bool
		expect    func(*carmen.MockStateDB, *MockProcessor)
		wantErr   string
		cause     error
	}{
		"genesis block on archive": {
			number:    0,
			isArchive: true,
			wantErr:   "cannot apply genesis block in archive mode",
		},
		"failing archive state": {
			number:    5,
			isArchive: true,
			expect: func(db *carmen.MockStateDB, _ *MockProcessor) {
				db.EXPECT().GetArchiveStateDB(uint64(4)).Return(nil, injected)
			},
			wantErr: "failed to get archive state for block 4",
			cause:   injected,
		},
		"skipped transaction": {
			number: 5,
			expect: func(db *carmen.MockStateDB, processor *MockProcessor) {
				gomock.InOrder(
					db.EXPECT().BeginBlock(),
					processor.EXPECT().
						ProcessWithDifficulty(anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg).
						Return(evmcore.ProcessSummary{ProcessedTransactions: []evmcore.ProcessedTransaction{{Transaction: skipped}}}),
				)
			},
			wantErr: fmt.Sprintf("tx %v was skipped", skipped.Hash()),
		},
		"failing end block": {
			number: 5,
			expect: func(db *carmen.MockStateDB, processor *MockProcessor) {
				gomock.InOrder(
					db.EXPECT().BeginBlock(),
					processor.EXPECT().
						ProcessWithDifficulty(anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg, anyArg).
						Return(evmcore.ProcessSummary{}),
					db.EXPECT().EndBlock(uint64(5)).Return(nil, injected),
				)
			},
			wantErr: "failed to end block 5",
			cause:   injected,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := carmen.NewMockStateDB(ctrl)
			processor := NewMockProcessor(ctrl)
			if tt.expect != nil {
				tt.expect(db, processor)
			}
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(tt.number), GasLimit: 8_000_000})

			_, _, err := applyBlockWithProcessor(&state.State{DB: db}, block, testInterpreter(t), processor, params.MainnetChainConfig, tt.isArchive)
			require.ErrorContains(t, err, tt.wantErr)
			if tt.cause != nil {
				require.ErrorIs(t, err, tt.cause)
			}
		})
	}
}

func testInterpreter(t *testing.T) tosca.Interpreter {
	t.Helper()
	interpreter, err := tosca.NewInterpreter("sfvm")
	require.NoError(t, err)
	return interpreter
}

// blockHashes serves BLOCKHASH from the hashes of the blocks it holds.
type blockHashes map[uint64]common.Hash

func (h blockHashes) Header(_ common.Hash, number uint64) *evmcore.EvmHeader {
	return &evmcore.EvmHeader{Number: new(big.Int).SetUint64(number), Hash: h[number], ParentHash: h[number-1]}
}
