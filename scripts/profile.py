#!/usr/bin/env python3
"""Fold famicom-profile's samples into functions and source lines with addr2line.

    profile.py [external/profile.txt] [--lines N]

Each sample's address is resolved with its inline chain (``addr2line -i``):
the innermost frame is where the time went, the outer ones where it was
called from. Prints the functions by inclusive and by self share, then the
hottest lines. One addr2line run for all addresses (``-a`` marks where each
address's chain begins).
"""

from __future__ import annotations

import collections
import subprocess
import sys


def short(function: str) -> str:
    parts = function.split("::")
    return "::".join(parts[-2:]) if len(parts) > 1 else function


def main() -> int:
    argv = sys.argv[1:]
    lines_wanted = 40
    if "--lines" in argv:
        i = argv.index("--lines")
        lines_wanted = int(argv[i + 1])
        del argv[i : i + 2]
    path = argv[0] if argv else "external/profile.txt"
    exe, *addresses = open(path).read().split()
    counts = collections.Counter(addresses)
    unique = list(counts)
    out = subprocess.run(
        ["addr2line", "-e", exe, "-a", "-f", "-i", "-C", *("0x" + a for a in unique)],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    chains: dict[str, list[tuple[str, str]]] = {}
    current = None
    pending: list[str] = []
    for text in out:
        if text.startswith("0x") and len(pending) % 2 == 0:
            if current is not None:
                chains[current] = list(zip(pending[0::2], pending[1::2]))
            current = format(int(text, 16), "x")
            pending = []
        else:
            pending.append(text)
    if current is not None:
        chains[current] = list(zip(pending[0::2], pending[1::2]))
    total = sum(counts.values())
    inclusive: collections.Counter = collections.Counter()
    own: collections.Counter = collections.Counter()
    where: collections.Counter = collections.Counter()
    for address, n in counts.items():
        pairs = chains.get(format(int(address, 16), "x"), [])
        seen = set()
        for function, _ in pairs:
            name = short(function)
            if name not in seen:
                inclusive[name] += n
                seen.add(name)
        if pairs:
            own[short(pairs[0][0])] += n
            where[pairs[0][1].split("/src/")[-1].split(" ")[0]] += n
    print(f"{total} samples")
    print("\nfunctions (inclusive):")
    for name, n in inclusive.most_common(30):
        print(f"  {100 * n / total:5.1f}%  {name}")
    print("\nfunctions (self):")
    for name, n in own.most_common(30):
        print(f"  {100 * n / total:5.1f}%  {name}")
    print("\nlines (self):")
    for line, n in where.most_common(lines_wanted):
        print(f"  {100 * n / total:5.1f}%  {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
