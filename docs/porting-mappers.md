# Porting the reference's mappers

How a mapper module of famicom-python (`src/famicom_python/cartridge/*.py` at
the tag `famicom-rust-pin-1`) becomes a module of this crate, and how it is
judged. The reference is the spec: transcribe it, never improve it.

## Where things are

- The reference checkout: `external/famicom-python` (in a worktree, use the
  main tree's: `<famicom-rust checkout>/external/famicom-python`). Set
  `FAMICOM_REFERENCE` to it in a worktree; `scripts/lockstep.py`, the
  case scripts and the Rust loader's nes20db lookup all read it.
- One Rust module per Python module: `src/cartridge/<module>.rs`. Its
  `build(mapper, image, t)` returns the board for each mapper number the
  reference's `MAPPERS` gives a class of that module (the stub's header lists
  them), and hands the image back (`Err(image)`) for any other number.
  `cartridge::build` tries every module's `build`.
- Templates, in order of difficulty: `discrete.rs` (classes over NROM),
  `mmc1.rs` (subclasses folded into one struct and a `Kind`), `mmc3.rs` (a
  deep hierarchy whose base calls overridable helpers), `eeprom.rs` and
  `flash.rs` (devices a mapper holds).

## The rules of the transcription

- **The connector** (`connector.rs` notes): Python class attributes
  (`counts_m2 = True`, `watches_ppu_address`, `snoops_cpu_reads`,
  `latches_reads`, `expansion_audio`) are trait methods. `self.irq` is a
  field returned by `fn irq()`. `self.audio_out.expansion = level` sets the
  cartridge's own `expansion: f64` field, returned by `fn expansion()`. A
  CPU read that leaves lines undriven returns `driven | mask << 8` (u16).
- **Inheritance.** A mapper holds a `Banked` (or its parent's struct) and
  returns it from `banked`/`banked_mut` (`banked_field!`); every connector
  method's default does what `Banked` does, so override only what the Python
  class overrides, and call `self.b.base_*` (or the parent's function) where
  the Python calls `super()`. **Trace every `self.method()` call a base class
  makes**: if any subclass overrides `method` (`map_prg`, `map_chr`,
  `set_mirroring`, `_update`, `_restore`, ...), the base's call must reach the
  override, including from the base's `__init__` (where the subclass's
  fields set before `super().__init__()` are already there and later ones
  are not). The MMC3 pattern (a trait whose default methods are free
  `base_*` functions) is the general answer; an enum is fine for a closed
  family.
- **Python's integers.** Banks may go negative (`len // n - 1` on a small
  image): `Banked::map_prg`/`map_chr` take `i64` and use Python's modulo.
  Use `div_euclid`/`rem_euclid` wherever Python's `//` or `%` can see a
  negative operand, and remember `~x` and `&` on negative ints. Python ints
  do not overflow: widen, or mask exactly where Python masks.
- **Floats** (sound chips): the same operations in the same order (`a + b +
  c` is `(a + b) + c`), `x / n` on ints is a float division, `round` is
  `round_ties_even`, `int()` truncates. The gate compares the samples' bits.
- **Truthiness**: `x or y` returns a value; `if self.prg_ram:` is "not
  empty"; `None` is not 0.
- The source comments: keep each class's one-line summary and the citation
  (wiki page, judge); the reference holds the full reasoning, so do not copy
  long docstrings.

## Judging

Every class needs at least one dump that runs identically under the lockstep
gate. The index `external/mapper-dumps.tsv` (scripts/mapper_dumps.py) gives,
for every dump in No-Intro's NES sets (`$NO_INTRO_DIR/Nintendo - Nintendo Entertainment System (Headered)` and its
(Aftermarket) half), the board the reference builds:

    export FAMICOM_REFERENCE=<famicom-rust checkout>/external/famicom-python
    python3 scripts/make_mapper_cases.py --index <famicom-rust checkout>/external/mapper-dumps.tsv \
        --mappers 21,22,23 --per 2 --slices 600 --out /tmp/cases-vrc.tsv
    python3 scripts/lockstep.py --cases /tmp/cases-vrc.tsv --jobs 2 \
        --reference-cache <famicom-rust checkout>/external/refcache-mappers

A difference reports the first differing slice; `famicom-trace ... --detail
N` and `famicom_trace.py ... --detail N` print every bus cycle of slice N on
both sides. A class with no dump in the index is reported as such.

## House rules

- Never commit ROMs, traces or BIOS; never write into the ROM folders.
- At most 16 processes in all: `--jobs 2`. Kill by PID, never `pkill`. No
  `sed -i`, `rm -rf` or `ln -sfn` one-liners.
- Commits: `git -c user.name=alewman -c user.email=alewman@gmail.com
  commit`, ending with the model's `Co-Authored-By` trailer and a
  `Claude-Session` trailer. Never push.
