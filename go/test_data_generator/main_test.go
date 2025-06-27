package main

import (
	"math/big"
	"reflect"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/holiman/uint256"
)

// getTransactionFieldValueByMethod retrieves the value of a field from a transaction by its method name if it exists
func getTransactionFieldValueByMethod(tx *types.Transaction, fieldName string) (any, bool) {
	txType := uint(tx.Type())
	var isIncluded bool
	switch txType {
	case types.LegacyTxType:
		isIncluded = reflect.ValueOf(types.LegacyTx{}).FieldByName(fieldName).IsValid()
	case types.AccessListTxType:
		isIncluded = reflect.ValueOf(types.AccessListTx{}).FieldByName(fieldName).IsValid()
	case types.DynamicFeeTxType:
		isIncluded = reflect.ValueOf(types.DynamicFeeTx{}).FieldByName(fieldName).IsValid()
	case types.BlobTxType:
		isIncluded = reflect.ValueOf(types.BlobTx{}).FieldByName(fieldName).IsValid()
	case types.SetCodeTxType:
		isIncluded = reflect.ValueOf(types.SetCodeTx{}).FieldByName(fieldName).IsValid()
	default:
		panic("Unknown transaction type: ")
	}
	if !isIncluded {
		return nil, false
	}

	txValue := reflect.ValueOf(tx)
	var method reflect.Value
	switch fieldName {
	case "AuthList":
		method = txValue.MethodByName("SetCodeAuthorizations")
	case "BlobFeeCap":
		method = txValue.MethodByName("BlobGasFeeCap")
	default:
		method = txValue.MethodByName(fieldName)
	}
	if !method.IsValid() {
		panic("Method " + fieldName + " not found in transaction type")
	}
	fieldValue := method.Call([]reflect.Value{})[0].Interface()
	if fieldName != "To" && (fieldValue == nil || (reflect.ValueOf(fieldValue).Kind() == reflect.Ptr && reflect.ValueOf(fieldValue).IsNil())) {
		// Field is not in  the transaction type
		return nil, false
	}
	return fieldValue, true
}

// getFieldValueFromStruct retrieves the value of a field from a struct by its name.
func getFieldValueFromStruct[T any](data T, fieldName string) (any, bool) {
	dataValue := reflect.ValueOf(data)
	var f reflect.Value
	if dataValue.Kind() == reflect.Ptr {
		f = dataValue.Elem().FieldByName(fieldName)
	} else {
		f = dataValue.FieldByName(fieldName)
	}
	if !f.IsValid() {
		return nil, false
	}
	if f.Kind() == reflect.Ptr && f.IsNil() {
		return nil, false
	}
	return f.Interface(), true
}

// removeElementByIndex removes an element from a slice at the specified index.
func removeElementByIndex(slice []any, index int) []any {
	if index < 0 || index >= len(slice) {
		return slice // Index out of range, return the original slice
	}
	return append(slice[:index], slice[index+1:]...)
}

// maxLenField returns the maximum length of the values in the fieldValues map.
func maxLenField(fieldValues map[string][]any) int {
	maxLen := 0
	for _, values := range fieldValues {
		if len(values) > maxLen {
			maxLen = len(values)
		}
	}
	return maxLen
}

