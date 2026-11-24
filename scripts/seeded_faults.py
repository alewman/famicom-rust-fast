#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""A gate that cannot fail proves nothing: build the board with one fault at a
time and check that some rung of scripts/ladder.sh catches each.

Each fault is one exact text replacement in a copy of ``src/``, built into its
own target directory (external/faults); then the ladder's rungs run against
it in order, cheapest first, until one fails: rung 1 (nestest), the lockstep
tables against the cached reference traces (rung 2 and its probes, rung 5's
board classes, the FDS, Lagrange Point, AutoDisk, the machine rows, the
peripherals, the host input), rung 4
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
    # The pixel dot evaluating sprites with rendering masked. On a 2C02 this
    # is no fault: the one way to `on` with the mask clear is a $2001 write
    # of both bits off while rendering, which also sets `oam_corrupt`, and
    # `pixel_dot_ready` refuses until a write turns rendering back on (missed
    # on 2026-11-08 and taken for a gap; then shown: the probe
    # probes/eval_turn_off.s turns rendering off mid-evaluation 329 times,
    # and a panic in place of the skipped evaluation fired on no 2C02 case).
    # An RGB PPU (the Vs. rows, 2026-11-23) arms no OAM corruption, so there
    # the mask test is all that stands between the pixel dot and a wrong
    # evaluation: the probe on the RP2C03B (probes/eval-turn-off-vs) must
    # catch it.
    "pixel-dot-evaluates-while-masked": (
        "ppu.rs",
        "        if self.mask & 0x18 != 0 {\n            self.evaluate(line, dot);\n        }\n        self.pixel(line, (dot - 2) as usize);",
        "        self.evaluate(line, dot);\n        self.pixel(line, (dot - 2) as usize);",
    ),
    # The Vs. mainboard's read out of line (`vs_read`) taken for too few
    # addresses: the shared RAM at $6000 left to the sockets.
    "vs-read-misses-the-ram": (
        "board.rs",
        "let read = if address < 0x8000 && self.vs.is_some() {",
        "let read = if address < 0x6000 && self.vs.is_some() {",
    ),
    # The watchdog's $4017 read, noted in the port branch, not noted.
    "vs-watchdog-not-fed": (
        "board.rs",
        "                if let Some(vs) = self.vs.as_mut() {\n                    vs.fed = t;\n                }",
        "",
    ),
    # The external /IRQ (a DualSystem's cross interrupt), tested first, lost.
    "external-irq-lost": (
        "board.rs",
        "let irq = self.apu.irq || self.external_irq || self.ppu.cart.irq();",
        "let irq = self.apu.irq || self.ppu.cart.irq();",
    ),
    # The frame counter's quiet run taken from before its first cycle.
    # The PlayChoice-10 (merged from famicom-rust 2026-11-23): the supervisor's
    # watchdog, the stop flip-flop, the PROM's inverting buffer, the one-monitor
    # board's screen flip, the held 2A03's idle cycle, the panel's serial input,
    # the cabinet's latch queue and its clock interleave.
    "pc10-watchdog-on-the-seventh-blank": (
        "supervisor.rs",
        "pub const WATCHDOG_BLANKS: u32 = 8;",
        "pub const WATCHDOG_BLANKS: u32 = 7;",
    ),
    "pc10-stop-ignores-game-stop": (
        "supervisor.rs",
        "self.stopped = self.supervisor.cpu.bus.latch[GAME_STOP] == 0;",
        "self.stopped = false;",
    ),
    "pc10-prom-data-not-inverted": (
        "supervisor.rs",
        "value |= (prom.data_out() ^ 1) << 3",
        "value |= prom.data_out() << 3",
    ),
    "pc10-single-monitor-never-flips": (
        "supervisor.rs",
        "self.monitor_on_game = !self.monitor_on_game;",
        "",
    ),
    "pc10-held-2a03-stops-the-apu": (
        "board.rs",
        "        self.t = t;\n        self.apu.clock();\n        self.ppu.run_to(t);",
        "        self.t = t;\n        self.ppu.run_to(t);",
    ),
    "pc10-panel-shifts-in-zeros": (
        "playchoice.rs",
        "self.shift = (self.shift >> 1) | 0x80; // the grounded",
        "self.shift = self.shift >> 1; // the grounded",
    ),
    "pc10-latch-sees-sdcs-late": (
        "supervisor.rs",
        "            sdcs: self.latch[SDCS],",
        "            sdcs: 1,",
    ),
    "pc10-z80-steps-on-a-tie-late": (
        "supervisor.rs",
        "self.supervisor.cpu.bus.t * 945 <= self.game.cpu.bus.t * 176",
        "self.supervisor.cpu.bus.t * 945 < self.game.cpu.bus.t * 176",
    ),
    "apu-quiet-run-without-its-start": (
        "apu.rs",
        "if mode5 == self.mode5 && first <= cycle && cycle < end {",
        "if mode5 == self.mode5 && cycle < end {",
    ),
}

