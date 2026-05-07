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
	"fmt"
	"math/big"
	"os"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestState_CanBeCreatedAndClosed(t *testing.T) {
	for _, archive := range []bool{false, true} {
		t.Run(fmt.Sprintf("withArchive=%t", archive), func(t *testing.T) {
			state, err := NewState(StateParameters{
				Directory:   t.TempDir(),
				WithArchive: archive,
				Schema:      5,
			}, &StaticMetadataStore{}, 0)
			require.NoError(t, err)
			require.NotNil(t, state)

			err = state.Close()
			require.NoError(t, err)
		})
	}
}

func TestNewState_CreatesEmptyDatabase(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
		Variant:   "go-file",
	}, &StaticMetadataStore{}, 0)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	root, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.Equal(t, types.EmptyRootHash, root)
}

func TestNewState_FailsWithInvalidDirectory(t *testing.T) {
	_, err := NewState(StateParameters{
		Directory: "/invalid/directory/that/does/not/exist",
		Schema:    5,
	}, &StaticMetadataStore{}, 0)
	require.Error(t, err)
	require.Contains(t, err.Error(), "failed to create state dir")
}

func TestNewState_FailsIfDirectoryIsReadOnly(t *testing.T) {
	// Create a temporary directory and make it read-only
	tempDir := t.TempDir()
	err := os.Chmod(tempDir, 0555) // Read-only permissions
	require.NoError(t, err)
	defer func() { require.NoError(t, os.Chmod(tempDir, 0755)) }()

	_, err = NewState(StateParameters{
		Directory: tempDir,
		Schema:    5,
	}, &StaticMetadataStore{}, 0)
	require.Error(t, err)
	require.Contains(t, err.Error(), "permission denied")
}

func TestState_Close_CanBeCalledOnClosedDb(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
	}, &StaticMetadataStore{}, 0)
	require.NoError(t, err)

	err = state.Close()
	require.NoError(t, err)

	// Calling Close again should not cause an error
	err = state.Close()
	require.NoError(t, err)
}

func TestState_GetStateRoot_ForwardsCallToDatabase(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	hash := cc.Hash{1, 2, 3}
	db.EXPECT().GetCommitment().Return(
		future.Immediate(result.Ok(hash)),
	)

	state := &State{db: db}

	got, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)
	require.Equal(t, common.Hash(hash), got)
}

func TestState_GetStateRoot_ForwardsErrorFromDatabase(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)

	issue := fmt.Errorf("database error")
	db.EXPECT().GetCommitment().Return(
		future.Immediate(result.Err[cc.Hash](issue)),
	)

	state := &State{db: db}

	_, err := state.GetStateRoot().Await().Get()
	require.ErrorIs(t, err, issue)
}

func TestState_ApplyGenesis_CanApplyGenesis(t *testing.T) {
	genesis := &Genesis{
		Accounts: []Account{
			{
				Address: common.Address{1},
				Balance: *uint256.NewInt(1000),
				Nonce:   1,
				Code:    []byte{1, 2, 3},
				Storage: map[common.Hash]common.Hash{
					common.HexToHash("0x1"): common.HexToHash("0x2"),
					common.HexToHash("0x3"): common.HexToHash("0x4"),
				},
			},
		},
	}

	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Schema:    5,
	}, &StaticMetadataStore{}, 0)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	require.NoError(t, state.ApplyGenesis(genesis))

	db := state.db
	addr := cc.Address{1}
	require.Equal(t, uint64(1000), db.GetBalance(addr).Uint64())
	require.Equal(t, uint64(1), db.GetNonce(addr))
	require.Equal(t, []byte{1, 2, 3}, db.GetCode(addr))

	key := cc.Key(common.HexToHash("0x1"))
	value := cc.Value(common.HexToHash("0x2"))
	require.Equal(t, value, db.GetState(addr, key))

	key = cc.Key(common.HexToHash("0x3"))
	value = cc.Value(common.HexToHash("0x4"))
	require.Equal(t, value, db.GetState(addr, key))
}

func TestState_ApplyBlock_CanApplyAnEmptyBlock(t *testing.T) {
	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
		&StaticMetadataStore{},
		1,
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{})
	require.NoError(t, err)

	receipts, err := state.ApplyBlock(block)
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestState_ApplyBlock_FailsOnSkippedTransaction(t *testing.T) {
	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
		&StaticMetadataStore{},
		1,
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Transactions: []*blockdb.Transaction{
			{
				TransactionType: types.LegacyTxType,
				Nonce:           123, // all nonces are 0 in the DB
			},
		},
	})
	require.NoError(t, err)

	_, err = state.ApplyBlock(block)
	require.ErrorContains(t, err, "skipped txs")
}

