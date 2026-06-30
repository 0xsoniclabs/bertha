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
	"encoding/binary"
	"fmt"
	"math/big"
	"os"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/params"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestState_CanBeCreatedAndClosed(t *testing.T) {
	for _, archive := range []bool{false, true} {
		t.Run(fmt.Sprintf("withArchive=%t", archive), func(t *testing.T) {
			state, err := NewState(StateParameters{
				Directory:   t.TempDir(),
				WithArchive: archive,
				Schema:      5,
			})
			require.NoError(t, err)
			require.NotNil(t, state)

			err = state.Close()
			require.NoError(t, err)
		})
	}
}

func TestNewState_CreatesEmptyDatabase(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
		Variant:   "go-file",
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	root, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.Equal(t, types.EmptyRootHash, root)
}

func TestNewState_FailsWithInvalidDirectory(t *testing.T) {
	_, err := NewState(StateParameters{
		Directory: "/invalid/directory/that/does/not/exist",
		Schema:    5,
	})
	require.Error(t, err)
	require.Contains(t, err.Error(), "failed to create state dir")
}

func TestNewState_FailsIfDirectoryIsReadOnly(t *testing.T) {
	// Create a temporary directory and make it read-only
	tempDir := t.TempDir()
	err := os.Chmod(tempDir, 0555) // Read-only permissions
	require.NoError(t, err)
	defer func() { require.NoError(t, os.Chmod(tempDir, 0755)) }()

	_, err = NewState(StateParameters{
		Directory: tempDir,
		Schema:    5,
	})
	require.Error(t, err)
	require.Contains(t, err.Error(), "permission denied")
}

func TestState_Close_CanBeCalledOnClosedDb(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
	})
	require.NoError(t, err)

	err = state.Close()
	require.NoError(t, err)

	// Calling Close again should not cause an error
	err = state.Close()
	require.NoError(t, err)
}

func TestState_GetStateRoot_ForwardsCallToDatabase(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	hash := cc.Hash{1, 2, 3}
	db.EXPECT().GetCommitment().Return(
		future.Immediate(result.Ok(hash)),
	)

	state := &State{db: db}

	got, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.Equal(t, common.Hash(hash), got)
}

func TestState_GetStateRoot_ForwardsErrorFromDatabase(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	issue := fmt.Errorf("database error")
	db.EXPECT().GetCommitment().Return(
		future.Immediate(result.Err[cc.Hash](issue)),
	)

	state := &State{db: db}

	_, err := state.GetStateRoot().Await().Get()
	require.ErrorIs(t, err, issue)
}

func TestState_ApplyGenesis_CanApplyGenesis(t *testing.T) {
	genesis := &Genesis{
		Accounts: []Account{
			{
				Address: common.Address{1},
				Balance: *uint256.NewInt(1000),
				Nonce:   1,
				Code:    []byte{1, 2, 3},
				Storage: map[common.Hash]common.Hash{
					common.HexToHash("0x1"): common.HexToHash("0x2"),
					common.HexToHash("0x3"): common.HexToHash("0x4"),
				},
			},
		},
	}

	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	require.NoError(t, state.ApplyGenesis(genesis))

	db := state.db
	addr := cc.Address{1}
	require.Equal(t, uint64(1000), db.GetBalance(addr).Uint64())
	require.Equal(t, uint64(1), db.GetNonce(addr))
	require.Equal(t, []byte{1, 2, 3}, db.GetCode(addr))

	key := cc.Key(common.HexToHash("0x1"))
	value := cc.Value(common.HexToHash("0x2"))
	require.Equal(t, value, db.GetState(addr, key))

	key = cc.Key(common.HexToHash("0x3"))
	value = cc.Value(common.HexToHash("0x4"))
	require.Equal(t, value, db.GetState(addr, key))
}

