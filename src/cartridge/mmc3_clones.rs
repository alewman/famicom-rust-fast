// SPDX-License-Identifier: MIT
//! MMC3 clones on pirate and multicart boards: outer banks, rewired
//! registers, 32 KiB PRG modes (`mmc3_clones.py`; each from its NESdev wiki
//! page, no judge ROM: the claims rest on sweeps against Mesen2).
//!
//! Every board is an [`Mmc3`] and an [`Mmc3Like`] (see `mmc3.rs`). Where the
//! reference subclasses a clone (`MRCM196` of `PRG32Override`, `Board158B` of
//! `SuperGame`, the flash boards of `FlashMMC3`), the subclass is the parent's
//! struct with the subclass's fields.

use super::banked::{Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::flash::Flash;
use super::ines::RomImage;
use super::mmc3::{base_cpu_write, base_ppu_address, base_ram_protect, Mmc3, Mmc3Like};
use crate::mmc3_cartridge;

/// The two `Mmc3Like` accessors, for a board whose `Mmc3` is the field `m`.
macro_rules! mmc3_field {
    () => {
        fn mmc3(&self) -> &Mmc3 {
            &self.m
        }
        fn mmc3_mut(&mut self) -> &mut Mmc3 {
            &mut self.m
        }
    };
}

/// The base's state; each constructor then calls `update()` where the
/// reference's `MMC3.__init__` calls `_update()` (reaching the overrides).
fn mmc3(image: RomImage, cpu_cycle_ticks: i64) -> Mmc3 {
    Mmc3::new(image, cpu_cycle_ticks)
}

// -- GA23C ---------------------------------------------------------------------

/// Mapper 45: the GA23C multicart ASIC, an MMC3 with four outer registers at
/// $6000 (INES_Mapper_045.wiki).
///
/// Power-on, which the wiki does not give (the MMC3's own registers are
/// unspecified). The CHR registers start as 0, 2, 4, 5, 6, 7, CHR in order:
/// MAME's BSD-3 mmc3.cpp (`mmc3_common_initialize`) starts them so for
/// unlicensed carts "that use VRAM and don't init banks properly", and the
/// three CHR-RAM dumps the reference checked (*7-in-1 Famicom Yarou* Vol. 2 and
/// 3, *8-Bit Slasher*) draw differently if they start at 0, so the games see it.
/// The outer registers start, and return on a reset, as $6001 leaves them ("as
/// a soft reset would", the wiki): 0, 0, $0F, 0, CHR-AND and PRG-AND keeping
/// every MMC3 bit, the masks MAME's `nes_bmc_hik8_device::pcb_reset` sets; two
/// menus (*23-in-1 Super Game*, *Mali Jiazu 7-in-1*) draw differently with a
/// CHR-AND of 0. OBSERVED closed-box, Mesen2 2.1.1 shows the same pages and
/// masks at power-on and after a reset that follows outer writes (the
/// reference's `scripts/provenance_probes.py 45-power-on`). No hardware test.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ga23c {
    pub m: Mmc3,
    pub outer: [u8; 4],
    pub next_register: usize,
    pub chr_rom: bool,
}

impl Ga23c {
    pub fn new(image: RomImage, t: i64) -> Ga23c {
        let chr_rom = !image.chr.is_empty();
        let mut board = Ga23c {
            m: mmc3(image, t),
            outer: [0, 0, 0x0F, 0],
            next_register: 0,
            chr_rom,
        };
        board.update();
        board.m.registers[..6].copy_from_slice(&[0, 2, 4, 5, 6, 7]);
        board.update();
        board
    }
}

impl Mmc3Like for Ga23c {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (o1, o2, o3) = (
            self.outer[1] as i64,
            self.outer[2] as i64,
            self.outer[3] as i64,
        );
        let mask = !o3 & 0x3F;
        let prg_or = o1 | ((o2 >> 6) << 8);
        self.m.b.map_prg(window, size_kib, (bank & mask) | prg_or);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (o0, o2) = (self.outer[0] as i64, self.outer[2] as i64);
        let bits = o2 & 0x0F;
        let mask = if bits >= 7 { 0xFF >> (15 - bits) } else { 0 };
        let chr_or = o0 | ((o2 & 0xF0) << 4);
        for i in 0..size_kib {
            let mut one = bank * size_kib as i64 + i as i64;
            if self.chr_rom {
                one = (one & mask) | chr_or;
            }
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Ga23c {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = [0, 0, 0x0F, 0];
        self.next_register = 0;
        self.update();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            if address & 0xF001 == 0x6001 {
                self.outer[3] &= !0x40;
                self.next_register = 0;
            } else if address & 0xF001 == 0x6000 && self.outer[3] & 0x40 == 0 {
                self.outer[self.next_register] = value;
                self.next_register = (self.next_register + 1) & 3;
                self.update();
                return;
            } else if self.outer[3] & 0x40 != 0 {
                base_cpu_write(self, address, value, t);
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- PRG32Override, MRCM196 ------------------------------------------------------

/// Mapper 189: TXC's and Yoko Soft's MMC3 games with a 32 KiB PRG register
/// (INES_Mapper_189.wiki); and its subclass, mapper 196 (MRCM's Mario hacks:
/// the MMC3's A0 wired to another address line, INES_Mapper_196.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Prg32Override {
    pub m: Mmc3,
    pub bank32: Option<i64>,
    /// 196: the submapper (the reference's `MRCM196.submapper`).
    pub mrcm196: Option<u8>,
}

impl Prg32Override {
    pub fn new(image: RomImage, t: i64) -> Prg32Override {
        let mrcm196 = (image.board.mapper == 196).then_some(image.board.submapper);
        let mut board = Prg32Override {
            m: mmc3(image, t),
            bank32: Some(0),
            mrcm196,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        if let Some(sub) = mrcm196 {
            if sub != 3 {
                board.bank32 = None;
                board.update();
            }
        }
        board
    }

    /// `PRG32Override.cpu_write`.
    fn prg32_cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x4020..0x8000).contains(&address) {
            self.bank32 = Some(((value & 0x0F) | (value >> 4)) as i64);
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

impl Mmc3Like for Prg32Override {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        match self.bank32 {
            None => self.m.b.map_prg(window, size_kib, bank),
            Some(bank32) => self.m.b.map_prg(window, 8, bank32 * 4 + window as i64),
        }
    }
}

impl Cartridge for Prg32Override {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let Some(sub) = self.mrcm196 else {
            self.prg32_cpu_write(address, value, t);
            return;
        };
        if address < 0x8000 {
            if (sub == 0 || sub == 3) && address >= 0x6000 {
                self.prg32_cpu_write(address, value, t);
            }
            return;
        }
        let a0 = match sub {
            1 | 3 => (address >> 1) & 1,
            2 => (address >> 2) & 1,
            _ => (address & 0x0E != 0 && address & 1 == 0) as u16,
        };
        self.prg32_cpu_write((address & !1) | a0, value, t);
    }
}

// -- SuperGame, Board158B --------------------------------------------------------

/// The 8 KiB bank an NROM override gives CPU window `window`.
fn nrom_bank(mode_bank: i64, thirty_two: bool, window: usize) -> i64 {
    let w = window as i64;
    let bank16 = if thirty_two {
        (mode_bank & !1) | (w >> 1)
    } else {
        mode_bank
    };
    bank16 * 2 + (w & 1)
}

type Scramble = &'static [(u16, u16)];

/// Address scrambling: (address written & $E001) -> the MMC3 register meant.
fn scramble_114(submapper: u8) -> Scramble {
    match submapper {
        0 => &[
            (0x8000, 0xA001),
            (0x8001, 0xA000),
            (0xA000, 0x8000),
            (0xA001, 0xC000),
            (0xC000, 0x8001),
        ],
        1 => &[
            (0x8000, 0xA001),
            (0xA000, 0x8000),
            (0xA001, 0xC001),
            (0xC000, 0xA000),
            (0xC001, 0xC000),
        ],
        _ => &[],
    }
}

fn scramble_215(pattern: u8) -> Scramble {
    match pattern {
        1 => &[
            (0x8000, 0xA001),
            (0x8001, 0xA000),
            (0xA000, 0x8000),
            (0xA001, 0xC000),
            (0xC000, 0x8001),
        ],
        3 => &[
            (0x8000, 0xC001),
            (0x8001, 0x8000),
            (0xA000, 0x8001),
            (0xA001, 0xA000),
            (0xC000, 0xA001),
            (0xC001, 0xE001),
            (0xE001, 0xC000),
        ],
        4 => &[
            (0x8000, 0xA001),
            (0xA000, 0x8000),
            (0xA001, 0xC000),
            (0xC000, 0xA000),
        ],
        _ => &[],
    }
}

/// Index scrambling: the low three bits written to $8000 -> the register meant.
const IDENTITY: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
const INDEX_114_0: [u8; 8] = [0, 3, 1, 5, 6, 7, 2, 4];

fn index_114(submapper: u8) -> [u8; 8] {
    match submapper {
        0 => INDEX_114_0,
        1 => [0, 2, 5, 3, 6, 1, 7, 4],
        _ => IDENTITY,
    }
}

fn index_215(pattern: u8) -> [u8; 8] {
    match pattern {
        1 => [0, 2, 6, 1, 7, 3, 4, 5],
        2 => [0, 5, 4, 1, 7, 2, 6, 3],
        3 => [0, 6, 3, 7, 5, 2, 4, 1],
        4 => [0, 2, 5, 3, 6, 1, 7, 4],
        _ => IDENTITY,
    }
}

/// Mapper 258's protection rows (MAME's mmc3_clones.cpp).
const PROTECTION_158B: [[u8; 8]; 8] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 3, 0],
    [0, 0, 0, 0, 3, 4, 0, 0],
    [0, 0, 0, 1, 0, 4, 5, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 2, 4, 15, 0],
];

/// Mappers 114/182, 123, 187 and 215: Kǎshèng's and SuperGame's MMC3 clones
/// with an NROM override register and scrambling (INES_Mapper_114.wiki,
/// INES_Mapper_215.wiki); and its subclass, mapper 258 (UNL-158B: 215's 8237
/// with protection, from MAME's mmc3_clones.cpp).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SuperGame {
    pub m: Mmc3,
    pub kind: u16,
    pub submapper: u8,
    pub mode: u8,
    pub chr_outer: u8,
    pub outer: u8,
    pub pattern: u8,
    pub big_chr: bool,
    /// 258: the protection row (the reference's `Board158B.row`).
    pub row: Option<u8>,
}

