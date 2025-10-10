package app

import (
	"errors"
	"log/slog"
	"os"
	"runtime/pprof"

	"github.com/urfave/cli/v3"
)

var (
	cpuProfileFlag = &cli.StringFlag{
		Name:  "cpu-profile",
		Usage: "write CPU profile to `file`",
	}
)

// StartCpuProfile starts CPU profiling and writes the profile to the specified
// path. If the path is empty, no profiling is started. If no file can be created,
// at the given path, an error is returned.
func StartCpuProfile(path string) (*profiler, error) {
	if path == "" {
		return nil, nil
	}
	out, err := os.Create(path)
	if err != nil {
		slog.Warn("Failed to create CPU profile file", "error", err)
		return nil, err
	}
	slog.Info("Starting CPU profile", "file", path)
	if err := pprof.StartCPUProfile(out); err != nil {
		return nil, errors.Join(err, os.Remove(path))
	}
	return &profiler{path: path}, nil
}

type profiler struct {
	path string
}

// Stop stops the CPU profiling and writes the profile to the file specified.
func (p *profiler) Stop() error {
	pprof.StopCPUProfile()
	slog.Info("CPU profile written", "file", p.path)
	return nil
}
