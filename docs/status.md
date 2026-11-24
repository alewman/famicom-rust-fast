<!-- SPDX-License-Identifier: MIT -->
# Status

The optimisation tier of the NES line: famicom-python → famicom-rust →
**famicom-rust-fast**. Every change leaves this board's traces identical to
famicom-rust's (whose equal famicom-python `c52a6d3`'s on the gate's cases) and
measures no slower on any code layout. The README holds the rule, the gate's
description, the speed record and the seeded faults; this page the latest counts.

## The gate: `scripts/ladder.sh`

Measured on 2026-11-24 on the tree committed as `34ea83a` (the PlayChoice-10's
merge), `JOBS=6`, 2,750 s on a shared, heavily loaded machine.

| Step | Count |
| --- | --- |
| `cargo test --release` | all pass; `tests/playchoice.rs` 26, `tests/vs_system.rs` 32 |
| Rung 1, nestest | 8,991 lines, 0 differ |
| Rung 2, lockstep | 200 of 200; the probes 2 of 2 |
| Rung 5, lockstep | 581 of 581; the FDS 6; Lagrange Point 1; the 5B 9; 559 2; AutoDisk 3 |
| Rung 6, lockstep | machine rows 33; peripherals 54; host input 36; the Vs. System 66; the PlayChoice-10 60 (of as many) |
| Rung 3, test ROM tables | 197 of 197 agree |
| Rung 4, AccuracyCoin | 147 of 147 on the reference's 126,984,584 CPU cycles and 4,264 frames |
| Save states | 363 of 363; the 5B and 559, 5 of 5; Vs. UniSystem 43 of 43; PlayChoice-10 cabinets 56 of 56 |
| CI's checks | `cargo clippy --release --all-targets -- -D warnings` and `cargo fmt --check` clean with the stable toolchain (rustc 1.99.1) |

## Speed of the merge

`scripts/ab.sh eefb681` (the tree before the merge against `34ea83a`), 12 rounds of
300 frames on the five bench games, best of each side, one pinned core:

| Layout | The plain carry | Kept: `Apu::clock` inlined, `idle_cycle` cold |
| --- | ---: | ---: |
| default | x1.002 | x1.006 |
| 32-byte blocks (`nf5`) | x0.989; again x0.985 (A/A x1.003) | x0.995 |
| 64-byte blocks (`nf6`) | x0.998 | x0.996 |

Logs: `external/ab-pc10.log`, `ab-pc10-nf5.log`, `ab-pc10-inline.log`.

## Seeded faults (the PlayChoice-10's)

`scripts/seeded_faults.py --only pc10-...` (fresh timestamps, a target directory per
fault), 2026-11-24 on `34ea83a`'s sources: 6 caught by the PlayChoice-10 table, 2
missed by every lockstep table: `pc10-watchdog-on-the-seventh-blank` (caught by
`cargo test`, `tests/playchoice.rs`) and `pc10-latch-sees-sdcs-late` (caught by
nothing: a blind spot, owed a watchdog-reset case in the reference). The README,
"Seeded faults", has the detail.
