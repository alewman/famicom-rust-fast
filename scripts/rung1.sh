#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 1: nestest in automation mode against nestest.log (all 8,991 lines).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ROMS=${NES_TEST_ROMS:-$ROOT/external/famicom-python/tests/test_roms}/nes-test-roms
cargo build --release -q --manifest-path "$ROOT/Cargo.toml"
exec "$ROOT/target/release/nestest" "$ROMS"
