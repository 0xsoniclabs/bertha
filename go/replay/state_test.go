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
		historyAdapter{},
		opera.Upgrades{},
	)

	receipts, err := state.ApplyBlock(block, processor, opera.Upgrades{}, nil)
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
		historyAdapter{},
		opera.Upgrades{},
	)

	_, err = state.ApplyBlock(block, processor, opera.Upgrades{}, nil)
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
		historyAdapter{},
		opera.Upgrades{},
	)

	receipts, err := state.ApplyBlock(block, processor, opera.Upgrades{}, corrections[17])
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
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
