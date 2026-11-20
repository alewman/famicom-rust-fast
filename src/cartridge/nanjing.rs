// SPDX-License-Identifier: MIT
//! One Chinese ASIC family on four boards: Nánjīng's FC-001 (163), Wàixīng's
//! FS304 (162), Dōngdá/Yànchéng's cy2000-3 (164) and Yànchéng's YC-03-09
//! (558) (`nanjing.py`).
//!
//! Each from its wiki page (INES_Mapper_162/163/164.wiki,
//! NES_2.0_Mapper_558.wiki). No judge ROM exists; 162-164 rest on sweeps
//! against Mesen2, 558 on its pictures by eye.
//!
//! The reference's `NanjingASIC` subclasses override `_update` (which its
//! `__init__` calls), `auto_switch` and `_chr_address`; here one struct
//! carries a [`Variant`].
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 162: `FS304`
//! - 163: `FC001`
//! - 164: `Dongda164`
//! - 558: `YC0309`

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::eeprom::Microwire;
use super::ines::RomImage;
use crate::banked_field;

fn swap_d0_d1(value: u8) -> u8 {
    (value & !3) | ((value & 1) << 1) | ((value >> 1) & 1)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum Variant {
    /// Mapper 163: Nánjīng's FC-001, with a protection latch (F) at $5100/$5101.
    Fc001 { feedback: u8 },
    /// Mapper 162: Wàixīng's FS304.
    Fs304,
    /// Mapper 164: the PEC-9588 / cy2000-3 board, with a 93C66 EEPROM for saves.
    Dongda164 { eeprom: Microwire },
    /// NES 2.0 mapper 558: Yànchéng's YC-03-09 (and Wàixīng's compatible
    /// board): the EEPROM unless the header has PRG-NVRAM.
    Yc0309 { eeprom: Option<Microwire> },
}

/// What the four boards share: a 32 KiB PRG bank and unbanked 8 KiB CHR-RAM;
/// the FC-001 and FS304 can make PPU A9, latched on PPU A13's last rise, CHR
/// A12 for pattern fetches ($5000 D7).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct NanjingAsic {
    pub b: Banked,
    pub variant: Variant,
    /// $5000, $5100, $5200, $5300.
    pub regs: [u8; 4],
    pub a13: u8,
    /// PPU A9 at A13's last rise.
    pub latched_a9: u8,
    /// PPU A0 likewise (164's 1 bpp mode).
    pub latched_a0: u8,
}

/// The index of a register in `regs` (the reference's dict keys), if it is one.
fn reg(register: u16) -> Option<usize> {
    match register {
        0x5000 => Some(0),
        0x5100 => Some(1),
        0x5200 => Some(2),
        0x5300 => Some(3),
        _ => None,
    }
}

impl NanjingAsic {
    fn new(image: RomImage, cpu_cycle_ticks: i64, variant: Variant) -> NanjingAsic {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; b.chr.len().max(0x2000)];
        b.chr_writable = true;
        b.map_chr(0, 8, 0);
        let mut cart = NanjingAsic {
            b,
            variant,
            regs: [0; 4],
            a13: 0,
            latched_a9: 0,
            latched_a0: 0,
        };
        cart.update();
        cart
    }

    pub fn fc001(image: RomImage, cpu_cycle_ticks: i64) -> NanjingAsic {
        NanjingAsic::new(image, cpu_cycle_ticks, Variant::Fc001 { feedback: 0 })
    }

    pub fn fs304(image: RomImage, cpu_cycle_ticks: i64) -> NanjingAsic {
        NanjingAsic::new(image, cpu_cycle_ticks, Variant::Fs304)
    }

    pub fn dongda164(image: RomImage, cpu_cycle_ticks: i64) -> NanjingAsic {
        let eeprom = Microwire::c93c66();
        let mut cart = NanjingAsic::new(image, cpu_cycle_ticks, Variant::Dongda164 { eeprom });
        cart.b.battery = true;
        cart
    }

    pub fn yc0309(image: RomImage, cpu_cycle_ticks: i64) -> NanjingAsic {
        let eeprom = if image.board.prg_nvram != 0 {
            None
        } else {
            Some(Microwire::c93c66())
        };
        let has_eeprom = eeprom.is_some();
        let mut cart = NanjingAsic::new(image, cpu_cycle_ticks, Variant::Yc0309 { eeprom });
        if has_eeprom {
            cart.b.battery = true;
        }
        cart
    }

    fn update(&mut self) {
        let r = self.regs;
        match self.variant {
            Variant::Fc001 { .. } => {
                let mut bank = (((r[2] & 3) as i64) << 4) | (r[0] & 0x0F) as i64;
                if r[3] & 4 == 0 {
                    bank |= 3;
                }
                self.b.map_prg(0, 32, bank);
            }
            Variant::Fs304 => {
                let (r5000, r5300) = (r[0] as i64, r[3] as i64);
                let a16 = if r5300 & 4 != 0 { (r5000 >> 1) & 1 } else { 1 };
                let mut a15 = (r[1] as i64 >> 1) & 1;
                if r5300 & 1 != 0 {
                    a15 = if r5300 & 4 != 0 { r5000 & 1 } else { 1 };
                }
                let bank =
                    (((r[2] & 3) as i64) << 4) | (((r5000 >> 2) & 3) << 2) | (a16 << 1) | a15;
                self.b.map_prg(0, 32, bank);
            }
            Variant::Dongda164 { .. } => {
                let r0 = r[0] as i64;
                let high = ((r[1] & 3) as i64) << 5;
                if r0 & 0x10 != 0 {
                    // BxROM
                    self.b.map_prg(0, 32, (high >> 1) | (r0 & 0x0F));
                    self.b.set_mirroring(if r[3] & 0x80 != 0 {
                        VERTICAL
                    } else {
                        HORIZONTAL
                    });
                } else {
                    // UxROM
                    self.b
                        .map_prg(0, 16, high | ((r0 >> 1) & 0x10) | (r0 & 0x0F));
                    let fixed = if r0 & 0x40 != 0 {
                        0x1C | ((r0 & 1) << 1)
                    } else {
                        0x1F
                    };
                    self.b.map_prg(2, 16, high | fixed);
                    self.b.set_mirroring(VERTICAL);
                }
            }
            Variant::Yc0309 { .. } => {
                let mut bank = (((r[1] & 3) as i64) << 4) | (r[0] & 0x0F) as i64;
                if r[3] & 4 == 0 {
                    bank |= 3;
                }
                self.b.map_prg(0, 32, bank);
            }
        }
    }

    fn auto_switch(&self) -> bool {
        match self.variant {
            Variant::Dongda164 { .. } | Variant::Yc0309 { .. } => false,
            _ => self.regs[0] & 0x80 != 0,
        }
    }

    fn chr_address(&self, address: u16) -> usize {
        let a = address as usize;
        if let Variant::Dongda164 { .. } = self.variant {
            if self.regs[0] & 0x80 != 0 {
                return (a & 0x0FF7)
                    | ((self.latched_a9 as usize) << 12)
                    | ((self.latched_a0 as usize) << 3);
            }
            return a;
        }
        if self.auto_switch() {
            return (a & 0x0FFF) | ((self.latched_a9 as usize) << 12);
        }
        a
    }

    /// Store a register write (the reference's `if register in self.regs`).
    fn store(&mut self, register: u16, value: u8) {
        if let Some(i) = reg(register) {
            self.regs[i] = value;
            self.update();
        }
    }
}

