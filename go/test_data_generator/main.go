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

func generateTransactions() []*types.Transaction {
	return generateTransactionsWithFields(transactionFieldCases)
}

func generateTransactionsWithFields(fields map[string][]any) []*types.Transaction {
	transactionFields := generateNamedFields(fields)

	txs := []*types.Transaction{}
	for payload := range generateStruct(func() *types.LegacyTx { return &types.LegacyTx{GasPrice: common.Big0} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.AccessListTx { return &types.AccessListTx{GasPrice: common.Big0} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.DynamicFeeTx { return &types.DynamicFeeTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.BlobTx { return &types.BlobTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	for payload := range generateStruct(func() *types.SetCodeTx { return &types.SetCodeTx{} }, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload, getTransactionSignatureKey()))
	}
	return txs
}

func generateTransactionsWithFieldsAndType(txType uint8, fields map[string][]any) []*types.Transaction {
	transactionFields := generateNamedFields(fields)
	var txs []*types.Transaction
	for payload := range generateStruct(func() any {
		switch txType {
		case types.LegacyTxType:
			return &types.LegacyTx{GasPrice: common.Big0}
		case types.AccessListTxType:
			return &types.AccessListTx{GasPrice: common.Big0}
		case types.DynamicFeeTxType:
			return &types.DynamicFeeTx{}
		case types.BlobTxType:
			return &types.BlobTx{}
		case types.SetCodeTxType:
			return &types.SetCodeTx{}
		default:
			panic(fmt.Sprintf("Unknown transaction type: %d", txType))
		}
	}, transactionFields) {
		txs = append(txs, signTransaction(big.NewInt(1), payload.(types.TxData), getTransactionSignatureKey()))
	}
	return txs
}

func generateTransactionsReceipts() []*types.Receipt {
	return generateTransactionsReceiptsWithFields(transactionReceiptFieldCases)
}

func generateTransactionsReceiptsWithFields(fields map[string][]any) []*types.Receipt {
	transactionReceiptFields := generateNamedFields(fields)
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
	return generateBlockHeadersWithFields(BlockHeaderFieldCases)
}

func generateBlockHeadersWithFields(fields map[string][]any) []*types.Header {
	blockFields := generateNamedFields(fields)
	blocks := generateStruct(func() *types.Header { return &types.Header{} }, blockFields)
	return seqToSlice(blocks)
}

func generateLogs() []*types.Log {
	return generateLogsWithFields(logFieldCases)
}

func generateLogsWithFields(fields map[string][]any) []*types.Log {
	logFields := generateNamedFields(fields)
	logs := generateStruct(func() *types.Log { return &types.Log{} }, logFields)
	return seqToSlice(logs)
}

func generateBlocks() []BlockWithReceipts {

	blockHeaders := generateBlockHeaders()
	var blockFields = make(map[string][]any)
	// Add each block as an individual field
	blockFields["Header"] = []any{}
	for _, block := range blockHeaders {
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
	includeLogs := false
	receipts := make([][]*types.Receipt, len(transactions))
	for i, txs := range transactions {
		receipts[i] = make([]*types.Receipt, len(txs))
		for j := range receipts[i] {
			if includeLogs {
				receipts[i][j] = &types.Receipt{
					Logs: []*types.Log{
						{},
					},
				}
			} else {
				receipts[i][j] = &types.Receipt{}
			}
			includeLogs = !includeLogs // Toggle includeLogs for next iteration
		}
	}

	// Add to blockFields
	blockFields["Transactions"] = []any{flattenSlice(transactions)}
	blockFields["Receipts"] = []any{flattenSlice(receipts)}

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

	blocks := constructAndGenerateData(
		generateNamedFields(blockFields),
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
			block := types.NewBlock(header, &body, receipts, trie.NewStackTrie(nil))
			receiptsContainer := types.Receipts(receipts)
			// Ignoring blobGasPrice as we don't need them
			_ = receiptsContainer.DeriveFields(params.MainnetChainConfig, block.Header().Hash(), block.NumberU64(), block.Time(), block.BaseFee(), common.Big0, body.Transactions)
			return BlockWithReceipts{
				Block:    types.NewBlock(header, &body, receipts, trie.NewStackTrie(nil)),
				Receipts: []*types.Receipt(receipts),
			}
		},
	)

	return seqToSlice(blocks)
}

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

func dumpBlocks(data []BlockWithReceipts) string {
	rustCode := `
	// This file is generated by go/test_data_generator/main.go
	 #![allow(dead_code)]
	use crate::{Address, Hash, HexConvert, Block, Transaction, TransactionReceipt, TransactionType, U256, AccessListEntry, SetCodeAuthorization};
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
