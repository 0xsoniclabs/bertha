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

package verify

import (
	"cmp"
	"context"
	"crypto/ecdsa"
	"fmt"
	"maps"
	"math/big"
	"slices"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestCheckBlobTxs_MissingDatabase_ReportsAnIssue(t *testing.T) {
	ctrl := gomock.NewController(t)
	directory := t.TempDir()
	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Info("Opening block database", "directory", directory)

	require.ErrorContains(t,
		CheckBlobTxs(t.Context(), directory, 146, logger, utils.NewMockProgressIndicatorFactory(ctrl)),
		"failed to open database",
	)
}

func TestCheckBlobTxs_ReportsBlobTxsOfBlocksAndFailedBundles(t *testing.T) {
	chainID := uint64(146)
	signer := types.LatestSignerForChainID(new(big.Int).SetUint64(chainID))
	blobEnvelope, bundledBlobTx := wrapInBundle(t, signer, types.MustSignNewTx(newKey(t), signer, &types.BlobTx{}))
	encodedBlobEnvelope, err := blobEnvelope.MarshalBinary()
	require.NoError(t, err)

	blobBlocks := utils.CreateValidBlocks(t, 3)
	blobBlocks[1].Transactions = []*blockdb.Transaction{{}, {TransactionType: types.BlobTxType}}
	blobBlocks[2].Transactions = []*blockdb.Transaction{
		{To: bundle.BundleProcessor.Bytes(), Data: blobEnvelope.Data()},
		{TransactionType: types.BlobTxType, To: bundle.BundleProcessor.Bytes(), Data: blobEnvelope.Data()},
	}

	tests := map[string]struct {
		blocks             []*blockdb.Block
		failedBundles      string
		numFailedBundles   int
		rulesUpdateHeights string
		brioBlock          uint64
		wantLocations      []string
	}{
		"blob txs in blocks and failed bundles": {
			blocks:             blobBlocks,
			failedBundles:      fmt.Sprintf(`[{"block": 1, "pos": 2, "tx": "%[1]s"}, {"block": 5, "pos": 0, "tx": "%[1]s"}]`, hexutil.Encode(encodedBlobEnvelope)),
			numFailedBundles:   2,
			rulesUpdateHeights: `[{"Block": 1, "Rules": {}}, {"Block": 2, "Rules": {"Upgrades": {"Brio": true}}}]`,
			brioBlock:          2,
			wantLocations: []string{
				"failed bundle at block 1, position 2 > bundled tx " + bundledBlobTx.Hash().Hex(),
				"failed bundle at block 5, position 0 > bundled tx " + bundledBlobTx.Hash().Hex(),
				"block 1, tx 1",
				"block 2, tx 0 > bundled tx " + bundledBlobTx.Hash().Hex(),
				"block 2, tx 1",
				"block 2, tx 1 > bundled tx " + bundledBlobTx.Hash().Hex(),
			},
		},
		"bundles in blocks before Brio": {
			blocks:             blobBlocks,
			rulesUpdateHeights: `[{"Block": 3, "Rules": {"Upgrades": {"Brio": true}}}]`,
			brioBlock:          3,
			wantLocations:      []string{"block 1, tx 1", "block 2, tx 1"},
		},
		"no blob txs": {
			blocks: utils.CreateValidBlocks(t, 3),
		},
		"no blocks": {
			failedBundles:    fmt.Sprintf(`[{"block": 5, "pos": 0, "tx": "%s"}]`, hexutil.Encode(encodedBlobEnvelope)),
			numFailedBundles: 1,
			wantLocations:    []string{"failed bundle at block 5, position 0 > bundled tx " + bundledBlobTx.Hash().Hex()},
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			directory := createDatabase(t, chainID, tt.blocks)
			db, writeOptions := openDatabaseForWriting(t, directory)
			if tt.failedBundles != "" {
				require.NoError(t, db.Put(writeOptions, blockdb.MakeFailedBundlesKey(chainID), []byte(tt.failedBundles)))
			}
			if tt.rulesUpdateHeights != "" {
				require.NoError(t, db.Put(writeOptions, blockdb.MakeRulesUpdateHeightsKey(chainID), []byte(tt.rulesUpdateHeights)))
			}
			db.Close()

			ctrl := gomock.NewController(t)
			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Info("Opening block database", "directory", directory)
			progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
			if tt.numFailedBundles > 0 {
				logger.EXPECT().Info("Checking failed bundles for blob transactions", "chain_id", chainID, "num_failed_bundles", tt.numFailedBundles)
				progressIndicator := utils.NewMockProgressIndicator(ctrl)
				progressIndicator.EXPECT().Add(1).Return(nil).Times(tt.numFailedBundles)
				progressIndicatorFactory.EXPECT().New(int64(tt.numFailedBundles), "Checking failed bundles").Return(progressIndicator)
			} else {
				logger.EXPECT().Warn("No failed bundles found", "chain_id", chainID)
			}
			if tt.rulesUpdateHeights != "" {
				logger.EXPECT().Info("Opening bundles in blocks from the Brio upgrade on", "chain_id", chainID, "brio_block", tt.brioBlock)
			} else {
				logger.EXPECT().Warn("No Brio upgrade found, bundles in blocks are not opened", "chain_id", chainID)
			}
			if len(tt.blocks) > 0 {
				logger.EXPECT().Info("Checking blocks for blob transactions", "chain_id", chainID, "start_block", uint64(0), "end_block", uint64(len(tt.blocks)-1))
				progressIndicator := utils.NewMockProgressIndicator(ctrl)
				progressIndicator.EXPECT().Add(1).Return(nil).Times(len(tt.blocks))
				progressIndicatorFactory.EXPECT().New(int64(len(tt.blocks)), "Checking blocks").Return(progressIndicator)
			} else {
				logger.EXPECT().Warn("No blocks found", "chain_id", chainID)
			}
			for _, location := range tt.wantLocations {
				logger.EXPECT().Error("Found blob transaction", "location", location)
			}
			if len(tt.wantLocations) == 0 {
				logger.EXPECT().Info("Found no blob transactions")
			}

			err := CheckBlobTxs(t.Context(), directory, chainID, logger, progressIndicatorFactory)
			if len(tt.wantLocations) == 0 {
				require.NoError(t, err)
			} else {
				require.EqualError(t, err, fmt.Sprintf("found %d blob transactions", len(tt.wantLocations)))
			}
		})
	}
}

