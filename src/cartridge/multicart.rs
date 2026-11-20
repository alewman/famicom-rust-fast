// SPDX-License-Identifier: MIT
//! Discrete-logic multicarts: outer and inner banks latched from the data or
//! the address bus (`multicart.py`).
//!
//! Each from its NESdev wiki page (cited per board). No judge ROM exists for
//! any of them; the claims rest on sweeps against Mesen2, a detector. A
//! board's DIP switches or solder pads, which pick a menu, read as 0. The
//! reference's `ResetMMC1` is [`Mmc1`] with `Kind::Reset` (mmc1.rs);
//! `SB5013` follows the MMC3 pattern (mmc3.rs).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 15: `K1029`
//! - 57: `GK57`
//! - 58: `AddressLatch58`
//! - 59: `AddressLatch59`
//! - 60: `ResetMulticart60`
//! - 61: `Latch61`
//! - 62: `Super700`
//! - 63: `Powerful63`
//! - 81: `NTDEC81`
//! - 122: `JY043`
//! - 212: `AddressLatch212`
//! - 213: `AddressLatch58`
//! - 221: `N625092`
//! - 225: `AddressLatch225`
//! - 226: `Latch226`
//! - 227: `AddressLatch242`
//! - 229: `Latch229`
//! - 234: `Maxi15`
//! - 235: `GoldenGame235`
//! - 242: `AddressLatch242`
//! - 255: `AddressLatch225`
//! - 285: `A65AS`
//! - 286: `BS5`
//! - 289: `Champion289`
//! - 290: `Asder290`
//! - 299: `Latch299`
//! - 301: `AddressLatch242`
//! - 314: `NoRepeat314`
//! - 319: `HP898F`
//! - 332: `WS332`
//! - 359: `SB5013`
//! - 374: `ResetMMC1`
//! - 380: `Latch380`
//! - 389: `Caltron389`
//! - 396: `YY850437C`
//! - 400: `RetroUSB400`
//! - 409: `DPCMCart409`
//! - 471: `ImpactSoft471`
//! - 493: `Maxivision493`
//! - 519: `Latch519`
//! - 540: `SB5013`
//! - 541: `LittleCom541`

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::mmc1::Mmc1;
use super::mmc3::{base_ppu_address, Mmc3, Mmc3Like};
use crate::{banked_field, mmc3_cartridge};

/// `a` if `cond`, else `b` (the reference's `A if x else B` on mirrorings).
fn pick(cond: bool, a: Mirroring, b: Mirroring) -> Mirroring {
    if cond {
        a
    } else {
        b
    }
}

/// The same 16 KiB bank at $8000 and $C000.
fn map_both_16(b: &mut Banked, bank: i64) {
    b.map_prg(0, 16, bank);
    b.map_prg(2, 16, bank);
}

// -- address latches ---------------------------------------------------------

/// Mapper 58 (and its duplicate 213): the address of a write to $8000-$FFFF
/// is the latch (INES_Mapper_058.wiki). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddressLatch58 {
    pub b: Banked,
}

