#!/bin/bash
# Copyright 2026 Sonic Operations Ltd
# This file is part of the Sonic Client
#
# Sonic is free software: you can redistribute it and/or modify
# it under the terms of the GNU Lesser General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# Sonic is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU Lesser General Public License for more details.
#
# You should have received a copy of the GNU Lesser General Public License
# along with Sonic. If not, see <http://www.gnu.org/licenses/>.


# Script to run go with local carmen modifications enabled.
# Usage: ./go-run-with-carmen.sh <go-run-args>
# e.g. ./go-run-with-carmen.sh ./cmd/block-db replay -g sonic.json -db .blockdb --db-schema 6 --db-variant rust-memory
# It is assumed that the carmen repository is located next to bertha.

# Exit on error.
set -e
# Print all commands before executing.
set -x

# Revert the patch on exit.
trap 'git apply --reverse enable-carmen.patch &> /dev/null' EXIT

CARMEN_RUST_DIR=$(pwd)/../../carmen/rust
BLOCK_DB_GO_BINDINGS_DIR=$(pwd)

# Build carmen.
cd $CARMEN_RUST_DIR
cargo build --release $CARMEN_RUST_BUILD_FLAGS

# Override the go carmen dependencies to use local modified versions.
# Check if the diff can be applied cleanly in reverse. In this case it is already applied.
# Otherwise apply the diff
cd $BLOCK_DB_GO_BINDINGS_DIR
git apply --reverse --check enable-carmen.patch 2> /dev/null || git apply enable-carmen.patch

# Run go
go run "$@"
