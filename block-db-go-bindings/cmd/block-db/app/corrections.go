package app

import (
	_ "embed"
	"encoding/json"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
)

//go:embed corrections.json
var rawData []byte

type Account struct {
	Balance uint256.Int
}

type Corrections map[uint64]map[common.Address]Account

func getCorrections() Corrections {
	res := Corrections{}
	if err := json.Unmarshal(rawData, &res); err != nil {
		panic(err)
	}
	return res
}
