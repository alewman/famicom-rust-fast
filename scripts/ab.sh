#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# A/B a base revision against the working tree under several code layouts.
#
# Two builds of the same source differ here by a few per cent from where the linker
# puts the hot blocks alone (methodology, "Measure across code layouts"), so each side is
# built under each layout (RUSTFLAGS) and timed with scripts/ab.py (alternating runs on
# one pinned CPU, best of ROUNDS); a change counts only when it wins, or for a carry
# that should cost nothing, does not lose, on every layout.
#
#   scripts/ab.sh [BASE_REV]      (default HEAD)
#   env: ROUNDS (default 8), FRAMES (300), CPU (31), LAYOUTS ("default nf5 nf6")
#
# The base side is a worktree at BASE_REV (target/ab-base), built with its own
# famicom-bench; ab.py refuses to compare builds whose checksums differ.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BASE=${1:-HEAD}
ROUNDS=${ROUNDS:-8}
FRAMES=${FRAMES:-300}
CPU=${CPU:-31}
LAYOUTS=${LAYOUTS:-"default nf5 nf6"}
declare -A FLAGS=(
    [default]=""
    [nf5]="-C llvm-args=-align-all-nofallthru-blocks=5"
    [nf6]="-C llvm-args=-align-all-nofallthru-blocks=6"
)
rev=$(git -C "$ROOT" rev-parse --short "$BASE")
B="$ROOT/target/ab-base"
[ -d "$B" ] || git -C "$ROOT" worktree add -q --detach "$B" "$rev"
git -C "$B" checkout -q -f --detach "$rev"
build() { # tree layout -> binary
    local tree=$1 layout=$2 target="$1/target/ab-$2"
    RUSTFLAGS="${FLAGS[$layout]}" CARGO_TARGET_DIR="$target" \
        nice cargo build -j 4 --release -q --manifest-path "$tree/Cargo.toml" --bin famicom-bench >&2
    echo "$target/release/famicom-bench"
}
for layout in $LAYOUTS; do
    a=$(build "$B" "$layout")
    b=$(build "$ROOT" "$layout")
    echo "== $layout: $rev against the working tree"
    python3 "$ROOT/scripts/ab.py" "$a" "$b" --rounds "$ROUNDS" --frames "$FRAMES" --cpu "$CPU"
done