impl SuperGame {
    pub fn new(image: RomImage, t: i64) -> SuperGame {
        let mut image = image;
        let row = if image.board.mapper == 258 {
            image.board.mapper = 215;
            Some(7)
        } else {
            None
        };
        let board_mapper = image.board.mapper;
        let kind = if board_mapper == 182 {
            114
        } else {
            board_mapper
        };
        let submapper = image.board.submapper;
        let big_chr = kind == 187 && image.chr.len() > 0x40000;
        let mut board = SuperGame {
            m: mmc3(image, t),
            kind,
            submapper,
            mode: 0,
            chr_outer: 0,
            outer: if kind == 215 { 0x0F } else { 0 },
            pattern: 0,
            big_chr,
            row,
        };
        board.update();
        board.m.nec_irq = kind == 114;
        board.m.b.prg_ram_enabled = false;
        board
    }

    /// (active, 16 KiB bank, 32 KiB) from the override register.
    fn override_(&self) -> (bool, i64, bool) {
        let v = self.mode as i64;
        if self.kind == 123 {
            let one = ((v >> 4) | (v >> 3)) & 1;
            let bank = (v & 1) | (one << 1) | (((v >> 2) & 1) << 2) | (((v >> 5) & 1) << 3);
            return (v & 0x40 != 0, bank, v & 2 != 0);
        }
        if self.kind == 187 {
            return (v & 0x80 != 0, (v >> 1) & 0x0F, v & 0x20 != 0);
        }
        (v & 0x80 != 0, v & 0x0F, v & 0x20 != 0)
    }

    /// 215: (PRG outer in 128 KiB units, CHR outer in 128 KiB units, A17-replacing).
    fn outer_215(&self) -> (i64, i64, bool) {
        let o = self.outer as i64;
        let replace = self.mode & 0x40 != 0;
        let (mut prg, mut chr) = if self.submapper == 1 {
            ((o & 3) | ((o >> 1) & 4), (o >> 1) & 7)
        } else {
            (o & 3, (o >> 2) & 3)
        };
        prg <<= 1;
        chr <<= 1;
        if replace {
            prg |= (o >> 4) & 1;
            chr |= (o >> 5) & 1;
        }
        (prg, chr, replace)
    }

    /// `SuperGame.cpu_read`.
    fn super_game_cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if self.kind == 187 && (0x5000..0x6000).contains(&address) {
            return Some(0x80 | (0x7F << 8));
        }
        self.m.b.base_cpu_read(address, t)
    }

    /// `SuperGame.cpu_write`.
    fn super_game_cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let kind = self.kind;
        let mut value = value;
        if address < 0x8000 {
            if (kind == 114 && address & 0xE001 == 0x6000)
                || (kind == 187 && address & 0xF001 == 0x5000)
            {
                self.mode = value;
            } else if kind == 114 && address & 0xE001 == 0x6001 {
                self.chr_outer = value & 1;
            } else if (kind == 123 && address & 0xF800 == 0x5800)
                || (kind == 215 && address & 0xF007 == 0x5000)
            {
                self.mode = value;
            } else if kind == 215 && address & 0xF007 == 0x5001 {
                self.outer = value;
            } else if kind == 215 && address & 0xF007 == 0x5007 {
                self.pattern = value & 7;
            } else {
                base_cpu_write(self, address, value, t);
                return;
            }
            self.update();
            return;
        }
        let (table, index): (Scramble, [u8; 8]) = if kind == 114 {
            (scramble_114(self.submapper), index_114(self.submapper))
        } else if kind == 215 {
            (scramble_215(self.pattern), index_215(self.pattern))
        } else if kind == 123 {
            (&[], INDEX_114_0)
        } else {
            (&[], IDENTITY)
        };
        let key = address & 0xE001;
        let register = table
            .iter()
            .find(|(from, _)| *from == key)
            .map_or(key, |&(_, to)| to);
        if register == 0x8000 {
            value = (value & 0xC0) | index[(value & 7) as usize];
        }
        base_cpu_write(self, register, value, t);
    }
}

impl Mmc3Like for SuperGame {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (active, bank16, thirty_two) = self.override_();
        let mut bank = bank;
        if self.kind == 215 {
            let (prg, _, replace) = self.outer_215();
            let inner = if replace { 0x0F } else { 0x1F };
            if active {
                bank = nrom_bank(bank16 & (inner >> 1), thirty_two, window);
            }
            self.m
                .b
                .map_prg(window, size_kib, (bank & inner) | (prg << 4));
            return;
        }
        if active {
            bank = nrom_bank(bank16, thirty_two, window);
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            let w = window + i;
            let mut one = bank * size_kib as i64 + i as i64;
            if self.kind == 215 {
                let (_, chr, replace) = self.outer_215();
                one = (one & if replace { 0x7F } else { 0xFF }) | (chr << 7);
            } else if self.kind == 114 {
                one = (one & 0xFF) | ((self.chr_outer as i64) << 8);
            } else if self.big_chr {
                one = (one & 0xFF) | (((w >> 2) as i64) << 8);
            }
            self.m.b.map_chr(w, 1, one);
        }
    }
}

impl Cartridge for SuperGame {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        if self.kind == 215 {
            self.outer = 0x0F;
            self.update();
        }
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if let Some(row) = self.row {
            if (0x5000..0x6000).contains(&address) {
                return Some(
                    PROTECTION_158B[row as usize][(address & 7) as usize] as u16 | (0xF0 << 8),
                );
            }
        }
        self.super_game_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.row.is_some() && address & 0xF003 == 0x5002 {
            self.row = Some(value & 7);
            return;
        }
        self.super_game_cpu_write(address, value, t);
    }
}

// -- A9711 ---------------------------------------------------------------------

/// Mapper 121's protection array, indexed by the last write to $5000-$5FFF.
const A9711_ARRAY: [u8; 4] = [0x83, 0x83, 0x42, 0x00];

fn reverse6(value: u8) -> u8 {
    (value & 0xC0)
        | (0..6)
            .map(|bit| ((value >> bit) & 1) << (5 - bit))
            .sum::<u8>()
}

/// Mapper 121: Kǎshèng's A9711/A9713 protected boards (INES_Mapper_121.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct A9711 {
    pub m: Mmc3,
    /// The protection's PRG banks by CPU window (the reference's `overrides`).
    pub overrides: [Option<i64>; 4],
    pub protection_index: u8,
    pub protection_value: u8,
    pub array_index: usize,
    pub outer: u8,
    pub big_chr: bool,
    pub big_prg: bool,
}

impl A9711 {
    pub fn new(image: RomImage, t: i64) -> A9711 {
        let big_chr = image.chr.len() > 0x40000;
        let big_prg = image.prg.len() > 0x40000;
        let mut board = A9711 {
            m: mmc3(image, t),
            overrides: [None; 4],
            protection_index: 0,
            protection_value: 0,
            array_index: 0,
            outer: 0,
            big_chr,
            big_prg,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        board
    }

    fn apply(&mut self, index: u8, persistent: bool) {
        let value = self.protection_value as i64;
        match index {
            0x26 => self.overrides[3] = Some(value),
            0x28 => self.overrides[2] = Some(value),
            0x2A => self.overrides[1] = Some(value),
            0x2C => {
                if value != 0 {
                    self.overrides[3] = Some(value);
                }
            }
            0x20 | 0x29 | 0x2B | 0x3C | 0x3F => {
                if !persistent {
                    self.overrides[3] = Some(value);
                }
            }
            _ => {
                if index != 0x2F && !persistent {
                    self.overrides = [None; 4];
                }
            }
        }
        self.update();
    }
}

impl Mmc3Like for A9711 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let mut bank = self.overrides[window].unwrap_or(bank);
        if self.big_prg {
            bank = (bank & 0x1F) | ((self.outer as i64) << 5);
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            let w = window + i;
            let mut one = bank * size_kib as i64 + i as i64;
            if self.big_chr {
                one = (one & 0xFF) | (((w >> 2) as i64) << 8);
            } else if self.big_prg {
                one = (one & 0xFF) | ((self.outer as i64) << 8);
            }
            self.m.b.map_chr(w, 1, one);
        }
    }
}

