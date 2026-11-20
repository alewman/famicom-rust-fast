// SPDX-License-Identifier: MIT
//! Sachen's boards: the 8259 family (137, 138, 139, 141), the "74LS374N" ASIC
//! (150, 243) and the discrete latches (133, 143, 145, 148, 149), and the 3013
//! and 3014 boards (553, 533) (`sachen.py`).
//!
//! Each from its NESdev wiki page (Sachen_8259.wiki, INES_Mapper_137/150/243/
//! 133/143/145/148/149.wiki, NES_2.0_Mapper_533/553.wiki). No judge ROM
//! exists; the claims rest on sweeps against Mesen2, a detector.
//!
//! The reference's `SachenRegisterFile` subclasses override `_update` (which
//! its `__init__` calls); here one struct carries a [`Variant`].
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 133: `SachenLatch`
//! - 137: `Sachen8259D`
//! - 138: `Sachen8259`
//! - 139: `Sachen8259`
//! - 141: `Sachen8259`
//! - 143: `Sachen143`
//! - 145: `SachenLatch`
//! - 148: `SachenLatch`
//! - 149: `SachenLatch`
//! - 150: `SachenLS374N`
//! - 243: `SachenLS374N`
//! - 533: `Sachen3014`
//! - 553: `Sachen3013`

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::discrete::nrom;
use super::ines::RomImage;
use crate::banked_field;

/// A mirroring the register file can select: Banked's, or `L_SHAPED` (one
/// quadrant on the other CIRAM page: (0, 1, 1, 1) or (0, 0, 0, 1)).
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
enum Arrangement {
    Plain(Mirroring),
    LShaped,
}
use Arrangement::{LShaped as L_SHAPED, Plain};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Variant {
    /// Mappers 138 (8259B), 141 (8259A) and 139 (8259C): a 32 KiB PRG bank,
    /// four 2 KiB CHR banks. 141 with CHR-RAM (Q-Boy) has 8 KiB unbanked.
    S8259 { shift: u32, banked_chr: bool },
    /// Mapper 137: Sachen 8259D (The Great Wall): four 1 KiB CHR banks,
    /// $1000 the last 4 KiB (INES_Mapper_137.wiki).
    S8259D,
    /// Mappers 150 (SA-015/SA-630) and 243 (SA-020A): the ASIC marked
    /// "74LS374N" (INES_Mapper_150.wiki, _243.wiki); all eight registers are
    /// readable at $4101.
    Ls374n { honey_peach: bool },
}

/// An index register at $4100 and eight 3-bit data registers at $4101 (mask $C101).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SachenRegisterFile {
    pub b: Banked,
    pub variant: Variant,
    pub index: u8,
    pub regs: [u8; 8],
    pub l_pages: [u8; 4],
    /// The mirroring is `L_SHAPED` (the reference's `mirroring == "L"`).
    pub l_shaped: bool,
}

impl SachenRegisterFile {
    fn new(image: RomImage, cpu_cycle_ticks: i64, variant: Variant) -> SachenRegisterFile {
        let mut cart = SachenRegisterFile {
            b: Banked::new(image, cpu_cycle_ticks),
            variant,
            index: 0,
            regs: [0; 8],
            l_pages: [0, 1, 1, 1],
            l_shaped: false,
        };
        cart.b.prg_ram_enabled = false;
        cart.update();
        cart
    }

    pub fn sachen_8259(image: RomImage, cpu_cycle_ticks: i64) -> SachenRegisterFile {
        let shift = match image.board.mapper {
            138 => 0,
            141 => 1,
            _ => 2, // 139
        };
        let banked_chr = !image.chr.is_empty();
        SachenRegisterFile::new(image, cpu_cycle_ticks, Variant::S8259 { shift, banked_chr })
    }

    pub fn sachen_8259d(image: RomImage, cpu_cycle_ticks: i64) -> SachenRegisterFile {
        SachenRegisterFile::new(image, cpu_cycle_ticks, Variant::S8259D)
    }

    pub fn ls374n(image: RomImage, cpu_cycle_ticks: i64) -> SachenRegisterFile {
        let honey_peach = image.board.mapper == 243;
        let mut cart =
            SachenRegisterFile::new(image, cpu_cycle_ticks, Variant::Ls374n { honey_peach });
        cart.l_pages = [0, 0, 0, 1];
        cart
    }

    fn set_mirroring(&mut self, mode: Arrangement) {
        match mode {
            Plain(m) => {
                self.l_shaped = false;
                self.b.set_mirroring(m);
            }
            L_SHAPED => self.l_shaped = true,
        }
    }

