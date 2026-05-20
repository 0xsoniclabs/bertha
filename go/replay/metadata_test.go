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
	"testing"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestStaticMetadataStore_GetUpgradesAtBlock_ObtainsUpgradesBasedOnBlockNumber(t *testing.T) {
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
		var expect opera.Upgrades
		if blockNr >= 11 {
			expect = upgrades[2]
		} else if blockNr >= 7 {
			expect = upgrades[1]
		} else if blockNr >= 5 {
			expect = upgrades[0]
		}

		got := store.GetUpgradesAtBlock(uint64(blockNr))
		require.Equal(t, expect, got, "block number %d", blockNr)
	}
}

func TestNewStaticMetadataStore_SonicChain_ContainsCorrections(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	store, err := NewStaticMetadataStore(SonicMainNetChainID, logger)
	require.NoError(t, err)
	require.NotEmpty(t, store.metadata.Corrections)
}

func TestNewStaticMetadataStore_AllegroTestChain_NoCorrectionsButUpgrades(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	store, err := NewStaticMetadataStore(AllegroTestNetChainID, logger)
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
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	chainID := uint64(12345)

	logger.EXPECT().Warn("no metadata available for chain ID, proceeding without upgrades or corrections", "chainId", chainID).Times(1)

	store, err := NewStaticMetadataStore(chainID, logger)
	require.NoError(t, err)
	require.Empty(t, store.metadata.Upgrades)
	require.Empty(t, store.metadata.Corrections)
}

func TestStaticMetadataStore_PatchUpgrades_ReturnsErrorForInvalidDiff(t *testing.T) {
	store := &StaticMetadataStore{}
	err := store.PatchUpgrades(0, []byte("not valid json {{{"))
	require.Error(t, err)
}

func TestStaticMetadataStore_PatchUpgrades_IgnoresUpdatesWithoutChanges(t *testing.T) {
	store := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				{Upgrades: opera.Upgrades{Sonic: true}, Height: 2},
			},
		},
	}

	diff := []byte(`{"Upgrades":{"Sonic":true}}`)
	err := store.PatchUpgrades(5, diff)
	require.NoError(t, err)
	require.Nil(t, store.nextUpgrades)
}

func TestStaticMetadataStore_PatchUpgrades_AppliesDiffToNextUpgrades(t *testing.T) {
	store := &StaticMetadataStore{}
	require.Nil(t, store.nextUpgrades)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	err := store.PatchUpgrades(0, diff)
	require.NoError(t, err)
	require.NotNil(t, store.nextUpgrades)
	// Berlin, London and Sonic are enabled by default.
	require.True(t, store.nextUpgrades.Berlin)
	require.True(t, store.nextUpgrades.London)
	require.True(t, store.nextUpgrades.Sonic)
	require.True(t, store.nextUpgrades.Allegro)
	require.False(t, store.nextUpgrades.Brio)

	diff = []byte(`{"Upgrades":{"Brio":true}}`)
	err = store.PatchUpgrades(0, diff)
	require.NoError(t, err)
	require.NotNil(t, store.nextUpgrades)
	require.True(t, store.nextUpgrades.Berlin)
	require.True(t, store.nextUpgrades.London)
	require.True(t, store.nextUpgrades.Sonic)
	require.True(t, store.nextUpgrades.Allegro)
	require.True(t, store.nextUpgrades.Brio)
}

func TestStaticMetadataStore_CommitUpgrades_VerifiesUpgradeExists(t *testing.T) {
	sonicUpgrades := opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Height: 4}
	store := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				sonicUpgrades,
			},
		},
	}

	// correct upgrade and height
	store.nextUpgrades = &sonicUpgrades.Upgrades
	require.NoError(t, store.CommitUpgrades(uint64(sonicUpgrades.Height-1)))

	// no upgrade
	store.nextUpgrades = nil
	require.NoError(t, store.CommitUpgrades(2))

	// wrong upgrade
	store.nextUpgrades = &opera.Upgrades{Sonic: false}
	require.Error(t, store.CommitUpgrades(uint64(sonicUpgrades.Height-1)))

	// wrong height
	store.nextUpgrades = &sonicUpgrades.Upgrades
	require.Error(t, store.CommitUpgrades(0))
}

func TestStaticMetadataStore_CommitUpgrades_ClearsNextUpgradesAfterSuccess(t *testing.T) {
	store := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				{Upgrades: opera.Upgrades{Sonic: true}, Height: 2},
			},
		},
	}

	sonicUpgrades := opera.Upgrades{Sonic: true}
	store.nextUpgrades = &sonicUpgrades
	require.NoError(t, store.CommitUpgrades(1))
	// nextUpgrades must be cleared after a successful commit
	require.Nil(t, store.nextUpgrades)
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
