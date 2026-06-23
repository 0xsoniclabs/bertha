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
	"errors"
	"log/slog"
	"os"
	"runtime/pprof"
)

// StartCPUProfile starts CPU profiling and writes the profile to the specified
// path. If the path is empty, no profiling is started. If no file can be created,
// at the given path, an error is returned.
func StartCPUProfile(path string) (*profiler, error) {
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
		return nil, errors.Join(err, out.Close(), os.Remove(path))
	}
	return &profiler{path: path, out: out}, nil
}

type profiler struct {
	path string
	out  *os.File
}

// Stop stops the CPU profiling and writes the profile to the file specified.
func (p *profiler) Stop() error {
	pprof.StopCPUProfile()
	if err := p.out.Close(); err != nil {
		return err
	}
	slog.Info("CPU profile written", "file", p.path)
	return nil
}
