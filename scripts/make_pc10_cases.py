#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write scripts/cases-pc10.tsv and scripts/inputs/pc10/: the PlayChoice-10's lockstep cases.

    make_pc10_cases.py [--images DIR]

Every image the reference's ``scripts/pc10_images.py`` assembled from MAME's sets
(``$PC10_IMAGES_DIR``, outside the repository; its ``manifest.json`` names each
image) becomes one case of 1,320 slices in the cabinet (``--pc10``: the
supervisor with MAME's dual-monitor BIOS from ``MAME_ROMPATH``, the game board
under it) with ``--rgb``. The play script (``play.txt``): SW2 at $05 (60 units of
time a second, so a coin's 300 units last 5 s), and the BIOS left to boot, check
the security PROM and run the game's attract mode (about 11.7 s); then a coin,
Enter, play on the panel (Start, A on both registers, the stick) until the time
runs out and the supervisor stops the game at one of its interrupts, and the
menu after it. A gun game (NES 2.0 expansion device 8) gets ``gun.txt``: the
same, with the gun aimed and fired instead of the stick. Four more cases:
*Super Mario Bros.* left in attract mode on MAME's default DIP switches
(``attract.txt``, 300 units a coin, one a second), and *Super Mario Bros.*, *Duck
Hunt* and *Tennis* on the one-monitor BIOS (``--bios single``: its time digits,
its screen flipped by /DISP MASK) with the cabinet's Reset and Channel buttons
pressed (``single.txt``). Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SLICES = 1320
GUN = 8

BOOT = ["0 dip 40 05"]
ENTER = ["720 coin 0", "780 button ENTER 1", "786 button ENTER 0"]
PANEL = [
    "850 0 08", "856 0 00", "910 0 08", "916 0 00", "960 0 01", "960 1 01", "966 0 00",
    "966 1 00", "1000 0 80", "1060 0 00", "1070 0 08", "1076 0 00", "1100 0 02", "1106 0 00",
]  # fmt: skip
SHOTS = [(900, 128, 120), (960, 40, 60), (1020, 200, 180), (1060, 128, 40)]

SCRIPTS = {
    "play.txt": BOOT + ENTER + PANEL,
    "gun.txt": BOOT
    + ENTER
    + ["850 0 08", "856 0 00"]
    + [
        line
        for frame, x, y in SHOTS
        for line in (f"{frame} aim {x} {y}", f"{frame} trigger 1", f"{frame + 4} trigger 0")
    ]
    + ["1100 aim none", "1110 trigger 1", "1114 trigger 0"],
    "attract.txt": ["0 dip 40 3F", "1000 coin 1", "1100 button SERVICE 1", "1104 button SERVICE 0"],
    "single.txt": BOOT
    + ENTER
    + PANEL[:6]
    + ["1150 button CHANNEL 1", "1154 button CHANNEL 0", "1200 button RESET 1",
       "1206 button RESET 0"],  # fmt: skip
}


def safe(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--images", type=Path, default=os.environ.get("PC10_IMAGES_DIR"))
    args = parser.parse_args()
    if args.images is None:
        parser.error("--images or PC10_IMAGES_DIR")
    manifest = json.loads((args.images / "manifest.json").read_text())
    inputs = HERE / "inputs" / "pc10"
    inputs.mkdir(parents=True, exist_ok=True)
    for name, lines in SCRIPTS.items():
        (inputs / name).write_text("\n".join(lines) + "\n")
    rows = ["# name\tslices\trom\targuments"]
    for entry in manifest:
        script = "gun.txt" if entry["expansion"] == GUN else "play.txt"
        rows.append(
            f"pc10-{safe(entry['file'][:-4])}\t{SLICES}\t$PC10_IMAGES_DIR/{entry['file']}\t"
            f"--pc10 --rgb --input $INPUTS/pc10/{script}"
        )
    smb = "$PC10_IMAGES_DIR/Super Mario Bros..nes"
    rows.append(f"pc10-attract-Super_Mario_Bros\t900\t{smb}\t--pc10 --rgb --input "
                "$INPUTS/pc10/attract.txt")  # fmt: skip
    for game in ("Super Mario Bros.", "Duck Hunt", "Tennis"):
        rows.append(
            f"pc10-single-{safe(game)}\t{SLICES}\t$PC10_IMAGES_DIR/{game}.nes\t"
            "--pc10 --bios single --rgb --input $INPUTS/pc10/single.txt"
        )
    (HERE / "cases-pc10.tsv").write_text("\n".join(rows) + "\n")
    print(f"{len(rows) - 1} cases")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
