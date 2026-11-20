// SPDX-License-Identifier: MIT
//! The latch-adder-inverter protection chips and their boards: TXC's
//! 05-00002-010 (36, 132, 173) and the JV001 (136, 147, 172) (`txc.py`).
//!
//! Each from its NESdev wiki page (INES_Mapper_036/132/136/147/172/173.wiki);
//! no judge ROM exists, the claims rest on sweeps against Mesen2.
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 36: `ProtectedLatch`
//! - 132: `ProtectedLatch`
//! - 136: `ProtectedLatch`
//! - 147: `ProtectedLatch`
//! - 172: `ProtectedLatch`
//! - 173: `ProtectedLatch`

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// D0-D5 in reverse order (mapper 172's board).
pub fn reverse6(value: u8) -> u8 {
    (0..6).map(|bit| ((value >> bit) & 1) << (5 - bit)).sum()
}

/// The 05-00002-010: 3-bit P, R and output, S, increment (C) and invert (V).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TxcChip {
    pub p: u8,
    pub r: u8,
    pub output: u8,
    pub s: u8,
    pub increment: u8,
    pub invert: u8,
}

impl TxcChip {
    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.r = if self.increment != 0 {
                    (self.r + 1) & 7
                } else {
                    self.p ^ if self.invert != 0 { 7 } else { 0 }
                }
            }
            1 => self.invert = value & 1,
            2 => {
                self.s = (value >> 3) & 1;
                self.p = value & 7;
            }
            _ => self.increment = value & 1,
        }
    }

    pub fn read(&self) -> u8 {
        self.r | ((self.s ^ self.invert) << 3)
    }
}

/// The JV001: 6-bit input, register and output, mode and invert.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Jv001 {
    pub input: u8,
    pub register: u8,
    pub output: u8,
    pub mode: u8,
    pub invert: u8,
}

impl Jv001 {
    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                if self.mode != 0 {
                    self.register = (self.register & 0x30) | ((self.register + 1) & 0x0F);
                } else {
                    self.register = self.input ^ if self.invert != 0 { 0x0F } else { 0 };
                }
            }
            1 => self.invert = value & 1,
            2 => self.input = value & 0x3F,
            _ => self.mode = value & 1,
        }
    }

    pub fn read(&self) -> u8 {
        self.register ^ if self.invert != 0 { 0x30 } else { 0 }
    }
}

/// Either chip (the reference's duck-typed `self.chip`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Chip {
    Txc(TxcChip),
    Jv001(Jv001),
}

impl Chip {
    fn write(&mut self, register: u16, value: u8) {
        match self {
            Chip::Txc(c) => c.write(register, value),
            Chip::Jv001(c) => c.write(register, value),
        }
    }
    fn read(&self) -> u8 {
        match self {
            Chip::Txc(c) => c.read(),
            Chip::Jv001(c) => c.read(),
        }
    }
    fn latch(&mut self) {
        match self {
            Chip::Txc(c) => c.output = c.r,
            Chip::Jv001(c) => c.output = c.register,
        }
    }
    fn output(&self) -> u8 {
        match self {
            Chip::Txc(c) => c.output,
            Chip::Jv001(c) => c.output,
        }
    }
    fn invert(&self) -> u8 {
        match self {
            Chip::Txc(c) => c.invert,
            Chip::Jv001(c) => c.invert,
        }
    }
    fn set_invert(&mut self, value: u8) {
        match self {
            Chip::Txc(c) => c.invert = value,
            Chip::Jv001(c) => c.invert = value,
        }
    }
}

/// A board with one of the chips at $4100-$4103 (mask $E103) and its output
/// latched by writes to $8000-$FFFF, each board wiring the data bits and the
/// outputs its own way (the reference lists each board's wiring).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProtectedLatch {
    pub b: Banked,
    pub board: u16,
    pub chip: Chip,
    pub chr_latch: u8,
}

impl ProtectedLatch {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> ProtectedLatch {
        let board = image.board.mapper;
        let mut chip = if matches!(board, 36 | 132 | 173) {
            Chip::Txc(TxcChip::default())
        } else {
            Chip::Jv001(Jv001::default())
        };
        chip.set_invert(if board == 172 { 1 } else { 0 });
        let mut cart = ProtectedLatch {
            b: Banked::new(image, cpu_cycle_ticks),
            board,
            chip,
            chr_latch: 0,
        };
        cart.b.prg_ram_enabled = false;
        cart.update();
        cart
    }

    /// The written byte as the chip's data pins see it.
    fn chip_value(&self, value: u8) -> u8 {
        match self.board {
            36 => (value >> 4) & 3,
            147 => value >> 2,
            172 => reverse6(value & 0x3F),
            _ => value,
        }
    }

    fn update(&mut self) {
        let out = self.chip.output() as i64;
        let mut prg = 0;
        let chr8;
        match self.board {
            132 => {
                prg = (out >> 2) & 1;
                chr8 = out & 3;
            }
            173 => {
                chr8 =
                    (out & 1) | ((!(self.chip.invert() as i64) & 1) << 1) | (((out >> 1) & 1) << 2);
            }
            36 => {
                prg = out & 3;
                chr8 = self.chr_latch as i64;
            }
            136 => {
                prg = (out >> 4) & 1;
                chr8 = out & 7;
            }
            147 => {
                prg = (((out >> 5) & 1) << 1) | (out & 1);
                chr8 = (out >> 1) & 0x0F;
            }
            _ => chr8 = out & 3, // 172
        }
        self.b.map_prg(0, 32, prg);
        self.b.map_chr(0, 8, chr8);
    }
}

impl Cartridge for ProtectedLatch {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let board = self.board;
        if matches!(board, 36 | 132 | 173) && address & 0xE100 == 0x4100 {
            if board == 36 {
                return Some((((self.chip.read() & 3) as u16) << 4) | (0xCF << 8));
            }
            return Some(self.chip.read() as u16 | (0xF0 << 8));
        }
        if matches!(board, 136 | 147 | 172) && address & 0xE100 == 0x4100 {
            let value = self.chip.read() as u16;
            if board == 147 {
                return Some((value << 2) | (0x03 << 8));
            }
            if board == 172 {
                return Some(reverse6(value as u8) as u16 | (0xC0 << 8));
            }
            return Some(value | (0xC0 << 8));
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.chip.latch();
            if self.board == 172 {
                self.b.set_mirroring(if self.chip.invert() != 0 {
                    VERTICAL
                } else {
                    HORIZONTAL
                });
            }
            self.update();
        } else if self.board == 36 && address & 0xE200 == 0x4200 {
            self.chr_latch = value & 0x0F;
            self.update();
        } else if address & 0xE100 == 0x4100 {
            let register = address & 3;
            let mut data = self.chip_value(value);
            if matches!(register, 1 | 3) && self.board == 172 {
                data = (value >> 5) & 1;
            }
            self.chip.write(register, data);
            if self.board == 173 {
                self.update(); // V is CHR A14 at once
            }
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    match mapper {
        36 | 132 | 136 | 147 | 172 | 173 => {
            Ok(Box::new(ProtectedLatch::new(image, cpu_cycle_ticks)))
        }
        _ => Err(image),
    }
}
