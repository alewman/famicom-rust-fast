// SPDX-License-Identifier: MIT
//! Discrete-logic boards (`discrete.py`): NROM (0), UxROM (2), CNROM (3),
//! AxROM (7), CPROM (13), UNROM 512 (30), BNROM and NINA-001 (34), GxROM
//! (66), J87 (87), UN1ROM (94), UNROM180 (180), CNROM with copy protection
//! (185), Fujiya 170 and the AOROM multicart (470). Each class's sources are
//! in the reference's docstrings.

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::flash::Flash;
use super::ines::RomImage;
use crate::banked_field;

/// Mapper 0: 16 or 32 KiB of PRG (16 KiB mirrored), 8 KiB of CHR, no registers.
pub fn nrom(image: RomImage, cpu_cycle_ticks: i64) -> Banked {
    let mut b = Banked::new(image, cpu_cycle_ticks);
    nrom_map(&mut b);
    b
}

/// NROM's windows, which the boards built on it start from.
pub fn nrom_map(b: &mut Banked) {
    for window in 0..4 {
        b.map_prg(window, 8, window as i64);
    }
    b.map_chr(0, 8, 0);
}

/// Mapper 2: a write to $8000-$FFFF selects the 16 KiB bank at $8000; $C000
/// is the last. Submapper 2 has AND-type bus conflicts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UxRom {
    pub b: Banked,
    pub bus_conflicts: bool,
}

impl UxRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> UxRom {
        let bus_conflicts = image.board.submapper == 2;
        let last = (image.prg.len() / 0x4000) as i64 - 1;
        let mut b = nrom(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        b.map_prg(2, 16, last);
        UxRom { b, bus_conflicts }
    }
}

impl Cartridge for UxRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = if self.bus_conflicts {
            self.b.bus_conflict(address, value)
        } else {
            value
        };
        self.b.map_prg(0, 16, value as i64);
    }
}

/// Mapper 3: a write to $8000-$FFFF selects the 8 KiB CHR bank.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CnRom {
    pub b: Banked,
    pub bus_conflicts: bool,
}

impl CnRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> CnRom {
        let bus_conflicts = image.board.submapper == 2;
        CnRom {
            b: nrom(image, cpu_cycle_ticks),
            bus_conflicts,
        }
    }
}

impl Cartridge for CnRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = if self.bus_conflicts {
            self.b.bus_conflict(address, value)
        } else {
            value
        };
        self.b.map_chr(0, 8, value as i64);
    }
}

/// Mapper 7: a write to $8000-$FFFF selects the 32 KiB PRG bank and the
/// screen (D4). Also NES 2.0 mapper 470 (INX_007T_V01), AOROM with an outer
/// 256 KiB bank at $5000-$5FFF that a reset clears: the reference's
/// `AOROMMulticart` overrides `map_prg`, here `outer` is `Some`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AxRom {
    pub b: Banked,
    pub bus_conflicts: bool,
    pub outer: Option<i64>,
    pub inner: i64,
}

impl AxRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> AxRom {
        AxRom::build(image, cpu_cycle_ticks, None)
    }

    pub fn multicart(image: RomImage, cpu_cycle_ticks: i64) -> AxRom {
        AxRom::build(image, cpu_cycle_ticks, Some(0))
    }

    fn build(image: RomImage, cpu_cycle_ticks: i64, outer: Option<i64>) -> AxRom {
        let bus_conflicts = image.board.submapper == 2;
        let mut axrom = AxRom {
            b: Banked::new(image, cpu_cycle_ticks),
            bus_conflicts,
            outer,
            inner: 0,
        };
        axrom.map_prg(0);
        axrom.b.map_chr(0, 8, 0);
        axrom.b.set_mirroring(SCREEN_A);
        axrom
    }

    /// `map_prg(0, 32, bank)`, through the multicart's override.
    fn map_prg(&mut self, bank: i64) {
        match self.outer {
            None => self.b.map_prg(0, 32, bank),
            Some(outer) => {
                self.inner = bank & 7;
                self.b.map_prg(0, 32, (outer << 3) | self.inner);
            }
        }
    }
}

impl Cartridge for AxRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.outer.is_some() && (0x5000..0x6000).contains(&address) {
            self.outer = Some(value as i64);
            let inner = self.inner;
            self.map_prg(inner);
            return;
        }
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = if self.bus_conflicts {
            self.b.bus_conflict(address, value)
        } else {
            value
        };
        self.map_prg((value & 0x0F) as i64);
        self.b.set_mirroring(if value & 0x10 != 0 {
            SCREEN_B
        } else {
            SCREEN_A
        });
    }

    fn m2_gap(&mut self, _t: i64) {
        if self.outer.is_some() {
            self.outer = Some(0);
            let inner = self.inner;
            self.map_prg(inner);
        }
    }
}