impl Cartridge for A9711 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5000..0x6000).contains(&address) {
            return Some(A9711_ARRAY[self.array_index] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            if self.big_prg && address & 0xF180 == 0x5180 {
                self.outer = value >> 7;
                self.update();
            }
            self.array_index = (value & 3) as usize;
            return;
        }
        let register = address & 0xE003;
        if register == 0x8001 {
            self.protection_value = reverse6(value);
            base_cpu_write(self, 0x8001, value, t);
            if matches!(self.protection_index, 0x26 | 0x28 | 0x2A) {
                self.apply(self.protection_index, true);
            }
            return;
        }
        if register == 0x8003 {
            self.protection_index = value & 0x3F;
            base_cpu_write(self, 0x8000, value, t);
            self.apply(self.protection_index, false);
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- Rewired197 ----------------------------------------------------------------

/// Mapper 197: for each real 1 KiB PPU window, the MMC3 window it sees, by submapper.
const REWIRED_197: [[usize; 8]; 4] = [
    [0, 0, 1, 1, 4, 4, 5, 5],
    [2, 2, 3, 3, 6, 6, 7, 7],
    [0, 0, 3, 3, 4, 4, 7, 7],
    [0, 0, 1, 1, 4, 4, 5, 5],
];

/// Mapper 197: Kǎshèng's mapper hacks, an MMC3 rewired for 512 KiB of CHR in
/// 2/4 KiB banks (INES_Mapper_197.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rewired197 {
    pub m: Mmc3,
    pub wiring: [usize; 8],
    pub outer: u8,
    pub ram_control: u8,
    pub mmc3_banks: [i64; 8],
}

impl Rewired197 {
    pub fn new(image: RomImage, t: i64) -> Rewired197 {
        let sub = image.board.submapper as usize;
        let wiring = if sub < 4 {
            REWIRED_197[sub]
        } else {
            REWIRED_197[0]
        };
        let mut board = Rewired197 {
            m: mmc3(image, t),
            wiring,
            outer: 0,
            ram_control: 0x80,
            mmc3_banks: [0; 8],
        };
        board.update();
        board
    }
}

impl Mmc3Like for Rewired197 {
    mmc3_field!();
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            self.mmc3_banks[window + i] = bank * size_kib as i64 + i as i64;
        }
        for real in 0..8 {
            let one = self.mmc3_banks[self.wiring[real]] * 2 + (real as i64 & 1);
            self.m.b.map_chr(real, 1, one);
        }
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let mut bank = bank;
        if self.outer & 8 != 0 {
            bank = (bank & 0x0F) | (((self.outer & 1) as i64) << 4);
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
}

impl Cartridge for Rewired197 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        // The reference compares the wiring tuples, so submappers 0 (and any
        // unknown one), whose wiring equals 3's, take 197.3's register too.
        if (0x6000..0x8000).contains(&address) && self.wiring == REWIRED_197[3] {
            if self.ram_control & 0xC0 == 0x80 {
                self.outer = value;
                self.update();
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- F003 ----------------------------------------------------------------------

/// Mapper 245: Waixing's F003, TNROM-like with the MMC3's CHR A11 as PRG A19
/// (INES_Mapper_245.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct F003 {
    pub m: Mmc3,
}

impl F003 {
    pub fn new(image: RomImage, t: i64) -> F003 {
        let mut board = F003 { m: mmc3(image, t) };
        board.update();
        board
    }
}

impl Mmc3Like for F003 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let r = self.m.registers;
        let half = ((if self.m.bank_select & 0x80 != 0 {
            r[2]
        } else {
            r[0]
        } >> 1)
            & 1) as i64;
        self.m
            .b
            .map_prg(window, size_kib, (bank & 0x3F) | (half << 6));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, _bank: i64) {
        let len = self.m.b.chr.len();
        for i in 0..size_kib {
            self.m.b.chr_offsets[window + i] = (window + i) * 0x400 % len;
        }
    }
}

impl Cartridge for F003 {
    mmc3_cartridge!(cpu_write, ppu_address);
}

// -- Latched165 ------------------------------------------------------------------

/// Mapper 165: an MMC3 with MMC2-style CHR latches and 4 KiB of CHR-RAM as
/// page 0 (INES_Mapper_165.wiki). The latches flip on PPU addresses
/// $xFD0-$xFD7 (0) and $xFE8-$xFEF (1), after the access. OBSERVED closed-box:
/// Mesen2 2.1.1 has the same ranges (the reference's
/// `scripts/provenance_probes.py 165-latch-ranges`). Not settled: Disch's notes
/// say the game reads $xFD0 or $xFE0, and the games do read $1FD0 and $1FE0
/// through $2007, which $xFE8-$xFEF misses; their CHR-RAM uploads land either
/// way, while their pictures then differ, with no judge to say which is right.
/// No hardware source.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latched165 {
    pub m: Mmc3,
    pub latches: [u8; 2],
    pub ram_half: [bool; 2],
    pub rom: Vec<u8>,
    pub chr_ram: Vec<u8>,
}

impl Latched165 {
    pub fn new(image: RomImage, t: i64) -> Latched165 {
        let rom = image.chr.clone();
        let mut board = Latched165 {
            m: mmc3(image, t),
            latches: [0, 0],
            ram_half: [false, false],
            rom,
            chr_ram: vec![0; 0x1000],
        };
        board.update();
        board
    }
}

impl Mmc3Like for Latched165 {
    mmc3_field!();
    fn map_chr(&mut self, _window: usize, _size_kib: usize, _bank: i64) {
        let r = self.m.registers;
        for half in 0..2 {
            let index = if half == 0 {
                if self.latches[0] != 0 {
                    1
                } else {
                    0
                }
            } else if self.latches[1] != 0 {
                4
            } else {
                2
            };
            let page = (r[index] >> 2) as usize;
            self.ram_half[half] = page == 0;
            for i in 0..4 {
                self.m.b.chr_offsets[half * 4 + i] = if page == 0 {
                    i * 0x400
                } else {
                    (page * 0x1000 + i * 0x400) % self.rom.len()
                };
            }
        }
    }
}

impl Cartridge for Latched165 {
    mmc3_cartridge!(cpu_write);
    fn ppu_address(&mut self, address: u16, t: i64) {
        base_ppu_address(self, address, t);
        let low = address & 0x2FF8;
        if low == 0x0FD0 || low == 0x0FE8 {
            self.latches[((address >> 12) & 1) as usize] = (low == 0x0FE8) as u8;
            self.map_chr(0, 8, 0);
        }
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            let offset = self.m.b.chr_offsets[window] + (address as usize & 0x3FF);
            return Some(if self.ram_half[window >> 2] {
                self.chr_ram[offset]
            } else {
                self.rom[offset]
            });
        }
        self.m.b.base_ppu_read(address, t)
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            if self.ram_half[window >> 2] {
                let offset = self.m.b.chr_offsets[window] + (address as usize & 0x3FF);
                self.chr_ram[offset] = value;
            }
            return;
        }
        self.m.b.base_ppu_write(address, value, t);
    }
}

// -- T4A54A --------------------------------------------------------------------

/// Mapper 134: the T4A54A / WX-KB4K / BS-5652 MMC3 multicart (INES_Mapper_134.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct T4a54a {
    pub m: Mmc3,
    pub outer: [u8; 3],
}

impl T4a54a {
    pub fn new(image: RomImage, t: i64) -> T4a54a {
        let mut board = T4a54a {
            m: mmc3(image, t),
            outer: [0; 3],
        };
        board.update();
        board
    }
}

impl Mmc3Like for T4a54a {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, outer) = (self.outer[0] as i64, self.outer[1] as i64);
        let w = window as i64;
        let r6 = self.m.registers[6] as i64;
        let mut bank = bank;
        if outer & 0x80 != 0 {
            bank = (r6 & !3) | (w & 1);
            if outer & 8 == 0 {
                bank |= w & 2;
            } else {
                bank |= r6 & 2;
            }
        }
        let small = outer & 4 != 0;
        let inner = if small { 0x0F } else { 0x1F };
        let high = (if small { outer & 3 } else { outer & 2 }) | (((mode >> 2) & 1) << 2);
        self.m
            .b
            .map_prg(window, size_kib, (bank & inner) | (high << 4));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, outer) = (self.outer[0] as i64, self.outer[1] as i64);
        let small = outer & 0x40 != 0;
        let inner = if small { 0x7F } else { 0xFF };
        let high = (if small {
            (outer >> 4) & 3
        } else {
            (outer >> 4) & 2
        }) | (((mode >> 5) & 1) << 2);
        for i in 0..size_kib {
            let cnrom = mode & 8 != 0;
            let one = if cnrom {
                self.outer[2] as i64 * 8 + window as i64 + i as i64
            } else {
                bank * size_kib as i64 + i as i64
            };
            self.m.b.map_chr(window + i, 1, (one & inner) | (high << 7));
        }
    }
}

