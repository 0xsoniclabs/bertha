package app

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/require"
)

func TestIsEmptyOrMissingDir_DetectsMissingAndEmptyDirs(t *testing.T) {
	missingDir := filepath.Join(t.TempDir(), "missing")
	emptyDir := t.TempDir()
	nonEmptyDir := t.TempDir()

	filePath := filepath.Join(nonEmptyDir, "file.txt")
	err := os.WriteFile(filePath, []byte("data"), 0644)
	require.NoError(t, err)

	isEmpty, err := IsEmptyOrMissingDir(missingDir)
	require.NoError(t, err)
	require.True(t, isEmpty, "Expected missing directory to be considered empty")

	isEmpty, err = IsEmptyOrMissingDir(emptyDir)
	require.NoError(t, err)
	require.True(t, isEmpty, "Expected empty directory to be considered empty")

	isEmpty, err = IsEmptyOrMissingDir(nonEmptyDir)
	require.NoError(t, err)
	require.False(t, isEmpty, "Expected non-empty directory to not be considered empty")
}

func TestDirSize_ComputesSizeRecursively(t *testing.T) {
	dir := t.TempDir()

	size, err := DirSize(dir)
	require.NoError(t, err)
	require.Equal(t, int64(0), size)

	file1Path := filepath.Join(dir, "file1.txt")
	err = os.WriteFile(file1Path, []byte("Hello, World!"), 0644)
	require.NoError(t, err)
	subDir := filepath.Join(dir, "subdir")
	err = os.Mkdir(subDir, 0755)
	require.NoError(t, err)
	file2Path := filepath.Join(subDir, "file2.txt")
	err = os.WriteFile(file2Path, []byte("Hello, World!"), 0644)
	require.NoError(t, err)

	size, err = DirSize(dir)
	require.NoError(t, err)
	require.Equal(t, int64(26), size)
}
