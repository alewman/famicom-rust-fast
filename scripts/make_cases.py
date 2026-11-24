#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write scripts/cases.tsv, the lockstep gate's cases (names and arguments, no ROMs).

    make_cases.py

Rung 2 is every test ROM under $NES_TEST_ROMS (the reference's
tests/test_roms) whose header names a mapper this board builds, 600 slices
(ten seconds) each from power, plus AccuracyCoin's run-all from its menu
(Start pressed as the reference's harness presses it), 3,000 slices. The
inputs live in scripts/inputs/. Paths are written with the variables and
resolved by scripts/lockstep.py against DEFAULTS or the environment.
The dump folders ($NES_DUMPS, $NES_AFTERMARKET, $FDS_DUMPS) default to No-Intro's
sets under $NO_INTRO_DIR; with neither set, their cases are reported MISSING.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
REFERENCE = Path(os.environ.get("FAMICOM_REFERENCE", ROOT / "external" / "famicom-python"))
NO_INTRO = os.environ.get("NO_INTRO_DIR")


def no_intro(folder: str) -> str | None:
    """No-Intro's FOLDER under $NO_INTRO_DIR, or None when that is unset."""
    return str(Path(NO_INTRO) / folder) if NO_INTRO else None


#: The case variables; None: no default (set the variable or NO_INTRO_DIR).
DEFAULTS: dict[str, str | None] = {
    "NES_TEST_ROMS": str(REFERENCE / "tests" / "test_roms"),
    "NES_DUMPS": no_intro("Nintendo - Nintendo Entertainment System (Headered)"),
    "NES_AFTERMARKET": no_intro("Nintendo - Nintendo Entertainment System (Headered) (Aftermarket)"),
    "FDS_DUMPS": no_intro("Nintendo - Family Computer Disk System (FDS)"),
    "INPUTS": str(HERE / "inputs"),
    # rainwarrior's nes-audio-tests, prebuilt, in the reference's fetched reference/.
    "NES_AUDIO_TESTS": str(REFERENCE / "reference" / "src" / "nes-audio-tests" / "build"),
    "PROBES": str(ROOT / "external" / "probes"),
    # The Vs. System's images, assembled from MAME's sets ($MAME_ROMPATH) by the
    # reference's scripts/vs_images.py, outside the repository: no default.
    "VS_IMAGES_DIR": None,
}
# The mappers built so far (cartridge::build).
BUILT = {0}


def mapper(path: Path) -> int | None:
    header = path.read_bytes()[:16]
    if len(header) < 16 or header[:4] != b"NES\x1a":
        return None
    number = (header[6] >> 4) | (header[7] & 0xF0)
    if (header[7] & 0x0C) == 0x08:
        number |= (header[8] & 0x0F) << 8
    elif any(header[12:16]):
        number &= 0x0F
    return number


def main() -> None:
    roots = Path(DEFAULTS["NES_TEST_ROMS"])
    rows = [
        "# name\tslices\trom\targuments (shell words); $NES_TEST_ROMS $INPUTS",
        "accuracycoin-run-all\t3000\t$NES_TEST_ROMS/AccuracyCoin/AccuracyCoin.nes"
        "\t--input $INPUTS/accuracycoin-run-all.txt",
    ]
    for path in sorted(roots.rglob("*.nes")):
        relative = path.relative_to(roots)
        if relative.parts[0] == "AccuracyCoin" or mapper(path) not in BUILT:
            continue
        name = re.sub(r"\.nes$", "", "/".join(relative.parts))
        name = re.sub(r"\s+", "_", name)
        rows.append(f"{name}\t600\t$NES_TEST_ROMS/{'/'.join(relative.parts)}")
    (HERE / "cases.tsv").write_text("\n".join(rows) + "\n")
    print(f"{len(rows) - 1} cases")


if __name__ == "__main__":
    main()
