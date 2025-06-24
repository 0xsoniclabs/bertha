package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
)

func toRustBlock(block BlockWithReceipts) string {
	blockData := block.Block
	receipts := block.Receipts

	base_fee_per_gas := ""
	if blockData.Header().BaseFee != nil {
		base_fee_per_gas = fmt.Sprintf("base_fee_per_gas: Some(U256::try_from_hex(\"%s\").unwrap()),\n", blockData.Header().BaseFee.Text(16))
	}
	withdrawals_root := ""
	if blockData.Header().WithdrawalsHash != nil {
		withdrawals_root = fmt.Sprintf("withdrawals_root: Some(Hash::try_from_hex(\"%s\").unwrap()),\n", blockData.Header().WithdrawalsHash.Hex())
	}
	blob_gas_used := ""
	if blockData.Header().BlobGasUsed != nil {
		blob_gas_used = fmt.Sprintf("blob_gas_used: Some(%d),\n", *blockData.Header().BlobGasUsed)
	}
	excess_blob_gas := ""
	if blockData.Header().ExcessBlobGas != nil {
		excess_blob_gas = fmt.Sprintf("excess_blob_gas: Some(%d),\n", *blockData.Header().ExcessBlobGas)
	}
	request_hash := ""
	if blockData.Header().RequestsHash != nil {
		request_hash = fmt.Sprintf("request_hash: Some(Hash::try_from_hex(\"%s\").unwrap())", blockData.Header().RequestsHash.Hex())
	}
	block_nonce, _ := blockData.Header().Nonce.MarshalText()
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
		block_nonce,
		toRustVector(blockData.Transactions(), func(tx *types.Transaction) string {
			return ToRustTransaction(tx)
		}),
		toRustVector(receipts, func(receipt *types.Receipt) string {
			return toRustReceipt(receipt)
		}),
		base_fee_per_gas,
		withdrawals_root,
		blob_gas_used,
		excess_blob_gas,
		request_hash,
	)
}
