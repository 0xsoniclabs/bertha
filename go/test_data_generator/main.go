package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math/big"
	"os"
	"os/exec"
	"strings"

	gethapi "github.com/0xsoniclabs/bertha/go/test_data_generator/geth_files"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/trie"
)

func generateTransactions() []*types.Transaction {
	transactionFields := [][]NamedField{
		{
			{
				"To",
				getNullPtr[common.Address](),
			},
			{
				"To",
				new(common.Address),
			},
		},
		{
			{
				"AccessList",
				types.AccessList{},
			},
			{
				"AccessList",
				types.AccessList{
					types.AccessTuple{
						Address:     common.Address{},
						StorageKeys: []common.Hash{},
					},
				},
			},
			{
				"AccessList",
				types.AccessList{
					types.AccessTuple{
						Address: common.Address{},
						StorageKeys: []common.Hash{
							{},
						},
					},
				},
			},
		},
		{
			{
				"Data",
				[]byte{},
			},
			{
				"Data",
				[]byte{0x1},
			},
		},
		{
			{
				"BlobHashes",
				[]common.Hash{},
			},
			{
				"BlobHashes",
				[]common.Hash{
					{},
				},
			},
		},
		{
			{
				"AuthList",
				[]types.SetCodeAuthorization{},
			},
			{
				"AuthList",
				[]types.SetCodeAuthorization{
					{},
				},
			},
		},
	}

	txs := []*types.Transaction{}
	for payload := range generateStruct(func() *types.LegacyTx { return &types.LegacyTx{GasPrice: common.Big0} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, KEY))
	}
	for payload := range generateStruct(func() *types.AccessListTx { return &types.AccessListTx{GasPrice: common.Big0} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, KEY))
	}
	for payload := range generateStruct(func() *types.DynamicFeeTx { return &types.DynamicFeeTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, KEY))
	}
	for payload := range generateStruct(func() *types.BlobTx { return &types.BlobTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, KEY))
	}
	for payload := range generateStruct(func() *types.SetCodeTx { return &types.SetCodeTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, KEY))
	}
	return txs
}

func generateTransactionsReceipts() []*types.Receipt {
	transactionReceiptFields := [][]NamedField{
		{
			{
				"Type",
				uint8(types.LegacyTxType),
			},
			{
				"Type",
				uint8(types.AccessListTxType),
			},
			{
				"Type",
				uint8(types.DynamicFeeTxType),
			},
			{
				"Type",
				uint8(types.BlobTxType),
			},
			{
				"Type",
				uint8(types.SetCodeTxType),
			},
		},
		{
			{
				"Logs",
				[]*types.Log{},
			},
			{
				"Logs",
				generateLogs(),
			},
		},
	}
	receipts := generateStruct(func() *types.Receipt {
		return &types.Receipt{
			Status: 1,
		}
	}, transactionReceiptFields)
	// convert sequence to slice
	receiptsSlice := []*types.Receipt{}
	for receipt := range receipts {
		// Compute bloom
		receipt.Bloom = types.CreateBloom(receipt)
		receiptsSlice = append(receiptsSlice, receipt)
	}
	return receiptsSlice
}

func generateBlockHeaders() []*types.Header {
	blockFields := [][]NamedField{
		{
			{
				"Extra",
				[]byte{},
			},
			{
				"Extra",
				[]byte{0x1},
			},
		},
	}

	blocks := generateStruct(func() *types.Header { return &types.Header{} }, blockFields)
	return seqToSlice(blocks)
}

func generateLogs() []*types.Log {
	logFields := [][]NamedField{
		{
			{
				"Topics",
				[]common.Hash{},
			},
			{
				"Topics",
				[]common.Hash{
					{},
				},
			},
		},
		{
			{
				"Data",
				[]byte{},
			},
			{
				"Data",
				[]byte{0x1},
			},
		},
	}
	logs := generateStruct(func() *types.Log { return &types.Log{} }, logFields)
	return seqToSlice(logs)
}

