#!/usr/bin/env python3
"""The lockstep gate's reference side: famicom-python, traced frame by frame.

Run on the reference's interpreter (external/famicom-python/.venv-pypy). Each
slice is one ``System.run_frames(1)``; its line holds the CPU registers at the
slice's end, the cycle and instruction counts, a CRC-32 of every CPU-bus cycle
of the slice as ``board.bus_observers`` sees it (packed ``t << 25 | write <<
24 | address << 8 | value``, little-endian u64), a CRC of the picture (the
9-bit values, little-endian u16), the PPU's v, t and status, and a CRC of the
slice's audio (``take_samples``, little-endian f64). ``target/release/famicom-trace``
writes the same lines; ``scripts/lockstep.py`` compares them.

    famicom_trace.py ROM --slices N [--machine KEY] [--input FILE] [--device N]
                     [--reset FRAME ...] [--detail FRAME] [--auto-disk] --out FILE

``--input``: lines ``FRAME PAD BUTTONS`` (hex), applied before that slice to a
standard pad, and lines ``FRAME set PATH FIELD [VALUE ...]`` that drive the
device ``--device`` plugged (see ``drive``).
``--device N``: ``System.plug(N)`` before the first slice (NES 2.0 expansion
number, decimal or 0x hex); ``auto``, the cartridge's own (``System.plug()``);
``hori-adapter``, a ``peripherals.HoriAdapter`` on the expansion port (no number
plugs it). With R.O.B., each line ends with ``rob HEIGHT TURN ARMS LED COMMANDS``
after ``see(ppu.picture)``; every other line is unchanged.
``--auto-disk``: the FDS's disks change as the program asks (``fds_auto.AutoDisk``,
observing after the tracer, as the Rust board calls it after its log), each
change written to stderr with its slice.
``--detail FRAME``: also write every bus cycle of that slice, one per line.
"""

from __future__ import annotations

import argparse
import struct
import sys
import zlib
from array import array

from famicom_python import peripherals
from famicom_python.cartridge import read_rom_file
from famicom_python.machine import MACHINES
from famicom_python.system import System


def drive(plugged: object, path: str, field: str, values: list[str]) -> None:
    """``set PATH FIELD VALUE...``: PATH walks from what plug returned (``-``: itself;
    dot-separated attribute names and list indices); FIELD is a method (called with
    the values as ints: ``move``, ``play``, ``record``, ``stop``) or an attribute:
    ``aim`` takes ``X Y`` or ``none``, a bool ``0``/``1``, an int a number, a set
    the names (none: empty)."""
    target = plugged
    if path != "-":
        for token in path.split("."):
            target = target[int(token)] if token.isdigit() else getattr(target, token)
    current = getattr(target, field)
    if callable(current):
        current(*(int(v, 0) for v in values))
        return
    if field == "aim":
        value = None if values[0] == "none" else (int(values[0], 0), int(values[1], 0))
    elif isinstance(current, bool):
        value = values[0] in ("1", "true")
    elif isinstance(current, int):
        value = int(values[0], 0)
    elif isinstance(current, set):
        value = set(values)
    else:
        raise ValueError(f"cannot set {field}")
    setattr(target, field, value)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom")
    parser.add_argument("--slices", type=int, required=True)
    parser.add_argument("--machine", default="nes")
    parser.add_argument("--input")
    parser.add_argument("--device")
    parser.add_argument("--reset", type=int, action="append", default=[])
    parser.add_argument("--detail", type=int)
    parser.add_argument("--auto-disk", action="store_true")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    inputs: dict[int, list[tuple[int, int]]] = {}
    sets: dict[int, list[list[str]]] = {}
    if args.input:
        with open(args.input) as handle:
            for text in handle:
                parts = text.split()
                if text.startswith("#"):
                    continue
                if len(parts) == 3:
                    inputs.setdefault(int(parts[0]), []).append((int(parts[1]), int(parts[2], 16)))
                elif len(parts) >= 4 and parts[1] == "set":
                    sets.setdefault(int(parts[0]), []).append(parts[2:])

    system = System.from_rom(read_rom_file(args.rom), MACHINES[args.machine])
    board, cpu, ppu = system.board, system.cpu, system.board.ppu
    plugged = None
    if args.device == "auto":
        plugged = system.plug()
    elif args.device == "hori-adapter":
        plugged = board.expansion = peripherals.HoriAdapter()
    elif args.device is not None:
        plugged = system.plug(int(args.device, 0))
    rob = plugged if isinstance(plugged, peripherals.Rob) else None
    log = array("Q")
    append = log.append

    def observe(kind: str, address: int, value: int, t: int) -> None:
        append(t << 25 | (kind == "write") << 24 | address << 8 | value)

    board.bus_observers.append(observe)
    slice_now = [0]
    if args.auto_disk:
        from famicom_python.fds_auto import AutoDisk

        AutoDisk(system, lambda message: print(f"slice {slice_now[0]}: {message}", file=sys.stderr))
    with open(args.out, "w") as out:
        for frame in range(args.slices):
            for pad, buttons in inputs.get(frame, ()):
                board.pads[pad].buttons = buttons
            for path, field, *values in sets.get(frame, ()):
                drive(plugged, path, field, values)
            if frame in args.reset:
                system.reset()
            del log[:]
            slice_now[0] = frame
            system.run_frames(1)
            if sys.byteorder != "little":
                log.byteswap()
            bus = zlib.crc32(log.tobytes())
            picture = array("H", ppu.picture)
            if sys.byteorder != "little":
                picture.byteswap()
            pic = zlib.crc32(picture.tobytes())
            samples = system.take_samples()
            snd = zlib.crc32(struct.pack(f"<{len(samples)}d", *samples))
            out.write(
                f"{frame} cyc {board.cycles} ins {system.instructions} "
                f"pc {cpu.pc:04X} a {cpu.a:02X} x {cpu.x:02X} y {cpu.y:02X} "
                f"s {cpu.s:02X} p {cpu.p:02X} bus {bus:08X} {len(log)} "
                f"pic {pic:08X} v {ppu.v:04X} t {ppu.t:04X} st {ppu.status:02X} "
                f"snd {snd:08X} {len(samples)}"
            )
            if rob is not None:
                rob.see(ppu.picture)
                out.write(
                    f" rob {rob.height} {rob.turn} {int(rob.arms_closed)} {int(rob.led)}"
                    f" {len(rob.commands)}"
                )
            out.write("\n")
            if frame == args.detail:
                for packed in log:
                    t, rest = packed >> 25, packed & 0x1FFFFFF
                    kind = "W" if rest >> 24 else "R"
                    out.write(f"  {t} {kind} {(rest >> 8) & 0xFFFF:04X} {rest & 0xFF:02X}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
