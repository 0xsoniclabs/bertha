// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

package app

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
)

// Genesis is a data structure capturing the genesis state information.
type Genesis struct {
	ChainId  uint64
	Accounts []Account
}

// Account represents an Ethereum account with its balance, nonce, code, and storage.
type Account struct {
	Address common.Address
	Balance uint256.Int
	Nonce   uint64
	Code    []byte
	Storage map[common.Hash]common.Hash
}

// ParseGenesis takes a JSON byte slice and unmarshals it into a Genesis struct.
func ParseGenesis(jsonData []byte) (*Genesis, error) {
	var genesis struct {
		Rules struct {
			NetworkID uint64 `json:"NetworkID"`
		}
		Accounts []struct {
			Address common.Address              `json:"address"`
			Nonce   uint64                      `json:"nonce"`
			Balance uint256.Int                 `json:"balance"`
			Code    string                      `json:"code"`
			Storage map[common.Hash]common.Hash `json:"storage"`
		}
	}
	if err := json.Unmarshal(jsonData, &genesis); err != nil {
		return nil, fmt.Errorf("failed to unmarshal genesis data: %w", err)
	}

	res := &Genesis{
		ChainId:  genesis.Rules.NetworkID,
		Accounts: nil,
	}
	for _, account := range genesis.Accounts {
		code, err := hex.DecodeString(strings.TrimPrefix(account.Code, "0x"))
		if err != nil {
			return nil, fmt.Errorf("failed to decode code for account %s: %w", account.Address, err)
		}
		res.Accounts = append(res.Accounts, Account{
			Address: account.Address,
			Balance: account.Balance,
			Nonce:   account.Nonce,
			Code:    code,
			Storage: account.Storage,
		})
	}

	return res, nil
}

// ReadGenesisFromFile reads a JSON genesis file from the specified path and
// returns a Genesis struct.
func ReadGenesisFromFile(filePath string) (*Genesis, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, fmt.Errorf("failed to read genesis file %q: %w", filePath, err)
	}
	return ParseGenesis(data)
}
