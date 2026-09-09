#!/bin/bash -l

# Downloads the Hoodi beacon chain `.era` files (~114 GiB) and imports them into
# a block database. Both steps resume, so the job can be resubmitted; an `.era`
# file left half-downloaded by a killed job has to be deleted by hand.
#
# Submit with `sbatch setup-hoodi-blockdb.sh`.

#SBATCH --job-name=setup-hoodi-blockdb
#SBATCH --partition=IFIAMD
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=8
#SBATCH --mem=16G
#SBATCH --time=2:00:00
#SBATCH --output=/home/%u/logs/%x.%N.%j.log

set -eu

BERTHA_DIR=/home/$USER/bertha
ERA_DIR=/scratch/$USER/source/hoodi
BLOCKDB_DIR=/mnt/ifi-ssd/dps/$USER/blockdbs/hoodi

mkdir -p "$ERA_DIR" "$BLOCKDB_DIR"

# Existing files are skipped with --no-clobber instead of being resumed with
# --continue. The resume request for a complete file is answered with a
# text/html 416, which makes wget parse the .era file as HTML and then fail on
# the phantom links it finds in the block data.
wget --recursive --no-parent --no-host-directories \
	--accept '*.era' --no-clobber --no-verbose --directory-prefix="$ERA_DIR" \
	https://hoodi.era.nimbus.team

cd "$BERTHA_DIR"
cargo run --release -- --dir "$BLOCKDB_DIR" init
cargo run --release -- --dir "$BLOCKDB_DIR" import-era "$ERA_DIR" hoodi
