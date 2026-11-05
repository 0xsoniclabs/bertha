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
	"fmt"
	"math/big"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestIsEthereum_IdentifiesEthereumChains(t *testing.T) {
	tests := map[string]struct {
		chainID uint64
		want    bool
	}{
		"Mainnet":       {chainID: 1, want: true},
		"Sepolia":       {chainID: 11155111, want: true},
		"Hoodi":         {chainID: 560048, want: true},
		"Sonic":         {chainID: 146},
		"Sonic testnet": {chainID: 14601},
		"Unknown chain": {chainID: 9999},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			require.Equal(t, tt.want, IsEthereum(tt.chainID))
		})
	}
}

func TestGetChainConfigAndUpgrades_MapsUpgradesInEffectAtBlock(t *testing.T) {
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
			gotCfg, upgrades := getChainConfigAndUpgrades(block, cfg.ChainID.Uint64())
			require.Same(t, cfg, gotCfg)
			require.Equal(t, tt.upgrades, upgrades)
		})
	}
}

func TestEndBeforeUnsupportedFork_EndsEthereumChainsBeforeTheFirstUnsupportedBlock(t *testing.T) {
	amsterdamTime := *params.SepoliaChainConfig.AmsterdamTime
	blocks := func(yield func(*blockdb.Block, error) bool) {
		for i, timestamp := range []uint64{amsterdamTime - 2, amsterdamTime - 1, amsterdamTime, amsterdamTime + 1} {
			if !yield(&blockdb.Block{Number: 20_000_000 + uint64(i), Timestamp: timestamp}, nil) {
				return
			}
		}
	}

	logger := utils.NewMockLogger(gomock.NewController(t))
	logger.EXPECT().Warn("Ending replay before the first block of an unsupported fork", "fork", "Amsterdam", "block", uint64(20_000_002), "timestamp", amsterdamTime)

	var got []uint64
	for block, err := range EndBeforeUnsupportedFork(blocks, params.SepoliaChainConfig.ChainID.Uint64(), logger) {
		require.NoError(t, err)
		got = append(got, block.Number)
	}
	require.Equal(t, []uint64{20_000_000, 20_000_001}, got)
}

func TestEndBeforeUnsupportedFork_ForwardsErrors(t *testing.T) {
	injected := fmt.Errorf("injected")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injected)
	}

	var errs []error
	for _, err := range EndBeforeUnsupportedFork(blocks, params.SepoliaChainConfig.ChainID.Uint64(), utils.NewMockLogger(gomock.NewController(t))) {
		errs = append(errs, err)
	}
	require.Equal(t, []error{injected}, errs)
}

func TestUnsupportedFork_NamesTheLatestUnsupportedFork(t *testing.T) {
	const ubtTime = uint64(2_000_000_000)
	chainConfig := *params.MainnetChainConfig
	chainConfig.UBTTime = new(ubtTime)
	chainConfig.AmsterdamTime = new(ubtTime + 1)
	chainConfig.BogotaTime = new(ubtTime + 2)

	tests := map[string]struct {
		time uint64
		want string
	}{
		"before UBT": {time: ubtTime - 1, want: ""},
		"UBT":        {time: ubtTime, want: "UBT"},
		"Amsterdam":  {time: ubtTime + 1, want: "Amsterdam"},
		"Bogota":     {time: ubtTime + 2, want: "Bogota"},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			require.Equal(t, tt.want, unsupportedFork(&chainConfig, big.NewInt(20_000_000), tt.time))
		})
	}
}
