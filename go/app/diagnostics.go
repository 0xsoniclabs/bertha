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
	"context"
	"fmt"
	"net"
	"net/http"
	"time"

	_ "net/http/pprof"

	"github.com/0xsoniclabs/bertha/utils"
)

// diagnostic represents a running diagnostics server.
type diagnostic struct {
	server  *http.Server
	address string
	done    <-chan struct{}
}

// StartDiagnostics starts a diagnostics server on the given port. The
// diagnostics server provides pprof endpoints for profiling and debugging.
// Among others, it provides access to CPU, heap, and synchronization profiles.
// Also, trace information can be accessed via a HTTP endpoint. The server
// listens on localhost on the provided port (or a port chosen by the OS if 0 is
// specified). For details see https://pkg.go.dev/net/http/pprof.
func StartDiagnostics(
	logger utils.Logger,
	port uint16,
) (*diagnostic, error) {
	address := fmt.Sprintf("127.0.0.1:%d", port)

	listener, err := net.Listen("tcp", address)
	if err != nil {
		return nil, fmt.Errorf("starting diagnostics server failed: %w", err)
	}

	// Optionally, set the block and mutex profile rates to enable profiling of blocking operations and mutex contention.
	// runtime.SetBlockProfileRate(1)
	// runtime.SetMutexProfileFraction(1)

	address = listener.Addr().String()
	logger.Info("Starting diagnostics server",
		"address", fmt.Sprintf("http://%s/debug/pprof", address),
		"see", "https://pkg.go.dev/net/http/pprof",
	)
	server := &http.Server{}
	done := make(chan struct{})
	go func() {
		defer close(done)
		if err := server.Serve(listener); err != nil {
			if err != http.ErrServerClosed {
				logger.Error("Diagnostics server failed", "err", err)
			}
		}
	}()
	return &diagnostic{
		server:  server,
		address: address,
		done:    done,
	}, nil
}

// Stop stops the diagnostics server.
func (d *diagnostic) Stop() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := d.server.Shutdown(ctx)
	<-d.done
	return err
}
