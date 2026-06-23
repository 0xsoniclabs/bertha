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
	"os"
	"testing"

	"github.com/stretchr/testify/require"
)

func Test_SnapshotHandler_ShouldCreateSnapshot(t *testing.T) {
	require := require.New(t)

	handler := &SnapshotHandler{
		blockInterval: 1000,
		startBlock:    100,
		endBlock:      3000,
	}

	require.True(handler.ShouldCreateSnapshot(1000))
	require.True(handler.ShouldCreateSnapshot(2000))
	require.False(handler.ShouldCreateSnapshot(1500))
	require.False(handler.ShouldCreateSnapshot(0))
	require.False(handler.ShouldCreateSnapshot(50))
	require.False(handler.ShouldCreateSnapshot(4000))
}

func Test_NewSnapshotHandler_InitializeSnapshotHandlerCorrectly(t *testing.T) {
	require := require.New(t)

	handler := NewSnapshotHandler(1000, 100, 3000, 3)
	require.Equal(uint64(1000), handler.blockInterval)
	require.Equal(uint64(100), handler.startBlock)
	require.Equal(uint64(3000), handler.endBlock)
	require.Equal(len(handler.pastSnapshotList), 3)
	for _, blockNumber := range handler.pastSnapshotList {
		require.Nil(blockNumber)
	}
}

func Test_SnapshotHandler_CreatesAndRemovesSnapshots(t *testing.T) {
	require := require.New(t)

	dir := t.TempDir()
	state, err := NewState(
		StateParameters{Directory: dir, Schema: 5},
	)
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	handler := NewSnapshotHandler(1000, 100, 10000, 3)

	newState, err := handler.Snapshot(1000, state)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(2000, newState)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(3000, newState)
	require.NoError(err)
	require.NotNil(newState)

	_, err = os.Stat(handler.snapshotDir(dir, 1000))
	require.NoError(err)
	_, err = os.Stat(handler.snapshotDir(dir, 2000))
	require.NoError(err)
	_, err = os.Stat(handler.snapshotDir(dir, 3000))
	require.NoError(err)

	// Next two snapshots should clear the oldest two ones
	newState, err = handler.Snapshot(4000, newState)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(5000, newState)
	require.NoError(err)
	require.NotNil(newState)

	_, err = os.Stat(handler.snapshotDir(dir, 1000))
	require.Error(err)
	_, err = os.Stat(handler.snapshotDir(dir, 2000))
	require.Error(err)
	_, err = os.Stat(handler.snapshotDir(dir, 3000))
	require.NoError(err)
	_, err = os.Stat(handler.snapshotDir(dir, 4000))
	require.NoError(err)
	_, err = os.Stat(handler.snapshotDir(dir, 5000))
	require.NoError(err)
}

func Test_SnapshotHandler_GetOldestSnapshotDirReturnsOldestSnapshot(t *testing.T) {
	require := require.New(t)
	dir := t.TempDir()

	state, err := NewState(
		StateParameters{Directory: dir, Schema: 5},
	)
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	handler := NewSnapshotHandler(1000, 100, 10000, 3)

	newState, err := handler.Snapshot(1000, state)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(2000, newState)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(3000, newState)
	require.NoError(err)
	require.NotNil(newState)

	oldest := handler.GetOldestSnapshotDir(dir)
	require.Equal(oldest, handler.snapshotDir(dir, 1000))

	newState, err = handler.Snapshot(4000, newState)
	require.NoError(err)
	require.NotNil(newState)
	_, err = os.Stat(handler.snapshotDir(dir, 1000))
	require.Error(err)
	newOldest := handler.GetOldestSnapshotDir(dir)
	require.Equal(newOldest, handler.snapshotDir(dir, 2000))
}

func Test_SnapshotHandler_GetSnapshotDirsReturnsExistingSnapshotList(t *testing.T) {
	require := require.New(t)
	dir := t.TempDir()

	state, err := NewState(
		StateParameters{Directory: dir, Schema: 5},
	)
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	handler := NewSnapshotHandler(1000, 100, 10000, 3)

	snapshotList := handler.GetSnapshotDirs(dir)
	require.Empty(snapshotList)

	newState, err := handler.Snapshot(1000, state)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(2000, newState)
	require.NoError(err)
	require.NotNil(newState)
	newState, err = handler.Snapshot(3000, newState)
	require.NoError(err)
	require.NotNil(newState)

	snapshotList = handler.GetSnapshotDirs(dir)
	require.Equal(snapshotList, []string{
		handler.snapshotDir(dir, 1000),
		handler.snapshotDir(dir, 2000),
		handler.snapshotDir(dir, 3000),
	})

}

func Test_SnapshotHandler_snapshotDirReturnsCorrectName(t *testing.T) {
	handler := SnapshotHandler{}
	require.Equal(t, "directory_snapshot_1000", handler.snapshotDir("directory", 1000))
}
