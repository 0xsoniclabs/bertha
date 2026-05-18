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

// Package app provides the main application logic for the block database CLI.
package app

import (
	"context"
	"log/slog"
	"maps"
	"math"
	"os"
	"os/signal"
	"slices"
	"strings"
	"syscall"

	"github.com/0xsoniclabs/bertha/replay"
	"github.com/0xsoniclabs/bertha/verify"
	carmen "github.com/0xsoniclabs/carmen/go/state"
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
		Name:  "bertha",
		Usage: "Block DB Tools",
		Commands: []*cli.Command{
			{
				Name:   "replay",
				Usage:  "replay the full block chain from the block database",
				Action: parseReplayArgsAndRunReplay,
				Flags: []cli.Flag{
					jsonGenesisFlag,
					blockDatabaseDirectoryFlag,
					stateDBDirectoryFlag,
					initDBFlag,
					keepDBFlag,
					withArchiveFlag,
					dbSchema,
					dbVariant,
					startBlockFlag,
					endBlockFlag,
					usePipelineFlag,
					snapshotInterval,
					snapshotStartBlock,
					snapshotEndBlock,
					snapshotNumToKeep,
					overwriteStateRoot,
					noStateRootCheck,
					logDBSize,
					confirmAllPromptsFlag,
				},
			},
			{
				Name:   "verify",
				Usage:  "Verify the block database",
				Action: parseVerifyArgsAndRunVerify,
				Flags: []cli.Flag{
					blockDatabaseDirectoryFlag,
					chainIDFlag,
					startBlockFlag,
					endBlockFlag,
				},
			},
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
			profiler, err = StartCPUProfile(cmd.String(cpuProfileFlag.Name))
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
				if err := profiler.Stop(); err != nil {
					return err
				}
			}
			if diagnostic != nil {
				if err := diagnostic.Stop(); err != nil {
					return err
				}
			}
			if tracyEnabled {
				tracy.ShutdownProfiler()
			}
			return nil
		},
	}
}

var (
	cpuProfileFlag = &cli.StringFlag{
		Name:  "cpu-profile",
		Usage: "write CPU profile to `file`",
	}
	diagnosticsFlag = &cli.BoolFlag{
		Name:  "diagnostics",
		Usage: "enable diagnostics server (pprof)",
		Value: false,
	}
	diagnosticsPortFlag = &cli.Uint16Flag{
		Name:  "diagnostics-port",
		Usage: "port for diagnostics server (pprof)",
		Value: 6060,
	}

	blockDatabaseDirectoryFlag = &cli.StringFlag{
		Name:    "database-dir",
		Aliases: []string{"db"},
		Usage:   "Path to the block database directory",
		Value:   "./.blockdb",
	}

	chainIDFlag = &cli.Uint64Flag{
		Name:    "chain-id",
		Aliases: []string{"c"},
		Usage:   "Chain ID to verify",
		Value:   146, // Default to Sonic mainnet chain ID
	}

	startBlockFlag = &cli.Uint64Flag{
		Name:    "start-block",
		Aliases: []string{"s"},
		Usage:   "Starting block number",
		Value:   0,
	}
	endBlockFlag = &cli.Uint64Flag{
		Name:    "end-block",
		Aliases: []string{"e"},
		Usage:   "Ending block number (inclusive)",
		Value:   math.MaxUint64, // Default to the maximum block number
	}

	jsonGenesisFlag = &cli.StringFlag{
		Name:    "json-genesis",
		Aliases: []string{"g"},
		Usage:   "JSON encoded genesis data to use for replaying the blockchain",
	}
	stateDBDirectoryFlag = &cli.StringFlag{
		Name:    "state-db-dir",
		Aliases: []string{"sdb"},
		Usage:   "Path to the state database directory (default: OS-defined temporary directory)",
		Value:   "",
	}
	initDBFlag = &cli.StringFlag{
		Name:  "init-db-dir",
		Usage: "Path to a state database directory to use to init the state database. The database will be copied to a temporary folder or the directory specified by '--state-db-dir' before replaying.",
		Value: "",
	}
	keepDBFlag = &cli.BoolFlag{
		Name:  "keep-db",
		Usage: "Keep the state database after running the replay",
	}

	withArchiveFlag = &cli.BoolFlag{
		Name:    "with-archive",
		Aliases: []string{"a"},
		Usage:   "Use the archive mode for the state database",
		Value:   false,
	}
	dbSchema = &cli.IntFlag{
		Name:    "db-schema",
		Aliases: []string{"schema"},
		Usage:   "Block database schema version to use",
		Value:   5,
	}
	dbVariant = &cli.StringFlag{
		Name:    "db-variant",
		Aliases: []string{"variant"},
		Usage:   "Block database variant to use (" + strings.Join(getListOfCarmenVariants(), ", ") + ")",
		Value:   "go-file",
	}
	usePipelineFlag = &cli.BoolFlag{
		Name:  "use-pipeline",
		Usage: "Enable the replay pipeline (default: true)",
		Value: true,
	}

	snapshotInterval = &cli.Uint64Flag{
		Name:    "snapshot-interval",
		Aliases: []string{"si"},
		Usage:   "Interval of blocks at which to perform database snapshots (0 = disabled)",
		Value:   0,
	}
	snapshotStartBlock = &cli.Uint64Flag{
		Name:  "snapshot-start-block",
		Usage: "Block number from which to start taking snapshots (default: 0)",
		Value: 0,
	}
	snapshotEndBlock = &cli.Uint64Flag{
		Name:  "snapshot-end-block",
		Usage: "Block number at which to stop taking snapshots (default: max block)",
		Value: math.MaxUint64,
	}
	snapshotNumToKeep = &cli.Uint64Flag{
		Name:  "snapshot-num-to-keep",
		Usage: "Number of snapshots to keep (default: 1)",
		Value: 1,
	}

	overwriteStateRoot = &cli.BoolFlag{
		Name:  "overwrite-state-roots",
		Usage: "Overwrite the state roots in the block database with the ones computed from the state",
		Value: false,
	}
	noStateRootCheck = &cli.BoolFlag{
		Name:    "no-state-root-check",
		Aliases: []string{"no-src"},
		Usage:   "Skip checking the state roots with the ones stored in the block database",
		Value:   false,
	}

	logDBSize = &cli.BoolFlag{
		Name:    "log-db-size",
		Aliases: []string{"lds"},
		Usage:   "Include the disk size of the database in progress log messages (default = disabled)",
		Value:   false,
	}

	confirmAllPromptsFlag = &cli.BoolFlag{
		Name:  "y",
		Usage: "Automatically confirm all prompts",
		Value: false,
	}
)

