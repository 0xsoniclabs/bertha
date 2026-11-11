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
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/require"
)

func TestEthereumExecutor_ExecuteBlock_And_ExecuteBlockOnArchive_ServeBlockHashFromHashHistory(t *testing.T) {
	executeBlock := func(e *ethereumExecutor, state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
		receipts, _, err := e.ExecuteBlock(state, block, hashHistory)
		return receipts, err
	}
	executeBlockOnArchive := func(e *ethereumExecutor, state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
		return e.ExecuteBlockOnArchive(state, block, hashHistory)
	}
	tests := map[string]struct {
		execute      func(*ethereumExecutor, *State, *types.Block, *blockHashHistory) (types.Receipts, error)
		hashOfBlock1 common.Hash
		wantStatus   uint64
	}{
		"ExecuteBlock with hash":             {execute: executeBlock, hashOfBlock1: common.Hash{1}, wantStatus: types.ReceiptStatusSuccessful},
		"ExecuteBlock without hash":          {execute: executeBlock, wantStatus: types.ReceiptStatusFailed},
		"ExecuteBlockOnArchive with hash":    {execute: executeBlockOnArchive, hashOfBlock1: common.Hash{1}, wantStatus: types.ReceiptStatusSuccessful},
		"ExecuteBlockOnArchive without hash": {execute: executeBlockOnArchive, wantStatus: types.ReceiptStatusFailed},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			executor := &ethereumExecutor{chainID: 11155111, interpreter: testInterpreter(t)}
			for number := range int64(2) {
				_, _, err = executor.ExecuteBlock(state, types.NewBlockWithHeader(&types.Header{Number: big.NewInt(number + 1)}), &blockHashHistory{})
				require.NoError(t, err)
			}
			require.NoError(t, state.db.Flush())

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			// Init code that reverts if BLOCKHASH(1) is zero. It runs in block 3,
			// since the hash of the parent block comes from the block's ParentHash.
			tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(11155111)), &types.LegacyTx{
				Gas:  100_000,
				Data: common.FromHex("0x600140600a57600080fd5b00"),
			})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(3), GasLimit: 100_000}).
				WithBody(types.Body{Transactions: types.Transactions{tx}})
			hashHistory := &blockHashHistory{}
			hashHistory.SetBlockHash(1, tt.hashOfBlock1)

			receipts, err := tt.execute(executor, state, block, hashHistory)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Equal(t, tt.wantStatus, receipts[0].Status)
		})
	}
}
