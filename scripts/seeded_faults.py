#!/usr/bin/env python3
"""A gate that cannot fail proves nothing: build the board with one fault at a
time and check that some rung of scripts/ladder.sh catches each.

Each fault is one exact text replacement in a copy of ``src/``, built into its
own target directory (external/faults); then the ladder's rungs run against
it in order, cheapest first, until one fails: rung 1 (nestest), the lockstep
tables against the cached reference traces (rung 2, rung 5's board classes,
the FDS, Lagrange Point, the machine rows, the peripherals), rung 4
(AccuracyCoin on the reference's counts) and rung 3 (the test ROM tables).
A fault no rung catches is reported as MISSED.

    seeded_faults.py [--only NAME] [--jobs N]
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXT = ROOT / "external"

# name: (file, exact text, replacement). The fork's own shortcuts: each must
# be caught, or it could diverge unseen.
FAULTS = {
    # The frame counter's quiet run reaching over a step right after another
    # (29828, 29829, 29830 on NTSC: the IRQ's three cycles).
    "apu-quiet-run-over-the-next-step": (
        "apu.rs",
        ".find(|&at| at > cycle)",
        ".find(|&at| at > cycle + 1)",
    ),
    # The pixel dot taken while dark (the dot after wake-up, before the
    # line's next fetch). (Taking it while fading is no fault: `fading` is
    # only set with `on` cleared, so the test is redundant, kept as a guard.)
    "pixel-dot-while-dark": (
        "ppu.rs",
        "            && !self.dark\n            && self.oam_corrupt",
        "            && self.oam_corrupt",
    ),
    # The pixel dot's last, dot 256, without fine Y's increment. (Taking it
    # with an OAM corruption pending is no fault: the corruption is set on a
    # visible line only with rendering going off, and cleared when rendering
    # comes back on one, so it is spent on the pre-render line or a dot 0
    # before any pixel dot; the test is a guard.)
    "pixel-dot-skips-increment-y": (
        "ppu.rs",
        "        self.background_fetch(dot);\n        if dot == WIDTH as i32 {\n            self.increment_y();\n        }\n    }",
        "        self.background_fetch(dot);\n    }",
    ),
    # ... on the dot of PPU wake-up (rendering switched on).
    "pixel-dot-while-waking": (
        "ppu.rs",
        "            && !self.waking\n",
        "",
    ),
    # run_to's pixel dot taken with a register effect due on that dot.
    "run-to-pixel-dot-over-a-due-event": (
        "ppu.rs",
        ".is_none_or(|&(due, _)| due > self.time + ticks)",
        ".is_none_or(|&(due, _)| due > self.time)",
    ),
    # The pixel dot evaluating sprites in the dot between $2001's write of
    # sprites and background off and rendering's turning off. MISSED by the
    # whole ladder (2026-11-08): no case turns rendering off mid-evaluation
    # and then shows OAMADDR's or the corruption's effect of that one step.
    # A gap in the gate (famicom-rust's too), not taken as equivalent.
    "pixel-dot-evaluates-with-rendering-masked": (
        "ppu.rs",
        "    fn pixel_dot(&mut self, line: i32, dot: i32) {\n        if self.mask & 0x18 != 0 {\n            self.evaluate(line, dot);\n        }",
        "    fn pixel_dot(&mut self, line: i32, dot: i32) {\n        self.evaluate(line, dot);",
    ),
    # The frame counter's quiet run taken from before its first cycle.
    "apu-quiet-run-without-its-start": (
        "apu.rs",
        "if mode5 == self.mode5 && first <= cycle && cycle < end {",
        "if mode5 == self.mode5 && cycle < end {",
    ),
}

ENV = {
    "NES_TEST_ROMS": "/data/emu/famicom-rust/external/famicom-python/tests/test_roms",
    "FAMICOM_FDS_BIOS": "/data/emu/roms/fds/[BIOS] Family Computer Disk System (Japan) (En) (Rev 1).zip",
    "FAMICOM_NES20DB": str(EXT / "nes20db-2026-09-26.7z"),
    # The case tables' inputs (the binaries' default is beside their source).
    "INPUTS": str(ROOT / "scripts" / "inputs"),
}
TABLES = [
    ("rung 2", ROOT / "scripts" / "cases.tsv", EXT / "refcache"),
    ("rung 5", EXT / "cases-rung5.tsv", EXT / "refcache-mappers"),
    ("rung 5 FDS", EXT / "cases-fds.tsv", EXT / "refcache-mappers"),
    ("rung 5 VRC7", EXT / "cases-lagrange.tsv", EXT / "refcache-mappers"),
    ("rung 6 machines", ROOT / "scripts" / "cases-machines.tsv", EXT / "refcache"),
    ("rung 6 peripherals", ROOT / "scripts" / "cases-peripherals.tsv", EXT / "refcache-devices"),
]


def run(command: list[str], env: dict) -> subprocess.CompletedProcess:
    return subprocess.run(["nice", *command], capture_output=True, text=True, env=env)


def caught_by(binaries: Path, env: dict, jobs: int) -> str | None:
    roms = Path(env["NES_TEST_ROMS"])
    if run([str(binaries / "nestest"), str(roms / "nes-test-roms")], env).returncode:
        return "rung 1 (nestest)"
    for name, table, cache in TABLES:
        result = run(
            [str(binaries / "famicom-lockstep"), "--trace", str(binaries / "famicom-trace"),
             "--jobs", str(jobs), "--cases", str(table), "--cache", str(cache)],
            env,
        )  # fmt: skip
        if result.returncode:
            first = next(
                (line.split(":")[0] for line in result.stdout.splitlines()
                 if ": identical" not in line and not line.startswith(" ")),
                "?",
            )  # fmt: skip
            return f"{name} lockstep ({first})"
    coin = run(
        [str(binaries / "famicom-accuracycoin"), str(roms / "AccuracyCoin" / "AccuracyCoin.nes"),
         str(EXT / "accuracycoin-tests.json"), "--failures"],
        env,
    )  # fmt: skip
    last = coin.stdout.strip().splitlines()[-1] if coin.stdout.strip() else ""
    if coin.returncode or last != "147 of 147 pass; 126984584 CPU cycles, 4264 frames":
        return f"rung 4 ({last})"
    out = EXT / "faults" / "testrom-rust.jsonl"
    run(
        [str(binaries / "famicom-testrom"), str(EXT / "testrom-reference.jsonl"), "--roms",
         str(roms / "nes-test-roms"), "--out", str(out), "--jobs", str(jobs)],
        env,
    )  # fmt: skip
    if run([str(binaries / "famicom-testrom-compare"), str(EXT / "testrom-reference.jsonl"), str(out)],
           env).returncode:  # fmt: skip
        return "rung 3"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--work", type=Path, default=EXT / "faults")
    parser.add_argument("--only")
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    env = {**os.environ, **{k: os.environ.get(k, v) for k, v in ENV.items()}}
    missed = ran = 0
    for name, (file, old, new) in FAULTS.items():
        if args.only and name not in args.only.split(","):
            continue
        ran += 1
        tree = args.work / name
        if tree.exists():
            shutil.rmtree(tree)
        shutil.copytree(ROOT / "src", tree / "src")
        shutil.copytree(ROOT / "tests", tree / "tests")
        shutil.copy(ROOT / "Cargo.lock", tree / "Cargo.lock")
        # The VRC7's chip is a path dependency, beside this checkout.
        manifest = (ROOT / "Cargo.toml").read_text()
        ym = (ROOT.parent / "ym2413-rust").resolve()
        (tree / "Cargo.toml").write_text(manifest.replace('path = "../ym2413-rust"', f'path = "{ym}"'))
        source = tree / "src" / file
        text = source.read_text()
        if text.count(old) != 1:
            print(f"{name}: the fault's text is not in {file} exactly once")
            return 2
        source.write_text(text.replace(old, new))
        target = args.work / "target"
        build = subprocess.run(
            ["nice", "cargo", "build", "-j", "4", "--release", "-q", "--manifest-path",
             str(tree / "Cargo.toml"), "--target-dir", str(target)],
            capture_output=True, text=True, env=env,
        )  # fmt: skip
        if build.returncode:
            print(f"{name}: does not build\n{build.stderr[-800:]}")
            return 2
        caught = caught_by(target / "release", env, args.jobs)
        print(f"{name}: {'caught by ' + caught if caught else 'MISSED'}", flush=True)
        missed += caught is None
    print(f"{ran - missed} caught, {missed} missed")
    return 1 if missed else 0


if __name__ == "__main__":
    raise SystemExit(main())
