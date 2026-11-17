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
	"os"
	"path/filepath"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/replay/sonic"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
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
func createSonicBlockDb(t *testing.T, corrections sonic.Corrections) (string, string) {
	rules := opera.FakeNetRules(opera.GetAllegroUpgrades())
	alloc := types.GenesisAlloc{
		params.HistoryStorageAddress: {Nonce: 1, Code: params.HistoryStorageCode, Balance: common.Big0},
	}
	blocks := utils.CreateSonicBlocks(t, rules, utils.NewGenesisState(t, alloc), 100)
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
	blocks := utils.CreateEthereumBlocks(t, config, utils.NewGenesisState(t, alloc), 100)
	return writeBlockDb(t, rules, alloc, blocks, nil)
}

// writeBlockDb writes a genesis with the given rules and accounts, and the given
// blocks and corrections into a new block database, and returns the database's
// directory and the genesis file.
func writeBlockDb(t *testing.T, rules opera.Rules, alloc types.GenesisAlloc, blocks []*blockdb.Block, corrections sonic.Corrections) (string, string) {
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
