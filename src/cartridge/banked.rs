// SPDX-License-Identifier: MIT
//! What most mappers share: PRG and CHR in windows, PRG-RAM, the CIRAM wiring
//! (`banked.py`).
//!
//! The reference's mappers subclass `Banked`; here a mapper holds a
//! [`Banked`] and the connector's defaults forward to it (see the notes in
//! `connector.rs`).

use super::connector::Cartridge;
use super::ines::RomImage;

/// CIRAM A10 per nametable arrangement: horizontal mirroring takes PPU A11,
/// vertical takes A10, single-screen a constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    ScreenA,
    ScreenB,
}
pub use Mirroring::{
    Horizontal as HORIZONTAL, ScreenA as SCREEN_A, ScreenB as SCREEN_B, Vertical as VERTICAL,
};

/// PRG in four 8 KiB windows at $8000, CHR in eight 1 KiB windows, PRG-RAM at $6000.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Banked {
    #[serde(skip, default = "crate::state::live_image")]
    pub image: RomImage,
    pub cpu_cycle_ticks: i64,
    #[serde(with = "crate::state::rom_bytes::prg")]
    pub prg: Vec<u8>,
    pub chr_writable: bool,
    #[serde(with = "crate::state::rom_bytes::chr")]
    pub chr: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub prg_ram_enabled: bool,
    pub prg_ram_writable: bool,
    pub prg_ram_offset: usize,
    pub prg_ram_mask: usize,
    pub battery: bool,
    pub prg_offsets: [usize; 4],
    pub chr_offsets: [usize; 8],
    pub four_screen: bool,
    pub vram: Vec<u8>,
    pub mirroring: Mirroring,
}

impl Banked {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Banked {
        let board = image.board.clone();
        let mut prg = image.prg.clone();
        if !prg.is_empty() && prg.len() < 0x2000 {
            prg = prg.repeat(0x2000 / prg.len());
        }
        let chr_ram = board.chr_ram + board.chr_nvram;
        let chr_writable = image.chr.is_empty();
        let chr = if image.chr.is_empty() {
            vec![0; if chr_ram != 0 { chr_ram } else { 0x2000 }]
        } else {
            image.chr.clone()
        };
        let ram_size = board.prg_ram + board.prg_nvram;
        let four_screen = board.mirroring == '4';
        let mut banked = Banked {
            image,
            cpu_cycle_ticks,
            prg,
            chr_writable,
            chr,
            prg_ram: vec![0; ram_size],
            prg_ram_enabled: ram_size > 0,
            prg_ram_writable: true,
            prg_ram_offset: 0,
            prg_ram_mask: ram_size.min(0x2000).wrapping_sub(1),
            battery: board.battery || board.prg_nvram > 0,
            prg_offsets: [0; 4],
            chr_offsets: [0; 8],
            four_screen,
            vram: vec![0; if four_screen { 0x1000 } else { 0 }],
            mirroring: HORIZONTAL,
        };
        banked.set_mirroring(if board.mirroring == 'V' {
            VERTICAL
        } else {
            HORIZONTAL
        });
        banked
    }

    // -- banking helpers ---------------------------------------------------

    /// Point `size_kib` KiB of CPU space from 8 KiB window `window` at `bank`.
    ///
    /// `bank` may be negative, as Python's arithmetic makes it (a last bank of
    /// an image too small to have one): the offset is Python's modulo.
    pub fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let base = bank * size_kib as i64 * 1024;
        let len = self.prg.len().max(1) as i64;
        for i in 0..size_kib / 8 {
            self.prg_offsets[window + i] = (base + i as i64 * 0x2000).rem_euclid(len) as usize;
        }
    }

    /// Point `size_kib` KiB of PPU space from 1 KiB window `window` at `bank`.
    pub fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let base = bank * size_kib as i64 * 1024;
        let len = self.chr.len().max(1) as i64;
        for i in 0..size_kib {
            self.chr_offsets[window + i] = (base + i as i64 * 0x400).rem_euclid(len) as usize;
        }
    }

    pub fn set_mirroring(&mut self, mode: Mirroring) {
        self.mirroring = mode;
    }

    /// An AND-type bus conflict: the ROM drives the byte at `address` as the CPU writes.
    pub fn bus_conflict(&self, address: u16, value: u8) -> u8 {
        value
            & self.prg[self.prg_offsets[(address as usize >> 13) & 3] + (address as usize & 0x1FFF)]
    }

    // -- the connector, as Banked does it (the trait's defaults call these) --

    pub fn base_cpu_read(&self, address: u16, _t: i64) -> Option<u16> {
        let a = address as usize;
        if address >= 0x8000 {
            return Some(self.prg[self.prg_offsets[(a >> 13) & 3] + (a & 0x1FFF)] as u16);
        }
        if address >= 0x6000 && self.prg_ram_enabled {
            return Some(self.prg_ram[self.prg_ram_offset + (a & self.prg_ram_mask)] as u16);
        }
        None
    }

    pub fn base_cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) && self.prg_ram_enabled && self.prg_ram_writable {
            let a = address as usize;
            self.prg_ram[self.prg_ram_offset + (a & self.prg_ram_mask)] = value;
        }
    }

    pub fn base_ciram_a10(&self, address: u16) -> Option<u8> {
        if self.four_screen {
            return None;
        }
        Some(match self.mirroring {
            VERTICAL => ((address >> 10) & 1) as u8,
            HORIZONTAL => ((address >> 11) & 1) as u8,
            SCREEN_A => 0,
            SCREEN_B => 1,
        })
    }

    pub fn base_ppu_read(&self, address: u16, _t: i64) -> Option<u8> {
        let a = address as usize;
        if address < 0x2000 {
            return Some(self.chr[self.chr_offsets[a >> 10] + (a & 0x3FF)]);
        }
        if self.four_screen {
            return Some(self.vram[a & 0x0FFF]);
        }
        None
    }

    pub fn base_ppu_write(&mut self, address: u16, value: u8, _t: i64) {
        let a = address as usize;
        if address < 0x2000 {
            if self.chr_writable {
                self.chr[self.chr_offsets[a >> 10] + (a & 0x3FF)] = value;
            }
        } else if self.four_screen {
            self.vram[a & 0x0FFF] = value;
        }
    }

    pub fn base_battery_ram(&self) -> Option<Vec<u8>> {
        if self.battery {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }
}

/// `Banked` itself is a cartridge: NROM is no more than its windows.
impl Cartridge for Banked {
    fn banked(&self) -> Option<&Banked> {
        Some(self)
    }
    fn banked_mut(&mut self) -> Option<&mut Banked> {
        Some(self)
    }
}
