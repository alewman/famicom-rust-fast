#!/usr/bin/env python3
"""The host gate's input: one script of a player's window input (famicom_trace.py
``--host`` format), deterministic, touching everything the reference's window
reads, so each device's case drives that device and the pads through
``Frontend._apply_inputs`` and ``famicom_rust::host::apply`` alike.

    make_host_inputs.py --slices 900 --out scripts/inputs/host-all.txt

Four gamepads (buttons and sticks, now and then past half way); the keys of
every table the window has (pad keys, Power Pad rows, both keyboards, the
mahjong panel, the Partytap, the microphone); the mouse inside the picture
and past its edges, its motion, both buttons and the window's focus. Start on
gamepad 1 every 120 slices so games leave their titles.
"""

from __future__ import annotations

import argparse
import random

KEYS = sorted(
    {
        *(f"K_{c}" for c in "abcdefghijklmnopqrstuvwxyz0123456789"),
        *(f"K_F{n}" for n in range(1, 13)),
        *(f"K_KP{d}" for d in range(10)),
        "K_UP", "K_DOWN", "K_LEFT", "K_RIGHT", "K_RETURN", "K_KP_ENTER", "K_RSHIFT",
        "K_LSHIFT", "K_LCTRL", "K_RCTRL", "K_LALT", "K_RALT", "K_SPACE", "K_BACKSPACE",
        "K_TAB", "K_MINUS", "K_EQUALS", "K_BACKQUOTE", "K_LEFTBRACKET", "K_RIGHTBRACKET",
        "K_SEMICOLON", "K_QUOTE", "K_BACKSLASH", "K_COMMA", "K_PERIOD", "K_SLASH",
        "K_HOME", "K_END", "K_INSERT", "K_DELETE", "K_PAUSE", "K_PAGEUP", "K_PAGEDOWN",
        "K_CAPSLOCK", "K_NUMLOCK", "K_KP_MINUS", "K_KP_PLUS", "K_KP_MULTIPLY",
        "K_KP_DIVIDE", "K_KP_PERIOD",
    }
)  # fmt: skip
MAT = ("K_1", "K_2", "K_3", "K_4", "K_q", "K_w", "K_e", "K_r", "K_a", "K_s", "K_d", "K_f")
FIELDS = ("up", "down", "left", "right", "south", "east", "north", "back", "start",
          "lshoulder", "rshoulder")  # fmt: skip


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--slices", type=int, default=900)
    parser.add_argument("--seed", type=int, default=2026)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rng = random.Random(args.seed)
    lines = ["# make_host_inputs.py --slices %d --seed %d" % (args.slices, args.seed),
             "0 gamepads 4", "0 mouse 128 120"]  # fmt: skip
    held: set[str] = set()
    for frame in range(1, args.slices):
        if frame % 120 == 0:
            lines.append(f"{frame} pad 0 start 1")
        if frame % 120 == 4:
            lines.append(f"{frame} pad 0 start 0")
        if frame % 7 == 0:  # keys: press one, let one go
            key = rng.choice(KEYS)
            lines.append(f"{frame} key {key} 1")
            held.add(key)
            if len(held) > 3:
                gone = rng.choice(sorted(held))
                held.discard(gone)
                lines.append(f"{frame} key {gone} 0")
        if frame % 5 == 0:
            pad = rng.randrange(4)
            field = rng.choice(FIELDS[:8] if pad else FIELDS[:4] + FIELDS[4:8] + FIELDS[9:])
            lines.append(f"{frame} pad {pad} {field} {rng.randrange(2)}")
        if frame % 29 == 0:  # the stick, past half way, short of it, and exactly at it
            values = (-32768, -20000, -16385, -16384, -16000, 0, 16000, 16384, 16385, 32767)
            lines.append(f"{frame} stick {rng.randrange(4)} {rng.choice(values)} "
                         f"{rng.choice(values)}")  # fmt: skip
        if frame % 3 == 1:  # the mat's and the bag's keys, often: each bit seen alone
            lines.append(f"{frame} key {rng.choice(MAT)} {rng.randrange(2)}")
        if frame % 3 == 0:
            lines.append(f"{frame} mouse {rng.randrange(-12, 268)} {rng.randrange(-12, 252)}")
        if frame % 2 == 0:
            lines.append(f"{frame} move {rng.randrange(-40, 41)} {rng.randrange(-40, 41)}")
        if frame % 11 == 0:
            lines.append(f"{frame} click {rng.choice(('left', 'right'))} {rng.randrange(2)}")
        if frame % 97 == 0:
            lines.append(f"{frame} focus {int(rng.random() < 0.7)}")
    with open(args.out, "w") as out:
        out.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