    fn update(&mut self) {
        let r = self.regs;
        match self.variant.clone() {
            Variant::S8259 { shift, banked_chr } => {
                let simple = r[7] & 1 != 0;
                self.b.map_prg(0, 32, r[5] as i64);
                if simple {
                    self.set_mirroring(Plain(VERTICAL));
                } else {
                    self.set_mirroring(
                        [
                            Plain(VERTICAL),
                            Plain(HORIZONTAL),
                            L_SHAPED,
                            Plain(SCREEN_A),
                        ][((r[7] >> 1) & 3) as usize],
                    );
                }
                if !banked_chr {
                    self.b.map_chr(0, 8, 0);
                    return;
                }
                for window in 0..4 {
                    let bank = ((r[4] as i64) << 3) | r[if simple { 0 } else { window }] as i64;
                    for half in 0..2 {
                        let ppu_1k = window * 2 + half;
                        let passed = ppu_1k as i64 & ((1 << (shift + 1)) - 1);
                        self.b.map_chr(ppu_1k, 1, (bank << (shift + 1)) | passed);
                    }
                }
            }
            Variant::S8259D => {
                let simple = r[7] & 1 != 0;
                self.b.map_prg(0, 32, r[5] as i64);
                if simple {
                    self.set_mirroring(Plain(HORIZONTAL));
                } else {
                    self.set_mirroring(
                        [
                            Plain(HORIZONTAL),
                            Plain(VERTICAL),
                            L_SHAPED,
                            Plain(SCREEN_A),
                        ][((r[7] >> 1) & 3) as usize],
                    );
                }
                let top = [
                    0,
                    (r[4] & 1) << 4,
                    (r[4] & 2) << 3,
                    ((r[4] & 4) << 2) | ((r[6] & 1) << 3),
                ];
                for (i, top) in top.iter().enumerate() {
                    self.b
                        .map_chr(i, 1, (top | r[if simple { 0 } else { i }]) as i64);
                }
                let last = (self.b.chr.len() / 0x1000) as i64 - 1;
                self.b.map_chr(4, 4, last);
            }
            Variant::Ls374n { honey_peach } => {
                self.b.map_prg(0, 32, (r[5] & 3) as i64);
                let bank = if honey_peach {
                    (r[2] & 1) | ((r[4] & 1) << 1) | ((r[6] & 3) << 2)
                } else {
                    (r[6] & 3) | ((r[4] & 1) << 2)
                };
                self.b.map_chr(0, 8, bank as i64);
                self.set_mirroring(
                    [
                        L_SHAPED,
                        Plain(HORIZONTAL),
                        Plain(VERTICAL),
                        Plain(SCREEN_B),
                    ][((r[7] >> 1) & 3) as usize],
                );
            }
        }
    }
}

impl Cartridge for SachenRegisterFile {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if matches!(self.variant, Variant::Ls374n { .. }) && address & 0xC101 == 0x4101 {
            return Some(self.regs[self.index as usize] as u16 | (0xF8 << 8)); // D2-D0 driven
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let register = address & 0xC101;
        if register == 0x4100 {
            self.index = value & 7;
        } else if register == 0x4101 {
            self.regs[self.index as usize] = value & 7;
            self.update();
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        if self.l_shaped {
            return Some(self.l_pages[((address >> 10) & 3) as usize]);
        }
        self.b.base_ciram_a10(address)
    }
}

/// Sachen's latches: 133 (SA-72008), 145 (SA-72007), 148 (SA-008-A), 149
/// (SA-0036) (INES_Mapper_133/145/148/149.wiki). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SachenLatch {
    pub b: Banked,
    pub kind: u16,
}

impl SachenLatch {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> SachenLatch {
        let kind = image.board.mapper;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        SachenLatch { b, kind }
    }
}

impl Cartridge for SachenLatch {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let kind = self.kind;
        if matches!(kind, 133 | 145) && address & 0xE100 == 0x4100 {
            if kind == 133 {
                self.b.map_prg(0, 32, ((value >> 2) & 1) as i64);
                self.b.map_chr(0, 8, (value & 3) as i64);
            } else {
                self.b.map_chr(0, 8, (value >> 7) as i64);
            }
        } else if matches!(kind, 148 | 149) && address >= 0x8000 {
            let value = self.b.bus_conflict(address, value);
            if kind == 148 {
                self.b.map_prg(0, 32, ((value >> 3) & 1) as i64);
                self.b.map_chr(0, 8, (value & 7) as i64);
            } else {
                self.b.map_chr(0, 8, (value >> 7) as i64);
            }
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 143: NROM with a protection read (Dancing Blocks, Magical
/// Mathematics; INES_Mapper_143.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sachen143 {
    pub b: Banked,
}

impl Sachen143 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sachen143 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        for window in 0..4 {
            b.map_prg(window, 8, window as i64);
        }
        b.map_chr(0, 8, 0);
        Sachen143 { b }
    }
}

impl Cartridge for Sachen143 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xE100 == 0x4100 {
            return Some((!address & 0x3F) | (0xC0 << 8));
        }
        self.b.base_cpu_read(address, t)
    }
}

/// Mapper 533: the Sachen 3014: CNROM whose latch reads back
/// (NES_2.0_Mapper_533.wiki; here the latch wins at once).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sachen3014 {
    pub b: Banked,
    pub latch: u8,
}

impl Sachen3014 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sachen3014 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        Sachen3014 { b, latch: 0 }
    }
}

impl Cartridge for Sachen3014 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let value = self.b.base_cpu_read(address, t);
        if address & 0xF000 == 0xE000 {
            if let Some(value) = value {
                return Some((value & 0xF0) | self.latch as u16);
            }
        }
        value
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x8000 {
            self.latch = self.b.bus_conflict(address, value) >> 4;
            self.b.map_chr(0, 8, (self.latch & 1) as i64);
        }
    }
}

/// Mapper 553: Sachen's 3013 board (Dong Dongnao 1): NROM-128 whose
/// $8000-$BFFF reads $3A (NES_2.0_Mapper_553.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sachen3013 {
    pub b: Banked,
}

impl Cartridge for Sachen3013 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x8000..0xC000).contains(&address) {
            return Some(0x3A);
        }
        self.b.base_cpu_read(address, t)
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
        133 | 145 | 148 | 149 => Box::new(SachenLatch::new(image, t)),
        137 => Box::new(SachenRegisterFile::sachen_8259d(image, t)),
        138 | 139 | 141 => Box::new(SachenRegisterFile::sachen_8259(image, t)),
        143 => Box::new(Sachen143::new(image, t)),
        150 | 243 => Box::new(SachenRegisterFile::ls374n(image, t)),
        533 => Box::new(Sachen3014::new(image, t)),
        553 => Box::new(Sachen3013 { b: nrom(image, t) }),
        _ => return Err(image),
    })
}
