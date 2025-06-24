package main

import (
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/core/types"
)

func ToRustTransaction(tx *types.Transaction) string {
	to := "None"
	if tx.To() != nil {
		to = fmt.Sprintf("Some(Address::try_from_hex(\"%s\").unwrap())", tx.To().Hex())
	}
	v, r, s := tx.RawSignatureValues()
	blobGasFeeCap := tx.BlobGasFeeCap()
	if blobGasFeeCap == nil {
		blobGasFeeCap = new(big.Int)
	}
	return fmt.Sprintf(
		`Transaction {
			transaction_type: TransactionType::try_from(%d).unwrap(),
			chain_id: U256::try_from_hex("%s").unwrap(),
			nonce: %d,
			gas_price: U256::try_from_hex("%s").unwrap(),
			gas_limit: %d,
			to: %s,
			value: U256::try_from_hex("%s").unwrap(),
			data: %s,
			access_list: %s,
			max_fee_per_gas: U256::try_from_hex("%s").unwrap(),
			max_priority_fee_per_gas: U256::try_from_hex("%s").unwrap(),
			blob_versioned_hashes: %s,
			max_fee_per_blob_gas: U256::try_from_hex("%s").unwrap(),
			authorization_list: %s,
			y_parity: U256::try_from_hex("%s").unwrap(),
			r: U256::try_from_hex("%s").unwrap(),
			s: U256::try_from_hex("%s").unwrap(),
		}`,
		tx.Type(),
		tx.ChainId().Text(16),
		tx.Nonce(),
		tx.GasPrice().Text(16),
		tx.Gas(),
		to,
		tx.Value().Text(16),
		toRustByteVec(tx.Data()),
		toRustAccessList(tx.AccessList()),
		tx.GasFeeCap().Text(16),
		tx.GasTipCap().Text(16),
		toRustHashList(tx.BlobHashes()),
		blobGasFeeCap.Text(16),
		toRustAuthorizationList(tx.SetCodeAuthorizations()),
		v.Text(16),
		r.Text(16),
		s.Text(16),
	)
}
