package app

import (
	"math/big"
	"os"
	"testing"

	"github.com/0xsoniclabs/blockdb"
	cc "github.com/0xsoniclabs/carmen/go/common"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

func TestState_CanBeCreatedAndClosed(t *testing.T) {
	modes := []carmen.ArchiveType{carmen.S5Archive, carmen.NoArchive}
	for _, archive := range modes {
		t.Run(string(archive), func(t *testing.T) {
			state, err := NewState(StateParameters{
				Directory: t.TempDir(),
				Archive:   archive,
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
		Archive:   carmen.NoArchive,
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
		Archive:   carmen.NoArchive,
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
		Archive:   carmen.NoArchive,
	})
	require.Error(t, err)
	require.Contains(t, err.Error(), "permission denied")
}

func TestState_Close_CanBeCalledOnClosedDb(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Archive:   carmen.NoArchive,
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
		Accounts: map[common.Address]Account{
			common.Address{1}: Account{
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
		Archive:   carmen.NoArchive,
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

	receipts, err := state.ApplyBlock(1, block, nil, nil)
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

	_, err = state.ApplyBlock(1, block, nil, nil)
	require.ErrorContains(t, err, "skipped txs")
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

	receipts, err := state.ApplyBlock(1, block, nil, corrections)
	require.NoError(t, err)
	require.Empty(t, receipts)

	require.Equal(t, uint64(1000), state.db.GetBalance(cc.Address{1}).Uint64())
}

func TestState_setBalance_CanIncreaseAndDecreaseBalance(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
		Archive:   carmen.NoArchive,
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
