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

package app

import (
	"net"
	"strings"
	"testing"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestStartDiagnostics_PortZero_PicksFreePortAndLogsAddressAndUserInfo(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	var address string
	logger.EXPECT().Info(
		"Starting diagnostics server",
		"address", gomock.Any(),
		"see", "https://pkg.go.dev/net/http/pprof",
	).Do(func(msg string, keysAndValues ...interface{}) {
		address = keysAndValues[1].(string)
		require.True(t, strings.HasPrefix(address, "http://"))
		require.True(t, strings.HasSuffix(address, "/debug/pprof"))
		address = strings.TrimPrefix(address, "http://")
		address = strings.TrimSuffix(address, "/debug/pprof")
	}).Times(1)

	server, err := StartDiagnostics(logger, 0)
	require.NoError(t, err)
	require.Equal(t, address, server.address)
	require.NoError(t, server.Stop())
}

func TestStartDiagnostics_InvalidPort_ReturnsError(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	// Occupy a port to make the diagnostics server fail to start.
	listener, err := net.Listen("tcp", "127.0.0.1:0") // occupy a random port
	require.NoError(t, err)
	defer func() {
		require.NoError(t, listener.Close())
	}()

	port := uint16(listener.Addr().(*net.TCPAddr).Port)

	_, err = StartDiagnostics(logger, port)
	require.ErrorContains(t, err, "starting diagnostics server failed")
}
