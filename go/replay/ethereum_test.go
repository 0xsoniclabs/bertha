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
	"testing"

	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/stretchr/testify/require"
)

func TestGetEthereumChainConfigAndRules_MapsUpgradesInEffectAtBlock(t *testing.T) {
	cfg := params.MainnetChainConfig
	tests := map[string]struct {
		number   uint64
		time     uint64
		upgrades opera.Upgrades
	}{
		"BeforeBerlin": {number: 12_243_999},
		"Berlin":       {number: 12_244_000, upgrades: opera.Upgrades{Berlin: true}},
		"London":       {number: 12_965_000, upgrades: opera.Upgrades{Berlin: true, London: true}},
		"Cancun":       {number: 20_000_000, time: *cfg.CancunTime, upgrades: opera.Upgrades{Berlin: true, London: true, Sonic: true}},
		"Prague":       {number: 22_000_000, time: *cfg.PragueTime, upgrades: opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true}},
		"Osaka":        {number: 24_000_000, time: *cfg.OsakaTime, upgrades: opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true, Brio: true}},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			block := types.NewBlockWithHeader(&types.Header{Number: new(big.Int).SetUint64(tt.number), Time: tt.time})
			gotCfg, rules := getEthereumChainConfigAndRules(block, cfg.ChainID.Uint64())
			require.Same(t, cfg, gotCfg)
			require.Equal(t, opera.Rules{Upgrades: tt.upgrades}, rules)
		})
	}
}
