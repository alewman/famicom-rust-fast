// SPDX-License-Identifier: MIT
//! Waixing's MMC3 boards: the 8025 "enhanced MMC3" (176), the CHR-ROM/CHR-RAM
//! mixes (74, 191, 192, 194, 195), FS309 (199), T9552 (249), Xiangfeng's 198,
//! Jncota's 523, and the VRC4e clones 252 and 253 (`waixing.py`).
//!
//! INES_Mapper_176.wiki, MMC3_with_CHR_ROM_and_CHR_RAM.wiki and each mapper's
//! page. No judge ROM exists; the claims rest on sweeps against Mesen2, a
//! detector whose 176 blends the submappers where this one follows the wiki's
//! per-submapper registers (docs/claims.md).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 74: `MixedCHR`
//! - 176: `FK23C`
//! - 191: `MixedCHR`
//! - 192: `MixedCHR`
//! - 194: `MixedCHR`
//! - 195: `MixedCHR`
//! - 198: `Xiangfeng198`
//! - 199: `FS309`
//! - 249: `T9552`
//! - 252: `WaixingVRC4`
//! - 253: `WaixingVRC4`
//! - 523: `Jncota523`
//!
//! `Jncota523` subclasses `FK23C` and overrides `set_mirroring` and
//! `_map_chr_1k`: here a flag of [`Fk23c`]. Where a reference class keeps
//! `self.rom = image.chr` beside its `Banked`, the `Banked`'s CHR is that
//! same ROM and serves as it.

use super::banked::{Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::konami::{rewired, vrc4e, Vrc24, Vrc24Like};
use super::mmc3::{base_cpu_write, Mmc3, Mmc3Like};
use crate::{mmc3_cartridge, vrc24_cartridge};

/// The power-on bank registers: MMC3 R0-R7, then the extended mode's R8-RB.
pub const POWER_ON_REGISTERS: [u8; 12] = [
    0x00, 0x02, 0x04, 0x05, 0x06, 0x07, 0x00, 0x01, 0xFE, 0xFF, 0xFF, 0xFF,
];

// -- FK23C -------------------------------------------------------------------------

/// Mapper 176: the 8025 chipset, an MMC3 with outer banks, NROM/UNROM/CNROM
/// modes and more (INES_Mapper_176.wiki; the registers are described in the
/// reference). Also mapper 523 (`Jncota523`, `jncota`): Jncota's re-release
/// of Fengshenbang, FS005 with CHR rewired for 2 KiB banks
/// (NES_2.0_Mapper_523.wiki): horizontal mirroring, hard-wired; CHR A10 is
/// PPU A10.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fk23c {
    pub m: Mmc3,
    pub jncota: bool,
    pub submapper: u8,
    pub wide_mmc3: bool,
    pub mode: u8,
    pub prg_base: u8,
    pub chr_base: u8,
    pub extended: bool,
    /// 176.3 $5xx5 / 176.5 $4800
    pub prg_high: u8,
    /// 176.3 $5xx6
    pub chr_high: u8,
    /// UNROM / CNROM latch
    pub discrete_latch: u8,
    /// 176.2 $A001 while D5 is set
    pub ram_config: u8,
    /// `bool(self.rom)`: the `Banked`'s CHR is the ROM.
    pub has_rom: bool,
    pub chr_ram: Vec<u8>,
    pub chr_ram_window: [bool; 8],
    pub registers: [u8; 12],
}

impl Fk23c {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fk23c {
        Fk23c::build(image, cpu_cycle_ticks, false)
    }

    /// Mapper 523.
    pub fn jncota523(image: RomImage, cpu_cycle_ticks: i64) -> Fk23c {
        let submapper = 2;
        let mapper = image.board.mapper;
        Fk23c::build(rewired(image, mapper, submapper), cpu_cycle_ticks, true)
    }

