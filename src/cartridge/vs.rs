// SPDX-License-Identifier: MIT
//! The Vs. System's own ROM sockets: iNES mapper 99 (`cartridge/vs.py`).
//!
//! A Vs. mainboard has no cartridge connector. Each side has six EPROM sockets,
//! four of program (1A-1D or 6A-6D, 8 KiB each, at $E000, $C000, $A000 and $8000)
//! and two of graphics, and the 2A03's OUT2 ($4016 D2) chooses the graphics socket
//! through "a simple 1-to-2 demultiplexer" (INES_Mapper_099.wiki; Vs._System.wiki
//! "Controller and CHR ROM bank"; specification tier). Games on a daughterboard
//! are the mappers they would be on a Famicom (1, 2, 67, 75, 206), on a board
//! with four-screen nametable RAM.
//!
//! The mainboard's 2 KiB of RAM at $6000-$7FFF and its coin counter are the
//! board's (`vs.rs`): nothing of them is here.
//!
//! Differences forced by Rust: the reference indexes `prg` with `address +
//! _prg_start`, which Python lets fall below 0 (an image of 16 KiB read at
//! $A000-$BFFF counts from the end); [`py_index`] does the same, and fails where
//! Python's index would.

use super::banked::Banked;
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

pub const BANK: usize = 0x2000;

/// Python's `sequence[index]` for an index that may be negative.
fn py_index(data: &[u8], index: i64) -> u8 {
    let len = data.len() as i64;
    let at = if index < 0 { index + len } else { index };
    assert!((0..len).contains(&at), "prg index {index} out of range");
    data[at as usize]
}

/// Mapper 99: unbanked PRG, and OUT2 selecting one of two 8 KiB CHR ROMs.
///
/// How an image's PRG lies (NES_2.0.wiki "PRG-ROM Area"): up to 32 KiB end at
/// $FFFF, "24 KiB (half-)blocks are mapped to $A000-$FFFF"; *Vs. Gumshoe*'s 40
/// KiB are the 32 KiB seen with OUT2 low and then the 8 KiB that OUT2 high
/// puts at $8000-$9FFF (its 16 KiB EPROM in socket 1D has OUT2 on A13).
///
/// "Unlike all other mappers, an undersize mapper 99 image implies open bus
/// instead of mirroring ... for games with only 8 KiB of CHR, when the bank
/// select bit is high the PPU will have no tile data to render"
/// (INES_Mapper_099.wiki): an empty socket drives nothing.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct VsSockets {
    pub b: Banked,
    /// The lowest address a socket answers.
    prg_base: i64,
    /// `prg[address + prg_start]`.
    prg_start: i64,
    /// Gumshoe's second $8000 bank, if the image has one.
    alternate: Option<usize>,
    pub out2: u8,
    chr_present: bool,
}

impl VsSockets {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> VsSockets {
        let size = image.prg.len().min(4 * BANK) as i64;
        let alternate = if image.prg.len() >= 5 * BANK {
            Some(4 * BANK)
        } else {
            None
        };
        let mut m = VsSockets {
            b: Banked::new(image, cpu_cycle_ticks),
            prg_base: 0x10000 - size,
            prg_start: -(0x10000 - size),
            alternate,
            out2: 0,
            chr_present: true,
        };
        m.b.prg_ram_enabled = false; // $6000-$7FFF is the mainboard's
        m.b.map_chr(0, 8, 0);
        m
    }
}

impl Cartridge for VsSockets {
    banked_field!(b);

    fn out(&mut self, value: u8) {
        self.out2 = (value >> 2) & 1;
        if !self.b.chr_writable {
            self.chr_present = (self.out2 as usize) * BANK < self.b.chr.len();
            if self.chr_present {
                self.b.map_chr(0, 8, self.out2 as i64);
            }
        }
    }

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        let a = address as i64;
        if address >= 0xA000 || (a >= self.prg_base && self.out2 == 0) {
            return Some(py_index(&self.b.prg, a + self.prg_start) as u16);
        }
        if address >= 0x8000 {
            if let (Some(alternate), true) = (self.alternate, self.out2 != 0) {
                return Some(self.b.prg[alternate + (address as usize & 0x1FFF)] as u16);
            }
            if a >= self.prg_base {
                return Some(py_index(&self.b.prg, a + self.prg_start) as u16);
            }
        }
        None
    }

    fn cpu_write(&mut self, _address: u16, _value: u8, _t: i64) {
        // ROM sockets only
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && !self.chr_present {
            return None;
        }
        self.b.base_ppu_read(address, t)
    }
}
