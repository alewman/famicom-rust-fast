#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The lockstep gate: run each case on both boards and compare the traces.

Each case of ``scripts/cases.tsv`` runs ``famicom-trace`` (this board) and
``scripts/famicom_trace.py`` on the reference's interpreter, into
``external/traces/``, several at a time under ``nice``; the two traces are
compared line for line. A case passes when they are identical; otherwise the
first differing line is reported with its slice.

    lockstep.py [--only REGEX] [--jobs N] [--rust PATH] [--keep]
                [--reference-cache DIR]

``$NES_TEST_ROMS`` and the other case variables come from the environment,
with the defaults in scripts/make_cases.py. Stdlib only.
"""

from __future__ import annotations

import argparse
import os
import re
import shlex
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
from make_cases import DEFAULTS  # noqa: E402

# The reference checkout (scripts/fetch_famicompython.sh); FAMICOM_REFERENCE moves it
# (a worktree shares the main tree's).
REFERENCE = Path(os.environ.get("FAMICOM_REFERENCE", ROOT / "external" / "famicom-python"))
PYTHON = REFERENCE / ".venv-pypy" / "bin" / "python"


def cases(table: Path) -> list[tuple[str, int, str, list[str]]]:
    env = {**DEFAULTS, **{k: os.environ[k] for k in DEFAULTS if k in os.environ}}
    rows = []
    for line in table.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, slices, rom, args = (line.split("\t") + [""])[:4]
        for key, value in env.items():
            rom = rom.replace(f"${key}", value)
            args = args.replace(f"${key}", value)
        rows.append((name, int(slices), rom, shlex.split(args)))
    return rows


def one(case, rust: Path, traces: Path, keep: bool, cache: Path | None) -> tuple[str, bool, str]:
    name, slices, rom, args = case
    safe = re.sub(r"[^A-Za-z0-9._-]+", "_", name)
    ref, ours = traces / f"{safe}.ref", traces / f"{safe}.rust"
    if cache is not None:
        ref = cache / f"{safe}.{slices}.ref"  # kept: the reference's trace does not change
    common = [rom, "--slices", str(slices), *args]
    began = time.monotonic()
    if cache is None or not ref.is_file():
        reference = subprocess.run(
            ["nice", str(PYTHON), str(HERE / "famicom_trace.py"), *common, "--out", str(ref)],
            capture_output=True,
            text=True,
        )
        if reference.returncode:
            ref.unlink(missing_ok=True)
            return name, False, "reference failed: " + reference.stderr.strip()[-300:]
    board = subprocess.run(
        ["nice", str(rust), *common, "--out", str(ours)], capture_output=True, text=True
    )
    if board.returncode:
        return name, False, "rust failed: " + board.stderr.strip()[-300:]
    seconds = time.monotonic() - began
    with ref.open() as a, ours.open() as b:
        for number, (left, right) in enumerate(zip(a, b)):
            if left != right:
                return (
                    name,
                    False,
                    f"line {number}: reference {left.strip()!r}\n      rust      {right.strip()!r}",
                )
    if ref.stat().st_size != ours.stat().st_size:
        return name, False, "one trace is longer"
    if not keep:
        if cache is None:
            ref.unlink()
        ours.unlink()
    return name, True, f"identical, {slices} slices ({seconds:.0f} s)"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--only", help="a regular expression on the case name")
    parser.add_argument("--jobs", type=int, default=12)
    parser.add_argument("--cases", type=Path, default=HERE / "cases.tsv")
    parser.add_argument("--rust", type=Path, help="a trace binary to judge instead of this tree's")
    parser.add_argument("--traces", type=Path, default=ROOT / "external" / "traces")
    parser.add_argument("--keep", action="store_true", help="keep the traces of identical cases")
    parser.add_argument(
        "--reference-cache",
        type=Path,
        help="keep the reference's traces here and reuse them (the pin does not move)",
    )
    args = parser.parse_args()
    rust = args.rust
    if rust is None:
        manifest = str(ROOT / "Cargo.toml")
        build = subprocess.run(["cargo", "build", "--release", "-q", "--manifest-path", manifest])
        if build.returncode:
            return 1
        rust = ROOT / "target" / "release" / "famicom-trace"
    args.traces.mkdir(parents=True, exist_ok=True)
    if args.reference_cache:
        args.reference_cache.mkdir(parents=True, exist_ok=True)
    todo = [c for c in cases(args.cases) if not args.only or re.search(args.only, c[0])]
    # The longest first, so the pool does not end on one long case.
    todo.sort(key=lambda c: -c[1])
    failed = 0

    def run(case):
        return one(case, rust, args.traces, args.keep, args.reference_cache)

    with ThreadPoolExecutor(args.jobs) as pool:
        for name, ok, text in pool.map(run, todo):
            print(f"{name}: {text}", flush=True)
            failed += not ok
    print(f"{len(todo) - failed} of {len(todo)} cases identical")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
