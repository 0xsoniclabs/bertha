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
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/ethereum/go-ethereum/core/types"
)

// ethereumExecutor is the Ethereum implementation of BlockExecutor.
type ethereumExecutor struct {
	chainID     uint64
	interpreter tosca.Interpreter
}

func (e *ethereumExecutor) ExecuteBlock(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, uint64, error) {
	return e.applyBlock(state, block, hashHistory, false)
}

func (e *ethereumExecutor) ExecuteBlockOnArchive(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
	receipts, _, err := e.applyBlock(state, block, hashHistory, true)
	return receipts, err
}

// applyBlock applies the given block to state, see State.ApplyBlock.
func (e *ethereumExecutor) applyBlock(
	state *State,
	block *types.Block,
	hashHistory *blockHashHistory,
	isArchive bool,
) (types.Receipts, uint64, error) {
	chainConfig, rules := getEthereumChainConfigAndRules(block, e.chainID)

	processor := evmcore.NewStateProcessorForReplay(chainConfig, hashHistory, rules.Upgrades)

	return state.ApplyBlock(block, e.interpreter, processor, rules, nil, nil, chainConfig, nil, isArchive)
}
