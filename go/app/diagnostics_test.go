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

package app

import (
	"fmt"
	"net"
	"testing"
	"testing/synctest"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestStartDiagnostics_LogsServerPortAndUserInfo(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	logger.EXPECT().Info(
		"Starting diagnostics server",
		"address", "http://localhost:12345/debug/pprof",
		"see", "https://pkg.go.dev/net/http/pprof",
	).Times(1)

	server := StartDiagnostics(logger, 12345)
	require.NoError(t, server.Stop())
}

func TestStartDiagnostics_InvalidPort_LogsErrorRunningServer(t *testing.T) {
	ctrl := gomock.NewController(t)
	logger := utils.NewMockLogger(ctrl)

	// Occupy a port to make the diagnostics server fail to start.
	listener, err := net.Listen("tcp", "localhost:0") // occupy a random port
	require.NoError(t, err)
	defer func() {
		require.NoError(t, listener.Close())
	}()

	port := uint16(listener.Addr().(*net.TCPAddr).Port)

	logger.EXPECT().Info(
		"Starting diagnostics server",
		"address", fmt.Sprintf("http://localhost:%d/debug/pprof", port),
		"see", "https://pkg.go.dev/net/http/pprof",
	).Times(1)

	logger.EXPECT().Error(
		"Diagnostics server failed",
		"err", gomock.Any(),
	).Times(1)

	synctest.Test(t, func(t *testing.T) {
		server := StartDiagnostics(logger, port)
		synctest.Wait()
		require.NoError(t, server.Stop())
	})
}
