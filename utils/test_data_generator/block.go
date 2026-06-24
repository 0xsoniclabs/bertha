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
	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
)

// blockHeaderFieldCases contains the corner cases for the fields of a block header.
var blockHeaderFieldCases = map[string][]any{
	"Difficulty": toAnySlice(getBigIntCases()),
	"Number":     toAnySlice(getBigIntCases()),
	"GasLimit":   toAnySlice(getUint64FieldCases()),
	"Time":       toAnySlice(getUint64FieldCases()),
	"Nonce":      toAnySlice(getBlockNonceFieldCases()),
	"Extra": {
		[]byte{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11}, // 12 bytes
	},
}

// generateBlockHeaders returns the corner cases for the fields of a block
func getBlockFieldCases() map[string][]any {
	var blockFields = make(map[string][]any)

	blockFields["Header"] = []any{}
	for _, block := range generateBlockHeaders() {
		blockFields["Header"] = append(blockFields["Header"], block)
	}

	// Add transactions and receipts as fields
	defaultFields := map[string][]any{
		"AccessList": {
			types.AccessList{},
			types.AccessList{
				types.AccessTuple{
					Address: common.Address{},
					StorageKeys: []common.Hash{
						{},
					},
				},
			},
		},
		"AuthList": {
			[]types.SetCodeAuthorization{},
			[]types.SetCodeAuthorization{
				{},
			},
		},
	}
	transactions := [][]*types.Transaction{
		{},
		generateTransactionsWithFieldsAndType(types.LegacyTxType, defaultFields),
		flattenSlice([][]*types.Transaction{
			generateTransactionsWithFieldsAndType(types.LegacyTxType, defaultFields),
			generateTransactionsWithFieldsAndType(types.AccessListTxType, defaultFields),
		}),
		flattenSlice([][]*types.Transaction{
			generateTransactionsWithFieldsAndType(types.LegacyTxType, defaultFields),
			generateTransactionsWithFieldsAndType(types.AccessListTxType, defaultFields),
			generateTransactionsWithFieldsAndType(types.DynamicFeeTxType, defaultFields),
			generateTransactionsWithFieldsAndType(types.BlobTxType, defaultFields),
			generateTransactionsWithFieldsAndType(types.SetCodeTxType, defaultFields),
		}),
	}
	// Generate matching receipts
	// For simplicity, we will alternate between including logs and not including logs in the receipts.
	includeLogs := false
	receipts := make([][]*types.Receipt, len(transactions))
	for i, txs := range transactions {
		receipts[i] = make([]*types.Receipt, len(txs))
		for j := range receipts[i] {
			receipts[i][j] =
				DefaultReceipt()
			if includeLogs {
				receipts[i][j].Logs =
					[]*types.Log{
						{Topics: []common.Hash{}},
					}
			}
			includeLogs = !includeLogs
			receipts[i][j].Bloom = types.CreateBloom(receipts[i][j])
		}

	}

	// Add to blockFields
	blockFields["Transactions"] = []any{}
	for _, txSlice := range transactions {
		blockFields["Transactions"] = append(blockFields["Transactions"], txSlice)
	}
	blockFields["Receipts"] = []any{}
	for _, receiptSlice := range receipts {
		blockFields["Receipts"] = append(blockFields["Receipts"], receiptSlice)
	}

	blockFields["Uncles"] = []any{
		[]*types.Header{},
		[]*types.Header{
			{},
		},
	}

	blockFields["Withdrawals"] = []any{
		[]*types.Withdrawal{},
		[]*types.Withdrawal{
			{},
		},
	}

	return blockFields
}

