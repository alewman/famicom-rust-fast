// SPDX-License-Identifier: MIT
//! Taito's mappers: TC0190 (33), TC0690 (48), X1-005 (80, 207) and X1-017
//! (82, 552) (`taito.py`; no judge ROM, the claims rest on sweeps against
//! Mesen2).

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::mmc3::{base_clock_counter, Mmc3, Mmc3Like};
use crate::{banked_field, mmc3_cartridge};

/// TC0690's /IRQ lags the MMC3's: "about 4 CPU cycles" (INES_Mapper_048.wiki).
const TC0690_IRQ_DELAY: u8 = 4;

// -- TC0190 --------------------------------------------------------------------

/// Mapper 33: two 8 KiB PRG banks and the last two fixed; CHR 2 KiB x 2 and
/// 1 KiB x 4 (INES_Mapper_033.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tc0190 {
    pub b: Banked,
}

impl Tc0190 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Tc0190 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(2, 8, last - 1);
        b.map_prg(3, 8, last);
        Tc0190 { b }
    }
}

impl Cartridge for Tc0190 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if !(0x8000..0xC000).contains(&address) {
            return;
        }
        let register = address & 0xA003;
        let b = &mut self.b;
        if register == 0x8000 {
            b.map_prg(0, 8, (value & 0x3F) as i64);
            b.set_mirroring(if value & 0x40 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        } else if register == 0x8001 {
            b.map_prg(1, 8, (value & 0x3F) as i64);
        } else if register == 0x8002 || register == 0x8003 {
            b.map_chr(((register & 1) * 2) as usize, 2, value as i64);
        } else {
            b.map_chr(4 + (register & 3) as usize, 1, value as i64);
        }
    }
}

// -- TC0690 --------------------------------------------------------------------

/// Mapper 48: Taito TC0690: TC0190's banks, the MMC3's scanline counter, a
/// later /IRQ (INES_Mapper_048.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tc0690 {
    pub m: Mmc3,
    pub irq_delay: u8,
}

impl Tc0690 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Tc0690 {
        let mut board = Tc0690 {
            m: Mmc3::new(image, cpu_cycle_ticks),
            irq_delay: 0,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        board
    }
}

impl Mmc3Like for Tc0690 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn update(&mut self) {
        let r = self.m.registers;
        let last = (self.m.b.prg.len() / 0x2000) as i64 - 1;
        self.map_prg(0, 8, (r[0] & 0x3F) as i64);
        self.map_prg(1, 8, (r[1] & 0x3F) as i64);
        self.map_prg(2, 8, last - 1);
        self.map_prg(3, 8, last);
        self.map_chr(0, 2, r[2] as i64);
        self.map_chr(2, 2, r[3] as i64);
        for i in 0..4 {
            self.map_chr(4 + i, 1, r[4 + i] as i64);
        }
    }
    fn clock_counter(&mut self) {
        let was_pending = self.m.irq;
        base_clock_counter(&mut self.m);
        if self.m.irq && !was_pending {
            self.m.irq = false;
            self.irq_delay = TC0690_IRQ_DELAY;
        }
    }
}

impl Cartridge for Tc0690 {
    mmc3_cartridge!(ppu_address);

    fn counts_m2(&self) -> bool {
        true
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address < 0x8000 {
            return;
        }
        let register = address & 0xE003;
        if register < 0xC000 {
            let index = (register & 3) as usize + if register >= 0xA000 { 4 } else { 0 };
            self.m.registers[index] = value;
            self.update();
        } else if register == 0xC000 {
            self.m.latch = value ^ 0xFF;
        } else if register == 0xC001 {
            self.m.counter = 0;
            self.m.reload = true;
        } else if register == 0xC002 {
            self.m.irq_enabled = true;
        } else if register == 0xC003 {
            self.m.irq_enabled = false;
            self.m.irq = false;
            self.irq_delay = 0;
        } else if register == 0xE000 {
            self.m.b.set_mirroring(if value & 0x40 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.irq_delay != 0 {
            self.irq_delay -= 1;
            if self.irq_delay == 0 {
                self.m.irq = true;
            }
        }
    }
}

// -- X1-005 --------------------------------------------------------------------

/// Mappers 80 and 207: Taito X1-005: 8 KiB PRG banks, 2 KiB and 1 KiB CHR,
/// 128 bytes of RAM (Taito_X1-005.wiki; 207: INES_Mapper_207.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct X1005 {
    pub b: Banked,
    pub cart_nametables: bool,
    pub ram: Vec<u8>,
    pub ram_enabled: bool,
    pub screens: [u8; 2],
}

impl X1005 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> X1005 {
        let cart_nametables = image.board.mapper == 207;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        X1005 {
            b,
            cart_nametables,
            ram: vec![0; 128],
            ram_enabled: false,
            screens: [0, 0],
        }
    }
}

