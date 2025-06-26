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
			legacyTx := types.LegacyTx{}
			if SetValueInStruct(&legacyTx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&legacyTx), getTransactionSignatureKey()))
			}
			accessListTx := types.AccessListTx{}
			if SetValueInStruct(&accessListTx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&accessListTx), getTransactionSignatureKey()))
			}
			dynamicFeeTx := types.DynamicFeeTx{}
			if SetValueInStruct(&dynamicFeeTx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&dynamicFeeTx), getTransactionSignatureKey()))
			}
			blobTx := types.BlobTx{}
			if SetValueInStruct(&blobTx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&blobTx), getTransactionSignatureKey()))
			}
			setCodeTx := types.SetCodeTx{}
			if SetValueInStruct(&setCodeTx, fieldName, value) {
				expectedCombination = append(expectedCombination, signTransaction(big.NewInt(1), types.TxData(&setCodeTx), getTransactionSignatureKey()))
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
	expectedReceiptsCombination := []*types.Receipt{}

	for fieldName, values := range transactionReceiptFieldCases {
		for _, value := range values {
			receipt := types.Receipt{
				Status: 1,
			}
			if SetValueInStruct(&receipt, fieldName, value) {
				receipt.Bloom = types.CreateBloom(&receipt)
				expectedReceiptsCombination = append(expectedReceiptsCombination, &receipt)
			}
		}
	}

	for _, expectedReceipts := range expectedReceiptsCombination {
		// Use encoding as receipts are not comparable
		found := false
		for _, receipt := range receipts {
			if haveSameEncoding(receipt, expectedReceipts) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected receipt %v not found in generated receipts", expectedReceipts)
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
	expectedBlockHeaderCombination := []*types.Header{}

	for fieldName, values := range blockHeaderFieldCases {
		for _, value := range values {
			header := types.Header{}
			if SetValueInStruct(&header, fieldName, value) {
				expectedBlockHeaderCombination = append(expectedBlockHeaderCombination, &header)
			}
		}
	}

	for _, expectedHeader := range expectedBlockHeaderCombination {
		found := false
		for _, header := range headers {
			if header.Hash() == expectedHeader.Hash() {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected header %v not found in generated headers", expectedHeader)
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
	expectedLogCombination := []*types.Log{}

	for fieldName, values := range logFieldCases {
		for _, value := range values {
			log := types.Log{}
			if SetValueInStruct(&log, fieldName, value) {
				expectedLogCombination = append(expectedLogCombination, &log)
			}
		}
	}

	for _, expectedLog := range expectedLogCombination {
		found := false
		for _, log := range logs {
			if haveSameEncoding(log, expectedLog) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected log %v not found in generated logs", expectedLog)
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

	expectedBlockWithReceiptsCombinations := []BlockWithReceipts{}
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
		expectedBlockWithReceiptsCombinations = append(expectedBlockWithReceiptsCombinations, BuildBlock(fields))
	}

	// remove all blocks with mismatched receipt and transactions count
	filteredBlocks := []BlockWithReceipts{}
	for _, block := range expectedBlockWithReceiptsCombinations {
		if len(block.Block.Transactions()) == len(block.Receipts) {
			filteredBlocks = append(filteredBlocks, block)
		}
	}
	expectedBlockWithReceiptsCombinations = filteredBlocks

	for _, expectedBlockWithReceipt := range expectedBlockWithReceiptsCombinations {
		found := false
		for _, block := range blocks {
			if haveSameEncoding(block.Block, expectedBlockWithReceipt.Block) && sliceHaveSameEncoding(block.Receipts, expectedBlockWithReceipt.Receipts) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected block %v not found in generated blocks", expectedBlockWithReceipt)
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

func haveSameEncoding[T Encodable](a T, b T) bool {
	var aEncoding bytes.Buffer
	err := a.EncodeRLP(&aEncoding)
	if err != nil {
		panic(err)
	}
	var bEncoding bytes.Buffer
	err = b.EncodeRLP(&bEncoding)
	if err != nil {
		panic(err)
	}
	return bytes.Equal(aEncoding.Bytes(), bEncoding.Bytes())
}

func sliceHaveSameEncoding[T Encodable](a []T, b []T) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if !haveSameEncoding(a[i], b[i]) {
			return false
		}
	}
	return true
}
