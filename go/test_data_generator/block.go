package main

import (
	"fmt"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
)

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
		blockNonce,
		toRustVector(blockData.Transactions(), func(tx *types.Transaction) string {
			return ToRustTransaction(tx)
		}),
		toRustVector(receipts, func(receipt *types.Receipt) string {
			return toRustReceipt(receipt)
		}),
		baseFeePerGas,
		withdrawalsRoot,
		blobGasUsed,
		excessBlobGas,
		requestHash,
	)
}
