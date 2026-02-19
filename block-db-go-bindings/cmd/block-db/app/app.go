// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

package app

import (
	"context"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

	"github.com/0xsoniclabs/tracy"
	"github.com/urfave/cli/v3"
)

func Run(args []string) error {
	return getApp().Run(context.Background(), args)
}

func getApp() *cli.Command {
	var tracyEnabled bool
	var profiler *profiler
	var diagnostic *diagnostic
	return &cli.Command{
		Name:  "block-db",
		Usage: "Block Database CLI",
		Commands: []*cli.Command{
			getReplayCommand(),
			getVerifyCommand(),
		},
		Flags: []cli.Flag{
			cpuProfileFlag,
			diagnosticsFlag,
			diagnosticsPortFlag,
		},
		Before: func(ctx context.Context, cmd *cli.Command) (context.Context, error) {
			tracy.StartupProfiler()
			tracyEnabled = true
			if cmd.Bool(diagnosticsFlag.Name) {
				diagnostic = StartDiagnostics(slog.Default(), cmd.Uint16(diagnosticsPortFlag.Name))
			}
			var err error
			profiler, err = StartCpuProfile(cmd.String(cpuProfileFlag.Name))
			if err != nil {
				return ctx, err
			}
			ctx, cancel := context.WithCancel(ctx)
			go func() {
				sigs := make(chan os.Signal, 1)
				signal.Notify(sigs, syscall.SIGINT, syscall.SIGTERM)
				select {
				case <-ctx.Done():
					return
				case <-sigs:
					slog.Warn("Received interrupt signal")
					cancel()
				}
			}()
			return ctx, nil
		},
		After: func(_ context.Context, cmd *cli.Command) error {
			if profiler != nil {
				return profiler.Stop()
			}
			if diagnostic != nil {
				return diagnostic.Stop()
			}
			if tracyEnabled {
				tracy.ShutdownProfiler()
			}
			return nil
		},
	}
}
