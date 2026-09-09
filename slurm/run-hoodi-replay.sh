#!/bin/bash -l

# Replays the Hoodi block history from the block database built by
# setup-hoodi-blockdb.sh.
#
# Submit with `sbatch run-hoodi-replay.sh <lfvm|sfvm|evmzero|evmrs|evmrs-nightly> <end-block>`.

#SBATCH --job-name=hoodi-replay
# The interpreter configurations share one Tosca build tree, so the jobs are
# serialized: singleton admits one job of this user and job name at a time.
#SBATCH --dependency=singleton
#SBATCH --partition=IFIAMD
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=32
#SBATCH --exclusive
#SBATCH --mem=24G
#SBATCH --time=2:00:00
#SBATCH --output=/home/%u/logs/%x.%N.%j.log

set -eu

BERTHA_DIR=/home/$USER/bertha
TOSCA_DIR=/home/$USER/tosca
BLOCKDB_SRC_DIR=/mnt/ifi-ssd/dps/$USER/blockdbs/hoodi
BLOCKDB_DIR=/mnt/local-disk/$USER/blockdbs/hoodi

CLANG_DIR=/home/$USER/.local/llvm-23.1.1
# Clang selects GCC 14, which ships without libstdc++, so the C++ toolchain is
# pinned to the complete GCC 13 install.
GCC_DIR=/usr/lib/gcc/x86_64-linux-gnu/13
# evmc-sys's bindgen 0.69 generates empty structs against libclang 23, so
# bindgen is pinned to the system libclang. The C++ toolchain above is
# unaffected.
export LIBCLANG_PATH=/usr/lib/llvm-20/lib


if [ $# -ne 2 ]; then
	echo "usage: sbatch run-hoodi-replay.sh <lfvm|sfvm|evmzero|evmrs|evmrs-nightly> <end-block>" >&2
	exit 1
fi

INTERPRETER=$1
INTERPRETER_VARIANT=$1
END_BLOCK=$2

if [ ! -f "$TOSCA_DIR/third_party/evmc/CMakeLists.txt" ]; then
	echo "tosca submodules are missing; run 'git submodule update --init --recursive' in $TOSCA_DIR" >&2
	exit 1
fi

NIGHTLY=
case "$INTERPRETER" in
lfvm | sfvm | evmzero | evmrs) ;;
evmrs-nightly)
	INTERPRETER=evmrs
	NIGHTLY=1
	;;
*)
	echo "usage: sbatch run-hoodi-replay.sh <lfvm|sfvm|evmzero|evmrs|evmrs-nightly> <end-block>" >&2
	exit 1
	;;
esac


echo "### ENVIRONMENT ###"
echo "INTERPRETER=$INTERPRETER_VARIANT"
echo "BLOCKS=$END_BLOCK"
go version
rustc --version
rustc +nightly --version
"$CLANG_DIR/bin/clang" --version 2>/dev/null | head -n1
for REPO in "$BERTHA_DIR" "$TOSCA_DIR" "$TOSCA_DIR/third_party/evmc"; do
	echo "$REPO $(git -C "$REPO" rev-parse HEAD)"
done
cd "$BERTHA_DIR/go"
go list -m github.com/0xsoniclabs/sonic github.com/0xsoniclabs/carmen/go
echo "### ENVIRONMENT END ###"


# Both Tosca shared libraries are built whatever the interpreter, because the
# replay command imports all of the bindings: the Go binary links
# libevmzero.so, and the evmrs binding loads libevmrs.so from its init
# function and panics when it is missing.
cd "$TOSCA_DIR/rust"
if [ -n "$NIGHTLY" ]; then
	cargo +nightly build --lib --release --features performance-nightly
else
	cargo build --lib --release --features performance
fi

# evmzero is built without mimalloc, which would otherwise replace the global
# operator new and delete and capture RocksDB's allocations.
cd "$TOSCA_DIR/cpp"
cmake -Bbuild \
	-DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_C_COMPILER="$CLANG_DIR/bin/clang" \
	-DCMAKE_CXX_COMPILER="$CLANG_DIR/bin/clang++" \
	-DCMAKE_CXX_FLAGS="--gcc-install-dir=$GCC_DIR" \
	-DCMAKE_SHARED_LIBRARY_SUFFIX_CXX=.so \
	-DTOSCA_ASSERT=ON \
	-DEVMZERO_MIMALLOC=OFF
cmake --build build --parallel 4 -t evmzero


# Both databases live on the node-local disk because the shared SSD is
# congested. The block database copy is incremental, so re-running on a node
# that already holds it is cheap.
mkdir -p "$BLOCKDB_DIR"
rsync -a "$BLOCKDB_SRC_DIR/.blockdb" "$BLOCKDB_DIR/"

STATE_DB_DIR=/mnt/local-disk/$USER/statedbs/hoodi-$INTERPRETER_VARIANT-$END_BLOCK-${SLURM_JOB_ID:-$$}
mkdir -p "$STATE_DB_DIR"

cd "$BERTHA_DIR/go"

go run . replay \
	--database-dir "$BLOCKDB_DIR/.blockdb" \
	--json-genesis "$BERTHA_DIR/data/genesis/hoodi.json" \
	--state-db-dir "$STATE_DB_DIR" \
	--interpreter "$INTERPRETER" \
	--end-block "$END_BLOCK" \
	--no-receipts-check
