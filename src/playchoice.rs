// SPDX-License-Identifier: MIT
//! The PlayChoice-10's game board: what differs from a console around the
//! 2A03 (`playchoice.py`).
//!
//! Sources, by tier: Nintendo's own PCH1-CPU schematic (© 1986 Nintendo of
//! America, six sheets; the NESdev wiki links it from CPU_Test_Mode.wiki; pinned
//! by the reference as `reference/docs/Playchoice.pdf`; **manufacturer's
//! documentation**, read by eye, each reading named by sheet and part);
//! PC10_ROM-Images.wiki and NES_2.0.wiki (specification tier); MAME's
//! `playch10.cpp` (BSD-3, pinned by the reference as
//! `reference/mame-nes/playch10.cpp`; emulator-derived).
//!
//! A PlayChoice-10 is an NES on an arcade board: an RP2A03E, an RP2C03B (RGB),
//! 2 KiB of work RAM, 2 KiB of nametable RAM and ten game sockets ("channels"),
//! each a small board with the game's ROMs, its mapper, an 8 KiB INST ROM for the
//! supervisor and an RP5H01 security PROM. A Z80 supervisor on the same board
//! (`supervisor.rs`) chooses the channel, holds the game's reset and stop
//! lines, and sells time. This module is the game's side of it:
//!
//! - **The panel** (sheet 5). Two 74LS165 shift registers, 8B and 8A, stand where
//!   the controller ports are: loaded while OUT0 is high (through an inverter to
//!   their S/L), clocked by the 2A03's /INP0 and /INP1, and read through
//!   inverting buffers (74LS368A) on D0. Their inputs H to A are A, B, Game
//!   Select, Start, Up, Down, Left, Right, so a report is a controller's. The
//!   serial inputs are grounded, which the buffer inverts: after eight bits a
//!   read returns 1, as an official controller does (MAME shifts in 0: the
//!   reference's docs/undocumented-behavior.md C82). Player 1's Start and Game
//!   Select pass through OR gates (74LS32, 8C) with the supervisor's /CNTRL MASK:
//!   while it is low they read released. Player 2's register has nothing on its E
//!   and F inputs: no Select, no Start (MAME's port agrees; its comment, "wired
//!   to 1p select button", does not).
//! - **The light gun** (sheet 5, "Gun Hit", "Gun Trigger"): two lines to D3 and
//!   D4 of $4017 beside player 2's serial data, the trigger through an OR gate
//!   with /CNTRL MASK. The polarity is set by solder jumpers (CL4-CL7, SL6-SL9)
//!   the schematic draws both ways; MAME's is the NES Zapper's (D3 = 0 while
//!   light is seen, D4 = 1 while the trigger is pulled), and the games are the
//!   NES ones, so [`PlayChoiceGun`] is the NES Zapper's model.
//! - **$4016/$4017**: the buffers drive D0-D4 (D1 and D2 have nothing on their
//!   inputs and read 0); D5-D7 are open bus, as on a console.
//! - **The image** ([`game_roms`]): the INST ROM and the PROM's two
//!   recordings, after the CHR ROM.
//!
//! Differences forced by Rust: the reference's `PlayChoicePad` subclasses
//! `StandardController` and `PlayChoiceGun` subclasses `Zapper`; here the pad
//! has the controller's fields and its own, and the gun holds its `zapper` and
//! the `pad` it shares $4017 with as fields. The reference's `masked` property
//! of the gun (the pad's flag) is [`PlayChoiceGun::masked`] and
//! [`PlayChoiceGun::set_masked`].

use crate::cartridge::RomImage;
use crate::joypad::{SELECT, START};
use crate::peripherals::{PortDevice, Zapper};
use crate::ppu::Ppu;
use std::any::Any;

pub const INST_SIZE: usize = 0x2000;
pub const PROM_SIZE: usize = 16;
/// "Usually constant" (PC10_ROM-Images.wiki): the RP5H01's counter bit 5 over
/// 128 clocks. What a dump without the PROM's recordings is given.
pub const COUNTER_OUT: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// A PlayChoice-10 game board's two chips besides the game's own.
///
/// `inst` is the INST ROM, at the Z80's $C000; `prom` and `counter_out` are the
/// RP5H01's Data and Counter Out pins recorded over 128 clocks, the first
/// clock's in bit 0 of the first byte (PC10_ROM-Images.wiki). `prom` is None
/// for a dump made without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRoms {
    pub inst: Vec<u8>,
    pub prom: Option<Vec<u8>>,
    pub counter_out: Vec<u8>,
}

