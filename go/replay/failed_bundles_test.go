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
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"
)

func TestReadFailedBundlesFromFile_GroupsByBlockAndSortsByPosition(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	envelopes := make([]*types.Transaction, 4)
	for i := range envelopes {
		envelopes[i] = bundle.NewBuilder().
			SetEnvelopeNonce(uint64(i)).
			With(bundle.Step(key, &types.AccessListTx{})).
			Build()
	}

	path := filepath.Join(t.TempDir(), "failed_bundles.json")
	require.NoError(t, os.WriteFile(path, fmt.Appendf(nil, `[
		{"block": 7, "pos": 2, "tx": "%s"},
		{"block": 9, "pos": 1, "tx": "%s"},
		{"block": 7, "pos": 0, "tx": "%s"},
		{"block": 7, "pos": 2, "tx": "%s"}
	]`, encodeTx(t, envelopes[0]), encodeTx(t, envelopes[1]), encodeTx(t, envelopes[2]), encodeTx(t, envelopes[3])), 0644))

	failedBundles, err := ReadFailedBundlesFromFile(path)
	require.NoError(t, err)

	type entry struct {
		Pos  uint64
		Hash common.Hash
	}
	got := map[uint64][]entry{}
	for block, bundles := range failedBundles {
		for _, b := range bundles {
			got[block] = append(got[block], entry{b.Pos, b.Tx.Hash()})
		}
	}
	require.Equal(t, map[uint64][]entry{
		7: {{0, envelopes[2].Hash()}, {2, envelopes[0].Hash()}, {2, envelopes[3].Hash()}},
		9: {{1, envelopes[1].Hash()}},
	}, got)
}

func TestReadFailedBundlesFromFile_ReportsIssues(t *testing.T) {
	tests := map[string]struct {
		content string
		wantErr string
	}{
		"missing file":        {wantErr: "failed to read failed bundles file"},
		"invalid JSON":        {content: `[{`, wantErr: "failed to parse failed bundles"},
		"invalid transaction": {content: `[{"block": 1, "pos": 0, "tx": "0x01"}]`, wantErr: "invalid transaction of failed bundle in block 1"},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "failed_bundles.json")
			if tt.content != "" {
				require.NoError(t, os.WriteFile(path, []byte(tt.content), 0644))
			}
			_, err := ReadFailedBundlesFromFile(path)
			require.ErrorContains(t, err, tt.wantErr)
		})
	}
}

func encodeTx(t *testing.T, tx *types.Transaction) string {
	data, err := tx.MarshalBinary()
	require.NoError(t, err)
	return hexutil.Encode(data)
}
