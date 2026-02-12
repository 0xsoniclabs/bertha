package app

import (
	"context"
	"fmt"
	"io"
	"path/filepath"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/carmen/go/common/witness"
	"github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/prque"
	"github.com/ethereum/go-ethereum/core/rawdb"
	geth "github.com/ethereum/go-ethereum/core/state"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/ethdb/leveldb"
	"github.com/ethereum/go-ethereum/triedb"
	"github.com/ethereum/go-ethereum/triedb/pathdb"
)

func init() {
	state.RegisterStateFactory(
		state.Configuration{
			Variant: "go-geth-ldb",
			Schema:  5,
			Archive: state.NoArchive,
		},
		func(params state.Parameters) (state.State, error) {
			return makeGethStateDBWithoutArchive(params.Directory)
		},
	)

	state.RegisterStateFactory(
		state.Configuration{
			Variant: "go-geth-ldb",
			Schema:  5,
			Archive: state.S5Archive,
		},
		func(params state.Parameters) (state.State, error) {
			return makeGethStateDBWithArchive(params.Directory)
		},
	)
}

func makeGethStateDBWithoutArchive(
	directory string,
) (*gethStateDB, error) {
	return _makeGethStateDB(directory, false)
}

func makeGethStateDBWithArchive(
	directory string,
) (*gethStateDB, error) {
	return _makeGethStateDB(directory, true)
}

func _makeGethStateDB(
	directory string,
	isArchiveMode bool,
) (*gethStateDB, error) {

	// TODO: load this from a file
	var rootHash common.Hash

	// create the live DB directory
	dir := filepath.Join(directory, "live")

	const cacheSize = 512
	const fileHandle = 128
	ldb, err := leveldb.New(dir, cacheSize, fileHandle, "", false)
	if err != nil {
		return nil, fmt.Errorf("failed to create a new Level DB, %w", err)
	}
	pathdbDefaults := pathdb.Defaults
	trieDb := triedb.NewDatabase(rawdb.NewDatabase(ldb), &triedb.Config{
		PathDB: &pathdb.Config{
			StateHistory:        0,
			EnableStateIndexing: false,
			TrieCleanSize:       pathdbDefaults.TrieCleanSize,
			StateCleanSize:      pathdbDefaults.StateCleanSize,
			WriteBufferSize:     pathdbDefaults.WriteBufferSize,
		},
	})

	evmState := geth.NewDatabase(trieDb, nil)
	if rootHash == (common.Hash{}) {
		rootHash = types.EmptyRootHash
	}
	db, err := geth.New(rootHash, evmState)
	if err != nil {
		return nil, err
	}

	return &gethStateDB{
		db:            db,
		evmState:      evmState,
		stateRoot:     cc.Hash(rootHash),
		triegc:        prque.New[uint64, common.Hash](nil),
		isArchiveMode: isArchiveMode,
		backend:       ldb,
	}, nil
}

type gethStateDB struct {
	db            *geth.StateDB // statedb
	evmState      geth.Database // key-value database
	stateRoot     cc.Hash       // lastest root hash
	triegc        *prque.Prque[uint64, common.Hash]
	isArchiveMode bool
	block         uint64
	backend       *leveldb.Database
}

func (s *gethStateDB) Exists(address cc.Address) (bool, error) {
	return s.db.Exist(common.Address(address)), nil
}

func (s *gethStateDB) GetBalance(address cc.Address) (amount.Amount, error) {
	return amount.NewFromUint256(s.db.GetBalance(common.Address(address))), nil
}

func (s *gethStateDB) GetNonce(address cc.Address) (cc.Nonce, error) {
	return cc.ToNonce(s.db.GetNonce(common.Address(address))), nil
}

func (s *gethStateDB) GetStorage(address cc.Address, key cc.Key) (cc.Value, error) {
	return cc.Value(s.db.GetState(common.Address(address), common.Hash(key))), nil
}

func (s *gethStateDB) GetCode(address cc.Address) ([]byte, error) {
	return s.db.GetCode(common.Address(address)), nil
}

