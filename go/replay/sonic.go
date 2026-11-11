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
	"io"
	"log/slog"
	"math/big"

	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
)

// onNewLog stages the rules diff carried by an UpdateNetworkRules event of
// Sonic's driver contract in the metadata store and ignores all other logs.
func onNewLog(metadataStore MetadataStore, blockNumber uint64, l *core_types.Log) {
	// https://github.com/0xsoniclabs/sonic/blob/c3816115c9ae51682aa475c715aabbe10e0dcef4/gossip/blockproc/drivermodule/driver_txs.go#L351
	if l.Address == driver.ContractAddress &&
		len(l.Topics) > 0 &&
		l.Topics[0] == driverpos.Topics.UpdateNetworkRules &&
		len(l.Data) >= 64 {
		diff, err := decodeDataBytes(l)
		if err != nil {
			slog.Warn("Failed to decode UpdateNetworkRules event data", "block", blockNumber, "error", err)
			return
		}
		err = metadataStore.PatchRules(blockNumber, diff)
		if err != nil {
			slog.Warn("Failed to patch rules", "block", blockNumber, "error", err)
		}
	}
}

// decodeDataBytes returns the single ABI-encoded dynamic bytes parameter in the
// data of the given log, as Sonic does in
// https://github.com/0xsoniclabs/sonic/blob/c3816115c9ae51682aa475c715aabbe10e0dcef4/gossip/blockproc/drivermodule/driver_txs.go#L296
func decodeDataBytes(l *core_types.Log) ([]byte, error) {
	if len(l.Data) < 32 {
		return nil, io.ErrUnexpectedEOF
	}
	start := new(big.Int).SetBytes(l.Data[24:32]).Uint64()
	if start+32 > uint64(len(l.Data)) {
		return nil, io.ErrUnexpectedEOF
	}
	size := new(big.Int).SetBytes(l.Data[start+24 : start+32]).Uint64()
	if start+32+size > uint64(len(l.Data)) {
		return nil, io.ErrUnexpectedEOF
	}
	return l.Data[start+32 : start+32+size], nil
}
