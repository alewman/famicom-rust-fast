#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The reference, famicom-python at a pinned commit, into external/ with a PyPy
# venv holding exactly the cores its pyproject pins, plus links to a local
# clone's test ROMs and reference/ (fetched by its scripts/fetch_references.py;
# never committed anywhere).
# Usage: scripts/fetch_famicompython.sh [DEST] [SOURCE]
# FAMICOM_PYTHON_COMMIT overrides the pin; PYTHON names the interpreter (PyPy).
# FAMICOM_PYTHON_SOURCE (or the SOURCE argument) names another clone to fetch the pin
# from, a local one while the pinned commit is not on GitHub yet; the default is GitHub.
set -euo pipefail
# c52a6d3 (2026-11-23): famicom-python's main, b3dbf1b plus the PlayChoice-10's BIOS
# found through MAME_ROMPATH and MAME's BSD-3 notice in NOTICE.md (no behaviour changed:
# comments, SPDX lines and bios_path); the reference the PlayChoice-10 was transcribed
# from. Before it, b3dbf1b (2026-11-23): famicom-python's main, which repins z80-python to b59c80b by git
# commit (the PlayChoice-10's supervisor; was the 0.4.0 release, which no Rust Z80
# transcribes) and ay8910-python to a3bb485 (was e9420aa, whose src/ differs only by
# SPDX lines; ay8910-rust transcribes a3bb485). Before it, e3e1474, which pinned the
# restarted ym2413-python 67fdb1e (written from documents); before that, the tag
# famicom-rust-pin-1 (47d33c1, fe89099 before the 2026-11-21 history rewrite), which
# pinned the archived ym2413-python 13df60c, a translation of GPL code that this
# family's MIT work must not reach. No new tag is made while the clean-up settles, so
# the pin is the commit. Until c52a6d3 is on GitHub, FAMICOM_PYTHON_SOURCE names a
# local clone that has it.
COMMIT=${FAMICOM_PYTHON_COMMIT:-c52a6d3f65c64d74528bb80dea5576f292d71d63}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=${1:-"$ROOT/external/famicom-python"}
SOURCE=${2:-${FAMICOM_PYTHON_SOURCE:-https://github.com/alewman/famicom-python.git}}
PYTHON=${PYTHON:-$(command -v pypy3.11 || command -v pypy3)}
if [ ! -d "$DEST/.git" ]; then
    git clone -q "$SOURCE" "$DEST"
fi
git -C "$DEST" fetch -q origin
if [ "$SOURCE" != "$(git -C "$DEST" remote get-url origin)" ]; then
    git -C "$DEST" fetch -q "$SOURCE" "+refs/heads/*:refs/remotes/source/*"
fi
git -C "$DEST" checkout -q --detach "$COMMIT"
if [ ! -x "$DEST/.venv-pypy/bin/python" ]; then
    "$PYTHON" -m venv "$DEST/.venv-pypy"
fi
# pip keeps an installed core whose pin URL changed (an old ym2413-python would survive
# this repin that way), so the reference's cores are uninstalled first and come back at
# the pins its pyproject names: 6502-python, ym2413-python and ay8910-python by git
# commit on GitHub, and z80-python (the PlayChoice-10's supervisor only) the same way.
"$DEST/.venv-pypy/bin/python" -m pip uninstall -q -y \
    6502-python ym2413-python ay8910-python z80-python famicom-python || true
"$DEST/.venv-pypy/bin/python" -m pip install -q -e "$DEST"
# The test ROMs and the reference documents (nes20db among them) come from a
# local famicom-python clone that has fetched them: LOCAL names it.
LOCAL=${LOCAL:-}
if [ -n "$LOCAL" ]; then
    [ -e "$DEST/tests/test_roms" ] || ln -s "$LOCAL/tests/test_roms" "$DEST/tests/test_roms"
    [ -e "$DEST/reference" ] || ln -s "$LOCAL/reference" "$DEST/reference"
fi
"$DEST/.venv-pypy/bin/python" -m pip freeze | grep -E "python @ git" || true
echo "famicom-python $(git -C "$DEST" rev-parse --short HEAD) in $DEST"
