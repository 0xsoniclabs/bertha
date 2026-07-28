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
	if tmp.Balance != nil {
		c.Balance = tmp.Balance
	} else {
		c.Balance = nil
	}
	if len(tmp.Storage) > 0 {
		c.Storage = make(map[cc.Key]cc.Value, len(tmp.Storage))
		for k, v := range tmp.Storage {
			key, err := decodeFixed(k, 32)
			if err != nil {
				return fmt.Errorf("invalid storage key %q: %w", k, err)
			}
			value, err := decodeFixed(v, 32)
			if err != nil {
				return fmt.Errorf("invalid storage value %q: %w", v, err)
			}
			c.Storage[cc.Key(key)] = cc.Value(value)
		}
	} else {
		c.Storage = nil
	}

	return nil
}

// decodeFixed parses a hex string (with optional 0x prefix) into a fixed-size
// byte array. Shorter inputs are left-padded with zeros.
func decodeFixed(s string, numBytes int) ([]byte, error) {
	if numBytes < 0 {
		return []byte{}, fmt.Errorf("invalid size %d", numBytes)
	}
	s = strings.TrimPrefix(s, "0x")
	if len(s)%2 == 1 {
		return []byte{}, fmt.Errorf("odd-length hex string")
	}
	b, err := hex.DecodeString(s)
	if err != nil {
		return []byte{}, err
	}
	if len(b) > numBytes {
		return []byte{}, fmt.Errorf("value exceeds %d bytes (%d)", numBytes, len(b))
	}
	out := make([]byte, numBytes)
	copy(out[numBytes-len(b):], b)
	return out, nil
}