// getListOfCarmenVariants returns a sorted list of all registered database variants.
func getListOfCarmenVariants() []string {
	variants := map[string]struct{}{}
	for config := range carmen.GetAllRegisteredStateFactories() {
		variants[string(config.Variant)] = struct{}{}
	}
	return slices.Sorted(maps.Keys(variants))
}

func parseReplayArgsAndRunReplay(ctx context.Context, c *cli.Command) error {
	args := replay.ReplayArgs{
		JSONGenesisFile:    c.String(jsonGenesisFlag.Name),
		BlockDBDir:         c.String(blockDatabaseDirectoryFlag.Name),
		StateDBDir:         c.String(stateDBDirectoryFlag.Name),
		InitDBDir:          c.String(initDBFlag.Name),
		KeepDB:             c.Bool(keepDBFlag.Name),
		WithArchive:        c.Bool(withArchiveFlag.Name),
		DBSchema:           carmen.Schema(c.Int(dbSchema.Name)),
		DBVariant:          carmen.Variant(c.String(dbVariant.Name)),
		UsePipeline:        c.Bool(usePipelineFlag.Name),
		StartBlock:         c.Uint64(startBlockFlag.Name),
		EndBlock:           c.Uint64(endBlockFlag.Name),
		SnapshotInterval:   c.Uint64(snapshotInterval.Name),
		SnapshotStartBlock: c.Uint64(snapshotStartBlock.Name),
		SnapshotEndBlock:   c.Uint64(snapshotEndBlock.Name),
		SnapshotNumToKeep:  c.Uint64(snapshotNumToKeep.Name),
		OverwriteStateRoot: c.Bool(overwriteStateRoot.Name),
		NoStateRootCheck:   c.Bool(noStateRootCheck.Name),
		LogDBSize:          c.Bool(logDBSize.Name),
		ConfirmAllPrompts:  c.Bool(confirmAllPromptsFlag.Name),
	}
	return replay.Replay(ctx, args)
}

func parseVerifyArgsAndRunVerify(ctx context.Context, c *cli.Command) error {
	args := verify.VerifyArgs{
		DatabaseDir: c.String(blockDatabaseDirectoryFlag.Name),
		ChainID:     c.Uint64(chainIDFlag.Name),
		StartBlock:  c.Uint64(startBlockFlag.Name),
		EndBlock:    c.Uint64(endBlockFlag.Name),
	}
	return verify.Verify(ctx, args)
}