    fn build(image: RomImage, cpu_cycle_ticks: i64, jncota: bool) -> Fk23c {
        let board = image.board.clone();
        let submapper = board.submapper;
        let has_rom = !image.chr.is_empty();
        // MMC3.__init__'s _update() returns at once (the registers are not ours yet).
        let mut f = Fk23c {
            m: Mmc3::new(image, cpu_cycle_ticks),
            jncota,
            submapper,
            wide_mmc3: submapper == 1 || submapper == 3,
            mode: 0,
            prg_base: 0,
            chr_base: 0,
            extended: false,
            prg_high: 0,
            chr_high: 0,
            discrete_latch: 0,
            ram_config: 0,
            has_rom,
            chr_ram: vec![0; (board.chr_ram + board.chr_nvram).max(0x2000)],
            chr_ram_window: [false; 8],
            registers: POWER_ON_REGISTERS,
        };
        if jncota {
            // Banked.__init__'s set_mirroring, through Jncota523's.
            f.m.b.set_mirroring(HORIZONTAL);
        }
        let ram = f.m.b.prg_ram.len().max(0x2000);
        f.m.b.prg_ram = vec![0; ram];
        f.m.b.prg_ram_enabled = false; // $A001 powers on $00 (the wiki)
        f.fk_update();
        f
    }

    fn set_mirroring(&mut self, mode: Mirroring) {
        self.m
            .b
            .set_mirroring(if self.jncota { HORIZONTAL } else { mode });
    }

    // -- the CPU side --------------------------------------------------------

    fn outer_enabled(&self) -> bool {
        self.ram_config & 0x20 == 0 || self.ram_config & 0x40 != 0
    }

    /// The WRAM byte a CPU address reaches, or None.
    fn wram_offset(&self, address: u16) -> Option<usize> {
        let config = self.ram_config as usize;
        let len = self.m.b.prg_ram.len();
        let a = address as usize;
        if self.submapper == 2 && config & 0x20 != 0 {
            if (0x6000..0x8000).contains(&address) {
                return Some(((config & 3) * 0x2000 + (a & 0x1FFF)) % len);
            }
            if (0x5000..0x6000).contains(&address) && config & 0x40 == 0 {
                return Some((2 * 0x2000 + 0x1000 + (a & 0x0FFF)) % len);
            }
            return None;
        }
        if (0x6000..0x8000).contains(&address) && self.m.b.prg_ram_enabled {
            return Some((a & 0x1FFF) % len);
        }
        None
    }

    fn outer_write(&mut self, address: u16, value: u8) {
        if address & 0x10 == 0 {
            return; // the solder pad's address line
        }
        let register = address & if self.submapper == 3 { 7 } else { 3 };
        match register {
            0 => self.mode = value,
            1 => self.prg_base = value & 0x7F,
            2 => {
                self.chr_base = value;
                self.discrete_latch = 0;
            }
            3 => self.extended = value & 0x02 != 0 && (self.submapper == 1 || self.submapper == 2),
            5 => self.prg_high = value & 0x0F,
            6 => self.chr_high = value & 0x0F,
            _ => {}
        }
        self.fk_update();
    }

    // -- banking -------------------------------------------------------------

    /// PRG A21 and up, as a 16 KiB bank offset.
    fn prg_high_bits(&self) -> i64 {
        let (mode, chr_base) = (self.mode as i64, self.chr_base as i64);
        match self.submapper {
            2 => {
                let a21 = (mode >> 3) & 1
                    | (mode >> 6) & 2
                    | (chr_base >> 4) & 0x0C
                    | (chr_base >> 1) & 0x10;
                a21 << 7
            }
            3 => (self.prg_high as i64) << 7,
            4 => (chr_base >> 7) << 7,
            5 => (self.prg_high as i64) << 5,
            _ => 0,
        }
    }

