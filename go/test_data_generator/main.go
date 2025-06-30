package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math/big"
	"os"
	"os/exec"
	"strings"

	geth "github.com/0xsoniclabs/bertha/go/test_data_generator/geth_files"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/trie"
)

// generateTransactions generates a slice of transactions for each combination of [transaction::transactionFieldCases].
func generateTransactions() []*types.Transaction {
	txs := []*types.Transaction{}
	for payload := range generateStruct(func() *types.LegacyTx { return &types.LegacyTx{} }, getLegacyAndAccessListFields()) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.AccessListTx { return &types.AccessListTx{} }, getLegacyAndAccessListFields()) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.DynamicFeeTx { return &types.DynamicFeeTx{} }, getDynamicFeeFields()) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.BlobTx { return &types.BlobTx{} }, getBlobAndSetCodeFields()) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.SetCodeTx { return &types.SetCodeTx{} }, getBlobAndSetCodeFields()) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	return txs
}

// generateTransactionsWithFieldsAndType generates a slice of transactions of type txType for each combination of fields.
func generateTransactionsWithFieldsAndType(txType uint8, fields map[string][]any) []*types.Transaction {
	var txs []*types.Transaction
	for payload := range generateStruct(func() any {
		switch txType {
		case types.LegacyTxType:
			return &types.LegacyTx{}
		case types.AccessListTxType:
			return &types.AccessListTx{}
		case types.DynamicFeeTxType:
			return &types.DynamicFeeTx{}
		case types.BlobTxType:
			return &types.BlobTx{}
		case types.SetCodeTxType:
			return &types.SetCodeTx{}
		default:
			panic(fmt.Sprintf("Unknown transaction type: %d", txType))
		}
	}, fields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload.(types.TxData), getTransactionSignatureKey()))
	}
	return txs
}

// generateTransactionsReceipts generates a slice of receipts for each combination of [receipt::transactionReceiptFieldCases].
func generateTransactionsReceipts() []*types.Receipt {
	return generateTransactionsReceiptsWithFields(transactionReceiptFieldCases)
}

// generateTransactionsReceiptsWithFields generates a slice of receipts for each combination of fields.
func generateTransactionsReceiptsWithFields(fields map[string][]any) []*types.Receipt {
	receiptsSlice := []*types.Receipt{}
	receiptsSlice = append(receiptsSlice, generateTransactionReceiptsWithFieldsAndType(types.LegacyTxType, fields)...)
	receiptsSlice = append(receiptsSlice, generateTransactionReceiptsWithFieldsAndType(types.AccessListTxType, fields)...)
	receiptsSlice = append(receiptsSlice, generateTransactionReceiptsWithFieldsAndType(types.DynamicFeeTxType, fields)...)
	receiptsSlice = append(receiptsSlice, generateTransactionReceiptsWithFieldsAndType(types.BlobTxType, fields)...)
	receiptsSlice = append(receiptsSlice, generateTransactionReceiptsWithFieldsAndType(types.SetCodeTxType, fields)...)
	return receiptsSlice
}

// generateTransactionReceiptsWithFieldsAndType generates a slice of receipts of type txType for each combination of fields.
func generateTransactionReceiptsWithFieldsAndType(txType uint8, fields map[string][]any) []*types.Receipt {
	receipts := []*types.Receipt{}
	for receipt := range generateStruct(func() *types.Receipt {
		return &types.Receipt{
			Type:   txType,
			Logs:   []*types.Log{},
			Status: 1,
		}
	}, fields) {
		// Compute bloom
		receipt.Bloom = types.CreateBloom(receipt)
		receipts = append(receipts, receipt)
	}
	return receipts
}

// generateBlockHeaders generates a slice of block headers for each combination of [block::blockHeaderFieldCases].
func generateBlockHeaders() []*types.Header {
	return generateBlockHeadersWithFields(blockHeaderFieldCases)
}

// generateBlockHeadersWithFields generates a slice of block headers for each combination of fields.
func generateBlockHeadersWithFields(fields map[string][]any) []*types.Header {
	blocks := generateStruct(func() *types.Header { return &types.Header{} }, fields)
	return seqToSlice(blocks)
}

// generateLogs generates a slice of logs for each combination of [receipt::logFieldCases].
func generateLogs() []*types.Log {
	return generateLogsWithFields(logFieldCases)
}

// generateLogsWithFields generates a slice of logs for each combination of fields.
func generateLogsWithFields(fields map[string][]any) []*types.Log {
	logs := generateStruct(func() *types.Log {
		return &types.Log{
			Topics: []common.Hash{},
		}
	}, fields)
	return seqToSlice(logs)
}