func TestCheckBlobTxs_InvalidData_ReportsAnIssue(t *testing.T) {
	tests := map[string]struct {
		failedBundles      string
		rulesUpdateHeights string
		corruptBlock       bool
		tx                 *blockdb.Transaction
		wantErr            string
	}{
		"invalid failed bundles": {
			failedBundles: `not-json`,
			wantErr:       "failed to parse stored failed bundles",
		},
		"invalid rules update heights": {
			rulesUpdateHeights: `not-json`,
			wantErr:            "failed to parse stored rules update heights",
		},
		"unreadable block": {
			corruptBlock: true,
			wantErr:      "failed to get block",
		},
		"unsupported tx type": {
			tx:      &blockdb.Transaction{TransactionType: 99},
			wantErr: "failed to convert transaction 0 of block 1",
		},
		"invalid bundle": {
			tx:      &blockdb.Transaction{To: bundle.BundleProcessor.Bytes()},
			wantErr: "block 1, tx 0: failed to open bundle",
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(146)
			blocks := utils.CreateValidBlocks(t, 3)
			if tt.tx != nil {
				blocks[1].Transactions = []*blockdb.Transaction{tt.tx}
			}
			directory := createDatabase(t, chainID, blocks)
			db, writeOptions := openDatabaseForWriting(t, directory)
			if tt.failedBundles != "" {
				require.NoError(t, db.Put(writeOptions, blockdb.MakeFailedBundlesKey(chainID), []byte(tt.failedBundles)))
			}
			rulesUpdateHeights := cmp.Or(tt.rulesUpdateHeights, `[{"Block": 0, "Rules": {"Upgrades": {"Brio": true}}}]`)
			require.NoError(t, db.Put(writeOptions, blockdb.MakeRulesUpdateHeightsKey(chainID), []byte(rulesUpdateHeights)))
			db.Close()
			if tt.corruptBlock {
				corruptBlock(t, directory, chainID, 1)
			}

			ctrl := gomock.NewController(t)
			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Info("Opening block database", "directory", directory)
			progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
			if tt.failedBundles == "" {
				logger.EXPECT().Warn("No failed bundles found", "chain_id", chainID)
			}
			if tt.failedBundles == "" && tt.rulesUpdateHeights == "" {
				logger.EXPECT().Info("Opening bundles in blocks from the Brio upgrade on", "chain_id", chainID, "brio_block", uint64(0))
				logger.EXPECT().Info("Checking blocks for blob transactions", "chain_id", chainID, "start_block", uint64(0), "end_block", uint64(2))
				progressIndicator := utils.NewMockProgressIndicator(ctrl)
				progressIndicator.EXPECT().Add(1).Return(nil) // only block 0 is checked
				progressIndicatorFactory.EXPECT().New(int64(3), "Checking blocks").Return(progressIndicator)
			}

			require.ErrorContains(t, CheckBlobTxs(t.Context(), directory, chainID, logger, progressIndicatorFactory), tt.wantErr)
		})
	}
}

