// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

package app

import (
	"math/big"
	"testing"

	"github.com/0xsoniclabs/blockdb"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

func TestConvertToGethBlock_NilBlock_ReturnsAnError(t *testing.T) {
	_, err := ConvertToGethBlock(nil)
	require.ErrorContains(t, err, "cannot convert nil block")
}

func TestConvertToGethBlock_InvalidTransactionType_ReturnsAnError(t *testing.T) {
	_, err := ConvertToGethBlock(&blockdb.Block{
		Transactions: []*blockdb.Transaction{
			{TransactionType: 999},
		},
	})
	require.ErrorContains(t, err, "failed to convert transaction")
}

func TestConvertToGethBlock_ConvertsBlockToGethBlock(t *testing.T) {

	input := &blockdb.Block{
		ParentHash:      []byte{0x01, 0x02, 0x03, 0x04},
		OmmersHash:      []byte{0x05, 0x06, 0x07, 0x08},
		Beneficiary:     []byte{0x09, 0x0a, 0x0b, 0x0c},
		StateRoot:       []byte{0x0d, 0x0e, 0x0f, 0x10},
		Difficulty:      1000,
		Number:          42,
		GasLimit:        8000000,
		Timestamp:       1633036800,
		ExtraData:       []byte{0x11, 0x12, 0x13, 0x14},
		PrevRandao:      []byte{0x15, 0x16, 0x17, 0x18},
		Nonce:           []byte{0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28},
		BaseFeePerGas:   []byte{0x29, 0x2a},
		WithdrawalsRoot: []byte{0x2b, 0x2c, 0x2d, 0x2e},
		BlobGasUsed:     func() *uint64 { x := uint64(100); return &x }(),
		ExcessBlobGas:   func() *uint64 { x := uint64(10); return &x }(),
		Transactions:    []*blockdb.Transaction{{}, {}},
		Receipts:        []*blockdb.TransactionReceipt{{}, {}},
	}

	transactions := types.Transactions{}
	for _, tx := range input.Transactions {
		tx, err := toGethTransaction(tx)
		require.NoError(t, err)
		transactions = append(transactions, tx)
	}

	receipts := types.Receipts{}
	for _, receipt := range input.Receipts {
		receipts = append(receipts, toGethReceipt(receipt))
	}

	want := (&types.Block{}).
		WithSeal(&types.Header{
			ParentHash:      common.BytesToHash([]byte{0x01, 0x02, 0x03, 0x04}),
			UncleHash:       common.BytesToHash([]byte{0x05, 0x06, 0x07, 0x08}),
			Coinbase:        common.BytesToAddress([]byte{0x09, 0x0a, 0x0b, 0x0c}),
			Root:            common.BytesToHash([]byte{0x0d, 0x0e, 0x0f, 0x10}),
			Difficulty:      new(big.Int).SetUint64(1000),
			Number:          new(big.Int).SetUint64(42),
			GasLimit:        8000000,
			GasUsed:         receipts[len(receipts)-1].CumulativeGasUsed,
			Time:            1633036800,
			Extra:           []byte{0x11, 0x12, 0x13, 0x14},
			MixDigest:       common.BytesToHash([]byte{0x15, 0x16, 0x17, 0x18}),
			Nonce:           types.BlockNonce([]byte{0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28}),
			BaseFee:         new(big.Int).SetBytes([]byte{0x29, 0x2a}),
			WithdrawalsHash: toOptionalHash([]byte{0x2b, 0x2c, 0x2d, 0x2e}),
			BlobGasUsed:     func() *uint64 { x := uint64(100); return &x }(),
			ExcessBlobGas:   func() *uint64 { x := uint64(10); return &x }(),
			TxHash:          types.DeriveSha(transactions, trie.NewStackTrie(nil)),
			ReceiptHash:     types.DeriveSha(receipts, trie.NewStackTrie(nil)),
			Bloom:           types.MergeBloom(receipts),
		}).
		WithBody(types.Body{
			Transactions: transactions,
		})

	got, err := ConvertToGethBlock(input)
	require.NoError(t, err)
	require.Equal(t, want.Number(), got.Number())
	require.Equal(t, want.ParentHash(), got.ParentHash())
	require.Equal(t, want.UncleHash(), got.UncleHash())
	require.Equal(t, want.Coinbase(), got.Coinbase())
	require.Equal(t, want.Root(), got.Root())
	require.Equal(t, want.TxHash(), got.TxHash())
	require.Equal(t, want.ReceiptHash(), got.ReceiptHash())
	require.Equal(t, want.Bloom(), got.Bloom())
	require.Equal(t, want.Difficulty(), got.Difficulty())
	require.Equal(t, want.GasLimit(), got.GasLimit())
	require.Equal(t, want.GasUsed(), got.GasUsed())
	require.Equal(t, want.Time(), got.Time())
	require.Equal(t, want.Extra(), got.Extra())
	require.Equal(t, want.MixDigest(), got.MixDigest())
	require.Equal(t, want.Nonce(), got.Nonce())
	require.Equal(t, want.BaseFee(), got.BaseFee())
	require.Equal(t, want.Header(), got.Header())
	require.Equal(t, want.Hash(), got.Hash())
}

func TestToGethTransaction_NilTransaction_ReturnsError(t *testing.T) {
	_, err := toGethTransaction(nil)
	require.ErrorContains(t, err, "cannot convert nil transaction")
}

func TestToGethTransaction_UnsupportedType_ReturnsError(t *testing.T) {
	_, err := toGethTransaction(&blockdb.Transaction{
		TransactionType: 999, // Unsupported type
	})
	require.ErrorContains(t, err, "unsupported transaction type: 999")
}

func TestToGethTransaction_ConvertsTransactionToGethTransaction(t *testing.T) {
	tests := map[string]struct {
		input *blockdb.Transaction
		want  *types.Transaction
	}{
		"empty": {
			input: &blockdb.Transaction{},
			want:  types.NewTx(&types.LegacyTx{}),
		},
		"legacy": {
			input: &blockdb.Transaction{
				TransactionType: types.LegacyTxType,
				Nonce:           1,
				To:              []byte{0x01, 0x02, 0x03},
				Value:           big.NewInt(1000).Bytes(),
				GasPrice:        []byte{0x04, 0x05},
				GasLimit:        21000,
				Data:            []byte{0x06, 0x07},
				YParity:         []byte{0x08},
				R:               []byte{0x09, 0x0a},
				S:               []byte{0x0b, 0x0c},
			},
			want: types.NewTx(&types.LegacyTx{
				Nonce:    1,
				GasPrice: new(big.Int).SetBytes([]byte{0x04, 0x05}),
				Gas:      21000,
				To:       toOptionalAddress([]byte{0x01, 0x02, 0x03}),
				Value:    new(big.Int).SetInt64(1000),
				Data:     []byte{0x06, 0x07},
				V:        new(big.Int).SetBytes([]byte{0x08}),
				R:        new(big.Int).SetBytes([]byte{0x09, 0x0a}),
				S:        new(big.Int).SetBytes([]byte{0x0b, 0x0c}),
			}),
		},
		"access list": {
			input: &blockdb.Transaction{
				TransactionType: types.AccessListTxType,
				ChainId:         []byte{0x01, 0x02},
				Nonce:           2,
				To:              []byte{0x01, 0x02, 0x03},
				Value:           big.NewInt(2000).Bytes(),
				GasPrice:        []byte{0x04, 0x05},
				GasLimit:        21000,
				Data:            []byte{0x06, 0x07},
				YParity:         []byte{0x08},
				R:               []byte{0x09, 0x0a},
				S:               []byte{0x0b, 0x0c},
				AccessList: []*blockdb.AccessListEntry{
					{
						Address: []byte{0x01, 0x02, 0x03},
						StorageKeys: [][]byte{
							{0x04, 0x05},
							{0x06, 0x07},
						},
					},
				},
			},
			want: types.NewTx(&types.AccessListTx{
				ChainID:  new(big.Int).SetBytes([]byte{0x01, 0x02}),
				Nonce:    2,
				GasPrice: new(big.Int).SetBytes([]byte{0x04, 0x05}),
				Gas:      21000,
				To:       toOptionalAddress([]byte{0x01, 0x02, 0x03}),
				Value:    new(big.Int).SetInt64(2000),
				Data:     []byte{0x06, 0x07},
				AccessList: types.AccessList{
					{
						Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
						StorageKeys: []common.Hash{
							common.BytesToHash([]byte{0x04, 0x05}),
							common.BytesToHash([]byte{0x06, 0x07}),
						},
					},
				},
				V: new(big.Int).SetBytes([]byte{0x08}),
				R: new(big.Int).SetBytes([]byte{0x09, 0x0a}),
				S: new(big.Int).SetBytes([]byte{0x0b, 0x0c}),
			}),
		},
		"dynamic fee": {
			input: &blockdb.Transaction{
				TransactionType:      types.DynamicFeeTxType,
				ChainId:              []byte{0x01, 0x02},
				Nonce:                3,
				To:                   []byte{0x01, 0x02, 0x03},
				Value:                big.NewInt(3000).Bytes(),
				MaxFeePerGas:         []byte{0x04, 0x05},
				MaxPriorityFeePerGas: []byte{0x06, 0x07},
				GasLimit:             21000,
				Data:                 []byte{0x08, 0x09},
				AccessList: []*blockdb.AccessListEntry{
					{
						Address: []byte{0x01, 0x02, 0x03},
						StorageKeys: [][]byte{
							{0x04, 0x05},
							{0x06, 0x07},
						},
					},
				},
				YParity: []byte{0x0a},
				R:       []byte{0x0b, 0x0c},
				S:       []byte{0x0d, 0x0e},
			},
			want: types.NewTx(&types.DynamicFeeTx{
				ChainID:   new(big.Int).SetBytes([]byte{0x01, 0x02}),
				Nonce:     3,
				GasFeeCap: new(big.Int).SetBytes([]byte{0x04, 0x05}),
				GasTipCap: new(big.Int).SetBytes([]byte{0x06, 0x07}),
				Gas:       21000,
				To:        toOptionalAddress([]byte{0x01, 0x02, 0x03}),
				Value:     new(big.Int).SetInt64(3000),
				Data:      []byte{0x08, 0x09},
				AccessList: types.AccessList{
					{
						Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
						StorageKeys: []common.Hash{
							common.BytesToHash([]byte{0x04, 0x05}),
							common.BytesToHash([]byte{0x06, 0x07}),
						},
					},
				},
				V: new(big.Int).SetBytes([]byte{0x0a}),
				R: new(big.Int).SetBytes([]byte{0x0b, 0x0c}),
				S: new(big.Int).SetBytes([]byte{0x0d, 0x0e}),
			}),
		},
		"blob": {
			input: &blockdb.Transaction{
				TransactionType:      types.BlobTxType,
				ChainId:              []byte{0x01, 0x02},
				Nonce:                4,
				To:                   []byte{0x01, 0x02, 0x03},
				Value:                big.NewInt(4000).Bytes(),
				MaxFeePerGas:         []byte{0x04, 0x05},
				MaxPriorityFeePerGas: []byte{0x06, 0x07},
				GasLimit:             21000,
				Data:                 []byte{0x08, 0x09},
				AccessList: []*blockdb.AccessListEntry{
					{
						Address: []byte{0x01, 0x02, 0x03},
						StorageKeys: [][]byte{
							{0x04, 0x05},
							{0x06, 0x07},
						},
					},
				},
				MaxFeePerBlobGas: []byte{0x0a, 0x0b},
				BlobVersionedHashes: [][]byte{
					{0x0a, 0x0b},
					{0x0c, 0x0d},
				},
				YParity: []byte{0x0a},
				R:       []byte{0x0b, 0x0c},
				S:       []byte{0x0d, 0x0e},
			},
			want: types.NewTx(&types.BlobTx{
				ChainID:   new(uint256.Int).SetBytes([]byte{0x01, 0x02}),
				Nonce:     4,
				GasTipCap: new(uint256.Int).SetBytes([]byte{0x06, 0x07}),
				GasFeeCap: new(uint256.Int).SetBytes([]byte{0x04, 0x05}),
				Gas:       21000,
				To:        common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
				Value:     uint256.NewInt(4000),
				Data:      []byte{0x08, 0x09},
				AccessList: types.AccessList{
					{
						Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
						StorageKeys: []common.Hash{
							common.BytesToHash([]byte{0x04, 0x05}),
							common.BytesToHash([]byte{0x06, 0x07}),
						},
					},
				},
				BlobFeeCap: new(uint256.Int).SetBytes([]byte{0x0a, 0x0b}),
				BlobHashes: []common.Hash{
					common.BytesToHash([]byte{0x0a, 0x0b}),
					common.BytesToHash([]byte{0x0c, 0x0d}),
				},
				V: new(uint256.Int).SetBytes([]byte{0x0a}),
				R: new(uint256.Int).SetBytes([]byte{0x0b, 0x0c}),
				S: new(uint256.Int).SetBytes([]byte{0x0d, 0x0e}),
			}),
		},
		"set code": {
			input: &blockdb.Transaction{
				TransactionType:      types.SetCodeTxType,
				ChainId:              []byte{0x01, 0x02},
				Nonce:                5,
				To:                   []byte{0x01, 0x02, 0x03},
				Value:                big.NewInt(5000).Bytes(),
				MaxFeePerGas:         []byte{0x04, 0x05},
				MaxPriorityFeePerGas: []byte{0x06, 0x07},
				GasLimit:             21000,
				Data:                 []byte{0x08, 0x09},
				AccessList: []*blockdb.AccessListEntry{
					{
						Address: []byte{0x01, 0x02, 0x03},
						StorageKeys: [][]byte{
							{0x04, 0x05},
							{0x06, 0x07},
						},
					},
				},
				AuthorizationList: []*blockdb.SetCodeAuthorization{
					{
						ChainId: []byte{0x01, 0x02, 0x03},
						Address: []byte{0x11, 0x12, 0x13},
						Nonce:   12,
						YParity: 0x1a,
						R:       []byte{0x1b, 0x1c},
						S:       []byte{0x1d, 0x1e},
					},
				},
				YParity: []byte{0x0a},
				R:       []byte{0x0b, 0x0c},
				S:       []byte{0x0d, 0x0e},
			},
			want: types.NewTx(&types.SetCodeTx{
				ChainID:   new(uint256.Int).SetBytes([]byte{0x01, 0x02}),
				Nonce:     5,
				GasTipCap: new(uint256.Int).SetBytes([]byte{0x06, 0x07}),
				GasFeeCap: new(uint256.Int).SetBytes([]byte{0x04, 0x05}),
				Gas:       21000,
				To:        common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
				Value:     uint256.NewInt(5000),
				Data:      []byte{0x08, 0x09},
				AccessList: types.AccessList{
					{
						Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
						StorageKeys: []common.Hash{
							common.BytesToHash([]byte{0x04, 0x05}),
							common.BytesToHash([]byte{0x06, 0x07}),
						},
					},
				},
				AuthList: []types.SetCodeAuthorization{
					{
						ChainID: *new(uint256.Int).SetBytes([]byte{0x01, 0x02, 0x03}),
						Address: common.BytesToAddress([]byte{0x11, 0x12, 0x13}),
						Nonce:   12,
						V:       0x1a,
						R:       *new(uint256.Int).SetBytes([]byte{0x1b, 0x1c}),
						S:       *new(uint256.Int).SetBytes([]byte{0x1d, 0x1e}),
					},
				},
				V: new(uint256.Int).SetBytes([]byte{0x0a}),
				R: new(uint256.Int).SetBytes([]byte{0x0b, 0x0c}),
				S: new(uint256.Int).SetBytes([]byte{0x0d, 0x0e}),
			}),
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			require := require.New(t)
			got, err := toGethTransaction(tc.input)
			require.NoError(err)
			require.Equal(tc.want.ChainId(), got.ChainId())
			require.Equal(tc.want.Nonce(), got.Nonce())
			require.Equal(tc.want.Gas(), got.Gas())
			require.Equal(tc.want.To(), got.To())
			require.Equal(tc.want.Value(), got.Value())
			require.Equal(tc.want.Data(), got.Data())
			require.Equal(tc.want.GasPrice(), got.GasPrice())
			require.Equal(tc.want.GasFeeCap(), got.GasFeeCap())
			require.Equal(tc.want.GasTipCap(), got.GasTipCap())
			require.Equal(tc.want.AccessList(), got.AccessList())
			require.Equal(tc.want.SetCodeAuthorizations(), got.SetCodeAuthorizations())
			require.Equal(tc.want.BlobHashes(), got.BlobHashes())
			require.Equal(tc.want.BlobGasFeeCap(), got.BlobGasFeeCap())

			wantV, wantR, wantS := tc.want.RawSignatureValues()
			gotV, gotR, gotS := got.RawSignatureValues()

			require.Equal(wantV, gotV)
			require.Equal(wantR, gotR)
			require.Equal(wantS, gotS)

			require.Equal(tc.want.Hash(), got.Hash())
		})
	}
}

