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

package state

import (
	"fmt"
	"os"
	"testing"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
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

func TestState_Close_ClosesDatabaseOnlyOnce(t *testing.T) {
	db := carmen.NewMockStateDB(gomock.NewController(t))
	db.EXPECT().Close()
	state := &State{DB: db}

	require.NoError(t, state.Close())
	require.NoError(t, state.Close())
}

func TestState_GetStateRoot_ForwardsCallToDatabase(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	hash := cc.Hash{1, 2, 3}
	db.EXPECT().GetCommitment().Return(
		future.Immediate(result.Ok(hash)),
	)

	state := &State{DB: db}

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

	state := &State{DB: db}

	_, err := state.GetStateRoot().Await().Get()
	require.ErrorIs(t, err, issue)
}

func TestState_ApplyGenesis_CanApplyGenesis(t *testing.T) {
	genesis := &makefakegenesis.GenesisJson{
		Accounts: []makefakegenesis.Account{
			{
				Address: common.Address{1},
				Balance: uint256.NewInt(1000),
				Nonce:   1,
				Code:    []byte{1, 2, 3},
				Storage: map[common.Hash]common.Hash{
					common.HexToHash("0x1"): common.HexToHash("0x2"),
					common.HexToHash("0x3"): common.HexToHash("0x4"),
				},
			},
			{Address: common.Address{2}},
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

	db := state.DB
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

	require.Zero(t, db.GetBalance(cc.Address{2}).Uint64())
}

func TestState_ApplyGenesis_ReturnsErrorIfEndBlockFails(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	issue := fmt.Errorf("injected error")
	gomock.InOrder(
		db.EXPECT().BeginBlock(),
		db.EXPECT().BeginTransaction(),
		db.EXPECT().EndTransaction(),
		db.EXPECT().EndBlock(uint64(0)).Return(nil, issue),
	)

	state := &State{DB: db}
	require.ErrorIs(state.ApplyGenesis(&makefakegenesis.GenesisJson{}), issue)
}

func TestState_EndBlockAndCommit_EndsAndCommitsBlock(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)
	staged := carmen.NewMockStagedBlock(ctrl)

	handle := carmen.NewWaitHandle(make(chan error))
	gomock.InOrder(
		db.EXPECT().EndBlock(uint64(42)).Return(staged, nil),
		staged.EXPECT().Commit().Return(handle, nil),
	)

	got, err := EndBlockAndCommit(&State{DB: db}, 42)
	require.NoError(err)
	require.Same(handle, got)
}

func TestState_EndBlockAndCommit_ReportsErrors(t *testing.T) {
	issue := fmt.Errorf("injected error")
	tests := map[string]struct {
		setup   func(db *carmen.MockStateDB, staged *carmen.MockStagedBlock)
		wantErr string
	}{
		"end block fails": {
			setup: func(db *carmen.MockStateDB, staged *carmen.MockStagedBlock) {
				db.EXPECT().EndBlock(uint64(42)).Return(nil, issue)
			},
			wantErr: "failed to end block 42",
		},
		"staged block is nil": {
			setup: func(db *carmen.MockStateDB, staged *carmen.MockStagedBlock) {
				db.EXPECT().EndBlock(uint64(42)).Return(nil, nil)
			},
			wantErr: "staged state is nil",
		},
		"commit fails": {
			setup: func(db *carmen.MockStateDB, staged *carmen.MockStagedBlock) {
				db.EXPECT().EndBlock(uint64(42)).Return(staged, nil)
				staged.EXPECT().Commit().Return(nil, issue)
			},
			wantErr: "failed to commit block 42",
		},
	}

	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			require := require.New(t)
			ctrl := gomock.NewController(t)
			db := carmen.NewMockStateDB(ctrl)
			staged := carmen.NewMockStagedBlock(ctrl)
			test.setup(db, staged)

			_, err := EndBlockAndCommit(&State{DB: db}, 42)
			require.ErrorContains(err, test.wantErr)
		})
	}
}