func TestState_ApplyBlock_PrevRandaoIsMixDigestPostMerge(t *testing.T) {
	tests := map[string]struct {
		difficulty     *big.Int
		mixDigest      common.Hash
		wantPrevRandao common.Hash
	}{
		"pre-merge": {
			difficulty:     big.NewInt(1000),
			mixDigest:      common.Hash{0xab},
			wantPrevRandao: common.Hash{},
		},
		"post-merge": {
			difficulty:     big.NewInt(0),
			mixDigest:      common.Hash{0xab},
			wantPrevRandao: common.Hash{0xab},
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			processor := NewMockProcessor(ctrl)

			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block := types.NewBlockWithHeader(&types.Header{
				Difficulty: tt.difficulty,
				MixDigest:  tt.mixDigest,
				GasLimit:   8_000_000,
			})

			chainConfig := opera.CreateTransientEvmChainConfig(
				1,
				[]opera.UpgradeHeight{},
				idx.Block(block.NumberU64()),
			)

			processor.EXPECT().ProcessWithDifficulty(
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(),
			).DoAndReturn(func(
				evmBlock *evmcore.EvmBlock, _ interface{}, _ vm.Config,
				_ uint64, _ *uint64, _ int, _ interface{},
				difficulty *big.Int, _ uint64,
			) evmcore.ProcessSummary {
				require.Equal(t, tt.wantPrevRandao, evmBlock.PrevRandao)
				require.Equal(t, tt.difficulty, difficulty)
				return evmcore.ProcessSummary{}
			})

			_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
			require.NoError(t, err)
		})
	}
}

func TestState_ApplyBlock_CanApplyAnEmptyBlock(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{})
	require.NoError(t, err)

	chainConfig := opera.CreateTransientEvmChainConfig(
		1,
		[]opera.UpgradeHeight{},
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	receipts, err := state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestState_ApplyBlock_FailsOnSkippedTransaction(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Transactions: []*blockdb.Transaction{
			{
				TransactionType: types.LegacyTxType,
				Nonce:           123, // all nonces are 0 in the DB
			},
		},
	})
	require.NoError(t, err)

	chainConfig := opera.CreateTransientEvmChainConfig(
		1,
		[]opera.UpgradeHeight{},
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
	require.ErrorContains(t, err, "skipped txs")
}

func TestState_ApplyBlock_AppliesCorrections(t *testing.T) {
	corrections := Corrections{
		17: map[common.Address]Correction{
			{1}: {
				Balance: *uint256.NewInt(1000),
			},
		},
	}

	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: 17})
	require.NoError(t, err)

	chainConfig := opera.CreateTransientEvmChainConfig(
		1,
		[]opera.UpgradeHeight{},
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	receipts, err := state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, corrections[17], chainConfig, nil, false)
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
}

