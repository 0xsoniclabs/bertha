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

func TestRun_RunWithoutParameters_PrintsHelp(t *testing.T) {
	tmp := filepath.Join(t.TempDir(), "out.txt")

	backup := os.Stdout
	defer func() {
		os.Stdout = backup
	}()
	outFile, err := os.Create(tmp)
	require.NoError(t, err, "failed to create output file")
	defer func() {
		require.NoError(t, outFile.Close(), "failed to close output file")
	}()
	os.Stdout = outFile

	require.NoError(t, Run(nil))

	os.Stdout = backup
	content, err := os.ReadFile(tmp)
	require.NoError(t, err, "failed to read output file")
	require.Contains(t, string(content), "Block DB Tools")
	require.Contains(t, string(content), "USAGE")
}

func TestRun_RunWithProfileFlag_ProducesProfile(t *testing.T) {
	tmp := filepath.Join(t.TempDir(), "out.txt")
	require.False(t, exists(tmp))

	args := []string{"test", "--" + cpuProfileFlag.Name, tmp}
	_ = Run(args) // we ignore the error here
	require.True(t, exists(tmp), "expected profile file to exist")
}

func TestRun_FailedRun_ReportsIssue(t *testing.T) {
	require.ErrorContains(t,
		Run([]string{"test", "verify"}),
		"failed to open database",
	)
}