// filterFields checks that each field in remainingFields is present in at least one of the data entries.
// It uses the extractor function to get the field value from each data entry.
func filterFields[T any](data []T, remainingFields map[string][]any,
	extractor func(v T, fieldName string) (any, bool)) bool {
	for _, dataEntry := range data {
		for fieldName, values := range remainingFields {
			fieldValue, ok := extractor(dataEntry, fieldName)
			if !ok {
				delete(remainingFields, fieldName)
				continue
			}
			for i, value := range values {
				var equality bool
				// check if value has type *uint256.Int
				if v, ok := value.(*uint256.Int); ok {
					value = v.ToBig() // convert *uint256.Int to *big.Int
					fieldValue, _ := fieldValue.(*big.Int)
					equality = fieldValue.Cmp(value.(*big.Int)) == 0
				} else if v, ok := value.([]*types.Transaction); ok {
					blockTransactions, _ := fieldValue.([]*types.Transaction)
					if types.TxDifference(types.Transactions(blockTransactions), types.Transactions(v)).Len() == 0 {
						equality = true
					}
				} else if v, ok := value.([]*types.Header); ok {
					blockHeaders, _ := fieldValue.([]*types.Header)
					equality = true
					for i, header := range blockHeaders {
						if header.Hash() != v[i].Hash() {
							equality = false
							break
						}
					}
				} else {
					equality = reflect.DeepEqual(fieldValue, value)
				}

				if equality {
					remainingFields[fieldName] = removeElementByIndex(remainingFields[fieldName], i)
					break
				}
			}
			if len(remainingFields[fieldName]) == 0 {
				delete(remainingFields, fieldName)
			}
		}
	}
	return len(remainingFields) == 0
}

func TestFilterFields(t *testing.T) {
	type foo struct {
		bar int
		baz int
	}
	extractor := func(v foo, fieldName string) (any, bool) {
		if fieldName == "bar" {
			return v.bar, true
		}
		return nil, false
	}

	// Every combination of field values are found
	combinations := map[string][]any{
		"bar": {1, 2, 3},
		"baz": {4, 5, 6},
	}
	data := []foo{
		{bar: 1, baz: 4},
		{bar: 2, baz: 5},
		{bar: 3, baz: 6},
	}
	if !filterFields(data, combinations, extractor) {
		t.Errorf("Expected all combinations to be found, but some were not")
	}

	// Some combinations are not found
	combinations = map[string][]any{
		"bar": {1, 2, 4, 3},
	}
	if filterFields(data, combinations, extractor) {
		t.Errorf("Expected some combinations to not be found, but all were found")
	}

	// filterFields ignores fields that are not in the data
	combinations = map[string][]any{
		"bar": {1},
		"baz": {4},
	}
	data = []foo{
		{bar: 10, baz: 4}, // only baz matches
		{bar: 1, baz: 5},  // only bar matches
		{bar: 2, baz: 6},  // no matches
	}
	if !filterFields(data, combinations, extractor) {
		t.Errorf("Expected some combinations to be found, but none were found")
	}
}