func (s *gethStateDB) GetCodeSize(address cc.Address) (int, error) {
	return s.db.GetCodeSize(common.Address(address)), nil
}

func (s *gethStateDB) GetCodeHash(address cc.Address) (cc.Hash, error) {
	return cc.Hash(s.db.GetCodeHash(common.Address(address))), nil
}

func (s *gethStateDB) HasEmptyStorage(addr cc.Address) (bool, error) {
	return true, nil // not relevant for the Sonic chain
}

func (s *gethStateDB) Apply(block uint64, update cc.Update) error {

	oldStateRoot := s.stateRoot
	// init potentially empty accounts with empty code hash,
	for _, address := range update.CreatedAccounts {
		s.db.CreateAccount(common.Address(address))
	}

	for _, update := range update.Nonces {
		s.db.SetNonce(
			common.Address(update.Account),
			update.Nonce.ToUint64(),
			tracing.NonceChangeUnspecified,
		)
	}

	for _, update := range update.Balances {
		balance := update.Balance.Uint256()
		s.db.SetBalance(
			common.Address(update.Account),
			&balance,
			tracing.BalanceChangeUnspecified,
		)
	}

	for _, update := range update.Slots {
		s.db.SetState(
			common.Address(update.Account),
			common.Hash(update.Key),
			common.Hash(update.Value),
		)
	}

	for _, update := range update.Codes {
		s.db.SetCode(
			common.Address(update.Account),
			update.Code,
			tracing.CodeChangeUnspecified,
		)
	}

	// Commit those changes.
	s.db.Finalise(true)
	stateRoot, err := s.db.Commit(block, true, false)
	if err != nil {
		return fmt.Errorf("StateDB commit failed: %w", err)
	}
	s.stateRoot = cc.Hash(stateRoot)
	s.block = block

	// Save trie changes to the database.
	if s.isArchiveMode {
		tdb := s.db.Database().TrieDB()
		if err := tdb.Commit(stateRoot, false); err != nil {
			return err
		}
	}

	newDB, err := geth.New(common.Hash(s.stateRoot), s.evmState)
	if err != nil {
		return fmt.Errorf("failed to create new StateDB: %w", err)
	}
	s.db = newDB

	if oldStateRoot != s.stateRoot {
		s.db.Database().TrieDB().Dereference(common.Hash(oldStateRoot))
	}
	return nil
}

func (s *gethStateDB) GetHash() (cc.Hash, error) {
	return s.stateRoot, nil
}

func (s *gethStateDB) GetCommitment() future.Future[result.Result[cc.Hash]] {
	return future.Immediate(result.Ok(s.stateRoot))
}

func (s *gethStateDB) Flush() error {
	// Close underlying trie caching intermediate results.
	tdb := s.db.Database().TrieDB()
	if err := tdb.Commit(common.Hash(s.stateRoot), true); err != nil {
		return err
	}
	return nil
}

func (s *gethStateDB) Close() error {
	// Commit data to trie.
	hash, err := s.db.Commit(s.block, true, false)
	if err != nil {
		return err
	}

	// Close underlying trie caching intermediate results.
	tdb := s.db.Database().TrieDB()
	if err := tdb.Commit(hash, true); err != nil {
		return err
	}
	// Close underlying LevelDB instance.
	if err := tdb.Close(); err != nil {
		return err
	}
	// backend can be nil if we are using an in-memory version of gethDb (offTheChainDb)
	// as this version of StateDB does not require a file system.
	return s.backend.Close()
}

func (s *gethStateDB) GetMemoryFootprint() *cc.MemoryFootprint {
	panic("not implemented")
}

func (s *gethStateDB) GetArchiveState(block uint64) (state.State, error) {
	panic("not implemented")
}

func (s *gethStateDB) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	panic("not implemented")
}

func (s *gethStateDB) Check() error {
	return nil
}

func (s *gethStateDB) CreateWitnessProof(address cc.Address, keys ...cc.Key) (witness.Proof, error) {
	panic("not implemented")
}

func (s *gethStateDB) Export(ctx context.Context, out io.Writer) (cc.Hash, error) {
	panic("not implemented")
}
