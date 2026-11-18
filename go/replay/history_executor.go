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
	"fmt"
	"sync"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/replay/state"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

//go:generate mockgen -source=history_executor.go -destination=history_executor_mock.go -package=replay

// HistoryExecutor executes a sequence of blocks, one after another, on a state.
type HistoryExecutor interface {
	ChainID() uint64
	IsMptConformant() bool
	IsVerkleConformant() bool
	ApplyBlock(*types.Block) (
		types.Receipts,
		future.Future[result.Result[common.Hash]],
		error,
	)
	// MaybeSnapshot creates a state database snapshot if one is due at the
	// given block number. It must be called after ApplyBlock and receives the
	// state-root future returned by ApplyBlock so it can be resolved before
	// the state pointer is swapped.
	MaybeSnapshot(
		blockNumber uint64,
		stateRoot future.Future[result.Result[common.Hash]],
	) (future.Future[result.Result[common.Hash]], error)
	GetBlockHash(blockNumber uint64) common.Hash
	CloneBlockHashHistory() *blockHashHistory
	// GetHeadHeader returns the header of the block whose state the live state
	// holds.
	GetHeadHeader() *types.Header
}

// ArchiveExecutor exposes the operations required by the archive
// verifier. Implementations must be safe to call concurrently with block
// application on the same underlying state.
type ArchiveExecutor interface {
	GetArchiveBlockHeight() (height uint64, empty bool, err error)
	// ApplyBlockOnArchive re-executes the given block on the archived state of
	// its parent, whose header is parentHeader, serving BLOCKHASH from
	// hashHistory, and returns the receipts.
	ApplyBlockOnArchive(block *types.Block, parentHeader *types.Header, hashHistory *blockHashHistory) (types.Receipts, error)
}

// StateFlusher exposes the state-flush operation required by the progress
// logger. Implementations must be safe to call concurrently with block
// application on the same underlying state.
type StateFlusher interface {
	FlushState() error
}

// BlockExecutor executes the blocks on a given State.
type BlockExecutor interface {
	// ExecuteBlock executes the given block on the live state, serving BLOCKHASH
	// from hashHistory, and returns its receipts and complete header.
	ExecuteBlock(state *state.State, block *types.Block, hashHistory evmcore.DummyChain) (types.Receipts, *types.Header, error)
	// ExecuteBlockOnArchive executes the given block on the archived state of its
	// parent, whose header is parentHeader, serving BLOCKHASH from hashHistory,
	// and returns its receipts.
	ExecuteBlockOnArchive(state *state.State, block *types.Block, parentHeader *types.Header, hashHistory evmcore.DummyChain) (types.Receipts, error)
}

// historyExecutor implements HistoryExecutor, ArchiveExecutor and StateFlusher
// on a State, executing each block with its BlockExecutor.
//
// Concurrency: two mutexes coordinate access to the historyExecutor.
//
//   - stateSwitchMutex guards accesses to the underlying *state.State pointer.
//     RLock is held during operations that use the current state (block
//     application via ApplyBlock, archive queries via GetArchiveBlockHeight
//     and ApplyBlockOnArchive, flushes via FlushState). Lock is held only for
//     the snapshot pointer swap in MaybeSnapshot, which closes and reopens
//     the state database.
//
//   - stateModifyMutex serializes live block application. It ensures that at
//     most one goroutine executes ApplyBlock at a time and protects
//     mutations to its fields performed during block application
//     (notably blockHashHistory and headHeader writes).
//
// Together they allow live block application and concurrent archive
// verification / flushes to proceed in parallel while ensuring the state
// pointer is not swapped while any operation is using it. Lock ordering:
// callers acquiring both locks must take stateSwitchMutex before
// stateModifyMutex.
type historyExecutor struct {
	chainID          uint64
	blockExecutor    BlockExecutor
	blockHashHistory *blockHashHistory
	// headHeader is the header of the block whose state the live state holds.
	headHeader       *types.Header
	state            *state.State
	stateSwitchMutex sync.RWMutex
	stateModifyMutex sync.Mutex
	schema           carmen.Schema
	snapshotHandler  *SnapshotHandler
}

func (e *historyExecutor) ChainID() uint64 {
	return e.chainID
}

func (e *historyExecutor) IsMptConformant() bool {
	return e.schema == 5
}

func (e *historyExecutor) IsVerkleConformant() bool {
	return e.schema == 6
}

func (e *historyExecutor) GetBlockHash(blockNumber uint64) common.Hash {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	e.stateModifyMutex.Lock()
	defer e.stateModifyMutex.Unlock()
	return e.blockHashHistory.GetBlockHash(blockNumber)
}

func (e *historyExecutor) CloneBlockHashHistory() *blockHashHistory {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	e.stateModifyMutex.Lock()
	defer e.stateModifyMutex.Unlock()
	return e.blockHashHistory.Clone()
}

