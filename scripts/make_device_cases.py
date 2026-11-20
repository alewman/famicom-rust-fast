#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Write scripts/cases-peripherals.tsv, rung 6's device rows: for each device
class, test ROMs and dumps (/data/emu/roms/nes, nes20db's expansion number)
that use it, plugged with ``--device`` and driven by scripts/inputs/devices
(scripts/make_device_inputs.py). A dump's machine is the reference's
``System.machine_for`` unless the row names one. Run on the reference's
interpreter (it loads each dump).
"""

from pathlib import Path

from famicom_python.cartridge import read_rom_file
from famicom_python.machine import MACHINES
from famicom_python.system import System

R = "/data/emu/roms/nes"
T = "$NES_TEST_ROMS/nes-test-roms"
I = "$INPUTS/devices"
# name, rom, slices, device, input, machine (None: machine_for; test ROMs: given)
ROWS = [
    # four players
    ("four-score/gauntlet-ii", f"{R}/Gauntlet II (USA).zip", 1500, "0x02", "four-score", None),
    ("four-score/read_joy3-test_buttons", f"{T}/read_joy3/test_buttons.nes", 900, "0x02", "four-score", "nes"),
    ("famicom-four/bomberman-ii", f"{R}/Bomberman II (Japan) (En).zip", 1500, "0x03", "expansion-pads", None),
    ("famicom-four/kunio-soccer", f"{R}/Kunio-kun no Nekketsu Soccer League (Japan).zip", 1500, "0x03", "expansion-pads", None),
    ("hori-adapter/bomberman-ii", f"{R}/Bomberman II (Japan) (En).zip", 1500, "hori-adapter", "hori-adapter", None),
    ("hori-adapter/kunio-soccer", f"{R}/Kunio-kun no Nekketsu Soccer League (Japan).zip", 1500, "hori-adapter", "hori-adapter", None),
    # Zappers
    ("zapper-famicom/duck-hunt", f"{R}/Duck Hunt (World).zip", 1500, "0x08", "zapper-duck-hunt", None),
    ("zapper-nes/duck-hunt", f"{R}/Duck Hunt (World).zip", 1500, "0x08", "zapper-duck-hunt", "nes"),
    ("zapper-nes/hogans-alley", f"{R}/Hogan's Alley (World).zip", 1500, "0x08", "zapper-duck-hunt", "nes"),
    ("zapper-nes/hogans-alley-pal", f"{R}/Hogan's Alley (World).zip", 1500, "0x08", "zapper-duck-hunt", "pal"),
    # Chiller (0x09) and Space Shadow (0x0A) are mappers 11 and 70, not in this tree
    # yet: Duck Hunt hosts both devices (it reads $4017 D3-D4).
    ("two-zappers/duck-hunt", f"{R}/Duck Hunt (World).zip", 1500, "0x09", "two-zappers", "nes"),
    ("zapper-4016/shark-pirate", f"{R}/Shark Pirate's Harpoon The Dolphin - Win a Prize! (World) (Demo) (Aftermarket) (Unl).zip", 1500, "0x49", "zapper-4016", None),
    ("bandai-hyper-shot/duck-hunt", f"{R}/Duck Hunt (World).zip", 1500, "0x0A", "bandai-hyper-shot", None),
    # Vaus
    ("vaus-nes/vaus-test", f"{T}/vaus-test/vaus-test.nes", 1500, "0x0F", "vaus-nes", "nes"),
    # PaddleTest3 reads a Vaus on $4016 D4 (port 1), where plug never puts one:
    # no row (its trace is the same with or without the device).
    ("vaus-nes/arkanoid-usa", f"{R}/Arkanoid (USA).zip", 1500, "0x0F", "vaus-nes", None),
    ("vaus-famicom/arkanoid-japan", f"{R}/Arkanoid (Japan) (En).zip", 1500, "0x10", "vaus-nes", None),
    ("vaus-famicom/vaus-test", f"{T}/vaus-test/vaus-test.nes", 1500, "0x10", "vaus-nes", "famicom"),
    ("vaus-famicom-on-nes/chase-hq", f"{R}/Taito Chase H.Q. (Japan).zip", 1500, "0x10", "vaus-nes", "nes"),
    # Arkanoid II (0x11) is mapper 152, not in this tree yet: Arkanoid hosts the pair.
    ("vaus-pair/arkanoid-japan", f"{R}/Arkanoid (Japan) (En).zip", 1500, "0x11", "vaus-pair", None),
    # mats
    ("power-pad/world-class-track-meet", f"{R}/World Class Track Meet (USA).zip", 1500, "0x0C", "power-pad", None),
    ("power-pad/athletic-world", f"{R}/Athletic World (USA).zip", 1500, "0x0B", "power-pad", None),
    ("power-pad/test-cartridge", f"{R}/Nintendo World Class Service - Power Pad Test Cartridge (USA) (Test Program).zip", 1500, "0x0C", "power-pad", None),
    ("family-trainer/athletic-world", f"{R}/Family Trainer 1 - Athletic World (Japan).zip", 1500, "0x0D", "family-trainer", None),
    ("family-trainer/aerobics-studio", f"{R}/Family Trainer 3 - Aerobics Studio (Japan).zip", 1500, "0x0E", "family-trainer", None),
    ("taptap-mat/mogura-tataki", f"{R}/Super Mogura Tataki!! - Pokkun Mogurar (Japan).zip", 1500, "0x1A", "family-trainer", None),
    # keyboards and storage
    ("family-basic-keyboard/family-basic-v2.1", f"{R}/Family BASIC v2.1 (Japan).zip", 1500, "0x23", "family-basic-keyboard", None),
    ("data-recorder/family-basic-v2.1-save-load", f"{R}/Family BASIC v2.1 (Japan).zip", 7700, "0x23", "data-recorder", None),
    ("data-recorder/excitebike", f"{R}/Excitebike (Japan, USA) (En).zip", 1500, "0x20", "data-recorder-only", None),
        # Of the Turbo File dumps, Dungeon Kid is the one whose start (menus by pad 1)
    # reads and writes the Turbo File; the others only save from deep in play.
    ("turbo-file/dungeon-kid", f"{R}/Dungeon Kid (Japan).zip", 2500, "0x21", "turbo-file", None),
    ("subor-keyboard/text-editor", f"{R}/Text Editor (Russia) (Subor Keyboard) (Unl).zip", 1500, "0x26", "subor-keyboard", None),
    ("subor-mouse-4016/text-editor", f"{R}/Text Editor (Russia) (Subor Keyboard) (Unl).zip", 1500, "0x28", "subor-mouse", None),
    ("subor-mouse-4017/text-editor", f"{R}/Text Editor (Russia) (Subor Keyboard) (Unl).zip", 1500, "0x36", "subor-mouse", None),
    ("macro-winners/text-editor", f"{R}/Text Editor (Russia) (Subor Keyboard) (Unl).zip", 1500, "0x27", "subor-mouse", None),
    ("mega-book/mega-book-4-in-1", f"{R}/Mega Book 4-in-1 (Italy) (Aftermarket) (Unl).zip", 1500, "0x4F", "subor-mouse", None),
    ("subor-yuxing/text-editor", f"{R}/Text Editor (Russia) (Subor Keyboard) (Unl).zip", 1500, "0x44", "subor-mouse", None),
    # The Yuxing Computer's BIOS (0x43) is mapper 169, not in the reference; alone,
    # the mouse needs a host that reads $4016 D0 every frame (Duck Hunt's title).
    ("yuxing-mouse/duck-hunt", f"{R}/Duck Hunt (World).zip", 1500, "0x43", "mouse-only", None),
    # serial reports
    ("snes-mouse-4016/thwaite", f"{R}/Thwaite (World) (Proto 4) (Aftermarket) (Unl).zip", 1500, "0x29", "snes-mouse", None),
    ("snes-mouse-4016/crosspaint", f"{R}/CrossPaint (World) (Demo 5) (Aftermarket) (Unl).zip", 1500, "0x29", "snes-mouse", None),
    ("snes-mouse-4017/theremin", f"{R}/Theremin (World) (Aftermarket) (Unl).zip", 1500, "0x48", "snes-mouse", None),
    ("snes-controllers/read_joy3-test_buttons", f"{T}/read_joy3/test_buttons.nes", 900, "0x2B", "snes-controllers", "nes"),
    ("snes-controllers/gauntlet-ii", f"{R}/Gauntlet II (USA).zip", 1500, "0x2B", "snes-controllers", None),
    ("hori-track/bomberman-ii", f"{R}/Bomberman II (Japan) (En).zip", 1500, "0x50", "hori-track", None),
    ("hori-track/kunio-soccer", f"{R}/Kunio-kun no Nekketsu Soccer League (Japan).zip", 1500, "0x50", "hori-track", None),
    ("pachinko/pachio-kun-4", f"{R}/Pachio-kun 4 (Japan).zip", 1500, "0x13", "pachinko", None),
    # the tail
    ("konami-hyper-shot/hyper-olympic", f"{R}/Hyper Olympic (Japan) (En).zip", 1500, "0x12", "konami-hyper-shot", None),
    # Exciting Boxing and the Oeka Kids carts are mappers 75 and 96, not in this tree
    # yet: Hyper Olympic, which reads $4017 D1-D4, hosts both devices.
    ("boxing-bag/hyper-olympic", f"{R}/Hyper Olympic (Japan) (En).zip", 1500, "0x14", "boxing", None),
    ("jissen-mahjong/jissen-mahjong", f"{R}/Ide Yousuke Meijin no Jissen Mahjong (Japan).zip", 1500, "0x15", "jissen-mahjong", None),
    ("partytap/casino-derby", f"{R}/Casino Derby (Japan).zip", 1500, "0x16", "partytap", None),
    ("oeka-kids/hyper-olympic", f"{R}/Hyper Olympic (Japan) (En).zip", 1500, "0x17", "oeka-kids", None),
    ("famicom-3d/highway-star", f"{R}/Highway Star (Japan) (En) (Famicom 3D System).zip", 1500, "0x1D", "famicom-3d", None),
    ("rob/gyromite", f"{R}/Gyromite (World).zip", 1500, "0x1F", "rob-gyromite-direct", None),
    ("rob/stack-up", f"{R}/Stack-Up (World).zip", 1500, "0x2E", "rob-stack-up-direct", None),
    ("port-test/port-test-cartridge", f"{R}/Nintendo World Class Service - Port Test Cartridge (USA) (Test Program).zip", 1500, "0x37", "port-test", None),
    ("auto/arkanoid-japan", f"{R}/Arkanoid (Japan) (En).zip", 600, "auto", "vaus-nes", None),
]

out = ["# name\tslices\trom\targuments: device rows (rung 6's peripherals); $NES_TEST_ROMS $INPUTS"]
for name, rom, slices, device, inputs, machine in ROWS:
    if machine is None:
        m = System.machine_for(read_rom_file(rom))
        machine = next(k for k, v in MACHINES.items() if v is m)
    out.append(f"{name}\t{slices}\t{rom}\t--machine {machine} --device {device} --input {I}/{inputs}.txt")
target = Path(__file__).resolve().parent / "cases-peripherals.tsv"
target.write_text("\n".join(out) + "\n")
print(len(ROWS), "cases")
