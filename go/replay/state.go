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
	"log/slog"
	"math"
	"math/big"
	"os"
	"strings"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/inter/state"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/params"
	"github.com/holiman/uint256"
	// Uncomment to enable experimental Carmen features.
	//_ "github.com/0xsoniclabs/carmen/go/experimental"
)

//go:generate mockgen -source=state.go -destination=state_mock.go -package=replay

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

type Processor interface {
	ProcessWithDifficulty(
		block *evmcore.EvmBlock,
		statedb state.StateDB,
		cfg vm.Config,
		gasLimit uint64,
		usedGas *uint64,
		trueTxOffset int,
		onNewLog func(*core_types.Log),
		difficulty *big.Int,
		remainingSize uint64,
	) evmcore.ProcessSummary
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
	block *types.Block,
	interpreter tosca.Interpreter,
	processor Processor,
	upgrades opera.Upgrades,
	corrections map[common.Address]Correction,
	chainConfig *params.ChainConfig,
	onLog func(*core_types.Log),
) (types.Receipts, error) {
	isPostMerge := block.Difficulty().Sign() == 0
	blobBaseFee := big.NewInt(1)
	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsCancun(block.Number(), block.Time()) && block.ExcessBlobGas() != nil {
		blobBaseFee = eip4844.CalcBlobFee(chainConfig, block.Header())
	}
	prevRandao := block.Header().MixDigest
	if !isPostMerge {
		// Before the Merge, PrevRandao is not used; set to zero. This indicates
		// to the EVM that the difficulty should be used instead.
		prevRandao = common.Hash{}
	}

	evmBlock := &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  prevRandao,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: blobBaseFee,
			Coinbase:    block.Coinbase(),
		},
		Transactions: block.Transactions(),
	}

	stateDB := evmstore.CreateCarmenStateDb(s.db, nil)

	var vmConfig vm.Config
	if !isEthereum(chainConfig.ChainID.Uint64()) {
		// Apply Sonic-specific VM settings that are not applicable to Ethereum chains.
		vmConfig = opera.GetVmConfig(opera.Rules{Upgrades: upgrades})
	}
	vmConfig.Interpreter = geth_adapter.NewGethInterpreterFactory(interpreter)

	zone := tracy.ZoneBegin("TransactionProcessing")
	s.db.BeginBlock()

	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsCancun(block.Number(), block.Time()) {
		// EIP-4788: store the parent beacon block root in the beacon roots contract.
		if beaconRoot := block.BeaconRoot(); beaconRoot != nil {
			err := processSystemCall(&evmBlock.EvmHeader, stateDB, chainConfig, vmConfig, params.BeaconRootsAddress, beaconRoot.Bytes())
			if err != nil {
				return nil, fmt.Errorf("failed to process EIP-4788 system call: %v", err)
			}
		}
	}

	var usedGas uint64
	processed := processor.ProcessWithDifficulty(
		evmBlock,
		stateDB,
		vmConfig,
		block.GasLimit(),
		&usedGas,
		0, // Tx index offset
		onLog,
		block.Difficulty(),
		math.MaxUint64,
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

	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsPrague(block.Number(), block.Time()) {
		// EIP-7002: call the withdrawal request contract as a system call.
		err := processSystemCall(&evmBlock.EvmHeader, stateDB, chainConfig, vmConfig, params.WithdrawalQueueAddress, nil)
		if err != nil {
			return nil, fmt.Errorf("failed to process EIP-7002 system call: %v", err)
		}

		// EIP-7251: call the consolidation request contract as a system call.
		err = processSystemCall(&evmBlock.EvmHeader, stateDB, chainConfig, vmConfig, params.ConsolidationQueueAddress, nil)
		if err != nil {
			return nil, fmt.Errorf("failed to process EIP-7251 system call: %v", err)
		}
	}

	// Apply corrections if any are provided.
	if len(corrections) > 0 {
		s.db.BeginTransaction()
		slog.Info("Applying corrections", "block", block.NumberU64())
		for addr, acc := range corrections {
			slog.Info("Correcting account",
				"address", addr.Hex(),
				"old_balance", s.db.GetBalance(cc.Address(addr)).ToBig().String(),
				"new_balance", acc.Balance.ToBig().String(),
			)
			s.setBalance(addr, acc.Balance.ToBig())
		}
		s.db.EndTransaction()
	}

	if isEthereum(chainConfig.ChainID.Uint64()) {
		if isPostMerge {
			creditWithdrawals(block, s.db, chainConfig)
		} else {
			accumulateRewards(chainConfig, s.db, block.Header(), block.Uncles())
		}
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

// processSystemCall executes a system call to the given address with the provided input data.
func processSystemCall(
	header *evmcore.EvmHeader,
	stateDB *evmstore.CarmenStateDB,
	chainConfig *params.ChainConfig,
	vmConfig vm.Config,
	addr common.Address,
	data []byte,
) error {
	// the chain is not needed for the current system calls
	blockContext := evmcore.NewEVMBlockContextWithDifficulty(header, nil, nil, big.NewInt(0))
	evm := vm.NewEVM(blockContext, stateDB, chainConfig, vmConfig)

	msg := &core.Message{
		From:      params.SystemAddress,
		GasLimit:  30_000_000,
		GasPrice:  common.Big0,
		GasFeeCap: common.Big0,
		GasTipCap: common.Big0,
		To:        &addr,
		Data:      data,
	}

	txContext, err := evmcore.NewEVMTxContext(msg)
	if err != nil {
		return fmt.Errorf("failed to create EVM transaction context: %w", err)
	}
	evm.SetTxContext(txContext)
	stateDB.AddAddressToAccessList(addr)
	defer stateDB.EndTransaction()
	_, _, err = evm.Call(msg.From, *msg.To, msg.Data, msg.GasLimit, common.U2560)
	if err != nil {
		return fmt.Errorf("failed to execute system call: %w", err)
	}
	return nil
}

func creditWithdrawals(block *types.Block, stateDB carmen.StateDB, chainConfig *params.ChainConfig) {
	// Derived from https://github.com/0xsoniclabs/go-ethereum/blob/949ae6d396a5798262c0d228a8de0e3fa504e00c/consensus/beacon/consensus.go#L329-L342
	for _, w := range block.Withdrawals() {
		// Convert amount from gwei to wei.
		amnt := new(uint256.Int).SetUint64(w.Amount)
		amnt = amnt.Mul(amnt, uint256.NewInt(params.GWei))
		stateDB.AddBalance(cc.Address(w.Address), amount.NewFromUint256(amnt))
	}
}

// accumulateRewards credits the coinbase of the given block with the mining
// reward. The total reward consists of the static block reward and rewards for
// included uncles. The coinbase of each uncle block is also rewarded.
// Copied from
// https://github.com/0xsoniclabs/go-ethereum/blob/949ae6d396a5798262c0d228a8de0e3fa504e00c/consensus/ethash/consensus.go#L570
func accumulateRewards(config *params.ChainConfig, stateDB carmen.StateDB, header *types.Header, uncles []*types.Header) {
	// Select the correct block reward based on chain progression
	blockReward := ethash.FrontierBlockReward
	if config.IsByzantium(header.Number) {
		blockReward = ethash.ByzantiumBlockReward
	}
	if config.IsConstantinople(header.Number) {
		blockReward = ethash.ConstantinopleBlockReward
	}
	// Accumulate the rewards for the miner and any included uncles
	reward := new(uint256.Int).Set(blockReward)
	r := new(uint256.Int)
	hNum, _ := uint256.FromBig(header.Number)
	for _, uncle := range uncles {
		uNum, _ := uint256.FromBig(uncle.Number)
		r.AddUint64(uNum, 8)
		r.Sub(r, hNum)
		r.Mul(r, blockReward)
		r.Rsh(r, 3)
		stateDB.AddBalance(cc.Address(uncle.Coinbase), amount.NewFromUint256(r))

		r.Rsh(blockReward, 5)
		reward.Add(reward, r)
	}
	stateDB.AddBalance(cc.Address(header.Coinbase), amount.NewFromUint256(reward))
}
