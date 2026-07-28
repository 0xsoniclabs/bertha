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
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
)

// Corrections map block numbers to a map of addresses and their corresponding
// account corrections at the end of the respective blocks.
type Corrections map[uint64]map[common.Address]Correction

// Correction represents a diff that needs to be applied to an account's
// state at the end of a block.
type Correction struct {
	Balance *uint256.Int
	Storage map[cc.Key]cc.Value
}

func (c Correction) MarshalJSON() ([]byte, error) {
	// encoding/json does not support [32]byte-based types as map keys, so
	// convert the storage map to use hex-encoded string keys on the wire.
	var storage map[string]string
	if len(c.Storage) > 0 {
		storage = make(map[string]string, len(c.Storage))
		for k, v := range c.Storage {
			storage["0x"+hex.EncodeToString(k[:])] = "0x" + hex.EncodeToString(v[:])
		}
	}
	return json.Marshal(&struct {
		Balance *uint256.Int      `json:",omitempty"`
		Storage map[string]string `json:",omitempty"`
	}{Balance: c.Balance, Storage: storage})
}

func (c *Correction) UnmarshalJSON(data []byte) error {
	var tmp struct {
		Balance *uint256.Int
		Storage map[string]string
	}
	if err := json.Unmarshal(data, &tmp); err != nil {
		return err
	}
	c.Balance = tmp.Balance
	if len(tmp.Storage) > 0 {
		c.Storage = make(map[cc.Key]cc.Value, len(tmp.Storage))
		for k, v := range tmp.Storage {
			key, err := decodeHexTo32Bytes(k)
			if err != nil {
				return fmt.Errorf("invalid storage key %q: %w", k, err)
			}
			value, err := decodeHexTo32Bytes(v)
			if err != nil {
				return fmt.Errorf("invalid storage value %q: %w", v, err)
			}
			c.Storage[cc.Key(key)] = cc.Value(value)
		}
	}

	return nil
}

// decodeHexTo32Bytes parses a hex string (with optional 0x prefix) into a fixed-size
// 32-byte byte array. Shorter inputs are left-padded with zeros.
func decodeHexTo32Bytes(s string) ([32]byte, error) {
	s = strings.TrimPrefix(strings.TrimPrefix(s, "0x"), "0X")
	if len(s)%2 == 1 {
		return [32]byte{}, fmt.Errorf("odd-length hex string")
	}
	b, err := hex.DecodeString(s)
	if err != nil {
		return [32]byte{}, err
	}
	if len(b) > 32 {
		return [32]byte{}, fmt.Errorf("value exceeds 32 bytes (%d)", len(b))
	}
	var out [32]byte
	copy(out[32-len(b):], b)
	return out, nil
}
