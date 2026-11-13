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
	"encoding/json"
	"fmt"
	"log/slog"
	"math/big"
	"slices"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
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
		10: {common.HexToAddress("0x123"): {Balance: uint256.NewInt(100)}},
	}

	rulesUpdateHeightsWithGenesisData, err := json.Marshal(storedHeightsWithGenesis)
	require.NoError(t, err)
	rulesUpdateHeightsWithoutGenesisData, err := json.Marshal(storedHeightsWithoutGenesis)
	require.NoError(t, err)
	correctionsData, err := json.Marshal(corrections)
	require.NoError(t, err)
	txs := []*types.Transaction{
		types.NewTx(&types.LegacyTx{Nonce: 0}),
		types.NewTx(&types.LegacyTx{Nonce: 1}),
		types.NewTx(&types.LegacyTx{Nonce: 2}),
	}
	failedBundles := FailedBundles{
		7: {{Pos: 0, Tx: txs[0]}, {Pos: 2, Tx: txs[1]}},
		9: {{Pos: 1, Tx: txs[2]}},
	}
	type failedBundleEntry struct {
		Block uint64 `json:"block"`
		Pos   uint64 `json:"pos"`
		Tx    string `json:"tx"`
	}
	failedBundlesData, err := json.Marshal([]failedBundleEntry{
		{Block: 7, Pos: 0, Tx: encodeTx(t, txs[0])},
		{Block: 7, Pos: 2, Tx: encodeTx(t, txs[1])},
		{Block: 9, Pos: 1, Tx: encodeTx(t, txs[2])},
	})
	require.NoError(t, err)

	testCases := map[string]struct {
		rulesUpdateHeightsData   []byte
		correctionsData          []byte
		failedBundlesData        []byte
		expectErr                string
		expectLog                func(logger *utils.MockLogger)
		expectRulesUpdateHeights []RulesUpdateHeight
		expectCorrections        Corrections
		expectFailedBundles      FailedBundles
	}{
		"none": {
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
				logger.EXPECT().Warn("No failed bundles available").Times(1)
			},
			expectRulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: genesisRules}},
		},
		"stored heights with genesis": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithGenesisData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", len(storedHeightsWithGenesis)).Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
				logger.EXPECT().Warn("No failed bundles available").Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
		},
		"stored heights without genesis prepends it": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithoutGenesisData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", 1).Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
				logger.EXPECT().Warn("No failed bundles available").Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
		},
		"only corrections": {
			correctionsData: correctionsData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
				logger.EXPECT().Warn("No failed bundles available").Times(1)
			},
			expectRulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: genesisRules}},
			expectCorrections:        corrections,
		},
		"only failed bundles": {
			failedBundlesData: failedBundlesData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
				logger.EXPECT().Info("Loaded failed bundles from block db", "num_blocks", len(failedBundles)).Times(1)
			},
			expectRulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: genesisRules}},
			expectFailedBundles:      failedBundles,
		},
		"all": {
			rulesUpdateHeightsData: rulesUpdateHeightsWithGenesisData,
			correctionsData:        correctionsData,
			failedBundlesData:      failedBundlesData,
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Info("Loaded rules update heights from block db", "num_rules_update_heights", len(storedHeightsWithGenesis)).Times(1)
				logger.EXPECT().Info("Loaded corrections from block db", "num_corrections", len(corrections)).Times(1)
				logger.EXPECT().Info("Loaded failed bundles from block db", "num_blocks", len(failedBundles)).Times(1)
			},
			expectRulesUpdateHeights: storedHeightsWithGenesis,
			expectCorrections:        corrections,
			expectFailedBundles:      failedBundles,
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
		"invalid failed bundles": {
			failedBundlesData: []byte("not-json"),
			expectErr:         "failed to parse stored failed bundles",
			expectLog: func(logger *utils.MockLogger) {
				logger.EXPECT().Warn("No rules update heights available").Times(1)
				logger.EXPECT().Warn("No corrections available").Times(1)
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
			db.EXPECT().GetFailedBundles(genesisRules.NetworkID).Return(tc.failedBundlesData, nil).Times(1)

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
			require.Equal(t, failedBundleHashes(tc.expectFailedBundles), failedBundleHashes(store.metadata.FailedBundles))
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

// Regression test: see PatchRules comment.
func TestBlockDBMetadataStore_PatchRules_DoesNotModifyStoredRules(t *testing.T) {
	tests := map[string]struct {
		newMinBaseFee  *big.Int
		newMinGasPrice *big.Int // nil leaves MinGasPrice out of the diff
		makeDiff       func(baseFee, gasPrice *big.Int) []byte
	}{
		"base fee and gas price": {
			newMinBaseFee:  big.NewInt(50000000000),
			newMinGasPrice: big.NewInt(2000000000),
			makeDiff: func(baseFee, gasPrice *big.Int) []byte {
				return fmt.Appendf(nil, `{"Economy":{"MinBaseFee":%d,"MinGasPrice":%d}}`, baseFee, gasPrice)
			},
		},
		"base fee only": {
			newMinBaseFee: big.NewInt(50000000000),
			makeDiff: func(baseFee, gasPrice *big.Int) []byte {
				return fmt.Appendf(nil, `{"Economy":{"MinBaseFee":%d}}`, baseFee)
			},
		},
	}
	for diffName, test := range tests {
		for name, upgrades := range opera.GetAllHardForksInOrder() {
			t.Run(diffName+"/"+name, func(t *testing.T) {
				require := require.New(t)
				rules := opera.FakeNetRules(upgrades)
				originalMinBaseFee := new(big.Int).Set(rules.Economy.MinBaseFee)
				originalMinGasPrice := new(big.Int).Set(rules.Economy.MinGasPrice)

				require.NotEqual(originalMinBaseFee, test.newMinBaseFee)
				wantNextMinGasPrice := originalMinGasPrice
				if test.newMinGasPrice != nil {
					require.NotEqual(originalMinGasPrice, test.newMinGasPrice)
					wantNextMinGasPrice = test.newMinGasPrice
				}

				diff := test.makeDiff(test.newMinBaseFee, test.newMinGasPrice)
				store := &BlockDBMetadataStore{
					metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: rules}}},
				}

				err := store.PatchRules(0, diff)
				require.NoError(err)
				require.NotNil(store.nextRules)
				require.Equal(test.newMinBaseFee, store.nextRules.Economy.MinBaseFee)
				require.Equal(wantNextMinGasPrice, store.nextRules.Economy.MinGasPrice)
				require.Equal(originalMinBaseFee, store.metadata.RulesUpdateHeights[0].Rules.Economy.MinBaseFee)
				require.Equal(originalMinGasPrice, store.metadata.RulesUpdateHeights[0].Rules.Economy.MinGasPrice)
			})
		}
	}
}

