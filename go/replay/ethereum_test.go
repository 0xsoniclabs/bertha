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

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/stretchr/testify/require"
)

func TestGetEthereumChainConfigAndUpgrades_MapsUpgradesInEffectAtBlock(t *testing.T) {
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
			gotCfg, upgrades := getEthereumChainConfigAndUpgrades(block, cfg.ChainID.Uint64())
			require.Same(t, cfg, gotCfg)
			require.Equal(t, tt.upgrades, upgrades)
		})
	}
}

func TestToEvmBlock_ConvertsBlockToEvmBlock(t *testing.T) {
	excessBlobGas := uint64(10_000_000)
	chainConfig := params.MainnetChainConfig
	cancun := *chainConfig.CancunTime
	cancunTime := inter.Timestamp(cancun * 1e9)
	blobBaseFee := eip4844.CalcBlobFee(chainConfig, &types.Header{ExcessBlobGas: &excessBlobGas, Time: cancun})
	cases := map[string]struct {
		difficulty      int64
		excessBlobGas   *uint64
		time            uint64
		wantTime        inter.Timestamp
		wantPrevRandao  common.Hash
		wantBlobBaseFee *big.Int
	}{
		"without excess blob gas": {
			time:            cancun,
			wantTime:        cancunTime,
			wantPrevRandao:  common.Hash{6},
			wantBlobBaseFee: big.NewInt(1),
		},
		"with excess blob gas": {
			excessBlobGas:   &excessBlobGas,
			time:            cancun,
			wantTime:        cancunTime,
			wantPrevRandao:  common.Hash{6},
			wantBlobBaseFee: blobBaseFee,
		},
		"with excess blob gas before cancun": {
			excessBlobGas:   &excessBlobGas,
			time:            cancun - 1,
			wantTime:        cancunTime - 1e9,
			wantPrevRandao:  common.Hash{6},
			wantBlobBaseFee: big.NewInt(1),
		},
		"before merge": {
			difficulty:      1,
			time:            5,
			wantTime:        5e9,
			wantBlobBaseFee: big.NewInt(1),
		},
	}
	for name, c := range cases {
		t.Run(name, func(t *testing.T) {
			txs := types.Transactions{types.NewTx(&types.LegacyTx{Nonce: 8})}
			block := types.NewBlockWithHeader(&types.Header{
				ParentHash:    common.Hash{1},
				Coinbase:      common.Address{2},
				Difficulty:    big.NewInt(c.difficulty),
				Number:        big.NewInt(20_000_000),
				GasLimit:      4,
				Time:          c.time,
				MixDigest:     common.Hash{6},
				BaseFee:       big.NewInt(7),
				ExcessBlobGas: c.excessBlobGas,
			}).WithBody(types.Body{Transactions: txs})

			require.Equal(t, &evmcore.EvmBlock{
				EvmHeader: evmcore.EvmHeader{
					Number:      big.NewInt(20_000_000),
					ParentHash:  common.Hash{1},
					Time:        c.wantTime,
					GasLimit:    4,
					PrevRandao:  c.wantPrevRandao,
					BaseFee:     big.NewInt(7),
					BlobBaseFee: c.wantBlobBaseFee,
					Coinbase:    common.Address{2},
				},
				Transactions: txs,
			}, toEvmBlock(block, chainConfig))
		})
	}
}
