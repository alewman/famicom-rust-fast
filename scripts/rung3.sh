#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 3: the reference's test ROM tables (pytest -m slow's cases) on both
# boards: verdict, CPU cycles, instructions and detail, case for case.
# The reference's outcomes are slow to make (hours on PyPy): kept in
# external/testrom-reference.jsonl; REFRESH=1 makes them again.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REF=${FAMICOM_REFERENCE:-$ROOT/external/famicom-python}
OUT=$ROOT/external
if [ ! -s "$OUT/testrom-reference.jsonl" ] || [ -n "${REFRESH:-}" ]; then
    nice "$REF/.venv-pypy/bin/python" "$ROOT/scripts/testrom_outcomes.py" --jobs "${JOBS:-8}" \
        --out "$OUT/testrom-reference.jsonl"
fi
cargo build --release -q --manifest-path "$ROOT/Cargo.toml"
"$ROOT/target/release/famicom-testrom" "$OUT/testrom-reference.jsonl" \
    --roms "$REF/tests/test_roms/nes-test-roms" --out "$OUT/testrom-rust.jsonl" --jobs "${JOBS:-8}"
exec python3 "$ROOT/scripts/testrom_compare.py" "$OUT/testrom-reference.jsonl" "$OUT/testrom-rust.jsonl"
