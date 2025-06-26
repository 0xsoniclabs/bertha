package main

import (
	"crypto/ecdsa"
	"fmt"
	"iter"
	"math/big"
	"reflect"
	"sort"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
)

// getTestKey() returns a dummy private key for transaction signature
func getTransactionSignatureKey() *ecdsa.PrivateKey {
	return crypto.ToECDSAUnsafe(common.FromHex("066361a741d5da2eb952b1d6061d60f4ce0efab63a10fff4137e7605e6a5702d"))
}

// NamedField is a utility struct to represent a struct field by its name and value.
type NamedField struct {
	Name  string
	Value any
}

// BlockWithReceipts is a utility struct that holds a block and its associated receipts.
// This is useful for testing purposes where we need to pair blocks with their receipts.
type BlockWithReceipts struct {
	Block    *types.Block
	Receipts []*types.Receipt
}

// Construct an element of type T, modifies it with the provided pieces by transforming
// it into type K, and yields the result.
func generateData[T any, K any, Piece any](
	constructor func() T,
	pieces [][]Piece,
	apply func(constructor func() T, modifier []Piece) K,
) iter.Seq[K] {

	return func(yield func(data K) bool) {
		_cartesianProductRecursion(nil, pieces,
			func(pieces []Piece) bool {
				res := apply(constructor, pieces)
				return yield(res)
			})
	}
}

// constructAndGenerateData constructs an element of type K by applying the provided
// transformation function to the pieces. The constructor is not used in this case
func constructAndGenerateData[K any, Piece any](
	pieces [][]Piece,
	genFunction func(modifier []Piece) K,
) iter.Seq[K] {
	return generateData(
		func() K { return *new(K) }, // Unused constructor
		pieces,
		func(constructor func() K, modifier []Piece) K {
			return genFunction(modifier) // Apply the transformation ignoring the constructor
		},
	)
}

// SetValueInStruct sets the value of a field in a struct T by its name.
// Returns true if the field was found and set, false otherwise.
func SetValueInStruct[T any, K any](data T, fieldName string, value K) bool {
	dataValue := reflect.ValueOf(data)
	var f reflect.Value
	if dataValue.Kind() == reflect.Ptr {
		f = reflect.ValueOf(data).Elem().FieldByName(fieldName)
	} else {
		f = reflect.ValueOf(data).FieldByName(fieldName)
	}
	if !f.IsValid() {
		return false
	}
	fieldValue := reflect.ValueOf(value)
	if f.Kind() != reflect.Ptr {
		if fieldValue.Kind() == reflect.Ptr {
			if fieldValue.IsNil() {
				return false
			}
			f.Set(fieldValue.Elem())
		} else {
			f.Set(fieldValue)
		}
	} else {
		f.Set(fieldValue)
	}
	return true
}

// generateStruct sets the fields of a struct T based on the provided values.
// It uses reflection to set the fields of the struct based on the NamedField values.
func generateStruct[T any](constructor func() T, values [][]NamedField) iter.Seq[T] {
	return generateData(
		constructor,
		values,
		func(constructor func() T, modifier []NamedField) T {
			v := constructor()
			for _, field := range modifier {
				if !SetValueInStruct(v, field.Name, field.Value) {
					continue
				}
			}
			return v
		},
	)
}

// _cartesianProductRecursion is a recursive helper function that generates the Cartesian product of the provided elements.
func _cartesianProductRecursion[T any](current []T, elements [][]T, callback func(data []T) bool) bool {
	if len(elements) == 0 {
		return callback(current)
	}

	var next [][]T
	if len(elements) > 1 {
		next = elements[1:]
	}

	for _, element := range elements[0] {
		if !_cartesianProductRecursion(append(current, element), next, callback) {
			return false
		}
	}
	return true
}

// signTransaction is a testing helper that signs a transaction with the
// key from the provided account
func signTransaction(
	chainID *big.Int,
	payload types.TxData,
	key *ecdsa.PrivateKey,
) *types.Transaction {
	res, err := types.SignTx(
		types.NewTx(payload),
		types.NewPragueSigner(chainID),
		key)
	if err != nil {
		panic("failed to sign transaction: " + err.Error())
	}
	return res
}

