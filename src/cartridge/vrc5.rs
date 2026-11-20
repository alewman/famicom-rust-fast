// SPDX-License-Identifier: MIT
//! Konami's VRC5 in the Q-Ta adapter (mapper 547): NHK Gakuen's Space School
//! and Space College (`vrc5.py`, from NES_2.0_Mapper_547.wiki). No judge ROM
//! exists; the claims rest on looking (docs/claims.md).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 547: `QTa`

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    match mapper {
        547 => Ok(Box::new(QTa::new(image, cpu_cycle_ticks))),
        _ => Err(image),
    }
}

/// JIS X 0208 rows' 256-code pages to Kanji ROM pages (the page's pageTable,
/// which it says FCEUX replicates from the hardware).
#[rustfmt::skip]
pub const PAGES: [i64; 36] = [
    0x0, 0x0, 0x2, 0x2, 0x1, 0x1, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF,
    0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF, 0xD, 0xD,
];

/// 128 KiB of 8 KiB banks, before the cartridge's 512 KiB.
pub const INTERNAL_PRG_BANKS: i64 = 16;

/// The kanji ROM tile of a glyph's 8x8 corner, from its JIS X 0208 code.
pub fn kanji_tile(column: i64, row: i64, position: i64) -> i64 {
    let (row, column) = ((row - 0x21) & 0x7F, (column - 0x21) & 0x7F);
    let code = (column % 32 + (row % 16) * 32 + (column / 32) * 32 * 16 + (row / 16) * 32 * 16 * 3)
        % 0x2400;
    (((code & 0xFF) | (PAGES[(code >> 8) as usize] << 8)) * 4) | position
}

/// Mapper 547: PRG, RAM and an IRQ like any Konami, and two banks for every
/// tile (QTRAM picks, per background tile, the kanji ROM or CHR-RAM).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QTa {
    pub b: Banked,
    pub kanji: Vec<u8>,
    pub chr_ram: Vec<u8>,
    pub qtram: Vec<u8>,
    pub ram_banks: [u8; 2],
    pub sprite_bank: usize,
    pub latch: u32,
    pub counter: u32,
    pub irq_enable: bool,
    pub irq_after: bool,
    pub to_qtram: bool,
    pub attribute: u8,
    pub jis_column: u8,
    pub jis_row: u8,
    pub qt_byte: u8,
    pub background: bool,
    pub irq: bool,
}

impl QTa {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> QTa {
        let kanji = image.chr.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram = vec![0; 0x4000];
        b.prg_ram_mask = 0x0FFF;
        b.map_prg(0, 8, 0);
        b.map_prg(1, 8, 0);
        b.map_prg(2, 8, 0);
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        b.set_mirroring(VERTICAL);
        QTa {
            b,
            kanji,
            chr_ram: vec![0; 0x2000],
            qtram: vec![0; 0x800],
            ram_banks: [0, 0],
            sprite_bank: 0,
            latch: 0,
            counter: 0,
            irq_enable: false,
            irq_after: false,
            to_qtram: false,
            attribute: 0,
            jis_column: 0,
            jis_row: 0,
            qt_byte: 0,
            background: false,
            irq: false,
        }
    }

    fn ram_offset(&self, address: u16) -> usize {
        let bank = self.ram_banks[(address as usize >> 12) & 1] as usize;
        (((bank & 8) << 10) | ((bank & 1) << 12)) + (address as usize & 0x0FFF)
    }

    fn tile(&self) -> i64 {
        kanji_tile(
            self.jis_column as i64,
            self.jis_row as i64,
            (self.attribute & 3) as i64,
        )
    }