func (e *historyExecutor) GetHeadHeader() *types.Header {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	e.stateModifyMutex.Lock()
	defer e.stateModifyMutex.Unlock()
	return e.headHeader
}

// ApplyBlock applies the given block to the state database with the block
// executor, replaces the header of block by the complete header the block
// executor returns and records it as the head header and its hash in the block
// hash history. It returns the receipts and state-root future.
func (e *historyExecutor) ApplyBlock(block *types.Block) (
	types.Receipts,
	future.Future[result.Result[common.Hash]],
	error,
) {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	e.stateModifyMutex.Lock()
	defer e.stateModifyMutex.Unlock()

	zoneBlock := tracy.ZoneBegin("LiveBlockProcessing")
	defer zoneBlock.End()

	// Block 0 is skipped since it is equivalent with the genesis data
	// import. The archive does not accept two blocks with the same number.
	// Only its hash is recorded, for BLOCKHASH in the blocks after it.
	if block.NumberU64() == 0 {
		e.blockHashHistory.SetBlockHash(0, block.Hash())
		return nil, e.state.GetStateRoot(), nil
	}

	receipts, header, err := e.blockExecutor.ExecuteBlock(e.state, block, e.blockHashHistory)
	if err != nil {
		return nil, future.Future[result.Result[common.Hash]]{}, err
	}
	*block = *block.WithSeal(header)

	e.headHeader = header
	e.blockHashHistory.SetBlockHash(block.NumberU64(), header.Hash())

	return receipts, e.state.GetStateRoot(), nil
}

// MaybeSnapshot creates a state database snapshot if one is due at the given
// block number. When a snapshot is triggered, the stateRoot future is awaited
// and returned as an immediate future so it remains valid after the underlying
// state database has been closed and reopened. When no snapshot is due, the
// stateRoot future is returned unchanged.
//
// The snapshot swap runs under an exclusive lock on stateSwitchMutex, so it
// will block until all in-flight operations holding stateSwitchMutex.RLock()
// complete. The stateRoot future must be resolved before the state pointer is
// swapped so it remains valid after the old state DB is closed and reopened.
func (e *historyExecutor) MaybeSnapshot(
	blockNumber uint64,
	stateRoot future.Future[result.Result[common.Hash]],
) (future.Future[result.Result[common.Hash]], error) {
	if !e.snapshotHandler.ShouldCreateSnapshot(blockNumber) {
		return stateRoot, nil
	}
	// Resolve the state-root future before swapping the state pointer so it
	// remains valid after the underlying state has been closed and reopened.
	stateRoot = future.Immediate(stateRoot.Await())

	e.stateSwitchMutex.Lock()
	defer e.stateSwitchMutex.Unlock()
	newState, err := e.snapshotHandler.Snapshot(blockNumber, e.state)
	if err != nil {
		return stateRoot, fmt.Errorf("failed to create snapshot at block %d: %w", blockNumber, err)
	}
	e.state = newState
	return stateRoot, nil
}

// FlushState flushes pending writes in the state database to disk.
// It is safe to call concurrently with block application (ApplyBlock) and
// other archive-side operations.
func (e *historyExecutor) FlushState() error {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	return e.state.DB.Flush()
}

// GetArchiveBlockHeight returns the highest block currently available in the
// archive. It is safe to call concurrently with block application.
func (e *historyExecutor) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	return e.state.GetArchiveBlockHeight()
}

// ApplyBlockOnArchive re-executes the given block with the block executor on
// the archived state of its parent, whose header is parentHeader, serving
// BLOCKHASH from hashHistory, and returns the receipts. Block 0 is skipped since
// it is equivalent with the genesis data import. The live state is not
// modified. It is safe to call concurrently with block application.
func (e *historyExecutor) ApplyBlockOnArchive(block *types.Block, parentHeader *types.Header, hashHistory *blockHashHistory) (types.Receipts, error) {
	if block.NumberU64() == 0 {
		return nil, nil
	}
	e.stateSwitchMutex.RLock()
	defer e.stateSwitchMutex.RUnlock()
	return e.blockExecutor.ExecuteBlockOnArchive(e.state, block, parentHeader, hashHistory)
}

// getExpectedStateRoot returns the expected state root for the given block, based on the DB schema.
func getExpectedStateRoot(executor HistoryExecutor, block *blockdb.Block) common.Hash {
	var stateRoot []byte
	if executor.IsMptConformant() {
		stateRoot = block.StateRoot
	} else if executor.IsVerkleConformant() {
		stateRoot = block.VerkleStateRoot
	}
	return common.BytesToHash(stateRoot)
}

// updateStateRoot updates the state root in the block based on the DB schema.
func updateStateRoot(executor HistoryExecutor, block *blockdb.Block, stateRoot common.Hash) {
	if executor.IsMptConformant() {
		block.StateRoot = stateRoot.Bytes()
	} else if executor.IsVerkleConformant() {
		block.VerkleStateRoot = stateRoot.Bytes()
	}
}