func TestGenerateTransactionsGeneratesAllValueCases(t *testing.T) {
	transactions := types.Transactions(generateTransactions())

	checkFields := func(txType uint8, remainingFields map[string][]any) bool {
		filteredTransactions := []*types.Transaction{}
		for _, tx := range transactions {
			if tx.Type() == txType {
				filteredTransactions = append(filteredTransactions, tx)
			}
		}
		return filterFields(filteredTransactions, remainingFields, getTransactionFieldValueByMethod)
	}

	if !checkFields(types.LegacyTxType, getLegacyAndAccessListFields()) {
		t.Errorf("Expected all transaction fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.AccessListTxType, getLegacyAndAccessListFields()) {
		t.Errorf("Expected all access list transaction fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.DynamicFeeTxType, getDynamicFeeFields()) {
		t.Errorf("Expected all dynamic fee transaction fields to be covered, but the following fields were not found:")
	}
	if !checkFields(types.BlobTxType, getBlobAndSetCodeFields()) {
		t.Errorf("Expected all blob transaction fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.SetCodeTxType, getBlobAndSetCodeFields()) {
		t.Errorf("Expected all set code transaction fields to be covered, but the following fields were not found")
	}

	// Check that the number of transactions is equal to the product of the number of cases for each field
	totalCombinations := 2*maxLenField(transactionFieldCases) + maxLenField(insertMap(copyMap(transactionFieldCases), dynamicFeeFields)) + 2*maxLenField(insertMap(copyMap(transactionFieldCases), blobAndSetCodeFields))
	if len(transactions) != totalCombinations {
		t.Errorf("Expected %d transactions, but got %d", totalCombinations, len(transactions))
	}
}

func TestGenerateTransactionReceiptsGenerateAllValueCase(t *testing.T) {
	receipts := generateTransactionsReceipts()

	checkFields := func(txType uint8, remainingFields map[string][]any) bool {
		filteredReceipts := []*types.Receipt{}
		for _, receipt := range receipts {
			if receipt.Type == txType {
				filteredReceipts = append(filteredReceipts, receipt)
			}
		}
		return filterFields(filteredReceipts, remainingFields, getFieldValueFromStruct)
	}

	if !checkFields(types.LegacyTxType, copyMap(transactionReceiptFieldCases)) {
		t.Errorf("Expected all legacy transaction receipt fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.AccessListTxType, copyMap(transactionReceiptFieldCases)) {
		t.Errorf("Expected all access list transaction receipt fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.DynamicFeeTxType, copyMap(transactionReceiptFieldCases)) {
		t.Errorf("Expected all dynamic fee transaction receipt fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.BlobTxType, copyMap(transactionReceiptFieldCases)) {
		t.Errorf("Expected all blob transaction receipt fields to be covered, but the following fields were not found")
	}
	if !checkFields(types.SetCodeTxType, copyMap(transactionReceiptFieldCases)) {
		t.Errorf("Expected all set code transaction receipt fields to be covered, but the following fields were not found")
	}

	// Check number of combinations
	if len(receipts) != 5*maxLenField(transactionReceiptFieldCases) {
		t.Errorf("Expected %d transaction receipts, but got %d", 5*maxLenField(transactionReceiptFieldCases), len(receipts))
	}
}

func TestGenerateBlockHeadersGenerateAllValueCase(t *testing.T) {
	headers := generateBlockHeaders()

	if !filterFields(headers, copyMap(blockHeaderFieldCases), getFieldValueFromStruct) {
		t.Errorf("Expected all block header fields to be covered, but the following fields were not found")
	}

	if len(headers) != maxLenField(blockHeaderFieldCases) {
		t.Errorf("Expected %d block headers, but got %d", maxLenField(blockHeaderFieldCases), len(headers))
	}

}

func TestGenerateLogsGeneratesAllValueCase(t *testing.T) {
	logs := generateLogs()

	if !filterFields(logs, copyMap(logFieldCases), getFieldValueFromStruct) {
		t.Errorf("Expected all log fields to be covered, but the following fields were not found")
	}
}

func TestGenerateBlocksGenerateAllValueCase(t *testing.T) {
	blocks := generateBlocks()
	// Checks that all block header fields are initialized
	for _, block := range blocks {
		header := block.Block.Header()
		if header.TxHash == (common.Hash{}) || header.ReceiptHash == (common.Hash{}) ||
			header.UncleHash == (common.Hash{}) || *header.WithdrawalsHash == (common.Hash{}) {
			t.Errorf("Block header hashes are not initialized for block %s", block.Block.Hash().Hex())
		}
	}
	// Remove header as it is covered before
	blockFields := getBlockFieldCases()
	delete(blockFields, "Header")

	blockFieldExtractor := func(v BlockWithReceipts, fieldName string) (any, bool) {
		if fieldName == "Receipts" {
			return v.Receipts, true
		}
		if fieldName == "Transactions" {
			return []*types.Transaction(v.Block.Transactions()), true
		}
		if fieldName == "Withdrawals" {
			return []*types.Withdrawal(v.Block.Withdrawals()), true
		}
		value := reflect.ValueOf(v.Block).MethodByName(fieldName).Call([]reflect.Value{})
		if !value[0].IsValid() {
			return nil, false
		}
		return value[0].Interface(), true
	}

	if !filterFields(blocks, blockFields, blockFieldExtractor) {
		t.Errorf("Expected all block fields to be covered, but the following fields were not  found")
	}

	if len(blocks) != maxLenField(getBlockFieldCases()) {
		t.Errorf("Expected %d blocks, but got %d", maxLenField(blockFields), len(blocks))
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
