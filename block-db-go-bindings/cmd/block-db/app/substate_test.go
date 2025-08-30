package app

/*
import (
	"math/big"
	"testing"

	sdb "github.com/0xsoniclabs/substate/db"
	"github.com/0xsoniclabs/substate/substate"
	"github.com/stretchr/testify/require"
)

func TestSubstateDB_Recording(t *testing.T) {
	require := require.New(t)

	//dir := "substateDB"
	dir := t.TempDir()
	db, err := sdb.NewDefaultSubstateDB(dir)
	require.NoError(err)
	require.NoError(db.SetSubstateEncoding(sdb.ProtobufEncodingSchema))

	state := &substate.Substate{
		Block:       12,
		Transaction: 3,
		Env: &substate.Env{
			Number:     3,
			Difficulty: new(big.Int),
		},
		Message: &substate.Message{
			CheckNonce: true,
			GasPrice:   new(big.Int),
			Value:      new(big.Int),
		},
		Result: &substate.Result{},
	}

	require.NoError(db.PutSubstate(state))
	require.NoError(db.Close())

	// Try to re-load the substate.
	db2, err := sdb.NewReadOnlySubstateDB(dir)
	require.NoError(err)
	require.NoError(db2.SetSubstateEncoding(sdb.ProtobufEncodingSchema))
	found, err := db2.HasSubstate(12, 3)
	require.NoError(err)
	require.True(found)

	restored, err := db2.GetSubstate(12, 3)
	require.NoError(err)
	require.Equal(state, restored)

	require.NoError(db2.Close())
}
*/
