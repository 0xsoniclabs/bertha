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
