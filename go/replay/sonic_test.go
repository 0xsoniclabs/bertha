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
	"math/big"
	"testing"

	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestOnNewLog_PatchesRulesOnUpdateNetworkRulesEvent(t *testing.T) {
	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	tests := map[string]struct {
		modify    func(*core_types.Log)
		wantPatch bool
	}{
		"update network rules event": {modify: func(*core_types.Log) {}, wantPatch: true},
		"wrong address":              {modify: func(l *core_types.Log) { l.Address = common.Address{0x42} }},
		"wrong topic":                {modify: func(l *core_types.Log) { l.Topics = []common.Hash{{0xFF}} }},
		"no topic":                   {modify: func(l *core_types.Log) { l.Topics = nil }},
		"too short data":             {modify: func(l *core_types.Log) { l.Data = make([]byte, 32) }},
		"undecodable data":           {modify: func(l *core_types.Log) { l.Data[63]++ }},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			metadataStore := NewMockMetadataStore(gomock.NewController(t))
			if test.wantPatch {
				metadataStore.EXPECT().PatchRules(uint64(42), diff).Return(nil)
			}
			log := makeUpdateNetworkRulesLog(diff)
			test.modify(log)

			onNewLog(metadataStore, 42, log)
		})
	}
}

func TestDecodeDataBytes(t *testing.T) {
	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	tests := map[string]struct {
		modify  func([]byte) []byte
		want    []byte
		wantErr error
	}{
		"bytes parameter":    {modify: func(d []byte) []byte { return d }, want: diff},
		"no offset":          {modify: func(d []byte) []byte { return d[:31:31] }, wantErr: io.ErrUnexpectedEOF},
		"offset beyond data": {modify: func(d []byte) []byte { d[31] = byte(len(d) - 31); return d }, wantErr: io.ErrUnexpectedEOF},
		"length beyond data": {modify: func(d []byte) []byte { d[63]++; return d }, wantErr: io.ErrUnexpectedEOF},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			log := makeUpdateNetworkRulesLog(diff)
			log.Data = test.modify(log.Data)

			got, err := decodeDataBytes(log)
			require.Equal(t, test.wantErr, err)
			require.Equal(t, test.want, got)
		})
	}
}

// makeUpdateNetworkRulesLog returns a *core_types.Log that looks like an
// UpdateNetworkRules event from the driver contract. The diff is ABI-encoded
// as a single dynamic bytes parameter:
//
//	word 0 (bytes  0–31): offset = 32
//	word 1 (bytes 32–63): length of diff
//	bytes 64+:            diff
func makeUpdateNetworkRulesLog(diff []byte) *core_types.Log {
	data := make([]byte, 64+len(diff))
	new(big.Int).SetInt64(32).FillBytes(data[0:32])
	new(big.Int).SetInt64(int64(len(diff))).FillBytes(data[32:64])
	copy(data[64:], diff)
	return &core_types.Log{
		Address: driver.ContractAddress,
		Topics:  []common.Hash{driverpos.Topics.UpdateNetworkRules},
		Data:    data,
	}
}
