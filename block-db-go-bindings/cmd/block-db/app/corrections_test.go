package app

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestGetSonicMainnetCorrections_CanBeLoaded(t *testing.T) {
	corrections, err := GetSonicMainnetCorrections()
	require.NoError(t, err)
	require.NotEmpty(t, corrections)
}
