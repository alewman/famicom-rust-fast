# Provenance (licence audit 2026-11-13)

A speed fork of famicom-rust (fork point `553ead2`), held trace-identical to it. It
**inherits famicom-rust's verdicts, and so famicom-python's** (famicom-python's
`docs/provenance.md`, "Licence provenance (audit 2026-11-13)"): the same docstrings and
open items (Mesen2's `BandaiFcg.h` in `bandai.rs`, mapper 208's table "from FCEUX" in
`mmc3_clones.rs`, `vrc5.rs`'s page table, the MMC3 A12 filter). The speed changes add no
outside source: the comments citing outside sources are the same set as famicom-rust's.

The audit read this repository's comments, docs, `Cargo.toml`/`Cargo.lock` and git history.
No GPL or LGPL source was opened.

| Part | Source | Licence | Verdict |
| --- | --- | --- | --- |
| Crates (`Cargo.lock`) | crates.io, the same set as famicom-rust's | permissive (MIT, Apache-2.0, Zlib, Unlicense, 0BSD, Unicode-3.0) | clean |
| `ym2413-rust` | `main`: a path to the restarted ym2413-rust (2026-11-13) | written from documents, by its own record | clean (board side); pin by git when it has a remote |
| Speed changes (`docs/not-kept.md` for what was tried) | our own work against famicom-rust's traces | | clean |

## The VRC7 and its history (the repository is public)

- `3cf7a74` (from famicom-rust) brought the old ym2413-rust, pinned at `bdf6365`: withdrawn
  on 2026-11-12 as a translation of GPL code. Commits up to `9871b28` and `e718930` carry
  that pin; **play-nes's first deployment (`9871b28`) and famicom-libretro (`e718930`) were
  built against it** (see their provenance files).
- `3b1746f` switched the sound off (`vrc7-sound-off`, on GitHub; `origin/main` at `93ca1b6`
  includes it; `85b42b9`, which play-nes now pins, has no ym2413 dependency at all).
  `83b95b7` switched it back on from the restarted core (local `main`, unpushed).
- No commit holds Nuked-derived code; an earlier `OpllPlaceholder` was silent.

## Not done here

No SPDX headers yet (0 of 88 source files): owed, as across the family.