    /// `FK23C._update`.
    fn fk_update(&mut self) {
        let r = self.registers.map(|x| x as i64);
        let select = self.m.bank_select;
        let mode = (self.mode & 7) as i64;
        let base16 = self.prg_base as i64 & if self.submapper == 5 { 0x1F } else { 0x7F };
        let high16 = self.prg_high_bits();
        if mode >= 3 && !self.extended {
            if mode == 3 {
                // NROM-128
                self.map_prg(0, 16, high16 | base16);
                self.map_prg(2, 16, high16 | base16);
            } else if mode == 4 {
                // NROM-256
                self.map_prg(0, 32, (high16 | base16) >> 1);
            } else {
                // UNROM: inner bank from the latch, 7 fixed
                let latch = self.discrete_latch as i64;
                self.map_prg(0, 16, high16 | base16 & !7 | latch & 7);
                self.map_prg(2, 16, high16 | base16 & !7 | 7);
            }
        } else {
            let inner: i64 = if self.extended || self.wide_mmc3 {
                0xFF
            } else {
                0x3F >> mode.min(2)
            };
            let outer8 = ((high16 | base16) << 1) & !inner;
            let banks = if self.extended {
                if select & 0x40 != 0 {
                    [r[8], r[7], r[6], r[9]]
                } else {
                    [r[6], r[7], r[8], r[9]]
                }
            } else if select & 0x40 != 0 {
                [0xFE, r[7], r[6], 0xFF]
            } else {
                [r[6], r[7], 0xFE, 0xFF]
            };
            for (window, bank) in banks.into_iter().enumerate() {
                self.map_prg(window, 8, outer8 | bank & inner);
            }
        }
        self.update_chr();
    }

    fn update_chr(&mut self) {
        let r = self.registers.map(|x| x as i64);
        let select = self.m.bank_select;
        let mode = self.mode;
        let base8 = self.chr_base as i64 | (self.chr_high as i64) << 8;
        if mode & 0x40 != 0 {
            // CHR A12-A10 from the PPU: 8 KiB banks
            let bank8 = if self.submapper == 1 && mode & 0x20 == 0 {
                // CNROM latch
                let mask = if mode & 0x10 != 0 { 1 } else { 3 };
                base8 & !mask | self.discrete_latch as i64 & mask
            } else {
                base8
            };
            for i in 0..8 {
                self.map_chr_1k(i, bank8 * 8 + i as i64);
            }
            return;
        }
        let a12 = if select & 0x80 != 0 { 4 } else { 0 };
        let (ones, inner) = if self.extended {
            ([r[0], r[10], r[1], r[11]], 0xFF)
        } else {
            (
                [r[0] & 0xFE, r[0] | 1, r[1] & 0xFE, r[1] | 1],
                if mode & 0x10 != 0 { 0x7F } else { 0xFF },
            )
        };
        let outer1 = (base8 << 3) & !inner;
        for (i, bank) in ones.into_iter().enumerate() {
            self.map_chr_1k(a12 + i, outer1 | bank & inner);
        }
        for i in 0..4 {
            self.map_chr_1k((a12 ^ 4) + i, outer1 | r[2 + i] & inner);
        }
    }

    /// `_map_chr_1k`, through `Jncota523`'s.
    fn map_chr_1k(&mut self, window: usize, bank: i64) {
        if self.jncota {
            if window & 1 == 0 {
                self.fk_map_chr_1k(window, bank * 2);
                self.fk_map_chr_1k(window + 1, bank * 2 + 1);
            }
        } else {
            self.fk_map_chr_1k(window, bank);
        }
    }

    /// `FK23C._map_chr_1k`.
    fn fk_map_chr_1k(&mut self, window: usize, bank: i64) {
        let mode = self.mode;
        let sub = self.submapper;
        let ram = !self.has_rom
            || ((sub == 0 || sub == 1) && mode & 0x20 != 0 && mode & 0x40 == 0)
            || (sub == 2 && self.ram_config & 0x24 == 0x24 && bank < 8);
        self.chr_ram_window[window] = ram;
        let len = if ram {
            self.chr_ram.len()
        } else {
            self.m.b.chr.len()
        };
        self.m.b.chr_offsets[window] = (bank * 0x400).rem_euclid(len as i64) as usize;
    }
}