// generateBlocks generates a slice of blocks with receipts for each combination of [block::getBlockFieldCases].
// The generated blocks follow the following rules:
// 1. The parent hash of each block is set to the previous block's hash.
// 2. The number of transactions in each block matches the number of receipts.
func generateBlocks() []BlockWithReceipts {
	blockFields := getBlockFieldCases()
	blocks := generateDataWithMaxLengthCombination(blockFields, BuildBlock)

	// remove all blocks with mismatched receipt and transactions count
	filteredBlocks := []BlockWithReceipts{}
	for block := range blocks {
		if len(block.Block.Transactions()) == len(block.Receipts) {
			filteredBlocks = append(filteredBlocks, block)
		}
	}

	for i := 0; i < len(filteredBlocks); i++ {
		newHeader := filteredBlocks[i].Block.Header()
		// Set the parent hash for each block to the previous block's hash
		if i == 0 {
			newHeader.ParentHash = common.Hash{}
		} else {
			newHeader.ParentHash = filteredBlocks[i-1].Block.Hash()
		}
		// Set the header gas used
		if len(filteredBlocks[i].Receipts) > 0 {
			newHeader.GasUsed = filteredBlocks[i].Receipts[len(filteredBlocks[i].Receipts)-1].GasUsed
		} else {
			newHeader.GasUsed = 0
		}

		filteredBlocks[i] = BlockWithReceipts{
			Block:    types.NewBlock(newHeader, filteredBlocks[i].Block.Body(), filteredBlocks[i].Receipts, trie.NewStackTrie(nil)),
			Receipts: filteredBlocks[i].Receipts,
		}
	}

	return filteredBlocks
}

// dumpTransactions generates a Rust iterator over transactions with test data.
// It includes:
// 1. The transaction
// 2. The transaction byte marshalling
// 3. The transaction JSON representation
func dumpTransactions(data []*types.Transaction) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	#![allow(dead_code)]

	use crate::{AccessListEntry, Address,Hash, HexConvert, SetCodeAuthorization, Transaction, TransactionType, U256};

	`
	// Compute transaction root hash
	block := types.NewBlock(&types.Header{}, &types.Body{Transactions: data}, nil, trie.NewStackTrie(nil))
	transactionRoot := block.Header().TxHash.Hex()
	rustCode += fmt.Sprintf("pub const TRANSACTION_ROOT : &str = \"%s\";\n", transactionRoot)
	// Generate the Transaction struct
	rustCode += `
	#[derive(Debug, Clone)]
	pub struct TransactionWithTestData {
		pub transaction: Transaction,
		pub rlp_encoding: Vec<u8>,
		pub json_representation: String,
	}

	`
	rustCode += "pub fn generate_transactions_with_data() -> impl IntoIterator<Item = TransactionWithTestData> {\n"
	rustCode += "[\n"
	for _, tx := range data {
		rustTx := ToRustTransaction(tx)
		encoding, _ := tx.MarshalBinary()
		jsonRepresentation, _ := json.Marshal(geth.NewRPCTransactionWithoutBlock(tx))
		rustCode += "TransactionWithTestData {\n"
		rustCode += fmt.Sprintf("\t\ttransaction: %s,\n", rustTx)
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(encoding))
		rustCode += fmt.Sprintf("\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}"
	return rustCode
}

// dumpReceipts generates a Rust iterator over receipts with test data.
// It includes:
// 1. The receipt
// 2. The receipt bloom
// 3. The receipt byte marshalling
// 4. The receipt JSON representation
func dumpReceipts(data []*types.Receipt) string {
	rustCode := `
    // This file is generated by go/test_data_generator/main.go
    #![allow(dead_code)]

    use crate::{Address, Hash, Bloom, HexConvert, Log, TransactionReceipt, TransactionType};

    `
	// Compute receipts root hash
	block := types.NewBlock(&types.Header{}, &types.Body{}, data, trie.NewStackTrie(nil))
	receiptsRoot := block.Header().ReceiptHash.Hex()
	rustCode += fmt.Sprintf("pub const RECEIPTS_ROOT : &str = \"%s\";\n\n", receiptsRoot)
	// Generate the TransactionReceipt struct
	rustCode += `
	#[derive(Debug, Clone)]
	pub struct TransactionReceiptWithTestData {
		pub receipt: TransactionReceipt,
		pub bloom: Bloom,
		pub rlp_encoding: Vec<u8>,
		pub json_representation: String,
	}

	`
	rustCode += "pub fn generate_receipts_with_data() -> impl IntoIterator<Item = TransactionReceiptWithTestData> {\n"
	rustCode += "[\n"
	for _, receipt := range data {
		rustReceipt := toRustReceipt(receipt)
		encoding, _ := receipt.MarshalBinary()
		jsonRepresentation, _ := geth.MarshallReceiptJson(receipt)
		rustCode += "TransactionReceiptWithTestData {\n"
		rustCode += fmt.Sprintf("\t\treceipt: %s,\n", rustReceipt)
		rustCode += fmt.Sprintf("\t\tbloom: %s,\n", toRustBloom(receipt))
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(encoding))
		rustCode += fmt.Sprintf("\t\t\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}"
	return rustCode
}

// dumpBlocks generates a Rust iterator over blocks with test data.
// It includes:
// 1. The block
// 2. The block hash
// 3. The transaction root hash
// 4. The receipts root hash
// 5. The RLP encoding of the block
// 6. The JSON representation of the block
func dumpBlocks(data []BlockWithReceipts) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	 #![allow(dead_code)]
	use crate::{Address, Hash, HexConvert, Block, Transaction, TransactionReceipt, TransactionType, U256, AccessListEntry, SetCodeAuthorization, Log};
	`
	// Generate the Block struct
	rustCode += `
	#[derive(Debug, Clone)]
	pub struct BlockWithTestData {
		pub block: Block,
		pub block_hash: Hash,
		pub transaction_root: Hash,
		pub receipts_root: Hash,
		pub rlp_encoding: Vec<u8>,
		pub json_representation: String,
	}

	`
	rustCode += "pub fn generate_blocks_with_data() -> impl IntoIterator<Item = BlockWithTestData> {\n"
	rustCode += "[\n"
	for _, block := range data {
		rustBlock := toRustBlock(block)
		var blockEncoding bytes.Buffer
		_ = block.Block.Header().EncodeRLP(&blockEncoding)
		transactionRoot := block.Block.Header().TxHash.Hex()
		receiptsRoot := block.Block.Header().ReceiptHash.Hex()
		// NOTE: consider adding the receipts json marshalling to match the rust block structure
		jsonRepresentation, _ := json.Marshal(geth.RPCMarshalBlock(block.Block, true, true, params.MainnetChainConfig))
		rustCode += "BlockWithTestData {\n"
		rustCode += fmt.Sprintf("\t\tblock: %s,\n", rustBlock)
		rustCode += fmt.Sprintf("\t\tblock_hash: Hash::try_from_hex(\"%s\").unwrap(),\n", block.Block.Hash().Hex())
		rustCode += fmt.Sprintf("\t\ttransaction_root: Hash::try_from_hex(\"%s\").unwrap(),\n", transactionRoot)
		rustCode += fmt.Sprintf("\t\treceipts_root: Hash::try_from_hex(\"%s\").unwrap(),\n", receiptsRoot)
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(blockEncoding.Bytes()))
		rustCode += fmt.Sprintf("\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}"
	return rustCode
}

