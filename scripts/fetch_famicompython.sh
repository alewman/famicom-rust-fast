#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The reference, famicom-python at the tag famicom-rust-pin-1, into external/
# with a PyPy venv, plus links to a local clone's test ROMs and reference/
# (fetched by its scripts/fetch_references.py; never committed anywhere).
# Usage: scripts/fetch_famicompython.sh [DEST] [SOURCE]
set -euo pipefail
PIN=famicom-rust-pin-1
# 47d33c1 = fe89099 after famicom-python's history rewrite of 2026-11-21: the tag keeps its name.
# The code is the same; that rewrite took out the GPL tools and .mailmap and replaced the
# furrtek-sourced A12 text in the docs and in mmc3.py's docstring (same line count).
COMMIT=47d33c1  # what the tag names (short)
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=${1:-"$ROOT/external/famicom-python"}
SOURCE=${2:-https://github.com/alewman/famicom-python.git}
PYTHON=${PYTHON:-pypy3}
if [ ! -d "$DEST/.git" ]; then
    git clone -q "$SOURCE" "$DEST"
fi
git -C "$DEST" fetch -q --tags origin 2>/dev/null || true
git -C "$DEST" checkout -q "$PIN"
case "$(git -C "$DEST" rev-parse HEAD)" in
    "$COMMIT"*) ;;
    *) echo "$PIN is not $COMMIT" >&2; exit 1 ;;
esac
if [ ! -x "$DEST/.venv-pypy/bin/python" ]; then
    "$PYTHON" -m venv "$DEST/.venv-pypy"
fi
"$DEST/.venv-pypy/bin/python" -m pip install -q -e "$DEST"
# The test ROMs and the reference documents (nes20db among them) come from a
# local famicom-python clone that has fetched them: LOCAL names it.
LOCAL=${LOCAL:-}
if [ -n "$LOCAL" ]; then
    [ -e "$DEST/tests/test_roms" ] || ln -s "$LOCAL/tests/test_roms" "$DEST/tests/test_roms"
    [ -e "$DEST/reference" ] || ln -s "$LOCAL/reference" "$DEST/reference"
fi
echo "famicom-python $(git -C "$DEST" describe --tags) in $DEST"
