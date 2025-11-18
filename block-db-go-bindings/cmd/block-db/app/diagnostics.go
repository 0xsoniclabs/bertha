package app

import (
	"context"
	"fmt"
	"net/http"
	"runtime"
	"time"

	"github.com/urfave/cli/v3"

	_ "net/http/pprof"
)

//go:generate mockgen -source=diagnostics.go -destination=diagnostics_mock.go -package=app

var (
	diagnosticsFlag = &cli.BoolFlag{
		Name:  "diagnostics",
		Usage: "enable diagnostics server (pprof)",
		Value: false,
	}
	diagnosticsPortFlag = &cli.Uint16Flag{
		Name:  "diagnostics-port",
		Usage: "port for diagnostics server (pprof)",
		Value: 6060,
	}
)

// diagnostic represents a running diagnostics server.
type diagnostic struct {
	server *http.Server
	done   <-chan struct{}
}

// StartDiagnostics starts a diagnostics server on the given port. The
// diagnostics server provides pprof endpoints for profiling and debugging.
// Among others, it provides access to CPU, heap, and synchronization profiles.
// Also, trace information can be accessed via a HTTP endpoint. For details
// see https://pkg.go.dev/net/http/pprof.
func StartDiagnostics(
	logger _infoLogger,
	port uint16,
) *diagnostic {
	address := fmt.Sprintf("localhost:%d", port)
	runtime.SetBlockProfileRate(1)
	runtime.SetMutexProfileFraction(1)

	logger.Info("Starting diagnostics server",
		"address", fmt.Sprintf("http://%s/debug/pprof", address),
		"see", "https://pkg.go.dev/net/http/pprof",
	)
	server := &http.Server{Addr: address}
	done := make(chan struct{})
	go func() {
		defer close(done)
		if err := server.ListenAndServe(); err != nil {
			if err != http.ErrServerClosed {
				logger.Error("Diagnostics server failed", "err", err)
			}
		}
	}()
	return &diagnostic{
		server: server,
		done:   done,
	}
}

// Stop stops the diagnostics server.
func (d *diagnostic) Stop() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := d.server.Shutdown(ctx)
	<-d.done
	return err
}

// _infoLogger is an interface for mocking slog.Logger interactions in unit tests.
type _infoLogger interface {
	Info(msg string, keysAndValues ...any)
	Error(msg string, keysAndValues ...any)
}
