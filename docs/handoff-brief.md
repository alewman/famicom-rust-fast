# Handoff brief: famicom-rust-fast

## The request

Aubrey, 2026-11-08: the vertical slice is famicom-rust, then a rust-fast
version, then a libretro core: "the first real test of the distro plan, one
family emulator running at full speed inside RetroArch on an ARM handheld".
famicom-rust is done (every rung of its ladder, save states included); this
is the fast version. famicom-libretro (`/data/emu/famicom-libretro`) wraps
famicom-rust today and will switch to this crate (the library keeps the name
`famicom_rust`, so the switch is one dependency line).

## What it is

A fork of famicom-rust at `553ead2` (remote `upstream` is
/data/emu/famicom-rust), held in lockstep with it: every change must leave
this board's traces identical to famicom-rust's, which are identical to
famicom-python's at `famicom-rust-pin-1`, on every case, and must measure
faster, or it is not kept. The family's pattern is a2600-rust-fast
(`/data/emu/a2600-rust-fast`: read its README, `scripts/ladder.sh`,
`scripts/ab.py`, `src/bin/a2600-bench.rs`, the `profile` feature and
`scripts/profile.py`, `docs/not-kept.md`) and megadrive-rust-fast.

Baseline: about 8x real time on this loaded x86 machine (1,200 frames of
Super Mario Bros. 3 in about 2.5 s). An ARM handheld core is several times
slower, so the target is headroom there: as fast as the board can be made
while staying identical. Speed comes from the board (the PPU's dot loop is
the likely bulk; the APU per CPU cycle, the bus decode, the DMA paths and
the cartridge connector next), not from approximations.

## The gate (build it first)

`scripts/ladder.sh`, which needs no Python: everything the reference ran is
cached in `external/` (copied from famicom-rust; gitignored):

- `cargo test` (incl. tests/mapper_coverage.rs: all 294 mappers build);
- rung 1, nestest (needs the test ROMs: `NES_TEST_ROMS`, default
  `/data/emu/famicom-rust/external/famicom-python/tests/test_roms`);
- the lockstep cases against the cached reference traces, Rust side only
  (`scripts/lockstep.py --reference-cache ...`): `scripts/cases.tsv` (rung 2,
  refcache), `external/cases-rung5.tsv` (every board class, refcache-mappers),
  `external/cases-fds.tsv` and `external/cases-lagrange.tsv`
  (refcache-mappers; the FDS cases need `FAMICOM_FDS_BIOS="/data/emu/roms/fds/[BIOS]
  Family Computer Disk System (Japan) (En) (Rev 1).zip"`),
  `scripts/cases-machines.tsv` (refcache), `scripts/cases-peripherals.tsv`
  (refcache-devices). A cache file is `<case>.<slices>.ref` for the mapper
  cache and `<case>.ref` otherwise: check lockstep.py's naming and make the
  ladder hit every cache file without running Python (fail loudly if a case
  has no cached reference);
- rung 3 and 4 on the Rust side: `famicom-testrom external/testrom-reference.jsonl`
  compared by `scripts/testrom_compare.py`, and `famicom-accuracycoin` (147/147,
  126,984,584 cycles, 4,264 frames);
- save states: `famicom-state-roundtrip` on a dump list (one per board class;
  `/tmp/claude-1000/-data-emu/95574a8d-82b1-46e8-9061-53d407206449/scratchpad/state-dumps.txt`
  — copy it into `external/`).

Seeded faults (as a2600-rust-fast's): a few one-line faults in what you
change, each caught by the ladder.

## Measuring

The machine is shared (load often 20-50) and `perf` is not permitted
(perf_event_paranoid). As a2600-rust-fast: a bench binary timing the
thread's own CPU time (`/proc/thread-self/schedstat`) over a fixed set of
games (NROM, MMC1, MMC3, MMC5, a VRC game, FDS optional), `scripts/ab.py`
alternating two builds, best of N; a SIGPROF sampling profiler behind a
`profile` feature with addr2line. Keep a build of the fork point to measure
the total against directly.

## Working rules

- Every kept change: ladder green, A/B measured, one commit each, its number
  in the README's table; tried-and-not-kept ones go in `docs/not-kept.md`.
- Do not change famicom-rust or famicom-python. If you find a bug in the
  base, stop and report it with the case.
- No unsafe unless it pays measurably and is justified in its comment.
- Keep `state.rs` working: save states must still round-trip (fields you add
  as caches need `#[serde(skip)]` and rebuilding on load, or must be saved).
- Commits: `git -c user.name=alewman -c user.email=alewman@gmail.com commit`,
  trailers `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: 95574a8d-82b1-46e8-9061-53d407206449`. Local only: no
  GitHub repo, never push. At most 4 processes of your own (`nice`). No
  `sed -i`, `rm -rf` or `ln -sfn` one-liners.