impl Cartridge for NanjingAsic {
    banked_field!(b);

    fn watches_ppu_address(&self) -> bool {
        true
    }

    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a13 = ((address >> 13) & 1) as u8;
        if a13 != 0 && self.a13 == 0 {
            self.latched_a9 = ((address >> 9) & 1) as u8;
            self.latched_a0 = (address & 1) as u8;
        }
        self.a13 = a13;
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            return Some(self.b.chr[self.chr_address(address)]);
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            let a = self.chr_address(address);
            self.b.chr[a] = value;
            return;
        }
        self.b.base_ppu_write(address, value, t);
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        match &self.variant {
            Variant::Fc001 { feedback } => {
                if address & 0xF300 == 0x5100 {
                    // D2 driven, the rest open bus
                    return Some((((!feedback & 1) as u16) << 2) | (0xFB << 8));
                }
            }
            Variant::Fs304 => {}
            Variant::Dongda164 { eeprom } => {
                if address & 0xFF00 == 0x5500 {
                    return Some((((!eeprom.output & 1) as u16) << 2) | (0xFB << 8));
                }
            }
            Variant::Yc0309 { eeprom } => {
                if address & 0xFF00 == 0x5500 {
                    if let Some(eeprom) = eeprom {
                        return Some((((eeprom.output & 1) as u16) << 2) | (0xFB << 8));
                    }
                }
            }
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if !(0x5000..0x6000).contains(&address) {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let mut value = value;
        match &mut self.variant {
            Variant::Fc001 { feedback } => {
                let register = address & 0xFF00;
                if register != 0x5300 && self.regs[3] & 1 != 0 {
                    value = swap_d0_d1(value);
                }
                if register == 0x5100 {
                    if address & 1 != 0 {
                        if value & 1 != 0 {
                            *feedback ^= 1;
                        }
                    } else {
                        *feedback = (value >> 2) & 1;
                    }
                    return;
                }
                self.store(register, value);
            }
            Variant::Fs304 => self.store(address & 0xFF00, value),
            Variant::Dongda164 { eeprom } => {
                let register = address & 0xFF00;
                if register == 0x5200 {
                    eeprom.lines((value >> 4) & 1, (value >> 2) & 1, value & 1);
                } else {
                    self.store(register, value);
                }
            }
            Variant::Yc0309 { eeprom } => {
                let register = if address & 0xFB00 == 0x5100 {
                    0x5100
                } else {
                    address & 0xFF00
                };
                let swapping = self.regs[3] & 2 == 0;
                if register == 0x5200 {
                    if let Some(eeprom) = eeprom {
                        // the wiki's table, CPU bits: CLK on D0 while swapping, else DI
                        let (d0, d1) = (value & 1, (value >> 1) & 1);
                        let (clk, di) = if swapping { (d0, d1) } else { (d1, d0) };
                        eeprom.lines((value >> 2) & 1, clk, di);
                    }
                    return;
                }
                if register != 0x5300 && swapping {
                    value = swap_d0_d1(value);
                }
                self.store(register, value);
            }
        }
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        match &self.variant {
            Variant::Dongda164 { eeprom } => Some(eeprom.memory.clone()),
            Variant::Yc0309 {
                eeprom: Some(eeprom),
            } => Some(eeprom.memory.clone()),
            _ => self.b.base_battery_ram(),
        }
    }

    fn restore(&mut self, data: &[u8]) {
        match &mut self.variant {
            Variant::Dongda164 { eeprom } => eeprom.memory.copy_from_slice(data),
            Variant::Yc0309 {
                eeprom: Some(eeprom),
            } => eeprom.memory.copy_from_slice(data),
            _ => self.b.prg_ram.copy_from_slice(data),
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
        162 => Box::new(NanjingAsic::fs304(image, t)),
        163 => Box::new(NanjingAsic::fc001(image, t)),
        164 => Box::new(NanjingAsic::dongda164(image, t)),
        558 => Box::new(NanjingAsic::yc0309(image, t)),
        _ => return Err(image),
    })
}
