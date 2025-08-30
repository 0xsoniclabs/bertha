package app

import (
	"encoding/binary"
	"fmt"
	"log/slog"
	"math/big"
	"os"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/interpreter/lfvm"
	_ "github.com/0xsoniclabs/tosca/go/processor/geth"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/holiman/uint256"
)

func init() {
	lfvm.RegisterExperimentalInterpreterConfigurations()
}

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
}

// StateParameters is a configuration struct for creating a new State instance.
type StateParameters struct {
	Directory   string
	WithArchive bool
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
		archive = carmen.S5Archive
	}

	state, err := carmen.NewState(carmen.Parameters{
		Directory:    dir,
		Variant:      "go-file",
		Schema:       carmen.Schema(5),
		Archive:      archive,
		LiveCache:    100 * 1024 * 1024, // 100MB
		ArchiveCache: 100 * 1024 * 1024, // 100MB
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create state: %v", err)
	}
	db := carmen.CreateCustomStateDBUsing(state, 0)
	return &State{db: db, blockHashHistory: &blockHashHistory{}}, nil
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
func (s *State) GetStateRoot() common.Hash {
	return common.Hash(s.db.GetHash())
}

// ApplyGenesis applies the genesis data from the specified file on this state.
func (s *State) ApplyGenesis(genesis *Genesis) error {
	// apply the genesis accounts to the state
	s.db.BeginBlock()
	s.db.BeginTransaction()
	for address, account := range genesis.Accounts {
		if len(account.Code) != 0 {
			s.db.SetCode(cc.Address(address), account.Code)
		}
		s.setBalance(address, account.Balance.ToBig())
		s.db.SetNonce(cc.Address(address), account.Nonce)
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
	corrections Corrections,
) ([]tosca.Receipt, error) {
	return s.applyBlockUsingToscaProcessor(chainId, block, corrections)
	return s.applyBlockUsingGethProcessor(chainId, block, corrections)
}

// const targetBlock = 1 << 30 // 618770
const targetBlock = 1141735

// ERROR[09-01|16:49:13.053] Failed to run block-db                   error="state root mismatch after applying block 1141735: expected ad65bd562c8ee851c04864263954ce5e584ab7e932536e2f260ae9bc97ce2872, got ccbbae8c2c450ea0ffa4e145ce44cce5e194d086a370e61f6fb5a158bb54e95a"

func (s *State) applyBlockUsingToscaProcessor(
	chainId uint64,
	block *types.Block,
	corrections Corrections,
) ([]tosca.Receipt, error) {
	// TODO: create the processor at the beginning, only once
	interpreterFactory := tosca.GetInterpreterFactory("lfvm")
	if block.Number().Int64() == targetBlock {
		fmt.Printf("Block %d\n", block.Number().Int64())
		interpreterFactory = tosca.GetInterpreterFactory("lfvm-logging")
	}
	if interpreterFactory == nil {
		return nil, fmt.Errorf("failed to get interpreter factory")
	}
	interpreter, err := interpreterFactory(lfvm.Config{})
	if err != nil {
		return nil, fmt.Errorf("failed to create interpreter: %v", err)
	}
	processor := tosca.GetProcessor("geth-sonic", interpreter)
	if processor == nil {
		return nil, fmt.Errorf("failed to create processor instance")
	}

	blockParameter := tosca.BlockParameters{
		ChainID:     uint64ToWord(chainId),
		BlockNumber: block.Number().Int64(),
		Timestamp:   int64(block.Time()),
		Coinbase:    tosca.Address(block.Coinbase()),
		GasLimit:    tosca.Gas(block.GasLimit()),
		PrevRandao:  tosca.Hash(block.Header().MixDigest),
		BaseFee:     bigIntToValue(block.BaseFee()),
		BlobBaseFee: tosca.NewValue(1),
		Revision:    tosca.R13_Cancun,
	}

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	s.db.BeginBlock()

	// Run individual transactions.
	var receipts []tosca.Receipt
	signer := types.LatestSignerForChainID(big.NewInt(int64(chainId)))
	for i, tx := range block.Transactions() {
		if block.Number().Int64() == targetBlock {
			fmt.Printf("Block %d / Tx %d\n", block.Number().Int64(), i)
		}

		// TODO: factor transaction conversion out into its own function
		isInternal := func(tx *types.Transaction) bool {
			v, r, _ := tx.RawSignatureValues()
			return v.Sign() == 0 && r.Sign() == 0
		}

		var sender common.Address
		if !isInternal(tx) {
			sender, err = signer.Sender(tx)
			if err != nil {
				return nil, fmt.Errorf("failed to get sender: %v", err)
			}
		}
		var recipient *tosca.Address
		if tx.To() != nil {
			recipient = &tosca.Address{}
			copy((*recipient)[:], tx.To().Bytes())
		}

		var blobHashes []tosca.Hash
		for _, hash := range tx.BlobHashes() {
			blobHashes = append(blobHashes, tosca.Hash(hash))
		}

		var accessList []tosca.AccessTuple
		for _, entry := range tx.AccessList() {
			var keys []tosca.Key
			for _, key := range entry.StorageKeys {
				keys = append(keys, tosca.Key(key))
			}
			accessList = append(accessList, tosca.AccessTuple{
				Address: tosca.Address(entry.Address),
				Keys:    keys,
			})
		}

		transaction := tosca.Transaction{
			Sender:        tosca.Address(sender),
			Recipient:     recipient,
			Nonce:         tx.Nonce(),
			Input:         tx.Data(),
			Value:         bigIntToValue(tx.Value()),
			GasLimit:      tosca.Gas(tx.Gas()),
			GasFeeCap:     bigIntToValue(tx.GasFeeCap()),
			GasTipCap:     bigIntToValue(tx.GasTipCap()),
			BlobGasFeeCap: bigIntToValue(tx.BlobGasFeeCap()),
			BlobHashes:    blobHashes,
			AccessList:    accessList,
		}

		txContext := &transactionContextAdapter{
			db:      s.db,
			history: s.blockHashHistory,
		}

		s.db.BeginTransaction()
		receipt, err := processor.Run(blockParameter, transaction, txContext)
		s.db.EndTransaction()
		if err != nil {
			return nil, fmt.Errorf("failed to process transaction: %v", err)
		}
		receipts = append(receipts, receipt)

		// TODO: consider checking the block's gas limit (cumulative gas usage)
	}

	// Apply corrections if any are provided.
	if fixes := corrections[block.NumberU64()]; len(fixes) > 0 {
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

	s.db.EndBlock(block.NumberU64())

	return receipts, s.db.Check()
}

func uint64ToWord(value uint64) tosca.Word {
	var word tosca.Word
	binary.BigEndian.PutUint64(word[24:], value)
	return word
}

func bigIntToValue(value *big.Int) tosca.Value {
	var v tosca.Value
	if value != nil {
		value.FillBytes(v[:])
	}
	return v
}

func (s *State) applyBlockUsingGethProcessor(
	chainId uint64,
	block *types.Block,
	corrections Corrections,
) ([]tosca.Receipt, error) {
	// TODO: use Tosca's processor interface instead of Geth's processor.

	if block.Number().Int64() > targetBlock {
		panic("stop")
	}

	chainConfig := opera.CreateTransientEvmChainConfig(
		chainId,
		nil,
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessor(
		chainConfig,
		historyAdapter{history: s.blockHashHistory},
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

	stateDb := evmstore.CreateCarmenStateDb(s.db)

	vmConfig := opera.GetVmConfig(opera.Rules{})

	// For transaction processing, Tosca's LFVM is used.
	if block.Number().Int64() == targetBlock {
		interpreter, err := tosca.GetInterpreterFactory("lfvm-logging")(lfvm.Config{})
		if err != nil {
			panic(err)
		}
		vmConfig.Interpreter = geth_adapter.NewGethInterpreterFactory(interpreter)
	}

	gasLimit := block.GasLimit()

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	s.db.BeginBlock()
	var usedGas uint64
	receipts, _, skipped := processor.Process(
		evmBlock,
		stateDb,
		vmConfig,
		gasLimit,
		&usedGas,
		nil,
	)

	if len(skipped) > 0 {
		return nil, fmt.Errorf("found block with skipped txs: %d", len(skipped))
	}

	// Apply corrections if any are provided.
	if fixes := corrections[block.NumberU64()]; len(fixes) > 0 {
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

	s.db.EndBlock(block.NumberU64())

	res := []tosca.Receipt{}
	for _, r := range receipts {
		var contract *tosca.Address
		if r.ContractAddress != (common.Address{}) {
			contract = &tosca.Address{}
			copy((*contract)[:], r.ContractAddress[:])
		}
		var logs []tosca.Log
		for _, log := range r.Logs {
			var topics []tosca.Hash
			for _, topic := range log.Topics {
				topics = append(topics, tosca.Hash(topic))
			}
			logs = append(logs, tosca.Log{
				Address: tosca.Address(log.Address),
				Topics:  topics,
				Data:    log.Data,
			})
		}

		res = append(res, tosca.Receipt{
			Success:         r.Status == types.ReceiptStatusSuccessful,
			ContractAddress: contract,
			GasUsed:         tosca.Gas(r.GasUsed),
			BlobGasUsed:     tosca.Gas(r.BlobGasUsed),
			Logs:            logs,
		})
	}
	return res, s.db.Check()
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

func (h historyAdapter) GetHeader(_ common.Hash, number uint64) *evmcore.EvmHeader {
	// The only information required from the header is the block number, the
	// block's hash, and the parent hash. Everything else is ignored by the EVM.
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.history.GetBlockHash(number),
		ParentHash: h.history.GetBlockHash(number - 1),
	}
}

type transactionContextAdapter struct {
	db      carmen.StateDB
	history *blockHashHistory
}

var _ tosca.TransactionContext = (*transactionContextAdapter)(nil)

func (a *transactionContextAdapter) AccountExists(addr tosca.Address) bool {
	return a.db.Exist(cc.Address(addr))
}

func (a *transactionContextAdapter) CreateAccount(addr tosca.Address) {
	if !a.db.Exist(cc.Address(addr)) {
		a.db.CreateAccount(cc.Address(addr))
	}
	a.db.CreateContract(cc.Address(addr))
}

func (a *transactionContextAdapter) GetBalance(addr tosca.Address) tosca.Value {
	return tosca.Value(a.db.GetBalance(cc.Address(addr)).Bytes32())
}

func (a *transactionContextAdapter) SetBalance(addr tosca.Address, value tosca.Value) {
	current := a.db.GetBalance(cc.Address(addr)).Uint256()
	want := value.ToUint256()
	if want.Cmp(&current) > 0 {
		diff := new(uint256.Int).Sub(want, &current)
		a.db.AddBalance(cc.Address(addr), amount.NewFromUint256(diff))
	} else {
		diff := new(uint256.Int).Sub(&current, want)
		a.db.SubBalance(cc.Address(addr), amount.NewFromUint256(diff))
	}
}

func (a *transactionContextAdapter) GetNonce(addr tosca.Address) uint64 {
	return a.db.GetNonce(cc.Address(addr))
}

func (a *transactionContextAdapter) SetNonce(addr tosca.Address, nonce uint64) {
	a.db.SetNonce(cc.Address(addr), nonce)
}

func (a *transactionContextAdapter) GetCode(addr tosca.Address) tosca.Code {
	return tosca.Code(a.db.GetCode(cc.Address(addr)))
}

func (a *transactionContextAdapter) GetCodeHash(addr tosca.Address) tosca.Hash {
	return tosca.Hash(a.db.GetCodeHash(cc.Address(addr)))
}

func (a *transactionContextAdapter) GetCodeSize(addr tosca.Address) int {
	return a.db.GetCodeSize(cc.Address(addr))
}

func (a *transactionContextAdapter) SetCode(addr tosca.Address, code tosca.Code) {
	a.db.SetCode(cc.Address(addr), code)
}

func (a *transactionContextAdapter) HasEmptyStorage(addr tosca.Address) bool {
	return a.db.HasEmptyStorage(cc.Address(addr))
}

func (a *transactionContextAdapter) GetStorage(addr tosca.Address, key tosca.Key) tosca.Word {
	return tosca.Word(a.db.GetState(cc.Address(addr), cc.Key(key)))
}

func (a *transactionContextAdapter) SetStorage(addr tosca.Address, key tosca.Key, value tosca.Word) tosca.StorageStatus {
	original := tosca.Word(a.db.GetCommittedState(cc.Address(addr), cc.Key(key)))
	current := tosca.Word(a.db.GetState(cc.Address(addr), cc.Key(key)))
	res := tosca.GetStorageStatus(original, current, value)
	a.db.SetState(cc.Address(addr), cc.Key(key), cc.Value(value))
	return res
}

func (a *transactionContextAdapter) SelfDestruct(addr tosca.Address, beneficiary tosca.Address) bool {
	selfdestructed := !a.HasSelfDestructed(addr)

	if true /* Always Cancun so far */ {
		a.db.SuicideNewContract(cc.Address(addr))
	} else {
		a.db.Suicide(cc.Address(addr))
	}
	return selfdestructed
	/*
		// HasSelfDestructed only returns true if it is the first call to SelfDestruct
		selfdestructed := !a.db.HasSuicided(cc.Address(addr))

		balance := a.db.GetBalance(cc.Address(addr))
		a.db.AddBalance(cc.Address(beneficiary), balance)

		a.db.SubBalance(cc.Address(addr), balance)
		a.db.SuicideNewContract(cc.Address(addr))
		return selfdestructed
	*/
}

func (a *transactionContextAdapter) CreateSnapshot() tosca.Snapshot {
	return tosca.Snapshot(a.db.Snapshot())
}

func (a *transactionContextAdapter) RestoreSnapshot(snapshot tosca.Snapshot) {
	a.db.RevertToSnapshot(int(snapshot))
}

func (a *transactionContextAdapter) GetTransientStorage(addr tosca.Address, key tosca.Key) tosca.Word {
	return tosca.Word(a.db.GetTransientState(cc.Address(addr), cc.Key(key)))
}

func (a *transactionContextAdapter) SetTransientStorage(addr tosca.Address, key tosca.Key, value tosca.Word) {
	a.db.SetTransientState(cc.Address(addr), cc.Key(key), cc.Value(value))
}

func (a *transactionContextAdapter) AccessAccount(addr tosca.Address) tosca.AccessStatus {
	warm := a.IsAddressInAccessList(addr)
	a.db.AddAddressToAccessList(cc.Address(addr))
	if warm {
		return tosca.WarmAccess
	}
	return tosca.ColdAccess
}

func (a *transactionContextAdapter) AccessStorage(addr tosca.Address, key tosca.Key) tosca.AccessStatus {
	_, warm := a.IsSlotInAccessList(addr, key)
	a.db.AddSlotToAccessList(cc.Address(addr), cc.Key(key))
	if warm {
		return tosca.WarmAccess
	}
	return tosca.ColdAccess
}

func (a *transactionContextAdapter) EmitLog(log tosca.Log) {
	topics := make([]cc.Hash, len(log.Topics))
	for i, topic := range log.Topics {
		topics[i] = cc.Hash(topic)
	}
	a.db.AddLog(&cc.Log{
		Address: cc.Address(log.Address),
		Topics:  topics,
		Data:    log.Data,
	})
}

func (a *transactionContextAdapter) GetLogs() []tosca.Log {
	var logs []tosca.Log
	for _, log := range a.db.GetLogs() {
		var topics []tosca.Hash
		for _, topic := range log.Topics {
			topics = append(topics, tosca.Hash(topic))
		}
		logs = append(logs, tosca.Log{
			Address: tosca.Address(log.Address),
			Topics:  topics,
			Data:    log.Data,
		})
	}
	return logs
}

func (a *transactionContextAdapter) GetBlockHash(number int64) tosca.Hash {
	return tosca.Hash(a.history.GetBlockHash(uint64(number)))
}

func (a *transactionContextAdapter) GetCommittedStorage(addr tosca.Address, key tosca.Key) tosca.Word {
	return tosca.Word(a.db.GetCommittedState(cc.Address(addr), cc.Key(key)))
}

func (a *transactionContextAdapter) IsAddressInAccessList(addr tosca.Address) bool {
	return a.db.IsAddressInAccessList(cc.Address(addr))
}

func (a *transactionContextAdapter) IsSlotInAccessList(addr tosca.Address, key tosca.Key) (addressPresent, slotPresent bool) {
	return a.db.IsSlotInAccessList(cc.Address(addr), cc.Key(key))
}

func (a *transactionContextAdapter) HasSelfDestructed(addr tosca.Address) bool {
	return a.db.HasSuicided(cc.Address(addr))
}
