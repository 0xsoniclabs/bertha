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

func Test_checkBlockResults_ForwardsErrorsOfChecks(t *testing.T) {
	stateRoot := common.Hash{0x12}
	otherStateRoot := common.Hash{0x34}
	parentHash := common.Hash{0xAB}
	otherParentHash := common.Hash{0xCD}

	// The stored block holds no receipts and the stateRoot and parentHash
	// variables; the case fields are checked against it.
	cases := map[string]struct {
		receipts          types.Receipts
		stateRoot         common.Hash
		hashOfParent      common.Hash
		skipReceiptsCheck bool
		failsOnReceipts   bool
		expectedError     string
	}{
		"receipts mismatch": {
			receipts:        types.Receipts{{Status: types.ReceiptStatusSuccessful}},
			stateRoot:       stateRoot,
			hashOfParent:    parentHash,
			failsOnReceipts: true,
			expectedError:   "number of receipts mismatch",
		},
		"receipts mismatch with disabled receipts check": {
			receipts:          types.Receipts{{Status: types.ReceiptStatusSuccessful}},
			stateRoot:         stateRoot,
			hashOfParent:      parentHash,
			skipReceiptsCheck: true,
		},
		"state root mismatch": {
			stateRoot:     otherStateRoot,
			hashOfParent:  parentHash,
			expectedError: "state root mismatch",
		},
		"parent hash mismatch": {
			stateRoot:     stateRoot,
			hashOfParent:  otherParentHash,
			expectedError: "parent hash mismatch",
		},
		"all matching": {
			stateRoot:    stateRoot,
			hashOfParent: parentHash,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			executor := NewMockHistoryExecutor(ctrl)
			if !tc.failsOnReceipts {
				executor.EXPECT().IsMptConformant().Return(true)
			}

			block := &blockdb.Block{
				Number:     2,
				StateRoot:  stateRoot.Bytes(),
				ParentHash: parentHash.Bytes(),
			}
			err := checkBlockResults(
				executor,
				block,
				tc.receipts,
				future.Immediate(result.Ok(tc.stateRoot)),
				tc.hashOfParent,
				nil, // the blockDB is only needed for state root overwriting which is not exercised in this test
				&ReplayLoopContext{skipReceiptsCheck: tc.skipReceiptsCheck},
				utils.NewMockLogger(ctrl), // no root or hash is missing, so nothing is logged
			)

			if tc.expectedError == "" {
				require.NoError(t, err)
			} else {
				require.ErrorContains(t, err, tc.expectedError)
			}
		})
	}
}

