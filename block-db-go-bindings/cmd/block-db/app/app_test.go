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
	require.Contains(t, string(content), "Block Database CLI")
	require.Contains(t, string(content), "USAGE")
}

func TestRun_RunWithoutProfileFlag_ProducesProfile(t *testing.T) {
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
