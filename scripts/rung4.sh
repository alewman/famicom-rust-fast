#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 4: AccuracyCoin, all 147 tests, as the reference's scripts/accuracycoin.py
# runs them; must pass all and end on the reference's cycle and frame counts.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REF=${FAMICOM_REFERENCE:-$ROOT/external/famicom-python}
HOME_DIR=$REF/tests/test_roms/AccuracyCoin
"$REF/.venv-pypy/bin/python" "$ROOT/scripts/accuracycoin_tests.py" "$ROOT/external/accuracycoin-tests.json"
cargo build --release -q --manifest-path "$ROOT/Cargo.toml"
exec "$ROOT/target/release/famicom-accuracycoin" "$HOME_DIR/AccuracyCoin.nes" \
    "$ROOT/external/accuracycoin-tests.json" --failures