func TestState_ApplyBlock_BlobBaseFeeIsCalculatedFromHeaderForEthereum(t *testing.T) {
	sonicChainConfig := opera.CreateTransientEvmChainConfig(
		uint64(146),
		[]opera.UpgradeHeight{},
		idx.Block(1),
	)

	cancunTime := *params.MainnetChainConfig.CancunTime
	// excessBlobGas causes CalcBlobFee to return a value > 1.
	excessBlobGas := uint64(3338477)
	blobGasUsed := uint64(0)

	expectedEthBlobFee := eip4844.CalcBlobFee(params.MainnetChainConfig, &types.Header{ExcessBlobGas: &excessBlobGas, Time: cancunTime})
	require.True(t, expectedEthBlobFee.Cmp(big.NewInt(1)) > 0,
		"blob base fee with non-zero ExcessBlobGas should be > 1, got %s", expectedEthBlobFee)

	// A blob tx with BlobFeeCap=1 is below the Ethereum blob base fee (>1)
	// but meets the Sonic default (1). This causes the tx to be rejected on
	// Ethereum but processed on Sonic.
	tests := map[string]struct {
		chainConfig *params.ChainConfig
		upgrades    opera.Upgrades
		wantErr     string
	}{
		"Ethereum": {
			chainConfig: params.MainnetChainConfig,
			upgrades:    opera.Upgrades{},
			wantErr:     "skipped txs",
		},
		"Sonic": {
			chainConfig: sonicChainConfig,
			upgrades:    opera.GetSonicUpgrades(),
			wantErr:     "",
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			sender := crypto.PubkeyToAddress(key.PublicKey)
			signer := types.LatestSignerForChainID(tt.chainConfig.ChainID)

			// Fund the sender.
			state.db.BeginBlock()
			state.db.BeginTransaction()
			state.db.AddBalance(cc.Address(sender), amount.New(1e18))
			state.db.EndTransaction()
			state.db.EndBlock(0)

			// Create a blob tx with BlobFeeCap=1 (below Ethereum's blob base fee).
			blobHash := common.Hash{0x01, 0xab}
			tx := types.MustSignNewTx(key, signer, &types.BlobTx{
				ChainID:    uint256.MustFromBig(tt.chainConfig.ChainID),
				Nonce:      0,
				GasTipCap:  uint256.NewInt(0),
				GasFeeCap:  uint256.NewInt(0),
				Gas:        21_000,
				To:         common.Address{1},
				BlobFeeCap: uint256.NewInt(1), // intentionally too low for Ethereum
				BlobHashes: []common.Hash{blobHash},
			})

			block := types.NewBlockWithHeader(&types.Header{
				Number:        big.NewInt(20_000_000),
				Time:          cancunTime,
				GasLimit:      30_000_000,
				BaseFee:       big.NewInt(0),
				ExcessBlobGas: &excessBlobGas,
				BlobGasUsed:   &blobGasUsed,
				MixDigest:     common.Hash{1}, // non-zero PrevRandao
			}).WithBody(types.Body{Transactions: types.Transactions{tx}})

			processor := evmcore.NewStateProcessorForReplay(
				tt.chainConfig,
				&blockHashHistory{},
				tt.upgrades,
			)

			_, err = state.ApplyBlock(block, testInterpreter(t), processor, tt.upgrades, nil, tt.chainConfig, nil, false)
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
			} else {
				require.NoError(t, err)
			}
		})
	}
}

func TestState_ApplyBlock_ApplySonicVmConfigIfNotEthereumChain(t *testing.T) {
	tests := map[string]struct {
		chainConfig                     *params.ChainConfig
		wantChargeExcessGas             bool
		wantIgnoreGasFeeCap             bool
		wantInsufficientBalanceIsNotErr bool
		wantSkipTipPaymentToCoinbase    bool
	}{
		"Ethereum": {
			chainConfig: params.MainnetChainConfig,
		},
		"Sonic": {
			chainConfig:                     opera.CreateTransientEvmChainConfig(146, []opera.UpgradeHeight{}, idx.Block(1)),
			wantChargeExcessGas:             true,
			wantIgnoreGasFeeCap:             true,
			wantInsufficientBalanceIsNotErr: true,
			wantSkipTipPaymentToCoinbase:    true,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			processor := NewMockProcessor(ctrl)

			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block := types.NewBlockWithHeader(&types.Header{
				GasLimit: 8_000_000,
			})

			processor.EXPECT().ProcessWithDifficulty(
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(), gomock.Any(), gomock.Any(), gomock.Any(),
				gomock.Any(),
			).DoAndReturn(func(
				_ *evmcore.EvmBlock, _ interface{}, cfg vm.Config,
				_ uint64, _ *uint64, _ int, _ interface{},
				_ *big.Int, _ uint64,
			) evmcore.ProcessSummary {
				require.Equal(t, tt.wantChargeExcessGas, cfg.ChargeExcessGas)
				require.Equal(t, tt.wantIgnoreGasFeeCap, cfg.IgnoreGasFeeCap)
				require.Equal(t, tt.wantInsufficientBalanceIsNotErr, cfg.InsufficientBalanceIsNotAnError)
				require.Equal(t, tt.wantSkipTipPaymentToCoinbase, cfg.SkipTipPaymentToCoinbase)
				return evmcore.ProcessSummary{}
			})

			_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, tt.chainConfig, nil, false)
			require.NoError(t, err)
		})
	}
}

