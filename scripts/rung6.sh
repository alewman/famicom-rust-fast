#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 6: the machine rows (PAL, Dendy, Famicom, AV Famicom, NES-101, and the Vs.
# rows' RGB PPUs and letterless 2A03 on home test ROMs) in lockstep,
# scripts/cases-machines.tsv; then the peripherals, every device class plugged
# (--device) and driven (scripts/inputs/devices) on test ROMs and dumps that use it,
# scripts/cases-peripherals.tsv (scripts/make_device_cases.py); then the host, the
# reference's window (frontend.py on a stand-in pygame) and famicom_rust::host
# driving each device from one player script, scripts/cases-host.tsv
# (scripts/make_host_inputs.py); then the Vs. System, every image the reference's
# scripts/vs_images.py assembles from MAME's sets ($VS_IMAGES_DIR, outside the
# repository), scripts/cases-vs.tsv (scripts/make_vs_cases.py), and save states on
# every UniSystem image; then the PlayChoice-10, every image the reference's
# scripts/pc10_images.py assembles ($PC10_IMAGES_DIR) in the cabinet with MAME's BIOS
# (MAME_ROMPATH), scripts/cases-pc10.tsv (scripts/make_pc10_cases.py), and save
# states on every image, saved mid-game, and on one saved while the watchdog counts
# (--bios-program).
set -uo pipefail
HERE=$(dirname "$0")
ROOT=$(cd "$HERE/.." && pwd)
python3 "$HERE/lockstep.py" --cases "$HERE/cases-machines.tsv" --jobs "${JOBS:-12}" "$@"
machines=$?
python3 "$HERE/lockstep.py" --cases "$HERE/cases-peripherals.tsv" --jobs "${JOBS:-12}" "$@"
peripherals=$?
python3 "$HERE/lockstep.py" --cases "$HERE/cases-host.tsv" --jobs "${JOBS:-12}" "$@"
host=$?
vs=1
states=1
if [ -n "${VS_IMAGES_DIR:-}" ]; then
    python3 "$HERE/lockstep.py" --cases "$HERE/cases-vs.tsv" --jobs "${JOBS:-12}" \
        --reference-cache "$ROOT/external/refcache-vs" "$@"
    vs=$?
    # Save states on the UniSystem images (the DualSystem has none in the reference).
    mapfile -t roms < <(python3 -c 'import json, os, sys; d = sys.argv[1]
for e in json.load(open(os.path.join(d, "manifest.json"))):
    if e["vs_hardware"] not in (5, 6): print(os.path.join(d, e["file"]))' "$VS_IMAGES_DIR")
    nice "$ROOT/target/release/famicom-state-roundtrip" --machine auto --at 300 --to 420 \
        --input "$HERE/inputs/vs-state.txt" "${roms[@]}" | tee "$ROOT/external/states-vs.log" | grep -v ": identical"
    states=${PIPESTATUS[0]}
    echo "$(grep -c ': identical' "$ROOT/external/states-vs.log") of ${#roms[@]} Vs. images round-trip"
else
    echo "VS_IMAGES_DIR unset: the Vs. System's cases not run"
fi
pc10=1
pc10_states=1
if [ -n "${PC10_IMAGES_DIR:-}" ]; then
    python3 "$HERE/lockstep.py" --cases "$HERE/cases-pc10.tsv" --jobs "${JOBS:-12}" \
        --reference-cache "$ROOT/external/refcache-pc10" "$@"
    pc10=$?
    mapfile -t roms < <(python3 -c 'import json, os, sys; d = sys.argv[1]
for e in json.load(open(os.path.join(d, "manifest.json"))): print(os.path.join(d, e["file"]))' "$PC10_IMAGES_DIR")
    nice "$ROOT/target/release/famicom-pc10-states" --at 900 --to 1000 \
        --input "$HERE/inputs/pc10/play.txt" "${roms[@]}" | tee "$ROOT/external/states-pc10.log" | grep -v ": identical"
    pc10_states=${PIPESTATUS[0]}
    echo "$(grep -c ': identical' "$ROOT/external/states-pc10.log") of ${#roms[@]} PlayChoice-10 images round-trip"
    # Saved while the watchdog counts: our own supervisor program (the real BIOS never
    # lets it count), four save points, both parities of boot.
    for at in 40 43 49 52; do
        nice "$ROOT/target/release/famicom-pc10-states" --bios-program "$HERE/inputs/pc10/watchdog.hex" \
            --at $at --to $((at + 40)) "$PC10_IMAGES_DIR/Super Mario Bros..nes" || pc10_states=1
    done
else
    echo "PC10_IMAGES_DIR unset: the PlayChoice-10's cases not run"
fi
exit $((machines || peripherals || host || vs || states || pc10 || pc10_states))
