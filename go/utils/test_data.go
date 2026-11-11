// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

package utils

import (
	"bytes"
	"math/big"
	"testing"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/sonic/gossip/gasprice"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/misc/eip1559"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/state"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/params"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

// CreateValidBlocks creates a sequence of valid blocks with proper parent-child relationships.
// The first block has no parent, and each subsequent block's parent is the
// previous block.
func CreateValidBlocks(t *testing.T, num int) []*blockdb.Block {
	t.Helper()
	blocks := make([]*blockdb.Block, num)
	lastHash := common.Hash{}
	for i := range num {
		next := &blockdb.Block{
			Number:     uint64(i),
			ParentHash: bytes.Clone(lastHash[:]),
			StateRoot:  types.EmptyRootHash[:],
		}
		blocks[i] = next

		block, err := convert.ConvertToGethBlock(next)
		require.NoError(t, err, "failed to convert block to Geth format")
		lastHash = block.Hash()
	}
	return blocks
}

// CreateSonicBlocks returns the first num empty blocks of a Sonic network with
// the given rules.
func CreateSonicBlocks(t *testing.T, rules opera.Rules, db *state.StateDB, num int) []*blockdb.Block {
	config := opera.CreateTransientEvmChainConfig(rules.NetworkID, []opera.UpgradeHeight{{Upgrades: rules.Upgrades}}, 0)

	parent := inter.NewBlockBuilder().
		WithStateRoot(db.IntermediateRoot(true)).
		WithTime(inter.Timestamp(time.Unix(1_700_000_000, 0).UnixNano())).
		WithGasLimit(rules.Blocks.MaxBlockGas).
		WithBaseFee(gasprice.GetInitialBaseFee(rules.Economy)).
		Build()
	blocks := []*blockdb.Block{ToBlockDbBlock(t, parent.GetEthereumHeader())}
	for i := 1; i < num; i++ {
		timestamp, prevRandao := parent.Time+inter.Timestamp(time.Second), common.Hash{byte(i)}
		core.ProcessParentBlockHash(parent.Hash(), vm.NewEVM(vm.BlockContext{
			BlockNumber: big.NewInt(int64(i)),
			Time:        uint64(timestamp.Unix()),
			Random:      &prevRandao,
		}, db, config, vm.Config{}))
		block := inter.NewBlockBuilder().
			WithNumber(uint64(i)).
			WithParentHash(parent.Hash()).
			WithStateRoot(db.IntermediateRoot(true)).
			WithTime(timestamp).
			WithPrevRandao(prevRandao).
			WithDuration(time.Second).
			WithGasLimit(parent.GasLimit).
			WithBaseFee(gasprice.GetBaseFeeForNextBlock(gasprice.ParentBlockInfo{
				BaseFee:  parent.BaseFee,
				Duration: time.Duration(parent.Duration),
				GasUsed:  parent.GasUsed,
			}, rules.Economy)).
			Build()
		blocks = append(blocks, ToBlockDbBlock(t, block.GetEthereumHeader()))
		parent = block
	}
	return blocks
}

// CreateEthereumBlocks returns the first num empty post-merge blocks of an
// Ethereum network with the given chain config.
func CreateEthereumBlocks(t *testing.T, config *params.ChainConfig, db *state.StateDB, num int) []*blockdb.Block {
	var blocks []*blockdb.Block
	var parent *types.Header
	for i := range num {
		header := &types.Header{
			UncleHash:        types.EmptyUncleHash,
			TxHash:           types.EmptyTxsHash,
			ReceiptHash:      types.EmptyReceiptsHash,
			Difficulty:       new(big.Int),
			Number:           big.NewInt(int64(i)),
			GasLimit:         params.GenesisGasLimit,
			Time:             *config.PragueTime - 20*12,
			MixDigest:        common.Hash{byte(i)},
			BaseFee:          big.NewInt(params.InitialBaseFee),
			WithdrawalsHash:  &types.EmptyWithdrawalsHash,
			BlobGasUsed:      new(uint64),
			ExcessBlobGas:    new(uint64),
			ParentBeaconRoot: &common.Hash{byte(i)},
		}
		if parent != nil {
			header.ParentHash = parent.Hash()
			header.Time = parent.Time + 12
			header.BaseFee = eip1559.CalcBaseFee(config, parent)
			evm := vm.NewEVM(vm.BlockContext{
				BlockNumber: header.Number,
				Time:        header.Time,
				Random:      &header.MixDigest,
			}, db, config, vm.Config{})
			core.ProcessBeaconBlockRoot(*header.ParentBeaconRoot, evm)
			if config.IsPrague(header.Number, header.Time) {
				core.ProcessParentBlockHash(header.ParentHash, evm)
				var requests [][]byte
				require.NoError(t, core.ProcessWithdrawalQueue(&requests, evm))
				require.NoError(t, core.ProcessConsolidationQueue(&requests, evm))
				header.RequestsHash = new(types.CalcRequestsHash(requests))
			}
		}
		header.Root = db.IntermediateRoot(true)
		blocks = append(blocks, ToBlockDbBlock(t, header))
		parent = header
	}
	return blocks
}

// NewGenesisState returns an in-memory state holding the given accounts.
func NewGenesisState(t *testing.T, alloc types.GenesisAlloc) *state.StateDB {
	db, err := state.New(types.EmptyRootHash, state.NewDatabaseForTesting())
	require.NoError(t, err)
	for address, account := range alloc {
		db.SetBalance(address, uint256.MustFromBig(account.Balance), tracing.BalanceChangeUnspecified)
		db.SetCode(address, account.Code, tracing.CodeChangeUnspecified)
		db.SetNonce(address, account.Nonce, tracing.NonceChangeUnspecified)
		for key, value := range account.Storage {
			db.SetState(address, key, value)
		}
	}
	return db
}

// ToBlockDbBlock returns the block database's form of the empty block with the
// given header, and checks that converting it back keeps the block's hash.
func ToBlockDbBlock(t *testing.T, header *types.Header) *blockdb.Block {
	block := &blockdb.Block{
		Number:          header.Number.Uint64(),
		ParentHash:      header.ParentHash.Bytes(),
		OmmersHash:      header.UncleHash.Bytes(),
		Beneficiary:     header.Coinbase.Bytes(),
		StateRoot:       header.Root.Bytes(),
		Difficulty:      header.Difficulty.Uint64(),
		GasLimit:        header.GasLimit,
		Timestamp:       header.Time,
		ExtraData:       header.Extra,
		PrevRandao:      header.MixDigest.Bytes(),
		Nonce:           header.Nonce[:],
		BaseFeePerGas:   header.BaseFee.Bytes(),
		WithdrawalsRoot: header.WithdrawalsHash.Bytes(),
		BlobGasUsed:     header.BlobGasUsed,
		ExcessBlobGas:   header.ExcessBlobGas,
	}
	if header.ParentBeaconRoot != nil {
		block.ParentBeaconBlockRoot = header.ParentBeaconRoot.Bytes()
	}
	if header.RequestsHash != nil {
		block.RequestsHash = header.RequestsHash.Bytes()
	}
	converted, err := convert.ConvertToGethBlock(block)
	require.NoError(t, err)
	require.Equal(t, header.Hash(), converted.Hash())
	return block
}
