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

	profiler, err := StartCpuProfile(path)
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

	_, err := StartCpuProfile(path)
	require.Error(t, err)
	require.False(t, exists(path))
}

func TestStartProfiling_StartingASecondProfilerFails(t *testing.T) {
	dir := t.TempDir()
	path1 := filepath.Join(dir, "cpu1.prof")
	path2 := filepath.Join(dir, "cpu2.prof")

	profiler1, err := StartCpuProfile(path1)
	require.NoError(t, err)
	require.True(t, exists(path1))

	// Starting a second profiler should fail
	profiler2, err := StartCpuProfile(path2)
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
