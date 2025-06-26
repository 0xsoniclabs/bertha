package main

import (
	"bytes"
	"io"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
)

func TestGenerateTransactionsGeneratesAllValueCases(t *testing.T) {
	transactions := types.Transactions(generateTransactions())
	expectedCombination := []*types.Transaction{}

	for fieldName, values := range transactionFieldCases {
		for _, value := range values {
			legacy_tx := types.LegacyTx{}
			if SetValueInStruct(&legacy_tx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&legacy_tx), getTransactionSignatureKey()))
			}
			access_list_tx := types.AccessListTx{}
			if SetValueInStruct(&access_list_tx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&access_list_tx), getTransactionSignatureKey()))
			}
			dynamic_fee_tx := types.DynamicFeeTx{}
			if SetValueInStruct(&dynamic_fee_tx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&dynamic_fee_tx), getTransactionSignatureKey()))
			}
			blob_tx := types.BlobTx{}
			if SetValueInStruct(&blob_tx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&blob_tx), getTransactionSignatureKey()))
			}
			set_code_tx := types.SetCodeTx{}
			if SetValueInStruct(&set_code_tx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&set_code_tx), getTransactionSignatureKey()))
			}
		}
	}

	diff := types.TxDifference(types.Transactions(expectedCombination), transactions)
	// if diff is not empty, it means that the transactions generated do not contain all combinations of field values
	if len(diff) != 0 {
		t.Errorf("Expected transaction s to contain all combinations of field values, but ")
	}

	// Check that the number of transactions is equal to the product of the number of cases for each field
	totalCombination := 5 // 5 types of transactions
	for _, values := range transactionFieldCases {
		totalCombination *= len(values)
	}
	if len(transactions) != totalCombination {
		t.Errorf("Expected %d transactions, but got %d", totalCombination, len(transactions))
	}
}

func TestGenerateTransactionReceiptsGenerateAllValueCase(t *testing.T) {
	receipts := generateTransactionsReceipts()
	expectedCombination := []*types.Receipt{}

	for fieldName, values := range transactionReceiptFieldCases {
		for _, value := range values {
			receipt := types.Receipt{
				Status: 1,
			}
			if SetValueInStruct(&receipt, fieldName, value) {
				receipt.Bloom = types.CreateBloom(&receipt)
				expectedCombination = append(expectedCombination, &receipt)
			}
		}
	}

	for _, receiptCombination := range expectedCombination {
		found := false
		for _, receipt := range receipts {
			// Use encoding as receipts are not comparable
			var receiptEncoding bytes.Buffer
			receipt.EncodeRLP(&receiptEncoding)
			var expectedEncoding bytes.Buffer
			receiptCombination.EncodeRLP(&expectedEncoding)
			if bytes.Equal(receiptEncoding.Bytes(), expectedEncoding.Bytes()) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected receipt %v not found in generated receipts", receiptCombination)
		}
	}

	// Check that the number of receipts is equal to the product of the number of cases for each field
	totalCombination := 1
	for _, values := range transactionReceiptFieldCases {
		totalCombination *= len(values)
	}
	if len(receipts) != totalCombination {
		t.Errorf("Expected %d receipts, but got %d", totalCombination, len(receipts))
	}
}

