package app

import (
	"fmt"
	"math/big"

	"github.com/0xsoniclabs/blockdb"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/trie"
	"github.com/holiman/uint256"
)

// ConvertToGethBlock converts a blockdb.Block to an Ethereum types.Block.
// It handles the conversion of transactions, receipts, and other block fields.
// If the error is nil, the resulting block is never nil.
func ConvertToGethBlock(block *blockdb.Block) (*types.Block, error) {
	if block == nil {
		return nil, fmt.Errorf("cannot convert nil block")
	}

	// Start by converting the transactions.
	transactions := types.Transactions{}
	for _, tx := range block.Transactions {
		transaction, err := toGethTransaction(tx)
		if err != nil {
			return nil, fmt.Errorf("failed to convert transaction: %w", err)
		}
		transactions = append(transactions, transaction)
	}
	txHash := types.DeriveSha(transactions, trie.NewStackTrie(nil))

	// Convert the receipts.
	receipts := types.Receipts{}
	for _, receipt := range block.Receipts {
		receipts = append(receipts, toGethReceipt(receipt))
	}
	receiptsHash := types.DeriveSha(receipts, trie.NewStackTrie(nil))
	bloom := types.MergeBloom(receipts)

	// Obtain the total gas used in this block.
	gasUsed := uint64(0)
	if len(receipts) > 0 {
		gasUsed = receipts[len(receipts)-1].CumulativeGasUsed
	}

	var nonce [8]byte
	copy(nonce[:], block.Nonce)

	// Reconstruct the block.
	return (&types.Block{}).
		WithSeal(&types.Header{
			ParentHash:       common.BytesToHash(block.ParentHash),
			UncleHash:        common.BytesToHash(block.OmmersHash),
			Coinbase:         common.BytesToAddress(block.Beneficiary),
			Root:             common.BytesToHash(block.StateRoot),
			TxHash:           txHash,
			ReceiptHash:      receiptsHash,
			Bloom:            bloom,
			Difficulty:       new(big.Int).SetUint64(block.Difficulty),
			Number:           new(big.Int).SetUint64(block.Number),
			GasLimit:         block.GasLimit,
			GasUsed:          gasUsed,
			Time:             block.Timestamp,
			Extra:            block.ExtraData,
			MixDigest:        common.BytesToHash(block.PrevRandao),
			Nonce:            types.BlockNonce(nonce),
			BaseFee:          new(big.Int).SetBytes(block.BaseFeePerGas),
			WithdrawalsHash:  toOptionalHash(block.WithdrawalsRoot),
			BlobGasUsed:      block.BlobGasUsed,
			ExcessBlobGas:    block.ExcessBlobGas,
			ParentBeaconRoot: toOptionalHash(block.ParentBeaconBlockRoot),
			RequestsHash:     toOptionalHash(block.RequestsHash),
		}).
		WithBody(types.Body{
			Transactions: transactions,
		}), nil
}

// --- Conversion helper functions for various types ---

