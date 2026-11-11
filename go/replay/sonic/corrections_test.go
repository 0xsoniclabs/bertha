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

package sonic

import (
	"encoding/json"
	"fmt"
	"math/big"
	"strings"
	"testing"

	"github.com/0xsoniclabs/bertha/replay/state"
	"github.com/0xsoniclabs/bertha/utils"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
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
		"short string": {
			input:    "0x1234",
			expected: [32]byte{30: 0x12, 31: 0x34},
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

func TestApplyCorrections_AppliesCorrectionsToStateDB(t *testing.T) {
	blockNumber := uint64(17)
	address := common.Address{1}
	oldBalance := amount.New(0)
	newBalance := uint256.NewInt(1000)
	balanceDiff := amount.New(1000)
	storageKey := cc.Key{1}
	oldStorageValue := cc.Value{2}
	newStorageValue := cc.Value{3}

	corrections := Corrections{
		blockNumber: map[common.Address]Correction{
			address: {
				Balance: newBalance,
				Storage: map[cc.Key]cc.Value{
					storageKey: newStorageValue,
				},
			},
		},
	}

	ctrl := gomock.NewController(t)
	stateDB := carmen.NewMockVmStateDB(ctrl)
	logger := utils.NewMockLogger(ctrl)

	stateDB.EXPECT().BeginTransaction().Times(1)
	stateDB.EXPECT().GetBalance(cc.Address(address)).Return(oldBalance).Times(2)
	stateDB.EXPECT().AddBalance(cc.Address(address), balanceDiff).Times(1)
	stateDB.EXPECT().GetState(cc.Address(address), storageKey).Return(oldStorageValue).Times(1)
	stateDB.EXPECT().SetState(cc.Address(address), storageKey, newStorageValue).Times(1)
	stateDB.EXPECT().EndTransaction().Times(1)

	logger.EXPECT().Info("Applying corrections", "block", blockNumber).Times(1)
	logger.EXPECT().Info("Correcting balance",
		"address", address.Hex(),
		"old", oldBalance.ToBig().String(),
		"new", newBalance.ToBig().String(),
	).Times(1)
	logger.EXPECT().Info("Correcting storage",
		"address", address.Hex(),
		"storage_keys", len(corrections[blockNumber][address].Storage),
	).Times(1)
	logger.EXPECT().Info("Correcting storage entry",
		"address", address.Hex(),
		"key", fmt.Sprintf("0x%x", storageKey[:]),
		"old", fmt.Sprintf("0x%x", oldStorageValue[:]),
		"new", fmt.Sprintf("0x%x", newStorageValue[:]),
	).Times(1)

	block := types.NewBlockWithHeader(&types.Header{Number: new(big.Int).SetUint64(blockNumber)})
	applyCorrections(stateDB, corrections[blockNumber], block, logger)
}

func TestSetBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := state.NewState(state.StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	addr := common.Address{1}
	balance := big.NewInt(1000)
	setBalance(state.DB, addr, balance)

	// Check initial balance
	have := state.DB.GetBalance(cc.Address(addr))
	require.Equal(t, balance.Uint64(), have.Uint64())

	// Increase balance
	balance = big.NewInt(1500)
	setBalance(state.DB, addr, balance)
	have = state.DB.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(1500), have.Uint64())

	// Decrease balance
	balance = big.NewInt(750)
	setBalance(state.DB, addr, balance)
	have = state.DB.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(750), have.Uint64())
}