func Test_checkReceipts_FailsIfComputedReceiptsMismatchStoredOnes(t *testing.T) {
	logAddress := common.Address{0x01}
	otherLogAddress := common.Address{0x02}
	topic1 := common.Hash{0x03}
	topic2 := common.Hash{0x04}
	logData := []byte{0x05, 0x06}
	otherLogData := []byte{0x07, 0x08}
	successful := &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful}
	matching := &types.Receipt{
		Status: types.ReceiptStatusSuccessful,
		Logs:   []*types.Log{{Address: logAddress, Topics: []common.Hash{topic1}, Data: logData}},
	}
	matching.Bloom = types.CreateBloom(matching)

	cases := map[string]struct {
		stored        []*blockdb.TransactionReceipt
		receipts      types.Receipts
		expectedError string
	}{
		"receipt count mismatch": {
			stored:        []*blockdb.TransactionReceipt{},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful}},
			expectedError: "number of receipts mismatch",
		},
		"receipt status mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusFailed}},
			expectedError: "receipt status mismatch",
		},
		"receipt cumulative gas used mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, CumulativeGasUsed: 100}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, CumulativeGasUsed: 200}},
			expectedError: "receipt cumulative gas used mismatch",
		},
		"log count mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes()}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: logAddress}, {Address: logAddress}}}},
			expectedError: "receipt logs length mismatch",
		},
		"log address mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes()}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: otherLogAddress}}}},
			expectedError: "receipt log address mismatch",
		},
		"log topics length mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes(), Topics: [][]byte{topic1.Bytes()}}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: logAddress, Topics: []common.Hash{topic1, topic2}}}}},
			expectedError: "receipt log topics length mismatch",
		},
		"log topic mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes(), Topics: [][]byte{topic1.Bytes()}}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: logAddress, Topics: []common.Hash{topic2}}}}},
			expectedError: "receipt log topic mismatch",
		},
		"log data mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes(), Data: logData}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: logAddress, Data: otherLogData}}}},
			expectedError: "receipt log data mismatch",
		},
		"receipt bloom mismatch": {
			stored:        []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes()}}}},
			receipts:      types.Receipts{{Status: types.ReceiptStatusSuccessful, Logs: []*types.Log{{Address: logAddress}}, Bloom: types.Bloom{0xFF}}},
			expectedError: "receipt bloom mismatch",
		},
		"all matching": {
			stored:   []*blockdb.TransactionReceipt{{PostStateOrStatus: successful, Logs: []*blockdb.Log{{Address: logAddress.Bytes(), Topics: [][]byte{topic1.Bytes()}, Data: logData}}}},
			receipts: types.Receipts{matching},
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			err := checkReceipts(&blockdb.Block{Number: 2, Receipts: tc.stored}, tc.receipts)
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
		stateRootErr                error
		expectUpdate                bool
		expectWarning               bool
		expectedError               string
		expectedStateRootNotSetSeen bool
	}{
		"Off errors on failed state root": {
			policy:          OverwriteStateRootPolicyOff,
			storedStateRoot: computedStateRoot,
			stateRootErr:    fmt.Errorf("injected"),
			expectedError:   "failed to get state root after applying block 5",
		},
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
		"Uninitialized writes when stored is zero": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: zeroStateRoot,
			expectUpdate:    true,
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
		"Uninitialized errors on mismatch": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: otherStateRoot,
			expectedError:   "state root mismatch",
		},
		"Uninitialized passes when stored matches": {
			policy:          OverwriteStateRootPolicyUninitialized,
			storedStateRoot: computedStateRoot,
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
		"Off passes when stored matches": {
			policy:          OverwriteStateRootPolicyOff,
			storedStateRoot: computedStateRoot,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			executor := NewMockHistoryExecutor(ctrl)
			executor.EXPECT().ChainID().Return(chainID).AnyTimes()
			executor.EXPECT().IsMptConformant().Return(true).AnyTimes()

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

			stateRoot := result.Ok(computedStateRoot)
			if tc.stateRootErr != nil {
				stateRoot = result.Err[common.Hash](tc.stateRootErr)
			}
			err := checkStateRoot(
				executor,
				block,
				future.Immediate(stateRoot),
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

func Test_checkParentHash_FailsIfHashOfPreviousBlockMismatchesParentHash(t *testing.T) {
	cases := map[string]struct {
		hashOfParent  common.Hash
		expectWarning bool
		expectedError string
	}{
		"previous unset": {expectWarning: true},
		"mismatching":    {hashOfParent: common.Hash{0xCD}, expectedError: "parent hash mismatch: hash of block 2 is cd"},
		"matching":       {hashOfParent: common.Hash{0xAB}},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			block := &blockdb.Block{Number: 3, ParentHash: common.Hash{0xAB}.Bytes()}
			logger := utils.NewMockLogger(gomock.NewController(t))
			if tc.expectWarning {
				logger.EXPECT().Warn("No block hash set. Parent hash verification skipped", "block_number", block.Number)
			}

			err := checkParentHash(block, tc.hashOfParent, logger)
			if tc.expectedError == "" {
				require.NoError(t, err)
			} else {
				require.ErrorContains(t, err, tc.expectedError)
			}
		})
	}
}