impl AddressLatch58 {
    pub fn new(image: RomImage, t: i64) -> AddressLatch58 {
        let mut cart = AddressLatch58 {
            b: Banked::new(image, t),
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let prg = a & 7;
        if a & 0x40 != 0 {
            map_both_16(&mut self.b, prg);
        } else {
            self.b.map_prg(0, 32, prg >> 1);
        }
        self.b.map_chr(0, 8, (a >> 3) & 7);
        self.b
            .set_mirroring(pick(a & 0x80 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for AddressLatch58 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 286: the BS-5 multicart: four 8 KiB PRG and four 2 KiB CHR
/// windows, all from the address (NES_2.0_Mapper_286.wiki; DIP setting 1, A4).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Bs5 {
    pub b: Banked,
}

impl Bs5 {
    pub fn new(image: RomImage, t: i64) -> Bs5 {
        let mut b = Banked::new(image, t);
        for window in 0..4 {
            b.map_prg(window, 8, 0x0F);
            b.map_chr(window * 2, 2, 0x1F);
        }
        b.set_mirroring(VERTICAL);
        Bs5 { b }
    }
}

impl Cartridge for Bs5 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let window = ((address >> 10) & 3) as usize;
        if address & 0xE000 == 0x8000 {
            self.b.map_chr(window * 2, 2, (address & 0x1F) as i64);
        } else if address & 0xE000 == 0xA000 && address & 0x10 != 0 {
            self.b.map_prg(window, 8, (address & 0x0F) as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 319: the HP-898F and KD-7/9-E boards, two registers decoded
/// by A14, A13 and A2 (NES_2.0_Mapper_319.wiki, the wiki's NES 2.0 order).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hp898f {
    pub b: Banked,
}

impl Hp898f {
    pub fn new(image: RomImage, t: i64) -> Hp898f {
        let mut cart = Hp898f {
            b: Banked::new(image, t),
        };
        cart.b.prg_ram_enabled = false;
        cart.prg(0);
        cart.b.map_chr(0, 8, 0);
        cart
    }

    fn prg(&mut self, value: u8) {
        let v = value as i64;
        let bank32 = (v >> 3) & 3; // PRG A16-A15
        if v & 0x40 != 0 {
            self.b.map_prg(0, 32, bank32);
        } else {
            let bank16 = (bank32 << 1) | ((v >> 5) & 1);
            map_both_16(&mut self.b, bank16);
        }
        self.b
            .set_mirroring(pick(v & 0x80 != 0, VERTICAL, HORIZONTAL));
    }
}

impl Cartridge for Hp898f {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address == 0x5FF0 {
            return Some(0xBF << 8); // the solder pad on D6: 0
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0x6000 != 0x6000 {
            self.b.base_cpu_write(address, value, t);
        } else if address & 4 != 0 {
            self.prg(value);
        } else {
            self.b.map_chr(0, 8, (value >> 4) as i64);
        }
    }
}

/// Mappers 242, 227 (and 301): address-latch multicarts with UNROM-like and
/// NROM modes (INES_Mapper_242.wiki, _227.wiki; the reference details the
/// boards). Power-on and reset clear the latch.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddressLatch242 {
    pub b: Banked,
    pub wide: bool,
    pub submapper: u8,
    pub rpg: bool,
    pub two_chips: bool,
    pub chr_ram: bool,
    pub protects: bool,
    pub latch: u16,
    pub pads_shown: bool,
}

impl AddressLatch242 {
    pub fn new(image: RomImage, t: i64) -> AddressLatch242 {
        let board = image.board.clone();
        let chr_ram = image.chr.is_empty();
        let b = Banked::new(image, t);
        let wide = board.mapper == 227;
        let submapper = board.submapper;
        let rpg = b.battery && !(wide && matches!(submapper, 1 | 2));
        let two_chips = b.prg.len() == 0xA0000;
        let protects = !rpg && (wide || b.prg.len() >= 0x80000);
        let mut cart = AddressLatch242 {
            b,
            wide,
            submapper,
            rpg,
            two_chips,
            chr_ram,
            protects,
            latch: 0,
            pads_shown: false,
        };
        cart.b.map_chr(0, 8, 0);
        cart.do_latch(0);
        cart
    }

    fn do_latch(&mut self, address: u16) {
        self.latch = address;
        let a = address as i64;
        let inner = (a >> 2) & 7;
        let mut outer = (a >> 5) & 3;
        if self.wide {
            outer |= ((a >> 8) & 1) << 2;
            self.pads_shown = a & 0x400 != 0 && self.submapper == 1;
        } else {
            self.pads_shown = a & 0x100 != 0;
        }
        let thirty_two = a & 1 != 0;
        let nrom = a & 0x80 != 0 || self.rpg;
        let base = outer << 3;
        let (mut low, mut high);
        if nrom {
            if thirty_two {
                low = base | (inner & 6);
                high = base | inner | 1;
            } else {
                low = base | inner;
                high = low;
            }
        } else {
            low = base | if thirty_two { inner & 6 } else { inner };
            if a & 0x200 != 0 {
                high = base | 7;
            } else {
                high = if self.wide && self.submapper == 2 {
                    base & !0x18
                } else {
                    base
                };
            }
        }
        if self.two_chips && !self.wide && a & 0x400 == 0 {
            low = 32 + (low & 7);
            high = 32 + (high & 7);
        }
        self.b.map_prg(0, 16, low);
        self.b.map_prg(2, 16, high);
        self.b.set_mirroring(pick(a & 2 != 0, HORIZONTAL, VERTICAL));
        self.b.chr_writable = self.chr_ram && !(self.protects && nrom);
    }
}

impl Cartridge for AddressLatch242 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.do_latch(0);
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let mut address = address;
        if address >= 0x8000 && self.pads_shown {
            address &= !(if self.wide { 0x0F } else { 0x1F });
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.do_latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 59 (BMC-T3H53, BMC-D1038): an address latch with a lock and a
/// jumper read (INES_Mapper_059.wiki). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddressLatch59 {
    pub b: Banked,
    pub locked: bool,
    pub jumper_read: bool,
}

impl AddressLatch59 {
    pub fn new(image: RomImage, t: i64) -> AddressLatch59 {
        let mut cart = AddressLatch59 {
            b: Banked::new(image, t),
            locked: false,
            jumper_read: false,
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let bank = (a >> 4) & 7;
        if a & 0x80 != 0 {
            map_both_16(&mut self.b, bank);
        } else {
            self.b.map_prg(0, 32, bank >> 1);
        }
        self.b.map_chr(0, 8, a & 7);
        self.b.set_mirroring(pick(a & 8 != 0, HORIZONTAL, VERTICAL));
        self.jumper_read = a & 0x100 != 0;
        self.locked = a & 0x200 != 0;
    }
}

impl Cartridge for AddressLatch59 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.locked = false;
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.jumper_read {
            return Some(0xFC << 8);
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            if !self.locked {
                self.latch(address);
            }
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 212 (BMC Super HiK 300-in-1): an address latch with a read-back
/// quirk (INES_Mapper_212.wiki, from FCEUX's source). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddressLatch212 {
    pub b: Banked,
}

impl AddressLatch212 {
    pub fn new(image: RomImage, t: i64) -> AddressLatch212 {
        let mut cart = AddressLatch212 {
            b: Banked::new(image, t),
        };
        cart.b.prg_ram_enabled = false;
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let bank = a & 7;
        if a & 0x4000 != 0 {
            self.b.map_prg(0, 32, bank >> 1);
        } else {
            map_both_16(&mut self.b, bank);
        }
        self.b.map_chr(0, 8, bank);
        self.b.set_mirroring(pick(a & 8 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for AddressLatch212 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some((if address & 0x10 != 0 { 0 } else { 0x80 }) | (0x70 << 8));
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mappers 225 and 255: an address latch with sixteen bits of RAM at $5800
/// (INES_Mapper_225.wiki). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddressLatch225 {
    pub b: Banked,
    pub nibbles: [u8; 4],
}

impl AddressLatch225 {
    pub fn new(image: RomImage, t: i64) -> AddressLatch225 {
        let mut cart = AddressLatch225 {
            b: Banked::new(image, t),
            nibbles: [0; 4],
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let high = (a >> 14) & 1;
        let prg = (high << 6) | ((a >> 6) & 0x3F);
        if a & 0x1000 != 0 {
            map_both_16(&mut self.b, prg);
        } else {
            self.b.map_prg(0, 32, prg >> 1);
        }
        self.b.map_chr(0, 8, (high << 6) | (a & 0x3F));
        self.b
            .set_mirroring(pick(a & 0x2000 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for AddressLatch225 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5800..0x6000).contains(&address) {
            return Some(self.nibbles[(address & 3) as usize] as u16 | (0xF0 << 8));
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else if (0x5800..0x6000).contains(&address) {
            self.nibbles[(address & 3) as usize] = value & 0x0F;
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 226 (76-in-1, Super 42-in-1): two latches at $8000/$8001 (mask
/// $8001; INES_Mapper_226.wiki). Power-on and reset clear both.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch226 {
    pub b: Banked,
    pub regs: [u8; 2],
}

impl Latch226 {
    pub fn new(image: RomImage, t: i64) -> Latch226 {
        let mut cart = Latch226 {
            b: Banked::new(image, t),
            regs: [0, 0],
        };
        cart.b.map_chr(0, 8, 0);
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (low, high) = (self.regs[0] as i64, self.regs[1] as i64);
        let bank = (low & 0x1F) | ((low >> 2) & 0x20) | ((high & 1) << 6);
        if low & 0x20 != 0 {
            map_both_16(&mut self.b, bank);
        } else {
            self.b.map_prg(0, 32, bank >> 1);
        }
        self.b
            .set_mirroring(pick(low & 0x40 != 0, VERTICAL, HORIZONTAL));
        self.b.chr_writable = self.b.image.chr.is_empty() && high & 2 == 0;
    }
}

impl Cartridge for Latch226 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.regs = [0, 0];
        self.update();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.regs[(address & 1) as usize] = value;
            self.update();
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 332 (BMC-WS: Super 40-in-1, Super 500-in-1;
/// NES_2.0_Mapper_332.wiki). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ws332 {
    pub b: Banked,
    pub regs: [u8; 2],
    pub latch: u8,
}

impl Ws332 {
    pub fn new(image: RomImage, t: i64) -> Ws332 {
        let mut cart = Ws332 {
            b: Banked::new(image, t),
            regs: [0, 0],
            latch: 0,
        };
        cart.b.prg_ram_enabled = false;
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (r0, r1) = (self.regs[0] as i64, self.regs[1] as i64);
        let latch = self.latch as i64;
        let high = (r0 >> 6) & 1;
        let bank = (high << 3) | (r0 & 7);
        if r0 & 8 != 0 {
            map_both_16(&mut self.b, bank);
        } else {
            self.b.map_prg(0, 32, bank >> 1);
        }
        self.b
            .set_mirroring(pick(r0 & 0x10 != 0, HORIZONTAL, VERTICAL));
        let mode = (r1 >> 4) & 3;
        let low = if mode == 3 {
            r1 & 3
        } else if mode == 2 {
            (r1 & 2) | (latch & 1)
        } else {
            latch & 3
        };
        self.b
            .map_chr(0, 8, (high << 3) | (((r1 >> 2) & 1) << 2) | low);
    }
}

impl Cartridge for Ws332 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch = value;
        } else if matches!(address & 0xE001, 0x6000 | 0x6001) {
            self.regs[(address & 1) as usize] = value;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// Mapper 60: the reset-based NROM-128 4-in-1 (INES_Mapper_060.wiki, Disch's
/// notes): each reset counts the game up. Power-on game 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ResetMulticart60 {
    pub b: Banked,
    pub game: i64,
}

impl ResetMulticart60 {
    pub fn new(image: RomImage, t: i64) -> ResetMulticart60 {
        let mut cart = ResetMulticart60 {
            b: Banked::new(image, t),
            game: 0,
        };
        cart.select();
        cart
    }

    fn select(&mut self) {
        map_both_16(&mut self.b, self.game);
        self.b.map_chr(0, 8, self.game);
    }
}

impl Cartridge for ResetMulticart60 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.game = (self.game + 1) & 3;
        self.select();
    }
}

/// Mapper 57 (GK 47-in-1, 6-in-1 SuperGK): two registers at $8000 and $8800
/// (mask $8800; INES_Mapper_057.wiki). Power-on registers 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Gk57 {
    pub b: Banked,
    pub regs: [u8; 2],
}

impl Gk57 {
    pub fn new(image: RomImage, t: i64) -> Gk57 {
        let mut cart = Gk57 {
            b: Banked::new(image, t),
            regs: [0, 0],
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (low, high) = (self.regs[0] as i64, self.regs[1] as i64);
        let inner = if low & 0x80 != 0 { high & 3 } else { low & 3 };
        self.b.map_chr(
            0,
            8,
            (((low >> 6) & 1) << 3) | (((high >> 2) & 1) << 2) | inner,
        );
        let bank = high >> 5;
        if high & 0x10 != 0 {
            self.b.map_prg(0, 32, bank >> 1);
        } else {
            map_both_16(&mut self.b, bank);
        }
        self.b
            .set_mirroring(pick(high & 8 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Gk57 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.regs[((address >> 11) & 1) as usize] = value;
            self.update();
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 519: an address-and-data latch with a lock and a DIP switch
/// read (NES_2.0_Mapper_519.wiki). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch519 {
    pub b: Banked,
    pub address: u16,
    pub value: u8,
    pub locked: bool,
}

impl Latch519 {
    pub fn new(image: RomImage, t: i64) -> Latch519 {
        let mut cart = Latch519 {
            b: Banked::new(image, t),
            address: 0,
            value: 0,
            locked: false,
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (a, d) = (self.address as i64, self.value as i64);
        let bank = a & 0x3F;
        if a & 0x80 != 0 {
            map_both_16(&mut self.b, bank);
        } else {
            self.b.map_prg(0, 32, bank >> 1);
        }
        self.b.map_chr(0, 8, d & 0x7F);
        self.b
            .set_mirroring(pick(d & 0x80 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Latch519 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let mut address = address;
        if address >= 0x8000 && self.address & 0x40 != 0 {
            address &= 0xFFF0;
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if self.locked {
            self.value = (self.value & !3) | (value & 3);
        } else {
            self.address = address;
            self.value = value;
            self.locked = address & 0x100 != 0;
        }
        self.update();
    }
}

/// Mapper 235 ("Golden Game" 150-in-1 and kin): an address latch over up to
/// four 1 MiB ROMs; chips the file lacks read open bus (INES_Mapper_235.wiki,
/// The Mad Dumper's notes). Power-on and reset clear the latch.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GoldenGame235 {
    pub b: Banked,
    pub open_bus: bool,
}

impl GoldenGame235 {
    pub fn new(image: RomImage, t: i64) -> GoldenGame235 {
        let mut cart = GoldenGame235 {
            b: Banked::new(image, t),
            open_bus: false,
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let (chip, page) = ((a >> 8) & 3, a & 0x1F);
        let bank32 = (chip << 5) | page;
        self.open_bus = bank32 * 0x8000 >= self.b.prg.len() as i64;
        if a & 0x800 != 0 {
            let bank16 = (bank32 << 1) | ((a >> 12) & 1);
            map_both_16(&mut self.b, bank16);
        } else {
            self.b.map_prg(0, 32, bank32);
        }
        if a & 0x400 != 0 {
            self.b.set_mirroring(SCREEN_A);
        } else {
            self.b
                .set_mirroring(pick(a & 0x2000 != 0, VERTICAL, HORIZONTAL));
        }
    }
}

impl Cartridge for GoldenGame235 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.latch(0);
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.open_bus {
            return None;
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 234: the Maxi 15 multicart, banking on the value the CPU *reads* at
/// $FF80-$FFF7 (INES_Mapper_234.wiki; the lockout defeat is left out).
/// Power-on and reset clear the registers.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Maxi15 {
    pub b: Banked,
    pub outer: u8,
    pub inner: u8,
    pub disabled: bool,
}

impl Maxi15 {
    pub fn new(image: RomImage, t: i64) -> Maxi15 {
        let mut cart = Maxi15 {
            b: Banked::new(image, t),
            outer: 0,
            inner: 0,
            disabled: false,
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (o, i) = (self.outer as i64, self.inner as i64);
        let block = o & 0x0F;
        let chip = if o & 0x20 != 0 { 0x10 } else { 0 }; // ROMs 3+4: the second half
        self.disabled = o & 0x20 != 0 && o & 0x10 != 0;
        let (prg, chr8) = if o & 0x40 != 0 {
            // NINA-03
            (
                (block & 0x0E) | (i & 1),
                ((block & 0x0E) << 2) | ((i >> 4) & 7),
            )
        } else {
            (block, (block << 2) | ((i >> 4) & 3))
        };
        self.b.map_prg(0, 32, chip | prg);
        self.b.map_chr(0, 8, (chip << 2) | chr8);
        self.b
            .set_mirroring(pick(o & 0x80 != 0, HORIZONTAL, VERTICAL));
    }

    fn latch(&mut self, address: u16, value: u8) {
        if (0xFF80..=0xFF9F).contains(&address) {
            if self.outer & 0x3F == 0 {
                self.outer = value;
                self.update();
            }
        } else if (0xFFE8..=0xFFF7).contains(&address) {
            self.inner = value;
            self.update();
        }
    }
}

impl Cartridge for Maxi15 {
    banked_field!(b);
    fn latches_reads(&self) -> bool {
        true
    }
    fn m2_gap(&mut self, _t: i64) {
        self.outer = 0;
        self.inner = 0;
        self.update();
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.disabled {
            return None;
        }
        self.b.base_cpu_read(address, t)
    }
    fn read_data(&mut self, address: u16, value: u8, _t: i64) {
        self.latch(address, value);
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            let value = self.b.bus_conflict(address, value);
            self.latch(address, value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 15: the K-1029/K-1030P boards (100-in-1 and 168-in-1 Contra
/// Function 16; INES_Mapper_015.wiki). Power-on and reset clear the latch.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct K1029 {
    pub b: Banked,
}

impl K1029 {
    pub fn new(image: RomImage, t: i64) -> K1029 {
        let mut cart = K1029 {
            b: Banked::new(image, t),
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0, 0);
        cart
    }

    fn latch(&mut self, address: u16, value: u8) {
        let mode = address & 3;
        let v = value as i64;
        let bank = v & 0x3F;
        match mode {
            0 => self.b.map_prg(0, 32, bank >> 1),
            1 => {
                self.b.map_prg(0, 16, bank);
                self.b.map_prg(2, 16, bank | 7);
            }
            2 => {
                let bank8 = (bank << 1) | (v >> 7);
                for window in 0..4 {
                    self.b.map_prg(window, 8, bank8);
                }
            }
            _ => map_both_16(&mut self.b, bank),
        }
        self.b
            .set_mirroring(pick(v & 0x40 != 0, HORIZONTAL, VERTICAL));
        self.b.chr_writable = self.b.image.chr.is_empty() && matches!(mode, 1 | 2);
    }
}

impl Cartridge for K1029 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.latch(0, 0);
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address, value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 314 (BMC-64in1NoRepeat: the Y2K multicarts;
/// NES_2.0_Mapper_314.wiki, MAME's multigame.cpp for the decoding). Power-on
/// $80, $43, $00.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct NoRepeat314 {
    pub b: Banked,
    pub regs: [u8; 4],
    pub register_mask: u16,
    pub latch: u8,
}

impl NoRepeat314 {
    pub fn new(image: RomImage, t: i64) -> NoRepeat314 {
        let b = Banked::new(image, t);
        let register_mask = if b.chr_writable { 1 } else { 3 };
        let mut cart = NoRepeat314 {
            b,
            regs: [0x80, 0x43, 0x00, 0x00],
            register_mask,
            latch: 0,
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (r0, r1, r2) = (
            self.regs[0] as i64,
            self.regs[1] as i64,
            self.regs[2] as i64,
        );
        let bank32 = ((r0 & 1) << 6) | (r1 & 0x3F);
        if r0 & 0x80 != 0 {
            // NROM
            if r1 & 0x80 != 0 {
                self.b.map_prg(0, 32, bank32);
            } else {
                let half = (bank32 << 1) | ((r1 >> 6) & 1);
                map_both_16(&mut self.b, half);
            }
        } else {
            // UNROM inside a 128 KiB block
            let base = (bank32 << 1) & !7;
            self.b.map_prg(0, 16, base | (self.latch as i64 & 7));
            self.b.map_prg(2, 16, base | 7);
        }
        self.b.map_chr(0, 8, ((r2 & 0x0F) << 2) | ((r0 >> 1) & 3));
        self.b
            .set_mirroring(pick(r0 & 0x20 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for NoRepeat314 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch = value;
        } else if (0x5000..0x6000).contains(&address) {
            self.regs[(address & self.register_mask) as usize] = value;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// NES 2.0 mapper 289 (Super HiK 17-in-1, Champion Cassette 1200-in-1, Super
/// 76-in-1; NES_2.0_Mapper_289.wiki). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Champion289 {
    pub b: Banked,
    pub mode: u8,
    pub outer: u8,
    pub latch: u8,
}

impl Champion289 {
    pub fn new(image: RomImage, t: i64) -> Champion289 {
        let mut cart = Champion289 {
            b: Banked::new(image, t),
            mode: 0,
            outer: 0,
            latch: 0,
        };
        cart.b.prg_ram_enabled = false;
        cart.b.map_chr(0, 8, 0);
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (mode, outer) = (self.mode as i64, self.outer as i64);
        let base = (outer >> 3) << 3; // 16 KiB banks of 128 KiB
        let kind = mode & 3;
        if kind == 0 {
            map_both_16(&mut self.b, base | (outer & 7));
        } else if kind == 1 {
            self.b.map_prg(0, 32, (base | (outer & 6)) >> 1);
        } else {
            let low = base | if kind == 2 { self.latch as i64 & 7 } else { 7 };
            self.b.map_prg(0, 16, low);
            self.b.map_prg(2, 16, base | 7);
        }
        self.b
            .set_mirroring(pick(mode & 8 != 0, HORIZONTAL, VERTICAL));
        self.b.chr_writable = self.b.image.chr.is_empty() && mode & 4 == 0;
    }
}

impl Cartridge for Champion289 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xE000 == 0x6000 {
            return Some(0xFC << 8);
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch = value;
        } else if address & 0xE001 == 0x6000 {
            self.mode = value;
        } else if address & 0xE001 == 0x6001 {
            self.outer = value;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// NES 2.0 mapper 290 (Asder 20-in-1): an address latch
/// (NES_2.0_Mapper_290.wiki). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Asder290 {
    pub b: Banked,
}

impl Asder290 {
    pub fn new(image: RomImage, t: i64) -> Asder290 {
        let mut cart = Asder290 {
            b: Banked::new(image, t),
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let bank32 = (a >> 11) & 0x0F;
        if a & 0x80 != 0 {
            let half = (bank32 << 1) | ((a >> 6) & 1);
            map_both_16(&mut self.b, half);
        } else {
            self.b.map_prg(0, 32, bank32);
        }
        self.b.map_chr(0, 8, ((a >> 5) & 0x18) | (a & 7));
        self.b
            .set_mirroring(pick(a & 0x400 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Asder290 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 299: a data latch at $8000-$FFFF (NES_2.0_Mapper_299.wiki).
/// Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch299 {
    pub b: Banked,
}

impl Latch299 {
    pub fn new(image: RomImage, t: i64) -> Latch299 {
        let mut cart = Latch299 {
            b: Banked::new(image, t),
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, value: u8) {
        let v = value as i64;
        let outer = (v >> 4) & 3;
        self.b.map_prg(0, 32, outer);
        self.b.map_chr(0, 8, (outer << 2) | (v & 3));
        self.b
            .set_mirroring(pick(v & 0x80 != 0, VERTICAL, HORIZONTAL));
    }
}

impl Cartridge for Latch299 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 229 (BMC 31-in-1): the address of a write to $8000-$FFFF (mask
/// $803F; INES_Mapper_229.wiki). Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch229 {
    pub b: Banked,
}

impl Latch229 {
    pub fn new(image: RomImage, t: i64) -> Latch229 {
        let mut cart = Latch229 {
            b: Banked::new(image, t),
        };
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let bank = (address & 0x1F) as i64;
        self.b.map_prg(0, 16, bank);
        self.b.map_prg(2, 16, if bank != 0 { bank } else { 1 });
        self.b.map_chr(0, 8, bank);
        self.b
            .set_mirroring(pick(address & 0x20 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Latch229 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 221 (NTDEC N625092 multicarts): two address latches
/// (INES_Mapper_221.wiki; A6 also drives PRG A18, which the page does not
/// show). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct N625092 {
    pub b: Banked,
    pub mode: u16,
    pub inner: u16,
}

impl N625092 {
    pub fn new(image: RomImage, t: i64) -> N625092 {
        let mut cart = N625092 {
            b: Banked::new(image, t),
            mode: 0,
            inner: 0,
        };
        cart.b.map_chr(0, 8, 0);
        cart.update();
        cart
    }

    fn update(&mut self) {
        let mode = self.mode as i64;
        let inner = self.inner as i64 & 7;
        let base = (((mode >> 2) & 7) | ((mode >> 5) & 2) | ((mode >> 6) & 8)) << 3; // 16 KiB banks
        if mode & 2 == 0 {
            map_both_16(&mut self.b, base | inner);
        } else if mode & 0x100 != 0 {
            self.b.map_prg(0, 16, base | inner);
            self.b.map_prg(2, 16, base | 7);
        } else {
            self.b.map_prg(0, 32, (base | inner) >> 1);
        }
        self.b
            .set_mirroring(pick(mode & 1 != 0, HORIZONTAL, VERTICAL));
        self.b.chr_writable = self.b.image.chr.is_empty() && self.inner & 8 == 0;
    }
}

impl Cartridge for N625092 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x8000..0xC000).contains(&address) {
            self.mode = address;
        } else if address >= 0xC000 {
            self.inner = address;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// NES 2.0 mapper 285: the A65AS (285.0) and JY-066 (285.1) multicarts, a
/// data latch (NES_2.0_Mapper_285.wiki). 8 KiB CHR-RAM. Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct A65as {
    pub b: Banked,
    pub jy066: bool,
}

impl A65as {
    pub fn new(image: RomImage, t: i64) -> A65as {
        let jy066 = image.board.submapper == 1;
        let mut cart = A65as {
            b: Banked::new(image, t),
            jy066,
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, value: u8) {
        let v = value as i64;
        let outer = if self.jy066 {
            (v >> 4) & 3
        } else {
            (v >> 3) & 3
        };
        if v & 0x40 != 0 {
            self.b.map_prg(0, 32, (outer << 2) | ((v >> 1) & 3));
        } else {
            self.b.map_prg(0, 16, (outer << 3) | (v & 7));
            self.b.map_prg(2, 16, (outer << 3) | 7);
        }
        if v & 0x80 != 0 {
            self.b
                .set_mirroring(pick(v & 0x20 != 0, SCREEN_B, SCREEN_A));
        } else {
            let horizontal = if self.jy066 { v & 8 } else { v & 0x20 };
            self.b
                .set_mirroring(pick(horizontal != 0, HORIZONTAL, VERTICAL));
        }
    }
}

impl Cartridge for A65as {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 63 (Powerful 255-in-1 and kin): an address latch, unpopulated banks
/// open bus, which the menu uses to count the games (INES_Mapper_063.wiki).
/// Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Powerful63 {
    pub b: Banked,
    pub narrow: bool,
    pub banks: (i64, i64),
}

impl Powerful63 {
    pub fn new(image: RomImage, t: i64) -> Powerful63 {
        let narrow = image.board.submapper == 1;
        let mut cart = Powerful63 {
            b: Banked::new(image, t),
            narrow,
            banks: (0, 0),
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let bank = (a >> 2) & if self.narrow { 0x7F } else { 0xFF };
        let (low, high) = if a & 2 != 0 {
            (bank & !1, bank | 1)
        } else {
            (bank, bank)
        };
        self.banks = (low, high);
        self.b.map_prg(0, 16, low);
        self.b.map_prg(2, 16, high);
        self.b.set_mirroring(pick(a & 1 != 0, HORIZONTAL, VERTICAL));
        let protect = a & if self.narrow { 0x200 } else { 0x400 };
        self.b.chr_writable = self.b.image.chr.is_empty() && protect == 0;
    }
}

impl Cartridge for Powerful63 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 {
            let bank = if (address >> 14) & 1 != 0 {
                self.banks.1
            } else {
                self.banks.0
            };
            if bank * 0x4000 >= self.b.prg.len() as i64 {
                return None;
            }
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 62 (Super 700-in-1): the address and the data of a write to
/// $8000-$FFFF (INES_Mapper_062.wiki, Disch's notes). Power-on 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Super700 {
    pub b: Banked,
}

impl Super700 {
    pub fn new(image: RomImage, t: i64) -> Super700 {
        let mut cart = Super700 {
            b: Banked::new(image, t),
        };
        cart.latch(0, 0);
        cart
    }

    fn latch(&mut self, address: u16, value: u8) {
        let a = address as i64;
        let bank = (a & 0x40) | ((a >> 8) & 0x3F);
        if a & 0x20 != 0 {
            map_both_16(&mut self.b, bank);
        } else {
            self.b.map_prg(0, 32, bank >> 1);
        }
        self.b.map_chr(0, 8, ((a & 0x1F) << 2) | (value as i64 & 3));
        self.b
            .set_mirroring(pick(a & 0x80 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Super700 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address, value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 61 (Tetris Family 9-in-1, HQ 15-in-1, 32-in-1): an address latch
/// (INES_Mapper_061.wiki). CHR-RAM on GS-2017, unbanked. Power-on latch 0.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch61 {
    pub b: Banked,
    pub wide_chr: bool,
    pub rom_chr: bool,
}

impl Latch61 {
    pub fn new(image: RomImage, t: i64) -> Latch61 {
        let wide_chr = image.board.submapper == 1;
        let rom_chr = !image.chr.is_empty();
        let mut cart = Latch61 {
            b: Banked::new(image, t),
            wide_chr,
            rom_chr,
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let prg = a & 0x0F;
        if a & 0x10 != 0 {
            let half = (prg << 1) | ((a >> 5) & 1);
            map_both_16(&mut self.b, half);
        } else {
            self.b.map_prg(0, 32, prg);
        }
        if self.rom_chr {
            let mut bank = (a >> 8) & 0x0F;
            if self.wide_chr {
                bank = (bank << 1) | ((a >> 6) & 1);
            }
            self.b.map_chr(0, 8, bank);
        }
        self.b
            .set_mirroring(pick(a & 0x80 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Latch61 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 380: the 970630C (0) and KN-35A (1) multicarts' address latch
/// (NES_2.0_Mapper_380.wiki). Reset clears it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch380 {
    pub b: Banked,
    pub submapper: u8,
    pub pads_replace: bool,
}

impl Latch380 {
    pub fn new(image: RomImage, t: i64) -> Latch380 {
        let submapper = image.board.submapper;
        let mut cart = Latch380 {
            b: Banked::new(image, t),
            submapper,
            pads_replace: false,
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let (bank, nrom128, unrom_off, m) = ((a >> 2) & 0x1F, a & 1, a & 0x200, a & 0x100);
        let (mut low, mut high) = (bank, bank);
        if unrom_off != 0 {
            if nrom128 == 0 {
                low = bank & !1;
                high = bank | 1;
            }
        } else {
            high = bank
                | if m != 0 && self.submapper == 1 {
                    0x0F
                } else {
                    7
                };
        }
        self.b.map_prg(0, 16, low);
        self.b.map_prg(2, 16, high);
        self.b.set_mirroring(pick(a & 2 != 0, HORIZONTAL, VERTICAL));
        self.b.chr_writable = a & 0x80 == 0;
        self.pads_replace = m != 0 && self.submapper == 0;
    }
}

impl Cartridge for Latch380 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.latch(0); // the page: all bits cleared on reset
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let mut address = address;
        if self.pads_replace && address >= 0x8000 {
            address &= !0x0F;
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address >= 0x8000 {
            self.latch(address);
        }
    }
}

/// Mapper 389: the Caltron 9-in-1, all by address (NES_2.0_Mapper_389.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Caltron389 {
    pub b: Banked,
    pub outer: u16,
    pub mode: u16,
    pub inner: u16,
}

impl Caltron389 {
    pub fn new(image: RomImage, t: i64) -> Caltron389 {
        let mut cart = Caltron389 {
            b: Banked::new(image, t),
            outer: 0,
            mode: 0,
            inner: 0,
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (outer, mode, inner) = (self.outer as i64, self.mode as i64, self.inner as i64);
        let outer_prg = (outer >> 3) & 0x0F;
        if mode & 2 != 0 {
            let base = (outer_prg & !1) << 1;
            self.b.map_prg(0, 16, base | ((inner >> 2) & 3));
            self.b.map_prg(2, 16, base | 3);
        } else {
            self.b.map_prg(0, 32, outer_prg);
        }
        self.b.map_chr(0, 8, (((mode >> 3) & 7) << 2) | (inner & 3));
        self.b
            .set_mirroring(pick(outer & 1 != 0, HORIZONTAL, VERTICAL));
    }
}

impl Cartridge for Caltron389 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address < 0x8000 {
            return;
        }
        if address < 0x9000 {
            self.outer = address;
        } else if address < 0xA000 {
            self.mode = address;
        } else {
            self.inner = address;
        }
        self.update();
    }
}

/// Mapper 396: J.Y.'s YY850437C and Realtec's GN-51: UNROM with an outer bank
/// (NES_2.0_Mapper_396.wiki; 396.3 starts in the last outer bank).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Yy850437c {
    pub b: Banked,
    pub submapper: u8,
    pub outer: i64,
    pub inner: i64,
}

impl Yy850437c {
    pub fn new(image: RomImage, t: i64) -> Yy850437c {
        let submapper = image.board.submapper;
        let mut cart = Yy850437c {
            b: Banked::new(image, t),
            submapper,
            outer: if submapper == 3 { 7 } else { 0 },
            inner: 0,
        };
        cart.b.map_chr(0, 8, 0);
        cart.update();
        cart
    }

    fn update(&mut self) {
        self.b.map_prg(0, 16, (self.outer << 3) | self.inner);
        self.b.map_prg(2, 16, (self.outer << 3) | 7);
    }
}

impl Cartridge for Yy850437c {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address < 0x8000 {
            return;
        }
        let outer = if self.submapper == 3 {
            address & 0xC000 == 0x8000
        } else {
            address & 0xE000 == 0xA000
        };
        if outer {
            self.outer = (value & 7) as i64;
            let bits = match self.submapper {
                0 => 0x60,
                1 => 0x40,
                _ => 0x20,
            };
            self.b
                .set_mirroring(pick(value & bits != 0, HORIZONTAL, VERTICAL));
        } else {
            self.inner = (value & 7) as i64;
        }
        self.update();
    }
}

/// Mapper 400: retroUSB's 8-BIT XMAS 2017, UNROM 512's Christmas lights as a
/// multicart (NES_2.0_Mapper_400.wiki; the LEDs and LCD are not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct RetroUsb400 {
    pub b: Banked,
    pub low_outer: u8,
    pub high_outer: u8,
    pub inner: u8,
}

impl RetroUsb400 {
    pub fn new(image: RomImage, t: i64) -> RetroUsb400 {
        let mut cart = RetroUsb400 {
            b: Banked::new(image, t),
            low_outer: 0,
            high_outer: 0x1F,
            inner: 0,
        };
        cart.update();
        cart
    }

    fn update(&mut self) {
        let inner = self.inner as i64;
        self.b
            .map_prg(0, 16, ((self.low_outer as i64) << 3) | (inner & 7));
        self.b.map_prg(2, 16, ((self.high_outer as i64) << 3) | 7);
        self.b.map_chr(0, 8, (inner >> 5) & 3);
    }
}

impl Cartridge for RetroUsb400 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0xC000 {
            self.inner = value;
        } else if address & 0xF83F == 0x7000 {
            self.low_outer = value >> 3;
        } else if address & 0xF800 == 0x7800 {
            self.high_outer = value >> 3;
            self.b
                .set_mirroring(pick(value & 0x20 != 0, HORIZONTAL, VERTICAL));
        } else {
            return;
        }
        self.update();
    }
}

/// Mapper 409: retroUSB's DPCMcart: UNROM addressing 64 MiB by a 12-bit
/// address latch at $C000-$CFFF (NES_2.0_Mapper_409.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DpcmCart409 {
    pub b: Banked,
}

impl DpcmCart409 {
    pub fn new(image: RomImage, t: i64) -> DpcmCart409 {
        let mut b = Banked::new(image, t);
        b.map_chr(0, 8, 0);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        DpcmCart409 { b }
    }
}

impl Cartridge for DpcmCart409 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address & 0xF000 == 0xC000 {
            self.b.map_prg(0, 16, (address & 0x0FFF) as i64);
        }
    }
}

/// Mapper 471: Impact Soft's IM1 (Haratyler, Haraforce): mapper 201 with a
/// scanline IRQ on every fall of PPU A12, unfiltered (NES_2.0_Mapper_471.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImpactSoft471 {
    pub b: Banked,
    pub a12: u8,
    pub irq: bool,
}

impl ImpactSoft471 {
    pub fn new(image: RomImage, t: i64) -> ImpactSoft471 {
        let mut b = Banked::new(image, t);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        ImpactSoft471 {
            b,
            a12: 0,
            irq: false,
        }
    }
}

impl Cartridge for ImpactSoft471 {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address >= 0x8000 {
            self.b.map_prg(0, 32, (address & 1) as i64);
            self.b.map_chr(0, 8, (address & 1) as i64);
            self.irq = false;
        }
    }
    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a12 = ((address >> 12) & 1) as u8;
        if self.a12 != 0 && a12 == 0 {
            self.irq = true;
        }
        self.a12 = a12;
    }
}

/// Mapper 493: the Maxivision 30-in-1 prototype: mapper 11 with an outer bank
/// (NES_2.0_Mapper_493.wiki). Vertical mirroring.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Maxivision493 {
    pub b: Banked,
    pub outer: u8,
    pub inner: u8,
}

impl Maxivision493 {
    pub fn new(image: RomImage, t: i64) -> Maxivision493 {
        let mut cart = Maxivision493 {
            b: Banked::new(image, t),
            outer: 0,
            inner: 0,
        };
        cart.b.set_mirroring(VERTICAL);
        cart.update();
        cart
    }

    fn update(&mut self) {
        let (outer, inner) = (self.outer as i64, self.inner as i64);
        self.b.map_prg(0, 32, (outer << 1) | (inner & 1));
        self.b.map_chr(0, 8, (outer << 3) | ((inner >> 4) & 7));
    }
}

impl Cartridge for Maxivision493 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x8000 {
            self.inner = self.b.bus_conflict(address, value);
        } else if address >= 0x6000 {
            self.outer = value & 0x0F;
        } else {
            return;
        }
        self.update();
    }
}

/// Mapper 541: the LittleCom 160-in-1's address latch at $C000-$FFFF
/// (NES_2.0_Mapper_541.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct LittleCom541 {
    pub b: Banked,
}

impl LittleCom541 {
    pub fn new(image: RomImage, t: i64) -> LittleCom541 {
        let mut cart = LittleCom541 {
            b: Banked::new(image, t),
        };
        cart.b.map_chr(0, 8, 0);
        cart.latch(0);
        cart
    }

    fn latch(&mut self, address: u16) {
        let a = address as i64;
        let bank = (a >> 2) & 0x3F;
        self.b
            .map_prg(0, 16, if a & 2 != 0 { bank } else { bank & !1 });
        self.b
            .map_prg(2, 16, if a & 2 != 0 { bank } else { bank | 1 });
        self.b.set_mirroring(pick(a & 1 != 0, VERTICAL, HORIZONTAL));
    }
}

impl Cartridge for LittleCom541 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address >= 0xC000 {
            self.latch(address);
        }
    }
}

/// Mapper 81: NTDEC's N715021 (Super Gun; INES_Mapper_081.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ntdec81 {
    pub b: Banked,
}

impl Ntdec81 {
    pub fn new(image: RomImage, t: i64) -> Ntdec81 {
        let mut b = Banked::new(image, t);
        b.map_prg(0, 16, 0);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 8, 0);
        Ntdec81 { b }
    }
}

impl Cartridge for Ntdec81 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address >= 0x8000 {
            self.b.map_prg(0, 16, ((address >> 2) & 3) as i64);
            self.b.map_chr(0, 8, (address & 3) as i64);
        }
    }
}

/// Mapper 122: the JY043: 32 KiB of PRG and two 4 KiB CHR banks
/// (INES_Mapper_122.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jy043 {
    pub b: Banked,
}

impl Jy043 {
    pub fn new(image: RomImage, t: i64) -> Jy043 {
        let mut b = Banked::new(image, t);
        b.map_prg(0, 32, 0);
        Jy043 { b }
    }
}

impl Cartridge for Jy043 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x8000 {
            self.b
                .map_chr(if address & 1 != 0 { 4 } else { 0 }, 4, (value & 7) as i64);
        }
    }
}

// -- SB-5013 (an MMC3 subclass) --------------------------------------------------

/// Mappers 359 (SB-5013, GCL8050, 841242C: Super Boy multicarts) and 540
/// (UNL-82112C, 2 KiB CHR banks) (NES_2.0_Mapper_359.wiki, _540.wiki; the
/// registers and the two IRQ sources are described in the reference).
///
/// The reference's counter is the MMC3's attribute, 16 bits wide in the M2
/// mode; here it is this board's own `counter` (the MMC3's `u8` one unused),
/// and `clock_counter` is the MMC3's with it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sb5013 {
    pub m: Mmc3,
    pub two_kib: bool,
    pub prg8: [u8; 4],
    pub chr1: [u8; 8],
    pub prg_outer: u8,
    pub prg_size: u8,
    pub chr_outer: u8,
    pub chr_wide: u8,
    pub a12_source: bool,
    pub auto: bool,
    pub counter: u32,
}

impl Sb5013 {
    pub fn new(image: RomImage, t: i64) -> Sb5013 {
        let two_kib = image.board.mapper == 540;
        let mut board = Sb5013 {
            m: Mmc3::new(image, t),
            two_kib,
            prg8: [0; 4],
            chr1: [0; 8],
            prg_outer: 0,
            prg_size: 0,
            chr_outer: 0,
            chr_wide: 0,
            a12_source: false,
            auto: false,
            counter: 0,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        board
    }

    fn bank(&self, bank: u8) -> i64 {
        let mask: i64 = [0x3F, 0x1F, 0x2F, 0x0F][self.prg_size as usize];
        ((self.prg_outer as i64 * 16) & !mask) | (bank as i64 & mask)
    }

    fn enable(&mut self, on: bool) {
        self.m.irq_enabled = on;
        if !on {
            self.m.irq = false;
        }
    }
}

impl Mmc3Like for Sb5013 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn update(&mut self) {
        for window in 0..3 {
            let bank = self.bank(self.prg8[window]);
            self.map_prg(window, 8, bank);
        }
        let last = self.bank(0xFF);
        self.map_prg(3, 8, last);
        if self.m.b.chr_writable {
            self.map_chr(0, 8, 0);
        } else if self.two_kib {
            for (window, register) in [0, 1, 6, 7].into_iter().enumerate() {
                let bank = self.chr1[register] as i64;
                self.map_chr(window * 2, 2, bank);
            }
        } else {
            let mask: i64 = if self.chr_wide != 0 { 0xFF } else { 0x7F };
            for window in 0..8 {
                let bank =
                    (((self.chr_outer as i64) << 7) & !mask) | (self.chr1[window] as i64 & mask);
                self.map_chr(window, 1, bank);
            }
        }
    }
    /// `MMC3.clock_counter` on this board's wider counter.
    fn clock_counter(&mut self) {
        let was = self.counter;
        let reload = self.m.reload;
        if was == 0 || reload {
            self.counter = self.m.latch as u32;
        } else {
            self.counter = was - 1;
        }
        self.m.reload = false;
        if self.counter == 0 && self.m.irq_enabled && (!self.m.nec_irq || was != 0 || reload) {
            self.m.irq = true;
        }
    }
}

impl Cartridge for Sb5013 {
    mmc3_cartridge!();
    fn counts_m2(&self) -> bool {
        true
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            let bank = self.bank(self.prg8[3]);
            let len = self.m.b.prg.len() as i64;
            let offset = (bank * 0x2000 + (address as i64 & 0x1FFF)).rem_euclid(len);
            return Some(self.m.b.prg[offset as usize] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let register = address & 0xF003;
        if address < 0x8000 {
            return;
        }
        if register < 0x8004 {
            self.prg8[(register & 3) as usize] = value;
        } else if register == 0x9000 {
            self.prg_outer = (value >> 3) & 7;
        } else if register == 0x9001 {
            self.prg_size = value & 3;
            self.chr_wide = value & 0x40;
        } else if register == 0x9002 {
            self.m
                .b
                .set_mirroring([VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B][(value & 3) as usize]);
        } else if register == 0x9003 {
            self.chr_outer = value & 3;
        } else if (0xA000..0xC000).contains(&register) {
            self.chr1[(((register >> 10) & 4) | (register & 3)) as usize] = value;
        } else if register == 0xC000 {
            if self.a12_source {
                self.m.reload = true;
            } else {
                self.counter = (self.counter & 0xFF00) | value as u32;
            }
            if self.auto {
                self.enable(false);
            }
        } else if register == 0xC001 {
            if self.a12_source {
                self.m.latch = value;
            } else {
                self.counter = (self.counter & 0x00FF) | ((value as u32) << 8);
            }
            if self.auto {
                self.enable(true);
            }
        } else if register == 0xC002 {
            self.auto = value & 4 != 0;
            self.a12_source = value & 2 != 0;
            self.enable(value & 1 != 0);
        } else if register == 0xC003 && !self.auto {
            self.enable(value & 1 != 0);
        }
        self.update();
    }
    fn m2(&mut self, _t: i64) {
        if !self.a12_source && self.m.irq_enabled && self.counter != 0 {
            self.counter -= 1;
            if self.counter == 0 {
                self.m.irq = true;
            }
        }
    }
    fn ppu_address(&mut self, address: u16, t: i64) {
        if self.a12_source {
            base_ppu_address(self, address, t);
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
        15 => Box::new(K1029::new(image, t)),
        57 => Box::new(Gk57::new(image, t)),
        58 | 213 => Box::new(AddressLatch58::new(image, t)),
        59 => Box::new(AddressLatch59::new(image, t)),
        60 => Box::new(ResetMulticart60::new(image, t)),
        61 => Box::new(Latch61::new(image, t)),
        62 => Box::new(Super700::new(image, t)),
        63 => Box::new(Powerful63::new(image, t)),
        81 => Box::new(Ntdec81::new(image, t)),
        122 => Box::new(Jy043::new(image, t)),
        212 => Box::new(AddressLatch212::new(image, t)),
        221 => Box::new(N625092::new(image, t)),
        225 | 255 => Box::new(AddressLatch225::new(image, t)),
        226 => Box::new(Latch226::new(image, t)),
        227 | 242 | 301 => Box::new(AddressLatch242::new(image, t)),
        229 => Box::new(Latch229::new(image, t)),
        234 => Box::new(Maxi15::new(image, t)),
        235 => Box::new(GoldenGame235::new(image, t)),
        285 => Box::new(A65as::new(image, t)),
        286 => Box::new(Bs5::new(image, t)),
        289 => Box::new(Champion289::new(image, t)),
        290 => Box::new(Asder290::new(image, t)),
        299 => Box::new(Latch299::new(image, t)),
        314 => Box::new(NoRepeat314::new(image, t)),
        319 => Box::new(Hp898f::new(image, t)),
        332 => Box::new(Ws332::new(image, t)),
        359 | 540 => Box::new(Sb5013::new(image, t)),
        374 => Box::new(Mmc1::reset_multicart(image, t)),
        380 => Box::new(Latch380::new(image, t)),
        389 => Box::new(Caltron389::new(image, t)),
        396 => Box::new(Yy850437c::new(image, t)),
        400 => Box::new(RetroUsb400::new(image, t)),
        409 => Box::new(DpcmCart409::new(image, t)),
        471 => Box::new(ImpactSoft471::new(image, t)),
        493 => Box::new(Maxivision493::new(image, t)),
        519 => Box::new(Latch519::new(image, t)),
        541 => Box::new(LittleCom541::new(image, t)),
        _ => return Err(image),
    })
}
