#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The lockstep gate's reference side: famicom-python, traced frame by frame.

Run on the reference's interpreter (external/famicom-python/.venv-pypy). Each
slice is one ``System.run_frames(1)``; its line holds the CPU registers at the
slice's end, the cycle and instruction counts, a CRC-32 of every CPU-bus cycle
of the slice as ``board.bus_observers`` sees it (packed ``t << 25 | write <<
24 | address << 8 | value``, little-endian u64), a CRC of the picture (the
9-bit values, little-endian u16), the PPU's v, t and status, and a CRC of the
slice's audio (``take_samples``, little-endian f64). ``target/release/famicom-trace``
writes the same lines; ``scripts/lockstep.py`` compares them.

    famicom_trace.py ROM --slices N [--machine KEY|auto] [--input FILE] [--device N]
                     [--reset FRAME ...] [--detail FRAME] [--auto-disk] [--rgb]
                     [--host FILE [--host-device N]] [--dual] --out FILE

``--machine auto``: ``System.machine_for`` (a Vs. game gets its Vs. row).
``--rgb``: each line also ends with ``rgb`` and a CRC of the picture in the
machine's colours (``video.machine_palette``: an RGB PPU's own), so the palette
tables are judged too.
``--dual``: a Vs. DualSystem image, both sides (``dual.VsDualSystem``), each slice
one ``run_frames(1)``; the line is the slice, ``wd`` and the watchdog's resets, then
``A`` and the primary side's fields, ``B`` and the secondary's (each as a single
system's line has them, from ``cyc`` to the sound). Input pads 0 and 1 are the
primary side's, 2 and 3 the secondary's.

``--input``: lines ``FRAME PAD BUTTONS`` (hex), applied before that slice to a
standard pad (a Vs. stick), and lines ``FRAME set PATH FIELD [VALUE ...]`` that drive the
device ``--device`` plugged (see ``drive``). A Vs. System's mainboard takes
``FRAME coin [SIDE] ACCEPTOR`` (``System.insert_coin``, before the slice runs),
``FRAME dip [SIDE] HEX`` (``vs.dip``) and ``FRAME service [SIDE] 0|1``; SIDE (0
primary, 1 secondary) only with ``--dual``.
``--device N``: ``System.plug(N)`` before the first slice (NES 2.0 expansion
number, decimal or 0x hex); ``auto``, the cartridge's own (``System.plug()``);
``hori-adapter``, a ``peripherals.HoriAdapter`` on the expansion port (no number
plugs it). With R.O.B., each line ends with ``rob HEIGHT TURN ARMS LED COMMANDS``
after ``see(ppu.picture)``; every other line is unchanged.
``--auto-disk``: the FDS's disks change as the program asks (``fds_auto.AutoDisk``,
observing after the tracer, as the Rust board calls it after its log), each
change written to stderr with its slice.
``--host FILE``: the reference's window drives the console instead of
``--input``/``--device``: ``Frontend._power_on`` plugs the device (``--host-device``,
else the cartridge's, as ``--device`` would not: the window's own choice) and
each slice ``Frontend._apply_inputs`` reads a stand-in pygame (``FakePygame``)
that FILE drives (see ``read_host``). Each line then ends with `` host`` and what
the window set (``host_summary``), before the slice ran.
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
from famicom_python.video import machine_palette, to_rgb

VS_EVENTS = ("coin", "dip", "service")


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


# -- --host: the reference's window on a stand-in pygame ------------------------

GAMEPAD_FIELDS = ("up", "down", "left", "right", "south", "east", "north", "back", "start",
                  "lshoulder", "rshoulder")  # fmt: skip


class FakeController:
    def __init__(self) -> None:
        self.held: set[str] = set()
        self.axes = {"LEFTX": 0, "LEFTY": 0}

    def get_button(self, name: str) -> bool:
        return name in self.held

    def get_axis(self, name: str) -> int:
        return self.axes[name]


