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
	"strings"
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
	input := "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
	expected := [32]byte{0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
		0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
		0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20}

	tests := map[string]struct {
		input    string
		expected [32]byte
	}{
		"lowercase": {
			input:    input,
			expected: expected,
		},
		"uppercase": {
			input:    strings.ToUpper(input),
			expected: expected,
		},
		"no prefix": {
			input:    strings.TrimPrefix(input, "0x"),
			expected: expected,
		},
		"empty string": {
			input:    "",
			expected: [32]byte{},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got, err := decodeHexTo32Bytes(tc.input)
			require.NoError(t, err)
			require.Equal(t, tc.expected, got)
		})
	}
}

func TestCorrections_decodeFixed_ReturnsErrors(t *testing.T) {
	tests := map[string]struct {
		input string
	}{
		"odd length": {
			input: "0x123",
		},
		"too big": {
			input: "0x" + strings.Repeat("01", 33),
		},
		"invalid hex": {
			input: "0x" + strings.Repeat("zz", 32),
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := decodeHexTo32Bytes(tc.input)
			require.Error(t, err)
		})
	}
}
