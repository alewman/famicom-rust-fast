#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The reference, famicom-python at a pinned commit, into external/ with a PyPy
# venv holding exactly the cores its pyproject pins, plus links to a local
# clone's test ROMs and reference/ (fetched by its scripts/fetch_references.py;
# never committed anywhere).
# Usage: scripts/fetch_famicompython.sh [DEST] [SOURCE]
# FAMICOM_PYTHON_COMMIT overrides the pin; PYTHON names the interpreter (PyPy).
set -euo pipefail
# e3e1474 (2026-11-23): famicom-python's main, which pins the restarted ym2413-python
# 67fdb1e (written from documents) and ay8910-python e9420aa, both public. The old pin,
# the tag famicom-rust-pin-1 (47d33c1, fe89099 before the 2026-11-21 history rewrite),
# pinned the archived ym2413-python 13df60c, a translation of GPL code that this family's
# MIT work must not reach. No new tag is made while the clean-up settles, so the pin is
# the commit.
COMMIT=${FAMICOM_PYTHON_COMMIT:-e3e14747355ab4404c1df4415a977f074a0404c9}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=${1:-"$ROOT/external/famicom-python"}
SOURCE=${2:-https://github.com/alewman/famicom-python.git}
PYTHON=${PYTHON:-$(command -v pypy3.11 || command -v pypy3)}
if [ ! -d "$DEST/.git" ]; then
    git clone -q "$SOURCE" "$DEST"
fi
git -C "$DEST" fetch -q origin
git -C "$DEST" checkout -q --detach "$COMMIT"
if [ ! -x "$DEST/.venv-pypy/bin/python" ]; then
    "$PYTHON" -m venv "$DEST/.venv-pypy"
fi
# pip keeps an installed core whose pin URL changed (an old ym2413-python would survive
# this repin that way), so the reference's cores are uninstalled first and come back at
# the pins its pyproject names: 6502-python, ym2413-python and ay8910-python by git
# commit on GitHub; z80-python (the PlayChoice-10's supervisor only) as a release.
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
"$DEST/.venv-pypy/bin/python" -m pip freeze | grep -E "python @ git|z80-python" || true
echo "famicom-python $(git -C "$DEST" rev-parse --short HEAD) in $DEST"