class FakePygame:
    """The few pygame calls ``Frontend._apply_inputs`` makes. Key constants are
    their own names (``pg.K_a == "K_a"``), controller ones the gamepad fields."""

    CONTROLLER_AXIS_LEFTX = "LEFTX"
    CONTROLLER_AXIS_LEFTY = "LEFTY"
    CONTROLLER_BUTTON_DPAD_UP = "up"
    CONTROLLER_BUTTON_DPAD_DOWN = "down"
    CONTROLLER_BUTTON_DPAD_LEFT = "left"
    CONTROLLER_BUTTON_DPAD_RIGHT = "right"
    CONTROLLER_BUTTON_A = "south"
    CONTROLLER_BUTTON_B = "east"
    CONTROLLER_BUTTON_Y = "north"
    CONTROLLER_BUTTON_BACK = "back"
    CONTROLLER_BUTTON_START = "start"
    CONTROLLER_BUTTON_LEFTSHOULDER = "lshoulder"
    CONTROLLER_BUTTON_RIGHTSHOULDER = "rshoulder"

    def __init__(self) -> None:
        from types import SimpleNamespace

        self.keys: set[str] = set()
        self.pos = (0, 0)
        self.rel = (0, 0)
        self.left = self.right = False
        self.focused = True
        held = self.keys
        self.key = SimpleNamespace(get_pressed=lambda: _Pressed(held))
        self.mouse = SimpleNamespace(
            get_pos=lambda: self.pos,
            get_rel=lambda: self.rel,
            get_pressed=lambda: (self.left, False, self.right),
            get_focused=lambda: self.focused,
        )

    def __getattr__(self, name: str) -> str:
        if name.startswith("K_"):
            return name
        raise AttributeError(name)


class _Pressed:
    def __init__(self, held: set[str]) -> None:
        self.held = held

    def __getitem__(self, name: str) -> bool:
        return name in self.held


def read_host(path: str) -> dict[int, list[list[str]]]:
    """FILE's lines, ``FRAME WHAT ...`` (``#`` comments), each held until changed
    except ``move``: ``gamepads N``; ``key NAME 1|0`` (a pygame name);
    ``mouse X Y`` (picture pixels); ``move DX DY`` (this slice's motion);
    ``click left|right 1|0``; ``focus 1|0``; ``pad N FIELD 1|0`` (FIELD one of
    ``GAMEPAD_FIELDS``); ``stick N X Y`` (raw, -32768 to 32767)."""
    script: dict[int, list[list[str]]] = {}
    with open(path) as handle:
        for text in handle:
            parts = text.split()
            if parts and not text.startswith("#"):
                script.setdefault(int(parts[0]), []).append(parts[1:])
    return script


def host_step(pg: FakePygame, controllers: dict, lines: list[list[str]]) -> None:
    pg.rel = (0, 0)
    for what, *values in lines:
        if what == "gamepads":
            for n in range(int(values[0])):
                controllers.setdefault(n, FakeController())
        elif what == "key":
            (pg.keys.add if values[1] == "1" else pg.keys.discard)(values[0])
        elif what == "mouse":
            pg.pos = (int(values[0]), int(values[1]))
        elif what == "move":
            pg.rel = (int(values[0]), int(values[1]))
        elif what == "click":
            setattr(pg, values[0], values[1] == "1")
        elif what == "focus":
            pg.focused = values[0] == "1"
        elif what == "pad":
            held = controllers[int(values[0])].held
            (held.add if values[2] == "1" else held.discard)(values[1])
        elif what == "stick":
            controllers[int(values[0])].axes = {"LEFTX": int(values[1]), "LEFTY": int(values[2])}
        else:
            raise ValueError(f"host: {what}?")