ENV = {
    # The test ROMs: the reference checkout of a famicom-rust beside this one.
    "NES_TEST_ROMS": str(ROOT.parent / "famicom-rust" / "external" / "famicom-python" / "tests" / "test_roms"),
    "FAMICOM_NES20DB": str(EXT / "nes20db-2026-09-26.7z"),
    # The case tables' inputs (the binaries' default is beside their source).
    "INPUTS": str(ROOT / "scripts" / "inputs"),
    "PROBES": str(EXT / "probes"),
    # pally's palettes, as the ladder reads them (without them the Zapper judges
    # light on greys and the host gate's light-gun cases differ on any build:
    # every fault reaching them was "caught" by the harness until 2026-11-23).
    "FAMICOM_REFERENCE": str(EXT / "reference"),
    # The Vs. System's images (famicom-rust's, assembled by the reference's vs_images.py).
    "VS_IMAGES_DIR": str(ROOT.parent / "famicom-rust" / "external" / "vs-images"),
    "PC10_IMAGES_DIR": str(ROOT.parent / "famicom-rust" / "external" / "pc10-images"),
}
TABLES = [
    ("rung 2", ROOT / "scripts" / "cases.tsv", EXT / "refcache"),
    ("rung 2 probes", ROOT / "scripts" / "cases-probes.tsv", EXT / "refcache-probes"),
    # Early, being quick and the only table that runs the PlayChoice-10's code.
    ("rung 6 PlayChoice-10", ROOT / "scripts" / "cases-pc10.tsv", EXT / "refcache-pc10"),
    ("rung 5", EXT / "cases-rung5.tsv", EXT / "refcache-mappers"),
    ("rung 5 FDS", EXT / "cases-fds.tsv", EXT / "refcache-mappers"),
    ("rung 5 VRC7", EXT / "cases-lagrange.tsv", EXT / "refcache-mappers"),
    ("rung 6 machines", ROOT / "scripts" / "cases-machines.tsv", EXT / "refcache"),
    ("rung 5 AutoDisk", ROOT / "scripts" / "cases-fds-auto.tsv", EXT / "refcache-mappers"),
    ("rung 6 peripherals", ROOT / "scripts" / "cases-peripherals.tsv", EXT / "refcache-devices"),
    ("rung 6 host", ROOT / "scripts" / "cases-host.tsv", EXT / "refcache-host"),
    ("rung 6 Vs.", ROOT / "scripts" / "cases-vs.tsv", EXT / "refcache-vs"),
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
    if "FAMICOM_FDS_BIOS" not in env:
        if "NO_INTRO_DIR" not in env:
            raise SystemExit("set NO_INTRO_DIR (No-Intro's sets) or FAMICOM_FDS_BIOS")
        env["FAMICOM_FDS_BIOS"] = str(Path(env["NO_INTRO_DIR"]) / "Nintendo - Family Computer Disk System (FDS)" / "[BIOS] Family Computer Disk System (Japan) (En) (Rev 1).zip")
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
