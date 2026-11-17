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
	tests := map[string]struct {
		dir  func(*testing.T) string
		want bool
	}{
		"missing directory": {
			dir:  func(t *testing.T) string { return filepath.Join(t.TempDir(), "missing") },
			want: true,
		},
		"empty directory": {
			dir:  func(t *testing.T) string { return t.TempDir() },
			want: true,
		},
		"non-empty directory": {
			dir: func(t *testing.T) string {
				dir := t.TempDir()
				require.NoError(t, os.WriteFile(filepath.Join(dir, "file.txt"), []byte("data"), 0644))
				return dir
			},
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			isEmpty, err := IsEmptyOrMissingDir(tt.dir(t))
			require.NoError(t, err)
			require.Equal(t, tt.want, isEmpty)
		})
	}
}

func TestDirSize_ComputesSizeRecursively(t *testing.T) {
	tests := map[string]struct {
		files []string
		want  int64
	}{
		"empty directory": {want: 0},
		"nested files":    {files: []string{"file1.txt", filepath.Join("subdir", "file2.txt")}, want: 2 * 13},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			for _, file := range tt.files {
				path := filepath.Join(dir, file)
				require.NoError(t, os.MkdirAll(filepath.Dir(path), 0755))
				require.NoError(t, os.WriteFile(path, []byte("Hello, World!"), 0644))
			}

			size, err := DirSize(dir)
			require.NoError(t, err)
			require.Equal(t, tt.want, size)
		})
	}
}
