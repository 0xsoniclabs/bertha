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

package utils

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
