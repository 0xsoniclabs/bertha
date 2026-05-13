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
	"log/slog"
	"os"
)

// SnapshotHandler is a utility to handle intermediate state database snapshots, allowing
// to create snapshots in a specific block interval.
type SnapshotHandler struct {
	blockInterval     uint64
	startBlock        uint64
	endBlock          uint64
	pastSnapshotList  []*uint64
	pastSnapshotIndex uint64
}

func NewSnapshotHandler(blockInterval uint64, startBlock uint64, endBlock uint64, snapshotToKeep uint64) *SnapshotHandler {
	return &SnapshotHandler{
		blockInterval:     blockInterval,
		startBlock:        startBlock,
		endBlock:          endBlock,
		pastSnapshotList:  make([]*uint64, snapshotToKeep),
		pastSnapshotIndex: 0,
	}
}

// ShouldCreateSnapshot returns true if a snapshot should be created at the given block number.
// A snapshot should be created if the block interval is set and the current block number is between
// the start block height and end block height and a multiple of the specified interval.
func (s *SnapshotHandler) ShouldCreateSnapshot(currentBlock uint64) bool {
	return s.blockInterval > 0 && currentBlock >= s.startBlock && currentBlock <= s.endBlock && currentBlock%s.blockInterval == 0
}

// Snapshot creates a snapshot of the state database at the given block number.
// It closes and reopens the state to ensure all data is flushed to disk,
// copies the state database directory to a new location, and removes a previous snapshot
// if needed to keep the specified number of snapshots.
func (s *SnapshotHandler) Snapshot(currentBlock uint64, state *State) (*State, error) {
	stateDBDir := state.stateParameter.Directory
	if !s.ShouldCreateSnapshot(currentBlock) {
		return state, nil
	}
	slog.Info("Creating state database snapshot", "block_number", currentBlock)

	// Remove the oldest snapshot if it exists
	oldestSnapshotDir := s.GetOldestSnapshotDir(stateDBDir)
	if s.pastSnapshotList[s.pastSnapshotIndex] != nil && oldestSnapshotDir != "" {
		slog.Info("Removing previous state database snapshot", "directory", oldestSnapshotDir)
		err := os.RemoveAll(oldestSnapshotDir)
		if err != nil {
			return nil, fmt.Errorf("failed to remove previous state database snapshot: %w", err)
		}
	}
	s.pastSnapshotList[s.pastSnapshotIndex] = &currentBlock
	s.pastSnapshotIndex = (s.pastSnapshotIndex + 1) % uint64(len(s.pastSnapshotList))

	// Create the snapshot by copying the state database directory
	snapshotDir := s.snapshotDir(stateDBDir, currentBlock)
	if err := os.RemoveAll(snapshotDir); err != nil { // remove existing snapshot if any
		return nil, fmt.Errorf("failed to remove existing snapshot directory %q: %w", snapshotDir, err)
	}
	// Close and reopen the state to ensure all data is flushed to disk
	err := state.Close()
	if err != nil {
		return nil, fmt.Errorf("failed to close state database before snapshot: %w", err)
	}
	err = os.CopyFS(snapshotDir, os.DirFS(stateDBDir))
	if err != nil {
		return nil, fmt.Errorf("failed to copy state database for snapshot: %w", err)
	}
	// Open the state again
	state, err = NewState(state.stateParameter)
	if err != nil {
		return nil, fmt.Errorf("failed to reopen state database after snapshot: %w", err)
	}

	slog.Info("Snapshot created successfully", "snapshot_directory", snapshotDir)
	return state, nil
}

// GetOldestSnapshotDir returns the path to the oldest snapshot created.
// If there are no snapshots, an empty string is returned.
func (s *SnapshotHandler) GetOldestSnapshotDir(stateDBDir string) string {
	for cnt := uint64(0); cnt < uint64(len(s.pastSnapshotList)); cnt++ {
		i := (s.pastSnapshotIndex + cnt) % uint64(len(s.pastSnapshotList))
		if s.pastSnapshotList[i] != nil {
			return s.snapshotDir(stateDBDir, *s.pastSnapshotList[i])
		}
	}
	return ""
}

// GetSnapshotDirs returns a list of paths to the current existing snapshots.
func (s *SnapshotHandler) GetSnapshotDirs(stateDBDir string) []string {
	list := make([]string, 0, len(s.pastSnapshotList))
	for _, pos := range s.pastSnapshotList {
		if pos != nil {
			list = append(list, s.snapshotDir(stateDBDir, *pos))
		}
	}
	return list
}

func (s *SnapshotHandler) snapshotDir(stateDBDir string, blockNum uint64) string {
	return fmt.Sprintf("%s_snapshot_%d", stateDBDir, blockNum)
}
