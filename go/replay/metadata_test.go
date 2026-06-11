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
	"encoding/json"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestNewBlockDBMetadataStore_LoadsMetadataFromDB(t *testing.T) {
	upgrades := []opera.UpgradeHeight{{Upgrades: opera.Upgrades{Sonic: true}, Height: 5}}
	corrections := Corrections{
		10: {common.HexToAddress("0x123"): {Balance: *uint256.NewInt(100)}},
	}

	upgradesData, err := json.Marshal(upgrades)
	require.NoError(t, err)
	correctionsData, err := json.Marshal(corrections)
	require.NoError(t, err)

	testCases := map[string]struct {
		upgradesData      []byte
		correctionsData   []byte
		expectErr         string
		expectLog         func(logger *utils.MockLogger)
		expectUpgrades    []opera.UpgradeHeight
		expectCorrections Corrections
	}{
		"no upgrade heights, no corrections": {
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No upgrade heights available").Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
			},
		},
		"only upgrade heights": {
			upgradesData: upgradesData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded upgrade heights from block db", "num_upgrade_heights", len(upgrades)).Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
			},
			expectUpgrades: upgrades,
		},
		"only corrections": {
			correctionsData: correctionsData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No upgrade heights available").Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
			},
			expectCorrections: corrections,
		},
		"both": {
			upgradesData:    upgradesData,
			correctionsData: correctionsData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded upgrade heights from block db", "num_upgrade_heights", len(upgrades)).Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
			},
			expectUpgrades:    upgrades,
			expectCorrections: corrections,
		},
		"invalid upgrade heights": {
			upgradesData:    []byte("not-json"),
			correctionsData: correctionsData,
			expectErr:       "failed to parse stored upgrade heights",
		},
		"invalid corrections": {
			upgradesData:    upgradesData,
			correctionsData: []byte("not-json"),
			expectErr:       "failed to parse stored corrections",
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded upgrade heights from block db", "num_upgrade_heights", len(upgrades)).Times(1)
			},
		},
	}

	for name, tc := range testCases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			logger := utils.NewMockLogger(ctrl)
			chainID := uint64(146)

			db.EXPECT().GetUpgradeHeights(chainID).Return(tc.upgradesData, nil).Times(1)
			db.EXPECT().GetCorrections(chainID).Return(tc.correctionsData, nil).Times(1)

			if tc.expectLog != nil {
				tc.expectLog(logger)
			}

			store, err := NewBlockDBMetadataStore(db, chainID, logger, false)
			if tc.expectErr != "" {
				require.ErrorContains(t, err, tc.expectErr)
				return
			}

			require.NoError(t, err)
			require.Equal(t, tc.expectUpgrades, store.metadata.UpgradeHeights)
			require.Equal(t, tc.expectCorrections, store.metadata.Corrections)
		})
	}
}

func TestBlockDBMetadataStore_PatchUpgrades_ReturnsErrorForInvalidDiff(t *testing.T) {
	store := &BlockDBMetadataStore{}
	err := store.PatchUpgrades(0, []byte("not valid json {{{"))
	require.Error(t, err)
}

func TestBlockDBMetadataStore_PatchUpgrades_IgnoresUpdatesWithoutChanges(t *testing.T) {
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			UpgradeHeights: []opera.UpgradeHeight{
				{Upgrades: opera.Upgrades{Sonic: true}, Height: 2},
			},
		},
	}

	diff := []byte(`{"Upgrades":{"Sonic":true}}`)
	err := store.PatchUpgrades(5, diff)
	require.NoError(t, err)
	require.Nil(t, store.nextUpgrades)
}

func TestBlockDBMetadataStore_PatchUpgrades_AppliesDiffToNextUpgrades(t *testing.T) {
	store := &BlockDBMetadataStore{}
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

func TestBlockDBMetadataStore_CommitUpgrades_NoNextUpgrades_IsNoOp(t *testing.T) {
	store := &BlockDBMetadataStore{}
	require.NoError(t, store.CommitUpgrades(5))
}

func TestBlockDBMetadataStore_CommitUpgrades_KnownUpgradeIsNotStoredAgain(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	knownUpgrade := opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Height: 4}
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			UpgradeHeights: []opera.UpgradeHeight{
				knownUpgrade,
			},
		},
		logger: logger,
	}

	logger.EXPECT().Info("Detected known upgrade", "block", knownUpgrade.Height).Times(1)

	store.nextUpgrades = &knownUpgrade.Upgrades
	require.NoError(t, store.CommitUpgrades(uint64(knownUpgrade.Height-1)))
	require.Nil(t, store.nextUpgrades)
	require.Len(t, store.metadata.UpgradeHeights, 1)
}

