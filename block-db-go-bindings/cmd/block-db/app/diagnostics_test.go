package app

import (
	"fmt"
	"net"
	"testing"
	"testing/synctest"

	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestStartDiagnostics_LogsServerPortAndUserInfo(t *testing.T) {
	ctrl := gomock.NewController(t)
	log := NewMock_infoLogger(ctrl)

	log.EXPECT().Info(
		"Starting diagnostics server",
		"address", "http://localhost:12345/debug/pprof",
		"see", "https://pkg.go.dev/net/http/pprof",
	).Times(1)

	server := StartDiagnostics(log, 12345)
	require.NoError(t, server.Stop())
}

func TestStartDiagnostics_InvalidPort_LogsErrorRunningServer(t *testing.T) {
	ctrl := gomock.NewController(t)
	log := NewMock_infoLogger(ctrl)

	// Occupy a port to make the diagnostics server fail to start.
	listener, err := net.Listen("tcp", "localhost:0") // occupy a random port
	require.NoError(t, err)
	defer func() {
		require.NoError(t, listener.Close())
	}()

	port := uint16(listener.Addr().(*net.TCPAddr).Port)

	log.EXPECT().Info(
		"Starting diagnostics server",
		"address", fmt.Sprintf("http://localhost:%d/debug/pprof", port),
		"see", "https://pkg.go.dev/net/http/pprof",
	).Times(1)

	log.EXPECT().Error(
		"Diagnostics server failed",
		"err", gomock.Any(),
	).Times(1)

	synctest.Test(t, func(t *testing.T) {
		server := StartDiagnostics(log, port)
		synctest.Wait()
		require.NoError(t, server.Stop())
	})
}
