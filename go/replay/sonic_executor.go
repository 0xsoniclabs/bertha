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
	"fmt"

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
)

// sonicExecutor is the Sonic implementation of BlockExecutor.
type sonicExecutor struct {
	chainID       uint64
	metadataStore MetadataStore
	interpreter   tosca.Interpreter
}

func (e *sonicExecutor) ExecuteBlock(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, *types.Header, error) {
	number := block.NumberU64()

	// Commit all pending rules updates if the block is an epoch sealing block.
	if block.Transactions().Len() > 0 {
		if _, err := drivercall.ParseSealEpochArgs(block.Transactions()[0]); err == nil {
			if err := e.metadataStore.CommitRules(number); err != nil {
				return nil, nil, fmt.Errorf("failed to commit rules at block %d: %w", number, err)
			}
		}
	}

	onLog := func(l *core_types.Log) { onNewLog(e.metadataStore, number, l) }

	receipts, usedGas, err := e.applyBlock(state, block, hashHistory, onLog, false)
	if err != nil {
		return nil, nil, err
	}

	// Reconstruct the complete header by filling in fields that are derived
	// from processing the block, which are not stored in the block DB. The gas
	// used includes failed bundles, which have no receipts.
	completeHeader := block.Header()
	completeHeader.GasUsed = usedGas
	completeHeader.Bloom = types.MergeBloom(receipts)
	completeHeader.ReceiptHash = types.DeriveSha(receipts, trie.NewStackTrie(nil))
	return receipts, completeHeader, nil
}

func (e *sonicExecutor) ExecuteBlockOnArchive(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
	receipts, _, err := e.applyBlock(state, block, hashHistory, nil, true)
	return receipts, err
}

// applyBlock applies the given block with the metadata of the block to state,
// see State.ApplyBlock.
func (e *sonicExecutor) applyBlock(
	state *State,
	block *types.Block,
	hashHistory *blockHashHistory,
	onLog func(*core_types.Log),
	isArchive bool,
) (types.Receipts, uint64, error) {
	number := block.NumberU64()
	chainConfig := opera.CreateTransientEvmChainConfig(e.chainID, e.metadataStore.GetUpgradeHeights(), idx.Block(number))
	rules := e.metadataStore.GetRulesAtBlock(number)

	processor := evmcore.NewStateProcessorForReplay(chainConfig, hashHistory, rules.Upgrades)

	corrections := e.metadataStore.GetCorrectionsAtBlock(number)
	failedBundles := e.metadataStore.GetFailedBundlesAtBlock(number)

	return state.ApplyBlock(
		block, e.interpreter, processor, rules, corrections, failedBundles, chainConfig, onLog, isArchive,
	)
}
