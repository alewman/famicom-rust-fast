#!/usr/bin/env bash
# One experiment: build, keep its bench as target/ab/NAME, run the quick
# lockstep check (rung 2's 200 cases), then A/B it against target/ab/BASE.
#     scripts/try.sh NAME [BASE] [ROUNDS]
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NAME=$1
BASE=${2:-base}
export NES_TEST_ROMS=${NES_TEST_ROMS:-/data/emu/famicom-rust/external/famicom-python/tests/test_roms}
export FAMICOM_NES20DB=${FAMICOM_NES20DB:-$ROOT/external/nes20db-2026-09-26.7z}
# Its own target directory, so a ladder running meanwhile keeps its binaries.
export CARGO_TARGET_DIR=$ROOT/target/exp
nice cargo build -j 4 --release -q --manifest-path "$ROOT/Cargo.toml" --bin famicom-bench --bin famicom-trace --bin famicom-lockstep
mkdir -p "$ROOT/target/ab"
cp "$CARGO_TARGET_DIR/release/famicom-bench" "$ROOT/target/ab/$NAME"
nice "$CARGO_TARGET_DIR/release/famicom-lockstep" --trace "$CARGO_TARGET_DIR/release/famicom-trace" --jobs "${JOBS:-3}" --cases "$ROOT/scripts/cases.tsv" --cache "$ROOT/external/refcache" | tail -1
python3 "$ROOT/scripts/ab.py" "$ROOT/target/ab/$BASE" "$ROOT/target/ab/$NAME" --rounds "${3:-8}"