/// Whether `image` is a PlayChoice-10 game: nes20db's console type, or the
/// header's own (byte 7's low bits, iNES and NES 2.0 alike). The header is
/// asked too because several PlayChoice games are their home cartridges' PRG
/// and CHR exactly, and nes20db, asked by those two hashes when the
/// miscellaneous ROM is not one it knows, answers with the cartridge.
pub fn is_playchoice(image: &RomImage) -> bool {
    image.board.console == 2 || image.header.get(7).is_some_and(|byte| byte & 0x03 == 2)
}

/// The miscellaneous-ROM area of a PlayChoice-10 image (NES_2.0.wiki): "an 8
/// KiB INST ROM, 16 bytes of PROM Data, 16 bytes of PROM Counter Out data".
/// "The two required PROM sections are missing in older ROM images"
/// (PC10_ROM-Images.wiki): then `prom` is None. None for an image without an
/// INST ROM.
pub fn game_roms(image: &RomImage) -> Option<GameRoms> {
    let misc = &image.misc;
    if !is_playchoice(image) || misc.len() < INST_SIZE {
        return None;
    }
    let (inst, rest) = misc.split_at(INST_SIZE);
    if rest.len() < 2 * PROM_SIZE {
        return Some(GameRoms {
            inst: inst.to_vec(),
            prom: None,
            counter_out: COUNTER_OUT.to_vec(),
        });
    }
    Some(GameRoms {
        inst: inst.to_vec(),
        prom: Some(rest[..PROM_SIZE].to_vec()),
        counter_out: rest[PROM_SIZE..2 * PROM_SIZE].to_vec(),
    })
}

/// One player's stick and buttons on the panel's 74LS165 (module notes).
///
/// `masked` is the supervisor's /CNTRL MASK held low: Start and Game Select
/// read released. It is false on a game board run alone (`System` on the
/// `playchoice` row), as if the supervisor had let go.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlayChoicePad {
    /// Pressed buttons, the controller's bits.
    pub buttons: u8,
    pub shift: u8,
    pub strobe: bool,
    wired: u8,
    pub masked: bool,
}

impl PlayChoicePad {
    pub fn new(select_start: bool) -> PlayChoicePad {
        PlayChoicePad {
            buttons: 0,
            shift: 0,
            strobe: false,
            wired: if select_start {
                0xFF
            } else {
                !(SELECT | START)
            },
            masked: false,
        }
    }

    fn inputs(&self) -> u8 {
        let mut pressed = self.buttons & self.wired;
        if self.masked {
            pressed &= !(SELECT | START);
        }
        pressed
    }

    pub fn set_strobe(&mut self, high: bool) {
        if high || self.strobe {
            self.shift = self.inputs();
        }
        self.strobe = high;
    }

    pub fn read(&self) -> u8 {
        (if self.strobe {
            self.inputs()
        } else {
            self.shift
        }) & 1
    }

    pub fn clock(&mut self) {
        if self.strobe {
            self.shift = self.inputs();
        } else {
            self.shift = (self.shift >> 1) | 0x80; // the grounded serial input, inverted
        }
    }
}

impl PortDevice for PlayChoicePad {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        PlayChoicePad::set_strobe(self, high);
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        PlayChoicePad::read(self)
    }
    fn clock(&mut self, _: &Ppu) {
        PlayChoicePad::clock(self);
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The cabinet's light gun: D3 and D4 of $4017, beside player 2's D0.
///
/// The sensor and trigger are the NES Zapper's model (module notes);
/// /CNTRL MASK holds the trigger released.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlayChoiceGun {
    pub zapper: Zapper,
    pub pad: PlayChoicePad,
}

impl PlayChoiceGun {
    pub fn new(ppu: &Ppu, palette: &[[u8; 3]], pad: PlayChoicePad) -> PlayChoiceGun {
        PlayChoiceGun {
            zapper: Zapper::new(ppu, palette),
            pad,
        }
    }

    pub fn masked(&self) -> bool {
        self.pad.masked
    }

    pub fn set_masked(&mut self, value: bool) {
        self.pad.masked = value;
    }

    pub fn read(&self, ppu: &Ppu) -> u8 {
        let mut gun = self.zapper.bits(ppu);
        if self.pad.masked {
            gun &= !0x10;
        }
        gun | self.pad.read()
    }
}

impl PortDevice for PlayChoiceGun {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        self.pad.set_strobe(high);
    }
    fn read(&mut self, ppu: &Ppu) -> u8 {
        PlayChoiceGun::read(self, ppu)
    }
    fn clock(&mut self, _: &Ppu) {
        self.pad.clock();
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
