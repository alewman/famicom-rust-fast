# Tried and not kept

Each with its A/B against the build before it (scripts/ab.py, best of 8 or
12 alternating runs a game, five games; an A/A run is within 1% overall and
2% a game).

- **Fat LTO, one codegen unit** (`lto = "fat"`, `codegen-units = 1`):
  x0.974, slower on every game (x0.949 to x0.992). Not kept.
- **The fast dot also for the sprite fetches (258-320) and the next
  line's first two tiles (321-336)** on a visible line: x0.999. Those dots
  are a quarter of a line's but cheap already. Not kept.
- **`pixel` inlined into the pixel dot** (`#[inline(always)]`): x0.986.
  Not kept.
- **Secondary OAM's clearing dots (1-64) in the pixel dot**, `evaluate`
  called only from dot 65: x0.999. The call costs nothing measurable.
  Not kept.
- **The APU's four rare countdowns behind one test** (the $4017 IRQ
  clear, the enable hold, the DMC's ending and load countdown, split into
  their own function): x0.998. Predicted branches cost nothing. Not kept.
- **ALEs to an MMC3 sent only when A12 changes** (a trait hint for the
  carts on `mmc3::base_ppu_address`, which does nothing unless A12 differs
  from the last ALE's): x0.995 overall, x1.006 on Super Mario Bros. 3. The
  connector's dynamic calls are cheap. Not kept.

Ablations (not identical, only to see where the time is; Super Mario Bros.
3 at 434 fps after the first three kept changes): without the background
fetches 575 fps, without the APU 517, without sprite evaluation 503,
without the pixel 495. The time is the work itself, spread over the dot;
the five experiments above show its branches and calls are already cheap.