impl Cartridge for T4a54a {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = [0; 3];
        self.update();
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.outer[0] & 0x40 != 0 {
            return Some(0);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            let register = (address & 3) as usize;
            if register < 3 {
                if self.outer[0] & 0x80 != 0 {
                    if register == 2 {
                        self.outer[2] = (self.outer[2] & !3) | (value & 3);
                    }
                } else {
                    self.outer[register] = value;
                }
                self.update();
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- ResetTxROM ------------------------------------------------------------------

/// Mapper 313: (first game's 128 KiB PRG blocks, other games', CHR 128 KiB blocks).
fn reset_txrom_sizes(submapper: u8) -> (i64, i64, i64) {
    match submapper {
        1 => (2, 2, 1),
        2 => (1, 1, 2),
        3 => (2, 2, 2),
        4 => (2, 1, 1),
        _ => (1, 1, 1),
    }
}

/// NES 2.0 mapper 313 (BMC-RESET-TXROM): MMC3 multicarts whose game advances
/// on each reset (NES_2.0_Mapper_313.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ResetTxRom {
    pub m: Mmc3,
    pub sizes: (i64, i64, i64),
    pub game: i64,
}

impl ResetTxRom {
    pub fn new(image: RomImage, t: i64) -> ResetTxRom {
        let sizes = reset_txrom_sizes(image.board.submapper);
        let mut board = ResetTxRom {
            m: mmc3(image, t),
            sizes,
            game: 0,
        };
        board.update();
        board
    }

    /// (first 8 KiB bank of the current game, its banks - 1).
    fn prg_base(&self) -> (i64, i64) {
        let (first, others, _) = self.sizes;
        if self.game == 0 {
            return (0, first * 16 - 1);
        }
        ((first + (self.game - 1) * others) * 16, others * 16 - 1)
    }
}

impl Mmc3Like for ResetTxRom {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (base, mask) = self.prg_base();
        let banks = (self.m.b.prg.len() / 0x2000) as i64;
        let mut bank = bank;
        if bank >= banks - 2 {
            bank = mask - (banks - 1 - bank);
        }
        self.m.b.map_prg(window, size_kib, base + (bank & mask));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let blocks = self.sizes.2;
        let (base, mask) = (self.game * blocks * 128, blocks * 128 - 1);
        for i in 0..size_kib {
            self.m.b.map_chr(
                window + i,
                1,
                base + ((bank * size_kib as i64 + i as i64) & mask),
            );
        }
    }
}

impl Cartridge for ResetTxRom {
    mmc3_cartridge!(cpu_write, ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.game = (self.game + 1) & 3;
        self.update();
    }
}

// -- Rex263, Nitra250 ------------------------------------------------------------

/// NES 2.0 mapper 263 (UNL-KOF97): the MMC3 clone's A0 is CPU A12, the data
/// lines shuffled (NES_2.0_Mapper_263.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rex263 {
    pub m: Mmc3,
}

impl Rex263 {
    pub fn new(image: RomImage, t: i64) -> Rex263 {
        let mut board = Rex263 { m: mmc3(image, t) };
        board.update();
        board
    }
}

impl Mmc3Like for Rex263 {
    mmc3_field!();
}

impl Cartridge for Rex263 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            base_cpu_write(self, address, value, t);
            return;
        }
        let register = (address & 0xE000) | ((address >> 12) & 1);
        let value = (value & 0xD8)
            | ((value & 0x20) >> 4)
            | ((value & 4) << 3)
            | ((value & 2) >> 1)
            | ((value & 1) << 2);
        base_cpu_write(self, register, value, t);
    }
}

/// Mapper 250 (Nitra): an MMC3 wired to the address bus: A0 is CPU A10, the
/// data the address's low byte (INES_Mapper_250.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Nitra250 {
    pub m: Mmc3,
}

impl Nitra250 {
    pub fn new(image: RomImage, t: i64) -> Nitra250 {
        let mut board = Nitra250 { m: mmc3(image, t) };
        board.update();
        board
    }
}

impl Mmc3Like for Nitra250 {
    mmc3_field!();
}

impl Cartridge for Nitra250 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            base_cpu_write(self, address, value, t);
            return;
        }
        let register = (address & 0xE000) | ((address >> 10) & 1);
        base_cpu_write(self, register, (address & 0xFF) as u8, t);
    }
}

// -- Gouder208 -------------------------------------------------------------------

/// Mapper 208's protection: the value XORed into a write to $5800-$5FFF, by the
/// index last written to $5000-$57FF. No hardware description of the chip exists
/// (the wiki's table is an emulator's). OBSERVED closed-box: recovered from
/// Mesen2 2.1.1 by writing $00 and $FF at every index and reading the registers
/// back (the reference's `scripts/provenance_probes.py 208-protection`, all 512
/// answers), measured with the program run as an instrument, not read. The
/// game's own code confirms the five entries it jumps through ($09, $0D, $62,
/// $8E, $B3: tests/mappers_since_pin_1.rs carries the reference's test).
const PROTECTION_208: [u8; 256] = {
    const HEX: &[u8] = b"\
59595959595959595949190959491909\
59595959595959595141110151411101\
59595959595959595949190959491909\
59595959595959595141110151411101\
00104050001040500000000000000000\
08184858081848580000000000000000\
00104050001040500000000000000000\
08184858081848580000000000000000\
59595959595959595848180858481808\
59595959595959595040100050401000\
59595959595959595848180858481808\
59595959595959595040100050401000\
01114151011141510000000000000000\
09194959091949590000000000000000\
01114151011141510000000000000000\
09194959091949590000000000000000";
    const fn nibble(c: u8) -> u8 {
        if c <= b'9' {
            c - b'0'
        } else {
            c - b'A' + 10
        }
    }
    let mut out = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        out[i] = nibble(HEX[2 * i]) << 4 | nibble(HEX[2 * i + 1]);
        i += 1;
    }
    out
};

/// Mapper 208 (Street Fighter IV, Gouder): MMC3 CHR and IRQ, a 32 KiB PRG
/// register, protection (INES_Mapper_208.wiki; the page's register map is an
/// emulator's, FCEUX's, as it says; the table is measured, `PROTECTION_208`).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Gouder208 {
    pub m: Mmc3,
    pub plain: bool,
    pub prg_mode: u8,
    pub index: u8,
    pub protection: [u8; 4],
}

impl Gouder208 {
    pub fn new(image: RomImage, t: i64) -> Gouder208 {
        let plain = image.board.submapper == 1;
        let mut board = Gouder208 {
            m: mmc3(image, t),
            plain,
            prg_mode: 0x11,
            index: 0,
            protection: [0; 4],
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        if !plain {
            let mode = if board.prg_mode & 0x20 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            };
            board.m.b.set_mirroring(mode);
        }
        board
    }
}

impl Mmc3Like for Gouder208 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, _size_kib: usize, _bank: i64) {
        let bank32 = if self.plain {
            (self.m.registers[6] >> 2) as i64
        } else {
            (((self.prg_mode >> 3) & 2) | (self.prg_mode & 1)) as i64
        };
        self.m.b.map_prg(window, 8, bank32 * 4 + window as i64);
    }
}

impl Cartridge for Gouder208 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if !self.plain && (0x5800..0x6000).contains(&address) {
            return Some(self.protection[(address & 3) as usize] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if !self.plain && address < 0x8000 {
            if (0x4800..0x5000).contains(&address) || (0x6800..0x7000).contains(&address) {
                self.prg_mode = value;
                self.m.b.set_mirroring(if value & 0x20 != 0 {
                    HORIZONTAL
                } else {
                    VERTICAL
                });
                self.update();
            } else if (0x5000..0x5800).contains(&address) {
                self.index = value;
            } else if (0x5800..0x6000).contains(&address) {
                self.protection[(address & 3) as usize] =
                    value ^ PROTECTION_208[self.index as usize];
            }
            return;
        }
        if !self.plain && address & 0xE000 == 0xA000 && address & 1 == 0 {
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- JC016 ---------------------------------------------------------------------

/// Mapper 205's blocks: (PRG AND, PRG OR, CHR AND, CHR OR) (INES_Mapper_205.wiki).
const JC016_BLOCKS: [(i64, i64, i64, i64); 4] = [
    (0x1F, 0x00, 0xFF, 0x000),
    (0x1F, 0x10, 0xFF, 0x080),
    (0x0F, 0x20, 0x7F, 0x100),
    (0x0F, 0x30, 0x7F, 0x180),
];

/// Mapper 205 (BMC-JC-016-2): an MMC3 inside four blocks (INES_Mapper_205.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jc016 {
    pub m: Mmc3,
    pub block: usize,
}

impl Jc016 {
    pub fn new(image: RomImage, t: i64) -> Jc016 {
        let mut board = Jc016 {
            m: mmc3(image, t),
            block: 0,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        board
    }
}

impl Mmc3Like for Jc016 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (prg_and, prg_or, _, _) = JC016_BLOCKS[self.block];
        self.m
            .b
            .map_prg(window, size_kib, (bank & prg_and) | prg_or);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (_, _, chr_and, chr_or) = JC016_BLOCKS[self.block];
        for i in 0..size_kib {
            self.m.b.map_chr(
                window + i,
                1,
                ((bank * size_kib as i64 + i as i64) & chr_and) | chr_or,
            );
        }
    }
}

impl Cartridge for Jc016 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.block = (value & 3) as usize;
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- SL5020B -------------------------------------------------------------------

/// Mapper 12 (Gouder's SL-5020B): the Huang-1 as an MMC3A with CHR A18
/// (INES_Mapper_012.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sl5020b {
    pub m: Mmc3,
    pub chr_high: u8,
}

