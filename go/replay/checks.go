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

package replay

import (
	"bytes"
	"fmt"
	"slices"
	"strings"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
)

// checkBlockResults checks the results of applying a block against the
// expected values in the block, including receipt fields and the resulting
// state root. It is factored out to allow its use in both the simple replay
// loop and the pipeline version.
func checkBlockResults(
	chain Chain,
	block *blockdb.Block,
	receipts types.Receipts,
	stateRootFuture future.Future[result.Result[common.Hash]],
	hashOfParentBlock common.Hash,
	blockDB blockdb.BlockDB,
	replayLoopContext *ReplayLoopContext,
	logger utils.Logger,
) error {
	zone := tracy.ZoneBegin("CheckResults")
	defer zone.End()

	if !replayLoopContext.skipReceiptsCheck {
		if err := checkReceipts(block, receipts); err != nil {
			return err
		}
	}

	if err := checkStateRoot(chain, block, stateRootFuture, blockDB, replayLoopContext, logger); err != nil {
		return err
	}

	if err := checkParentHash(block, hashOfParentBlock, logger); err != nil {
		return err
	}
	return nil
}

// checkReceipts checks that the provided receipts match the expected receipts
// in the block.
func checkReceipts(block *blockdb.Block, receipts types.Receipts) error {
	if len(receipts) != len(block.Receipts) {
		return fmt.Errorf("number of receipts mismatch for block %d: expected %d, got %d",
			block.Number, len(block.Receipts), len(receipts))
	}
	for i, receipt := range receipts {
		want := block.Receipts[i]
		// check all fields which contribute to the block hash via the receipts root (are part of receiptRLP)
		if receipt.Status != want.GetStatus() {
			return fmt.Errorf("receipt status mismatch for block %d, tx %d: expected %d, got %d",
				block.Number, i, want.GetStatus(), receipt.Status)
		}
		if receipt.CumulativeGasUsed != want.CumulativeGasUsed {
			return fmt.Errorf("receipt cumulative gas used mismatch for block %d, tx %d: expected %d, got %d",
				block.Number, i, want.CumulativeGasUsed, receipt.CumulativeGasUsed)
		}
		if len(receipt.Logs) != len(want.Logs) {
			return fmt.Errorf("receipt logs length mismatch for block %d, tx %d: expected %d, got %d",
				block.Number, i, len(want.Logs), len(receipt.Logs))
		}
		for j, log := range receipt.Logs {
			wantLog := want.Logs[j]
			// check all fields which contribute to the receipts root
			if !slices.Equal(log.Address.Bytes(), wantLog.Address) {
				return fmt.Errorf("receipt log address mismatch for block %d, tx %d, log %d: expected %s, got %s",
					block.Number, i, j, hexutil.Encode(wantLog.Address), hexutil.Encode(log.Address.Bytes()))
			}
			if len(log.Topics) != len(wantLog.Topics) {
				return fmt.Errorf("receipt log topics length mismatch for block %d, tx %d, log %d: expected %d, got %d",
					block.Number, i, j, len(wantLog.Topics), len(log.Topics))
			}
			for k, topic := range log.Topics {
				if !slices.Equal(topic.Bytes(), wantLog.Topics[k]) {
					return fmt.Errorf("receipt log topic mismatch for block %d, tx %d, log %d, topic %d: expected %s, got %s",
						block.Number, i, j, k, hexutil.Encode(wantLog.Topics[k]), hexutil.Encode(topic.Bytes()))
				}
			}
			if !bytes.Equal(log.Data, wantLog.Data) {
				return fmt.Errorf("receipt log data mismatch for block %d, tx %d, log %d: expected %s, got %s",
					block.Number, i, j, hexutil.Encode(wantLog.Data), hexutil.Encode(log.Data))
			}
		}
		expectedBloom := convert.ToGethReceipt(want).Bloom
		if receipt.Bloom != expectedBloom {
			return fmt.Errorf("receipt bloom mismatch for block %d, tx %d: expected %s, got %s",
				block.Number, i, hexutil.Encode(expectedBloom[:]), hexutil.Encode(receipt.Bloom[:]))
		}
	}
	return nil
}

// checkStateRoot checks that the computed state root matches the expected
// state root in the block, and updates the block in the database if overwriting
// is enabled.
func checkStateRoot(
	chain Chain,
	block *blockdb.Block,
	stateRootFuture future.Future[result.Result[common.Hash]],
	blockDB blockdb.BlockDB,
	replayLoopContext *ReplayLoopContext,
	logger utils.Logger,
) error {
	overwriteStateRoot := &replayLoopContext.overwriteStateRoot
	noStateRootCheck := replayLoopContext.skipStateRootCheck

	computedStateRoot, err := stateRootFuture.Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root after applying block %d: %w", block.Number, err)
	}
	expectedStateRoot := getExpectedStateRoot(chain, block)

	if overwriteStateRoot.IsEnabled() {
		if !overwriteStateRoot.IsConfirmed() && expectedStateRoot != (common.Hash{}) && expectedStateRoot != computedStateRoot {
			logger.Warn("Block has existing state root", "block_number", block.Number, "existing", expectedStateRoot, "new", computedStateRoot)
			fmt.Printf("Are you sure you want to overwrite the existing state root (y/n)? ")
			var response string
			if _, err := fmt.Scanln(&response); err != nil {
				return fmt.Errorf("failed to read user input: %w", err)
			}
			if strings.ToLower(strings.TrimSpace(response)) != "y" {
				logger.Info("State roots overriding disabled from this point onward")
				overwriteStateRoot.Disable() //disabled by the user
			} else {
				logger.Info("Overriding state roots from this point onward")
				overwriteStateRoot.Confirm() //confirmed by the user
			}
		}

		// Double check in case user disabled the overwrite
		if overwriteStateRoot.IsEnabled() {
			updateStateRoot(chain, block, computedStateRoot)
			err = blockDB.Update(chain.ChainID(), block)
			if err != nil {
				return fmt.Errorf("failed to update block %d in database: %w", block.Number, err)
			}
		}
	}

	if !noStateRootCheck && !overwriteStateRoot.IsEnabled() {
		if expectedStateRoot == (common.Hash{}) {
			if !replayLoopContext.stateRootNotSet {
				logger.Warn("No state root set in the block DB. State root verification skipped", "block_number", block.Number)
				replayLoopContext.stateRootNotSet = true
			}
		} else if computedStateRoot != expectedStateRoot {
			return fmt.Errorf("state root mismatch after applying block %d: expected %x, got %x",
				block.Number, expectedStateRoot, computedStateRoot)
		}
	}
	return nil
}

// checkParentHash checks that the parent hash of the block matches the hash of
// the previous block in the chain.
func checkParentHash(block *blockdb.Block, hashOfParentBlock common.Hash, logger utils.Logger) error {
	parentHash := common.BytesToHash(block.ParentHash)
	if hashOfParentBlock == (common.Hash{}) {
		logger.Warn("No block hash set. Parent hash verification skipped", "block_number", block.Number)
	} else if parentHash != hashOfParentBlock {
		return fmt.Errorf("parent hash mismatch: hash of block %d is %x, parent hash of block %d is %x",
			block.Number-1, hashOfParentBlock, block.Number, parentHash)
	}
	return nil
}
