#!/bin/bash

# Script to run go with local carmen modifications and tracy profiling enabled.
# Usage: ./go-run-with-carmen-and-tracy.sh <go-run-args>
# e.g. ./go-run-with-carmen-and-tracy.sh ./cmd/block-db replay -g sonic.json -db .blockdb --db-schema 6 --db-variant rust-memory
# It is assumed that the tracy and carmen repositories are located next to bertha.

# Exit on error.
set -e
# Print all commands before executing.
set -x

# Revert the patches on exit.
trap 'git apply --reverse enable-carmen.patch &> /dev/null && git apply --reverse enable-tracy.patch &> /dev/null' EXIT

TRACY_DIR=$(pwd)/../../tracy
CARMEN_RUST_DIR=$(pwd)/../../carmen/rust
BLOCK_DB_GO_BINDINGS_DIR=$(pwd)

# Build tracy shared library.
cd $TRACY_DIR
git submodule update --recursive --init
make

# Build carmen with tracy support.
cd $CARMEN_RUST_DIR
TRACY_CLIENT_LIB=TracyClient TRACY_CLIENT_LIB_PATH=$TRACY_DIR/tracy/build cargo build --release --features tracy

# Override the go tracy and carmen dependencies to use local modified versions.
# Check if the diff can be applied cleanly in reverse. In this case it is already applied.
# Otherwise apply the diff
cd $BLOCK_DB_GO_BINDINGS_DIR
git apply --reverse --check enable-carmen.patch 2> /dev/null || git apply enable-carmen.patch
git apply --reverse --check enable-tracy.patch 2> /dev/null || git apply enable-tracy.patch

# Run go with tracy tag.
go run --tags enable_tracy "$@"
