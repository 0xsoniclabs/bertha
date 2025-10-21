package app

import (
	"fmt"
	"math/big"
	"os"
	"testing"

	"github.com/0xsoniclabs/blockdb"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

func TestState_CanBeCreatedAndClosed(t *testing.T) {
	for _, archive := range []bool{false, true} {
		t.Run(fmt.Sprintf("withArchive=%t", archive), func(t *testing.T) {
			state, err := NewState(StateParameters{
				Directory:   t.TempDir(),
				WithArchive: archive,
			})
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
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	root := state.GetStateRoot()
	require.Equal(t, types.EmptyRootHash, root)
}

func TestNewState_FailsWithInvalidDirectory(t *testing.T) {
	_, err := NewState(StateParameters{
		Directory: "/invalid/directory/that/does/not/exist",
	})
	require.Error(t, err)
	require.Contains(t, err.Error(), "failed to create state dir")
}

func TestNewState_FailsIfDirectoryIsReadOnly(t *testing.T) {
	// Create a temporary directory and make it read-only
	tempDir := t.TempDir()
	err := os.Chmod(tempDir, 0555) // Read-only permissions
	require.NoError(t, err)
	defer os.Chmod(tempDir, 0755)

	_, err = NewState(StateParameters{
		Directory: tempDir,
	})
	require.Error(t, err)
	require.Contains(t, err.Error(), "permission denied")
}

func TestState_Close_CanBeCalledOnClosedDb(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)

	err = state.Close()
	require.NoError(t, err)

	// Calling Close again should not cause an error
	err = state.Close()
	require.NoError(t, err)
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
	})
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
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := ConvertToGethBlock(&blockdb.Block{})
	require.NoError(t, err)

	receipts, err := state.ApplyBlock(1, block, Metadata{})
	require.NoError(t, err)
	require.Empty(t, receipts)
}

func TestState_ApplyBlock_FailsOnSkippedTransaction(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := ConvertToGethBlock(&blockdb.Block{
		Transactions: []*blockdb.Transaction{
			&blockdb.Transaction{
				TransactionType: types.LegacyTxType,
				Nonce:           123, // all nonces are 0 in the DB
			},
		},
	})
	require.NoError(t, err)

	_, err = state.ApplyBlock(1, block, Metadata{})
	require.ErrorContains(t, err, "skipped txs")
}

func TestState_ApplyBlock_AppliesUpgrades(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	// To see an effect of upgrades, this test uses two different rule sets
	// such that gas costs for a simple transaction with excess gas differ.
	// If the single proposer block formation is enabled, no fees are charged
	// for transactions using too high gas limits. If it is disabled, a 10%
	// excess gas charge is applied.
	noExcessGasCharges := opera.GetAllegroUpgrades()
	noExcessGasCharges.SingleProposerBlockFormation = true

	withExcessGasCharges := opera.GetAllegroUpgrades()
	withExcessGasCharges.SingleProposerBlockFormation = false

	metadata := Metadata{
		Upgrades: []opera.UpgradeHeight{
			{Height: 5, Upgrades: noExcessGasCharges},
			{Height: 10, Upgrades: withExcessGasCharges},
			{Height: 15, Upgrades: noExcessGasCharges},
		},
	}

	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	signer := types.LatestSignerForChainID(big.NewInt(1))

	for blockNr := range 20 {
		// Apply a block before any upgrades.
		block, err := ConvertToGethBlock(&blockdb.Block{
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

		receipts, err := state.ApplyBlock(1, block, metadata)
		require.NoError(t, err)
		require.Len(t, receipts, 1)
		require.Equal(t, types.ReceiptStatusSuccessful, receipts[0].Status)

		rules := metadata.GetRulesAtBlock(uint64(blockNr))
		if rules.Upgrades.SingleProposerBlockFormation {
			require.Equal(t, uint64(21_000), receipts[0].GasUsed)
		} else {
			require.Greater(t, receipts[0].GasUsed, uint64(21_000))
		}
	}
}

func TestState_ApplyBlock_AppliesCorrections(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	block, err := ConvertToGethBlock(&blockdb.Block{Number: 17})
	require.NoError(t, err)

	corrections := Corrections{
		17: map[common.Address]Correction{
			common.Address{1}: {
				Balance: *uint256.NewInt(1000),
			},
		},
	}

	receipts, err := state.ApplyBlock(1, block, Metadata{
		Corrections: corrections,
	})
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
}

func TestState_setBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
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

	header := adapter.GetHeader(common.Hash{}, block)
	require.Equal(t, block, header.Number.Uint64())
	require.Equal(t, current, header.Hash)
	require.Equal(t, parent, header.ParentHash)

	header = adapter.GetHeader(common.Hash{}, block-1)
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
