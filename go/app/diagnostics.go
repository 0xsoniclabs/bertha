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
	"context"
	"fmt"
	"net/http"
	"runtime"
	"time"

	_ "net/http/pprof"
)

//go:generate mockgen -source=diagnostics.go -destination=diagnostics_mock.go -package=app

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
