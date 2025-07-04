package main

import (
	"context"
	"errors"
	"log/slog"
	"os"

	"github.com/0xsoniclabs/blockdb/cmd/block-db/app"
)

func main() {
	if err := app.Run(os.Args); err != nil && !errors.Is(err, context.Canceled) {
		slog.Error("Failed to run block-db", "error", err)
		os.Exit(1)
	}
}
