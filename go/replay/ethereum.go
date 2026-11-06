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

	"github.com/ethereum/go-ethereum/params"
)

// ethereumChainConfigMap maps chain IDs of Ethereum networks to their corresponding chain configurations.
var ethereumChainConfigMap = map[uint64]*params.ChainConfig{
	params.MainnetChainConfig.ChainID.Uint64(): params.MainnetChainConfig,
	params.SepoliaChainConfig.ChainID.Uint64(): params.SepoliaChainConfig,
	params.HoodiChainConfig.ChainID.Uint64():   params.HoodiChainConfig,
}

// isEthereum checks if the given chain ID corresponds to a known Ethereum chain.
func isEthereum(chainID uint64) bool {
	_, exists := ethereumChainConfigMap[chainID]
	return exists
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
