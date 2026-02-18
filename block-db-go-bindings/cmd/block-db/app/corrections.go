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
	_ "embed"
	"encoding/json"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
)

// GetSonicMainnetCorrections retrieves the corrections for the Sonic Mainnet.
// These are modifications to be added to blocks outside of the effects of
// transactions, such as balance corrections for accounts.
func GetSonicMainnetCorrections() (Corrections, error) {
	res := Corrections{}
	err := json.Unmarshal(sonicMainnetCorrections, &res)
	return res, err
}

// Corrections map block numbers to a map of addresses and their corresponding
// account corrections at the end of the respective blocks.
type Corrections map[uint64]map[common.Address]Correction

// Correction represents a diff that needs to be applied to an account's
// state at the end of a block.
type Correction struct {
	Balance uint256.Int
}

//go:embed corrections.json
var sonicMainnetCorrections []byte