// BuildBlock construct a BlockWithReceipts from the given fields.
// Every fields are default-initialized if not provided.
func BuildBlock(fields []NamedField) BlockWithReceipts {
	header := &types.Header{}
	var receipts []*types.Receipt
	var body types.Body
	body.Transactions = []*types.Transaction{}
	body.Uncles = []*types.Header{}
	body.Withdrawals = []*types.Withdrawal{}

	for _, field := range fields {
		switch field.Name {
		case "Header":
			header = field.Value.(*types.Header)
		case "Transactions":
			body.Transactions = field.Value.([]*types.Transaction)
		case "Receipts":
			receipts = field.Value.([]*types.Receipt)
		case "Uncles":
			body.Uncles = field.Value.([]*types.Header)
		case "Withdrawals":
			body.Withdrawals = field.Value.([]*types.Withdrawal)
		}
	}
	// New block computes the transaction hash root and receipts root
	block := types.NewBlock(header, &body, receipts, trie.NewStackTrie(nil))
	return BlockWithReceipts{
		Block:    block,
		Receipts: []*types.Receipt(receipts),
	}
}

// toRustBlock converts a BlockWithReceipts to the Bertha Block type in Rust.
func toRustBlock(block BlockWithReceipts) string {
	blockData := block.Block
	receipts := block.Receipts

	baseFeePerGas := ""
	if blockData.Header().BaseFee != nil {
		baseFeePerGas = fmt.Sprintf("base_fee_per_gas: Some(U256::try_from_hex(\"%s\").unwrap()),\n", blockData.Header().BaseFee.Text(16))
	}
	withdrawalsRoot := ""
	if blockData.Header().WithdrawalsHash != nil {
		withdrawalsRoot = fmt.Sprintf("withdrawals_root: Some(Hash::try_from_hex(\"%s\").unwrap()),\n", blockData.Header().WithdrawalsHash.Hex())
	}
	blobGasUsed := ""
	if blockData.Header().BlobGasUsed != nil {
		blobGasUsed = fmt.Sprintf("blob_gas_used: Some(%d),\n", *blockData.Header().BlobGasUsed)
	}
	excessBlobGas := ""
	if blockData.Header().ExcessBlobGas != nil {
		excessBlobGas = fmt.Sprintf("excess_blob_gas: Some(%d),\n", *blockData.Header().ExcessBlobGas)
	}
	requestHash := ""
	if blockData.Header().RequestsHash != nil {
		requestHash = fmt.Sprintf("request_hash: Some(Hash::try_from_hex(\"%s\").unwrap())", blockData.Header().RequestsHash.Hex())
	}
	blockNonce, _ := blockData.Header().Nonce.MarshalText()
	transactions := toRustVector(blockData.Transactions(), func(tx *types.Transaction) string {
		return ToRustTransaction(tx)
	})
	rustReceipt := toRustVector(receipts, func(receipt *types.Receipt) string {
		return toRustReceipt(receipt)
	})
	withdrawals := toRustVector(blockData.Withdrawals(), func(w *types.Withdrawal) string {
		return toRustWithdrawal(w)
	})
	return fmt.Sprintf(
		`Block {
			parent_hash: Hash::try_from_hex("%s").unwrap(),
			ommers_hash: Hash::try_from_hex("%s").unwrap(),
			beneficiary: Address::try_from_hex("%s").unwrap(),
			state_root: Hash::try_from_hex("%s").unwrap(),
			difficulty: %s,
			number: %s,
			gas_limit: %d,
			timestamp: %d,
			extra_data: Vec::<u8>::try_from_hex("%s").unwrap(),
			prev_randao: Hash::try_from_hex("%s").unwrap(),
			nonce: Vec::<u8>::try_from_hex("%s").unwrap().try_into().unwrap(),
			transactions: %s,
			receipts: %s,
			withdrawals: %s,
			%s%s%s%s%s
			..Default::default()
	}`,
		blockData.Header().ParentHash.Hex(),
		blockData.Header().UncleHash.Hex(),
		blockData.Header().Coinbase.Hex(),
		blockData.Header().Root.Hex(),
		blockData.Header().Difficulty.Text(10),
		blockData.Header().Number.Text(10),
		blockData.Header().GasLimit,
		blockData.Header().Time,
		hexutil.Bytes(blockData.Header().Extra).String(),
		blockData.Header().MixDigest.Hex(),
		blockNonce,
		transactions,
		rustReceipt,
		withdrawals,
		baseFeePerGas,
		withdrawalsRoot,
		blobGasUsed,
		excessBlobGas,
		requestHash,
	)
}
