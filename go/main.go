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

package main

import (
	"context"
	"errors"
	"log/slog"
	"os"

	"github.com/0xsoniclabs/bertha/app"
)

func main() {
	if err := app.Run(os.Args); err != nil && !errors.Is(err, context.Canceled) {
		slog.Error("Failed to run block-db", "error", err)
		os.Exit(1)
	}
}