func TestToGethAccessList_ConvertsAccessListToGethAccessList(t *testing.T) {
	tests := map[string]struct {
		input []*blockdb.AccessListEntry
		want  types.AccessList
	}{
		"nil": {
			input: nil,
			want:  nil,
		},
		"empty": {
			input: []*blockdb.AccessListEntry{},
			want:  nil,
		},
		"with content": {
			input: []*blockdb.AccessListEntry{
				{
					Address: []byte{0x01, 0x02, 0x03},
					StorageKeys: [][]byte{
						{0x04, 0x05},
						{0x06, 0x07},
					},
				},
				{
					Address: []byte{0x08, 0x09},
					StorageKeys: [][]byte{
						{0x0a, 0x0b},
					},
				},
				{
					Address: []byte{0x0c, 0x0d},
				},
			},
			want: types.AccessList{
				{
					Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
					StorageKeys: []common.Hash{
						common.BytesToHash([]byte{0x04, 0x05}),
						common.BytesToHash([]byte{0x06, 0x07}),
					},
				},
				{
					Address: common.BytesToAddress([]byte{0x08, 0x09}),
					StorageKeys: []common.Hash{
						common.BytesToHash([]byte{0x0a, 0x0b}),
					},
				},
				{
					Address: common.BytesToAddress([]byte{0x0c, 0x0d}),
				},
			},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got := toGethAccessList(tc.input)
			require.Equal(t, tc.want, got)
		})
	}
}

