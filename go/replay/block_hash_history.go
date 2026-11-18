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

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/ethereum/go-ethereum/common"
)

const blockHashHistoryLen = 256

// blockHashHistory keeps track of the last 256 block hashes. This is required
// for the BLOCKHASH opcode in the EVM.
// It implements the evmcore.DummyChain interface, allowing it to
// be used with the EVM state processor to serve historic block hashes.
type blockHashHistory struct {
	historicHashes [blockHashHistoryLen]common.Hash
}

func (h *blockHashHistory) Clone() *blockHashHistory {
	if h == nil {
		return nil
	}
	clone := *h
	return &clone
}

func (h *blockHashHistory) GetBlockHash(number uint64) common.Hash {
	return h.historicHashes[number%blockHashHistoryLen]
}

func (h *blockHashHistory) SetBlockHash(number uint64, hash common.Hash) {
	h.historicHashes[number%blockHashHistoryLen] = hash
}

func (h *blockHashHistory) Header(_ common.Hash, number uint64) *evmcore.EvmHeader {
	// The only information required from the header is the block number, the
	// block's hash, and the parent hash. Everything else is ignored by the EVM.
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.GetBlockHash(number),
		ParentHash: h.GetBlockHash(number - 1),
	}
}