impl Mmc3Like for Fk23c {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn update(&mut self) {
        self.fk_update();
    }
    fn ram_protect(&mut self, value: u8) {
        self.m.b.prg_ram_enabled = value & 0x80 != 0;
        self.m.b.prg_ram_writable = value & 0x40 == 0;
    }
}

impl Cartridge for Fk23c {
    mmc3_cartridge!(ppu_address);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 {
            return self.m.b.base_cpu_read(address, t);
        }
        self.wram_offset(address)
            .map(|offset| self.m.b.prg_ram[offset] as u16)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            let offset = self.wram_offset(address);
            let writable = self.ram_config & 0x20 != 0 || self.m.b.prg_ram_writable;
            if let (Some(offset), true) = (offset, writable) {
                self.m.b.prg_ram[offset] = value;
            } else if (0x5000..0x6000).contains(&address) && self.outer_enabled() {
                self.outer_write(address, value);
            } else if self.submapper == 5 && address & 0xF800 == 0x4800 {
                self.prg_high = value & 0x3F;
                self.fk_update();
            }
            return;
        }
        self.discrete_latch = value;
        let register = address & 0xE003;
        if register == 0x8000 {
            self.m.bank_select = value;
        } else if register == 0x8001 {
            let index = (self.m.bank_select & if self.extended { 0x0F } else { 0x07 }) as usize;
            if index < self.registers.len() {
                self.registers[index] = value;
            }
        } else if register == 0xA000 {
            let modes = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];
            let mask = if self.submapper == 2 && self.ram_config & 0x20 != 0 {
                3
            } else {
                1
            };
            self.set_mirroring(modes[(value & mask) as usize]);
        } else if register == 0xA001 {
            if self.submapper == 2 && (value & 0x20 != 0 || self.ram_config & 0x20 != 0) {
                self.ram_config = value;
            } else {
                self.ram_protect(value);
            }
        } else if matches!(register, 0xC000 | 0xC001 | 0xE000 | 0xE001) {
            base_cpu_write(self, register, value, t);
            return;
        }
        self.fk_update();
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            let memory = if self.chr_ram_window[window] {
                &self.chr_ram
            } else {
                &self.m.b.chr
            };
            return Some(memory[self.m.b.chr_offsets[window] + (address as usize & 0x3FF)]);
        }
        self.m.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            if self.chr_ram_window[window] {
                self.chr_ram[self.m.b.chr_offsets[window] + (address as usize & 0x3FF)] = value;
            }
            return;
        }
        self.m.b.base_ppu_write(address, value, t);
    }
}

// -- the CHR-ROM/CHR-RAM mixes ---------------------------------------------------------

/// Mapper -> (first CHR bank that is RAM, how many 1 KiB banks): the wiki's table.
fn mixed_chr(mapper: u16) -> Option<(i64, i64)> {
    match mapper {
        74 => Some((8, 2)),
        192 => Some((8, 4)),
        194 => Some((0, 2)),
        _ => None,
    }
}

/// Mapper 195's CHR-RAM window by the value written to CHR-ROM (D7, D6, D3,
/// D1): (first bank, banks); None for CHR-ROM only (INES_Mapper_195.wiki).
fn fs303_window(key: i64) -> Option<(i64, i64)> {
    match key {
        0x80 => Some((0x28, 4)),
        0x82 => Some((0x00, 4)),
        0x88 => Some((0x4C, 4)),
        0x8A => Some((0x64, 4)),
        0xC0 => Some((0x46, 2)),
        0xC2 => Some((0x7C, 2)),
        0xC8 => Some((0x0A, 2)),
        0xCA => None,
        _ => panic!("FS303_WINDOWS has no key {key:#x}"),
    }
}

/// Mappers 74, 191, 192, 194 and 195: MMC3 clones whose CHR bank numbers
/// pick CHR-RAM too (MMC3_with_CHR_ROM_and_CHR_RAM.wiki, INES_Mapper_195.wiki,
/// INES_Mapper_191.wiki; the details are in the reference).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MixedChr {
    pub m: Mmc3,
    pub kind: u16,
    pub window: Option<(i64, i64)>,
    pub outer: u8,
    pub dushen_outer: bool,
    pub chr_ram: Vec<u8>,
    pub chr_ram_window: [bool; 8],
    pub window_banks: [i64; 8],
    pub low_ram: Option<Vec<u8>>,
}

