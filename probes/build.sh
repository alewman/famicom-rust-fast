#!/usr/bin/env bash
# Assemble the probes (our own test ROMs, probes/*.s) into external/probes
# with cc65's ca65 and ld65 (CC65_DIR, default ~/.local/opt/cc65-2.19).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${PROBES:-$here/../external/probes}
bin=${CC65_DIR:-$HOME/.local/opt/cc65-2.19}/bin
mkdir -p "$out"
for source in "$here"/*.s; do
    name=$(basename "$source" .s)
    "$bin/ca65" -o "$out/$name.o" "$source"
    "$bin/ld65" -C "$here/nrom.cfg" -o "$out/${name//_/-}.nes" "$out/$name.o"
done
