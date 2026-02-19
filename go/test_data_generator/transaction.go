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
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// transactionFieldCases contains the corner cases for the fields of a transaction.
var transactionFieldCases = map[string][]any{
	"Nonce":      toAnySlice(getUint64FieldCases()),
	"GasPrice":   toAnySlice(getBigIntCases()),
	"Gas":        toAnySlice(getUint64FieldCases()),
	"BlobFeeCap": toAnySlice(getUint256FieldCases()),
	"To": {
		new(common.Address),
	},
	"AccessList": {
		types.AccessList{},
		types.AccessList{
			types.AccessTuple{
				Address:     common.Address{},
				StorageKeys: []common.Hash{},
			},
		},
		types.AccessList{
			types.AccessTuple{
				Address: common.Address{},
				StorageKeys: []common.Hash{
					{},
				},
			},
		},
	},
	"Data": {
		[]byte{},
		[]byte{0x1},
	},

	"BlobHashes": {
		[]common.Hash{},
		[]common.Hash{
			{},
		},
	},
	"AuthList": {
		[]types.SetCodeAuthorization{},
		[]types.SetCodeAuthorization{
			{},
		},
	},
}

var legacyAndAccessListFields = map[string][]any{
	"To": {
		(*common.Address)(nil),
		new(common.Address),
	},
}

var dynamicFeeFields = map[string][]any{
	"GasTipCap": toAnySlice(getBigIntCases()),
	"GasFeeCap": toAnySlice(getBigIntCases()),
	"Value":     toAnySlice(getBigIntCases()),
}

var blobAndSetCodeFields = map[string][]any{
	"GasTipCap": toAnySlice(getUint256FieldCases()),
	"GasFeeCap": toAnySlice(getUint256FieldCases()),
	"Value":     toAnySlice(getUint256FieldCases()),
}

func getLegacyAndAccessListFields() map[string][]any {
	return insertMap(copyMap(transactionFieldCases), legacyAndAccessListFields)
}

func getDynamicFeeFields() map[string][]any {
	return insertMap(copyMap(transactionFieldCases), dynamicFeeFields)
}

func getBlobAndSetCodeFields() map[string][]any {
	return insertMap(copyMap(transactionFieldCases), blobAndSetCodeFields)
}

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
