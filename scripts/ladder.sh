#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The fork's gate: every change must leave this board identical to
# famicom-rust (whose traces equal famicom-python's at b3dbf1b, the reference
# since 2026-11-23; famicom-rust's scripts/fetch_famicompython.sh).
# No Python: everything the reference ran is cached in external/ (copied
# from famicom-rust). In order:
#   cargo test (mapper_coverage: the reference's 296 mappers build; tests/vs_system.rs);
#   rung 1, nestest against nestest.log;
#   the lockstep cases against the cached reference traces: rung 2, rung 5
#     (every board class, the FDS, Lagrange Point's VRC7, the Sunsoft 5B's
#     sound, mapper 559), the machine rows (the Vs. rows' RGB PPUs among them),
#     the peripherals, the host input, and the Vs. System's images (VS_IMAGES_DIR);
#   rung 3, the reference's test ROM tables, outcome for outcome;
#   rung 4, AccuracyCoin: 147 of 147 on the reference's 126,984,584 cycles
#     and 4,264 frames;
#   save states round-tripping on one dump of every board class, and on every
#   Vs. UniSystem image.
# JOBS (default 4) runs that many boards at a time, under nice.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
EXT=$ROOT/external
JOBS=${JOBS:-4}
# The test ROMs: the reference checkout of a famicom-rust beside this one.
export NES_TEST_ROMS=${NES_TEST_ROMS:-$(dirname "$ROOT")/famicom-rust/external/famicom-python/tests/test_roms}
# The dumps and the FDS BIOS: No-Intro's sets under NO_INTRO_DIR.
if [ -z "${FAMICOM_FDS_BIOS:-}" ]; then
    [ -n "${NO_INTRO_DIR:-}" ] || { echo "set NO_INTRO_DIR (No-Intro's sets) or FAMICOM_FDS_BIOS"; exit 1; }
    export FAMICOM_FDS_BIOS="$NO_INTRO_DIR/Nintendo - Family Computer Disk System (FDS)/[BIOS] Family Computer Disk System (Japan) (En) (Rev 1).zip"
fi
NES_DUMPS=${NES_DUMPS:-${NO_INTRO_DIR:-}/Nintendo - Nintendo Entertainment System (Headered)}
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
# The Vs. System's images: assembled from MAME's sets ($MAME_ROMPATH) by the
# reference's scripts/vs_images.py, outside the repository (famicom-rust's
# external/vs-images by default; never committed).
export VS_IMAGES_DIR=${VS_IMAGES_DIR:-$(dirname "$ROOT")/famicom-rust/external/vs-images}
[ -s "$VS_IMAGES_DIR/manifest.json" ] || { echo "no Vs. images at $VS_IMAGES_DIR (VS_IMAGES_DIR)"; exit 1; }
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
step "lockstep: rung 5, the Sunsoft 5B's sound (nes-audio-tests, Gimmick!)"
lockstep "$ROOT/scripts/cases-5b.tsv" "$EXT/refcache-5b"
step "lockstep: rung 5, mapper 559"
lockstep "$ROOT/scripts/cases-559.tsv" "$EXT/refcache-559"
step "lockstep: rung 5, AutoDisk (FDS sides changed as the program asks)"
lockstep "$ROOT/scripts/cases-fds-auto.tsv" "$EXT/refcache-mappers"
step "lockstep: rung 6, machine rows"
lockstep "$ROOT/scripts/cases-machines.tsv" "$EXT/refcache"
step "lockstep: rung 6, peripherals"
lockstep "$ROOT/scripts/cases-peripherals.tsv" "$EXT/refcache-devices"
step "lockstep: rung 6, host input (frontend.py's input half)"
lockstep "$ROOT/scripts/cases-host.tsv" "$EXT/refcache-host"
step "lockstep: rung 6, the Vs. System (UniSystem and DualSystem images)"
lockstep "$ROOT/scripts/cases-vs.tsv" "$EXT/refcache-vs"

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
# This run's shares only (a run with more JOBS leaves more part files behind).
outs=()
for i in $(seq 0 $((JOBS - 1))); do outs+=("$split_dir/part$i.out"); done
same=$(cat "${outs[@]}" | grep -c ": identical" || true)
cat "${outs[@]}" | grep -v ": identical" || true
echo "$same of $total dumps round-trip"
[ "$same" = "$total" ] || failed=1

step "save states: the 5B on the Famicom, 559 on the Dendy"
A=$FAMICOM_REFERENCE/reference/src/nes-audio-tests/build
extra=$(nice "$BIN/famicom-state-roundtrip" --machine famicom "$NES_DUMPS/Gimmick! (Japan) (En).zip" \
    "$A/db_5b.nes" "$A/envelope_5b.nes" "$A/noise_5b.nes" 2>&1; nice "$BIN/famicom-state-roundtrip" \
    --machine dendy "$NES_DUMPS/Sangokushi II - Haou no Tairiku (China) (Ja) (Subor) (Pirate).zip" 2>&1)
echo "$extra" | grep -v ": identical" || true
same=$(echo "$extra" | grep -c ": identical" || true)
echo "$same of 5 round-trip"
[ "$same" = 5 ] || failed=1

step "save states: every Vs. UniSystem image"
mapfile -t vs < <(python3 -c 'import json, os, sys; d = sys.argv[1]
for e in json.load(open(os.path.join(d, "manifest.json"))):
    if e["vs_hardware"] not in (5, 6): print(os.path.join(d, e["file"]))' "$VS_IMAGES_DIR")
vsout=$(nice "$BIN/famicom-state-roundtrip" --machine auto --at 300 --to 420 \
    --input "$ROOT/scripts/inputs/vs-state.txt" "${vs[@]}" 2>&1 || true)
echo "$vsout" | grep -v ": identical" || true
same=$(echo "$vsout" | grep -c ": identical" || true)
echo "$same of ${#vs[@]} round-trip"
[ "$same" = "${#vs[@]}" ] || failed=1

step "done"
if [ "$failed" = 0 ]; then echo "LADDER GREEN"; else echo "LADDER RED"; fi
exit "$failed"
