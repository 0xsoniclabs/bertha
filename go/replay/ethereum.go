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
	"math/big"

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
)

// ethereumChainConfigMap maps chain IDs of Ethereum networks to their corresponding chain configurations.
var ethereumChainConfigMap = map[uint64]*params.ChainConfig{
	params.MainnetChainConfig.ChainID.Uint64(): params.MainnetChainConfig,
	params.SepoliaChainConfig.ChainID.Uint64(): params.SepoliaChainConfig,
	params.HoleskyChainConfig.ChainID.Uint64(): params.HoleskyChainConfig,
	params.HoodiChainConfig.ChainID.Uint64():   params.HoodiChainConfig,
}

// isEthereum checks if the given chain ID corresponds to a known Ethereum chain.
func isEthereum(chainID uint64) bool {
	_, exists := ethereumChainConfigMap[chainID]
	return exists
}

// getEthereumChainConfigAndUpgrades returns the config of the given Ethereum
// chain and the upgrades in effect at the given block.
func getEthereumChainConfigAndUpgrades(block *types.Block, chainID uint64) (*params.ChainConfig, opera.Upgrades) {
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

// toEvmBlock converts the given block of an Ethereum chain to an EVM block that
// runs the block's transactions.
func toEvmBlock(block *types.Block, chainConfig *params.ChainConfig) *evmcore.EvmBlock {
	blobBaseFee := big.NewInt(1)
	if chainConfig.IsCancun(block.Number(), block.Time()) && block.ExcessBlobGas() != nil {
		blobBaseFee = eip4844.CalcBlobFee(chainConfig, block.Header())
	}
	prevRandao := block.MixDigest()
	if block.Difficulty().Sign() != 0 {
		// Before the Merge, PrevRandao is unused and set to zero, which tells the EVM
		// to use the difficulty instead.
		prevRandao = common.Hash{}
	}
	return &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  prevRandao,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: blobBaseFee,
			Coinbase:    block.Coinbase(),
		},
		Transactions: block.Transactions(),
	}
}