func TestState_ApplyBlock_AppliesUpgrades(t *testing.T) {
	// To see an effect of upgrades, this test uses two different rule sets
	// such that gas costs for a simple transaction with excess gas differ.
	// If the single proposer block formation is enabled, no fees are charged
	// for transactions using too high gas limits. If it is disabled, a 10%
	// excess gas charge is applied.
	noExcessGasCharges := opera.GetAllegroUpgrades()
	noExcessGasCharges.SingleProposerBlockFormation = true

	withExcessGasCharges := opera.GetAllegroUpgrades()
	withExcessGasCharges.SingleProposerBlockFormation = false

	metadataStore := &StaticMetadataStore{
		metadata: Metadata{
			Upgrades: []opera.UpgradeHeight{
				{Height: 5, Upgrades: noExcessGasCharges},
				{Height: 10, Upgrades: withExcessGasCharges},
				{Height: 15, Upgrades: noExcessGasCharges},
			},
		},
	}

	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
		metadataStore,
		1,
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	signer := types.LatestSignerForChainID(big.NewInt(1))

	for blockNr := range 20 {
		// Apply a block before any upgrades.
		block, err := convert.ConvertToGethBlock(&blockdb.Block{
			Number:   uint64(blockNr),
			GasLimit: 100_000,
			Transactions: []*blockdb.Transaction{
				toBerthaTransaction(types.MustSignNewTx(
					key,
					signer,
					&types.LegacyTx{
						Nonce: uint64(blockNr),
						To:    &common.Address{0},
						Gas:   50_000, // extra gas to 21_000 to check rule effect
					},
				)),
			},
		})
		require.NoError(t, err)

		receipts, err := state.ApplyBlock(block)
		require.NoError(t, err)
		require.Len(t, receipts, 1)
		require.Equal(t, types.ReceiptStatusSuccessful, receipts[0].Status)

		rules := metadataStore.GetRulesAtBlock(uint64(blockNr))
		if rules.Upgrades.SingleProposerBlockFormation {
			require.Equal(t, uint64(21_000), receipts[0].GasUsed)
		} else {
			require.Greater(t, receipts[0].GasUsed, uint64(21_000))
		}
	}
}

func TestState_ApplyBlock_AppliesCorrections(t *testing.T) {
	corrections := Corrections{
		17: map[common.Address]Correction{
			{1}: {
				Balance: *uint256.NewInt(1000),
			},
		},
	}

	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
		&StaticMetadataStore{metadata: Metadata{Corrections: corrections}},
		1,
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: 17})
	require.NoError(t, err)

	receipts, err := state.ApplyBlock(block)
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
}

func TestState_setBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
		&StaticMetadataStore{},
		1,
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	addr := common.Address{1}
	balance := big.NewInt(1000)
	state.setBalance(addr, balance)

	// Check initial balance
	have := state.db.GetBalance(cc.Address(addr))
	require.Equal(t, balance.Uint64(), have.Uint64())

	// Increase balance
	balance = big.NewInt(1500)
	state.setBalance(addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(1500), have.Uint64())

	// Decrease balance
	balance = big.NewInt(750)
	state.setBalance(addr, balance)
	have = state.db.GetBalance(cc.Address(addr))
	require.Equal(t, uint64(750), have.Uint64())
}

func TestBlockHashHistory_CanSetAndRetrieveHistoricHashes(t *testing.T) {
	history := &blockHashHistory{}
	for _, offset := range []uint64{0, 12, 1234} {
		for i := uint64(0); i < 256; i++ {
			history.SetBlockHash(i+offset, common.BytesToHash([]byte{byte(i + offset)}))
		}
		for i := uint64(0); i < 256; i++ {
			expected := common.BytesToHash([]byte{byte(i + offset)})
			actual := history.GetBlockHash(i + offset)
			require.Equal(t, expected, actual)
		}
	}
}

func TestHistoryAdapter_ProducesHeaderWithCorrectHashes(t *testing.T) {
	history := &blockHashHistory{}

	block := uint64(12)
	current := common.Hash{1, 2, 3}
	parent := common.Hash{4, 5, 6}
	grandParent := common.Hash{7, 8, 9}

	history.SetBlockHash(block, current)
	history.SetBlockHash(block-1, parent)
	history.SetBlockHash(block-2, grandParent)

	adapter := historyAdapter{history: history}

	header := adapter.Header(common.Hash{}, block)
	require.Equal(t, block, header.Number.Uint64())
	require.Equal(t, current, header.Hash)
	require.Equal(t, parent, header.ParentHash)

	header = adapter.Header(common.Hash{}, block-1)
	require.Equal(t, block-1, header.Number.Uint64())
	require.Equal(t, parent, header.Hash)
	require.Equal(t, grandParent, header.ParentHash)
}

func toBerthaTransaction(tx *types.Transaction) *blockdb.Transaction {
	to := []byte{}
	if tx.To() != nil {
		to = tx.To().Bytes()
	}
	v, r, s := tx.RawSignatureValues()
	return &blockdb.Transaction{
		TransactionType: uint64(tx.Type()),
		Nonce:           tx.Nonce(),
		GasPrice:        tx.GasPrice().Bytes(),
		GasLimit:        tx.Gas(),
		To:              to,
		Value:           tx.Value().Bytes(),
		Data:            tx.Data(),
		YParity:         v.Bytes(),
		R:               r.Bytes(),
		S:               s.Bytes(),
	}
}
