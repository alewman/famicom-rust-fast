#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write a cases file for rung 5: dumps that use the given mappers.

Reads the index scripts/mapper_dumps.py wrote (default external/mapper-dumps.tsv)
and picks, for each (mapper, submapper, class) the reference builds, up to
``--per`` dumps (licensed ones first, then by name), each run for ``--slices``
with scripts/inputs/start-mash.txt so games leave their title screens.

    make_mapper_cases.py --mappers 21,22,23 [--per 3] [--slices 600]
                         [--index FILE] --out FILE
    make_mapper_cases.py --all ...      every mapper in the index
"""

from __future__ import annotations

import argparse
import re
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def rank(path: str) -> tuple[int, str]:
    name = Path(path).name
    odd = bool(re.search(r"\((Unl|Pirate|Proto|Beta|Demo|Sample|Aftermarket|Hack)", name))
    return (odd, name)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mappers", default="")
    parser.add_argument("--all", action="store_true")
    parser.add_argument("--per", type=int, default=3)
    parser.add_argument("--slices", type=int, default=600)  # rung 5's (rung5.sh)
    parser.add_argument("--index", type=Path, default=ROOT / "external" / "mapper-dumps.tsv")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    wanted = {int(m) for m in args.mappers.split(",") if m}
    groups: dict[tuple[int, int, str], list[str]] = defaultdict(list)
    for line in args.index.read_text().splitlines():
        mapper, submapper, cls, _source, path = line.split("\t")
        if cls == "-" or (not args.all and int(mapper) not in wanted):
            continue
        groups[(int(mapper), int(submapper), cls)].append(path)
    rows = ["# name\tslices\trom\targuments"]
    taken: dict[str, int] = {}
    for (mapper, submapper, cls), paths in sorted(groups.items()):
        for path in sorted(paths, key=rank)[: args.per]:
            stem = re.sub(r"[^A-Za-z0-9]+", "_", Path(path).stem).strip("_")[:60]
            name = f"m{mapper}.{submapper}-{cls}-{stem}"
            # Names that differ only in punctuation ("Fatal Fury 2" and "Fatal
            # Fury 2'") would share a cached reference trace: number repeats.
            count = taken.get(name, 0)
            taken[name] = count + 1
            if count:
                name = f"{name}-{count + 1}"
            rows.append(f"{name}\t{args.slices}\t{path}\t--input $INPUTS/start-mash.txt")
    args.out.write_text("\n".join(rows) + "\n")
    print(f"{len(rows) - 1} cases in {args.out}")


if __name__ == "__main__":
    main()
