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
	"fmt"
	"os"
	"strings"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/ethereum/go-ethereum/common"
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
	db             carmen.StateDB
	stateParameter StateParameters
}

// StateParameters is a configuration struct for creating a new State instance.
type StateParameters struct {
	Directory   string
	WithArchive bool
	Schema      carmen.Schema
	Variant     carmen.Variant
}

// stateCacheSize is the size in bytes of the live and the archive cache of a
// State.
var stateCacheSize int64 = 10 * 1024 * 1024 * 1024 // 10GB

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
		LiveCache:    stateCacheSize,
		ArchiveCache: stateCacheSize,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create state: %v", err)
	}
	db := carmen.CreateCustomStateDBUsing(state, 0)
	return &State{
		db:             db,
		stateParameter: params,
	}, nil
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

// GetArchiveBlockHeight returns the highest block currently available in the
// archive. If the archive is empty, empty is true.
func (s *State) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	return s.db.GetArchiveBlockHeight()
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

// ApplyGenesis applies the accounts of the given genesis on this state. The
// genesis transactions are not executed since they are part of block 1, which
// is replayed like any other block.
func (s *State) ApplyGenesis(genesis *makefakegenesis.GenesisJson) error {
	// apply the genesis accounts to the state
	s.db.BeginBlock()
	s.db.BeginTransaction()
	for _, account := range genesis.Accounts {
		address := account.Address
		var balance amount.Amount
		if account.Balance != nil {
			balance = amount.NewFromUint256(account.Balance)
		}
		s.db.AddBalance(cc.Address(address), balance)
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
	if _, err := endBlockAndCommit(s, 0); err != nil {
		return err
	}
	return s.db.Check()
}

// endBlockAndCommit calls `EndBlock` on the state database and commits
// the staged block to the archive.
// It returns the handle to the archive commit operation, or an error if the operation failed.
func endBlockAndCommit(s *State, blockNum uint64) (*carmen.WaitHandle, error) {
	staged, err := s.db.EndBlock(blockNum)
	if err != nil {
		return nil, fmt.Errorf("failed to end block %d: %w", blockNum, err)
	}
	if staged == nil {
		return nil, fmt.Errorf("failed to end block %d: staged state is nil", blockNum)
	}
	handle, err := staged.Commit()
	if err != nil {
		return nil, fmt.Errorf("failed to commit block %d: %w", blockNum, err)
	}
	return handle, nil
}
