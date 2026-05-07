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
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestParseGenesis_CanParseValidGenesisData(t *testing.T) {
	genesisData := []byte(`{
		"Rules": {
			"NetworkID": 12
		},
		"Accounts": [
			{
				"address": "0x01234567890abcdef1234567890abcdef1234567",
				"nonce": 7,
				"balance": 1234
			},
			{
				"address": "0x1234567890abcdef1234567890abcdef12345678",
				"nonce": 9,
				"balance": 5678,
				"code": "0xABCDEF",
				"storage": {
				    "0x0000000000000000000000000000000000000000000000000000000000000001": "0x0000000000000000000000000000000000000000000000000000000000000001",
					"0x0000000000000000000000000000000000000000000000000000000000000002": "0x0000000000000000000000000000000000000000000000000000000000000002"
				}
			}
		]
	}`)

	genesis, err := ParseGenesis(genesisData)
	require.NoError(t, err)
	require.NotNil(t, genesis)
	require.Equal(t, uint64(12), genesis.ChainID)

	require.Len(t, genesis.Accounts, 2)

	account := genesis.Accounts[0]
	require.Equal(t, common.HexToAddress("0x01234567890abcdef1234567890abcdef1234567"), account.Address)
	require.Equal(t, uint64(7), account.Nonce)
	require.Equal(t, uint64(1234), account.Balance.Uint64())
	require.Empty(t, account.Code)
	require.Empty(t, account.Storage)

	account = genesis.Accounts[1]
	require.Equal(t, common.HexToAddress("0x1234567890abcdef1234567890abcdef12345678"), account.Address)
	require.Equal(t, uint64(9), account.Nonce)
	require.Equal(t, uint64(5678), account.Balance.Uint64())
	require.Equal(t, "ABCDEF", fmt.Sprintf("%X", account.Code))
	require.Len(t, account.Storage, 2)
	require.Equal(t,
		account.Storage[common.HexToHash("0x0000000000000000000000000000000000000000000000000000000000000001")],
		common.HexToHash("0x0000000000000000000000000000000000000000000000000000000000000001"),
	)
	require.Equal(t,
		account.Storage[common.HexToHash("0x0000000000000000000000000000000000000000000000000000000000000002")],
		common.HexToHash("0x0000000000000000000000000000000000000000000000000000000000000002"),
	)
}

func TestParseGenesis_FailsOnInvalidJSON(t *testing.T) {
	_, err := ParseGenesis([]byte(`not a valid json`))
	require.ErrorContains(t, err, "failed to unmarshal genesis data")
}

func TestParseGenesis_DetectsInvalidCode(t *testing.T) {
	_, err := ParseGenesis([]byte(`{
		"Accounts": [
			{
				"Code": "not a hex string"
			}
		]
	}`))
	require.ErrorContains(t, err, "failed to decode code")
}

func TestReadGenesisFromFile_CanHandleValidJson(t *testing.T) {
	path := filepath.Join(t.TempDir(), "genesis.json")
	require.NoError(t, os.WriteFile(path, []byte(`{
		"Rules": {
			"NetworkID": 12
		}
	}`), 0644))
	genesis, err := ReadGenesisFromFile(path)
	require.NoError(t, err)
	require.NotNil(t, genesis)
	require.Equal(t, uint64(12), genesis.ChainID)
}

func TestReadGenesisFromFile_ReportsIoIssues(t *testing.T) {
	path := filepath.Join(t.TempDir(), "non_existent_file.json")
	_, err := ReadGenesisFromFile(path)
	require.ErrorContains(t, err, "failed to read genesis file")
}