func TestBlockDBMetadataStore_CommitUpgrades_KnownHeightWithDifferentUpgrade_ReturnsError(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	knownUpgrade := opera.UpgradeHeight{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Height: 4}
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			UpgradeHeights: []opera.UpgradeHeight{
				knownUpgrade,
			},
		},
		logger: logger,
	}

	mismatch := opera.Upgrades{Sonic: true, Brio: true}
	store.nextUpgrades = &mismatch
	err := store.CommitUpgrades(uint64(knownUpgrade.Height - 1))
	require.ErrorContains(t, err, "unexpected upgrade at block 4")
	require.Nil(t, store.nextUpgrades)
}

func TestBlockDBMetadataStore_CommitUpgrades_NewUpgradeIsStoredWhenWriteEnabled(t *testing.T) {
	cases := map[string]struct {
		writeUpgradeHeights bool
	}{
		"write enabled":  {writeUpgradeHeights: true},
		"write disabled": {writeUpgradeHeights: false},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			logger := utils.NewMockLogger(ctrl)

			newUpgrades := opera.Upgrades{Sonic: true, Allegro: true, Brio: true}
			store := &BlockDBMetadataStore{
				db:                  db,
				chainID:             146,
				logger:              logger,
				writeUpgradeHeights: tc.writeUpgradeHeights,
				metadata: Metadata{UpgradeHeights: []opera.UpgradeHeight{
					{Upgrades: opera.Upgrades{Sonic: true}, Height: 5},
					{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Height: 10},
				}},
			}

			if tc.writeUpgradeHeights {
				db.EXPECT().PutUpgradeHeights(store.chainID, gomock.Any()).DoAndReturn(func(chainID uint64, data []byte) error {
					var got []opera.UpgradeHeight
					require.NoError(t, json.Unmarshal(data, &got))
					require.Len(t, got, 3)
					require.Equal(t, uint64(5), uint64(got[0].Height))
					require.Equal(t, uint64(10), uint64(got[1].Height))
					require.Equal(t, uint64(15+1), uint64(got[2].Height))
					require.Equal(t, newUpgrades, got[2].Upgrades)
					return nil
				}).Times(1)
				logger.EXPECT().Info("New upgrade detected and stored in the block db", "block", gomock.Any()).Times(1)
			} else {
				logger.EXPECT().Warn("New upgrade detected but not stored in the block db (use --write-upgrade-heights to persist)", "block", gomock.Any()).Times(1)
			}

			store.nextUpgrades = &newUpgrades
			require.NoError(t, store.CommitUpgrades(15))
			require.Nil(t, store.nextUpgrades)
			// Upgrade is always tracked in-memory.
			require.Len(t, store.metadata.UpgradeHeights, 3)
		})
	}
}

func TestBlockDBMetadataStore_GetUpgradesAtBlock_ObtainsUpgradesFromCachedValuesBasedOnBlockNumber(t *testing.T) {
	upgrades := []opera.Upgrades{
		{Sonic: true},
		{Sonic: true, Allegro: true},
		{Sonic: true, SingleProposerBlockFormation: true},
	}

	store := &BlockDBMetadataStore{
		metadata: Metadata{
			UpgradeHeights: []opera.UpgradeHeight{
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

func TestBlockDBMetadataStore_GetCorrectionsAtBlock_ReturnsCorrections(t *testing.T) {
	corrections := Corrections{
		10: {
			common.HexToAddress("0x123"): {Balance: *uint256.NewInt(100)},
		},
		20: {
			common.HexToAddress("0x456"): {Balance: *uint256.NewInt(200)},
		},
	}

	store := &BlockDBMetadataStore{
		metadata: Metadata{Corrections: corrections},
	}

	require.Equal(t, corrections[10], store.GetCorrectionsAtBlock(10))
	require.Equal(t, corrections[20], store.GetCorrectionsAtBlock(20))
	require.Nil(t, store.GetCorrectionsAtBlock(30))
}