impl Sl5020b {
    pub fn new(image: RomImage, t: i64) -> Sl5020b {
        let mut board = Sl5020b {
            m: mmc3(image, t),
            chr_high: 0,
        };
        board.update();
        board.m.nec_irq = true;
        board
    }
}

impl Mmc3Like for Sl5020b {
    mmc3_field!();
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            let w = window + i;
            let a18 = ((self.chr_high >> if w >= 4 { 4 } else { 0 }) & 1) as i64;
            self.m.b.map_chr(
                w,
                1,
                ((bank * size_kib as i64 + i as i64) & 0xFF) | (a18 << 8),
            );
        }
    }
}

impl Cartridge for Sl5020b {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xE100 == 0x4100 && address < 0x6000 {
            return Some(0xFE << 8);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE100 == 0x4100 && address < 0x6000 {
            self.chr_high = value;
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- BMW8544 -------------------------------------------------------------------

/// NES 2.0 mapper 292 (Flying Star's Dragon Fighter): MMC3 PRG/IRQ, custom
/// CHR banking (NES_2.0_Mapper_292.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Bmw8544 {
    pub m: Mmc3,
    pub extra: [u8; 2],
    pub data_latch: u8,
    pub index: u8,
    pub ram_control: u8,
}

impl Bmw8544 {
    pub fn new(image: RomImage, t: i64) -> Bmw8544 {
        let mut board = Bmw8544 {
            m: mmc3(image, t),
            extra: [0, 0],
            data_latch: 0,
            index: 0,
            ram_control: 0x80,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Bmw8544 {
    mmc3_field!();
    fn map_chr(&mut self, _window: usize, _size_kib: usize, _bank: i64) {
        let r = self.m.registers;
        let inverted = self.m.bank_select & 0x80 != 0;
        let (first, second) = if inverted {
            (r[2] >> 1, r[4] >> 1)
        } else {
            (r[0] >> 1, r[1] >> 1)
        };
        let low = (self.extra[0] ^ first) as i64;
        let high = (((self.extra[1] << 1) & 0x80) ^ second) as i64;
        for i in 0..2 {
            self.m.b.map_chr(i, 1, low * 2 + i as i64);
            self.m.b.map_chr(2 + i, 1, high * 2 + i as i64);
        }
        for i in 0..4 {
            self.m
                .b
                .map_chr(4 + i, 1, (self.extra[1] & 0x3F) as i64 * 4 + i as i64);
        }
    }
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
}

impl Cartridge for Bmw8544 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) && self.ram_control & 0x80 != 0 {
            self.extra[((self.index >> 5) & 1) as usize] = self.data_latch;
            self.update();
            return None;
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        self.data_latch = value;
        if (0x6000..0x8000).contains(&address) {
            if self.ram_control & 0x80 != 0 {
                self.index = value;
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- Board513 ------------------------------------------------------------------

/// NES 2.0 mapper 513 (Mei Shaonu Meng Gongchang): MMC3, 32 KiB of CHR-RAM,
/// an outer bank (NES_2.0_Mapper_513.wiki; bank 3 mirrors 2, as FCEUX).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Board513 {
    pub m: Mmc3,
}

impl Board513 {
    pub fn new(image: RomImage, t: i64) -> Board513 {
        let mut board = Board513 { m: mmc3(image, t) };
        board.update();
        board.m.b.chr = vec![0; 0x8000];
        board.m.b.chr_writable = true;
        board.update();
        board
    }
}

impl Mmc3Like for Board513 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let last = (self.m.b.prg.len() / 0x2000) as i64 - 1;
        let bank = if bank < last - 1 {
            let r = self.m.registers;
            let reg = if self.m.bank_select & 0x80 != 0 {
                r[2]
            } else {
                r[0]
            };
            let outer = ((reg >> 6) as i64).min(2);
            (outer << 6) | (bank & 0x3F)
        } else {
            0x3E | (bank & 1)
        };
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            self.m
                .b
                .map_chr(window + i, 1, (bank * size_kib as i64 + i as i64) & 0x1F);
        }
    }
}

impl Cartridge for Board513 {
    mmc3_cartridge!(cpu_write, ppu_address);
    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.m.b.battery {
            Some(self.m.b.chr.clone())
        } else {
            None
        }
    }
    fn restore(&mut self, data: &[u8]) {
        self.m.b.chr.copy_from_slice(data);
    }
}

// -- XorRAM254 -----------------------------------------------------------------

/// Mapper 254 (Pikachu Y2K): MMC3 whose PRG-RAM reads are XORed until $8000
/// is written (INES_Mapper_254.wiki, from MesenCE's source).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct XorRam254 {
    pub m: Mmc3,
    pub xor_mask: u8,
    pub scrambling: bool,
}

impl XorRam254 {
    pub fn new(image: RomImage, t: i64) -> XorRam254 {
        let mut board = XorRam254 {
            m: mmc3(image, t),
            xor_mask: 0,
            scrambling: true,
        };
        board.update();
        board
    }
}

impl Mmc3Like for XorRam254 {
    mmc3_field!();
}

impl Cartridge for XorRam254 {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.scrambling = true;
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let value = self.m.b.base_cpu_read(address, t);
        if let Some(v) = value {
            if (0x6000..0x8000).contains(&address) && self.scrambling {
                return Some(v ^ self.xor_mask as u16);
            }
        }
        value
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            if address & 0xE001 == 0x8000 {
                self.scrambling = false;
            } else if address & 0xE001 == 0xA001 {
                self.xor_mask = value;
            }
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- SachenShero ---------------------------------------------------------------

/// NES 2.0 mapper 262 (UNL-SHERO: Sachen's Street Heroes): MMC3, CHR-ROM or
/// -RAM, four screens (NES_2.0_Mapper_262.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SachenShero {
    pub m: Mmc3,
    pub high: u8,
    pub chr_ram: Vec<u8>,
}

impl SachenShero {
    pub fn new(image: RomImage, t: i64) -> SachenShero {
        let mut board = SachenShero {
            m: mmc3(image, t),
            high: 0,
            chr_ram: vec![0; 0x2000],
        };
        board.update();
        // The reference's `rom` (the Banked CHR) is never read apart from it.
        board.m.b.four_screen = true;
        board.m.b.vram = vec![0; 0x1000];
        board
    }
}

impl Mmc3Like for SachenShero {
    mmc3_field!();
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        const SHIFT: [u8; 8] = [3, 3, 2, 2, 0, 0, 1, 1];
        for i in 0..size_kib {
            let w = window + i;
            let a18 = ((self.high >> SHIFT[w]) & 1) as i64;
            self.m.b.map_chr(
                w,
                1,
                ((bank * size_kib as i64 + i as i64) & 0xFF) | (a18 << 8),
            );
        }
    }
}

impl Cartridge for SachenShero {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xC100 == 0x4100 && address < 0x6000 {
            return Some(0x7F << 8);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xC100 == 0x4100 && address < 0x6000 {
            self.high = value;
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && self.high & 0x40 != 0 {
            return Some(self.chr_ram[address as usize]);
        }
        self.m.b.base_ppu_read(address, t)
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            if self.high & 0x40 != 0 {
                self.chr_ram[address as usize] = value;
            }
            return;
        }
        self.m.b.base_ppu_write(address, value, t);
    }
}

// -- Outer52 -------------------------------------------------------------------

/// Mapper 52 ("Mario Party" 7-in-1 and kin): MMC3 with an outer register at
/// $6000 (INES_Mapper_052.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Outer52 {
    pub m: Mmc3,
    pub outer: u8,
    pub ram_control: u8,
}