func TestToGethReceipt_ConvertsReceiptToGethReceipt(t *testing.T) {
	tests := map[string]struct {
		input *blockdb.TransactionReceipt
		want  *types.Receipt
	}{
		"nil": {
			input: nil,
			want:  nil,
		},
		"empty": {
			input: &blockdb.TransactionReceipt{},
			want:  &types.Receipt{},
		},
		"with content": {
			input: &blockdb.TransactionReceipt{
				PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: 1},
				CumulativeGasUsed: 1000,
				Logs: []*blockdb.Log{
					{Data: []byte{0x03}},
					{Data: []byte{0x06, 0x07}},
				},
			},
			want: &types.Receipt{
				Status:            1,
				CumulativeGasUsed: 1000,
				Logs: []*types.Log{
					{Data: []byte{0x03}},
					{Data: []byte{0x06, 0x07}},
				},
			},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got := toGethReceipt(tc.input)
			want := tc.want
			if want != nil {
				want.Bloom = types.CreateBloom(want)
			}
			require.Equal(t, want, got)
		})
	}
}

func TestToGethReceipt_PanicsOnPreEIP658Receipt(t *testing.T) {
	require.Panics(t, func() {
		toGethReceipt(&blockdb.TransactionReceipt{
			PostStateOrStatus: &blockdb.TransactionReceipt_PostState{PostState: make([]byte, 32)},
		})
	})
}

