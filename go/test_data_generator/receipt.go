package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// transactionReceiptFieldCases contains the corner cases for the fields of a transaction receipt.
var transactionReceiptFieldCases = map[string][]any{
	"Type": {
		uint8(types.LegacyTxType),
		uint8(types.AccessListTxType),
		uint8(types.DynamicFeeTxType),
		uint8(types.BlobTxType),
		uint8(types.SetCodeTxType),
	},
	"Logs": {
		[]*types.Log{},
		generateLogs(),
	},
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

func toRustBloom(r *types.Receipt) string {
	return toRustByteArray(r.Bloom.Bytes())
}