func TestState_ApplyBlock_EthereumCancunBlock_AppliesEIP4788(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	chainConfig := params.MainnetChainConfig
	cancunTime := *chainConfig.CancunTime
	beaconRoot := common.Hash{1, 2, 3}
	excessBlobGas := uint64(0)
	blobGasUsed := uint64(0)

	// Deploy the EIP-4788 beacon roots contract so the system call has an effect.
	state.db.BeginBlock()
	state.db.BeginTransaction()
	state.db.SetCode(cc.Address(params.BeaconRootsAddress), params.BeaconRootsCode)
	state.db.EndTransaction()
	state.db.EndBlock(0)

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		// Use a large block number to ensure all forks including London are active.
		Number:                20_000_000,
		Timestamp:             cancunTime,
		GasLimit:              30_000_000,
		ExcessBlobGas:         &excessBlobGas,
		BlobGasUsed:           &blobGasUsed,
		ParentBeaconBlockRoot: beaconRoot.Bytes(),
		// A non-zero PrevRandao activates post-merge EVM rules (Random != nil),
		// which enables Shanghai opcodes like PUSH0 used by the beacon roots contract.
		PrevRandao: []byte{1},
	})
	require.NoError(t, err)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
	require.NoError(t, err)

	// EIP-4788 stores the beacon root at storage slot (timestamp % 8191) + 8191.
	// Verify the root was written to the contract's storage.
	const historyBufferLen = uint64(8191)
	rootSlot := cancunTime%historyBufferLen + historyBufferLen
	var rootKey cc.Key
	binary.BigEndian.PutUint64(rootKey[24:], rootSlot)
	gotRoot := state.db.GetState(cc.Address(params.BeaconRootsAddress), rootKey)
	require.Equal(t, cc.Value(beaconRoot), gotRoot)
}

func TestState_ApplyBlock_EthereumPragueBlock_AppliesEIP7002(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	chainConfig := params.MainnetChainConfig
	pragueTime := *chainConfig.PragueTime
	excessBlobGas := uint64(0)
	blobGasUsed := uint64(0)

	// Deploy the EIP-7002 contract and pre-populate the
	// WITHDRAWAL_REQUEST_COUNT slot (slot 1) to simulate requests queued during
	// a prior block. The system call will compute new excess from this count.
	requestCount := cc.Value{31: 3} // 3 requests queued
	countSlot := cc.Key{31: 1}      // WITHDRAWAL_REQUEST_COUNT_SLOT = 1
	state.db.BeginBlock()
	state.db.BeginTransaction()
	state.db.SetCode(cc.Address(params.WithdrawalQueueAddress), params.WithdrawalQueueCode)
	state.db.SetState(cc.Address(params.WithdrawalQueueAddress), countSlot, requestCount)
	state.db.EndTransaction()
	state.db.EndBlock(0)

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:        20_000_000,
		Timestamp:     pragueTime,
		GasLimit:      30_000_000,
		ExcessBlobGas: &excessBlobGas,
		BlobGasUsed:   &blobGasUsed,
		// A non-zero PrevRandao activates post-merge EVM rules (Random != nil),
		// which enables Shanghai opcodes like PUSH0 used by the system contracts.
		PrevRandao: []byte{1},
	})
	require.NoError(t, err)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
	require.NoError(t, err)

	// Verify the main side effects of the system calls:
	// - EXCESS_REQUESTS (slot 0) = max(0, old_excess + count - TARGET)
	//   EIP-7002: TARGET=2 → 0 + 3 - 2 = 1
	// - WITHDRAWAL_REQUEST_COUNT (slot 1) reset to 0
	excessSlot := cc.Key{} // EXCESS_WITHDRAWAL_REQUESTS_SLOT = 0
	require.Equal(t, cc.Value{31: 1}, state.db.GetState(cc.Address(params.WithdrawalQueueAddress), excessSlot),
		"EIP-7002: excess should be count - TARGET_2 = 3 - 2 = 1")
	require.Equal(t, cc.Value{}, state.db.GetState(cc.Address(params.WithdrawalQueueAddress), countSlot),
		"EIP-7002: request count should be reset to 0")
}

