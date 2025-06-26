package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// transactionReceiptFieldCases contains the corner cases for the fields of a transaction receipt.
var transactionReceiptFieldCases = map[string][]any{
	"Logs": {
		[]*types.Log{},
		generateLogs(),
	},
	"CumulativeGasUsed": toAnySlice(getUint64FieldCases()),
}

// logFieldCases contains the corner cases for the fields of a log.
var logFieldCases = map[string][]any{
	"BlockNumber": toAnySlice(getUint64FieldCases()),
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
	return fmt.Sprintf(`TransactionReceipt {
		transaction_type: TransactionType::try_from(%d).unwrap(),
		status: %d,
		cumulative_gas_used: %d,
		logs: %s
		}
		`,
		receipt.Type,
		receipt.Status,
		receipt.CumulativeGasUsed,
		toRustLogList(receipt.Logs),
	)
}

// toRustBloom converts a Go bloom filter to the Bertha Bloom type in Rust.
func toRustBloom(r *types.Receipt) string {
	return toRustByteArray(r.Bloom.Bytes())
}
