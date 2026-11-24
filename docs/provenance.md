# Provenance (licence audit 2026-11-13)

A speed fork of famicom-rust (fork point `553ead2`), held trace-identical to it. It
**inherits famicom-rust's verdicts, and so famicom-python's** (famicom-rust's
`docs/provenance.md`): the same docstrings and open items. Since 2026-11-23 (merge of
famicom-rust `d40f26e`) those docstrings carry famicom-python's wave-2 rewording: the facts
once cited from Mesen2's source are DOC, OBSERVED closed-box (Mesen2 2.1.1 run as an
instrument) or MAME's BSD-3 drivers, and the MMC3 A12 filter rests on the test ROMs. The
speed changes add no outside source: the comments citing outside sources are the same set
as famicom-rust's.

The audit read this repository's comments, docs, `Cargo.toml`/`Cargo.lock` and git history.
No GPL or LGPL source was opened.

| Part | Source | Licence | Verdict |
| --- | --- | --- | --- |
| Crates (`Cargo.lock`) | crates.io, the same set as famicom-rust's | permissive (MIT, Apache-2.0, Zlib, Unlicense, 0BSD, Unicode-3.0) | clean |
| `ym2413-rust` | the restarted ym2413-rust, pinned by git at `94df223` | written from documents, by its own record | clean (board side) |
| `ay8910-rust` (the Sunsoft 5B, 2026-11-23) | pinned by git at `9b670a7`, a transcription of ay8910-python `a3bb485` | MIT | clean (audited with the cores) |
| The 5B cases' ROMs (`external/reference/.../nes-audio-tests/build`, gitignored) | rainwarrior's nes-audio-tests | "may be freely redistributed and modified for any purpose" | clean |
| Speed changes (`docs/not-kept.md` for what was tried) | our own work against famicom-rust's traces | | clean |
| The Vs. System, merged from famicom-rust `8eb496f` (2026-11-23): `vs.rs`, `dual.rs`, `cartridge/vs.rs`, the Vs. rows, the protection chips, `RGB_DAC` | famicom-rust's transcription of the reference's `b3dbf1b`: the NESdev wiki and MAME's BSD-3 `vsnes.cpp`/`ppu2c0x.cpp` facts | our MIT; tables and a value from BSD-3 files | **cited** (2026-11-23, merge of famicom-rust `f2e3840`): MAME's BSD-3 notice with the holders' names is in `NOTICE.md`, as in famicom-rust and the reference; `namco.rs`, `machine.rs`, `peripherals.rs` and `waixing.rs` say `MIT AND BSD-3-Clause` |
| The PlayChoice-10, merged from famicom-rust `f2e3840` (2026-11-23): `playchoice.rs`, `supervisor.rs`, the PlayChoice-10 row, `Board::idle_cycle`, the cabinet's states | famicom-rust's transcription of the reference's `c52a6d3`: Nintendo's PCH1-CPU schematic, the NESdev wiki, MAME's BSD-3 `playch10.cpp` and `rp5h01.cpp` | our MIT; tables from BSD-3 files | **cited**: `supervisor.rs` says `MIT AND BSD-3-Clause`, notice in `NOTICE.md` |
| `z80-rust` (the PlayChoice-10's supervisor) | our core, pinned by git at `7c61e5a` (z80-python `b59c80b` transcribed), as famicom-rust pins it | MIT | clean (audited with the cores) |
| `scripts/cases-pc10.tsv`, `scripts/inputs/pc10/` | famicom-rust's; the images assembled from `$MAME_ROMPATH` by the reference's `pc10_images.py`, outside the repository; the BIOS read from `$MAME_ROMPATH` at run time, never committed | | clean |
| `scripts/cases-vs.tsv`, `scripts/inputs/vs/` | famicom-rust's; the images assembled from `$MAME_ROMPATH` by the reference's `vs_images.py`, outside the repository | | clean |

## The VRC7 and its history (the repository is public)

- `3cf7a74` (from famicom-rust) brought the old ym2413-rust, pinned at `bdf6365`: withdrawn
  on 2026-11-12 as a translation of GPL code. Commits up to `9871b28` and `e718930` carry
  that pin; **play-nes's first deployment (`9871b28`) and famicom-libretro (`e718930`) were
  built against it** (see their provenance files).
- `3b1746f` switched the sound off (`vrc7-sound-off`, on GitHub; `origin/main` at `93ca1b6`
  includes it; `85b42b9`, which play-nes now pins, has no ym2413 dependency at all).
  `83b95b7` switched it back on from the restarted core (local `main`, unpushed).
- No commit holds Nuked-derived code; an earlier `OpllPlaceholder` was silent.

## Headers

Every code, test, script and CI file carries `SPDX-License-Identifier: MIT` (`7a0096f`;
the files added since carry it too).
