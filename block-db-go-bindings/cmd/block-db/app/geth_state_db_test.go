package app

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestGethStateDb_OpenClose(t *testing.T) {
	require := require.New(t)

	stateDB, err := makeGethStateDB(t.TempDir())
	require.NoError(err)

	hash, err := stateDB.GetHash()
	require.NoError(err)
	require.Equal(common.Hash{}, hash)

	require.NoError(stateDB.Close())
}
