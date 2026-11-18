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

	"github.com/0xsoniclabs/bertha/replay/state"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

// applyBlock applies the block with ApplyBlock and drops the state root.
func applyBlock(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
	receipts, _, err := executor.ApplyBlock(block)
	return receipts, err
}

// applyBlockOnArchive applies the block with ApplyBlockOnArchive, an empty
// parent header and an empty hash history.
func applyBlockOnArchive(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
	return executor.ApplyBlockOnArchive(block, &types.Header{}, &blockHashHistory{})
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_SkipBlockZero(t *testing.T) {
	tests := map[string]struct {
		apply          func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		readsStateRoot bool
		recordsHash    bool
	}{
		"ApplyBlock":          {apply: applyBlock, readsStateRoot: true, recordsHash: true},
		"ApplyBlockOnArchive": {apply: applyBlockOnArchive},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := carmen.NewMockStateDB(ctrl)
			if tt.readsStateRoot {
				db.EXPECT().GetCommitment().Return(future.Immediate(result.Ok(cc.Hash{})))
			}
			// The block executor expects no calls.
			executor := &historyExecutor{
				blockExecutor:    NewMockBlockExecutor(ctrl),
				blockHashHistory: &blockHashHistory{},
				state:            &state.State{DB: db},
			}

			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(0)})
			receipts, err := tt.apply(executor, block)
			require.NoError(t, err)
			require.Nil(t, receipts)
			want := &blockHashHistory{}
			if tt.recordsHash {
				want.SetBlockHash(0, block.Hash())
			}
			require.Equal(t, want, executor.blockHashHistory)
		})
	}
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_ForwardToBlockExecutor(t *testing.T) {
	injected := errors.New("injected")
	tests := map[string]struct {
		apply  func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		expect func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block)
	}{
		"ApplyBlock": {
			apply: applyBlock,
			expect: func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block) {
				blockExecutor.ExecuteBlock(executor.state, block, executor.blockHashHistory).Return(nil, nil, injected)
			},
		},
		"ApplyBlockOnArchive": {
			apply: applyBlockOnArchive,
			expect: func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block) {
				blockExecutor.ExecuteBlockOnArchive(executor.state, block, &types.Header{}, &blockHashHistory{}).Return(nil, injected)
			},
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			blockExecutor := NewMockBlockExecutor(gomock.NewController(t))
			// The executor's head header and hash history differ from the empty ones of applyBlockOnArchive.
			executor := &historyExecutor{blockExecutor: blockExecutor, blockHashHistory: &blockHashHistory{}, headHeader: &types.Header{GasUsed: 1}, state: &state.State{}}
			executor.blockHashHistory.SetBlockHash(0, common.Hash{1})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)})
			tt.expect(blockExecutor.EXPECT(), executor, block)

			_, err := tt.apply(executor, block)
			require.ErrorIs(t, err, injected)
		})
	}
}

func TestHistoryExecutor_ApplyBlock_ReplacesHeaderAndRecordsItAsHead(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)
	db.EXPECT().GetCommitment().Return(future.Immediate(result.Ok(cc.Hash{})))
	state := &state.State{DB: db}
	blockExecutor := NewMockBlockExecutor(ctrl)
	executor := &historyExecutor{blockExecutor: blockExecutor, blockHashHistory: &blockHashHistory{}, state: state}
	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)})
	header := &types.Header{Number: big.NewInt(1), GasUsed: 21_000}
	want := types.Receipts{{}}
	blockExecutor.EXPECT().ExecuteBlock(state, block, executor.blockHashHistory).Return(want, header, nil)

	receipts, _, err := executor.ApplyBlock(block)
	require.NoError(t, err)
	require.Equal(t, want, receipts)
	require.Equal(t, header.Hash(), block.Hash())
	require.Equal(t, header, executor.GetHeadHeader())
	require.Equal(t, header.Hash(), executor.blockHashHistory.GetBlockHash(1))
}