func TestState_ApplyBlock_EthereumPragueBlock_AppliesEIP7251(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	chainConfig := params.MainnetChainConfig
	pragueTime := *chainConfig.PragueTime
	excessBlobGas := uint64(0)
	blobGasUsed := uint64(0)

	// Deploy the EIP-7251 contract and pre-populate the
	// CONSOLIDATION_REQUEST_COUNT slot (slot 1) to simulate requests queued during
	// a prior block. The system call will compute new excess from this count.
	requestCount := cc.Value{31: 3} // 3 requests queued
	countSlot := cc.Key{31: 1}      // CONSOLIDATION_REQUEST_COUNT_SLOT = 1
	state.db.BeginBlock()
	state.db.BeginTransaction()
	state.db.SetCode(cc.Address(params.ConsolidationQueueAddress), params.ConsolidationQueueCode)
	state.db.SetState(cc.Address(params.ConsolidationQueueAddress), countSlot, requestCount)
	state.db.EndTransaction()
	state.db.EndBlock(0)

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:        20_000_000,
		Timestamp:     pragueTime,
		GasLimit:      30_000_000,
		ExcessBlobGas: &excessBlobGas,
		BlobGasUsed:   &blobGasUsed,
		// A non-zero PrevRandao activates post-merge EVM rules (Random != nil),
		// which enables Shanghai opcodes like PUSH0 used by the system contracts.
		PrevRandao: []byte{1},
	})
	require.NoError(t, err)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
	require.NoError(t, err)

	// Verify the main side effects of the system calls:
	// - EXCESS_REQUESTS (slot 0) = max(0, old_excess + count - TARGET)
	//   EIP-7251: TARGET=1 → 0 + 3 - 1 = 2
	// - CONSOLIDATION_REQUEST_COUNT (slot 1) reset to 0
	excessSlot := cc.Key{} // EXCESS_CONSOLIDATION_REQUESTS_SLOT = 0
	require.Equal(t, cc.Value{31: 2}, state.db.GetState(cc.Address(params.ConsolidationQueueAddress), excessSlot),
		"EIP-7251: excess should be count - TARGET_1 = 3 - 1 = 2")
	require.Equal(t, cc.Value{}, state.db.GetState(cc.Address(params.ConsolidationQueueAddress), countSlot),
		"EIP-7251: request count should be reset to 0")
}

func TestState_ApplyBlock_WithdrawalsAreCreditedInEthereumChainsPostMerge(t *testing.T) {
	const sonicChainID = uint64(146)
	sonicUpgrades := opera.GetSonicUpgrades()
	sonicChainConfig := opera.CreateTransientEvmChainConfig(
		sonicChainID,
		[]opera.UpgradeHeight{},
		idx.Block(1),
	)

	const amountGwei = uint64(1000)
	withdrawalAddr := common.Address{0xAB}

	tests := map[string]struct {
		chainConfig    *params.ChainConfig
		upgrades       opera.Upgrades
		difficulty     uint64
		wantBalanceWei uint64
	}{
		"ethereum post-merge": {
			chainConfig:    params.MainnetChainConfig,
			upgrades:       opera.Upgrades{},
			difficulty:     0,
			wantBalanceWei: amountGwei * params.GWei,
		},
		"ethereum pre-merge": {
			chainConfig:    params.MainnetChainConfig,
			upgrades:       opera.Upgrades{},
			difficulty:     1,
			wantBalanceWei: 0,
		},
		"non-ethereum": {
			chainConfig:    sonicChainConfig,
			upgrades:       sonicUpgrades,
			difficulty:     0,
			wantBalanceWei: 0,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
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
				tt.chainConfig,
				&blockHashHistory{},
				tt.upgrades,
			)

			_, err = state.ApplyBlock(block, testInterpreter(t), processor, tt.upgrades, nil, tt.chainConfig, nil, false)
			require.NoError(t, err)

			require.Equal(t, tt.wantBalanceWei, state.db.GetBalance(cc.Address(withdrawalAddr)).Uint64())
		})
	}
}