impl Outer52 {
    pub fn new(image: RomImage, t: i64) -> Outer52 {
        let mut board = Outer52 {
            m: mmc3(image, t),
            outer: 0,
            ram_control: 0x80,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Outer52 {
    mmc3_field!();
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let o = self.outer as i64;
        let high = (o & 3) | (((o >> 2) & 1) << 2);
        let small = o & 8 != 0;
        let (inner, outer) = if small {
            (0x0F, high)
        } else {
            (0x1F, high & 6)
        };
        self.m
            .b
            .map_prg(window, size_kib, (bank & inner) | (outer << 4));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let o = self.outer as i64;
        let high = ((o >> 4) & 3) | (((o >> 2) & 1) << 2);
        for i in 0..size_kib {
            let small = o & 0x40 != 0;
            let (inner, outer) = if small {
                (0x7F, high)
            } else {
                (0xFF, high & 6)
            };
            self.m.b.map_chr(
                window + i,
                1,
                ((bank * size_kib as i64 + i as i64) & inner) | (outer << 7),
            );
        }
    }
}

impl Cartridge for Outer52 {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = 0;
        self.update();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            if self.ram_control & 0xC0 == 0x80 && self.outer & 0x80 == 0 {
                self.outer = value;
                self.update();
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- HPxx260 -------------------------------------------------------------------

/// NES 2.0 mapper 260 (BMC-HPxx, BMC-HP2018-A multicarts) (NES_2.0_Mapper_260.wiki;
/// its CNROM mode from FCEUX's source).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hpxx260 {
    pub m: Mmc3,
    pub outer: [u8; 3],
    pub cnrom_latch: u8,
}

impl Hpxx260 {
    pub fn new(image: RomImage, t: i64) -> Hpxx260 {
        let mut board = Hpxx260 {
            m: mmc3(image, t),
            outer: [0; 3],
            cnrom_latch: 0,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Hpxx260 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, base) = ((self.outer[0] & 7) as i64, self.outer[1] as i64);
        let w = window as i64;
        let bank = if mode == 4 {
            base * 2 + (w & 1)
        } else if mode >= 5 {
            (base & !1) * 2 + w
        } else {
            let inner = if mode & 2 != 0 { 0x0F } else { 0x1F };
            (bank & inner) | ((base * 2) & !inner)
        };
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, base) = ((self.outer[0] & 7) as i64, self.outer[2] as i64);
        for i in 0..size_kib {
            let w = (window + i) as i64;
            let one = if mode < 4 {
                let inner = if mode & 1 != 0 { 0x7F } else { 0xFF };
                ((bank * size_kib as i64 + i as i64) & inner) | ((base * 8) & !inner)
            } else if mode < 6 {
                base * 8 + w
            } else {
                let mask = if mode == 6 { 1 } else { 3 };
                ((base & !mask) | (self.cnrom_latch as i64 & mask)) * 8 + w
            };
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Hpxx260 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address == 0x5000 {
            return Some(0xFC << 8);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            if self.outer[0] & 0x80 == 0 && address & 3 < 3 {
                self.outer[(address & 3) as usize] = value;
                self.update();
            }
            return;
        }
        if address >= 0x8000 {
            self.cnrom_latch = value;
            if self.outer[0] & 4 != 0 {
                self.m
                    .b
                    .set_mirroring(if value & 4 != 0 { HORIZONTAL } else { VERTICAL });
                self.update();
            }
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- A9461 ---------------------------------------------------------------------

/// Mapper 219 (Kǎshèng's A9461): an MMC3 with an extended mode
/// (INES_Mapper_219.wiki, FCEUX's and Nestopia's implementations).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct A9461 {
    pub m: Mmc3,
    pub extended: bool,
    pub outer: u8,
    pub prg_banks: [Option<i64>; 4],
    pub chr_banks: [Option<i64>; 8],
    pub chr_latch: u8,
}

impl A9461 {
    pub fn new(image: RomImage, t: i64) -> A9461 {
        let mut board = A9461 {
            m: mmc3(image, t),
            extended: false,
            outer: 3,
            prg_banks: [None; 4],
            chr_banks: [None; 8],
            chr_latch: 0,
        };
        board.update();
        board
    }

    fn extended_write(&mut self, value: u8) {
        let index = self.m.bank_select & 0x3F;
        if (0x23..=0x26).contains(&index) {
            let bits = (value >> 2) & 0x0F;
            let bank: i64 = (0..4).map(|b| (((bits >> b) & 1) as i64) << (3 - b)).sum();
            let window = match index {
                0x26 => 0,
                0x25 => 1,
                0x24 => 2,
                _ => 3,
            };
            self.prg_banks[window] = Some(bank);
        } else if index & 1 == 0 {
            self.chr_latch = value & 7;
        } else if index <= 0x0F {
            let window = ((index >> 1) & 3) as usize;
            self.chr_banks[window] = Some(
                (value >> 1) as i64 | ((index >> 1) & 1) as i64 | ((self.chr_latch as i64) << 4),
            );
        } else {
            let window = 4 + ((index >> 2) & 3) as usize;
            self.chr_banks[window] = Some((value >> 1) as i64 | ((self.chr_latch as i64) << 4));
        }
        self.update();
    }
}

impl Mmc3Like for A9461 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let mut bank = bank;
        if self.extended {
            if let Some(b) = self.prg_banks[window] {
                bank = b;
            }
        }
        self.m
            .b
            .map_prg(window, size_kib, (bank & 0x0F) | ((self.outer as i64) << 4));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            let w = window + i;
            let plain = bank * size_kib as i64 + i as i64;
            let one = if self.extended {
                self.chr_banks[w].unwrap_or(plain)
            } else {
                plain
            };
            self.m
                .b
                .map_chr(w, 1, (one & 0x7F) | ((self.outer as i64) << 7));
        }
    }
}

impl Cartridge for A9461 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            if address & 0x5003 == 0x5002 {
                self.outer = (self.outer & 2) | (value & 1);
            } else if address & 0x5003 == 0x5003 {
                self.outer = (self.outer & 1) | ((value >> 4) & 2);
            }
            self.update();
            return;
        }
        let register = address & 0xE003;
        if register == 0x8002 {
            self.extended = value & 0x20 != 0;
            base_cpu_write(self, 0x8000, value, t);
            return;
        }
        if register & 0xE001 == 0x8001 && self.extended {
            self.extended_write(value);
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- ING003C -------------------------------------------------------------------

/// Mappers 126 (TEC9719), 422 (ING003C) and 534 (ING-022): MMC3 multicarts
/// with four registers (NES_2.0_Mapper_534.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ing003c {
    pub m: Mmc3,
    pub outer: [u8; 4],
    pub ram_control: u8,
    pub kind: u16,
}

impl Ing003c {
    pub fn new(image: RomImage, t: i64) -> Ing003c {
        let kind = image.board.mapper;
        let mut board = Ing003c {
            m: mmc3(image, t),
            outer: [0; 4],
            ram_control: 0x80,
            kind,
        };
        board.update();
        board
    }

    fn prg_high(&self) -> i64 {
        let o = self.outer[0] as i64;
        (((o >> 1) & 3) << 5) | (((o >> 4) & 1) << 7) | (((!o >> 5) & 1) << 8)
    }

    /// The MMC3's own 5-bit bank for a CPU window.
    fn mmc3_bank(&self, window: usize) -> i64 {
        let r = self.m.registers;
        if window == 1 {
            return (r[7] & 0x1F) as i64;
        }
        if window == 3 {
            return 0x1F;
        }
        let swapped = self.m.bank_select & 0x40 != 0;
        if (window == 0) == swapped {
            0x1E
        } else {
            (r[6] & 0x1F) as i64
        }
    }

    fn outer_write(&mut self, register: usize, value: u8) {
        let locked = self.outer[3] & 0x80 != 0;
        if register == 2 {
            let old = self.outer[2];
            let keep = (((old >> 4) & 1) << 1) | (((old >> 5) & 1) << 2) | (((old >> 6) & 1) << 3);
            let mut value = (value & !keep) | (old & keep);
            if old & 0x80 != 0 {
                value = (value & !0x70) | (old & 0x70);
            }
            self.outer[2] = value;
        } else if !locked {
            self.outer[register] = value;
        }
        self.update();
    }
}

impl Mmc3Like for Ing003c {
    mmc3_field!();
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, _bank: i64) {
        let o = self.outer[0] as i64;
        let mode = self.outer[3] & 0x0F;
        let r6 = self.m.registers[6] as i64;
        let w = window as i64;
        let mut inner = if mode < 8 {
            match mode & 3 {
                0 => self.mmc3_bank(window),
                3 => (r6 & 0x1C) | w,
                _ => (r6 & 0x1E) | (w & 1),
            }
        } else if mode == 9 || mode == 0x0D {
            if window < 2 {
                ((r6 & 0x0F) << 1) | (w & 1)
            } else {
                0x1E | (w & 1)
            }
        } else if mode == 0x0B || mode == 0x0F {
            ((r6 & 7) << 2) | w
        } else {
            self.mmc3_bank(window)
        };
        if o & 0x40 != 0 {
            inner = (inner & 0x0F) | ((o & 1) << 4);
        }
        let high = self.prg_high();
        self.m.b.map_prg(window, size_kib, inner | high);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let o = self.outer[0] as i64;
        let mode = self.outer[3];
        let mut bb = (o >> 4) & 3;
        if self.kind == 126 {
            bb = (bb >> 1) | ((bb & 1) << 1);
        }
        let high = (bb << 8) | (((o >> 2) & 1) << 10);
        for i in 0..size_kib {
            let w = window + i;
            let eight_kib = mode & 0x10 != 0;
            let mut one = if eight_kib {
                (self.outer[2] & 0x0F) as i64 * 8 + w as i64
            } else {
                bank * size_kib as i64 + i as i64
            };
            if o & 0x80 != 0 {
                one = (one & 0x7F) | (((o >> 3) & 1) << 7);
            }
            self.m.b.map_chr(w, 1, (one & 0xFF) | high);
        }
    }
}