func TestGenerateBlockHeadersGenerateAllValueCase(t *testing.T) {
	headers := generateBlockHeaders()
	expectedCombination := []*types.Header{}

	for fieldName, values := range blockHeaderFieldCases {
		for _, value := range values {
			header := types.Header{}
			if SetValueInStruct(&header, fieldName, value) {
				expectedCombination = append(expectedCombination, &header)
			}
		}
	}

	for _, headerCombination := range expectedCombination {
		found := false
		for _, header := range headers {
			if header.Hash() == headerCombination.Hash() {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected header %v not found in generated headers", headerCombination)
		}
	}

	// Check that the number of headers is equal to the product of the number of cases for each field
	totalCombination := 1
	for _, values := range blockHeaderFieldCases {
		totalCombination *= len(values)
	}
	if len(headers) != totalCombination {
		t.Errorf("Expected %d headers, but got %d", totalCombination, len(headers))
	}
}

func TestGenerateLogsGeneratesAllValueCase(t *testing.T) {
	logs := generateLogs()
	expectedCombination := []*types.Log{}

	for fieldName, values := range logFieldCases {
		for _, value := range values {
			log := types.Log{}
			if SetValueInStruct(&log, fieldName, value) {
				expectedCombination = append(expectedCombination, &log)
			}
		}
	}

	for _, logCombination := range expectedCombination {
		found := false
		for _, log := range logs {
			var logEncoding bytes.Buffer
			log.EncodeRLP(&logEncoding)
			var expectedEncoding bytes.Buffer
			logCombination.EncodeRLP(&expectedEncoding)
			if bytes.Equal(logEncoding.Bytes(), expectedEncoding.Bytes()) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected log %v not found in generated logs", logCombination)
		}
	}

	// Check that the number of logs is equal to the product of the number of cases for each field
	totalCombination := 1
	for _, values := range logFieldCases {
		totalCombination *= len(values)
	}
	if len(logs) != totalCombination {
		t.Errorf("Expected %d logs, but got %d", totalCombination, len(logs))
	}
}

func TestGenerateBlocksGenerateAllValueCase(t *testing.T) {
	blocks := generateBlocks()
	// Removes parent hashes
	for i, block := range blocks {
		headerWithoutParentHash := block.Block.Header()
		headerWithoutParentHash.ParentHash = common.Hash{}
		blocks[i].Block = types.NewBlock(headerWithoutParentHash, block.Block.Body(), block.Receipts, trie.NewStackTrie(nil))
	}

	expectedCombination := []BlockWithReceipts{}
	blockFields := toNamedFields(getBlockFieldCases())
	// Generate a block with every field set to one of its possible values
	end := false
	idx := 0
	for end == false {
		fields := []NamedField{}
		for _, field := range blockFields {
			if idx < len(field) {
				fields = append(fields, field[idx])
			}
		}
		if len(fields) == 0 {
			end = true
			continue
		}
		idx++
		expectedCombination = append(expectedCombination, BuildBlock(fields))
	}

	// remove all blocks with mismatched receipt and transactions count
	filteredBlocks := []BlockWithReceipts{}
	for _, block := range expectedCombination {
		if len(block.Block.Transactions()) == len(block.Receipts) {
			filteredBlocks = append(filteredBlocks, block)
		}
	}

	for _, blockCombination := range filteredBlocks {
		found := false
		var expectedEncoding bytes.Buffer
		blockCombination.Block.EncodeRLP(&expectedEncoding)
		for _, block := range blocks {
			var blockEncoding bytes.Buffer
			block.Block.EncodeRLP(&blockEncoding)
			equality := bytes.Equal(blockEncoding.Bytes(), expectedEncoding.Bytes())
			// equality := blockCombination.Block.Hash() == block.Block.Hash()
			same_encoding := checkSameEncoding(block.Receipts, blockCombination.Receipts)
			if equality && same_encoding {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected block %v not found in generated blocks", blockCombination)
		}
	}

	// // Check that the number of blocks is equal to the product of the number of cases for each field
	totalCombinations := 1
	for name, values := range getBlockFieldCases() {
		if name == "Receipts" {
			continue // Skip as they match the transactions
		}
		totalCombinations *= len(values)
	}
	if len(blocks) != totalCombinations {
		t.Errorf("Expected %d blocks, but got %d", totalCombinations, len(filteredBlocks))
	}
}

func TestGenerateBlocksGeneratesBlocksWithMatchingParentHashes(t *testing.T) {
	blocks := generateBlocks()
	for i, block := range blocks {
		if i == 0 {
			if block.Block.ParentHash() != (common.Hash{}) {
				t.Errorf("Expected first block to have empty parent hash, but got %s", block.Block.ParentHash().Hex())
			}
		} else {
			if block.Block.ParentHash() != blocks[i-1].Block.Hash() {
				t.Errorf("Expected block %d to have parent hash %s, but got %s", i, blocks[i-1].Block.Hash().Hex(), block.Block.ParentHash().Hex())
			}
		}
	}
}

type Encodable interface {
	EncodeRLP(w io.Writer) error
}

func checkSameEncoding[T Encodable](a []T, b []T) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		var aEncoding bytes.Buffer
		a[i].EncodeRLP(&aEncoding)
		var bEncoding bytes.Buffer
		b[i].EncodeRLP(&bEncoding)
		if !bytes.Equal(aEncoding.Bytes(), bEncoding.Bytes()) {
			return false
		}
	}
	return true
}
