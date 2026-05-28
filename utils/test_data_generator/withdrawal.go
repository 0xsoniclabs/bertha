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

package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// withdrawalFieldCases contains the corner cases for the fields of a withdrawal.
var withdrawalFieldCases = map[string][]any{
	"Index":     toAnySlice(getUint64FieldCases()),
	"Validator": toAnySlice(getUint64FieldCases()),
	"Address": {
		common.Address{},
		common.HexToAddress("0x0102030405060708090a0b0c0d0e0f1011121314"),
	},
	"Amount": toAnySlice(getUint64FieldCases()),
}

// generateWithdrawals generates a slice of withdrawals for each combination of [withdrawalFieldCases].
func generateWithdrawals() []*types.Withdrawal {
	return generateWithdrawalsWithFields(withdrawalFieldCases)
}

// generateWithdrawalsWithFields generates a slice of withdrawals for each combination of fields.
func generateWithdrawalsWithFields(fields map[string][]any) []*types.Withdrawal {
	withdrawals := generateStruct(func() *types.Withdrawal { return &types.Withdrawal{} }, fields)
	return seqToSlice(withdrawals)
}

// toRustWithdrawal converts a geth Withdrawal to a Rust Withdrawal struct string.
func toRustWithdrawal(w *types.Withdrawal) string {
	return fmt.Sprintf(
		`Withdrawal {
			index: %d,
			validator_index: %d,
			address: Address::try_from_hex("%s").unwrap(),
			amount: %d,
		}`,
		w.Index,
		w.Validator,
		w.Address.Hex(),
		w.Amount,
	)
}
