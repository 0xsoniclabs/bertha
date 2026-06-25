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
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
	gomock "go.uber.org/mock/gomock"
)

func Test_checkBlockResults_FailsIfComputedValuesMismatchStoredOnes(t *testing.T) {
	logAddress := common.Address{0x01}
	otherLogAddress := common.Address{0x02}
	topic1 := common.Hash{0x03}
	topic2 := common.Hash{0x04}
	logData := []byte{0x05, 0x06}
	otherLogData := []byte{0x07, 0x08}

	stateRoot := common.Hash{0x12}
	otherStateRoot := common.Hash{0x34}
	parentHash := common.Hash{0xAB}
	otherParentHash := common.Hash{0xCD}

	cases := map[string]struct {
		block              *blockdb.Block
		receipts           types.Receipts
		stateRootFuture    future.Future[result.Result[common.Hash]]
		hashOfParent       common.Hash
		skipStateRootCheck bool
		skipReceiptsCheck  bool
		expectedError      string
	}{
		"receipt count mismatch with disabled receipts check": {
			block: &blockdb.Block{
				Number:     2,
				Receipts:   []*blockdb.TransactionReceipt{},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts:          types.Receipts{{Status: types.ReceiptStatusSuccessful}},
			stateRootFuture:   future.Immediate(result.Ok(stateRoot)),
			hashOfParent:      parentHash,
			skipReceiptsCheck: true,
			expectedError:     "",
		},
		"receipt count mismatch": {
			block: &blockdb.Block{
				Number:     2,
				Receipts:   []*blockdb.TransactionReceipt{},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts:        types.Receipts{{Status: types.ReceiptStatusSuccessful}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "number of receipts mismatch",
		},
		"receipt status mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts:        types.Receipts{{Status: types.ReceiptStatusFailed}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt status mismatch",
		},
		"receipt cumulative gas used mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					CumulativeGasUsed: 100,
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts:        types.Receipts{{Status: types.ReceiptStatusSuccessful, CumulativeGasUsed: 200}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt cumulative gas used mismatch",
		},
		"log count mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes()}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: logAddress}, {Address: logAddress}},
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt logs length mismatch",
		},
		"log address mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes()}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: otherLogAddress}},
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt log address mismatch",
		},
		"log topics length mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes(), Topics: [][]byte{topic1.Bytes()}}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: logAddress, Topics: []common.Hash{topic1, topic2}}},
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt log topics length mismatch",
		},
		"log topic mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes(), Topics: [][]byte{topic1.Bytes()}}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: logAddress, Topics: []common.Hash{topic2}}},
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt log topic mismatch",
		},
		"log data mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes(), Data: logData}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: logAddress, Data: otherLogData}},
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt log data mismatch",
		},
		"receipt bloom mismatch": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs:              []*blockdb.Log{{Address: logAddress.Bytes()}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: types.Receipts{{
				Status: types.ReceiptStatusSuccessful,
				Logs:   []*types.Log{{Address: logAddress}},
				Bloom:  types.Bloom{0xFF}, // incorrect bloom that doesn't match logs
			}},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "receipt bloom mismatch",
		},
		"state root future error": {
			block: &blockdb.Block{
				Number:     2,
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			stateRootFuture: future.Immediate(result.Err[common.Hash](fmt.Errorf("state root computation failed"))),
			hashOfParent:    parentHash,
			expectedError:   "failed to get state root",
		},
		"state root mismatch": {
			block: &blockdb.Block{
				Number:     2,
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			stateRootFuture: future.Immediate(result.Ok(otherStateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "state root mismatch",
		},
		"state root mismatch with disabled state root check": {
			block: &blockdb.Block{
				Number:     2,
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			stateRootFuture:    future.Immediate(result.Ok(otherStateRoot)),
			hashOfParent:       parentHash,
			skipStateRootCheck: true,
			expectedError:      "",
		},
		"parent hash mismatch": {
			block: &blockdb.Block{
				Number:     2,
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    otherParentHash,
			expectedError:   "parent hash mismatch",
		},
		"all matching": {
			block: &blockdb.Block{
				Number: 2,
				Receipts: []*blockdb.TransactionReceipt{{
					PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
					Logs: []*blockdb.Log{{
						Address: logAddress.Bytes(),
						Topics:  [][]byte{topic1.Bytes()},
						Data:    logData,
					}},
				}},
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			},
			receipts: func() types.Receipts {
				r := &types.Receipt{
					Status: types.ReceiptStatusSuccessful,
					Logs:   []*types.Log{{Address: logAddress, Topics: []common.Hash{topic1}, Data: logData}},
				}
				r.Bloom = types.CreateBloom(r)
				return types.Receipts{r}
			}(),
			stateRootFuture: future.Immediate(result.Ok(stateRoot)),
			hashOfParent:    parentHash,
			expectedError:   "",
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			chain := NewMockChain(ctrl)
			chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
			chain.EXPECT().GetBlockHash(tc.block.Number - 1).Return(tc.hashOfParent).AnyTimes()

			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Warn(gomock.Any(), gomock.Any()).AnyTimes()

			err := checkBlockResults(
				chain,
				tc.block,
				tc.receipts,
				tc.stateRootFuture,
				nil, // the blockDB is only needed for state root overwriting which is not exercised in this test
				&ReplayLoopContext{skipStateRootCheck: tc.skipStateRootCheck, skipReceiptsCheck: tc.skipReceiptsCheck},
				logger,
			)

			if tc.expectedError == "" {
				require.NoError(t, err)
			} else {
				require.ErrorContains(t, err, tc.expectedError)
			}
		})
	}
}

func Test_checkStateRoot_OverwritesStateRoot(t *testing.T) {
	ctrl := gomock.NewController(t)

	chainID := uint64(12)
	oldStateRoot := common.HexToHash("0xdeadbeef")
	newStateRoot := common.HexToHash("0xfeedface")
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(chainID).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	block := &blockdb.Block{
		Number:    0,
		StateRoot: oldStateRoot.Bytes(),
	}

	blockWithUpdatedStateRoot := &blockdb.Block{
		Number:    0,
		StateRoot: newStateRoot.Bytes(),
	}

	blockDB := blockdb.NewMockBlockDB(ctrl)
	blockDB.EXPECT().
		Update(chainID, blockWithUpdatedStateRoot).
		Return(nil)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(true, true),
		stateRootNotSet:    false,
	}

	logger := utils.NewMockLogger(ctrl)

	err := checkStateRoot(
		chain,
		block,
		future.Immediate(result.Ok(newStateRoot)),
		blockDB,
		&replayLoopContext,
		logger,
	)
	require.NoError(t, err)
}

func Test_checkStateRoot_LogsMessageIfStateRootNotSet(t *testing.T) {
	ctrl := gomock.NewController(t)

	block := &blockdb.Block{
		Number: 1,
	}

	stateRoot := common.HexToHash("0xfeedface")
	chain := NewMockChain(ctrl)
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	blockDB := blockdb.NewMockBlockDB(ctrl)
	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Warn("No state root set in the block DB. State root verification skipped", "block_number", uint64(1))

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(false, false),
		stateRootNotSet:    false,
	}

	err := checkStateRoot(
		chain,
		block,
		future.Immediate(result.Ok(stateRoot)),
		blockDB,
		&replayLoopContext,
		logger,
	)
	require.NoError(t, err)

	logger = utils.NewMockLogger(ctrl)

	replayLoopContext = ReplayLoopContext{
		overwriteStateRoot: New(false, false),
		stateRootNotSet:    true,
	}

	err = checkStateRoot(
		chain,
		block,
		future.Immediate(result.Ok(stateRoot)),
		blockDB,
		&replayLoopContext,
		logger,
	)
	require.NoError(t, err)
}

func Test_checkParentHash_LogsMessageIfPreviousBlockHashNotSet(t *testing.T) {
	ctrl := gomock.NewController(t)

	block := &blockdb.Block{
		Number:     3,
		ParentHash: common.Hash{0xAB}.Bytes(),
	}

	chain := NewMockChain(ctrl)
	chain.EXPECT().GetBlockHash(uint64(block.Number - 1)).Return(common.Hash{})

	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Warn("No block hash set. Parent hash verification skipped", "block_number", block.Number)

	err := checkParentHash(chain, block, logger)
	require.NoError(t, err)
}
