#!/bin/bash

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