func TestCheckBlobTxs_CancelledContext_AbortsWithError(t *testing.T) {
	ctrl := gomock.NewController(t)
	directory := createDatabase(t, 146, utils.CreateValidBlocks(t, 1))
	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Info("Opening block database", "directory", directory)
	logger.EXPECT().Warn("No failed bundles found", "chain_id", uint64(146))
	logger.EXPECT().Warn("No Brio upgrade found, bundles in blocks are not opened", "chain_id", uint64(146))
	logger.EXPECT().Info("Checking blocks for blob transactions", "chain_id", uint64(146), "start_block", uint64(0), "end_block", uint64(0))
	progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
	progressIndicatorFactory.EXPECT().New(int64(1), "Checking blocks").Return(utils.NewMockProgressIndicator(ctrl))

	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	require.ErrorIs(t, CheckBlobTxs(ctx, directory, 146, logger, progressIndicatorFactory), context.Canceled)
}

func TestFindBlobTxs_FindsBlobTxsInNestedBundles(t *testing.T) {
	signer := types.LatestSignerForChainID(big.NewInt(1))
	plainTx := types.MustSignNewTx(newKey(t), signer, &types.LegacyTx{})
	blobTx := types.MustSignNewTx(newKey(t), signer, &types.BlobTx{})
	plainEnvelope, _ := wrapInBundle(t, signer, plainTx)
	blobEnvelope, bundledBlobTx := wrapInBundle(t, signer, blobTx)
	nestedEnvelope, bundledBlobEnvelope := wrapInBundle(t, signer, blobEnvelope)
	twoBlobsEnvelope, twoBlobsBundle, _ := bundle.NewBuilder().WithSigner(signer).AllOf(bundle.Step(newKey(t), blobTx), bundle.Step(newKey(t), blobTx)).BuildEnvelopeBundleAndPlan()
	var twoBlobsPaths []string
	for _, tx := range twoBlobsBundle.Transactions {
		twoBlobsPaths = append(twoBlobsPaths, " > bundled tx "+tx.Hash().Hex())
	}
	slices.Sort(twoBlobsPaths)

	tests := map[string]struct {
		tx         *types.Transaction
		beforeBrio bool
		want       []string
		wantErr    string
	}{
		"plain tx": {
			tx: plainTx,
		},
		"blob tx": {
			tx:   blobTx,
			want: []string{""},
		},
		"bundle without blob tx": {
			tx: plainEnvelope,
		},
		"blob tx in bundle": {
			tx:   blobEnvelope,
			want: []string{" > bundled tx " + bundledBlobTx.Hash().Hex()},
		},
		"bundle before Brio": {
			tx:         blobEnvelope,
			beforeBrio: true,
		},
		"blob txs in bundle, sorted by hash": {
			tx:   twoBlobsEnvelope,
			want: twoBlobsPaths,
		},
		"blob tx in nested bundle": {
			tx:   nestedEnvelope,
			want: []string{" > bundled tx " + bundledBlobEnvelope.Hash().Hex() + " > bundled tx " + bundledBlobTx.Hash().Hex()},
		},
		"invalid bundle": {
			tx:      types.NewTx(&types.LegacyTx{To: &bundle.BundleProcessor}),
			wantErr: "failed to open bundle",
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			got, err := findBlobTxs(signer, tt.tx, !tt.beforeBrio)
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
			} else {
				require.NoError(t, err)
			}
			require.Equal(t, tt.want, got)
		})
	}
}

// wrapInBundle returns an envelope of a bundle that contains only the given
// transaction, and the transaction as it is stored in the bundle.
func wrapInBundle(t *testing.T, signer types.Signer, tx *types.Transaction) (*types.Transaction, *types.Transaction) {
	t.Helper()
	envelope, txBundle, _ := bundle.NewBuilder().WithSigner(signer).With(bundle.Step(newKey(t), tx)).BuildEnvelopeBundleAndPlan()
	bundledTxs := slices.Collect(maps.Values(txBundle.Transactions))
	require.Len(t, bundledTxs, 1)
	return envelope, bundledTxs[0]
}

func newKey(t *testing.T) *ecdsa.PrivateKey {
	t.Helper()
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	return key
}
