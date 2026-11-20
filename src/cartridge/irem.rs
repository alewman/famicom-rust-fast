// SPDX-License-Identifier: MIT
//! Irem's mappers: G-101 (32), H3001 (65), LROG017 (77) and TAM-S1 (97)
//! (`irem.py`). No judge ROM exists; the claims rest on sweeps against
//! Mesen2, a detector.
//!
//! **H3001's subclass.** multiasic's `HostH3001` overrides the `map_prg` and
//! `map_chr` the H3001 calls on `self`; here those are the methods of
//! [`H3001Like`] (the `mmc3.rs` pattern), the base's state is [`H3001`]
//! (itself the plain board), and [`h3001_cartridge!`] writes the `Cartridge`
//! methods a board takes from it.

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// Mapper 32: two 8 KiB PRG banks (one movable), the last two fixed; eight 1
/// KiB CHR banks (INES_Mapper_032.wiki; submapper 1, Major League, one screen).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct G101 {
    pub b: Banked,
    pub major_league: bool,
    pub prg_registers: [u8; 2],
    pub prg_mode: u8,
}

impl G101 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> G101 {
        let major_league = image.board.submapper == 1;
        let mut g = G101 {
            b: Banked::new(image, cpu_cycle_ticks),
            major_league,
            prg_registers: [0, 0],
            prg_mode: 0,
        };
        g.b.prg_ram_enabled = false;
        if g.major_league {
            g.b.set_mirroring(SCREEN_B);
        }
        g.update_prg();
        g
    }

    fn update_prg(&mut self) {
        let last = (self.b.prg.len() / 0x2000) as i64 - 1;
        let (mut first, mut third) = (self.prg_registers[0] as i64, last - 1);
        if self.prg_mode != 0 {
            std::mem::swap(&mut first, &mut third);
        }
        self.b.map_prg(0, 8, first);
        self.b.map_prg(1, 8, self.prg_registers[1] as i64);
        self.b.map_prg(2, 8, third);
        self.b.map_prg(3, 8, last);
    }
}

impl Cartridge for G101 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let region = address & 0xF000;
        if region == 0x8000 {
            self.prg_registers[0] = value & 0x1F;
            self.update_prg();
        } else if region == 0x9000 {
            if !self.major_league {
                self.b
                    .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
                self.prg_mode = value & 2;
                self.update_prg();
            }
        } else if region == 0xA000 {
            self.prg_registers[1] = value & 0x1F;
            self.update_prg();
        } else if region == 0xB000 {
            self.b.map_chr((address & 7) as usize, 1, value as i64);
        }
    }
}

/// Mapper 77: Irem LROG017 (Napoleon Senki): CHR-ROM, CHR-RAM and cartridge
/// nametables (INES_Mapper_077.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Lrog017 {
    pub b: Banked,
    pub rom: Vec<u8>,
    /// PPU $0800-$27FF.
    pub ram: Vec<u8>,
    pub rom_offset: usize,
}

impl Lrog017 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Lrog017 {
        let rom = image.chr.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        Lrog017 {
            b,
            rom,
            ram: vec![0; 0x2000],
            rom_offset: 0,
        }
    }
}

impl Cartridge for Lrog017 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.b.map_prg(0, 32, (value & 0x0F) as i64);
        self.rom_offset = ((value as usize >> 4) * 0x800) % self.rom.len();
    }
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        if address & 0x1800 == 0x0800 {
            Some(((address >> 10) & 1) as u8)
        } else {
            None
        }
    }
    fn ppu_read(&mut self, address: u16, _t: i64) -> Option<u8> {
        if address < 0x0800 {
            return Some(self.rom[self.rom_offset + address as usize]);
        }
        if address < 0x2800 {
            return Some(self.ram[address as usize - 0x0800]);
        }
        None
    }
    fn ppu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x0800..0x2800).contains(&address) {
            self.ram[address as usize - 0x0800] = value;
        }
    }
}

/// Mapper 97: Irem TAM-S1 (Kaiketsu Yanchamaru): the last 16 KiB at $8000,
/// $C000 switched (INES_Mapper_097.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TamS1 {
    pub b: Banked,
}

impl TamS1 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> TamS1 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(0, 16, last);
        b.map_prg(2, 16, 0);
        b.map_chr(0, 8, 0);
        TamS1 { b }
    }
}

impl Cartridge for TamS1 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x8000..0xC000).contains(&address) {
            self.b.map_prg(2, 16, (value & 0x1F) as i64);
            self.b.set_mirroring(if value & 0x80 != 0 {
                VERTICAL
            } else {
                HORIZONTAL
            });
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

// -- H3001 -----------------------------------------------------------------------

/// Mapper 65: Irem H3001: three 8 KiB PRG banks, eight 1 KiB CHR banks, a
/// cycle counter (INES_Mapper_065.wiki). The base class's state; also the
/// plain board.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct H3001 {
    pub b: Banked,
    pub prg_mode: u8,
    pub prg0: u8,
    pub reload: u16,
    pub counter: u16,
    pub enabled: bool,
    pub irq: bool,
}

