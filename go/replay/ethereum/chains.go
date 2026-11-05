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

package ethereum

import (
	"iter"
	"math/big"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
)

// ethereumChainConfigMap maps chain IDs of Ethereum networks to their corresponding chain configurations.
var ethereumChainConfigMap = map[uint64]*params.ChainConfig{
	params.MainnetChainConfig.ChainID.Uint64(): params.MainnetChainConfig,
	params.SepoliaChainConfig.ChainID.Uint64(): params.SepoliaChainConfig,
	params.HoodiChainConfig.ChainID.Uint64():   params.HoodiChainConfig,
}

// IsEthereum checks if the given chain ID corresponds to a known Ethereum chain.
func IsEthereum(chainID uint64) bool {
	_, exists := ethereumChainConfigMap[chainID]
	return exists
}

// getChainConfigAndUpgrades returns the config of the given Ethereum chain and
// the upgrades in effect at the given block.
func getChainConfigAndUpgrades(block *types.Block, chainID uint64) (*params.ChainConfig, opera.Upgrades) {
	cfg := ethereumChainConfigMap[chainID]
	rules := cfg.Rules(block.Number(), true, block.Time())
	return cfg, opera.Upgrades{
		Berlin: rules.IsBerlin,
		London: rules.IsLondon,
		Llr:    false,

		Sonic:   rules.IsCancun,
		Allegro: rules.IsPrague,
		Brio:    rules.IsOsaka,

		SingleProposerBlockFormation: false,
		GasSubsidies:                 false,
		TransactionBundles:           false,
	}
}

// EndBeforeUnsupportedFork ends the given blocks of an Ethereum chain before the
// first block of a fork that is not supported yet, so that the replay of the
// blocks before it finishes successfully.
func EndBeforeUnsupportedFork(blocks iter.Seq2[*blockdb.Block, error], chainID uint64, logger utils.Logger) iter.Seq2[*blockdb.Block, error] {
	chainConfig := ethereumChainConfigMap[chainID]
	return func(yield func(*blockdb.Block, error) bool) {
		for block, err := range blocks {
			if err == nil {
				if fork := unsupportedFork(chainConfig, new(big.Int).SetUint64(block.Number), block.Timestamp); fork != "" {
					logger.Warn("Ending replay before the first block of an unsupported fork", "fork", fork, "block", block.Number, "timestamp", block.Timestamp)
					return
				}
			}
			if !yield(block, err) {
				return
			}
		}
	}
}

// unsupportedFork returns the name of the latest fork active at a block with the
// given number and time that the Sonic technology stack does not support yet
// (Amsterdam, Bogota or UBT), or an empty string if there is none.
// TODO: remove forks from this list once they are supported.
func unsupportedFork(chainConfig *params.ChainConfig, number *big.Int, time uint64) string {
	switch {
	case chainConfig.IsBogota(number, time):
		return "Bogota"
	case chainConfig.IsAmsterdam(number, time):
		return "Amsterdam"
	case chainConfig.IsUBT(number, time):
		return "UBT"
	}
	return ""
}
