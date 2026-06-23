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
	genesisRules := opera.Rules{NetworkID: 146}
	storedHeightsWithGenesis := []RulesUpdateHeight{
		{Block: 0, Rules: genesisRules},
		{Block: 5, Rules: opera.Rules{NetworkID: 146, Upgrades: opera.Upgrades{Sonic: true}}},
	}
	storedHeightsWithoutGenesis := []RulesUpdateHeight{
		{Block: 5, Rules: opera.Rules{NetworkID: 146, Upgrades: opera.Upgrades{Sonic: true}}},
	}
	corrections := Corrections{
		10: {common.HexToAddress("0x123"): {Balance: *uint256.NewInt(100)}},
	}

	rulesUpdateHeightsWithGenesisData, err := json.Marshal(storedHeightsWithGenesis)
	require.NoError(t, err)
	rulesUpdateHeightsWithoutGenesisData, err := json.Marshal(storedHeightsWithoutGenesis)
	require.NoError(t, err)
	correctionsData, err := json.Marshal(corrections)
	require.NoError(t, err)

	testCases := map[string]struct {
		rulesUpdateHeightsData   []byte
		correctionsData          []byte
		expectErr                string
		expectLog                func(logger *utils.MockLogger)
		expectRulesUpdateHeights []RulesUpdateHeight
		expectCorrections        Corrections
	}{
		"no rules update heights, no corrections": {
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
			},
			expectRulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: genesisRules}},
		},
		"stored heights with genesis": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithGenesisData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", len(storedHeightsWithGenesis)).Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
		},
		"stored heights without genesis prepends it": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithoutGenesisData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", 1).Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
		},
		"only corrections": {
			correctionsData: correctionsData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
			},
			expectRulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: genesisRules}},
			expectCorrections:        corrections,
		},
		"both": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithGenesisData,
			correctionsData:        correctionsData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", len(storedHeightsWithGenesis)).Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
			expectCorrections:        corrections,
		},
		"genesis rules mismatch": {
			rulesUpdateHeightsData: func() []byte {
				data, _ := json.Marshal([]RulesUpdateHeight{
					{Block: 0, Rules: opera.Rules{NetworkID: 146, Upgrades: opera.Upgrades{Sonic: true}}},
				})
				return data
			}(),
			expectErr: "stored genesis rules at block 0 do not match provided genesis rules",
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", 1).Times(1)
			},
		},
		"invalid rules update heights": {
			rulesUpdateHeightsData: []byte("not-json"),
			correctionsData:        correctionsData,
			expectErr:              "failed to parse stored rules update heights",
		},
		"invalid corrections": {
			correctionsData: []byte("not-json"),
			expectErr:       "failed to parse stored corrections",
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
			},
		},
	}

	for name, tc := range testCases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			logger := utils.NewMockLogger(ctrl)

			db.EXPECT().GetRulesUpdateHeights(genesisRules.NetworkID).Return(tc.rulesUpdateHeightsData, nil).Times(1)
			db.EXPECT().GetCorrections(genesisRules.NetworkID).Return(tc.correctionsData, nil).Times(1)

			if tc.expectLog != nil {
				tc.expectLog(logger)
			}

			store, err := NewBlockDBMetadataStore(db, genesisRules, logger, false)
			if tc.expectErr != "" {
				require.ErrorContains(t, err, tc.expectErr)
				return
			}

			require.NoError(t, err)
			require.Equal(t, tc.expectRulesUpdateHeights, store.metadata.RulesUpdateHeights)
			require.Equal(t, tc.expectCorrections, store.metadata.Corrections)
		})
	}
}

func TestBlockDBMetadataStore_PatchRules_ReturnsErrorForInvalidDiff(t *testing.T) {
	rules := opera.FakeNetRules(opera.Upgrades{Berlin: true, London: true, Sonic: true})
	store := &BlockDBMetadataStore{
		metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: rules}}},
	}
	err := store.PatchRules(0, []byte("not valid json {{{"))
	require.Error(t, err)
}

func TestBlockDBMetadataStore_PatchRules_AppliesDiffToNextRules(t *testing.T) {
	rules := opera.FakeNetRules(opera.Upgrades{Berlin: true, London: true, Sonic: true})
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 0, Rules: rules},
			},
		},
	}
	require.Nil(t, store.nextRules)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	err := store.PatchRules(0, diff)
	require.NoError(t, err)
	require.NotNil(t, store.nextRules)
	require.True(t, store.nextRules.Upgrades.Allegro)
	require.False(t, store.nextRules.Upgrades.Brio)

	diff = []byte(`{"Upgrades":{"Brio":true}}`)
	err = store.PatchRules(0, diff)
	require.NoError(t, err)
	require.NotNil(t, store.nextRules)
	require.True(t, store.nextRules.Upgrades.Allegro)
	require.True(t, store.nextRules.Upgrades.Brio)
}

