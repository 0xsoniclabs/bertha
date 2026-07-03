#!/bin/bash
# Copyright 2026 Sonic Operations Ltd
# This file is part of the Bertha testing infrastructure for Sonic.
#
# Bertha is free software: you can redistribute it and/or modify
# it under the terms of the GNU Lesser General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# Bertha is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU Lesser General Public License for more details.
#
# You should have received a copy of the GNU Lesser General Public License
# along with Bertha. If not, see <http://www.gnu.org/licenses/>.

# Script to run go with tracy profiling enabled.
# Usage: ./go-run-with-tracy.sh <go-run-args>
# e.g. ./go-run-with-tracy.sh . replay -g sonic.json -db .blockdb
# It is assumed that the tracy repository is located next to bertha.

# Exit on error.
set -e
# Print all commands before executing.
set -x

# Revert the patches on exit.
trap 'git apply --reverse enable-tracy.patch &> /dev/null' EXIT

TRACY_DIR=$(pwd)/../../tracy
BERTHA_GO_DIR=$(pwd)

# Build tracy shared library.
cd $TRACY_DIR
git submodule update --recursive --init
make

# Override the go tracy dependencies to use local modified versions.
# Check if the diff can be applied cleanly in reverse. In this case it is already applied.
# Otherwise apply the diff
cd $BERTHA_GO_DIR
git apply --reverse --check enable-tracy.patch 2> /dev/null || git apply enable-tracy.patch

# Run go with tracy tag.
go run --tags "enable_tracy" "$@"
