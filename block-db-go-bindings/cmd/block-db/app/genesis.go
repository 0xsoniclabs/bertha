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
	Accounts map[common.Address]Account
}

// Account represents an Ethereum account with its balance, nonce, code, and storage.
type Account struct {
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
		Accounts: make(map[common.Address]Account),
	}
	for _, account := range genesis.Accounts {
		code, err := hex.DecodeString(strings.TrimPrefix(account.Code, "0x"))
		if err != nil {
			return nil, fmt.Errorf("failed to decode code for account %s: %w", account.Address, err)
		}
		res.Accounts[account.Address] = Account{
			Balance: account.Balance,
			Nonce:   account.Nonce,
			Code:    code,
			Storage: account.Storage,
		}
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
