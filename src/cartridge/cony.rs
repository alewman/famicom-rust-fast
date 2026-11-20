// SPDX-License-Identifier: MIT
//! Cony's (Yoko's) chip-on-board ASIC on its boards: mapper 83 (submappers
//! 0-3) and NES 2.0 264 (`cony.py`).
//!
//! INES_Mapper_083.wiki. No judge ROM exists; the claims rest on sweeps
//! against Mesen2, a detector.
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 83: `Cony`
//! - 264: `Cony`

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// 83.1 and 264 bank CHR in 2 KiB amounts: for each real 1 KiB window, the ASIC's register.
const TWO_KIB_CHR: [usize; 8] = [0, 0, 1, 1, 6, 6, 7, 7];

/// Mappers 83.0 (256 KiB CHR), 83.1 (512 KiB, 2 KiB CHR banks), 83.2 (4-in-1
/// with 32 KiB of PRG-NVRAM), 83.3 (4-in-1) and 264 (2-in-1; the ASIC's A9-A8
/// are CPU A11-A10). The registers are described in the reference.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Cony {
    pub b: Banked,
    pub remapped: bool,
    pub submapper: u8,
    pub two_kib_chr: bool,
    pub inner: i64,
    pub nvram: bool,
    pub base: u8,
    pub mode: u8,
    pub prg_regs: [u8; 4],
    pub chr_regs: [u8; 8],
    pub counter: u16,
    pub counting: bool,
    pub a12_clock: bool,
    pub a12: u8,
    pub scratch: [u8; 4],
    pub irq: bool,
}

impl Cony {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Cony {
        let board = image.board.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let remapped = board.mapper == 264;
        let submapper = if remapped { 0 } else { board.submapper };
        let nvram = submapper == 2;
        if nvram {
            b.prg_ram = vec![0; b.prg_ram.len().max(0x8000)];
        }
        b.prg_ram_enabled = false;
        let mut cart = Cony {
            b,
            remapped,
            submapper,
            two_kib_chr: remapped || submapper == 1,
            inner: if remapped || submapper == 3 {
                0x07
            } else {
                0x0F
            },
            nvram,
            base: 0,
            mode: 0,
            prg_regs: [0; 4],
            chr_regs: [0; 8],
            counter: 0,
            counting: false,
            a12_clock: false,
            a12: 0,
            scratch: [0; 4],
            irq: false,
        };
        cart.update();
        cart
    }

    /// (the PRG base in 16 KiB banks, CHR A19-A18 as a 1 KiB bank offset).
    fn base16(&self) -> (i64, i64) {
        let b = self.base as i64;
        match self.submapper {
            2 => ((((b >> 4) & 3) << 4) | (b & 0x0F), ((b >> 4) & 3) << 8),
            3 => (b & 0x3F, (b >> 6) << 8),
            _ => (b, 0),
        }
    }

    fn update(&mut self) {
        let (base16, chr_outer) = self.base16();
        let inner16 = self.inner;
        let prg_mode = (self.mode >> 3) & 3;
        let outer16 = base16 & !inner16;
        if prg_mode == 0 {
            self.b.map_prg(0, 16, base16);
            self.b.map_prg(2, 16, outer16 | inner16);
        } else if prg_mode == 1 {
            self.b.map_prg(0, 16, base16);
            self.b.map_prg(2, 16, base16);
        } else {
            let inner8 = (inner16 << 1) | 1;
            let outer8 = outer16 << 1;
            for window in 0..3 {
                self.b
                    .map_prg(window, 8, outer8 | (self.prg_regs[window] as i64 & inner8));
            }
            self.b.map_prg(3, 8, outer8 | inner8);
        }
        self.b
            .set_mirroring([VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B][(self.mode & 3) as usize]);
        #[allow(clippy::needless_range_loop)] // window is the bank number too
        for window in 0..8 {
            let bank = if self.two_kib_chr {
                self.chr_regs[TWO_KIB_CHR[window]] as i64 * 2 + (window as i64 & 1)
            } else {
                self.chr_regs[window] as i64
            };
            self.b.map_chr(window, 1, chr_outer | bank);
        }
    }

    /// 264's address as 83's: its CPU A11-A10 are the ASIC's A9-A8.
    fn address(&self, address: u16) -> u16 {
        if self.remapped {
            return (address & 0xF0FF) | (((address >> 10) & 3) << 8);
        }
        address
    }

    fn clock(&mut self) {
        if !self.counting {
            return;
        }
        self.counter = if self.mode & 0x40 != 0 {
            self.counter.wrapping_sub(1)
        } else {
            self.counter.wrapping_add(1)
        };
        if self.counter == 0 {
            self.counting = false;
            self.irq = true;
        }
    }
}

impl Cartridge for Cony {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let a = self.address(address);
        if a & 0xF100 == 0x5000 && a < 0x6000 {
            return Some(0xFC << 8);
        }
        if a & 0xF100 == 0x5100 && a < 0x6000 {
            return Some(self.scratch[(a & 3) as usize] as u16);
        }
        if (0x6000..0x8000).contains(&address) {
            if self.nvram {
                let bank = ((self.base >> 6) & 3) as usize;
                return Some(self.b.prg_ram[bank * 0x2000 + (address as usize & 0x1FFF)] as u16);
            }
            if self.mode & 0x20 != 0 {
                let (base16, _) = self.base16();
                let outer8 = (base16 & !self.inner) << 1;
                let bank = outer8 | (self.prg_regs[3] as i64 & ((self.inner << 1) | 1));
                let len = self.b.prg.len() as i64;
                let offset = (bank * 0x2000 + (address as i64 & 0x1FFF)).rem_euclid(len);
                return Some(self.b.prg[offset as usize] as u16);
            }
            return None;
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let a = self.address(address);
        if address < 0x8000 {
            if a & 0xF100 == 0x5100 && a < 0x6000 {
                self.scratch[(a & 3) as usize] = value;
            } else if self.nvram && address >= 0x6000 {
                let bank = ((self.base >> 6) & 3) as usize;
                self.b.prg_ram[bank * 0x2000 + (address as usize & 0x1FFF)] = value;
            }
            return;
        }
        if a & 0x8300 == 0x8000 {
            self.base = value;
        } else if a & 0x8300 == 0x8100 {
            self.mode = value;
        } else if a & 0x8301 == 0x8200 {
            self.counter = (self.counter & 0xFF00) | value as u16;
            self.irq = false;
        } else if a & 0x8301 == 0x8201 {
            self.counter = (self.counter & 0x00FF) | ((value as u16) << 8);
            self.counting = self.mode & 0x80 != 0;
        } else if a & 0x8318 == 0x8318 {
            self.a12_clock = value != 0;
        } else if a & 0x8318 == 0x8310 {
            self.chr_regs[(a & 7) as usize] = value;
        } else if a & 0x8310 == 0x8300 {
            self.prg_regs[(a & 3) as usize] = value;
        } else {
            return;
        }
        self.update();
    }

    fn m2(&mut self, _t: i64) {
        if !self.a12_clock {
            self.clock();
        }
    }

    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a12 = ((address >> 12) & 1) as u8;
        if self.a12_clock && a12 != 0 && self.a12 == 0 {
            self.clock();
        }
        self.a12 = a12;
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.nvram {
            Some(self.b.prg_ram.clone())
        } else {
            None
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
        83 | 264 => Ok(Box::new(Cony::new(image, cpu_cycle_ticks))),
        _ => Err(image),
    }
}
