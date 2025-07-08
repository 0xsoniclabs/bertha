package app

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"math/big"
	"os"
	"strings"
	"time"

	"github.com/0xsoniclabs/blockdb"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/interpreter/lfvm"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/urfave/cli/v3"
)

func init() {
	lfvm.RegisterExperimentalInterpreterConfigurations()
}

var (
	jsonGenesisFlag = &cli.StringFlag{
		Name:    "json-genesis",
		Aliases: []string{"g"},
		Usage:   "JSON encoded genesis data to use for replaying the blockchain",
	}

	stateDbDirectoryFlag = &cli.StringFlag{
		Name:    "state-db-dir",
		Aliases: []string{"sdb"},
		Usage:   "Path to the state database directory (default: OS-defined temporary directory)",
		Value:   "",
	}

	withArchiveFlag = &cli.BoolFlag{
		Name:    "with-archive",
		Aliases: []string{"a"},
		Usage:   "Use the archive mode for the state database",
		Value:   false,
	}

	// TODO: add flags for
	// - keep DB after run
	// - run block in "debug" mode, logging all steps
)

// TODO: fix known issues:
// Block 11553992
//   - Account: 10a46836f542ba043f0f3fb63387d3c1c17ec5bf
//   - Balance: 16239559479980437104

func getReplayCommand() *cli.Command {
	return &cli.Command{
		Name:   "replay",
		Usage:  "replay the full block chain from the block database",
		Action: runReplay,
		Flags: []cli.Flag{
			jsonGenesisFlag,
			blockDatabaseDirectoryFlag,
			stateDbDirectoryFlag,
			withArchiveFlag,
		},
	}
}

func runReplay(ctx context.Context, c *cli.Command) (err error) {

	genesisFileName := c.String(jsonGenesisFlag.Name)
	stateDbDirectory := c.String(stateDbDirectoryFlag.Name)
	blockDbDirectory := c.String(blockDatabaseDirectoryFlag.Name)
	withArchive := c.Bool(withArchiveFlag.Name)

	fmt.Printf("Loading genesis file %q ...\n", genesisFileName)

	// Create a temporary directory for the state database
	if stateDbDirectory == "" {
		stateDbDirectory = os.TempDir()
	}
	stateDbDirectory, err = os.MkdirTemp(stateDbDirectory, "replay_chain_state_")
	if err != nil {
		return fmt.Errorf("failed to create temporary state database directory: %w", err)
	}
	fmt.Printf("Using state database directory: %q\n", stateDbDirectory)
	defer fmt.Printf("State database directory: %q\n", stateDbDirectory)
	/*
		defer func() {
			err = errors.Join(err, os.RemoveAll(stateDbDirectory))
		}()
	*/

	// Open State Database in new directory.
	params := StateParameters{
		Directory: stateDbDirectory,
		Archive:   carmen.NoArchive,
	}
	if withArchive {
		params.Archive = carmen.S5Archive
	}
	state, err := NewState(params)
	if err != nil {
		return fmt.Errorf("failed to create state: %w", err)
	}
	defer func() {
		err = errors.Join(err, state.Close())
	}()

	// Apply genesis data to the state database.
	chainId, err := applyGenesis(state, genesisFileName)
	if err != nil {
		return fmt.Errorf("failed to apply genesis data: %w", err)
	}
	stateRoot, err := state.GetStateRoot()
	if err != nil {
		return fmt.Errorf("failed to get state root: %w", err)
	}
	fmt.Printf("Loaded genesis for network %d, resulting in state root %x\n", chainId, stateRoot)

	// Open the block database.
	fmt.Printf("Opening block database in %q ...\n", blockDbDirectory)
	database, err := blockdb.OpenDB(blockDbDirectory)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	corrections := getCorrections()

	lastUpdate := time.Now()
	lastTime := time.Unix(0, 0)
	txCounter := uint64(0)
	gasCounter := uint64(0)
	lastTxCounter := uint64(0)
	lastGasCounter := uint64(0)

	blockHashHistory := &blockHashHistory{}
	for block, err := range database.GetRange(chainId, 0, math.MaxUint64) {
		if err != nil {
			return fmt.Errorf("failed to get block: %w", err)
		}
		if ctx.Err() != nil {
			return ctx.Err()
		}

		if block.Number > 0 {
			blockHashHistory.SetBlockHash(block.Number-1, common.BytesToHash(block.ParentHash))
		}

		if block.Number != 0 && block.Number%10_000 == 0 {
			blockTime := time.Unix(int64(block.Timestamp), 0)
			deltaBlockTime := blockTime.Sub(lastTime)
			lastTime = blockTime
			deltaTx := txCounter - lastTxCounter
			deltaGas := gasCounter - lastGasCounter
			lastTxCounter = txCounter
			lastGasCounter = gasCounter
			now := time.Now()
			deltaTime := now.Sub(lastUpdate)
			lastUpdate = now
			fmt.Printf("Processing block %d @ %v, %.2f txs/s, %.2f MGas/s, %.2fx realtime\n",
				block.Number,
				blockTime,
				float64(deltaTx)/deltaTime.Seconds(),
				float64(deltaGas)/deltaTime.Seconds()/1000/1000,
				deltaBlockTime.Seconds()/deltaTime.Seconds(),
			)
		}

		gethBlock, err := ConvertToGethBlock(block)
		if err != nil {
			return fmt.Errorf("failed to convert block %d: %w", block.Number, err)
		}

		// Run the transactions in the block against the state database.
		if block.Number != 0 { // The archive can not handle block 0
			receipts, err := applyBlock(chainId, blockHashHistory, state, gethBlock, corrections)
			if err != nil {
				return fmt.Errorf("failed to apply block %d: %w", block.Number, err)
			}
			txCounter += uint64(len(gethBlock.Transactions()))
			gasCounter += gethBlock.GasUsed()

			// Check the receipts against the expected values in the block.
			for i, receipt := range receipts {
				want := block.Receipts[i]
				if receipt.Status != want.Status {
					return fmt.Errorf("receipt status mismatch for block %d, tx %d: expected %d, got %d",
						block.Number, i, want.Status, receipt.Status)
				}
				if receipt.CumulativeGasUsed != want.CumulativeGasUsed {
					return fmt.Errorf("receipt cumulative gas used mismatch for block %d, tx %d: expected %d, got %d",
						block.Number, i, want.CumulativeGasUsed, receipt.CumulativeGasUsed)
				}
				// TODO: check more fields if needed.
			}

			// TODO:
			// - check logs
		} else {
			lastTime = time.Unix(int64(block.Timestamp), 0)
		}

		// Check resulting state root.
		stateRoot, err := state.GetStateRoot()
		if err != nil {
			return fmt.Errorf("failed to get state root after applying block %d: %w", block.Number, err)
		}
		if common.Hash(block.StateRoot) != stateRoot {
			return fmt.Errorf("state root mismatch after applying block %d: expected %x, got %x",
				block.Number, block.StateRoot, stateRoot)
		}
	}

	// TODO:
	// - load genesis file
	// - process blocks from block DB

	return nil
}

