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

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

func TestCorrection_JSONRoundTrip(t *testing.T) {
	tests := map[string]struct {
		correction Correction
		json       string
	}{
		"non-zero balance": {
			correction: Correction{Balance: *uint256.NewInt(12345)},
			json:       `{"Balance":"12345"}`,
		},
		"zero balance": {
			correction: Correction{Balance: *uint256.NewInt(0)},
			json:       `{"Balance":"0"}`,
		},
		"missing balance defaults to zero": {
			correction: Correction{},
			json:       `{}`,
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got, err := json.Marshal(tc.correction)
			require.NoError(t, err)
			if tc.json != `{}` {
				require.JSONEq(t, tc.json, string(got))
			}

			var decoded Correction
			require.NoError(t, json.Unmarshal([]byte(tc.json), &decoded))
			require.Equal(t, tc.correction, decoded)
		})
	}
}

func TestCorrections_JSONRoundTrip_WorksWithMapValues(t *testing.T) {
	// Map values are not addressable, so this exercises the custom JSON methods
	// working correctly when Correction is used as a map value.
	original := Corrections{
		5: {
			common.HexToAddress("0xabc"): {Balance: *uint256.NewInt(999)},
		},
	}

	data, err := json.Marshal(original)
	require.NoError(t, err)

	var decoded Corrections
	require.NoError(t, json.Unmarshal(data, &decoded))
	require.Equal(t, original, decoded)
}