/// Mapper 34.2: BNROM, a write to $8000-$FFFF selects the 32 KiB PRG bank;
/// always AND-type bus conflicts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BnRom {
    pub b: Banked,
}

impl Cartridge for BnRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let bank = self.b.bus_conflict(address, value);
        self.b.map_prg(0, 32, bank as i64);
    }
}

/// Mapper 34.1: NINA-001, registers at $7FFD-$7FFF written through to the
/// PRG-RAM beneath.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Nina001 {
    pub b: Banked,
}

impl Cartridge for Nina001 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        self.b.base_cpu_write(address, value, t);
        match address {
            0x7FFD => self.b.map_prg(0, 32, value as i64),
            0x7FFE => self.b.map_chr(0, 4, value as i64),
            0x7FFF => self.b.map_chr(4, 4, value as i64),
            _ => {}
        }
    }
}

fn banked_32k(image: RomImage, cpu_cycle_ticks: i64) -> Banked {
    let mut b = Banked::new(image, cpu_cycle_ticks);
    b.map_prg(0, 32, 0);
    b.map_chr(0, 8, 0);
    b
}

/// Mapper 34's board by submapper; without one, CHR over 8 KiB means NINA-001.
pub fn mapper_34(image: RomImage, cpu_cycle_ticks: i64) -> Box<dyn Cartridge> {
    let submapper = image.board.submapper;
    if submapper == 1 || (submapper != 2 && image.chr.len() > 0x2000) {
        Box::new(Nina001 {
            b: banked_32k(image, cpu_cycle_ticks),
        })
    } else {
        Box::new(BnRom {
            b: banked_32k(image, cpu_cycle_ticks),
        })
    }
}

/// Mapper 66: GxROM, PRG bank in D5-D4, CHR in D1-D0; always bus conflicts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GxRom {
    pub b: Banked,
}

impl GxRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> GxRom {
        GxRom {
            b: banked_32k(image, cpu_cycle_ticks),
        }
    }
}

impl Cartridge for GxRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.b.map_prg(0, 32, ((value >> 4) & 3) as i64);
        self.b.map_chr(0, 8, (value & 3) as i64);
    }
}

/// Mapper 87: a write to $6000-$7FFF selects the 8 KiB CHR bank, its two
/// bits swapped. Other writes go nowhere (the reference does not call super).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct J87 {
    pub b: Banked,
}

impl J87 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> J87 {
        J87 {
            b: nrom(image, cpu_cycle_ticks),
        }
    }
}

impl Cartridge for J87 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.b
                .map_chr(0, 8, (((value & 1) << 1) | ((value >> 1) & 1)) as i64);
        }
    }
}

/// Mapper 185: CNROM whose CHR is enabled only by the right chip-select bits.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CnRomProtected {
    pub b: Banked,
    pub enabling: Option<u8>,
    pub chr_enabled: bool,
}

impl CnRomProtected {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> CnRomProtected {
        let submapper = image.board.submapper;
        CnRomProtected {
            enabling: if (4..=7).contains(&submapper) {
                Some(submapper - 4)
            } else {
                None
            },
            b: nrom(image, cpu_cycle_ticks),
            chr_enabled: true,
        }
    }
}

impl Cartridge for CnRomProtected {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if let Some(enabling) = self.enabling {
            self.chr_enabled = self.b.bus_conflict(address, value) & 3 == enabling;
        }
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && !self.chr_enabled {
            return Some((address & 0xFF) as u8 | 1);
        }
        self.b.base_ppu_read(address, t)
    }
}

/// Mapper 94: HVC-UN1ROM, UxROM with the bank in D4-D2; always bus conflicts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Un1Rom {
    pub u: UxRom,
}

impl Un1Rom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Un1Rom {
        let mut u = UxRom::new(image, cpu_cycle_ticks);
        u.bus_conflicts = true;
        Un1Rom { u }
    }
}

impl Cartridge for Un1Rom {
    banked_field!(u.b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.u.cpu_write(address, value, t);
            return;
        }
        let bank = (self.u.b.bus_conflict(address, value) >> 2) & 7;
        self.u.b.map_prg(0, 16, bank as i64);
    }
}

/// Mapper 180: UNROM with an AND gate, the first 16 KiB fixed, $C000 switched.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UnRom180 {
    pub b: Banked,
    pub bus_conflicts: bool,
}

impl UnRom180 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> UnRom180 {
        let bus_conflicts = image.board.submapper == 2;
        let mut b = nrom(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        b.map_prg(2, 16, 0);
        UnRom180 { b, bus_conflicts }
    }
}

impl Cartridge for UnRom180 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = if self.bus_conflicts {
            self.b.bus_conflict(address, value)
        } else {
            value
        };
        self.b.map_prg(2, 16, (value & 7) as i64);
    }
}

