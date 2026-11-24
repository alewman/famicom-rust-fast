#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 5: every mapper the reference builds, in lockstep on dumps that use it
# (scripts/mapper_dumps.py's index of No-Intro's NES sets; up to PER dumps per
# board class, 600 slices with Start pressed now and then), plus the FDS and
# its AutoDisk (scripts/cases-fds-auto.tsv), the Sunsoft 5B's sound (scripts/cases-5b.tsv)
# and mapper 559 (scripts/cases-559.tsv).
#
# 600 slices, written to external/cases-rung5.tsv: the table every recorded run
# of rung 5 used (581 cases since 2026-11-08) and famicom-rust-fast's ladder reads,
# with its 581 reference traces cached at 600 slices. This script asked for 900
# (make_mapper_cases.py's default, from the first version of 2026-11-08) and wrote
# external/cases-mappers.tsv, a table no certified run used: running it made the
# reference trace 581 dumps afresh at 900 slices instead of reading the cache
# (agreed 2026-11-23; README, rung 5).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/make_mapper_cases.py" --all --per "${PER:-2}" --slices 600 \
    --out "$ROOT/external/cases-rung5.tsv"
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/external/cases-rung5.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
# The Sunsoft 5B's sound (rainwarrior's nes-audio-tests and Gimmick!) and mapper 559.
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-5b.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-5b" "$@"
python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-559.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-559" "$@"
# AutoDisk (fds_auto): needs FAMICOM_FDS_BIOS.
exec python3 "$ROOT/scripts/lockstep.py" --cases "$ROOT/scripts/cases-fds-auto.tsv" \
    --jobs "${JOBS:-12}" --reference-cache "$ROOT/external/refcache-mappers" "$@"
