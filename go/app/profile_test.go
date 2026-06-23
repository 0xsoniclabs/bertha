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

package app

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestStartProfiling_ValidPath_CreatesNewFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "cpu.prof")
	require.False(t, exists(path))

	profiler, err := StartCPUProfile(path)
	require.NoError(t, err)
	require.True(t, exists(path))

	err = profiler.Stop()
	require.NoError(t, err)
	require.True(t, exists(path))
}

func TestStartProfiling_InvalidPath_ReturnsAnError(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "nonexistent", "cpu.prof")
	require.False(t, exists(path))

	_, err := StartCPUProfile(path)
	require.Error(t, err)
	require.False(t, exists(path))
}

func TestStartProfiling_StartingASecondProfilerFails(t *testing.T) {
	dir := t.TempDir()
	path1 := filepath.Join(dir, "cpu1.prof")
	path2 := filepath.Join(dir, "cpu2.prof")

	profiler1, err := StartCPUProfile(path1)
	require.NoError(t, err)
	require.True(t, exists(path1))

	// Starting a second profiler should fail
	profiler2, err := StartCPUProfile(path2)
	require.Error(t, err)
	require.Nil(t, profiler2)
	require.False(t, exists(path2))

	err = profiler1.Stop()
	require.NoError(t, err)
	require.True(t, exists(path1))
}

func exists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