impl H3001 {
    /// `H3001.__init__`'s fields, before its banking calls, which the board's
    /// constructor makes through [`h3001_init`].
    pub fn state(image: RomImage, cpu_cycle_ticks: i64) -> H3001 {
        H3001 {
            b: Banked::new(image, cpu_cycle_ticks),
            prg_mode: 0,
            prg0: 0,
            reload: 0,
            counter: 0,
            enabled: false,
            irq: false,
        }
    }

    /// The whole `H3001.__init__`: the plain board.
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> H3001 {
        let mut h = H3001::state(image, cpu_cycle_ticks);
        h3001_init(&mut h);
        h
    }
}

/// The H3001's overridable methods. Default bodies: `Banked`'s.
pub trait H3001Like {
    fn h3001(&self) -> &H3001;
    fn h3001_mut(&mut self) -> &mut H3001;

    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.h3001_mut().b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.h3001_mut().b.map_chr(window, size_kib, bank);
    }
}

/// The banking calls of `H3001.__init__`.
pub fn h3001_init<T: H3001Like + ?Sized>(this: &mut T) {
    this.map_prg(1, 8, 1);
    let last = (this.h3001().b.prg.len() / 0x2000) as i64 - 1;
    this.map_prg(3, 8, last);
    h3001_map_prg0(this);
}

/// `H3001._map_prg0`.
pub fn h3001_map_prg0<T: H3001Like + ?Sized>(this: &mut T) {
    let h = this.h3001();
    let second_last = (h.b.prg.len() / 0x2000) as i64 - 2;
    let prg0 = h.prg0 as i64;
    if h.prg_mode != 0 {
        this.map_prg(0, 8, second_last);
        this.map_prg(2, 8, prg0);
    } else {
        this.map_prg(0, 8, prg0);
        this.map_prg(2, 8, second_last);
    }
}

/// `H3001.m2`.
pub fn h3001_m2(h: &mut H3001) {
    if h.enabled && h.counter != 0 {
        h.counter -= 1;
        if h.counter == 0 {
            h.irq = true;
        }
    }
}

/// `H3001.cpu_write`.
pub fn h3001_cpu_write<T: H3001Like + ?Sized>(this: &mut T, address: u16, value: u8, t: i64) {
    if address < 0x8000 {
        this.h3001_mut().b.base_cpu_write(address, value, t);
        return;
    }
    let register = address & 0xF007;
    if register == 0x8000 {
        this.h3001_mut().prg0 = value;
        h3001_map_prg0(this);
    } else if register == 0x9000 {
        this.h3001_mut().prg_mode = value & 0x80;
        h3001_map_prg0(this);
    } else if register == 0x9001 {
        let mode = value >> 6;
        let arrangement = if mode & 1 != 0 {
            SCREEN_A
        } else if mode != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        };
        this.h3001_mut().b.set_mirroring(arrangement);
    } else if register == 0x9003 {
        let h = this.h3001_mut();
        h.enabled = value & 0x80 != 0;
        h.irq = false;
    } else if register == 0x9004 {
        let h = this.h3001_mut();
        h.counter = h.reload;
        h.irq = false;
    } else if register == 0x9005 {
        let h = this.h3001_mut();
        h.reload = (h.reload & 0x00FF) | (value as u16) << 8;
    } else if register == 0x9006 {
        let h = this.h3001_mut();
        h.reload = (h.reload & 0xFF00) | value as u16;
    } else if register == 0xA000 {
        this.map_prg(1, 8, value as i64);
    } else if (0xB000..=0xB007).contains(&register) {
        this.map_chr((register & 7) as usize, 1, value as i64);
    }
}

/// The `Cartridge` methods an H3001 board takes from the base class:
/// `banked`, `banked_mut`, `counts_m2` and `irq`, and each named one of
/// `cpu_write` and `m2`.
#[macro_export]
macro_rules! h3001_cartridge {
    ($($method:ident),* $(,)?) => {
        fn banked(&self) -> Option<&$crate::cartridge::Banked> {
            Some(&$crate::cartridge::irem::H3001Like::h3001(self).b)
        }
        fn banked_mut(&mut self) -> Option<&mut $crate::cartridge::Banked> {
            Some(&mut $crate::cartridge::irem::H3001Like::h3001_mut(self).b)
        }
        fn counts_m2(&self) -> bool {
            true
        }
        fn irq(&self) -> bool {
            $crate::cartridge::irem::H3001Like::h3001(self).irq
        }
        $( $crate::h3001_cartridge!(@ $method); )*
    };
    (@ cpu_write) => {
        fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
            $crate::cartridge::irem::h3001_cpu_write(self, address, value, t);
        }
    };
    (@ m2) => {
        fn m2(&mut self, _t: i64) {
            $crate::cartridge::irem::h3001_m2(
                $crate::cartridge::irem::H3001Like::h3001_mut(self));
        }
    };
}

impl H3001Like for H3001 {
    fn h3001(&self) -> &H3001 {
        self
    }
    fn h3001_mut(&mut self) -> &mut H3001 {
        self
    }
}

impl Cartridge for H3001 {
    h3001_cartridge!(cpu_write, m2);
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        32 => Box::new(G101::new(image, t)),
        65 => Box::new(H3001::new(image, t)),
        77 => Box::new(Lrog017::new(image, t)),
        97 => Box::new(TamS1::new(image, t)),
        _ => return Err(image),
    })
}
