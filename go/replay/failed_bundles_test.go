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
	"testing"

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"
)

func TestFailedBundles_UnmarshalJSON_GroupsByBlockInInputOrder(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	envelopes := make([]*types.Transaction, 4)
	for i := range envelopes {
		envelopes[i] = bundle.NewBuilder().
			SetEnvelopeNonce(uint64(i)).
			With(bundle.Step(key, &types.AccessListTx{})).
			Build()
	}

	data := fmt.Appendf(nil, `[
		{"block": 7, "pos": 0, "tx": "%s"},
		{"block": 7, "pos": 2, "tx": "%s"},
		{"block": 7, "pos": 2, "tx": "%s"},
		{"block": 9, "pos": 1, "tx": "%s"}
	]`, encodeTx(t, envelopes[0]), encodeTx(t, envelopes[1]), encodeTx(t, envelopes[2]), encodeTx(t, envelopes[3]))

	var failedBundles FailedBundles
	require.NoError(t, json.Unmarshal(data, &failedBundles))

	require.Equal(t, map[uint64][]failedBundleHash{
		7: {{0, envelopes[0].Hash()}, {2, envelopes[1].Hash()}, {2, envelopes[2].Hash()}},
		9: {{1, envelopes[3].Hash()}},
	}, failedBundleHashes(failedBundles))
}

func TestFailedBundles_UnmarshalJSON_ReportsIssues(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	tx := encodeTx(t, bundle.NewBuilder().With(bundle.Step(key, &types.AccessListTx{})).Build())

	tests := map[string]struct {
		content string
		wantErr string
	}{
		"invalid entry":       {content: `[{"block": "1"}]`, wantErr: "cannot unmarshal string"},
		"null entry":          {content: `[null]`, wantErr: "failed bundle 0 has no block"},
		"missing block":       {content: fmt.Sprintf(`[{"pos": 0, "tx": "%s"}]`, tx), wantErr: "failed bundle 0 has no block"},
		"missing position":    {content: fmt.Sprintf(`[{"block": 1, "tx": "%s"}]`, tx), wantErr: "failed bundle 0 has no position"},
		"invalid transaction": {content: fmt.Sprintf(`[{"block": 1, "pos": 0, "tx": "%s"}, {"block": 1, "pos": 2, "tx": "0x01"}]`, tx), wantErr: "invalid transaction of failed bundle 1 at block 1, position 2"},
		"unsorted blocks": {
			content: fmt.Sprintf(`[{"block": 2, "pos": 0, "tx": "%s"}, {"block": 1, "pos": 1, "tx": "%s"}]`, tx, tx),
			wantErr: "failed bundle 1 at block 1, position 1 is not sorted by block and position",
		},
		"unsorted positions": {
			content: fmt.Sprintf(`[{"block": 1, "pos": 2, "tx": "%s"}, {"block": 1, "pos": 1, "tx": "%s"}]`, tx, tx),
			wantErr: "failed bundle 1 at block 1, position 1 is not sorted by block and position",
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			var failedBundles FailedBundles
			require.ErrorContains(t, json.Unmarshal([]byte(tt.content), &failedBundles), tt.wantErr)
		})
	}
}

