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

// Correction represents a diff that needs to be applied to an account's
// state at the end of a block.
type Correction struct {
	Balance uint256.Int
}

// Corrections map block numbers to a map of addresses and their corresponding
// account states at the end of the respective blocks.
type Corrections map[uint64]map[common.Address]Correction

//go:embed corrections.json
var sonicMainnetCorrections []byte
