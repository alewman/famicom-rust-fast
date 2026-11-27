<!-- SPDX-License-Identifier: MIT -->
# Status

The optimisation tier of the NES line: famicom-python → famicom-rust →
**famicom-rust-fast**. Every change leaves this board's traces identical to
famicom-rust's (whose equal famicom-python `b5dbd2f`'s on the gate's cases; its
`src/` is `c52a6d3`'s) and measures no slower on any code layout. The README holds
the rule, the gate's description, the speed record and the seeded faults; this page
the latest counts.

## The gate: `scripts/ladder.sh`

Measured on 2026-11-27 on the tree committed as the merge of famicom-rust `cef790d`
(branch `pc10-watchdog-states`: PlayChoice-10 save states taken while the watchdog
counts), `JOBS=6`, 1,436 s on a shared machine at load about 40
(`external/ladder-wd-states.log`). The tree under `src/` differs from `ced0fab`'s only
in the binary `famicom-pc10-states`.

| Step | Count |
| --- | --- |
| `cargo test --release` | all pass; `tests/playchoice.rs` 28, `tests/vs_system.rs` 32 |
| Rung 1, nestest | 8,991 lines, 0 differ |
| Rung 2, lockstep | 200 of 200; the probes 2 of 2 |
| Rung 5, lockstep | 581 of 581; the FDS 6; Lagrange Point 1; the 5B 9; 559 2; AutoDisk 3 |
| Rung 6, lockstep | machine rows 33; peripherals 54; host input 36; the Vs. System 66; the PlayChoice-10 61 (of as many; the watchdog case among them) |
| Rung 3, test ROM tables | 197 of 197 agree |
| Rung 4, AccuracyCoin | 147 of 147 on the reference's 126,984,584 CPU cycles and 4,264 frames |
| Save states | 363 of 363; the 5B and 559, 5 of 5; Vs. UniSystem 43 of 43; PlayChoice-10 cabinets 56 of 56; saved while the watchdog counts (our supervisor program, *Super Mario Bros.*, `--at` 40, 43, 49, 52) 4 of 4 |
| CI's checks | `cargo clippy --release --all-targets -- -D warnings` and `cargo fmt --check` clean with the stable toolchain (rustc 1.99.1) |

## Speed

`ced0fab` and the merge of `cef790d` change nothing on the cycle path (the tracers'
`--bios-program`, a unit test and the gate's scripts), so no A/B was run; the last one is the PlayChoice-10's merge:
`scripts/ab.sh eefb681` against `34ea83a`, 12 rounds of 300 frames on the five bench
games, best of each side, one pinned core:

| Layout | The plain carry | Kept: `Apu::clock` inlined, `idle_cycle` cold |
| --- | ---: | ---: |
| default | x1.002 | x1.006 |
| 32-byte blocks (`nf5`) | x0.989; again x0.985 (A/A x1.003) | x0.995 |
| 64-byte blocks (`nf6`) | x0.998 | x0.996 |

Logs: `external/ab-pc10.log`, `ab-pc10-nf5.log`, `ab-pc10-inline.log`.

## Seeded faults

`scripts/seeded_faults.py` (fresh timestamps, a target directory per fault), all 18,
2026-11-24:

| Set | Tree | Result | Log |
| --- | --- | --- | --- |
| The fork's six, the mask test, `apu-quiet-run-without-its-start` | `ce18b7e` | 8 caught: rung 2 (7), the RP2C03B probe (1) | `external/faults-rerun-ce18b7e.log` |
| The Vs. System's three | `ce18b7e` | 3 caught, the Vs. table (*Vs. Ice Climber*) | the same |
| The PlayChoice-10's eight | `ced0fab` | 8 caught, the PlayChoice-10 table; the watchdog's count and the latch's SDCS rule by `pc10-watchdog-Super_Mario_Bros` | `external/faults-pc10-ced0fab.log` |

The ten re-run on `ce18b7e` were first judged before the script's fresh-build fix;
their verdicts are unchanged. Before `ced0fab` two PlayChoice-10 faults escaped every
lockstep table (the watchdog on the seventh blank, caught only by `cargo test`; the
latch's SDCS rule, by nothing); none does now. The README, "Seeded faults", has the
detail.
