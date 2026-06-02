// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

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
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
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

	receipts, err := state.ApplyBlock(block, processor, opera.Upgrades{}, nil, chainConfig, nil)
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

	_, err = state.ApplyBlock(block, processor, opera.Upgrades{}, nil, chainConfig, nil)
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

	receipts, err := state.ApplyBlock(block, processor, opera.Upgrades{}, corrections[17], chainConfig, nil)
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
}

func TestState_ApplyBlock_ApplySonicVmConfigIfNotEthereumChain(t *testing.T) {
	// Note: the vm config setting "InsufficientBalanceIsNotAnError" is used as
	// a proxy for whether the Sonic VM config is applied.
	tests := map[string]struct {
		chainConfig *params.ChainConfig
		blockNumber uint64
		wantSkipped bool
	}{
		"Ethereum": {
			chainConfig: params.MainnetChainConfig,
			blockNumber: 3_000_000, // past EIP-155 activation (block 2,675,000)
			wantSkipped: true,
		},
		"Sonic": {
			chainConfig: opera.CreateTransientEvmChainConfig(146, []opera.UpgradeHeight{}, idx.Block(1)),
			blockNumber: 1,
			wantSkipped: false,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			signer := types.LatestSignerForChainID(tt.chainConfig.ChainID)

			// Sender has no balance, but the tx transfers 1 wei.
			tx := types.MustSignNewTx(key, signer, &types.LegacyTx{
				Nonce:    0,
				To:       &common.Address{1},
				Gas:      21_000,
				GasPrice: big.NewInt(0),
				Value:    big.NewInt(1),
			})

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				Number:       tt.blockNumber,
				GasLimit:     8_000_000,
				Transactions: []*blockdb.Transaction{convert.ToBerthaTransaction(tx)},
			})
			require.NoError(t, err)

			processor := evmcore.NewStateProcessorForReplay(tt.chainConfig, &blockHashHistory{}, opera.Upgrades{})

			receipts, err := state.ApplyBlock(block, processor, opera.Upgrades{}, nil, tt.chainConfig, nil)
			if tt.wantSkipped {
				require.ErrorContains(t, err, "skipped txs")
			} else {
				require.NoError(t, err)
				require.Len(t, receipts, 1)
			}
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

	_, err = state.ApplyBlock(block, processor, opera.Upgrades{}, nil, chainConfig, nil)
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

	_, err = state.ApplyBlock(block, processor, opera.Upgrades{}, nil, chainConfig, nil)
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

	_, err = state.ApplyBlock(block, processor, opera.Upgrades{}, nil, chainConfig, nil)
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

func TestState_ApplyBlock_WithdrawalsAreCreditedInEthereumChains(t *testing.T) {
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
		wantBalanceWei uint64
	}{
		"ethereum": {
			chainConfig:    params.MainnetChainConfig,
			upgrades:       opera.Upgrades{},
			wantBalanceWei: amountGwei * params.GWei,
		},
		"non-ethereum": {
			chainConfig:    sonicChainConfig,
			upgrades:       sonicUpgrades,
			wantBalanceWei: 0,
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				GasLimit: 8_000_000,
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

			_, err = state.ApplyBlock(block, processor, tt.upgrades, nil, tt.chainConfig, nil)
			require.NoError(t, err)

			require.Equal(t, tt.wantBalanceWei, state.db.GetBalance(cc.Address(withdrawalAddr)).Uint64())
		})
	}
}

func TestState_setBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	addr := common.Address{1}
	balance := big.NewInt(1000)
	state.setBalance(addr, balance)

	// Check initial balance
	have := state.db.GetBalance(cc.Address(addr))
	require.Equal(t, balance.Uint64(), have.Uint64())

	// Increase balance
	balance = big.NewInt(1500)
	state.setBalance(addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(1500), have.Uint64())

	// Decrease balance
	balance = big.NewInt(750)
	state.setBalance(addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(750), have.Uint64())
}