func TestBlockDBMetadataStore_PatchRules_UsesLatestRulesAtOrBeforeNextBlock(t *testing.T) {
	rulesAt0 := opera.FakeNetRules(opera.Upgrades{Berlin: true, London: true, Sonic: true})
	rulesAt10 := rulesAt0
	rulesAt10.Upgrades = opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true}
	rulesAt20 := rulesAt0
	rulesAt20.Upgrades = opera.Upgrades{Berlin: true, London: true, Sonic: true, Allegro: true, Brio: true}

	tests := map[string]struct {
		block    uint64
		wantBrio bool
	}{
		"within epoch":        {block: 12},
		"epoch sealing block": {block: 19, wantBrio: true},
	}
	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			store := &BlockDBMetadataStore{
				metadata: Metadata{
					RulesUpdateHeights: []RulesUpdateHeight{
						{Block: 0, Rules: rulesAt0},
						{Block: 10, Rules: rulesAt10},
						{Block: 20, Rules: rulesAt20},
					},
				},
			}

			err := store.PatchRules(tc.block, []byte(`{"Upgrades":{"GasSubsidies":true}}`))
			require.NoError(t, err)
			require.NotNil(t, store.nextRules)
			require.True(t, store.nextRules.Upgrades.Allegro)
			require.Equal(t, tc.wantBrio, store.nextRules.Upgrades.Brio)
			require.True(t, store.nextRules.Upgrades.GasSubsidies)
		})
	}
}