impl MixedChr {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> MixedChr {
        let board = image.board.clone();
        let kind = board.mapper;
        let dushen_outer = kind == 191 && (board.submapper == 1 || image.chr.len() >= 0x40000);
        let mut mixed = MixedChr {
            kind,
            window: Some(mixed_chr(kind).unwrap_or((0x28, 4))),
            outer: 0,
            dushen_outer,
            chr_ram: vec![0; (board.chr_ram + board.chr_nvram).max(0x2000)],
            chr_ram_window: [false; 8],
            window_banks: [0; 8],
            low_ram: None,
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        mixed.update();
        if kind == 195 && board.prg_ram == 0x1000 {
            mixed.low_ram = Some(vec![0; 0x1000]);
        }
        mixed
    }

    fn map_1k(&mut self, window: usize, bank: i64) {
        self.window_banks[window] = bank;
        let mut bank = bank;
        let (ram, offset) = if self.kind == 191 {
            let ram = if self.dushen_outer && self.outer < 3 {
                bank |= 0x80;
                false
            } else {
                bank & 0x80 != 0
            };
            (
                ram,
                if ram {
                    (bank & 1) * 0x400
                } else {
                    bank * 0x400
                },
            )
        } else if self.kind == 195 {
            let ram = matches!(self.window, Some((first, count)) if first <= bank && bank < first + count);
            (ram, (if ram { bank & 7 } else { bank }) * 0x400)
        } else {
            let (first, count) = self.window.unwrap_or((0, 0));
            let ram = first <= bank && bank < first + count;
            (ram, (if ram { bank - first } else { bank }) * 0x400)
        };
        self.chr_ram_window[window] = ram;
        let len = if ram {
            self.chr_ram.len()
        } else {
            self.m.b.chr.len().max(1)
        };
        self.m.b.chr_offsets[window] = offset.rem_euclid(len as i64) as usize;
    }
}

impl Mmc3Like for MixedChr {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let mut bank = bank;
        if self.dushen_outer && window >= 2 && bank >= (self.m.b.prg.len() / 0x2000) as i64 - 2 {
            bank = 0x18 + 2 * self.outer as i64 + (window as i64 & 1);
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            self.map_1k(window + i, bank * size_kib as i64 + i as i64);
        }
    }
}

impl Cartridge for MixedChr {
    mmc3_cartridge!(ppu_address);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if let Some(low) = &self.low_ram {
            if (0x5000..0x6000).contains(&address) {
                return Some(low[address as usize & 0x0FFF] as u16);
            }
        }
        self.m.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if let Some(low) = &mut self.low_ram {
            if (0x5000..0x6000).contains(&address) {
                low[address as usize & 0x0FFF] = value;
                return;
            }
        }
        if self.dushen_outer && address & 0xF0FF == 0x90AA {
            self.outer = value & 3;
            self.update();
        }
        base_cpu_write(self, address, value, t);
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            let memory = if self.chr_ram_window[window] {
                &self.chr_ram
            } else {
                &self.m.b.chr
            };
            return Some(memory[self.m.b.chr_offsets[window] + (address as usize & 0x3FF)]);
        }
        self.m.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x2000 {
            self.m.b.base_ppu_write(address, value, t);
            return;
        }
        let window = (address >> 10) as usize;
        if self.chr_ram_window[window] {
            self.chr_ram[self.m.b.chr_offsets[window] + (address as usize & 0x3FF)] = value;
        } else if self.kind == 195 && self.window_banks[window] & 0x80 != 0 {
            let bank = self.window_banks[window];
            self.window = if bank & 0x10 != 0 {
                None
            } else {
                fs303_window(bank & 0xCA)
            };
            self.update();
        }
    }
}