impl Cartridge for X1005 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x7F00..0x8000).contains(&address) {
            return if self.ram_enabled {
                Some(self.ram[address as usize & 0x7F] as u16)
            } else {
                None
            };
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x7F00..0x8000).contains(&address) {
            if self.ram_enabled {
                self.ram[address as usize & 0x7F] = value;
            }
            return;
        }
        if address & 0xFF70 != 0x7E70 {
            return;
        }
        let register = (address & 0x0F) as usize;
        if register < 2 {
            let bank = if self.cart_nametables {
                value & 0x7F
            } else {
                value
            } >> 1;
            self.b.map_chr(register * 2, 2, bank as i64);
            self.screens[register] = value >> 7;
        } else if register < 6 {
            self.b.map_chr(register + 2, 1, value as i64);
        } else if register == 6 {
            if !self.cart_nametables {
                self.b
                    .set_mirroring(if value & 1 != 0 { VERTICAL } else { HORIZONTAL });
            }
        } else if register == 8 || register == 9 {
            self.ram_enabled = value == 0xA3;
        } else if register >= 0x0A {
            self.b
                .map_prg((register - 0x0A) >> 1, 8, (value & 0x3F) as i64);
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        if self.cart_nametables {
            return Some(self.screens[((address >> 11) & 1) as usize]);
        }
        self.b.base_ciram_a10(address)
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.b.battery {
            Some(self.ram.clone())
        } else {
            None
        }
    }

    fn restore(&mut self, data: &[u8]) {
        self.ram.copy_from_slice(data);
    }
}

// -- X1-017 --------------------------------------------------------------------

/// Mappers 82 and 552: Taito X1-017: 8 KiB PRG banks, CHR with A12
/// inversion, 5 KiB of RAM, an IRQ (Taito_X1-017.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct X1017 {
    pub b: Banked,
    pub reversed_prg: bool,
    pub ram: Vec<u8>,
    pub ram_enables: [bool; 3],
    pub chr_registers: [u8; 6],
    pub chr_mode: u8,
    pub latch: u8,
    pub control: u8,
    pub counter: i64,
    pub irq: bool,
}

impl X1017 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> X1017 {
        let reversed_prg = image.board.mapper == 552;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let mut board = X1017 {
            b,
            reversed_prg,
            ram: vec![0; 0x1400],
            ram_enables: [false; 3],
            chr_registers: [0; 6],
            chr_mode: 0,
            latch: 0,
            control: 0,
            counter: 0,
            irq: false,
        };
        let last = (board.b.prg.len() / 0x2000) as i64 - 1;
        board.b.map_prg(3, 8, last);
        board.map_chr_registers();
        board
    }

    /// `_map_chr`.
    fn map_chr_registers(&mut self) {
        let r = self.chr_registers;
        let low = if self.chr_mode != 0 { 4 } else { 0 };
        self.b.map_chr(low, 2, (r[0] >> 1) as i64);
        self.b.map_chr(low + 2, 2, (r[1] >> 1) as i64);
        for i in 0..4 {
            self.b.map_chr((low ^ 4) + i, 1, r[2 + i] as i64);
        }
    }

    fn ram_index(&self, address: u16) -> Option<usize> {
        if (0x6000..0x7400).contains(&address)
            && self.ram_enables[(((address - 0x6000) >> 11) as usize).min(2)]
        {
            return Some((address - 0x6000) as usize);
        }
        None
    }

    fn prg_bank(&self, value: u8) -> i64 {
        if self.reversed_prg {
            return (0..6)
                .map(|bit| (((value >> bit) & 1) as i64) << (5 - bit))
                .sum();
        }
        ((value >> 2) & 0x0F) as i64
    }
}

impl Cartridge for X1017 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 {
            return self.b.base_cpu_read(address, t);
        }
        match self.ram_index(address) {
            None => Some(0),
            Some(index) => Some(self.ram[index] as u16),
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if let Some(index) = self.ram_index(address) {
            self.ram[index] = value;
            return;
        }
        if !(0x7EF0..=0x7EFF).contains(&address) {
            return;
        }
        let register = (address & 0x0F) as usize;
        if register < 6 {
            self.chr_registers[register] = value;
            self.map_chr_registers();
        } else if register == 6 {
            self.b
                .set_mirroring(if value & 1 != 0 { VERTICAL } else { HORIZONTAL });
            self.chr_mode = value & 2;
            self.map_chr_registers();
        } else if register <= 9 {
            self.ram_enables[register - 7] = value == [0xCA, 0x69, 0x84][register - 7];
        } else if register <= 0x0C {
            let bank = self.prg_bank(value);
            self.b.map_prg(register - 0x0A, 8, bank);
        } else if register == 0x0D {
            self.latch = value;
        } else if register == 0x0E {
            self.control = value & 7;
            if value & 1 == 0 {
                self.counter = if self.latch != 0 {
                    (self.latch as i64 + 2) * 16
                } else {
                    17
                };
            }
            self.irq = self.irq && value & 2 != 0;
        } else {
            self.irq = false;
            self.counter = if self.latch != 0 {
                (self.latch as i64 + 1) * 16
            } else {
                1
            };
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.control & 5 == 1 && self.counter != 0 {
            self.counter -= 1;
            if self.counter == 0 && self.control & 2 != 0 {
                self.irq = true;
            }
        }
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.b.battery {
            Some(self.ram.clone())
        } else {
            None
        }
    }

    fn restore(&mut self, data: &[u8]) {
        self.ram.copy_from_slice(data);
    }
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        33 => Box::new(Tc0190::new(image, t)),
        48 => Box::new(Tc0690::new(image, t)),
        80 | 207 => Box::new(X1005::new(image, t)),
        82 | 552 => Box::new(X1017::new(image, t)),
        _ => return Err(image),
    })
}
