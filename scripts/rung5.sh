#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 5: every mapper the reference builds, in lockstep on dumps that use it
# (scripts/mapper_dumps.py's index of No-Intro's NES sets; up to PER dumps per
# board class, 900 slices with Start pressed now and then), plus the FDS and
# its AutoDisk (scripts/cases-fds-auto.tsv), the Sunsoft 5B's sound (scripts/cases-5b.tsv)
# and mapper 559 (scripts/cases-559.tsv).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/make_mapper_cases.py" --all --per "${PER:-2}" --slices 900 \
    --out "$ROOT/external/cases-mappers.tsv"
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/external/cases-mappers.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
# The Sunsoft 5B's sound (rainwarrior's nes-audio-tests and Gimmick!) and mapper 559.
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-5b.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-5b" "$@"
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-559.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-559" "$@"
# AutoDisk (fds_auto): needs FAMICOM_FDS_BIOS.
exec python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-fds-auto.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
