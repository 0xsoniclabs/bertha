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

	"github.com/0xsoniclabs/bertha/replay"
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
	tests := map[string]struct {
		command string
	}{
		"verify":         {command: "verify"},
		"check blob txs": {command: "check-blob-txs"},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			require.ErrorContains(t,
				Run([]string{"test", tt.command}),
				"failed to open database",
			)
		})
	}
}

func TestParseOverwriteStateRootPolicy_AcceptsOnlyValidValues(t *testing.T) {
	cases := map[string]struct {
		input   string
		want    replay.OverwriteStateRootPolicy
		wantErr bool
	}{
		"Empty":              {input: "", want: replay.OverwriteStateRootPolicyOff},
		"Off":                {input: "off", want: replay.OverwriteStateRootPolicyOff},
		"False":              {input: "false", want: replay.OverwriteStateRootPolicyOff},
		"OffUpperCase":       {input: "OFF", want: replay.OverwriteStateRootPolicyOff},
		"OffWithWhitespace":  {input: "  off  ", want: replay.OverwriteStateRootPolicyOff},
		"Uninitialized":      {input: "uninitialized", want: replay.OverwriteStateRootPolicyUninitialized},
		"UninitializedMixed": {input: "Uninitialized", want: replay.OverwriteStateRootPolicyUninitialized},
		"On":                 {input: "on", want: replay.OverwriteStateRootPolicyOn},
		"True":               {input: "true", want: replay.OverwriteStateRootPolicyOn},
		"OnUpperCase":        {input: "ON", want: replay.OverwriteStateRootPolicyOn},
		"Garbage":            {input: "maybe", wantErr: true},
		"AbbreviatedYes":     {input: "y", wantErr: true},
		"AbbreviatedNo":      {input: "n", wantErr: true},
		"NumericTrue":        {input: "1", wantErr: true},
		"NumericFalse":       {input: "0", wantErr: true},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			got, err := parseOverwriteStateRootPolicy(tc.input)
			if tc.wantErr {
				require.ErrorContains(t, err, "`off`, `uninitialized`, `on`")
				return
			}
			require.NoError(t, err)
			require.Equal(t, tc.want, got)
		})
	}
}