// -- 252, 253 ----------------------------------------------------------------------------

/// 252/253: a pattern-table write in this bank moves the RAM to (mask, match).
fn waixing_gal(bank: i64) -> Option<(i64, i64)> {
    match bank {
        0x88 => Some((0xFC, 0x4C)),
        0xC2 => Some((0xFE, 0x7C)),
        0xC8 => Some((0xFE, 0x04)),
        _ => None,
    }
}

/// Mappers 252 and 253: Waixing's VRC4e clones with 2 KiB of CHR-RAM among
/// the banks (INES_Mapper_252.wiki, INES_Mapper_253.wiki; the GAL from MAME's
/// vrc_clones.cpp, BSD-3).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct WaixingVrc4 {
    pub v: Vrc24,
    pub ram_mask: i64,
    pub ram_match: i64,
    pub chr_ram: Vec<u8>,
    pub chr_ram_window: [bool; 8],
    pub banks: [i64; 8],
}

impl WaixingVrc4 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> WaixingVrc4 {
        let ram_match = if image.board.mapper == 252 { 6 } else { 4 };
        let mut board = WaixingVrc4 {
            ram_mask: 0xFE,
            ram_match,
            chr_ram: vec![0; 0x800],
            chr_ram_window: [false; 8],
            banks: [0; 8],
            v: Vrc24::state(vrc4e(image), cpu_cycle_ticks),
        };
        board.update_prg();
        board
    }
}

impl Vrc24Like for WaixingVrc4 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn map_chr(&mut self, window: usize, _size_kib: usize, bank: i64) {
        self.banks[window] = bank;
        let rom_len = self.v.b.chr.len() as i64;
        for (w, &one) in self.banks.iter().enumerate() {
            let ram = one & self.ram_mask == self.ram_match;
            self.chr_ram_window[w] = ram;
            self.v.b.chr_offsets[w] = if ram {
                ((one & 1) * 0x400) as usize
            } else {
                (one * 0x400).rem_euclid(rom_len) as usize
            };
        }
    }
}

impl Cartridge for WaixingVrc4 {
    vrc24_cartridge!(cpu_read, cpu_write, m2);

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            let memory = if self.chr_ram_window[window] {
                &self.chr_ram
            } else {
                &self.v.b.chr
            };
            return Some(memory[self.v.b.chr_offsets[window] + (address as usize & 0x3FF)]);
        }
        self.v.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x2000 {
            self.v.b.base_ppu_write(address, value, t);
            return;
        }
        let window = (address >> 10) as usize;
        if self.chr_ram_window[window] {
            self.chr_ram[self.v.b.chr_offsets[window] + (address as usize & 0x3FF)] = value;
        }
        if let Some((mask, matched)) = waixing_gal(self.banks[window]) {
            self.ram_mask = mask;
            self.ram_match = matched;
        }
    }
}

// -- FS309 -------------------------------------------------------------------------------

/// Mapper 199: Waixing's FS309, a TNROM-like board with more PRG and 12 KiB
/// of RAM low (INES_Mapper_199.wiki; which 12 KiB is the reference's choice).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fs309 {
    pub m: Mmc3,
}

impl Fs309 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fs309 {
        let mut board = Fs309 {
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        let b = &mut board.m.b;
        b.prg_ram = vec![0; b.prg_ram.len().max(0x8000)];
        if b.chr_writable {
            b.chr = vec![0; b.chr.len().max(0x2000)];
        }
        board
    }
}

impl Mmc3Like for Fs309 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, _bank: i64) {
        for i in 0..size_kib {
            self.m.b.chr_offsets[window + i] = (window + i) * 0x400;
        }
    }
}

