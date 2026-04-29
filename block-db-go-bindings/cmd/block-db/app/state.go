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
	"fmt"
	"log/slog"
	"math/big"
	"os"
	"strings"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tracy"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	// Uncomment to enable experimental Carmen features.
	//_ "github.com/0xsoniclabs/carmen/go/experimental"
)

// State is an abstraction of the Chain State Database. It tracks the balances,
// nonces, codes, and storage states of accounts in the blockchain and provides
// transaction support for modifying these states.
//
// This type is an adapter for the Carmen state database, providing custom top
// level methods for managing instances in the context of the replay tool.
type State struct {
	// TODO: replace with Carmen facade
	db               carmen.StateDB
	blockHashHistory *blockHashHistory
	stateParameter   StateParameters
}

// StateParameters is a configuration struct for creating a new State instance.
type StateParameters struct {
	Directory   string
	WithArchive bool
	Schema      carmen.Schema
	Variant     carmen.Variant
}

// NewState creates a new State instance with the given parameters. The
// resulting state database is empty.
//
// Successfully created instances must be closed using the Close method.
func NewState(params StateParameters) (*State, error) {
	dir := params.Directory
	err := os.MkdirAll(dir, 0700)
	if err != nil {
		return nil, fmt.Errorf("failed to create state dir %q; %v", dir, err)
	}

	archive := carmen.NoArchive
	if params.WithArchive {
		if strings.HasPrefix(string(params.Variant), "rust") {
			archive = "file"
		} else if strings.HasPrefix(string(params.Variant), "go-geth2") {
			archive = carmen.LevelDbArchive
		} else {
			archive = carmen.S5Archive
		}
	}

	state, err := carmen.NewState(carmen.Parameters{
		Directory:    dir,
		Variant:      params.Variant,
		Schema:       params.Schema,
		Archive:      archive,
		LiveCache:    10 * 1024 * 1024 * 1024, // 10GB
		ArchiveCache: 10 * 1024 * 1024 * 1024, // 10GB
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create state: %v", err)
	}
	db := carmen.CreateCustomStateDBUsing(state, 0)
	return &State{db: db, blockHashHistory: &blockHashHistory{}, stateParameter: params}, nil
}

// Close closes the state database and releases any resources associated with it.
// After calling Close, the State instance should not be used anymore.
// If the state database was already closed, this method has no effect.
func (s *State) Close() error {
	if s.db == nil {
		return nil
	}
	err := s.db.Close()
	s.db = nil
	return err
}

// GetStateRoot returns the current state root hash of the state database.
func (s *State) GetStateRoot() future.Future[result.Result[common.Hash]] {
	return future.Then(s.db.GetCommitment(), func(res result.Result[cc.Hash]) result.Result[common.Hash] {
		value, err := res.Get()
		if err != nil {
			return result.Err[common.Hash](err)
		}
		return result.Ok(common.Hash(value))
	})
}

// ApplyGenesis applies the genesis data from the specified file on this state.
func (s *State) ApplyGenesis(genesis *Genesis) error {
	// apply the genesis accounts to the state
	s.db.BeginBlock()
	s.db.BeginTransaction()
	for _, account := range genesis.Accounts {
		address := account.Address
		s.db.AddBalance(cc.Address(address), amount.NewFromUint256(&account.Balance))
		if len(account.Code) != 0 {
			s.db.SetCode(cc.Address(address), account.Code)
		}
		if account.Nonce != 0 {
			s.db.SetNonce(cc.Address(address), account.Nonce)
		}
		for key, value := range account.Storage {
			s.db.SetState(cc.Address(address), cc.Key(key), cc.Value(value))
		}
	}
	s.db.EndTransaction()
	s.db.EndBlock(0)
	return s.db.Check()
}

// ApplyBlock applies the given block to this state, processing all transactions
// and updating the state accordingly. It returns the receipts of the transactions
// in the block, or an error if the block could not be processed.
func (s *State) ApplyBlock(
	chainId uint64,
	block *types.Block,
	metadata Metadata,
) (types.Receipts, error) {

	chainConfig := opera.CreateTransientEvmChainConfig(
		chainId,
		metadata.Upgrades,
		idx.Block(block.NumberU64()),
	)
	rules := metadata.GetRulesAtBlock(block.NumberU64())

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		historyAdapter{history: s.blockHashHistory},
		rules.Upgrades,
	)

	evmBlock := &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  block.Header().MixDigest,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: big.NewInt(1),
		},
		Transactions: block.Transactions(),
	}

	stateDb := evmstore.CreateCarmenStateDb(s.db, nil)

	vmConfig := opera.GetVmConfig(rules)
	gasLimit := block.GasLimit()

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	zone := tracy.ZoneBegin("TransactionProcessing")
	s.db.BeginBlock()
	var usedGas uint64
	processed := processor.Process(
		evmBlock,
		stateDb,
		vmConfig,
		gasLimit,
		&usedGas,
		0,
		nil,
	)

	// Check that all transactions were processed (i.e., none were skipped).
	for i, processed := range processed.ProcessedTransactions {
		if processed.Receipt == nil {
			return nil, fmt.Errorf("found block with skipped txs at index %d", i)
		}
	}

	// Retrieve the receipts from the processed transactions.
	receipts := make(types.Receipts, len(processed.ProcessedTransactions))
	for i, proc := range processed.ProcessedTransactions {
		receipts[i] = proc.Receipt
	}

	// Apply corrections if any are provided.
	if fixes := metadata.Corrections[block.NumberU64()]; len(fixes) > 0 {
		s.db.BeginTransaction()
		slog.Info("Applying corrections", "block", block.NumberU64())
		for addr, acc := range fixes {
			slog.Info("Correcting account",
				"address", addr.Hex(),
				"old_balance", s.db.GetBalance(cc.Address(addr)).ToBig().String(),
				"new_balance", acc.Balance.ToBig().String(),
			)
			s.setBalance(addr, acc.Balance.ToBig())
		}
		s.db.EndTransaction()
	}
	zone.End()

	zone = tracy.ZoneBegin("EndBlock")
	s.db.EndBlock(block.NumberU64())
	zone.End()
	return receipts, s.db.Check()
}

func (s *State) setBalance(address common.Address, balance *big.Int) {
	addr := cc.Address(address)
	cur := s.db.GetBalance(addr).ToBig()
	switch cur.Cmp(balance) {
	case -1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(balance, cur))
		s.db.AddBalance(addr, diff)
	case 1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(cur, balance))
		s.db.SubBalance(addr, diff)
	}
}

// --- block hash history tracking ---

// blockHashHistory keeps track of the last 256 block hashes. This is required
// for the BLOCKHASH opcode in the EVM.
type blockHashHistory struct {
	historicHashes [256]common.Hash
}

func (b *blockHashHistory) GetBlockHash(number uint64) common.Hash {
	return b.historicHashes[number%256]
}

func (b *blockHashHistory) SetBlockHash(number uint64, hash common.Hash) {
	b.historicHashes[number%256] = hash
}

// --- block hash history adapter ---

// historyAdapter implements the evmcore.DummyChain interface, allowing it to
// be used with the EVM state processor to serve historic block hashes.
type historyAdapter struct {
	history *blockHashHistory
}

func (h historyAdapter) Header(_ common.Hash, number uint64) *evmcore.EvmHeader {
	// The only information required from the header is the block number, the
	// block's hash, and the parent hash. Everything else is ignored by the EVM.
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.history.GetBlockHash(number),
		ParentHash: h.history.GetBlockHash(number - 1),
	}
}