type State struct {
	// TODO: replace with Carmen facade
	db carmen.StateDB
}

type StateParameters struct {
	Directory string
	Archive   carmen.ArchiveType
}

func NewState(params StateParameters) (*State, error) {
	dir := params.Directory
	err := os.MkdirAll(dir, 0700)
	if err != nil {
		return nil, fmt.Errorf("failed to create state dir %q; %v", dir, err)
	}

	state, err := carmen.NewState(carmen.Parameters{
		Directory:    dir,
		Variant:      "go-file",
		Schema:       carmen.Schema(5),
		Archive:      params.Archive,
		LiveCache:    100 * 1024 * 1024, // 100MB
		ArchiveCache: 100 * 1024 * 1024, // 100MB
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create state: %v", err)
	}
	db := carmen.CreateCustomStateDBUsing(state, 0)
	return &State{db: db}, nil
}

func (s *State) SetBalance(address common.Address, balance *big.Int) {
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

func (s *State) SetNonce(address common.Address, nonce uint64) {
	s.db.SetNonce(cc.Address(address), nonce)
}

func (s *State) SetCode(address common.Address, code []byte) {
	s.db.SetCode(cc.Address(address), code)
}

func (s *State) GetStateRoot() (common.Hash, error) {
	if s.db == nil {
		return common.Hash{}, fmt.Errorf("state is not initialized")
	}
	hash := s.db.GetHash()
	return common.Hash(hash), nil
}

func (s *State) Close() error {
	if s.db == nil {
		return nil
	}
	err := s.db.Close()
	s.db = nil
	return err
}

func applyGenesis(
	state *State,
	genesisFile string,
) (chainId uint64, _ error) {
	genesis := struct {
		Rules struct {
			NetworkID uint64 `json:"NetworkID"`
		}
		Accounts []struct {
			Address string            `json:"address"`
			Nonce   int               `json:"nonce"`
			Balance *big.Int          `json:"balance"`
			Code    string            `json:"code"`
			Storage map[string]string `json:"storage"`
		}
	}{}

	// read the genesis file
	data, err := os.ReadFile(genesisFile)
	if err != nil {
		return 0, fmt.Errorf("failed to read genesis file %q: %w", genesisFile, err)
	}
	if err := json.Unmarshal(data, &genesis); err != nil {
		return 0, fmt.Errorf("failed to unmarshal genesis file %q: %w", genesisFile, err)
	}

	// apply the genesis accounts to the state
	for _, account := range genesis.Accounts {
		address := common.HexToAddress(account.Address)
		if account.Code != "" {
			code, err := hex.DecodeString(strings.TrimPrefix(account.Code, "0x"))
			if err != nil {
				return 0, fmt.Errorf("failed to decode code for account %s: %w", account.Address, err)
			}
			state.SetCode(address, code)
		}
		if account.Balance != nil {
			state.SetBalance(address, account.Balance)
		}
		if account.Nonce > 0 {
			state.SetNonce(address, uint64(account.Nonce))
		}

		for k, v := range account.Storage {
			key, err := hex.DecodeString(strings.TrimPrefix(k, "0x"))
			if err != nil {
				return 0, fmt.Errorf("failed to decode storage key %s for account %s: %w", k, account.Address, err)
			}
			value, err := hex.DecodeString(strings.TrimPrefix(v, "0x"))
			if err != nil {
				return 0, fmt.Errorf("failed to decode storage value %s for account %s: %w", v, account.Address, err)
			}
			state.db.SetState(cc.Address(address), cc.Key(key), cc.Value(value))
		}
	}

	state.db.EndTransaction()
	state.db.EndBlock(0)

	return genesis.Rules.NetworkID, state.db.Check()
}

func applyBlock(
	chainId uint64,
	blockHashHistory *blockHashHistory,
	state *State,
	block *types.Block,
	corrections Corrections,
) (types.Receipts, error) {

	chainConfig := opera.CreateTransientEvmChainConfig(
		chainId,
		nil,
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessor(
		chainConfig,
		historyAdapter{history: blockHashHistory},
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

	stateDb := evmstore.CreateCarmenStateDb(state.db)

	vmConfig := opera.GetVmConfig(opera.Rules{})
	if false {
		vmConfig.Interpreter = func(evm *vm.EVM) vm.Interpreter {
			return vm.NewEVMInterpreter(evm)
		}
	}
	if false /*block.NumberU64() == 607404*/ {
		interpreter, err := tosca.NewInterpreter("lfvm-logging")
		if err != nil {
			panic(err)
		}
		vmConfig.Interpreter = geth_adapter.NewGethInterpreterFactory(interpreter)
	}
	gasLimit := block.GasLimit()

	state.db.BeginBlock()
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

	if fixes := corrections[block.NumberU64()]; len(fixes) > 0 {
		state.db.BeginTransaction()
		fmt.Printf("Applying corrections for block %d: %d accounts\n", block.NumberU64(), len(fixes))
		for addr, acc := range fixes {
			fmt.Printf("  - %s: balance %s\n", addr.Hex(), acc.Balance.ToBig().String())
			address := common.HexToAddress(addr.Hex())
			state.SetBalance(address, acc.Balance.ToBig())
		}
		state.db.EndTransaction()
	}

	state.db.EndBlock(block.NumberU64())
	return receipts, state.db.Check()
}

type blockHashHistory struct {
	historicHashes [256]common.Hash
}

func (b *blockHashHistory) GetBlockHash(number uint64) common.Hash {
	return b.historicHashes[number%256]
}

func (b *blockHashHistory) SetBlockHash(number uint64, hash common.Hash) {
	b.historicHashes[number%256] = hash
}

type historyAdapter struct {
	history *blockHashHistory
}

func (h historyAdapter) GetHeader(_ common.Hash, number uint64) *evmcore.EvmHeader {
	//fmt.Printf("Requesting block %d with hash %x and parent hash %x\n", number, h.history.GetBlockHash(number), h.history.GetBlockHash(number-1))
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.history.GetBlockHash(number),
		ParentHash: h.history.GetBlockHash(number - 1),
	}
}