impl Cartridge for Ing003c {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = [0; 4];
        self.update();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let mut address = address;
        let mut value = value;
        if (0x6000..0x8000).contains(&address) {
            if self.ram_control & 0x80 != 0 {
                self.outer_write((address & 3) as usize, value);
            }
            base_cpu_write(self, address, value, t);
            return;
        }
        if address >= 0x8000 {
            let mode = self.outer[3] & 0x0F;
            if mode >= 8 {
                address |= 1;
            }
            if mode == 0x0D || mode == 0x0F {
                address = 0x8001;
            }
        }
        let register = address & 0xE001;
        if self.kind == 534 && register == 0xC000 {
            value ^= 0xFF;
        }
        if register == 0xA000 && self.outer[1] & 2 != 0 {
            const FROM_MODES: [Mirroring; 4] = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];
            self.m.b.set_mirroring(FROM_MODES[(value & 3) as usize]);
            return;
        }
        base_cpu_write(self, address, value, t);
        if self.outer[3] & 0x20 != 0 && address >= 0x8000 {
            let mode = if self.m.registers[6] & 0x10 != 0 {
                SCREEN_B
            } else {
                SCREEN_A
            };
            self.m.b.set_mirroring(mode);
        }
    }
}

// -- Henggedianzi412 -----------------------------------------------------------

/// NES 2.0 mapper 412 (Intellivision 10-in-1, Henggedianzi multicarts): MMC3
/// plus NROM (NES_2.0_Mapper_412.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Henggedianzi412 {
    pub m: Mmc3,
    pub outer: [u8; 3],
    pub ram_control: u8,
}

impl Henggedianzi412 {
    pub fn new(image: RomImage, t: i64) -> Henggedianzi412 {
        let mut board = Henggedianzi412 {
            m: mmc3(image, t),
            outer: [0; 3],
            ram_control: 0x80,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Henggedianzi412 {
    mmc3_field!();
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, prg) = (self.outer[1] as i64, self.outer[2] as i64);
        let w = window as i64;
        let mut bank = bank;
        if prg & 2 != 0 {
            let mut bank16 = prg >> 3;
            if prg & 4 != 0 {
                bank16 = (bank16 & !1) | (w >> 1);
            }
            bank = (bank16 << 1) | (w & 1);
        } else {
            if mode & 2 != 0 {
                bank = (bank & 0x1F) | ((mode & 4) << 3);
            }
            if mode & 0x10 != 0 {
                bank = (bank & 0x2F) | ((mode >> 2) & 0x10);
            }
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let (mode, chr_reg, prg) = (
            self.outer[1] as i64,
            self.outer[0] as i64,
            self.outer[2] as i64,
        );
        for i in 0..size_kib {
            let w = (window + i) as i64;
            let one = if prg & 2 != 0 {
                (chr_reg >> 2) * 8 + w
            } else {
                let mut one = ((bank * size_kib as i64 + i as i64) & 0xFF) | ((mode & 8) << 5);
                if mode & 0x20 != 0 {
                    one = (one & !0x80) | ((mode >> 7) << 7);
                }
                one
            };
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Henggedianzi412 {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = [0; 3];
        self.update();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            let register = (address & 3) as usize;
            if self.ram_control & 0x80 != 0 && register < 3 {
                if register != 1 || self.outer[1] & 1 == 0 {
                    self.outer[register] = value;
                }
                self.update();
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- Multicart334 --------------------------------------------------------------

/// Mapper 334: the 5/20-in-1 1993: the MMC3 does mirroring and CHR, a
/// register does PRG (NES_2.0_Mapper_334.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Multicart334 {
    pub m: Mmc3,
    pub bank32: i64,
    pub registers_open: bool,
}

impl Multicart334 {
    pub fn new(image: RomImage, t: i64) -> Multicart334 {
        let mut board = Multicart334 {
            m: mmc3(image, t),
            bank32: 0,
            registers_open: false,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Multicart334 {
    mmc3_field!();
    fn ram_protect(&mut self, value: u8) {
        self.registers_open = value & 0xC0 == 0x80;
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, _bank: i64) {
        self.m
            .b
            .map_prg(window, size_kib, self.bank32 * 4 + window as i64);
    }
}

impl Cartridge for Multicart334 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if self.registers_open && address & 0xE003 == 0x6002 {
            return Some(0xFE << 8);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE003 == 0x6000 {
            if self.registers_open {
                self.bank32 = ((value >> 1) & 3) as i64;
                self.update();
            }
        } else {
            base_cpu_write(self, address, value, t);
        }
    }
}

// -- Cocoma516 -----------------------------------------------------------------

/// Mapper 516: EduBank's Cocoma Pack: an MMC3 clone in a 128 KiB window with
/// a PAL's outer bank (NES_2.0_Mapper_516.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Cocoma516 {
    pub m: Mmc3,
    pub outer: u8,
}

impl Cocoma516 {
    pub fn new(image: RomImage, t: i64) -> Cocoma516 {
        let mut board = Cocoma516 {
            m: mmc3(image, t),
            outer: 0,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Cocoma516 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.m.b.map_prg(
            window,
            size_kib,
            (bank & 0x0F) | (((self.outer & 3) as i64) << 4),
        );
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            let one = ((bank * size_kib as i64 + i as i64) & 0x7F)
                | ((((self.outer >> 2) & 3) as i64) << 7);
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Cocoma516 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0x8010 == 0x8010 {
            self.outer = (address & 0x0F) as u8;
            self.update();
        } else {
            base_cpu_write(self, address, value, t);
        }
    }
}

// -- Zhongguo512 ---------------------------------------------------------------

/// Mapper 512: Sachen's Zhongguo Daheng: an MMC3 clone, CHR-ROM and RAM, and
/// four more pages (NES_2.0_Mapper_512.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Zhongguo512 {
    pub m: Mmc3,
    pub mode: u8,
    pub chr_ram: Vec<u8>,
}

impl Zhongguo512 {
    pub fn new(image: RomImage, t: i64) -> Zhongguo512 {
        let board_info = image.board.clone();
        let mut board = Zhongguo512 {
            m: mmc3(image, t),
            mode: 0,
            chr_ram: Vec::new(),
        };
        board.update();
        // The reference's `rom` (the image's CHR) is never read apart from it.
        board.chr_ram = vec![0; (board_info.chr_ram + board_info.chr_nvram).max(0x2000)];
        board.m.b.vram = vec![0; 0x1000];
        board.m.b.prg_ram = vec![0; board.m.b.prg_ram.len().max(0x2000)];
        board.m.b.prg_ram_mask = 0x1FFF;
        board.m.b.prg_ram_enabled = true;
        board
    }
}

impl Mmc3Like for Zhongguo512 {
    mmc3_field!();
    fn ram_protect(&mut self, _value: u8) {}
}

impl Cartridge for Zhongguo512 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xC100 == 0x4100 {
            self.mode = value & 3;
            self.m.b.four_screen = self.mode == 1;
        } else if address & 0xE001 == 0xA000 {
            self.m
                .b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else {
            base_cpu_write(self, address, value, t);
        }
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && self.mode >= 2 {
            let offset =
                self.m.b.chr_offsets[(address >> 10) as usize] + (address as usize & 0x3FF);
            return Some(self.chr_ram[offset % self.chr_ram.len()]);
        }
        self.m.b.base_ppu_read(address, t)
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            if self.mode >= 2 {
                let offset =
                    self.m.b.chr_offsets[(address >> 10) as usize] + (address as usize & 0x3FF);
                let len = self.chr_ram.len();
                self.chr_ram[offset % len] = value;
            }
        } else {
            self.m.b.base_ppu_write(address, value, t);
        }
    }
}

// -- BATMAP399 -----------------------------------------------------------------

/// Mapper 399: BATMAP-000 (Star Versus): an MMC3's IRQ and mirroring with
/// simpler banking (NES_2.0_Mapper_399.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Batmap399 {
    pub m: Mmc3,
    pub chr4: [u8; 2],
    pub prg8: [u8; 2],
}

impl Batmap399 {
    pub fn new(image: RomImage, t: i64) -> Batmap399 {
        let mut board = Batmap399 {
            m: mmc3(image, t),
            chr4: [0, 0],
            prg8: [0, 0],
        };
        board.update();
        board
    }
}

impl Mmc3Like for Batmap399 {
    mmc3_field!();
    fn update(&mut self) {
        let last = (self.m.b.prg.len() / 0x2000) as i64 - 1;
        self.map_prg(0, 8, 0);
        self.map_prg(1, 8, self.prg8[0] as i64);
        self.map_prg(2, 8, self.prg8[1] as i64);
        self.map_prg(3, 8, last);
        self.map_chr(0, 4, self.chr4[0] as i64);
        self.map_chr(4, 4, self.chr4[1] as i64);
    }
}

impl Cartridge for Batmap399 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE000 != 0x8000 {
            base_cpu_write(self, address, value, t);
            return;
        }
        if address & 1 != 0 {
            self.prg8[(value >> 7) as usize] = value & 0x7F;
        } else {
            self.chr4[(value >> 7) as usize] = value & 7;
        }
        self.update();
    }
}

// -- the flash boards: Haradius406, Haratyler451 ---------------------------------

/// `FlashMMC3._flash_offset`.
fn flash_offset(m: &Mmc3, address: u16) -> usize {
    m.b.prg_offsets[((address >> 13) & 3) as usize] + (address as usize & 0x1FFF)
}

/// `FlashMMC3.cpu_read`.
fn flash_cpu_read(m: &Mmc3, flash: &Flash, address: u16, t: i64) -> Option<u16> {
    if flash.id_mode && address >= 0x8000 {
        return Some(flash.read(&m.b.prg, flash_offset(m, address)) as u16);
    }
    m.b.base_cpu_read(address, t)
}

/// Mapper 406: Haradius Zero: a homebrew TLROM saving high scores to flash
/// (NES_2.0_Mapper_406.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Haradius406 {
    pub m: Mmc3,
    pub rewired: u8,
    pub flash: Flash,
}

impl Haradius406 {
    pub fn new(image: RomImage, t: i64) -> Haradius406 {
        let rewired = image.board.submapper;
        let mut board = Haradius406 {
            m: mmc3(image, t),
            rewired,
            flash: Flash::flash_29f040(if rewired != 0 { 0x01 } else { 0xC2 }, 0xA4),
        };
        board.update();
        board
    }
}

impl Mmc3Like for Haradius406 {
    mmc3_field!();
}

impl Cartridge for Haradius406 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        flash_cpu_read(&self.m, &self.flash, address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let mut address = address;
        if address >= 0x8000 {
            let offset = flash_offset(&self.m, address);
            self.flash.write(&mut self.m.b.prg, offset, value);
            if self.rewired != 0 {
                if matches!(address & 0xE000, 0x8000 | 0xE000) {
                    address ^= 0x6000;
                }
            } else {
                address = (address & !1) | ((address >> 1) & 1);
            }
        }
        base_cpu_write(self, address, value, t);
    }
    fn battery_ram(&self) -> Option<Vec<u8>> {
        Some(self.m.b.prg.clone())
    }
    fn restore(&mut self, data: &[u8]) {
        self.m.b.prg.copy_from_slice(data);
    }
}