func generateBlocks() []BlockWithReceipts {

	blockHeaders := generateBlockHeaders()
	var blockFields [][]NamedField
	// Add each block as an individual field
	for _, block := range blockHeaders {
		blockFields = append(blockFields, []NamedField{
			{
				"Header",
				block,
			},
		})
	}

	// Add transactions and receipts as fields
	legacyTx := &types.LegacyTx{}
	accessListTx := &types.AccessListTx{}
	dynamicFeeTx := &types.DynamicFeeTx{}
	blockFields = append(blockFields, [][]NamedField{
		{
			{
				"Transactions",
				[]*types.Transaction{},
			},
			{
				"Transactions",
				[]*types.Transaction{
					signTransaction(big.NewInt(1), legacyTx, KEY),
				},
			},
			{
				"Transactions",
				[]*types.Transaction{
					signTransaction(big.NewInt(1), legacyTx, KEY),
					signTransaction(big.NewInt(1), accessListTx, KEY),
				},
			},
			{
				"Transactions",
				[]*types.Transaction{
					signTransaction(big.NewInt(1), legacyTx, KEY),
					signTransaction(big.NewInt(1), accessListTx, KEY),
					signTransaction(big.NewInt(1), dynamicFeeTx, KEY),
				},
			},
		},
		{
			{
				"Receipts",
				[]*types.Receipt{},
			},
			{
				"Receipts",
				generateTransactionsReceipts(),
			},
		},
		{
			{
				"Uncles",
				[]*types.Header{},
			},
			{
				"Uncles",
				[]*types.Header{
					{},
				},
			},
		},
		{
			{
				"Withdrawals",
				[]*types.Withdrawal{},
			},
			{
				"Withdrawals",
				[]*types.Withdrawal{
					{},
				},
			},
		},
	}...)

	blocks := constructAndGenerateData(
		blockFields,
		func(fields []NamedField) BlockWithReceipts {
			header := &types.Header{}
			var receipts []*types.Receipt
			var body types.Body

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
			return BlockWithReceipts{
				Block:    types.NewBlock(header, &body, receipts, trie.NewStackTrie(nil)),
				Receipts: receipts,
			}
		},
	)

	return seqToSlice(blocks)
}

func dumpTransactions(data []*types.Transaction) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	#[cfg(test)]
	pub mod tests {
	pub mod transaction {
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
		jsonRepresentation, _ := json.Marshal(gethapi.NewRPCTransactionWithoutBlock(tx))
		rustCode += "TransactionWithTestData {\n"
		rustCode += fmt.Sprintf("\t\ttransaction: %s,\n", rustTx)
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(encoding))
		rustCode += fmt.Sprintf("\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}\n}\n}"
	return rustCode
}

func dumpReceipts(data []*types.Receipt) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	#[cfg(test)]
	pub mod tests {
	pub mod receipt {
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
		jsonRepresentation, _ := gethapi.MarshallReceiptJson(receipt)
		rustCode += "TransactionReceiptWithTestData {\n"
		rustCode += fmt.Sprintf("\t\treceipt: %s,\n", rustReceipt)
		rustCode += fmt.Sprintf("\t\tbloom: %s,\n", toRustBloom(receipt))
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(encoding))
		rustCode += fmt.Sprintf("\t\t\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}\n}\n}"
	return rustCode
}

func dumpBlocks(data []BlockWithReceipts) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	#[cfg(test)]
	pub mod tests {
	pub mod block {
	use crate::{Address, Hash, HexConvert, Block, Transaction, TransactionReceipt, TransactionType, U256, AccessListEntry, SetCodeAuthorization, Log};
	`
	// Generate the Block struct
	rustCode += `
	#[derive(Debug, Clone)]
	pub struct BlockWithTestData {
		pub block: Block,
		block_hash: Hash,
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
		jsonRepresentation, _ := json.Marshal(gethapi.RPCMarshalBlock(block.Block, true, true, params.MainnetChainConfig))
		rustCode += "BlockWithTestData {\n"
		rustCode += fmt.Sprintf("\t\tblock: %s,\n", rustBlock)
		rustCode += fmt.Sprintf("\t\tblock_hash: Hash::try_from_hex(\"%s\").unwrap(),\n", block.Block.Hash().Hex())
		rustCode += fmt.Sprintf("\t\ttransaction_root: Hash::try_from_hex(\"%s\").unwrap(),\n", transactionRoot)
		rustCode += fmt.Sprintf("\t\treceipts_root: Hash::try_from_hex(\"%s\").unwrap(),\n", receiptsRoot)
		rustCode += fmt.Sprintf("\t\trlp_encoding: const_hex::decode(\"%s\").unwrap(),\n", common.Bytes2Hex(blockEncoding.Bytes()))
		rustCode += fmt.Sprintf("\t\tjson_representation: r#\"%s\"#.to_string(),\n", string(jsonRepresentation))
		rustCode += "},\n"
	}
	rustCode += "]\n}\n}\n}"
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
	case "transaction_verification":

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