// seqToSlice converts an iter.Seq[T] to a slice of T.
func seqToSlice[T any](seq iter.Seq[T]) []T {
	var res []T
	seq(func(data T) bool {
		res = append(res, data)
		return true
	})
	return res
}

// Flatten the outer dimension of a slice
func flattenSlice[T any](values [][]T) []T {
	var flat []T
	for _, v := range values {
		flat = append(flat, v...)
	}
	return flat
}

// toNamedFields converts a map of field names to slices of values into a slice of slices of NamedField.
// Each slice contains the same field name with different values
// NOTE: The fields are ordered by field name.
func toNamedFields(values map[string][]any) [][]NamedField {
	fields := [][]NamedField{}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	for _, fieldName := range keys {
		fieldValues := values[fieldName]
		field := []NamedField{}
		for _, v := range fieldValues {
			field = append(field, NamedField{fieldName, v})
		}
		fields = append(fields, field)
	}
	return fields
}

// ########## Rust conversion utility functions ########## //

// toRustVector converts a slice of data of type T into a Rust vector string representation using the rustStringGen marshalling function
func toRustVector[T any](data []T, rustStringGen func(v T) string) string {
	if len(data) == 0 {
		return "vec![]"
	}

	entries := "vec!["
	for _, v := range data {
		entries += rustStringGen(v) + ", "
	}
	entries = entries[:len(entries)-2] + "]"
	return entries
}

// toRustLogList converts a slice of Go logs to a Bertha Log type in Rust.
func toRustLogList(logs []*types.Log) string {
	return toRustVector(logs, func(log *types.Log) string {
		return fmt.Sprintf(`Log {
			address: Address::try_from_hex("%s").unwrap(),
			topics: %s,
			data: %s,
		}`, log.Address.Hex(), toRustHashList(log.Topics), toRustByteVec(log.Data))
	})
}

// toRustAccessList converts  a Go AccessList to a Bertha AccessList type in Rust.
func toRustAccessList(accessList types.AccessList) string {
	return toRustVector(accessList, func(entry types.AccessTuple) string {
		return fmt.Sprintf(`AccessListEntry {
			address: Address::try_from_hex("%s").unwrap(),
			storage_keys: %s,
		}`, entry.Address.Hex(), toRustHashList(entry.StorageKeys))
	})
}

// toRustHashList converts a slice of Go common.Hash to a Bertha Hash type in Rust.
func toRustHashList(hashes []common.Hash) string {
	return toRustVector(hashes, func(hash common.Hash) string {
		return fmt.Sprintf("Hash::try_from_hex(\"0x%s\").unwrap()", hash.Hex())
	})
}

// toRustTransaction converts a Go transaction to the Bertha Transaction type in Rust.
func toRustAuthorizationList(authList []types.SetCodeAuthorization) string {
	return toRustVector(authList, func(auth types.SetCodeAuthorization) string {
		return fmt.Sprintf(`SetCodeAuthorization {
			chain_id: U256::try_from_hex("%s").unwrap(),
			address: Address::try_from_hex("%s").unwrap(),
			nonce: %d,
			y_parity: %d,
			r: U256::try_from_hex("%s").unwrap(),
			s: U256::try_from_hex("%s").unwrap(),
		}`, auth.ChainID.ToBig().Text(16), auth.Address.Hex(),
			auth.Nonce, auth.V, auth.R.ToBig().Text(16), auth.S.ToBig().Text(16))
	})
}

// toRustByteVec converts a byte slice to a Rust byte vector string representation.
func toRustByteVec(data []byte) string {
	if len(data) == 0 {
		return "vec![]"
	}

	byteString := ""
	for i, b := range data {
		byteString += fmt.Sprintf("%d", b)
		if i < len(data)-1 {
			byteString += ", "
		}
	}
	return fmt.Sprintf("vec![%s]", byteString)
}

// toRustByteArray converts a byte slice to a Rust array string representation.
func toRustByteArray(data []byte) string {
	if len(data) == 0 {
		return "[]"
	}

	byteString := ""
	for i, b := range data {
		byteString += fmt.Sprintf("%d", b)
		if i < len(data)-1 {
			byteString += ", "
		}
	}
	return fmt.Sprintf("[%s]", byteString)
}
