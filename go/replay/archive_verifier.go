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

package replay

import (
	"context"
	"fmt"
	"log/slog"
	"math"
	"math/big"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/params"
)

const archiveVerificationOffset = 1000

// archiveVerifier re-executes old blocks against archive states to verify that
// the archive produces correct results. It runs a background goroutine that
// processes blocks submitted via submit().
type archiveVerifier struct {
	blockCh      chan uint64
	runWithState func(func(*State) error) error
	blockDB      blockdb.BlockDB
	metadata     MetadataStore
	interpreter  tosca.Interpreter
	schema       carmen.Schema
	chainID      uint64
	ctx          context.Context
	cancel       context.CancelFunc
	errCh        chan error
}

func newArchiveVerifier(
	ctx context.Context,
	runWithState func(func(*State) error) error,
	blockDB blockdb.BlockDB,
	metadata MetadataStore,
	interpreter tosca.Interpreter,
	schema carmen.Schema,
	chainID uint64,
) *archiveVerifier {
	ctx, cancel := context.WithCancel(ctx)
	v := &archiveVerifier{
		blockCh:      make(chan uint64, 16),
		runWithState: runWithState,
		blockDB:      blockDB,
		metadata:     metadata,
		interpreter:  interpreter,
		schema:       schema,
		chainID:      chainID,
		ctx:          ctx,
		cancel:       cancel,
		errCh:        make(chan error, 1),
	}
	go v.worker()
	return v
}

func (v *archiveVerifier) worker() {
	var err error
	defer func() { v.errCh <- err }()
	for blockNumber := range v.blockCh {
		if v.ctx.Err() != nil {
			err = v.ctx.Err()
			return
		}
		if err = v.verifyBlockOnArchive(blockNumber); err != nil {
			v.cancel()
			return
		}
	}
}

// submit enqueues a block number for archive verification.
func (v *archiveVerifier) submit(blockNumber uint64) {
	select {
	case v.blockCh <- blockNumber:
	case <-v.ctx.Done():
	}
}

// close signals no more blocks will be submitted and waits for the worker to
// finish. Returns any error encountered during verification.
func (v *archiveVerifier) close() error {
	close(v.blockCh)
	return <-v.errCh
}

func (v *archiveVerifier) verifyBlockOnArchive(blockNumber uint64) error {
	// Fetch the block from the database.
	block, err := v.blockDB.Get(v.chainID, blockNumber)
	if err != nil {
		return fmt.Errorf("archive verifier: failed to get block %d: %w", blockNumber, err)
	}

	gethBlock, err := convert.ConvertToGethBlock(block)
	if err != nil {
		return fmt.Errorf("archive verifier: failed to convert block %d: %w", blockNumber, err)
	}

	// Obtain archive state for the parent block.
	var archiveDB carmen.NonCommittableStateDB
	err = v.runWithState(func(s *State) error {
		var e error
		archiveDB, e = s.GetArchiveStateDB(blockNumber - 1)
		return e
	})
	if err != nil {
		return fmt.Errorf("archive verifier: failed to get archive state for block %d: %w", blockNumber-1, err)
	}
	defer archiveDB.Release()

	// Build chain config and processor.
	var chainConfig *params.ChainConfig
	var upgrades opera.Upgrades
	if cfg := ethereumChainConfigMap[v.chainID]; cfg != nil {
		chainConfig = cfg
		rules := chainConfig.Rules(gethBlock.Number(), false, gethBlock.Time())
		upgrades = opera.Upgrades{
			Berlin: rules.IsBerlin,
			London: rules.IsLondon,
			Llr:    false,

			Sonic:   rules.IsCancun,
			Allegro: rules.IsPrague,
			Brio:    rules.IsOsaka,

			SingleProposerBlockFormation: false,
			GasSubsidies:                 false,
			TransactionBundles:           false,
		}
	} else {
		chainConfig = opera.CreateTransientEvmChainConfig(
			v.chainID,
			v.metadata.GetUpgradeHeights(),
			idx.Block(blockNumber),
		)
		upgrades = v.metadata.GetUpgradesAtBlock(blockNumber)
	}

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		&blockHashHistory{}, // archive verification does not need accurate block hashes
		upgrades,
	)

	// Prepare the EVM block.
	isPostMerge := gethBlock.Difficulty().Sign() == 0
	blobBaseFee := big.NewInt(1)
	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsCancun(gethBlock.Number(), gethBlock.Time()) && gethBlock.ExcessBlobGas() != nil {
		blobBaseFee = eip4844.CalcBlobFee(chainConfig, gethBlock.Header())
	}
	prevRandao := gethBlock.Header().MixDigest
	if !isPostMerge {
		prevRandao = common.Hash{}
	}

	evmBlock := &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      gethBlock.Number(),
			ParentHash:  gethBlock.ParentHash(),
			Time:        inter.Timestamp(gethBlock.Time() * 1e9),
			GasLimit:    gethBlock.GasLimit(),
			PrevRandao:  prevRandao,
			BaseFee:     gethBlock.BaseFee(),
			BlobBaseFee: blobBaseFee,
			Coinbase:    gethBlock.Coinbase(),
		},
		Transactions: gethBlock.Transactions(),
	}

	stateDB := evmstore.CreateNonCommittableCarmenStateDb(archiveDB, nil)

	var vmConfig vm.Config
	if !isEthereum(chainConfig.ChainID.Uint64()) {
		vmConfig = opera.GetVmConfig(opera.Rules{Upgrades: upgrades})
	}
	vmConfig.Interpreter = geth_adapter.NewGethInterpreterFactory(v.interpreter)

	// Apply EIP-4788 system call before transaction processing.
	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsCancun(gethBlock.Number(), gethBlock.Time()) {
		if beaconRoot := gethBlock.BeaconRoot(); beaconRoot != nil {
			err := processSystemCall(&evmBlock.EvmHeader, stateDB, chainConfig, vmConfig, params.BeaconRootsAddress, beaconRoot.Bytes())
			if err != nil {
				return fmt.Errorf("archive verifier: failed to process EIP-4788 system call for block %d: %w", blockNumber, err)
			}
		}
	}

	// Process transactions.
	var usedGas uint64
	processed := processor.ProcessWithDifficulty(
		evmBlock,
		stateDB,
		vmConfig,
		gethBlock.GasLimit(),
		&usedGas,
		0,
		func(*core_types.Log) {}, // no-op log handler
		gethBlock.Difficulty(),
		math.MaxUint64,
	)

	// Collect receipts from processed transactions.
	finalReceipts := make(types.Receipts, len(processed.ProcessedTransactions))
	for i, proc := range processed.ProcessedTransactions {
		if proc.Receipt == nil {
			return fmt.Errorf("archive verifier: block %d tx %d was skipped", blockNumber, i)
		}
		finalReceipts[i] = proc.Receipt
	}

	// Validate receipts against expected values.
	if err := checkReceipts(block, finalReceipts); err != nil {
		return fmt.Errorf("archive verifier: %w", err)
	}

	slog.Debug("Archive verification passed", "block", blockNumber)
	return nil
}
