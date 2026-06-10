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

package replay

import (
	"encoding/json"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
)

// Corrections map block numbers to a map of addresses and their corresponding
// account corrections at the end of the respective blocks.
type Corrections map[uint64]map[common.Address]Correction

// Correction represents a diff that needs to be applied to an account's
// state at the end of a block.
type Correction struct {
	Balance uint256.Int
}

func (c Correction) MarshalJSON() ([]byte, error) {
	return json.Marshal(&struct {
		Balance *uint256.Int
	}{Balance: &c.Balance})
}

func (c *Correction) UnmarshalJSON(data []byte) error {
	var tmp struct {
		Balance *uint256.Int
	}
	if err := json.Unmarshal(data, &tmp); err != nil {
		return err
	}
	if tmp.Balance != nil {
		c.Balance = *tmp.Balance
	} else {
		c.Balance = *uint256.NewInt(0)
	}
	return nil
}