def host_summary(frontend) -> str:
    """What the window set: as famicom_rust::host::describe writes it."""
    P = peripherals
    pads = ",".join(f"{pad.buttons:02X}" for pad in frontend._players())
    out = f"pads {pads} mic {int(frontend.system.board.microphone)}"

    def keys(names) -> str:
        return "+".join(sorted(names)) or "-"

    def gun(zapper) -> str:
        aim = "none" if zapper.aim is None else f"{zapper.aim[0]},{zapper.aim[1]}"
        return f" zapper {aim} {int(zapper.trigger)}"

    mice = (P.SnesMouse, P.SuborMouse, P.MacroWinnersMouse, P.YuxingMouse)
    for d in frontend._devices():
        if isinstance(d, P.Zapper):
            out += gun(d)
        elif isinstance(d, P.Vaus):
            out += f" vaus {d.position} {int(d.fire)}"
        elif isinstance(d, (P.PowerPad, P.FamilyTrainer)):
            out += f" mat {d.buttons:03X}"
        elif isinstance(d, (P.FamilyBasicKeyboard, P.SuborKeyboard)):
            out += f" keys {keys(d.keys)}"
        elif isinstance(d, P.KonamiHyperShot):
            out += f" buttons {d.buttons:02X}"
        elif isinstance(d, mice):
            p = d.pointer if hasattr(d, "pointer") else d
            out += f" mouse {p._dx} {p._dy} {int(p.left)} {int(p.right)}"
        elif isinstance(d, P.HoriTrack):
            p = d.pointer if hasattr(d, "pointer") else d
            out += f" track {p._dx} {p._dy} {d.pad.buttons:02X}"
        elif isinstance(d, P.BandaiHyperShot):
            out += gun(d.zapper)
        elif isinstance(d, P.CoconutsPachinko):
            out += f" dial {d.dial}"
        elif isinstance(d, P.ExcitingBoxing):
            out += f" sensors {d.sensors:02X}"
        elif isinstance(d, P.Partytap):
            out += f" buttons {d.buttons:02X}"
        elif isinstance(d, P.JissenMahjong):
            out += f" keys {keys(d.keys)}"
        elif isinstance(d, P.OekaKidsTablet):
            out += f" tablet {d.x} {d.y} {int(d.touching)} {int(d.click)}"
        elif isinstance(d, P.SnesController):
            out += f" snes {d.buttons:03X}"
    return out


def vs_event(system: System, what: str, value: str) -> None:
    """A mainboard line of ``--input``: a coin, the DIP switches, the service button."""
    if what == "coin":
        system.insert_coin(int(value, 0))
    elif what == "dip":
        system.board.vs.dip = int(value, 16)
    else:
        system.board.vs.service = value == "1"


def fields(system: System, log, samples: list[float], rgb_palette) -> str:
    """One side's part of a line: from ``cyc`` to the sound (``--rgb``: and its colours)."""
    board, cpu, ppu = system.board, system.cpu, system.board.ppu
    packed = array("Q", log)
    if sys.byteorder != "little":
        packed.byteswap()
    bus = zlib.crc32(packed.tobytes())
    picture = array("H", ppu.picture)
    if sys.byteorder != "little":
        picture.byteswap()
    pic = zlib.crc32(picture.tobytes())
    snd = zlib.crc32(struct.pack(f"<{len(samples)}d", *samples))
    text = (
        f"cyc {board.cycles} ins {system.instructions} "
        f"pc {cpu.pc:04X} a {cpu.a:02X} x {cpu.x:02X} y {cpu.y:02X} "
        f"s {cpu.s:02X} p {cpu.p:02X} bus {bus:08X} {len(log)} "
        f"pic {pic:08X} v {ppu.v:04X} t {ppu.t:04X} st {ppu.status:02X} "
        f"snd {snd:08X} {len(samples)}"
    )
    if rgb_palette is not None:
        text += f" rgb {zlib.crc32(to_rgb(ppu.picture, rgb_palette)):08X}"
    return text