func TestState_ApplyBlock_RewardsAreAccumulatedInEthereumChainsPreMerge(t *testing.T) {
	const sonicChainID = uint64(146)
	sonicUpgrades := opera.GetSonicUpgrades()
	sonicChainConfig := opera.CreateTransientEvmChainConfig(
		sonicChainID,
		[]opera.UpgradeHeight{},
		idx.Block(1),
	)

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
		chainConfig      *params.ChainConfig
		upgrades         opera.Upgrades
		difficulty       uint64
		wantMinerBalance *uint256.Int
		wantUncleBalance *uint256.Int
	}{
		"ethereum pre-merge": {
			chainConfig:      params.MainnetChainConfig,
			upgrades:         opera.Upgrades{},
			difficulty:       1,
			wantMinerBalance: minerReward,
			wantUncleBalance: uncleReward,
		},
		"ethereum post-merge": {
			chainConfig:      params.MainnetChainConfig,
			upgrades:         opera.Upgrades{},
			difficulty:       0,
			wantMinerBalance: uint256.NewInt(0),
			wantUncleBalance: uint256.NewInt(0),
		},
		"non-ethereum": {
			chainConfig:      sonicChainConfig,
			upgrades:         sonicUpgrades,
			difficulty:       1,
			wantMinerBalance: uint256.NewInt(0),
			wantUncleBalance: uint256.NewInt(0),
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
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
				tt.chainConfig,
				&blockHashHistory{},
				tt.upgrades,
			)

			_, err = state.ApplyBlock(block, testInterpreter(t), processor, tt.upgrades, nil, tt.chainConfig, nil, false)
			require.NoError(t, err)

			require.Equal(t, tt.wantMinerBalance.ToBig(), state.db.GetBalance(cc.Address(coinbase)).ToBig())
			require.Equal(t, tt.wantUncleBalance.ToBig(), state.db.GetBalance(cc.Address(uncleCoinbase)).ToBig())
		})
	}
}

func TestState_ApplyBlock_CanApplyBlockToArchiveState(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory:   t.TempDir(),
		WithArchive: true,
		Schema:      5,
	})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	chainConfig := opera.CreateTransientEvmChainConfig(
		1,
		[]opera.UpgradeHeight{},
		idx.Block(2),
	)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{},
		opera.Upgrades{},
	)

	// Apply blocks 0, 1 and 2 normally to populate the archive.
	for i := range 3 {
		block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: uint64(i)})
		require.NoError(t, err)
		_, err = state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, false)
		require.NoError(t, err)
	}
	require.NoError(t, state.db.Flush())

	// Apply block 1 using archive mode.
	block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: 1})
	require.NoError(t, err)

	receipts, err := state.ApplyBlock(block, testInterpreter(t), processor, opera.Upgrades{}, nil, chainConfig, nil, true)
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestState_setBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	addr := common.Address{1}
	balance := big.NewInt(1000)
	setBalance(state.db, addr, balance)

	// Check initial balance
	have := state.db.GetBalance(cc.Address(addr))
	require.Equal(t, balance.Uint64(), have.Uint64())

	// Increase balance
	balance = big.NewInt(1500)
	setBalance(state.db, addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(1500), have.Uint64())

	// Decrease balance
	balance = big.NewInt(750)
	setBalance(state.db, addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(750), have.Uint64())
}

func testInterpreter(t *testing.T) tosca.Interpreter {
	t.Helper()
	interpreter, err := tosca.NewInterpreter("sfvm")
	require.NoError(t, err)
	return interpreter
}