impl Cartridge for Fs309 {
    mmc3_cartridge!(ppu_address);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5000..0x8000).contains(&address) {
            return Some(self.m.b.prg_ram[address as usize - 0x5000] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x8000).contains(&address) {
            self.m.b.prg_ram[address as usize - 0x5000] = value;
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- T9552 -------------------------------------------------------------------------------

/// T9552's scrambling (T9552.wiki): for each pattern, the address lines in table order.
const T9552_PRG: [[i64; 4]; 4] = [
    [16, 17, 15, 14],
    [17, 16, 14, 15],
    [14, 15, 16, 17],
    [15, 14, 17, 16],
];
const T9552_CHR: [[i64; 6]; 8] = [
    [15, 12, 16, 17, 14, 13],
    [14, 15, 13, 12, 17, 16],
    [12, 13, 14, 15, 16, 17],
    [16, 14, 12, 13, 17, 15],
    [15, 13, 17, 16, 12, 14],
    [14, 12, 15, 16, 17, 13],
    [13, 16, 14, 15, 12, 17],
    [12, 15, 16, 17, 13, 14],
];

/// Move the address lines of a bank number (bit 0 = A`unit_bit`) from the
/// MMC3's `order` to where the file stores them (`stored`).
pub fn unscramble(bank: i64, unit_bit: i64, order: &[i64], stored: &[i64]) -> i64 {
    let mut cleared = 0;
    for &line in order {
        cleared += 1i64 << (line - unit_bit);
    }
    let mut moved = bank & !cleared;
    for (row, &line) in order.iter().enumerate() {
        let bit = (bank >> (line - unit_bit)) & 1;
        moved |= bit << (stored[row] - unit_bit);
    }
    moved
}

/// Mapper 249: early Waixing boards whose T9552 scrambles the MMC3's PRG and
/// CHR lines (T9552.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct T9552 {
    pub m: Mmc3,
    pub pattern: u8,
}

impl T9552 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> T9552 {
        let mut board = T9552 {
            pattern: 0,
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        board
    }
}

impl Mmc3Like for T9552 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let order = &T9552_PRG[(self.pattern & 3) as usize];
        let bank = unscramble(bank, 13, order, &T9552_PRG[0]);
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let order = &T9552_CHR[(self.pattern & 7) as usize];
        for i in 0..size_kib {
            let one = bank * size_kib as i64 + i as i64;
            let one = unscramble(one, 10, order, &T9552_CHR[0]);
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for T9552 {
    mmc3_cartridge!(ppu_address);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF000 == 0x5000 {
            self.pattern = value & 7;
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- 198 ---------------------------------------------------------------------------------

/// Mapper 198: Xiangfeng Cartoon's Destiny of an Emperor II translation, 640
/// KiB of PRG (INES_Mapper_198.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Xiangfeng198 {
    pub m: Mmc3,
    pub low_ram: Vec<u8>,
}

impl Xiangfeng198 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Xiangfeng198 {
        let mut board = Xiangfeng198 {
            low_ram: vec![0; 0x1000],
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        board
    }
}

impl Mmc3Like for Xiangfeng198 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let last = (self.m.b.prg.len() / 0x2000) as i64 - 1;
        let mut bank = bank;
        if bank >= last - 1 {
            // the MMC3's fixed banks: $4E/$4F on the second chip
            bank = 0x4E + (bank - (last - 1));
        }
        self.m.b.map_prg(window, size_kib, bank.rem_euclid(0x50));
    }
}

impl Cartridge for Xiangfeng198 {
    mmc3_cartridge!(ppu_address);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5000..0x6000).contains(&address) {
            return Some(self.low_ram[address as usize & 0x0FFF] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            self.low_ram[address as usize & 0x0FFF] = value;
            return;
        }
        base_cpu_write(self, address, value, t);
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
        74 | 191 | 192 | 194 | 195 => Box::new(MixedChr::new(image, t)),
        176 => Box::new(Fk23c::new(image, t)),
        198 => Box::new(Xiangfeng198::new(image, t)),
        199 => Box::new(Fs309::new(image, t)),
        249 => Box::new(T9552::new(image, t)),
        252 | 253 => Box::new(WaixingVrc4::new(image, t)),
        523 => Box::new(Fk23c::jncota523(image, t)),
        _ => return Err(image),
    })
}
