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
	"log/slog"
	"testing"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

func TestStaticMetadataStore_GetRulesAtBlock_ObtainsUpgradesBasedOnBlockNumber(t *testing.T) {
	upgrades := []opera.Upgrades{
		{Sonic: true},
		{Sonic: true, Allegro: true},
		{Sonic: true, SingleProposerBlockFormation: true},
	}

	store := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				{Upgrades: upgrades[0], Height: 5},
				{Upgrades: upgrades[1], Height: 7},
				{Upgrades: upgrades[2], Height: 11},
			},
		},
	}

	for blockNr := range 20 {
		expect := opera.Rules{}
		if blockNr >= 11 {
			expect.Upgrades = upgrades[2]
		} else if blockNr >= 7 {
			expect.Upgrades = upgrades[1]
		} else if blockNr >= 5 {
			expect.Upgrades = upgrades[0]
		}

		rules := store.GetRulesAtBlock(uint64(blockNr))
		require.Equal(t, expect, rules, "block number %d", blockNr)
	}
}

func TestNewStaticMetadataStore_SonicChain_ContainsCorrections(t *testing.T) {
	store, err := NewStaticMetadataStore(SonicMainNetChainID)
	require.NoError(t, err)
	require.NotEmpty(t, store.metadata.Corrections)
}

func TestNewStaticMetadataStore_AllegroTestChain_NoCorrectionsButUpgrades(t *testing.T) {
	store, err := NewStaticMetadataStore(AllegroTestNetChainID)
	require.NoError(t, err)
	require.NotEmpty(t, store.GetUpgrades())
	require.Empty(t, store.metadata.Corrections)

	// Make sure the upgrades are in ascending order.
	last := 0
	for _, upgrade := range store.GetUpgrades() {
		require.Greater(t, int(upgrade.Height), last)
		last = int(upgrade.Height)
	}
}

func TestNewStaticMetadataStore_UnknownChainID_LogsWarningAndReturnsEmptyMetadata(t *testing.T) {
	handler := &utils.CapturingLogHandler{}
	old := slog.Default()
	slog.SetDefault(slog.New(handler))
	t.Cleanup(func() { slog.SetDefault(old) })

	store, err := NewStaticMetadataStore(0)
	require.NoError(t, err)
	require.Empty(t, store.metadata.Upgrades)
	require.Empty(t, store.metadata.Corrections)

	records := handler.Records()
	require.Len(t, records, 1)
	require.Equal(t, slog.LevelWarn, records[0].Level)
	require.Equal(t, "no metadata available for chain ID, proceeding without upgrades or corrections", records[0].Message)
}

func TestStaticMetadataStore_StoreUpgrade_VerifiesUpgradeExists(t *testing.T) {
	store := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				{Upgrades: opera.Upgrades{Sonic: true}, Height: 2},
				{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Height: 4},
			},
		},
	}

	// correct upgrade
	require.NoError(t, store.StoreUpgrade(opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: true}, Height: 2}))
	// incorrect upgrades
	require.Error(t, store.StoreUpgrade(opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: false}, Height: 2}))
	// incorrect height
	require.Error(t, store.StoreUpgrade(opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: true}, Height: 1}))
}

func TestStaticMetadataStore_GetCorrections_ReturnsCorrections(t *testing.T) {
	corrections := Corrections{
		10: {
			common.HexToAddress("0x123"): Correction{Balance: uint256.Int{100}},
		},
		20: {
			common.HexToAddress("0x456"): Correction{Balance: uint256.Int{200}},
		},
	}

	store := &StaticMetadataStore{
		metadata: Metadata{Corrections: corrections},
	}

	require.Equal(t, corrections[10], store.GetCorrections(10))
	require.Equal(t, corrections[20], store.GetCorrections(20))
	require.Nil(t, store.GetCorrections(30))
}
