#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Index dumps by the board the reference's loader builds for them.

Run on the reference's interpreter. For every .zip/.nes under the given
directories, ``famicom_python.cartridge.load`` identifies the board as the
reference does (nes20db first); one line per dump: mapper, submapper, the
mapper class's name, the source and the path. Dumps the loader refuses are
listed with class "-". Rung 5 picks its dumps from this index
(scripts/make_cases.py).

    mapper_dumps.py DIR... --out FILE [--jobs N]
"""

from __future__ import annotations

import argparse
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

from famicom_python.cartridge import load, read_rom_file


def one(path: str) -> str:
    try:
        loaded = load(read_rom_file(path))
    except Exception as error:  # noqa: BLE001 - a refused dump is a row too
        return f"-1\t0\t-\t{type(error).__name__}\t{path}"
    board = loaded.image.board
    return f"{board.mapper}\t{board.submapper}\t{type(loaded.cartridge).__name__}\t{loaded.source}\t{path}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("dirs", nargs="+")
    parser.add_argument("--out", required=True)
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    paths = sorted(
        str(p) for d in args.dirs for p in Path(d).rglob("*") if p.suffix.lower() in (".zip", ".nes")
    )
    with ProcessPoolExecutor(args.jobs) as pool, open(args.out, "w") as out:
        for row in pool.map(one, paths, chunksize=32):
            out.write(row + "\n")
    print(f"{len(paths)} dumps indexed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
