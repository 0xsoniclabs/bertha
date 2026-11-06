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
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/params"
	"github.com/stretchr/testify/require"
)

func TestUnsupportedFork_NamesTheLatestUnsupportedFork(t *testing.T) {
	const ubtTime = uint64(2_000_000_000)
	chainConfig := *params.MainnetChainConfig
	chainConfig.UBTTime = new(ubtTime)
	chainConfig.AmsterdamTime = new(ubtTime + 1)
	chainConfig.BogotaTime = new(ubtTime + 2)

	tests := map[string]struct {
		time uint64
		want string
	}{
		"before UBT": {time: ubtTime - 1, want: ""},
		"UBT":        {time: ubtTime, want: "UBT"},
		"Amsterdam":  {time: ubtTime + 1, want: "Amsterdam"},
		"Bogota":     {time: ubtTime + 2, want: "Bogota"},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			require.Equal(t, tt.want, unsupportedFork(&chainConfig, big.NewInt(20_000_000), tt.time))
		})
	}
}
