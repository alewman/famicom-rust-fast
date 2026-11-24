#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write scripts/cases-vs.tsv and scripts/inputs/vs/: the Vs. System's lockstep cases.

    make_vs_cases.py [--images DIR]

Every image the reference's ``scripts/vs_images.py`` assembled from MAME's sets
(``$VS_IMAGES_DIR``, outside the repository: its ``manifest.json`` names each
image, its board and MAME's default DIP switches) becomes one case of 900
slices on the machine ``System.machine_for`` picks (``--machine auto``), with
``--rgb`` so the RGB PPUs' colour tables are judged too. The input script is
vs_judge.py's play script on frames: MAME's DIP switches, two coins in
acceptor 1, the panel's button 1 twice, A on both sticks, then some play on
the sticks, a coin in acceptor 2 and the service button. A Vs. Zapper game
gets the gun (``--device auto``) aimed and fired instead of the first stick;
a DualSystem image runs both sides (``--dual``), each with its own DIP
switches (MAME's DSW0 for the primary side, DSW1 for the secondary) and coins.
One more case resets Vs. Super Mario Bros. mid-game (the letterless 2A03's
frame counter, the coin counter's latch). Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SLICES = 900

STICK_PLAY = [
    (700, "81"), (720, "80"), (740, "83"), (780, "41"), (820, "00"), (830, "10"),
    (840, "20"), (850, "02"), (860, "00"),
]  # fmt: skip


def uni(dip: int, gun: bool) -> list[str]:
    lines = [f"0 dip {dip:02X}", "240 coin 0", "270 coin 0"]
    if gun:
        # The gun on $4016's stick: aimed across the screen, fired now and then
        # (vs.VsZapper); the left stick still takes the panel's button 2 and A.
        shots = [(380, 128, 120), (400, 40, 60), (520, 200, 180), (640, 128, 40), (700, 20, 220)]
        for frame, x, y in shots:
            lines += [f"{frame} set - aim {x} {y}", f"{frame} set - trigger 1",
                      f"{frame + 6} set - trigger 0"]  # fmt: skip
        lines += ["760 set - aim none", "800 set - trigger 1", "806 set - trigger 0"]
        lines += ["640 1 01", "650 1 00"]
    else:
        lines += ["400 0 04", "408 0 00", "520 0 04", "528 0 00", "640 0 01", "640 1 01",
                  "650 0 00", "650 1 00"]  # fmt: skip
        lines += [f"{frame} 0 {buttons}" for frame, buttons in STICK_PLAY]
    lines += ["870 coin 1", "880 service 1", "884 service 0"]
    return lines


def dual(dips: tuple[int, int]) -> list[str]:
    lines = [f"0 dip 0 {dips[0]:02X}", f"0 dip 1 {dips[1]:02X}"]
    for side in (0, 1):
        lines += [f"240 coin {side} 0", f"270 coin {side} 0"]
    for pad in (0, 2):
        lines += [f"400 {pad} 04", f"408 {pad} 00", f"520 {pad} 04", f"528 {pad} 00"]
    lines += [f"640 {pad} 01" for pad in range(4)] + [f"650 {pad} 00" for pad in range(4)]
    for pad in (0, 2):
        lines += [f"{frame} {pad} {buttons}" for frame, buttons in STICK_PLAY]
    lines += ["870 coin 1 1", "880 service 0 1", "884 service 0 0"]
    return lines


def safe(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", name).strip("_.-")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--images", type=Path, default=os.environ.get("VS_IMAGES_DIR"))
    args = parser.parse_args()
    if args.images is None:
        raise SystemExit("set VS_IMAGES_DIR (scripts/vs_images.py's --out) or pass --images")
    manifest = json.loads((args.images / "manifest.json").read_text())
    inputs = HERE / "inputs" / "vs"
    inputs.mkdir(parents=True, exist_ok=True)
    rows = ["# name\tslices\trom\targuments: the Vs. System (scripts/make_vs_cases.py); "
            "$VS_IMAGES_DIR $INPUTS"]  # fmt: skip
    for entry in sorted(manifest, key=lambda e: e["file"]):
        stem = Path(entry["file"]).stem
        # A title outside ASCII is named by MAME's set too (safe() keeps ASCII only).
        stem = safe(stem) if stem.isascii() else safe(f"{entry['set']}-{stem}")
        dips = entry["dips"]
        if entry["vs_hardware"] in (5, 6) and "DSW1" in dips:
            lines = dual((dips.get("DSW0", 0), dips.get("DSW1", 0)))
            extra = "--dual --rgb"
        else:
            gun = entry.get("expansion") == 7
            lines = uni(dips.get("DSW0", 0), gun)
            extra = "--machine auto --rgb" + (" --device auto" if gun else "")
            if entry["vs_hardware"] == 6:  # Raid on Bungeling Bay: one DIP bank, both sides
                lines = dual((dips.get("DSW0", 0), dips.get("DSW0", 0)))
                extra = "--dual --rgb"
        (inputs / f"{stem}.txt").write_text("\n".join(lines) + "\n")
        rows.append(f"vs/{stem}\t{SLICES}\t$VS_IMAGES_DIR/{entry['file']}\t"
                    f"{extra} --input $INPUTS/vs/{stem}.txt")  # fmt: skip
        if entry["file"] == "Vs. Super Mario Bros. (SM4-4 E).nes":
            rows.append(f"vs/{stem}-reset\t{SLICES}\t$VS_IMAGES_DIR/{entry['file']}\t"
                        f"{extra} --reset 600 --input $INPUTS/vs/{stem}.txt")  # fmt: skip
    (HERE / "cases-vs.tsv").write_text("\n".join(rows) + "\n")
    print(f"{len(rows) - 1} cases written to {HERE / 'cases-vs.tsv'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