func TestToGethLog_ConvertsLogToGethLog(t *testing.T) {
	tests := map[string]struct {
		input *blockdb.Log
		want  types.Log
	}{
		"nil": {
			input: nil,
			want:  types.Log{},
		},
		"empty": {
			input: &blockdb.Log{},
			want:  types.Log{},
		},
		"with content": {
			input: &blockdb.Log{
				Address: []byte{0x01, 0x02, 0x03},
				Topics:  [][]byte{{0x04, 0x05}, {0x06, 0x07}},
				Data:    []byte{0x08, 0x09},
			},
			want: types.Log{
				Address: common.BytesToAddress([]byte{0x01, 0x02, 0x03}),
				Topics: []common.Hash{
					common.BytesToHash([]byte{0x04, 0x05}),
					common.BytesToHash([]byte{0x06, 0x07}),
				},
				Data: []byte{0x08, 0x09},
			},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got := toGethLog(tc.input)
			require.Equal(t, tc.want, got)
		})
	}
}

func TestToOptionalAddress_ConvertsBytesToAddressesIfNotEmpty(t *testing.T) {
	tests := map[string]struct {
		input []byte
		want  *common.Address
	}{
		"nil": {
			input: nil,
			want:  nil,
		},
		"empty": {
			input: []byte{},
			want:  nil,
		},
		"non-empty": {
			input: []byte{0x01, 0x02, 0x03},
			want:  &common.Address{0x01, 0x02, 0x03},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got := toOptionalAddress(tc.input)
			require.Equal(t, tc.want, got)
		})
	}
}

func TestToOptionalHash_ConvertsBytesToHashesIfNotEmpty(t *testing.T) {
	tests := map[string]struct {
		input []byte
		want  *common.Hash
	}{
		"nil": {
			input: nil,
			want:  nil,
		},
		"empty": {
			input: []byte{},
			want:  nil,
		},
		"non-empty": {
			input: []byte{0x01, 0x02, 0x03},
			want:  &common.Hash{0x01, 0x02, 0x03},
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			got := toOptionalHash(tc.input)
			require.Equal(t, tc.want, got)
		})
	}
}
