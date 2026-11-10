#!/usr/bin/env python3
"""Write scripts/cases.tsv, the lockstep gate's cases (names and arguments, no ROMs).

    make_cases.py

Rung 2 is every test ROM under $NES_TEST_ROMS (the reference's
tests/test_roms) whose header names a mapper this board builds, 600 slices
(ten seconds) each from power, plus AccuracyCoin's run-all from its menu
(Start pressed as the reference's harness presses it), 3,000 slices. The
inputs live in scripts/inputs/. Paths are written with the variables and
resolved by scripts/lockstep.py against DEFAULTS or the environment.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
REFERENCE = Path(os.environ.get("FAMICOM_REFERENCE", ROOT / "external" / "famicom-python"))
DEFAULTS = {
    "NES_TEST_ROMS": str(REFERENCE / "tests" / "test_roms"),
    "NES_DUMPS": "/data/emu/roms/nes",
    "INPUTS": str(HERE / "inputs"),
    "PROBES": str(ROOT / "external" / "probes"),
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