/// Mapper 451: Haratyler HG/MP: an MMC3's IRQ behind address-decoded
/// registers, and flash (NES_2.0_Mapper_451.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Haratyler451 {
    pub m: Mmc3,
    pub bank: u8,
    pub flash: Flash,
}

impl Haratyler451 {
    pub fn new(image: RomImage, t: i64) -> Haratyler451 {
        let mut board = Haratyler451 {
            m: mmc3(image, t),
            bank: 0,
            flash: Flash::flash_29f040(0x37, 0x86),
        };
        board.update();
        board
    }
}

impl Mmc3Like for Haratyler451 {
    mmc3_field!();
    fn update(&mut self) {
        let x = ((self.bank & 1) | ((self.bank & 2) << 2)) as i64;
        for (window, bank) in [0, 0x10 + x, 0x20 + x, 0x30].into_iter().enumerate() {
            self.map_prg(window, 8, bank);
        }
        self.map_chr(0, 8, (self.bank & 1) as i64);
    }
}

impl Cartridge for Haratyler451 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        flash_cpu_read(&self.m, &self.flash, address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            base_cpu_write(self, address, value, t);
            return;
        }
        let offset = flash_offset(&self.m, address);
        self.flash.write(&mut self.m.b.prg, offset, value);
        let region = address & 0xE000;
        if region == 0xA000 {
            self.m.b.set_mirroring(if address & 1 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        } else if region == 0xC000 {
            let v = address & 0xFF;
            base_cpu_write(self, 0xC000, (v.wrapping_sub(1) & 0xFF) as u8, t);
            base_cpu_write(self, 0xC001, 0, t);
            base_cpu_write(self, if v == 0xFF { 0xE000 } else { 0xE001 }, 0, t);
        } else if region == 0xE000 {
            self.bank = (address & 3) as u8;
            self.update();
        }
    }
    fn battery_ram(&self) -> Option<Vec<u8>> {
        Some(self.m.b.prg.clone())
    }
    fn restore(&mut self, data: &[u8]) {
        self.m.b.prg.copy_from_slice(data);
    }
}

// -- Rinco594 ------------------------------------------------------------------

/// Mapper 594's $B000 D7-D6: (the bank at $6000's mask and base, the MMC3's
/// PRG base in 8 KiB banks, its CHR base in 1 KiB banks, per PPU A12).
const RINCO_MODES: [(i64, i64, i64, [i64; 2]); 3] = [
    (0xFF, 0x000, 0x000, [0x000, 0x100]),
    (0x3F, 0x0C0, 0x0C0, [0x200, 0x200]),
    (0x7F, 0x100, 0x100, [0x300, 0x300]),
];

/// Mapper 594: Rinco's Talkman 3-in-1, for the RS-7600 famiclone's speech
/// hardware (NES_2.0_Mapper_594.wiki; the speech is not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rinco594 {
    pub m: Mmc3,
    pub mode: usize,
    pub extra: u8,
}

impl Rinco594 {
    pub fn new(image: RomImage, t: i64) -> Rinco594 {
        let mut board = Rinco594 {
            m: mmc3(image, t),
            mode: 0,
            extra: 0,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Rinco594 {
    mmc3_field!();
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.m
            .b
            .map_prg(window, size_kib, (bank & 0x3F) | RINCO_MODES[self.mode].2);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let bases = RINCO_MODES[self.mode].3;
        for i in 0..size_kib {
            let one = bases[(window + i) >> 2] | ((bank * size_kib as i64 + i as i64) & 0xFF);
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Rinco594 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xF001 == 0x5001 {
            return Some(0xBF << 8);
        }
        if (0x6000..0x8000).contains(&address) {
            let (mask, base, _, _) = RINCO_MODES[self.mode];
            let bank = base | (self.extra as i64 & mask);
            let prg = &self.m.b.prg;
            let offset = (bank * 0x2000 + (address as i64 & 0x1FFF)).rem_euclid(prg.len() as i64);
            return Some(prg[offset as usize] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF001 == 0x9000 {
            self.extra = value;
        } else if address & 0xF001 == 0x9001 {
        } else if address & 0xF000 == 0xB000 {
            self.mode = ((value >> 6) as usize).min(2);
            self.update();
        } else {
            base_cpu_write(self, address, value, t);
        }
    }
}

// -- ContraFighter238 ----------------------------------------------------------

/// Mapper 238 (Contra Fighter): an MMC3 and a protection register (C68).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ContraFighter238 {
    pub m: Mmc3,
    pub protection: usize,
}

const CONTRA_FIGHTER_PROTECTION: [u8; 4] = [0, 2, 2, 3];

impl ContraFighter238 {
    pub fn new(image: RomImage, t: i64) -> ContraFighter238 {
        let mut board = ContraFighter238 {
            m: mmc3(image, t),
            protection: 0,
        };
        board.update();
        board
    }
}

impl Mmc3Like for ContraFighter238 {
    mmc3_field!();
}

impl Cartridge for ContraFighter238 {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x4020..0x8000).contains(&address) {
            return Some(CONTRA_FIGHTER_PROTECTION[self.protection] as u16 | (0xFC << 8));
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x4020..0x8000).contains(&address) {
            self.protection = (value & 3) as usize;
        } else {
            base_cpu_write(self, address, value, t);
        }
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
        12 => Box::new(Sl5020b::new(image, t)),
        45 => Box::new(Ga23c::new(image, t)),
        52 => Box::new(Outer52::new(image, t)),
        114 | 123 | 182 | 187 | 215 | 258 => Box::new(SuperGame::new(image, t)),
        121 => Box::new(A9711::new(image, t)),
        126 | 422 | 534 => Box::new(Ing003c::new(image, t)),
        134 => Box::new(T4a54a::new(image, t)),
        165 => Box::new(Latched165::new(image, t)),
        189 | 196 => Box::new(Prg32Override::new(image, t)),
        197 => Box::new(Rewired197::new(image, t)),
        205 => Box::new(Jc016::new(image, t)),
        208 => Box::new(Gouder208::new(image, t)),
        219 => Box::new(A9461::new(image, t)),
        238 => Box::new(ContraFighter238::new(image, t)),
        245 => Box::new(F003::new(image, t)),
        250 => Box::new(Nitra250::new(image, t)),
        254 => Box::new(XorRam254::new(image, t)),
        260 => Box::new(Hpxx260::new(image, t)),
        262 => Box::new(SachenShero::new(image, t)),
        263 => Box::new(Rex263::new(image, t)),
        292 => Box::new(Bmw8544::new(image, t)),
        313 => Box::new(ResetTxRom::new(image, t)),
        334 => Box::new(Multicart334::new(image, t)),
        399 => Box::new(Batmap399::new(image, t)),
        406 => Box::new(Haradius406::new(image, t)),
        412 => Box::new(Henggedianzi412::new(image, t)),
        451 => Box::new(Haratyler451::new(image, t)),
        512 => Box::new(Zhongguo512::new(image, t)),
        513 => Box::new(Board513::new(image, t)),
        516 => Box::new(Cocoma516::new(image, t)),
        594 => Box::new(Rinco594::new(image, t)),
        _ => return Err(image),
    })
}