/// Mapper 13: NES-CPROM, 16 KiB of CHR-RAM in 4 KiB pages.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CpRom {
    pub b: Banked,
}

impl CpRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> CpRom {
        let mut b = nrom(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x4000];
        b.chr_writable = true;
        b.map_chr(0, 8, 0);
        CpRom { b }
    }
}

impl Cartridge for CpRom {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let page = self.b.bus_conflict(address, value) & 3;
        self.b.map_chr(4, 4, page as i64);
    }
}

/// Mapper 30: UNROM 512, the homebrew board with self-flashing PRG.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UnRom512 {
    pub b: Banked,
    pub submapper: u8,
    pub one_screen: bool,
    pub cart_vram: bool,
    pub bus_conflicts: bool,
    pub flashable: bool,
    pub latch: u8,
    pub flash: Flash,
}

impl UnRom512 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> UnRom512 {
        let board = image.board.clone();
        let submapper = board.submapper;
        let flags6 = if image.header.len() > 6 {
            image.header[6]
        } else if board.mirroring == '4' {
            0x08
        } else if board.mirroring == 'V' {
            0x01
        } else {
            0
        };
        let nes2 = image.nes2;
        let chr_empty = image.chr.is_empty();
        let prg = image.prg.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg = prg;
        if chr_empty && !(nes2 && board.chr_ram + board.chr_nvram != 0) {
            b.chr = vec![0; 0x8000];
        }
        b.four_screen = false;
        let mut cart = UnRom512 {
            b,
            submapper,
            one_screen: flags6 & 0x09 == 0x08 && submapper != 3,
            cart_vram: flags6 & 0x09 == 0x09 && submapper != 3,
            bus_conflicts: submapper == 2 || (submapper == 0 && !board.battery),
            flashable: board.battery && matches!(submapper, 0 | 1 | 4),
            latch: 0,
            flash: Flash::sst39sf040(),
        };
        let last = (cart.b.prg.len() / 0x4000) as i64 - 1;
        cart.b.map_prg(2, 16, last);
        cart.apply(0);
        cart
    }

    fn apply(&mut self, value: u8) {
        self.latch = value;
        self.b.map_prg(0, 16, (value & 0x1F) as i64);
        self.b.map_chr(0, 8, ((value >> 5) & 3) as i64);
        if self.one_screen {
            self.b.set_mirroring(if value & 0x80 != 0 {
                SCREEN_B
            } else {
                SCREEN_A
            });
        } else if self.submapper == 3 {
            self.b.set_mirroring(if value & 0x80 != 0 {
                VERTICAL
            } else {
                HORIZONTAL
            });
        }
    }

    fn flash_offset(&self, address: u16) -> usize {
        (((self.latch as usize & 0x1F) << 14) | (address as usize & 0x3FFF)) % self.b.prg.len()
    }
}

impl Cartridge for UnRom512 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if self.bus_conflicts {
            let value = self.b.bus_conflict(address, value);
            self.apply(value);
        } else if address >= 0xC000 {
            self.apply(value);
        } else if self.flashable {
            let offset = self.flash_offset(address);
            self.flash.write(&mut self.b.prg, offset, value);
        }
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if self.flash.id_mode && address >= 0x8000 {
            let a = address as usize;
            let offset = self.b.prg_offsets[(a >> 13) & 3] + (a & 0x1FFF);
            return Some(self.flash.read(&self.b.prg, offset) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        if self.cart_vram {
            return None;
        }
        self.b.base_ciram_a10(address)
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if self.cart_vram && address >= 0x2000 {
            let len = self.b.chr.len();
            return Some(self.b.chr[len - 0x2000 + (address as usize & 0x1FFF)]);
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.cart_vram && address >= 0x2000 {
            let len = self.b.chr.len();
            self.b.chr[len - 0x2000 + (address as usize & 0x1FFF)] = value;
            return;
        }
        self.b.base_ppu_write(address, value, t);
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.flashable {
            Some(self.b.prg.clone())
        } else {
            None
        }
    }

    fn restore(&mut self, data: &[u8]) {
        self.b.prg.copy_from_slice(data);
    }
}

/// Mapper 170 (Fujiya): NROM and a one-bit latch.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fujiya170 {
    pub b: Banked,
    pub latch: u8,
}

impl Fujiya170 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fujiya170 {
        Fujiya170 {
            b: nrom(image, cpu_cycle_ticks),
            latch: 0,
        }
    }
}

impl Cartridge for Fujiya170 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address == 0x7001 || address == 0x7777 {
            return Some(self.latch as u16 | (0x7F << 8));
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address == 0x6502 || address == 0x7000 {
            self.latch = (value & 0x40) << 1;
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}
