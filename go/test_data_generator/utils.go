package main

import (
	"crypto/ecdsa"
	"encoding/binary"
	"fmt"
	"iter"
	"math"
	"math/big"
	"math/rand"
	"reflect"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/holiman/uint256"
)

var numGenerator = rand.New(rand.NewSource(42))

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

func generateStruct[T any](
	constructor func() T,
	structFields map[string][]any) iter.Seq[T] {
	return generateDataWithMaxLengthCombination(
		structFields,
		func(fields []NamedField) T {
			value := constructor()
			for _, field := range fields {
				if !SetValueInStruct(value, field.Name, field.Value) {
					continue
				}
			}
			return value
		})
}

func generateDataWithMaxLengthCombination[T any](
	structFields map[string][]any,
	apply func(fields []NamedField) T) iter.Seq[T] {
	return func(yield func(data T) bool) {
		idx := 0
		for true {
			fields := []NamedField{}
			for fieldName, fieldValues := range structFields {
				if idx < len(fieldValues) {
					fields = append(fields, NamedField{Name: fieldName, Value: fieldValues[idx]})
				}
			}

			if len(fields) == 0 {
				return
			}

			data := apply(fields)

			idx++
			if !yield(data) {
				break
			}
		}
	}
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

// copyMap creates a deep copy of a map[string][]any.
func copyMap(m map[string][]any) map[string][]any {
	res := make(map[string][]any, len(m))
	for k, v := range m {
		res[k] = append([]any{}, v...)
	}
	return res
}

// Insert source map into destination map.
// If a key exists, it overrides the value
func insertMap(m map[string][]any, source map[string][]any) map[string][]any {
	for k, v := range source {
		m[k] = []any{}
		m[k] = append(m[k], v...)
	}
	return m
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

func getUint64FieldCases() []uint64 {
	return []uint64{
		0,
		1,
		math.MaxUint64,
		math.MaxUint64 - 1,
		numGenerator.Uint64(),
	}
}

func getBigIntCases() []*big.Int {
	return []*big.Int{
		big.NewInt(0),
		big.NewInt(1),
		new(big.Int).SetUint64(math.MaxUint64),
		new(big.Int).SetUint64(math.MaxUint64 - 1),
		new(big.Int).SetUint64(numGenerator.Uint64()),
	}
}

func getUint256FieldCases() []*uint256.Int {
	max := uint256.Int([]uint64{math.MaxUint64, math.MaxUint64, math.MaxUint64, math.MaxUint64})
	maxMinusOne := uint256.Int([]uint64{math.MaxUint64, math.MaxUint64, math.MaxUint64, math.MaxUint64 - 1})
	randomValue := uint256.Int([]uint64{numGenerator.Uint64(), numGenerator.Uint64(), numGenerator.Uint64(), numGenerator.Uint64()})
	return []*uint256.Int{
		new(uint256.Int),
		new(uint256.Int).SetUint64(1),
		&max,
		&maxMinusOne,
		&randomValue,
	}
}

func getBlockNonceFieldCases() []types.BlockNonce {
	buf := make([]byte, 8)
	binary.BigEndian.PutUint64(buf, numGenerator.Uint64())
	return []types.BlockNonce{
		{0, 0, 0, 0, 0, 0, 0, 0},
		{0, 0, 0, 0, 0, 0, 0, 1},
		{0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff},
		{0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe},
		types.BlockNonce(buf),
	}
}

func toAnySlice[T any](input []T) []any {
	result := make([]any, len(input))
	for i, v := range input {
		result[i] = v
	}
	return result
}