func TestBlockDBMetadataStore_PatchRules_AppliesPatchedMinBaseFeeFromNextBlockBeforeAllegro(t *testing.T) {
	sonic := opera.FakeNetRules(opera.GetSonicUpgrades())
	sealed := opera.FakeNetRules(opera.GetSonicUpgrades())
	sealed.Blocks.MaxBlockGas++
	noMinBaseFee := opera.FakeNetRules(opera.GetSonicUpgrades())
	noMinBaseFee.Economy.MinBaseFee = nil
	allegro := opera.FakeNetRules(opera.GetAllegroUpgrades())
	withMinBaseFee := func(rules opera.Rules) opera.Rules {
		rules.Economy.MinBaseFee = big.NewInt(50_000_000_000)
		return rules
	}
	diff := `{"Economy":{"MinBaseFee":50000000000}}`

	tests := map[string]struct {
		heights  []RulesUpdateHeight
		block    uint64
		diff     string
		wantErr  bool
		wantNext opera.Rules
	}{
		"before Allegro": {
			heights:  []RulesUpdateHeight{{Block: 0, Rules: sonic}, {Block: 10, Rules: allegro}},
			block:    5,
			diff:     diff,
			wantNext: withMinBaseFee(sonic),
		},
		"rejected update before Allegro": {
			heights:  []RulesUpdateHeight{{Block: 0, Rules: sonic}, {Block: 10, Rules: allegro}},
			block:    5,
			diff:     `{"Economy":{"MinBaseFee":50000000000},"Blocks":{"MaxBlockGas":"x"}}`,
			wantErr:  true,
			wantNext: withMinBaseFee(sonic),
		},
		"epoch sealing block before Allegro": {
			heights:  []RulesUpdateHeight{{Block: 0, Rules: sonic}, {Block: 6, Rules: sealed}, {Block: 10, Rules: allegro}},
			block:    5,
			diff:     diff,
			wantNext: withMinBaseFee(sealed),
		},
		"no MinBaseFee before Allegro": {
			heights:  []RulesUpdateHeight{{Block: 0, Rules: noMinBaseFee}, {Block: 10, Rules: allegro}},
			block:    5,
			diff:     diff,
			wantNext: noMinBaseFee,
		},
		"from Allegro on": {
			heights:  []RulesUpdateHeight{{Block: 0, Rules: sonic}, {Block: 10, Rules: allegro}},
			block:    15,
			diff:     diff,
			wantNext: allegro,
		},
	}
	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			store := &BlockDBMetadataStore{metadata: Metadata{RulesUpdateHeights: slices.Clone(tc.heights)}}
			rules := store.GetRulesAtBlock(tc.block)

			err := store.PatchRules(tc.block, []byte(tc.diff))
			require.Equal(t, tc.wantErr, err != nil)
			require.Equal(t, rules, store.GetRulesAtBlock(tc.block))
			require.Equal(t, tc.wantNext, store.GetRulesAtBlock(tc.block+1))
			require.Equal(t, tc.heights, store.metadata.RulesUpdateHeights)
		})
	}
}

func TestBlockDBMetadataStore_PatchRules_RejectedUpdateBeforeAllegroPatchesMinBaseFeeOfNextRules(t *testing.T) {
	noMinBaseFee := opera.FakeNetRules(opera.GetSonicUpgrades())
	noMinBaseFee.Economy.MinBaseFee = nil

	tests := map[string]struct {
		rules       opera.Rules
		wantCurrent *big.Int
	}{
		"with MinBaseFee":    {rules: opera.FakeNetRules(opera.GetSonicUpgrades()), wantCurrent: big.NewInt(50_000_000_000)},
		"without MinBaseFee": {rules: noMinBaseFee},
	}
	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			store := &BlockDBMetadataStore{metadata: Metadata{RulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: tc.rules}}}}
			require.NoError(t, store.PatchRules(1, []byte(`{"Economy":{"MinBaseFee":2000000000},"Blocks":{"MaxBlockGas":123456789}}`)))

			err := store.PatchRules(2, []byte(`{"Economy":{"MinBaseFee":50000000000},"Blocks":{"MaxBlockGas":"x"}}`))
			require.Error(t, err)
			require.Equal(t, big.NewInt(50_000_000_000), store.nextRules.Economy.MinBaseFee)
			require.Equal(t, uint64(123456789), store.nextRules.Blocks.MaxBlockGas)
			require.Equal(t, tc.wantCurrent, store.GetRulesAtBlock(3).Economy.MinBaseFee)
		})
	}
}

