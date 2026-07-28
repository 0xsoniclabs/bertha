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

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

var (
	key1   = cc.Key{0x01, 0x02, 0x03}
	value1 = cc.Value{0x0a, 0x0b, 0x0c}
)

func TestCorrection_JSONRoundTrip(t *testing.T) {
	tests := map[string]struct {
		correction Correction
		json       string
	}{
		"non-zero values": {
			correction: Correction{Balance: uint256.NewInt(12345), Storage: map[cc.Key]cc.Value{key1: value1}},
			json:       fmt.Sprintf(`{"Balance":"12345","Storage":{"0x%x":"0x%x"}}`, key1[:], value1[:]),
		},
		"only balance": {
			correction: Correction{Balance: uint256.NewInt(67890)},
			json:       `{"Balance":"67890"}`,
		},
		"only storage": {
			correction: Correction{Storage: map[cc.Key]cc.Value{key1: value1}},
			json:       fmt.Sprintf(`{"Storage":{"0x%x":"0x%x"}}`, key1[:], value1[:]),
		},
		"missing values": {
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
			common.HexToAddress("0xabc"): {Balance: uint256.NewInt(999)},
		},
		7: {
			common.HexToAddress("0xdef"): {Storage: map[cc.Key]cc.Value{key1: value1}},
		},
		9: {
			common.HexToAddress("0x123"): {Balance: uint256.NewInt(123), Storage: map[cc.Key]cc.Value{key1: value1}},
		},
	}

	data, err := json.Marshal(original)
	require.NoError(t, err)

	var decoded Corrections
	require.NoError(t, json.Unmarshal(data, &decoded))
	require.Equal(t, original, decoded)
}

func TestCorrections_decodeFixed_DecodesStringCorrectly(t *testing.T) {
	tests := map[string]struct {
		input    string
		size     int
		expected []byte
	}{
		"exact size": {
			input:    "0x01020304",
			size:     4,
			expected: []byte{0x01, 0x02, 0x03, 0x04},
		},
		"bigger size": {
			input:    "0x050607",
			size:     5,
			expected: []byte{0x00, 0x00, 0x05, 0x06, 0x07},
		},
		"no prefix": {
			input:    "0a0b0c",
			size:     3,
			expected: []byte{0x0a, 0x0b, 0x0c},
		},
		"empty string": {
			input:    "",
			size:     2,
			expected: []byte{0x00, 0x00},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got, err := decodeFixed(tc.input, tc.size)
			require.NoError(t, err)
			require.Equal(t, tc.expected, got)
		})
	}
}

func TestCorrections_decodeFixed_ReturnsErrors(t *testing.T) {
	tests := map[string]struct {
		input string
		size  int
	}{
		"odd length": {
			input: "0x123",
			size:  2,
		},
		"too big": {
			input: "0x01020304",
			size:  3,
		},
		"invalid hex": {
			input: "0xZZ",
			size:  2,
		},
		"negative size": {
			input: "0x01",
			size:  -1,
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := decodeFixed(tc.input, tc.size)
			require.Error(t, err)
		})
	}
}
