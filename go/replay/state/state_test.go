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

func TestNewState_CreatesEmptyDatabaseOrReportsUnusableDirectory(t *testing.T) {
	tests := map[string]struct {
		directory   func(*testing.T) string
		withArchive bool
		wantErr     string
	}{
		"invalid directory": {
			directory: func(*testing.T) string { return "/invalid/directory/that/does/not/exist" },
			wantErr:   "failed to create state dir",
		},
		"without archive": {
			directory: func(t *testing.T) string { return t.TempDir() },
		},
		"with archive": {
			directory:   func(t *testing.T) string { return t.TempDir() },
			withArchive: true,
		},
		"read-only directory": {
			directory: func(t *testing.T) string {
				dir := t.TempDir()
				require.NoError(t, os.Chmod(dir, 0555))
				t.Cleanup(func() { require.NoError(t, os.Chmod(dir, 0755)) })
				return dir
			},
			wantErr: "permission denied",
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{
				Directory:   tt.directory(t),
				WithArchive: tt.withArchive,
				Schema:      5,
				Variant:     "go-file",
			})
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			root, err := state.GetStateRoot().Await().Get()
			require.NoError(t, err)
			require.Equal(t, types.EmptyRootHash, root)
		})
	}
}

func TestState_Close_ClosesDatabaseOnlyOnce(t *testing.T) {
	db := carmen.NewMockStateDB(gomock.NewController(t))
	db.EXPECT().Close()
	state := &State{DB: db}

	require.NoError(t, state.Close())
	require.NoError(t, state.Close())
}

func TestState_GetStateRoot_ForwardsCommitmentOfDatabase(t *testing.T) {
	issue := fmt.Errorf("database error")
	tests := map[string]struct {
		commitment result.Result[cc.Hash]
		want       common.Hash
		wantErr    error
	}{
		"error":      {commitment: result.Err[cc.Hash](issue), wantErr: issue},
		"commitment": {commitment: result.Ok(cc.Hash{1, 2, 3}), want: common.Hash{1, 2, 3}},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			db := carmen.NewMockStateDB(gomock.NewController(t))
			db.EXPECT().GetCommitment().Return(future.Immediate(tt.commitment))

			got, err := (&State{DB: db}).GetStateRoot().Await().Get()
			require.ErrorIs(t, err, tt.wantErr)
			require.Equal(t, tt.want, got)
		})
	}
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
	issue := fmt.Errorf("injected error")
	handle := carmen.NewWaitHandle(make(chan error))
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
		"block is committed": {
			setup: func(db *carmen.MockStateDB, staged *carmen.MockStagedBlock) {
				gomock.InOrder(
					db.EXPECT().EndBlock(uint64(42)).Return(staged, nil),
					staged.EXPECT().Commit().Return(handle, nil),
				)
			},
		},
	}

	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			require := require.New(t)
			ctrl := gomock.NewController(t)
			db := carmen.NewMockStateDB(ctrl)
			staged := carmen.NewMockStagedBlock(ctrl)
			test.setup(db, staged)

			got, err := EndBlockAndCommit(&State{DB: db}, 42)
			if test.wantErr != "" {
				require.ErrorContains(err, test.wantErr)
				return
			}
			require.NoError(err)
			require.Same(handle, got)
		})
	}
}
