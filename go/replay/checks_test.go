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
			// IsMptConformant is only reached if checkReceipts succeeds, so
			// receipt-error cases will not call it at all.
			chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

			logger := utils.NewMockLogger(ctrl)
			// Warn is only emitted for specific cases (e.g. missing state root),
			// so most subtests do not call it.
			logger.EXPECT().Warn(gomock.Any(), gomock.Any()).AnyTimes()

			err := checkBlockResults(
				chain,
				tc.block,
				tc.receipts,
				tc.stateRootFuture,
				tc.hashOfParent,
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

func Test_checkStateRoot(t *testing.T) {
	chainID := uint64(12)
	computedStateRoot := common.HexToHash("0xab")
	zeroStateRoot := common.Hash{}
	otherStateRoot := common.HexToHash("0xcd")

	cases := map[string]struct {
		policy                      OverwriteStateRootPolicy
		storedStateRoot             common.Hash
		stateRootNotSetSeen         bool
		skipStateRootCheck          bool
		expectUpdate                bool
		expectWarning               bool
		expectedError               string
		expectedStateRootNotSetSeen bool
	}{
		"On overwrites when stored matches": {
			policy:          OverwriteStateRootPolicyOn,
			storedStateRoot: computedStateRoot,
			expectUpdate:    true,
		},
		"On overwrites when stored is zero": {
			policy:          OverwriteStateRootPolicyOn,
			storedStateRoot: zeroStateRoot,
			expectUpdate:    true,
		},
		"On overwrites when stored mismatches": {
			policy:          OverwriteStateRootPolicyOn,
			storedStateRoot: otherStateRoot,
			expectUpdate:    true,
		},
		"Uninitialized passes when stored matches": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: computedStateRoot,
		},
		"Uninitialized writes when stored is zero": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: zeroStateRoot,
			expectUpdate:    true,
		},
		"Uninitialized errors on mismatch": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: otherStateRoot,
			expectedError:   "state root mismatch",
		},
		"Uninitialized with skipStateRootCheck writes when stored is zero": {
			policy:             OverwriteStateRootPolicyUninitialized,
			storedStateRoot:    zeroStateRoot,
			skipStateRootCheck: true,
			expectUpdate:       true,
		},
		"Uninitialized with skipStateRootCheck ignores mismatch": {
			policy:             OverwriteStateRootPolicyUninitialized,
			storedStateRoot:    otherStateRoot,
			skipStateRootCheck: true,
		},
		"Off passes when stored matches": {
			policy:          OverwriteStateRootPolicyOff,
			storedStateRoot: computedStateRoot,
		},
		"Off warns when stored is zero and not yet seen": {
			policy:                      OverwriteStateRootPolicyOff,
			storedStateRoot:             zeroStateRoot,
			expectWarning:               true,
			expectedStateRootNotSetSeen: true,
		},
		"Off does not warn when stored is zero and already seen": {
			policy:                      OverwriteStateRootPolicyOff,
			storedStateRoot:             zeroStateRoot,
			stateRootNotSetSeen:         true,
			expectedStateRootNotSetSeen: true,
		},
		"Off errors on mismatch": {
			policy:          OverwriteStateRootPolicyOff,
			storedStateRoot: otherStateRoot,
			expectedError:   "state root mismatch",
		},
		"Off with skipStateRootCheck ignores zero stored root without warning": {
			policy:             OverwriteStateRootPolicyOff,
			storedStateRoot:    zeroStateRoot,
			skipStateRootCheck: true,
		},
		"Off with skipStateRootCheck ignores mismatch": {
			policy:             OverwriteStateRootPolicyOff,
			storedStateRoot:    otherStateRoot,
			skipStateRootCheck: true,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			chain := NewMockChain(ctrl)
			chain.EXPECT().ChainID().Return(chainID).AnyTimes()
			chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

			block := &blockdb.Block{
				Number:    5,
				StateRoot: tc.storedStateRoot.Bytes(),
			}

			blockDB := blockdb.NewMockBlockDB(ctrl)
			if tc.expectUpdate {
				updatedBlock := &blockdb.Block{
					Number:    5,
					StateRoot: computedStateRoot.Bytes(),
				}
				blockDB.EXPECT().Update(chainID, updatedBlock).Return(nil)
			}

			logger := utils.NewMockLogger(ctrl)
			if tc.expectWarning {
				logger.EXPECT().Warn(
					"No state root set in the block DB. State root verification skipped",
					"block_number", block.Number,
				)
			}

			replayLoopContext := ReplayLoopContext{
				overwriteStateRoot:  tc.policy,
				stateRootNotSetSeen: tc.stateRootNotSetSeen,
				skipStateRootCheck:  tc.skipStateRootCheck,
			}

			err := checkStateRoot(
				chain,
				block,
				future.Immediate(result.Ok(computedStateRoot)),
				blockDB,
				&replayLoopContext,
				logger,
			)
			if tc.expectedError == "" {
				require.NoError(t, err)
			} else {
				require.ErrorContains(t, err, tc.expectedError)
			}
			require.Equal(t, tc.expectedStateRootNotSetSeen, replayLoopContext.stateRootNotSetSeen)
		})
	}
}

func Test_checkParentHash_LogsMessageIfPreviousBlockHashNotSet(t *testing.T) {
	ctrl := gomock.NewController(t)

	block := &blockdb.Block{
		Number:     3,
		ParentHash: common.Hash{0xAB}.Bytes(),
	}

	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Warn("No block hash set. Parent hash verification skipped", "block_number", block.Number)

	err := checkParentHash(block, common.Hash{}, logger)
	require.NoError(t, err)
}