func TestInsertFailedBundles_InsertsEnvelopesBeforeTheTransactionAtTheirPosition(t *testing.T) {
	a := types.NewTx(&types.LegacyTx{Nonce: 1})
	b := types.NewTx(&types.LegacyTx{Nonce: 2})
	x := types.NewTx(&types.LegacyTx{Nonce: 3})
	y := types.NewTx(&types.LegacyTx{Nonce: 4})

	tests := map[string]struct {
		failedBundles []FailedBundle
		want          types.Transactions
		wantErr       string
	}{
		"no failed bundles":   {want: types.Transactions{a, b}},
		"first":               {failedBundles: []FailedBundle{{Pos: 0, Tx: x}}, want: types.Transactions{x, a, b}},
		"middle":              {failedBundles: []FailedBundle{{Pos: 1, Tx: x}}, want: types.Transactions{a, x, b}},
		"last":                {failedBundles: []FailedBundle{{Pos: 2, Tx: x}}, want: types.Transactions{a, b, x}},
		"same position":       {failedBundles: []FailedBundle{{Pos: 1, Tx: x}, {Pos: 1, Tx: y}}, want: types.Transactions{a, x, y, b}},
		"different positions": {failedBundles: []FailedBundle{{Pos: 0, Tx: x}, {Pos: 2, Tx: y}}, want: types.Transactions{x, a, b, y}},
		"beyond transactions": {failedBundles: []FailedBundle{{Pos: 3, Tx: x}}, wantErr: "position 3 of failed bundle exceeds the number of transactions 2"},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			got, err := insertFailedBundles(types.Transactions{a, b}, tt.failedBundles)
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			require.Equal(t, tt.want, got)
		})
	}
}

func TestBlockReceipts_ReturnsReceiptsUnlessTxsAreSkippedOrCausedByFailedBundles(t *testing.T) {
	txA := types.NewTx(&types.LegacyTx{Nonce: 1})
	txB := types.NewTx(&types.LegacyTx{Nonce: 2})
	failedBundle := types.NewTx(&types.LegacyTx{Nonce: 3})
	succeededBundle := types.NewTx(&types.LegacyTx{Nonce: 4})
	receiptA := &types.Receipt{TxHash: txA.Hash()}
	receiptB := &types.Receipt{TxHash: txB.Hash()}

	tests := map[string]struct {
		processed []evmcore.ProcessedTransaction
		causedBy  map[common.Hash]common.Hash
		want      types.Receipts
		wantErr   string
	}{
		"receipts": {
			processed: []evmcore.ProcessedTransaction{{Transaction: txA, Receipt: receiptA}, {Transaction: txB, Receipt: receiptB}},
			causedBy:  map[common.Hash]common.Hash{txA.Hash(): txA.Hash(), txB.Hash(): succeededBundle.Hash()},
			want:      types.Receipts{receiptA, receiptB},
		},
		"skipped tx": {
			processed: []evmcore.ProcessedTransaction{{Transaction: txA}},
			wantErr:   fmt.Sprintf("tx %v was skipped", txA.Hash()),
		},
		"skipped failed bundle": {
			processed: []evmcore.ProcessedTransaction{{Transaction: failedBundle}},
			wantErr:   fmt.Sprintf("failed bundle %v was skipped", failedBundle.Hash()),
		},
		"receipt of failed bundle": {
			processed: []evmcore.ProcessedTransaction{{Transaction: txA, Receipt: receiptA}},
			causedBy:  map[common.Hash]common.Hash{txA.Hash(): failedBundle.Hash()},
			wantErr:   fmt.Sprintf("failed bundle %v added a receipt for tx %v", failedBundle.Hash(), txA.Hash()),
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			processed := evmcore.ProcessSummary{ProcessedTransactions: tt.processed, CausedBy: tt.causedBy}
			got, err := blockReceipts(processed, []FailedBundle{{Tx: failedBundle}})
			if tt.wantErr != "" {
				require.EqualError(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			require.Equal(t, tt.want, got)
		})
	}
}

type failedBundleHash struct {
	Pos  uint64
	Hash common.Hash
}

// failedBundleHashes replaces the txs by their hashes, as decoded txs differ in
// their decoding time.
func failedBundleHashes(failedBundles FailedBundles) map[uint64][]failedBundleHash {
	hashes := map[uint64][]failedBundleHash{}
	for block, bundles := range failedBundles {
		for _, b := range bundles {
			hashes[block] = append(hashes[block], failedBundleHash{b.Pos, b.Tx.Hash()})
		}
	}
	return hashes
}

func encodeTx(t *testing.T, tx *types.Transaction) string {
	data, err := tx.MarshalBinary()
	require.NoError(t, err)
	return hexutil.Encode(data)
}
