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

package replay

import (
	"fmt"
	"path/filepath"
	"time"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/ethereum/go-ethereum/core/types"
)

// progressLogger is a UX helper utility for the replay command producing
// the main progress log output.
type progressLogger struct {
	logger                 utils.Logger
	runWithState           func(func(*State) error) error
	stateDBDirectory       string
	start                  time.Time
	lastUpdate             time.Time
	lastReportedBlockTime  time.Time
	lastProcessedBlockTime time.Time
	txCounter              uint64
	gasCounter             uint64
	lastTxCounter          uint64
	lastGasCounter         uint64
	firstBlockTime         time.Time
	logDBSize              bool
}

func startProgressLogger(logger utils.Logger, runWithState func(func(*State) error) error, stateDBDirectory string, logDBSize bool) *progressLogger {
	now := time.Now()
	return &progressLogger{
		logger:           logger,
		runWithState:     runWithState,
		stateDBDirectory: stateDBDirectory,
		start:            now,
		lastUpdate:       now,
		logDBSize:        logDBSize,
	}
}

func (p *progressLogger) LogProgress(block *types.Block) error {
	// Keep track of metrics for logging purposes.
	p.lastProcessedBlockTime = time.Unix(int64(block.Time()), 0).UTC()
	p.txCounter += uint64(len(block.Transactions()))
	p.gasCounter += block.GasUsed()

	number := block.NumberU64()
	if number == 0 {
		p.firstBlockTime = p.lastProcessedBlockTime
		p.lastReportedBlockTime = p.lastProcessedBlockTime
		return nil
	}

	// Periodically log the progress of the replay.
	if number%10_000 != 0 {
		return nil
	}

	currentBlockTime := time.Unix(int64(block.Time()), 0).UTC()
	deltaBlockTime := currentBlockTime.Sub(p.lastReportedBlockTime)
	p.lastReportedBlockTime = currentBlockTime
	deltaTx := p.txCounter - p.lastTxCounter
	deltaGas := p.gasCounter - p.lastGasCounter
	p.lastTxCounter = p.txCounter
	p.lastGasCounter = p.gasCounter

	now := time.Now()
	deltaTime := now.Sub(p.lastUpdate)
	p.lastUpdate = now

	runtime := time.Since(p.start)

	args := []any{
		"block", number,
		"block_time", currentBlockTime.Format(time.RFC3339),
		"elapsed", fmt.Sprintf("%02d:%02d:%02d", int(runtime.Hours()), int(runtime.Minutes())%60, int(runtime.Seconds())%60),
		"txs/s", int(float64(deltaTx) / deltaTime.Seconds()),
		"MGas/s", int(float64(deltaGas) / deltaTime.Seconds() / 1000 / 1000),
		"realtime", int(deltaBlockTime.Seconds() / deltaTime.Seconds()),
	}

	// Optionally log the size of the state database.
	if p.logDBSize {
		err := p.runWithState(func(state *State) error {
			err := state.db.Flush()
			if err != nil {
				return fmt.Errorf("failed to flush state database: %w", err)
			}
			liveSize, err := utils.DirSize(filepath.Join(p.stateDBDirectory, "live"))
			if err != nil {
				return fmt.Errorf("failed to compute live database size: %w", err)
			}

			args = append(args, "LiveDB size", fmt.Sprintf("%.3fGiB", float64(liveSize)/1024/1024/1024))

			archiveDir := filepath.Join(p.stateDBDirectory, "archive")
			archiveMissing, err := utils.IsEmptyOrMissingDir(archiveDir)
			if err != nil {
				return fmt.Errorf("failed to check existence of archive database directory: %w", err)
			}
			if !archiveMissing {
				archiveSize, err := utils.DirSize(archiveDir)
				if err != nil {
					return fmt.Errorf("failed to compute archive database size: %w", err)
				}
				args = append(args, "ArchiveDB size", fmt.Sprintf("%.3fGiB", float64(archiveSize)/1024/1024/1024))
			}
			return nil
		})
		if err != nil {
			return err
		}
	}

	p.logger.Info("Processing block", args...)
	return nil
}

func (p *progressLogger) LogSummary() {
	duration := time.Since(p.start)
	deltaBlockTime := p.lastProcessedBlockTime.Sub(p.firstBlockTime)
	p.logger.Info(
		"Replay finished",
		"elapsed", fmt.Sprintf("%02d:%02d:%02d", int(duration.Hours()), int(duration.Minutes())%60, int(duration.Seconds())%60),
		"txs", p.txCounter,
		"TGas", float64(p.gasCounter)/1e12,
		"txs/s", int(float64(p.txCounter)/duration.Seconds()),
		"MGas/s", int(float64(p.gasCounter)/duration.Seconds()/1e6),
		"realtime", int(deltaBlockTime.Seconds()/duration.Seconds()),
	)
}
