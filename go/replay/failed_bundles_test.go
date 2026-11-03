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
