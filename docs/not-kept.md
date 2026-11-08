# Tried and not kept

Each with its A/B against the build before it (scripts/ab.py, best of 8 or
12 alternating runs a game, five games; an A/A run is within 1% overall and
2% a game).

- **Fat LTO, one codegen unit** (`lto = "fat"`, `codegen-units = 1`):
  x0.974, slower on every game (x0.949 to x0.992). Not kept.