func TestBlockDBMetadataStore_PatchRules_DoesNotModifyRulesCommittedInSameBlock(t *testing.T) {
	heights := []RulesUpdateHeight{{Block: 0, Rules: opera.FakeNetRules(opera.GetSonicUpgrades())}}
	for range 2 {
		store := &BlockDBMetadataStore{metadata: Metadata{RulesUpdateHeights: heights}, logger: slog.New(slog.DiscardHandler)}
		require.NoError(t, store.PatchRules(2, []byte(`{"Blocks":{"MaxBlockGas":123456789}}`)))
		require.NoError(t, store.CommitRules(5))
		require.NoError(t, store.PatchRules(5, []byte(`{"Economy":{"MinBaseFee":50000000000}}`)))
		heights = store.metadata.RulesUpdateHeights
	}
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

func TestBlockDBMetadataStore_GetRulesAtBlock_ObtainsRulesFromCachedValuesBasedOnBlockNumber(t *testing.T) {
	rules := []opera.Rules{
		{Upgrades: opera.Upgrades{Sonic: true}, Economy: opera.EconomyRules{Gas: opera.GasRules{MaxEventGas: 1}}},
		{Upgrades: opera.Upgrades{Sonic: true, Allegro: true}, Economy: opera.EconomyRules{Gas: opera.GasRules{MaxEventGas: 2}}},
		{Upgrades: opera.Upgrades{Sonic: true, SingleProposerBlockFormation: true}, Economy: opera.EconomyRules{Gas: opera.GasRules{MaxEventGas: 3}}},
	}

	store := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 5, Rules: rules[0]},
				{Block: 7, Rules: rules[1]},
				{Block: 11, Rules: rules[2]},
			},
		},
	}

	for blockNr := range 20 {
		var expect opera.Rules
		if blockNr >= 11 {
			expect = rules[2]
		} else if blockNr >= 7 {
			expect = rules[1]
		} else if blockNr >= 5 {
			expect = rules[0]
		}

		got := store.GetRulesAtBlock(uint64(blockNr))
		require.Equal(t, expect, got)
	}
}

func TestBlockDBMetadataStore_GetCorrectionsAtBlock_ReturnsCorrections(t *testing.T) {
	corrections := Corrections{
		10: {
			common.HexToAddress("0x123"): {Balance: uint256.NewInt(100)},
		},
		20: {
			common.HexToAddress("0x456"): {Balance: uint256.NewInt(200)},
		},
	}

	store := &BlockDBMetadataStore{
		metadata: Metadata{Corrections: corrections},
	}

	require.Equal(t, corrections[10], store.GetCorrectionsAtBlock(10))
	require.Equal(t, corrections[20], store.GetCorrectionsAtBlock(20))
	require.Nil(t, store.GetCorrectionsAtBlock(30))
}

func TestBlockDBMetadataStore_GetFailedBundlesAtBlock_ReturnsFailedBundles(t *testing.T) {
	failedBundles := FailedBundles{
		10: {{Pos: 0, Tx: types.NewTx(&types.LegacyTx{Nonce: 1})}},
		20: {{Pos: 1, Tx: types.NewTx(&types.LegacyTx{Nonce: 2})}},
	}

	store := &BlockDBMetadataStore{
		metadata: Metadata{FailedBundles: failedBundles},
	}

	require.Equal(t, failedBundles[10], store.GetFailedBundlesAtBlock(10))
	require.Equal(t, failedBundles[20], store.GetFailedBundlesAtBlock(20))
	require.Nil(t, store.GetFailedBundlesAtBlock(30))
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