def run_dual(args, inputs, events) -> int:
    """``--dual``: both sides of a Vs. DualSystem, a slice at a time."""
    from famicom_python.dual import VsDualSystem

    dual = VsDualSystem.from_rom(read_rom_file(args.rom))
    logs = []
    for side in dual.sides:
        log = array("Q")
        logs.append(log)

        def observe(kind: str, address: int, value: int, t: int, append=log.append) -> None:
            append(t << 25 | (kind == "write") << 24 | address << 8 | value)

        side.board.bus_observers.append(observe)
    palettes = [machine_palette(side.machine.timing) if args.rgb else None for side in dual.sides]
    with open(args.out, "w") as out:
        for frame in range(args.slices):
            for pad, buttons in inputs.get(frame, ()):
                dual.sides[pad // 2].board.pads[pad % 2].buttons = buttons
            for what, side, value in events.get(frame, ()):
                vs_event(dual.sides[side], what, value)
            if frame in args.reset:
                dual.reset()
            for log in logs:
                del log[:]
            dual.run_frames(1)
            parts = [f"{frame} wd {dual.watchdog_resets}"]
            for name, side, log, palette in zip("AB", dual.sides, logs, palettes, strict=True):
                parts.append(f"{name} {fields(side, log, side.take_samples(), palette)}")
            out.write(" ".join(parts) + "\n")
            if frame == args.detail:
                for name, log in zip("AB", logs, strict=True):
                    for packed in log:
                        t, rest = packed >> 25, packed & 0x1FFFFFF
                        kind = "W" if rest >> 24 else "R"
                        out.write(f"  {name} {t} {kind} {(rest >> 8) & 0xFFFF:04X} {rest & 0xFF:02X}\n")
    return 0


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
    parser.add_argument("--host")
    parser.add_argument("--host-device")
    parser.add_argument("--rgb", action="store_true")
    parser.add_argument("--dual", action="store_true")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    inputs: dict[int, list[tuple[int, int]]] = {}
    sets: dict[int, list[list[str]]] = {}
    events: dict[int, list[tuple[str, int, str]]] = {}
    if args.input:
        with open(args.input) as handle:
            for text in handle:
                parts = text.split()
                if text.startswith("#"):
                    continue
                if len(parts) >= 3 and parts[1] in VS_EVENTS:
                    side = int(parts[2]) if args.dual else 0
                    events.setdefault(int(parts[0]), []).append((parts[1], side, parts[-1]))
                elif len(parts) == 3:
                    inputs.setdefault(int(parts[0]), []).append((int(parts[1]), int(parts[2], 16)))
                elif len(parts) >= 4 and parts[1] == "set":
                    sets.setdefault(int(parts[0]), []).append(parts[2:])

    if args.dual:
        return run_dual(args, inputs, events)
    frontend = host = None
    if args.host:
        from pathlib import Path

        from famicom_python.frontend import Frontend, save_path

        if save_path(Path(args.rom)).exists():
            raise SystemExit(f"{save_path(Path(args.rom))}: a save the window would load")
        host = read_host(args.host)
        frontend = Frontend.__new__(Frontend)
        frontend.pg = FakePygame()
        frontend.rom = Path(args.rom)
        frontend.machine_name = args.machine
        frontend.device_choice = None if args.host_device is None else int(args.host_device, 0)
        frontend.vaus_low = 0x62
        frontend.auto_disk = False
        frontend._load_storage = lambda system: None
        frontend.controllers = {}
        system = frontend._power_on()
        frontend.system = system
    else:
        data = read_rom_file(args.rom)
        machine = System.machine_for(data) if args.machine == "auto" else MACHINES[args.machine]
        system = System.from_rom(data, machine)
    board, cpu, ppu = system.board, system.cpu, system.board.ppu
    plugged = None
    if frontend is not None:
        plugged = frontend.peripheral
    elif args.device == "auto":
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
            for what, _side, value in events.get(frame, ()):
                vs_event(system, what, value)
            summary = None
            if frontend is not None:
                host_step(frontend.pg, frontend.controllers, host.get(frame, []))
                frontend._apply_inputs()
                summary = host_summary(frontend)
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
            if summary is not None:
                out.write(f" host {summary}")
            if args.rgb:
                out.write(f" rgb {zlib.crc32(to_rgb(ppu.picture, machine_palette(ppu.timing))):08X}")
            out.write("\n")
            if frame == args.detail:
                for packed in log:
                    t, rest = packed >> 25, packed & 0x1FFFFFF
                    kind = "W" if rest >> 24 else "R"
                    out.write(f"  {t} {kind} {(rest >> 8) & 0xFFFF:04X} {rest & 0xFF:02X}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
