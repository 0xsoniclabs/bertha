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

package replay

import (
	"encoding/binary"
	"encoding/json"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/sonic/gossip/gasprice"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
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
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

// blockDbsByNetwork maps network names to functions that create a block
// database of empty blocks of the network, and return the database's
// directory and the genesis file.
var blockDbsByNetwork = map[string]func(*testing.T) (string, string){
	"Sonic":    func(t *testing.T) (string, string) { return createSonicBlockDb(t, nil) },
	"Ethereum": createEthereumBlockDb,
}

// createSonicBlockDb writes a genesis with Sonic's fake network rules of the
// Allegro upgrade and the EIP-2935 history storage contract, and the network's
// first empty blocks, one second apart, with the given corrections into a new
// block database, and returns the database's directory and the genesis file.
func createSonicBlockDb(t *testing.T, corrections Corrections) (string, string) {
	rules := opera.FakeNetRules(opera.GetAllegroUpgrades())
	alloc := types.GenesisAlloc{
		params.HistoryStorageAddress: {Nonce: 1, Code: params.HistoryStorageCode, Balance: common.Big0},
	}
	db := newGenesisState(t, alloc)
	config := opera.CreateTransientEvmChainConfig(rules.NetworkID, []opera.UpgradeHeight{{Upgrades: rules.Upgrades}}, 0)

	parent := inter.NewBlockBuilder().
		WithStateRoot(db.IntermediateRoot(true)).
		WithTime(inter.Timestamp(time.Unix(1_700_000_000, 0).UnixNano())).
		WithGasLimit(rules.Blocks.MaxBlockGas).
		WithBaseFee(gasprice.GetInitialBaseFee(rules.Economy)).
		Build()
	blocks := []*blockdb.Block{toBlockDbBlock(t, parent.GetEthereumHeader())}
	for i := 1; i < 100; i++ {
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
		blocks = append(blocks, toBlockDbBlock(t, block.GetEthereumHeader()))
		parent = block
	}
	return writeBlockDb(t, rules, alloc, blocks, corrections)
}

// createEthereumBlockDb writes a genesis with the chain ID of Hoodi and the
// system contracts of Hoodi's Cancun and Prague upgrades, and empty post-merge
// blocks of Hoodi, twelve seconds apart with all but the first 20 after its
// Prague upgrade, into a new block database, and returns the database's
// directory and the genesis file. The request contracts start with their
// excess inhibitor set, which the first Prague block clears.
func createEthereumBlockDb(t *testing.T) (string, string) {
	config := params.HoodiChainConfig
	rules := opera.Rules{NetworkID: config.ChainID.Uint64()}
	inhibited := map[common.Hash]common.Hash{{}: common.MaxHash}
	alloc := types.GenesisAlloc{
		params.BeaconRootsAddress:        {Nonce: 1, Code: params.BeaconRootsCode, Balance: common.Big0},
		params.HistoryStorageAddress:     {Nonce: 1, Code: params.HistoryStorageCode, Balance: common.Big0},
		params.WithdrawalQueueAddress:    {Nonce: 1, Code: params.WithdrawalQueueCode, Balance: common.Big0, Storage: inhibited},
		params.ConsolidationQueueAddress: {Nonce: 1, Code: params.ConsolidationQueueCode, Balance: common.Big0, Storage: inhibited},
	}
	db := newGenesisState(t, alloc)

	var blocks []*blockdb.Block
	var parent *types.Header
	for i := range 100 {
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
		blocks = append(blocks, toBlockDbBlock(t, header))
		parent = header
	}
	return writeBlockDb(t, rules, alloc, blocks, nil)
}

// newGenesisState returns an in-memory state holding the given accounts.
func newGenesisState(t *testing.T, alloc types.GenesisAlloc) *state.StateDB {
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

// toBlockDbBlock returns the block database's form of the empty block with the
// given header, and checks that converting it back keeps the block's hash.
func toBlockDbBlock(t *testing.T, header *types.Header) *blockdb.Block {
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

// writeBlockDb writes a genesis with the given rules and accounts, and the given
// blocks and corrections into a new block database, and returns the database's
// directory and the genesis file.
func writeBlockDb(t *testing.T, rules opera.Rules, alloc types.GenesisAlloc, blocks []*blockdb.Block, corrections Corrections) (string, string) {
	require := require.New(t)
	dir := t.TempDir()

	accounts := make([]makefakegenesis.Account, 0, len(alloc))
	for address, account := range alloc {
		accounts = append(accounts, makefakegenesis.Account{
			Address: address,
			Balance: uint256.MustFromBig(account.Balance),
			Code:    account.Code,
			Nonce:   account.Nonce,
			Storage: account.Storage,
		})
	}
	genesis, err := json.Marshal(makefakegenesis.GenesisJson{Rules: rules, Accounts: accounts})
	require.NoError(err)
	genesisFile := filepath.Join(dir, "genesis.json")
	require.NoError(os.WriteFile(genesisFile, genesis, 0644))

	path := filepath.Join(dir, "block-db")
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	defer options.Destroy()
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err)
	defer db.Close()
	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()

	version := binary.BigEndian.AppendUint64(nil, blockdb.CurrentVersion)
	require.NoError(db.Put(writeOptions, blockdb.MakeVersionKey(), version))
	if corrections != nil {
		data, err := json.Marshal(corrections)
		require.NoError(err)
		require.NoError(db.Put(writeOptions, blockdb.MakeCorrectionsKey(rules.NetworkID), data))
	}

	for _, block := range blocks {
		data, err := proto.Marshal(block)
		require.NoError(err)
		require.NoError(db.Put(writeOptions, blockdb.MakeBlockKey(rules.NetworkID, block.Number), data))
	}
	return path, genesisFile
}