func toGethTransaction(tx *blockdb.Transaction) (*types.Transaction, error) {
	if tx == nil {
		return nil, fmt.Errorf("cannot convert nil transaction")
	}
	switch tx.TransactionType {
	case types.LegacyTxType:
		return types.NewTx(&types.LegacyTx{
			Nonce:    tx.Nonce,
			GasPrice: new(big.Int).SetBytes(tx.GasPrice),
			Gas:      tx.GasLimit,
			To:       toOptionalAddress(tx.To),
			Value:    new(big.Int).SetBytes(tx.Value),
			Data:     tx.Data,
			V:        new(big.Int).SetBytes(tx.YParity),
			R:        new(big.Int).SetBytes(tx.R),
			S:        new(big.Int).SetBytes(tx.S),
		}), nil
	case types.AccessListTxType:
		return types.NewTx(&types.AccessListTx{
			ChainID:    new(big.Int).SetBytes(tx.ChainId),
			Nonce:      tx.Nonce,
			GasPrice:   new(big.Int).SetBytes(tx.GasPrice),
			Gas:        tx.GasLimit,
			To:         toOptionalAddress(tx.To),
			Value:      new(big.Int).SetBytes(tx.Value),
			Data:       tx.Data,
			AccessList: toGethAccessList(tx.AccessList),
			V:          new(big.Int).SetBytes(tx.YParity),
			R:          new(big.Int).SetBytes(tx.R),
			S:          new(big.Int).SetBytes(tx.S),
		}), nil
	case types.DynamicFeeTxType:
		return types.NewTx(&types.DynamicFeeTx{
			ChainID:    new(big.Int).SetBytes(tx.ChainId),
			Nonce:      tx.Nonce,
			GasFeeCap:  new(big.Int).SetBytes(tx.MaxFeePerGas),
			GasTipCap:  new(big.Int).SetBytes(tx.MaxPriorityFeePerGas),
			Gas:        tx.GasLimit,
			To:         toOptionalAddress(tx.To),
			Value:      new(big.Int).SetBytes(tx.Value),
			Data:       tx.Data,
			AccessList: toGethAccessList(tx.AccessList),
			V:          new(big.Int).SetBytes(tx.YParity),
			R:          new(big.Int).SetBytes(tx.R),
			S:          new(big.Int).SetBytes(tx.S),
		}), nil
	case types.BlobTxType:
		return types.NewTx(&types.BlobTx{
			ChainID:    new(uint256.Int).SetBytes(tx.ChainId),
			Nonce:      tx.Nonce,
			GasTipCap:  new(uint256.Int).SetBytes(tx.MaxPriorityFeePerGas),
			GasFeeCap:  new(uint256.Int).SetBytes(tx.MaxFeePerGas),
			Gas:        tx.GasLimit,
			To:         common.BytesToAddress(tx.To),
			Value:      new(uint256.Int).SetBytes(tx.Value),
			Data:       tx.Data,
			AccessList: toGethAccessList(tx.AccessList),
			BlobFeeCap: new(uint256.Int).SetBytes(tx.MaxFeePerBlobGas),
			BlobHashes: toGethHashes(tx.BlobVersionedHashes),
			V:          new(uint256.Int).SetBytes(tx.YParity),
			R:          new(uint256.Int).SetBytes(tx.R),
			S:          new(uint256.Int).SetBytes(tx.S),
		}), nil
	case types.SetCodeTxType:
		return types.NewTx(&types.SetCodeTx{
			ChainID:    new(uint256.Int).SetBytes(tx.ChainId),
			Nonce:      tx.Nonce,
			GasTipCap:  new(uint256.Int).SetBytes(tx.MaxPriorityFeePerGas),
			GasFeeCap:  new(uint256.Int).SetBytes(tx.MaxFeePerGas),
			Gas:        tx.GasLimit,
			To:         common.BytesToAddress(tx.To),
			Value:      new(uint256.Int).SetBytes(tx.Value),
			Data:       tx.Data,
			AccessList: toGethAccessList(tx.AccessList),
			AuthList:   toGethAuthorizationList(tx.AuthorizationList),
			V:          new(uint256.Int).SetBytes(tx.YParity),
			R:          new(uint256.Int).SetBytes(tx.R),
			S:          new(uint256.Int).SetBytes(tx.S),
		}), nil
	default:
		return nil, fmt.Errorf("unsupported transaction type: %d", tx.TransactionType)
	}
}

func toGethAccessList(accessList []*blockdb.AccessListEntry) types.AccessList {
	var res types.AccessList
	for _, entry := range accessList {
		address := common.BytesToAddress(entry.Address)
		var storageKeys []common.Hash
		for _, key := range entry.StorageKeys {
			storageKeys = append(storageKeys, common.BytesToHash(key))
		}
		res = append(res, types.AccessTuple{
			Address:     address,
			StorageKeys: storageKeys,
		})
	}
	return res
}

func toGethHashes(hashes [][]byte) []common.Hash {
	var res []common.Hash
	for _, hash := range hashes {
		res = append(res, common.BytesToHash(hash))
	}
	return res
}

func toGethAuthorizationList(authorizations []*blockdb.SetCodeAuthorization) []types.SetCodeAuthorization {
	var res []types.SetCodeAuthorization
	for _, auth := range authorizations {
		res = append(res, types.SetCodeAuthorization{
			ChainID: *new(uint256.Int).SetBytes(auth.ChainId),
			Address: common.BytesToAddress(auth.Address),
			Nonce:   auth.Nonce,
			V:       uint8(auth.YParity),
			R:       *new(uint256.Int).SetBytes(auth.R),
			S:       *new(uint256.Int).SetBytes(auth.S),
		})
	}
	return res
}

func toGethReceipt(receipt *blockdb.TransactionReceipt) *types.Receipt {
	if receipt == nil {
		return nil
	}
	var logs []*types.Log
	for _, log := range receipt.Logs {
		entry := toGethLog(log)
		logs = append(logs, &entry)
	}

	res := &types.Receipt{
		Type:              uint8(receipt.TransactionType),
		Status:            receipt.Status,
		CumulativeGasUsed: receipt.CumulativeGasUsed,
		Logs:              logs,
	}
	res.Bloom = types.CreateBloom(res)
	return res
}

func toGethLog(log *blockdb.Log) types.Log {
	if log == nil {
		return types.Log{}
	}
	var topics []common.Hash
	for _, topic := range log.Topics {
		topics = append(topics, common.BytesToHash(topic))
	}

	return types.Log{
		Address: common.BytesToAddress(log.Address),
		Topics:  topics,
		Data:    log.Data,
	}
}

func toOptionalAddress(data []byte) *common.Address {
	if len(data) == 0 {
		return nil
	}
	var addr common.Address
	copy(addr[:], data)
	return &addr
}

func toOptionalHash(data []byte) *common.Hash {
	if len(data) == 0 {
		return nil
	}
	var hash common.Hash
	copy(hash[:], data)
	return &hash
}
