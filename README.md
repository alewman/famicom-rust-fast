# famicom-rust-fast

A speed fork of [famicom-rust](https://github.com/alewman/famicom-rust), the
Rust Famicom / NES board, held in lockstep with it: every change must leave
this board's traces identical to famicom-rust's (which are identical to
famicom-python's at `famicom-rust-pin-1`) on every case, and must measure
faster, or it is not kept. The fork point is famicom-rust `553ead2`; the
library keeps the name `famicom_rust`, so famicom-libretro switches to it by
one dependency line.

```text
cargo build --release
target/release/famicom ROM.zip --frames 600 --png last.png --wav sound.wav
```

famicom-rust's README holds the board's description and certification (the
reference, its pins, rungs 1-6 and the save states); this one, the fork's
rule, gate and speed.

## Speed

**x1.133** over the fork point, measured directly (`scripts/ab.py`, 12
alternating rounds of 600 frames a game): on one core of this loaded shared
machine, 463.7 frames a second against 403.0 for Super Mario Bros. (NROM),
464.8 against 404.1 for The Legend of Zelda (MMC1), 424.1 against 370.9 for
Super Mario Bros. 3 (MMC3), 369.3 against 328.8 for Castlevania III (MMC5)
and 387.7 against 352.6 for Akumajou Densetsu (VRC6, its sound on the
Famicom): x1.100 to x1.151 a game, 6.1 to 7.7 times real time (60.10 Hz)
where the fork point ran 5.5 to 6.7.

| Kept | A/B against the build before it |
| --- | ---: |
| The APU's frame counter: no scan of its step table inside a run of cycles known to hold no step (`#[serde(skip)]`, found again after a load) | x1.032 |
| The PPU's pixel dot (a visible line's dots 2-256, rendering on and settled) straight from `step`, without `dot_work`'s and `render`'s tests for the other dots | x1.067 |
| `run_to` takes that pixel dot itself when no register effect is due on it | x1.030 |

The three multiply to x1.134; measured directly, x1.133. Their A/Bs were
taken with a bench that spent about a tenth of its run on its own checksum
(fixed in `famicom-bench` since), so each understates a little.
Tried and not kept, with their numbers: [docs/not-kept.md](docs/not-kept.md).

**Where the time goes.** At the fork point (five games, 9,008 samples):
`dot_work` and `render` held the PPU's dot (35% and 23% inclusive), the APU's
cycle 15%, the background fetches 11%, the pixel 10%. Now (7,125 samples),
by self time: `run_to` with the pixel dot inside it 8.7%, the pixel 8.6%,
`Apu::clock` 8.3%, sprite evaluation 6.9%, the background fetch 4.7%, the
CPU bus (`Board::read`, `read_byte`, `begin_cycle`, `end_cycle`) 12% together,
and no source line above 2.3%. Ablations (not identical, only to see where the
time is) on Super Mario Bros. 3 at 434 fps: without the background fetches
575, without the APU 517, without sprite evaluation 503, without the pixel
495. The rest is the work itself; the experiments in docs/not-kept.md show
its branches and calls already cost little. The next step up would be
structural (a PPU that catches up only when the CPU side can see it, or a
tile's eight dots at once), not another local change.

## The rule and the gate

`scripts/ladder.sh` (about 10 minutes here, four boards at a time under
`nice`), no Python: everything the reference ran is cached in `external/`
(copied from famicom-rust; gitignored), and the ladder fails on a case
without a cached trace.

| Step | What | Count |
| --- | --- | --- |
| `cargo test` | incl. `tests/mapper_coverage.rs`: all 294 mappers build | |
| rung 1 | nestest against nestest.log | 8,991 lines |
| rung 2 | lockstep, every NROM test ROM (600 slices) and AccuracyCoin's run-all (3,000), `scripts/cases.tsv` | 200 cases |
| rung 5 | lockstep, every board class on dumps (`external/cases-rung5.tsv`), the FDS on three disks (`cases-fds.tsv`), Lagrange Point's VRC7 FM (`cases-lagrange.tsv`, 3,000 slices) | 581 + 6 + 1 |
| rung 6 | lockstep, the machine rows (`scripts/cases-machines.tsv`) and the peripherals (`scripts/cases-peripherals.tsv`) | 25 + 54 |
| rung 3 | the reference's test ROM tables, outcome for outcome (`famicom-testrom`, `famicom-testrom-compare`) | 197 |
| rung 4 | AccuracyCoin: 147 of 147 on the reference's 126,984,584 CPU cycles and 4,264 frames | 147 |
| save states | `famicom-state-roundtrip`, one dump of every board class (`external/state-dumps.txt`) | 363 |

`famicom-lockstep` is `scripts/lockstep.py --reference-cache` without the
reference: it runs `famicom-trace` on each case and compares the trace with
`<cache>/<name>.<slices>.ref`. The ladder needs `NES_TEST_ROMS` (default
famicom-rust's reference checkout's test ROMs), `FAMICOM_FDS_BIOS` and
nes20db at `external/nes20db-2026-09-26.7z` (`FAMICOM_NES20DB`; without it
16 cases identify their boards differently and differ), and pally's palettes
under `external/reference/reference/nesdev-wiki/images` (`FAMICOM_REFERENCE`,
copied from the reference's checkout; without them the Zapper judges light on
a grey palette and three light-gun cases of the host gate differ, while the
peripheral cases happened to pass: found 2026-11-08). The ladder refuses to
run without either.

**Seeded faults** (`scripts/seeded_faults.py`): one-line faults in the fork's
own code, each built apart and run up the ladder until a rung fails. Six of
seven are caught (the frame counter's quiet run over the next step and from
before its start; the pixel dot while waking, while dark, without dot 256's
fine-Y increment, and taken by `run_to` over a due event), all by rung 2. One
is **missed by the whole ladder**: the pixel dot evaluating sprites in the one
dot between a $2001 write of rendering off and rendering's turning off. No
case turns rendering off mid-evaluation and then shows that one step's effect
(OAMADDR, the corruption's row); it is a gap in famicom-rust's gate as well,
noted here rather than taken as equivalent. Two other candidates were
equivalent and are left out (the pixel dot's fading and OAM-corruption tests
are guards: `fading` is only set with rendering off, and a corruption is
spent before any visible pixel dot).

## Measuring

The machine is shared (load 15 to 50) and `perf` is not permitted.

- `famicom-bench ROM... [--frames N]` runs each game on the console it asks
  for, with the lockstep gate's start-mash input, timing the thread's own CPU
  time (`/proc/thread-self/schedstat`), and prints frames a second and a CRC
  of every picture, sample and the cycle count.
- `scripts/ab.py A B [--rounds N] [--frames N]` alternates two bench builds
  game by game on one pinned core and compares each side's best run (other
  work only ever slows a run down); the checksums must agree. An A/A run is
  within 1% overall and 2% a game.
- `scripts/try.sh NAME [BASE]`: one experiment, built in `target/exp` (so a
  ladder can keep its binaries), rung 2 as a quick check, then the A/B
  against `target/ab/BASE`.
- The fork point's bench is kept as `target/fork-point/famicom-bench2`
  (`git archive 4169496` with today's `famicom-bench.rs`, built in
  `target/fork-point`), for the total measured directly.
- `cargo build --release --features profile --bin famicom-profile`, then
  `famicom-profile ROM... --frames N --out external/profile.txt` samples the
  program counter on SIGPROF and `scripts/profile.py` folds the samples into
  functions and lines with `addr2line`, inlined frames included.

## Licence

MIT, as the reference.
