#!/usr/bin/env bash
# Rung 2: the lockstep gate on every NROM test ROM (600 slices) and
# AccuracyCoin's run-all (3,000), scripts/cases.tsv; then the probes, our own
# test ROMs (probes/*.s, assembled by probes/build.sh), scripts/cases-probes.tsv.
set -e
here=$(dirname "$0")
python3 "$here/lockstep.py" --jobs "${JOBS:-12}" "$@"
"$here/../probes/build.sh"
python3 "$here/lockstep.py" --jobs "${JOBS:-12}" --cases "$here/cases-probes.tsv" "$@"
