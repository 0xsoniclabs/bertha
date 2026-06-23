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

package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// transactionReceiptFieldCases contains the corner cases for the fields of a transaction receipt.
var transactionReceiptFieldCases = map[string][]any{
	"PostState": {
		[]byte{},
		[]byte{
			0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
		},
	},
	"Status": {
		uint64(0),
		uint64(1),
	},
	"Logs": {
		[]*types.Log{},
		generateLogs(),
	},
	"CumulativeGasUsed": toAnySlice(getUint64FieldCases()),
}

// logFieldCases contains the corner cases for the fields of a log.
var logFieldCases = map[string][]any{
	"Topics": {
		[]common.Hash{},
		[]common.Hash{
			{},
		},
	},
	"Data": {
		[]byte{},
		[]byte{0x1},
	},
}

// toRustReceipt converts a Go transaction receipt to the Bertha TransactionReceipt type in Rust.
func toRustReceipt(receipt *types.Receipt) string {
	var postStateOrStatus string
	if len(receipt.PostState) == 32 {
		postStateOrStatus =
			fmt.Sprintf(`PostStateOrStatus::PostState(%s)`, toRustByteArray(receipt.PostState))
	} else {
		postStateOrStatus =
			fmt.Sprintf(`PostStateOrStatus::Status(%d)`, receipt.Status)
	}
	return fmt.Sprintf(`TransactionReceipt {
		transaction_type: TransactionType::try_from(%d).unwrap(),
		post_state_or_status: %s,
		cumulative_gas_used: %d,
		logs: %s
		}
		`,
		receipt.Type,
		postStateOrStatus,
		receipt.CumulativeGasUsed,
		toRustLogList(receipt.Logs),
	)
}

// toRustBloom converts a Go bloom filter to the Bertha Bloom type in Rust.
func toRustBloom(r *types.Receipt) string {
	return toRustByteArray(r.Bloom.Bytes())
}

func DefaultReceipt() *types.Receipt {
	return &types.Receipt{
		PostState: []byte{},
		Status:    1,
		Logs:      []*types.Log{},
	}
}
