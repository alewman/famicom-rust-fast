#!/usr/bin/env bash
# Rung 6: the machine rows (PAL, Dendy, Famicom, AV Famicom, NES-101) in
# lockstep, scripts/cases-machines.tsv; then the peripherals, every device
# class plugged (--device) and driven (scripts/inputs/devices) on test ROMs and
# dumps that use it, scripts/cases-peripherals.tsv (scripts/make_device_cases.py);
# then the host, the reference's window (frontend.py on a stand-in pygame) and
# famicom_rust::host driving each device from one player script,
# scripts/cases-host.tsv (scripts/make_host_inputs.py).
set -uo pipefail
HERE=$(dirname "$0")
python3 "$HERE/lockstep.py" --cases "$HERE/cases-machines.tsv" --jobs "${JOBS:-12}" "$@"
machines=$?
python3 "$HERE/lockstep.py" --cases "$HERE/cases-peripherals.tsv" --jobs "${JOBS:-12}" "$@"
peripherals=$?
python3 "$HERE/lockstep.py" --cases "$HERE/cases-host.tsv" --jobs "${JOBS:-12}" "$@"
host=$?
exit $((machines || peripherals || host))