func TestBlockDBMetadataStore_PatchRules_UsesLatestRulesAtOrBeforeBlock(t *testing.T) {
	rulesAt0 := opera.FakeNetRules(opera.Upgrades{Berlin: true, London: true, Sonic: true})
	rulesAt10 := rulesAt0
	rulesAt10.Upgrades = opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true}
	rulesAt20 := rulesAt0
	rulesAt20.Upgrades = opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true, Brio: true}

	store := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 0, Rules: rulesAt0},
				{Block: 10, Rules: rulesAt10},
				{Block: 20, Rules: rulesAt20},
			},
		},
	}

	err := store.PatchRules(12, []byte(`{"Upgrades":{"GasSubsidies":true}}`))
	require.NoError(t, err)
	require.NotNil(t, store.nextRules)
	require.True(t, store.nextRules.Upgrades.Allegro)
	require.False(t, store.nextRules.Upgrades.Brio)
	require.True(t, store.nextRules.Upgrades.GasSubsidies)
}

func TestBlockDBMetadataStore_CommitRules_NoNextRules_IsNoOp(t *testing.T) {
	store := &BlockDBMetadataStore{}
	require.NoError(t, store.CommitRules(5))
}

func TestBlockDBMetadataStore_CommitRules_KnownRulesUpdateIsNotStoredAgain(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	knownRules := opera.Rules{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}}
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 4, Rules: knownRules},
			},
		},
		logger: logger,
	}

	logger.EXPECT().Info("Detected known rules update", "block", uint64(4)).Times(1)

	store.nextRules = &knownRules
	require.NoError(t, store.CommitRules(3)) // blockNumber+1 = 4
	require.Nil(t, store.nextRules)
	require.Len(t, store.metadata.RulesUpdateHeights, 1)
}

func TestBlockDBMetadataStore_CommitRules_KnownHeightWithDifferentRulesUpdate_ReturnsError(t *testing.T) {
	knownRules := opera.Rules{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}}
	store := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 4, Rules: knownRules},
			},
		},
	}

	mismatch := opera.Rules{Upgrades: opera.Upgrades{Sonic: true, Brio: true}}
	store.nextRules = &mismatch
	err := store.CommitRules(3) // blockNumber+1 = 4
	require.ErrorContains(t, err, "unexpected rules update at block 4")
	require.Nil(t, store.nextRules)
}

func TestBlockDBMetadataStore_CommitRules_NewRulesUpdateIsStoredWhenWriteEnabled(t *testing.T) {
	cases := map[string]struct {
		writeRulesUpdateHeights bool
	}{
		"write enabled":  {writeRulesUpdateHeights: true},
		"write disabled": {writeRulesUpdateHeights: false},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			logger := utils.NewMockLogger(ctrl)

			chainID := uint64(146)
			newRules := opera.Rules{Upgrades: opera.Upgrades{Sonic: true, Allegro: true, Brio: true}}
			store := &BlockDBMetadataStore{
				db:                      db,
				logger:                  logger,
				writeRulesUpdateHeights: tc.writeRulesUpdateHeights,
				metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{
					{Block: 5, Rules: opera.Rules{NetworkID: chainID, Upgrades: opera.Upgrades{Sonic: true}}},
					{Block: 10, Rules: opera.Rules{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}}},
				}},
			}

			if tc.writeRulesUpdateHeights {
				db.EXPECT().PutRulesUpdateHeights(chainID, gomock.Any()).DoAndReturn(func(chainID uint64, data []byte) error {
					var got []RulesUpdateHeight
					require.NoError(t, json.Unmarshal(data, &got))
					require.Len(t, got, 3)
					require.Equal(t, uint64(5), got[0].Block)
					require.Equal(t, uint64(10), got[1].Block)
					require.Equal(t, uint64(16), got[2].Block)
					require.Equal(t, newRules, got[2].Rules)
					return nil
				}).Times(1)
				logger.EXPECT().Info("New rules update detected and stored in the block db", "block", uint64(16)).Times(1)
			} else {
				logger.EXPECT().Warn("New rules update detected but not stored in the block db (use --write-rules-update-heights to persist)", "block", uint64(16)).Times(1)
			}

			store.nextRules = &newRules
			require.NoError(t, store.CommitRules(15))
			require.Nil(t, store.nextRules)
			// Rules update is always tracked in-memory.
			require.Len(t, store.metadata.RulesUpdateHeights, 3)
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
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 5, Rules: opera.Rules{Upgrades: upgrades[0]}},
				{Block: 7, Rules: opera.Rules{Upgrades: upgrades[1]}},
				{Block: 11, Rules: opera.Rules{Upgrades: upgrades[2]}},
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
		require.Equal(t, expect, got)
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

func TestRulesUpdateHeights_MarshalUnmarshal_IsIdentity(t *testing.T) {
	testCases := map[string]struct {
		rulesUpdateHeights []RulesUpdateHeight
	}{
		"all fields set": {
			rulesUpdateHeights: []RulesUpdateHeight{{
				Block: 123,
				Rules: opera.MainNetRules(),
			}},
		},
		"zero value fields": {
			rulesUpdateHeights: []RulesUpdateHeight{{}},
		},
		"empty slice": {
			rulesUpdateHeights: []RulesUpdateHeight{},
		},
		"nil slice": {
			rulesUpdateHeights: nil,
		},
	}

	for name, tc := range testCases {
		t.Run(name, func(t *testing.T) {
			data, err := json.Marshal(tc.rulesUpdateHeights)
			require.NoError(t, err)

			var got []RulesUpdateHeight
			require.NoError(t, json.Unmarshal(data, &got))
			require.Equal(t, tc.rulesUpdateHeights, got)
		})
	}
}
