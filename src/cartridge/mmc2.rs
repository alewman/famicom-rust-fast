// SPDX-License-Identifier: MIT
//! MMC2 (mapper 9) and MMC4 (mapper 10): CHR banks that the PPU's own fetches
//! switch (`mmc2.py`; MMC2.wiki and MMC4.wiki, judged by Holy Mapperel).
//!
//! The reference's `MMC4` subclasses `MMC2` with two class constants; here
//! they are fields.

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// Mapper 9: 8 KiB PRG at $8000 (the last three fixed), two latched pairs of
/// 4 KiB CHR banks. Mapper 10 (MMC4): 16 KiB PRG banking, both latches over a
/// range.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc2 {
    pub b: Banked,
    pub prg_bank_kib: usize,
    pub latch0_range: u16,
    /// $FD/0000, $FE/0000, $FD/1000, $FE/1000.
    pub chr_registers: [u8; 4],
    /// 0 = $FD, 1 = $FE.
    pub latches: [usize; 2],
}

impl Mmc2 {
    #[allow(clippy::self_named_constructors)] // the reference's class names: MMC2 and MMC4
    pub fn mmc2(image: RomImage, cpu_cycle_ticks: i64) -> Mmc2 {
        Mmc2::build(image, cpu_cycle_ticks, 8, 1)
    }

    pub fn mmc4(image: RomImage, cpu_cycle_ticks: i64) -> Mmc2 {
        Mmc2::build(image, cpu_cycle_ticks, 16, 8)
    }

    fn build(
        image: RomImage,
        cpu_cycle_ticks: i64,
        prg_bank_kib: usize,
        latch0_range: u16,
    ) -> Mmc2 {
        let mut m = Mmc2 {
            b: Banked::new(image, cpu_cycle_ticks),
            prg_bank_kib,
            latch0_range,
            chr_registers: [0; 4],
            latches: [1, 1],
        };
        let last = (m.b.prg.len() / 0x2000) as i64 - 1;
        if prg_bank_kib == 8 {
            for window in 1..4 {
                m.b.map_prg(window, 8, last - 3 + window as i64);
            }
        } else {
            m.b.map_prg(2, 16, last.div_euclid(2));
        }
        m.b.map_prg(0, prg_bank_kib, 0);
        m.update_chr();
        m
    }

    fn update_chr(&mut self) {
        let r = self.chr_registers;
        self.b.map_chr(0, 4, r[self.latches[0]] as i64);
        self.b.map_chr(4, 4, r[2 + self.latches[1]] as i64);
    }
}

impl Cartridge for Mmc2 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        if region == 0xA000 {
            self.b.map_prg(0, self.prg_bank_kib, (value & 0x0F) as i64);
        } else if region == 0xF000 {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if region >= 0xB000 {
            self.chr_registers[((region >> 12) - 0xB) as usize] = value & 0x1F;
            self.update_chr();
        }
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        let value = self.b.base_ppu_read(address, t);
        if address < 0x2000 && matches!(address & 0x0FF0, 0x0FD0 | 0x0FE0) {
            let low = address & 0x0F;
            let high = (address >> 12) as usize;
            if low >= 8 && (high != 0 || low < 8 + self.latch0_range) {
                let latch = if address & 0x0020 != 0 { 1 } else { 0 };
                if self.latches[high] != latch {
                    self.latches[high] = latch;
                    self.update_chr();
                }
            }
        }
        value
    }
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    Ok(match mapper {
        9 => Box::new(Mmc2::mmc2(image, cpu_cycle_ticks)),
        10 => Box::new(Mmc2::mmc4(image, cpu_cycle_ticks)),
        _ => return Err(image),
    })
}