// formatAndPrintRustCode formats the given Rust code using rustfmt and prints it to stdout.
// NOTE: It assumes that a `rustgfmt.toml` file is present in the root directory of the project (../../).
func formatAndPrintRustCode(rustCode string) {
	// Format once with a very high max_width to ensure rustfmt does not skip the type formatting
	cmd := exec.Command("rustfmt", "+nightly", "--edition", "2024", "--config-path", "../../", "--config", "max_width=100000", "--config", "struct_lit_width=0")
	cmd.Stdin = strings.NewReader(rustCode)
	output, err := cmd.CombinedOutput()
	if err != nil {
		panic("Failed to format Rust code: " + err.Error())
	}
	// Because the max_width is set to a very high value, some of the standard formatting rules might not apply.
	// So we run rustfmt again with the default settings to ensure proper formatting.
	cmd = exec.Command("rustfmt", "+nightly", "--edition", "2024", "--config-path", "../../")
	cmd.Stdin = bytes.NewReader(output)
	output, err = cmd.CombinedOutput()
	if err != nil {
		panic("Failed to re-format Rust code: " + err.Error())
	}

	fmt.Print(string(output))
}

// Generate a main function that allow to select from the command line if to generate block headers, transactions, or receipts. Add an optional flag for the RLP encoding
func main() {

	switch os.Args[1] {
	case "transactions":
		txs := generateTransactions()
		formatAndPrintRustCode(dumpTransactions(txs))
	case "receipts":
		receipts := generateTransactionsReceipts()
		formatAndPrintRustCode(dumpReceipts(receipts))
	case "blocks":
		blocks := generateBlocks()
		formatAndPrintRustCode(dumpBlocks(blocks))
	default:
		panic("Unknown command: " + os.Args[1])
	}
}
