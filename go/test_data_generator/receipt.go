package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/core/types"
)

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
	bloom := r.Bloom.Bytes()
	return toRustByteArray(bloom)
}
