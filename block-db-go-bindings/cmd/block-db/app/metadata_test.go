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

package app

import (
	"testing"

	"github.com/0xsoniclabs/sonic/opera"
	"github.com/stretchr/testify/require"
)

func TestMetadata_GetRulesAtBlock_ObtainsUpgradesBasedOnBlockNumber(t *testing.T) {
	upgrades := []opera.Upgrades{
		{Sonic: true},
		{Sonic: true, Allegro: true},
		{Sonic: true, SingleProposerBlockFormation: true},
	}

	metadata := Metadata{
		Upgrades: []opera.UpgradeHeight{
			{Upgrades: upgrades[0], Height: 5},
			{Upgrades: upgrades[1], Height: 7},
			{Upgrades: upgrades[2], Height: 11},
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

		rules := metadata.GetRulesAtBlock(uint64(blockNr))
		require.Equal(t, expect, rules, "block number %d", blockNr)
	}
}

func TestGetMetadataForChain_SonicChain_ContainsCorrections(t *testing.T) {
	metadata, err := GetMetadataForChain(SonicMainNetChainId)
	require.NoError(t, err)
	require.NotEmpty(t, metadata.Corrections)
}

func TestGetMetadataForChain_AllegroTestChain_NoCorrectionsButUpgrades(t *testing.T) {
	metadata, err := GetMetadataForChain(AllegroTestNetChainId)
	require.NoError(t, err)
	require.NotEmpty(t, metadata.Upgrades)
	require.Empty(t, metadata.Corrections)

	// Make sure the upgrades are in ascending order.
	last := 0
	for _, upgrade := range metadata.Upgrades {
		require.Greater(t, int(upgrade.Height), last)
		last = int(upgrade.Height)
	}
}
