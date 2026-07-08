#!/bin/bash
# Wrapper that runs the replay tool in chunks, closing and reopening the state
# database between chunks.
#
# The script takes a single dedicated argument, --chunk-size, which controls
# how many blocks are processed per invocation of the replay tool. All other
# arguments are forwarded verbatim to `go-run-with-carmen.sh . replay ...`.
#
# The overall block range is taken from the forwarded -s / --start-block and
# -e / --end-block flags (defaults: 0 and "max"). For each chunk the script
# rewrites the -s and -e values so the replay tool starts up, replays the
# chunk, closes the DB, and exits; the next iteration reopens the DB.
#
# Usage:
#   ./run-replay-chunked.sh --chunk-size 100000 \
#       -g ../data/genesis/sepolia.json \
#       -db /path/to/.blockdb/ \
#       --db-schema 5 --db-variant go-file \
#       --sdb /path/to/statedb \
#       --no-receipts-check --keep-db \
#       -s 0 -e 1000000

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_SCRIPT="${SCRIPT_DIR}/go-run-with-carmen.sh"

CHUNK_SIZE=""
START_BLOCK=0
END_BLOCK="max"
FORWARDED_ARGS=()

# ---- Argument parsing ----
# We consume --chunk-size ourselves, extract -s/--start-block and
# -e/--end-block (still forwarding them, but overridden per chunk),
# and forward the rest untouched.
while [[ $# -gt 0 ]]; do
    case "$1" in
        --chunk-size)
            CHUNK_SIZE="$2"
            shift 2
            ;;
        --chunk-size=*)
            CHUNK_SIZE="${1#*=}"
            shift
            ;;
        -s|--start-block)
            START_BLOCK="$2"
            shift 2
            ;;
        --start-block=*)
            START_BLOCK="${1#*=}"
            shift
            ;;
        -e|--end-block)
            END_BLOCK="$2"
            shift 2
            ;;
        --end-block=*)
            END_BLOCK="${1#*=}"
            shift
            ;;
        -h|--help)
            grep -E '^# ' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            FORWARDED_ARGS+=("$1")
            shift
            ;;
    esac
done

if [[ -z "$CHUNK_SIZE" ]]; then
    echo "Error: --chunk-size is required" >&2
    exit 1
fi
if ! [[ "$CHUNK_SIZE" =~ ^[0-9]+$ ]] || (( CHUNK_SIZE == 0 )); then
    echo "Error: --chunk-size must be a positive integer" >&2
    exit 1
fi
if ! [[ "$START_BLOCK" =~ ^[0-9]+$ ]]; then
    echo "Error: start block must be a non-negative integer (got: $START_BLOCK)" >&2
    exit 1
fi

# Resolve "max" for the end block to an actual upper bound we can iterate on.
# We use uint64 max, matching what the replay tool treats as "max".
UINT64_MAX=18446744073709551615
if [[ "$END_BLOCK" == "max" ]]; then
    END_NUM=$UINT64_MAX
elif [[ "$END_BLOCK" =~ ^[0-9]+$ ]]; then
    END_NUM=$END_BLOCK
else
    echo "Error: end block must be a non-negative integer or 'max' (got: $END_BLOCK)" >&2
    exit 1
fi

if (( END_NUM < START_BLOCK )); then
    echo "Error: end block ($END_NUM) must be >= start block ($START_BLOCK)" >&2
    exit 1
fi

echo "=========================================="
echo "Chunked replay:"
echo "  start block : $START_BLOCK"
echo "  end block   : $END_BLOCK"
echo "  chunk size  : $CHUNK_SIZE"
echo "  forwarded   : ${FORWARDED_ARGS[*]}"
echo "=========================================="

current=$START_BLOCK
chunk_index=0

while (( current <= END_NUM )); do
    # Compute the inclusive end of this chunk, being careful with overflow.
    remaining=$(( END_NUM - current ))
    if (( remaining < CHUNK_SIZE - 1 )); then
        chunk_end=$END_NUM
    else
        chunk_end=$(( current + CHUNK_SIZE - 1 ))
    fi

    chunk_index=$(( chunk_index + 1 ))

    echo ""
    echo "------------------------------------------"
    echo "Chunk #${chunk_index}: blocks ${current} -> ${chunk_end}"
    echo "------------------------------------------"

    "$RUN_SCRIPT" . replay \
        "${FORWARDED_ARGS[@]}" \
        --keep-db \
        -s "$current" \
        -e "$chunk_end"

    # Stop if we just processed the final block (guards against overflow).
    if (( chunk_end == END_NUM )); then
        break
    fi
    current=$(( chunk_end + 1 ))
done

echo ""
echo "=========================================="
echo "All chunks completed successfully."
echo "=========================================="
