#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write the device rows' input files, scripts/inputs/devices/*.txt (rung 6's
peripherals, scripts/cases-peripherals.tsv): pad lines ``FRAME PAD BUTTONS`` and
device lines ``FRAME set PATH FIELD VALUE...`` (scripts/famicom_trace.py) that
make each device matter: aim and pull, turn the knob, press the mat, type keys,
move the mouse. Stdlib only.
"""

from pathlib import Path

OUT = Path(__file__).resolve().parent / "inputs" / "devices"
files: dict[str, list[str]] = {}


def shots(path: str, start: int, count: int, every: int = 30, hold: int = 6) -> list[str]:
    lines = []
    for i in range(count):
        f = start + i * every
        x, y = (i * 53 + 17) % 256, 40 + (i * 37) % 160
        lines += [f"{f} set {path} aim {x} {y}", f"{f + 2} set {path} trigger 1",
                  f"{f + 2 + hold} set {path} trigger 0"]
    lines.append(f"{start + count * every} set {path} aim none")
    return lines


def pad(port: int, frames: list[tuple[int, int]], hold: int = 6) -> list[str]:
    out = []
    for f, b in frames:
        out += [f"{f} {port} {b:02x}", f"{f + hold} {port} 00"]
    return out


def start_presses(port: int = 0, frames=(60, 120, 180, 240)) -> list[str]:
    return pad(port, [(f, 0x08) for f in frames])


files["zapper-duck-hunt"] = ["# Duck Hunt with the Zapper: Start, then 40 shots across the picture"] + \
    start_presses() + shots("-", 300, 40)
files["zapper-4016"] = ["# A Zapper on $4016: shots across the picture"] + shots("-", 120, 60)
files["two-zappers"] = ["# Two Zappers: Start on neither (no pads); shots from both"] + \
    shots("0", 100, 40) + shots("1", 115, 40)
files["bandai-hyper-shot"] = ["# Bandai Hyper Shot: its pad's Start and pad 1's, then shots"] + start_presses() + \
    [f"{f} set pad buttons {b}" for f, b in [(60, 8), (66, 0), (120, 8), (126, 0), (180, 8), (186, 0)]] + \
    shots("zapper", 240, 40)


def vaus(path: str, start: int, count: int, fire_every: int = 50) -> list[str]:
    lines = []
    for i in range(count):
        f = start + i * 10
        position = (0x98 + (i * 23) % 0xA0) & 0xFF
        lines.append(f"{f} set {path} position {position}")
        if i % 5 == 0:
            lines += [f"{f} set {path} fire 1", f"{f + 4} set {path} fire 0"]
    return lines


files["vaus-nes"] = ["# Vaus: fire to start, then the knob sweeping, fire now and then"] + vaus("-", 60, 150)
files["vaus-pair"] = ["# Two Vaus (Arkanoid II): both knobs and buttons"] + vaus("0", 60, 150) + vaus("1", 65, 150)


def buttons(path: str, start: int, values: list[int], every: int = 20, hold: int = 8) -> list[str]:
    lines = []
    for i, v in enumerate(values):
        f = start + i * every
        lines += [f"{f} set {path} buttons {v:#x}", f"{f + hold} set {path} buttons 0"]
    return lines


files["four-score"] = ["# Four Score: Start on player 1, then all four players' buttons"] + \
    [line for f in (60, 120, 180, 240) for line in (f"{f} set pads.0 buttons 8", f"{f + 6} set pads.0 buttons 0")] + \
    [line for p in range(4) for line in buttons(f"pads.{p}", 300 + p * 5, [1 << ((i + p) % 8) for i in range(40)])]
files["expansion-pads"] = ["# Players 3 and 4 on the expansion port, plus pad 1's Start"] + start_presses() + \
    [line for p in range(2) for line in buttons(f"pads.{p}", 300 + p * 7, [1 << ((i + p) % 8) for i in range(40)])]
files["hori-adapter"] = ["# The Hori 4 Players Adapter's four pads, plus pad 1's Start"] + start_presses() + \
    [line for p in range(4) for line in buttons(f"pads.{p}", 300 + p * 5, [1 << ((i + p) % 8) for i in range(40)])]
files["famicom-3d"] = ["# The 3D System's passed-through pads, plus pad 1's Start"] + start_presses() + \
    [line for p in range(2) for line in buttons(f"through.pads.{p}", 300 + p * 7, [1 << ((i + p) % 8) for i in range(40)])]
files["snes-controllers"] = ["# Two SNES pads: Start on the first, then all 12 buttons on both"] + \
    buttons("0", 60, [8, 8, 8, 8], every=60) + \
    [line for p in range(2) for line in buttons(str(p), 300 + p * 5, [1 << ((i + p) % 12) for i in range(48)])]
files["power-pad"] = ["# Power Pad: pad 1's Start, each of the 12 buttons, then pairs"] + start_presses() + \
    buttons("-", 300, [1 << (i % 12) for i in range(48)] + [(1 << (i % 12)) | (1 << ((i + 5) % 12)) for i in range(24)])
files["family-trainer"] = ["# Family Trainer: each of the 12 buttons, then pairs"] + \
    buttons("-", 60, [1 << (i % 12) for i in range(48)] + [(1 << (i % 12)) | (1 << ((i + 5) % 12)) for i in range(24)])
files["konami-hyper-shot"] = ["# Konami Hyper Shot: the four buttons, pad 1's Start first"] + start_presses() + \
    buttons("-", 300, [1 << (i % 4) for i in range(60)] + [i % 16 for i in range(30)], every=12, hold=6)
files["partytap"] = ["# Partytap: the six buttons"] + start_presses() + \
    buttons("-", 300, [1 << (i % 6) for i in range(60)] + [i % 64 for i in range(30)], every=12, hold=6)
files["boxing"] = ["# Exciting Boxing: the eight sensors"] + start_presses() + \
    [line for i in range(80) for line in (f"{300 + i * 12} set - sensors {1 << (i % 8)}",
                                          f"{306 + i * 12} set - sensors 0")]


def keys(path: str, start: int, names: list[str], every: int = 12, hold: int = 6) -> list[str]:
    lines = []
    for i, name in enumerate(names):
        f = start + i * every
        lines += [f"{f} set {path} keys {name}", f"{f + hold} set {path} keys"]
    return lines


FB_KEYS = "RETURN F1 A S D F G H J K L 1 2 3 4 5 SPACE RETURN STOP SPACE RETURN Q W E R T Y U I O P RETURN LEFT RIGHT UP DOWN CLR INS DEL ESC CTR GRPH KANA LSHIFT RSHIFT RETURN".split()
SUBOR_KEYS = "ENTER F1 A S D F G H J K L 1 2 3 4 5 SPACE ENTER ESC SPACE ENTER Q W E R T Y U I O P ENTER LEFT RIGHT UP DOWN TAB DELETE BACKSPACE KP1 KP2 F12 NUMLOCK LSHIFT ENTER".split()
MAHJONG_KEYS = "START A B C D E F G H I J K L M N RON RIICHI CHII PON KAN SELECT START".split()

files["family-basic-keyboard"] = ["# Family BASIC: keys typed one at a time"] + keys("-", 120, FB_KEYS * 2, every=15, hold=7)
files["subor-keyboard"] = ["# Subor Keyboard: keys typed one at a time, pad Start too"] + start_presses() + \
    keys("-", 300, SUBOR_KEYS * 2, every=15, hold=7)
files["jissen-mahjong"] = ["# Jissen Mahjong: buttons one at a time"] + keys("-", 120, MAHJONG_KEYS * 4, every=15, hold=7)


def pointer(path: str, start: int, count: int, every: int = 6) -> list[str]:
    lines = []
    moves = [(1, 0), (0, 1), (-1, 0), (0, -1), (5, 3), (-7, 2), (40, -30), (-90, 70), (2, -2), (0, 0)]
    for i in range(count):
        f = start + i * every
        dx, dy = moves[i % len(moves)]
        lines.append(f"{f} set {path} move {dx} {dy}")
        if i % 7 == 0:
            lines += [f"{f} set {path} left 1", f"{f + 3} set {path} left 0"]
        if i % 11 == 0:
            lines += [f"{f} set {path} right 1", f"{f + 4} set {path} right 0"]
    return lines


files["snes-mouse"] = ["# SNES Mouse: motion and both buttons"] + pointer("-", 60, 200)
files["subor-mouse"] = ["# Subor Keyboard and mouse: motion, buttons, keys"] + pointer("1", 60, 200) + \
    keys("0", 63, SUBOR_KEYS, every=25, hold=7)
files["mouse-only"] = ["# A mouse alone: motion and both buttons"] + pointer("-", 60, 200)
files["hori-track"] = ["# Hori Track: motion, its pad, the switches"] + pointer("-", 60, 200) + \
    buttons("pad", 62, [8, 1, 2, 0x10, 0x20, 0x40, 0x80, 4] * 6, every=21) + \
    ["400 set - rotation_l 1", "700 set - slow 1", "900 set - rotation_l 0"]
files["pachinko"] = ["# Pachinko: pad Start (its own pad), the dial swept"] + \
    buttons("pad", 60, [8, 8, 8, 8, 1, 1], every=40) + \
    [f"{300 + i * 8} set - dial {(3 + i * 7) % 0x74}" for i in range(120)] + \
    buttons("pad", 310, [1] * 30, every=31)
files["oeka-kids"] = ["# Oeka Kids: pen moving, touching and clicking"] + \
    [line for i in range(150) for line in (f"{60 + i * 8} set - x {(i * 13) % 240}",
                                           f"{60 + i * 8} set - y {(i * 29) % 256}",
                                           f"{60 + i * 8} set - touching {int(i % 3 != 0)}",
                                           f"{60 + i * 8} set - click {int(i % 9 == 4)}")]
files["port-test"] = ["# Port test controller: both pads' buttons"] + \
    [line for p in range(2) for line in buttons(f"{p}.pad", 60 + p * 7, [1 << ((i + p) % 8) for i in range(40)])]
files["turbo-file"] = ["# Turbo File: pad 1 Start, A, Up, A, Down through the menus; locked from 1500"] + \
    pad(0, [(60 + i * 40, (0x08, 0x01, 0x01, 0x10, 0x01, 0x20)[i % 6]) for i in range(60)]) + \
    ["1500 set - locked 1"]


def family_basic_tape() -> list[str]:
    """Family BASIC v2.1 to SAVE and LOAD through the Data Recorder: RETURN through
    the start dialog, F1 (yes, GAME BASIC), 1 (BASIC), type 10 PRINT 7, record and
    SAVE (WRITING ... OK by frame 5500), stop, NEW, play and LOAD (LOADING ... OK),
    then LIST."""
    lines = [line for i in range(80) for line in (f"{100 + i * 30} set - keys RETURN",
                                                  f"{106 + i * 30} set - keys")]
    lines += ["2610 set - keys F1", "2616 set - keys", "2650 set - keys RETURN", "2656 set - keys",
              "3310 set - keys 1", "3316 set - keys"]
    frame = 3450

    def type_keys(names: list[str]) -> None:
        nonlocal frame
        for name in names:
            lines.extend([f"{frame} set - keys {name}", f"{frame + 5} set - keys"])
            frame += 12

    type_keys("1 0 SPACE P R I N T SPACE 7 RETURN".split())
    lines.append(f"{frame} set recorder record")
    frame += 10
    type_keys("S A V E RETURN".split())
    lines.append("5500 set recorder stop")
    frame = 5510
    type_keys("N E W RETURN".split())
    lines.append(f"{frame} set recorder play")
    frame += 10
    type_keys("L O A D RETURN".split())
    frame = 7500
    type_keys("L I S T RETURN".split())
    return lines


files["data-recorder"] = ["# Family BASIC: SAVE to the Data Recorder, NEW, LOAD it back, LIST"] + family_basic_tape()
files["data-recorder-only"] = ["# Data Recorder device: record, then play back"] + \
    ["60 set - record", "500 set - stop", "520 set - play"] + start_presses()
# Gyromite's DIRECT mode (Start at the title, Select once, Start), then pad 1
# moves R.O.B., each move flashed as a 13-frame command: Up, Down, Left, Right, A, B.
files["rob-gyromite-direct"] = ["# Gyromite DIRECT: pad 1's directions and A/B flash R.O.B. commands"] + \
    pad(0, [(230, 0x08), (280, 0x04), (300, 0x08)]) + \
    pad(0, [(450 + i * 80, (0x10, 0x20, 0x40, 0x80, 0x01, 0x02)[i % 6]) for i in range(13)], hold=20)
files["rob-stack-up-direct"] = ["# Stack-Up DIRECT (Start, Down, Start): pad 1 flashes R.O.B. commands"] + \
    pad(0, [(230, 0x08), (280, 0x20), (330, 0x08)]) + \
    pad(0, [(450 + i * 80, (0x10, 0x20, 0x40, 0x80, 0x01, 0x02)[i % 6]) for i in range(13)], hold=20)

OUT.mkdir(parents=True, exist_ok=True)
for name, lines in files.items():
    (OUT / f"{name}.txt").write_text("\n".join(lines) + "\n")
print(len(files), "files")