    fn register(&mut self, register: u16, value: u8) {
        if register < 2 {
            self.ram_banks[register as usize] = value;
        } else if register < 5 {
            let bank = (value & 0x3F) as i64;
            let bank = if value & 0x40 != 0 {
                INTERNAL_PRG_BANKS + bank
            } else {
                bank % INTERNAL_PRG_BANKS
            };
            self.b.map_prg(register as usize - 2, 8, bank);
        } else if register == 5 {
            self.sprite_bank = (value & 1) as usize;
        } else if register == 6 {
            self.latch = (self.latch & 0xFF00) | value as u32;
        } else if register == 7 {
            self.latch = (self.latch & 0x00FF) | (value as u32) << 8;
        } else if register == 8 {
            self.irq = false;
            self.irq_enable = self.irq_after;
        } else if register == 9 {
            self.irq = false;
            self.irq_enable = value & 2 != 0;
            self.irq_after = value & 1 != 0;
            if self.irq_enable {
                self.counter = self.latch;
            }
        } else if register == 0x0A {
            self.to_qtram = value & 1 != 0;
            self.b
                .set_mirroring(if value & 2 != 0 { HORIZONTAL } else { VERTICAL });
        } else if register == 0x0B {
            self.attribute = value & 7;
        } else if register == 0x0C {
            self.jis_column = value & 0x7F;
        } else if register == 0x0D {
            self.jis_row = value & 0x7F;
        }
    }

    fn chr_ram_offset(&self, address: u16) -> usize {
        if self.background {
            return ((self.qt_byte as usize & 1) << 12) | (address as usize & 0x0FFF);
        }
        let bank = if address & 0x1000 != 0 {
            1
        } else {
            self.sprite_bank
        };
        (bank << 12) | (address as usize & 0x0FFF)
    }
}

impl Cartridge for QTa {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(self.b.prg_ram[self.ram_offset(address)] as u16);
        }
        if address & 0xFF00 == 0xDC00 {
            return Some((self.tile() & 0xFF) as u16);
        }
        if address & 0xFF00 == 0xDD00 {
            let tile = self.tile();
            return Some(((tile >> 8) | 0x40 | ((self.attribute as i64 & 4) << 5)) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            let offset = self.ram_offset(address);
            self.b.prg_ram[offset] = value;
        } else if address & 0xF000 == 0xD000 {
            self.register((address >> 8) & 0x0F, value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.irq_enable {
            self.counter = (self.counter + 1) & 0xFFFF;
            if self.counter == 0 {
                self.counter = self.latch;
                self.irq = true;
            }
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        let a10 = self.b.base_ciram_a10(address);
        if let Some(a10) = a10 {
            self.background = address & 0x3FF >= 0x3C0;
            if !self.background {
                self.qt_byte = self.qtram[((a10 as usize) << 10) | (address as usize & 0x3FF)];
            }
        }
        if self.to_qtram {
            None
        } else {
            a10
        }
    }

    fn ppu_read(&mut self, address: u16, _t: i64) -> Option<u8> {
        if address >= 0x2000 {
            let a10 = self.b.base_ciram_a10(address).unwrap_or(0) as usize;
            return Some(self.qtram[(a10 << 10) | (address as usize & 0x3FF)]);
        }
        if self.background && self.qt_byte & 0x40 != 0 {
            if address & 8 != 0 {
                return Some(if self.qt_byte & 0x80 != 0 { 0xFF } else { 0 });
            }
            let a = address as usize;
            let line = ((a & 7) << 1) | ((a >> 4) & 1) | ((a & 0x0FE0) >> 1);
            let index = (line | ((self.qt_byte as usize & 0x3F) << 11)) % self.kanji.len();
            return Some(self.kanji[index]);
        }
        Some(self.chr_ram[self.chr_ram_offset(address)])
    }

    fn ppu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x2000 {
            let a10 = self.b.base_ciram_a10(address).unwrap_or(0) as usize;
            self.qtram[(a10 << 10) | (address as usize & 0x3FF)] = value;
        } else {
            self.background = false;
            let offset = self.chr_ram_offset(address);
            self.chr_ram[offset] = value;
        }
    }
}
