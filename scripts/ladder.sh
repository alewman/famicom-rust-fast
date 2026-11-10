#!/usr/bin/env bash
# The fork's gate: every change must leave this board identical to
# famicom-rust (whose traces equal famicom-python's at famicom-rust-pin-1).
# No Python: everything the reference ran is cached in external/ (copied
# from famicom-rust). In order:
#   cargo test (mapper_coverage: all 294 mappers build);
#   rung 1, nestest against nestest.log;
#   the lockstep cases against the cached reference traces: rung 2, rung 5
#     (every board class, the FDS, Lagrange Point's VRC7), the machine rows
#     and the peripherals;
#   rung 3, the reference's test ROM tables, outcome for outcome;
#   rung 4, AccuracyCoin: 147 of 147 on the reference's 126,984,584 cycles
#     and 4,264 frames;
#   save states round-tripping on one dump of every board class.
# JOBS (default 4) runs that many boards at a time, under nice.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
EXT=$ROOT/external
JOBS=${JOBS:-4}
export NES_TEST_ROMS=${NES_TEST_ROMS:-/data/emu/famicom-rust/external/famicom-python/tests/test_roms}
export FAMICOM_FDS_BIOS=${FAMICOM_FDS_BIOS:-"/data/emu/roms/fds/[BIOS] Family Computer Disk System (Japan) (En) (Rev 1).zip"}
# nes20db, which identifies the dumps, read at run time (copied into external/
# from the reference's reference/docs; never committed). Without it many
# dumps load as their headers say and the traces differ.
export FAMICOM_NES20DB=${FAMICOM_NES20DB:-$EXT/nes20db-2026-09-26.7z}
[ -s "$FAMICOM_NES20DB" ] || { echo "no nes20db at $FAMICOM_NES20DB"; exit 1; }
# pally's palettes, read at run time from the reference's
# reference/nesdev-wiki/images (copied into external/reference; never
# committed): the Zapper's light thresholds come from them. Without them the
# board falls back to a grey palette and the light-gun traces differ.
export FAMICOM_REFERENCE=${FAMICOM_REFERENCE:-$EXT/reference}
for pal in 2C02G_U_wiki.pal 2C07_wiki.pal; do
    [ -s "$FAMICOM_REFERENCE/reference/nesdev-wiki/images/$pal" ] || {
        echo "no $pal under $FAMICOM_REFERENCE/reference/nesdev-wiki/images"; exit 1; }
done
BIN=$ROOT/target/release
began=$SECONDS
failed=0
step() { echo "== $1 ($((SECONDS - began)) s)"; }

step "cargo test"
nice cargo build -j 4 --release -q --manifest-path "$ROOT/Cargo.toml"
if ! nice cargo test -j 4 --release -q --manifest-path "$ROOT/Cargo.toml" >"$EXT/ladder-test.log" 2>&1; then
    grep -E "test result|FAILED|panicked" "$EXT/ladder-test.log" || tail -20 "$EXT/ladder-test.log"
    failed=1
fi
grep -E "^test result" "$EXT/ladder-test.log" | grep -v " 0 passed" || true

step "rung 1: nestest"
nice "$BIN/nestest" "$NES_TEST_ROMS/nes-test-roms" | tail -1 || failed=1

lockstep() {
    nice "$BIN/famicom-lockstep" --jobs "$JOBS" --cases "$1" --cache "$2" >"$EXT/ladder-lockstep.log" || {
        grep -v ": identical" "$EXT/ladder-lockstep.log"
        failed=1
        return
    }
    tail -1 "$EXT/ladder-lockstep.log"
}
step "lockstep: rung 2"
lockstep "$ROOT/scripts/cases.tsv" "$EXT/refcache"
step "lockstep: rung 2, the probes (probes/*.s, assembled with cc65)"
PROBES="$EXT/probes" "$ROOT/probes/build.sh"
lockstep "$ROOT/scripts/cases-probes.tsv" "$EXT/refcache-probes"
step "lockstep: rung 5, every board class"
lockstep "$EXT/cases-rung5.tsv" "$EXT/refcache-mappers"
step "lockstep: rung 5, the FDS"
lockstep "$EXT/cases-fds.tsv" "$EXT/refcache-mappers"
step "lockstep: rung 5, Lagrange Point (VRC7)"
lockstep "$EXT/cases-lagrange.tsv" "$EXT/refcache-mappers"
step "lockstep: rung 5, AutoDisk (FDS sides changed as the program asks)"
lockstep "$ROOT/scripts/cases-fds-auto.tsv" "$EXT/refcache-mappers"
step "lockstep: rung 6, machine rows"
lockstep "$ROOT/scripts/cases-machines.tsv" "$EXT/refcache"
step "lockstep: rung 6, peripherals"
lockstep "$ROOT/scripts/cases-peripherals.tsv" "$EXT/refcache-devices"
step "lockstep: rung 6, host input (frontend.py's input half)"
lockstep "$ROOT/scripts/cases-host.tsv" "$EXT/refcache-host"

step "rung 3: test ROM tables"
nice "$BIN/famicom-testrom" "$EXT/testrom-reference.jsonl" --roms "$NES_TEST_ROMS/nes-test-roms" \
    --out "$EXT/testrom-rust.jsonl" --jobs "$JOBS" >/dev/null
"$BIN/famicom-testrom-compare" "$EXT/testrom-reference.jsonl" "$EXT/testrom-rust.jsonl" | tail -1 || failed=1

step "rung 4: AccuracyCoin"
coin=$(nice "$BIN/famicom-accuracycoin" "$NES_TEST_ROMS/AccuracyCoin/AccuracyCoin.nes" \
    "$EXT/accuracycoin-tests.json" --failures | tail -1) || failed=1
echo "$coin"
[ "$coin" = "147 of 147 pass; 126984584 CPU cycles, 4264 frames" ] || { echo "rung 4: not the reference's counts"; failed=1; }

step "save states"
# The dump list in JOBS shares, one famicom-state-roundtrip each.
split_dir=$EXT/ladder-states
mkdir -p "$split_dir"
for i in $(seq 0 $((JOBS - 1))); do
    awk -v n="$JOBS" -v i="$i" 'NR % n == i' "$EXT/state-dumps.txt" >"$split_dir/part$i.txt"
done
pids=()
for i in $(seq 0 $((JOBS - 1))); do
    mapfile -t roms <"$split_dir/part$i.txt"
    nice "$BIN/famicom-state-roundtrip" "${roms[@]}" >"$split_dir/part$i.out" 2>&1 &
    pids+=($!)
done
for pid in "${pids[@]}"; do wait "$pid" || true; done
total=$(cat "$EXT/state-dumps.txt" | wc -l)
same=$(cat "$split_dir"/part*.out | grep -c ": identical" || true)
cat "$split_dir"/part*.out | grep -v ": identical" || true
echo "$same of $total dumps round-trip"
[ "$same" = "$total" ] || failed=1

step "done"
if [ "$failed" = 0 ]; then echo "LADDER GREEN"; else echo "LADDER RED"; fi
exit "$failed"
