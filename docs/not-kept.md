# Tried and not kept

Each with its A/B against the build before it (scripts/ab.py, best of 8 or
12 alternating runs a game, five games; an A/A run is within 1% overall and
2% a game).

- **Fat LTO, one codegen unit** (`lto = "fat"`, `codegen-units = 1`):
  x0.974, slower on every game (x0.949 to x0.992). Not kept.
- **The fast dot also for the sprite fetches (258-320) and the next
  line's first two tiles (321-336)** on a visible line: x0.999. Those dots
  are a quarter of a line's but cheap already. Not kept.
