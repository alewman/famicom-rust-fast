// SPDX-License-Identifier: MIT
//! Tengen's RAMBO-1 (64): the MMC3 with more banks and a CPU-cycle counter
//! mode, and its TLS board (158) (`tengen.py`; RAMBO-1.wiki, judged by sweeps
//! against Mesen2).
//!
//! The reference's `RAMBO1TLS` subclasses `RAMBO1` only to record the raw CHR
//! banks; here one struct, the TLS board with `raw_banks`.

use super::banked::{HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::mmc3::{base_ppu_address, Mmc3, Mmc3Like};
use crate::mmc3_cartridge;

/// CPU cycles from the counter reaching 0 to /IRQ (RAMBO-1.wiki).
const SCANLINE_IRQ_DELAY: u8 = 2;
const CYCLE_IRQ_DELAY: u8 = 1;

/// Mapper 64: Tengen RAMBO-1; mapper 158 (Tengen 800037, Alien Syndrome):
/// the RAMBO-1 with CHR A17 as CIRAM A10.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rambo1 {
    pub m: Mmc3,
    pub cycle_mode: bool,
    pub prescaler: u8,
    pub irq_delay: u8,
    /// The sixteen registers (the reference's `registers`, widened after
    /// `MMC3.__init__`, whose `_update` call returns early).
    pub registers: [u8; 16],
    /// 158: the raw CHR bank of each 1 KiB window.
    pub raw_banks: Option<[i64; 8]>,
}

impl Rambo1 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Rambo1 {
        let tls = image.board.mapper == 158;
        let mut board = Rambo1 {
            m: Mmc3::new(image, cpu_cycle_ticks),
            cycle_mode: false,
            prescaler: 0,
            irq_delay: 0,
            registers: [0; 16],
            raw_banks: if tls { Some([0; 8]) } else { None },
        };
        board.m.b.prg_ram_enabled = false;
        board.update();
        board
    }
}

impl Mmc3Like for Rambo1 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn update(&mut self) {
        let r = self.registers;
        let select = self.m.bank_select;
        let last = (self.m.b.prg.len() / 0x2000) as i64 - 1;
        let (low, high) = if select & 0x40 != 0 {
            (r[15], r[6])
        } else {
            (r[6], r[15])
        };
        self.map_prg(0, 8, low as i64);
        self.map_prg(1, 8, r[7] as i64);
        self.map_prg(2, 8, high as i64);
        self.map_prg(3, 8, last);
        let a12 = if select & 0x80 != 0 { 4 } else { 0 };
        if select & 0x20 != 0 {
            for (i, bank) in [r[0], r[8], r[1], r[9]].into_iter().enumerate() {
                self.map_chr(a12 + i, 1, bank as i64);
            }
        } else {
            self.map_chr(a12, 2, (r[0] >> 1) as i64);
            self.map_chr(a12 + 2, 2, (r[1] >> 1) as i64);
        }
        for i in 0..4 {
            self.map_chr((a12 ^ 4) + i, 1, r[2 + i] as i64);
        }
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        if let Some(raw) = &mut self.raw_banks {
            for i in 0..size_kib {
                raw[window + i] = bank * size_kib as i64 + i as i64;
            }
        }
        self.m.b.map_chr(window, size_kib, bank);
    }
    fn clock_counter(&mut self) {
        let m = &mut self.m;
        if m.reload {
            m.counter = if m.latch != 0 { m.latch | 1 } else { 0 };
            m.reload = false;
        } else if m.counter == 0 {
            m.counter = m.latch;
        } else {
            m.counter -= 1;
        }
        if m.counter == 0 && m.irq_enabled {
            self.irq_delay = if self.cycle_mode {
                CYCLE_IRQ_DELAY
            } else {
                SCANLINE_IRQ_DELAY
            };
        }
    }
}

impl Cartridge for Rambo1 {
    mmc3_cartridge!();

    fn counts_m2(&self) -> bool {
        true
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address < 0x8000 {
            return;
        }
        let register = address & 0xE001;
        if register == 0x8000 {
            self.m.bank_select = value;
            self.update();
        } else if register == 0x8001 {
            self.registers[(self.m.bank_select & 0x0F) as usize] = value;
            self.update();
        } else if register == 0xA000 {
            self.m
                .b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if register == 0xC000 {
            self.m.latch = value;
        } else if register == 0xC001 {
            self.cycle_mode = value & 1 != 0;
            self.prescaler = 0;
            self.m.reload = true;
        } else if register == 0xE000 {
            self.m.irq_enabled = false;
            self.m.irq = false;
            self.irq_delay = 0;
        } else {
            self.m.irq_enabled = true;
        }
    }

    fn ppu_address(&mut self, address: u16, t: i64) {
        if !self.cycle_mode {
            base_ppu_address(self, address, t);
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.irq_delay != 0 {
            self.irq_delay -= 1;
            if self.irq_delay == 0 {
                self.m.irq = true;
            }
        }
        if self.cycle_mode {
            self.prescaler = (self.prescaler + 1) & 3;
            if self.prescaler == 0 {
                self.clock_counter();
            }
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        match &self.raw_banks {
            Some(raw) => Some(((raw[((address >> 10) & 3) as usize] >> 7) & 1) as u8),
            None => self.m.b.base_ciram_a10(address),
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
        64 | 158 => Ok(Box::new(Rambo1::new(image, cpu_cycle_ticks))),
        _ => Err(image),
    }
}
