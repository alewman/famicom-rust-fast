// SPDX-License-Identifier: MIT
//! Jaleco's mappers (`jaleco.py`): SS88006 (18) and the discrete JF boards:
//! JF-17/19 (72, 92), JF-16 (78), JF-13 (86) and JF-11/14 (140). The speech
//! chips (µPD7756C) are not modelled, as in the reference: their register
//! writes are taken and nothing sounds.

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// $F001: D3 overrides D2 overrides D1; none set, the counter is 16 bits.
const COUNTER_MASKS: [(u8, u32); 4] = [(8, 0x000F), (4, 0x00FF), (2, 0x0FFF), (0, 0xFFFF)];

/// Mapper 18: Jaleco SS88006, PRG and CHR banks in nibbles, a masked M2 counter
/// (INES_Mapper_018.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ss88006 {
    pub b: Banked,
    pub prg_registers: [u8; 3],
    pub chr_registers: [u8; 8],
    pub reload: [u32; 4],
    pub counter: u32,
    pub mask: u32,
    pub counting: bool,
    pub irq: bool,
}

impl Ss88006 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ss88006 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        Ss88006 {
            b,
            prg_registers: [0; 3],
            chr_registers: [0; 8],
            reload: [0; 4],
            counter: 0,
            mask: 0xFFFF,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Ss88006 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            let mask = self.mask;
            let low = self.counter & mask;
            if low == 0 {
                self.irq = true;
            }
            self.counter = (self.counter & !mask) | (low.wrapping_sub(1) & mask);
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = value & 0x0F;
        let register = address & 0xF003;
        let group = (register >> 12) as i32 - 8;
        let high = (register & 1) * 4;
        if register < 0x9002 {
            let bank = if register >= 0x9000 {
                2
            } else {
                ((register >> 1) & 1) as usize
            };
            let nibble = self.prg_registers[bank];
            self.prg_registers[bank] = (nibble & (0xF0u8 >> high)) | (value << high);
            self.b.map_prg(bank, 8, self.prg_registers[bank] as i64);
        } else if register == 0x9002 {
            self.b.prg_ram_enabled = !self.b.prg_ram.is_empty() && value & 1 != 0;
            self.b.prg_ram_writable = value & 2 != 0;
        } else if (0xA000..0xE000).contains(&register) {
            let bank = ((group - 2) * 2 + ((register >> 1) & 1) as i32) as usize;
            let nibble = self.chr_registers[bank];
            self.chr_registers[bank] = (nibble & (0xF0u8 >> high)) | (value << high);
            self.b.map_chr(bank, 1, self.chr_registers[bank] as i64);
        } else if register < 0xF000 {
            self.reload[(register & 3) as usize] = value as u32;
        } else if register == 0xF000 {
            let r = self.reload;
            self.counter = r[0] | (r[1] << 4) | (r[2] << 8) | (r[3] << 12);
            self.irq = false;
        } else if register == 0xF001 {
            self.mask = COUNTER_MASKS
                .iter()
                .find(|&&(bit, _)| value & bit != 0 || bit == 0)
                .map(|&(_, mask)| mask)
                .unwrap();
            self.counting = value & 1 != 0;
            self.irq = false;
        } else if register == 0xF002 {
            self.b
                .set_mirroring([HORIZONTAL, VERTICAL, SCREEN_A, SCREEN_B][(value & 3) as usize]);
        }
    }
}

/// Mappers 72 (JF-17) and 92 (JF-19): banks loaded on a command bit's rise
/// (INES_Mapper_072.wiki, _092.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jf17 {
    pub b: Banked,
    pub fixed_first: bool,
    pub latch: u8,
}

impl Jf17 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Jf17 {
        let fixed_first = image.board.mapper == 92;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 8, 0);
        Jf17 {
            b,
            fixed_first,
            latch: 0,
        }
    }
}

impl Cartridge for Jf17 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        let rises = value & !self.latch;
        self.latch = value;
        if rises & 0x80 != 0 {
            if self.fixed_first {
                self.b.map_prg(2, 16, (value & 0x0F) as i64);
            } else {
                self.b.map_prg(0, 16, (value & 0x07) as i64);
            }
        }
        if rises & 0x40 != 0 {
            self.b.map_chr(0, 8, (value & 0x0F) as i64);
        }
    }
}

/// Mapper 78: Jaleco JF-16 (Cosmo Carrier) and Irem's Holy Diver board
/// (INES_Mapper_078.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jf16 {
    pub b: Banked,
    pub holy_diver: bool,
}

impl Jf16 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Jf16 {
        let submapper = image.board.submapper;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let holy_diver = submapper == 3 || (submapper == 0 && b.four_screen);
        b.four_screen = false;
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        let mut board = Jf16 { b, holy_diver };
        board.latch(0);
        board
    }

    fn latch(&mut self, value: u8) {
        self.b.map_prg(0, 16, (value & 7) as i64);
        self.b.map_chr(0, 8, (value >> 4) as i64);
        if self.holy_diver {
            self.b
                .set_mirroring(if value & 8 != 0 { VERTICAL } else { HORIZONTAL });
        } else {
            self.b
                .set_mirroring(if value & 8 != 0 { SCREEN_B } else { SCREEN_A });
        }
    }
}

impl Cartridge for Jf16 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.latch(value);
    }
}

/// Mapper 86: Jaleco JF-13 (Moero!! Pro Yakyuu), registers at $6000-$6FFF;
/// other writes go nowhere (the reference does not call super).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jf13 {
    pub b: Banked,
}

impl Cartridge for Jf13 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x7000).contains(&address) {
            self.b.map_prg(0, 32, ((value >> 4) & 3) as i64);
            self.b
                .map_chr(0, 8, (((value >> 4) & 4) | (value & 3)) as i64);
        }
    }
}

/// Mapper 140: Jaleco JF-11/JF-14, GNROM with its register at $6000-$7FFF;
/// other writes go nowhere.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jf11 {
    pub b: Banked,
}

impl Cartridge for Jf11 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.b.map_prg(0, 32, ((value >> 4) & 3) as i64);
            self.b.map_chr(0, 8, (value & 0x0F) as i64);
        }
    }
}

/// JF-13 and JF-11's power-on: no PRG-RAM, the first 32 KiB and 8 KiB.
fn gnrom_like(image: RomImage, cpu_cycle_ticks: i64) -> Banked {
    let mut b = Banked::new(image, cpu_cycle_ticks);
    b.prg_ram_enabled = false;
    b.map_prg(0, 32, 0);
    b.map_chr(0, 8, 0);
    b
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        18 => Box::new(Ss88006::new(image, t)),
        72 | 92 => Box::new(Jf17::new(image, t)),
        78 => Box::new(Jf16::new(image, t)),
        86 => Box::new(Jf13 {
            b: gnrom_like(image, t),
        }),
        140 => Box::new(Jf11 {
            b: gnrom_like(image, t),
        }),
        _ => return Err(image),
    })
}
