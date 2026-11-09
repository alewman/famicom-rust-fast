#!/usr/bin/env bash
# Rung 5: every mapper the reference builds, in lockstep on dumps that use it
# (scripts/mapper_dumps.py's index of /data/emu/roms/nes; up to PER dumps per
# board class, 900 slices with Start pressed now and then), plus the FDS and
# its AutoDisk (scripts/cases-fds-auto.tsv).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/make_mapper_cases.py" --all --per "${PER:-2}" --slices 900 \
    --out "$ROOT/external/cases-mappers.tsv"
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/external/cases-mappers.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
# AutoDisk (fds_auto): needs FAMICOM_FDS_BIOS.
exec python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-fds-auto.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
