#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Rungs 3-4's reference side: every test ROM case of the reference, its board and outcome.

Run on the reference's interpreter. For each case of ``famicom_python.testrom``'s
suites (CPU, PPU, APU, MAPPER, FDS, PAL), one JSON line: the case's fields, the
board ``run_case`` builds for it (the identified board, 8 KiB of PRG-RAM added
for a $6000 ROM without, the case's changes), and the reference's own
``run_case`` outcome (passed, detail, CPU cycles, instructions). The Rust
harness (``famicom-testrom``) runs the same cases on that board and must
produce the same outcomes, field for field.

    testrom_outcomes.py --out FILE [--suite NAME ...] [--jobs N] [--only REGEX]
    testrom_outcomes.py --cases-only --out FILE     (the cases, no outcomes)
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import re
import sys
from concurrent.futures import ProcessPoolExecutor
from dataclasses import replace
from pathlib import Path

from famicom_python import testrom
from famicom_python.cartridge import load

REFERENCE = Path(testrom.__file__).resolve().parents[2]
ROMS = REFERENCE / "tests" / "test_roms" / "nes-test-roms"
SUITES = {
    "cpu": testrom.CPU_ROMS,
    "ppu": testrom.PPU_ROMS,
    "apu": testrom.APU_ROMS,
    "mappers": testrom.MAPPER_ROMS,
    "fds": testrom.FDS_ROMS,
    "pal": testrom.PAL_ROMS,
}


def board_for(case: testrom.RomCase) -> dict:
    """``run_case``'s board, as JSON."""
    data = testrom.rom_bytes(ROMS, case.path)
    loaded = load(data)
    board = loaded.image.board
    if case.protocol == "6000" and not board.prg_ram + board.prg_nvram:
        board = replace(board, prg_ram=0x2000)
    board = replace(board, **dict(case.board))
    return {"fds": loaded.source == "FDS image", **dataclasses.asdict(board)}


def describe(suite: str, case: testrom.RomCase) -> dict:
    fields = dataclasses.asdict(case)
    fields.pop("board")
    try:
        board = board_for(case)
    except Exception as error:  # noqa: BLE001 - a missing ROM is reported, not fatal
        board = {"error": f"{type(error).__name__}: {error}"}
    return {"suite": suite, "label": case.label, **fields, "resolved_board": board}


def run(item: tuple[str, testrom.RomCase]) -> str:
    suite, case = item
    row = describe(suite, case)
    if "error" not in row["resolved_board"]:
        outcome = testrom.run_case(case, ROMS)
        row["outcome"] = {
            "passed": outcome.passed,
            "detail": outcome.detail,
            "cycles": outcome.cycles,
            "instructions": outcome.instructions,
        }
    return json.dumps(row)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--suite", action="append", choices=sorted(SUITES))
    parser.add_argument("--only")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--cases-only", action="store_true")
    args = parser.parse_args()
    items = [
        (suite, case)
        for suite in args.suite or list(SUITES)
        for case in SUITES[suite]
        if not args.only or re.search(args.only, case.label)
    ]
    with args.out.open("w") as out:
        if args.cases_only:
            for suite, case in items:
                out.write(json.dumps(describe(suite, case)) + "\n")
        else:
            with ProcessPoolExecutor(args.jobs) as pool:
                for line in pool.map(run, items):
                    out.write(line + "\n")
                    out.flush()
    print(f"{len(items)} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
