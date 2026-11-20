// SPDX-License-Identifier: MIT
//! J.Y. Company's ASIC (209, and the boards 35, 90 and 211) and the
//! JY830623C (91), from J.Y._Company_ASIC.wiki and INES_Mapper_091.wiki
//! (`jycompany.py`). No judge ROM exists; the claims rest on sweeps against
//! Mesen2, a detector.
//!
//! **The ASIC's subclass.** multiasic's `HostJY` overrides the `map_prg` and
//! `_map_chr` the ASIC calls on `self`; here those are the methods of
//! [`JyLike`] (the `mmc3.rs` pattern), the base's state is [`JyCompany`]
//! (itself the plain board), and [`jy_cartridge!`] writes the `Cartridge`
//! methods a board takes from it. `HostJY.refresh` is [`jy_update`].

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// $D001 D1-D0, by CIRAM A10's source.
pub const ARRANGEMENTS: [Mirroring; 4] = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];
pub const SOURCE_M2: u8 = 0;
pub const SOURCE_A12: u8 = 1;
pub const SOURCE_PPU_READS: u8 = 2;
pub const SOURCE_CPU_WRITES: u8 = 3;

/// PRG mode 3: bank bits 0-6 reversed.
pub fn reverse7(bank: i64) -> i64 {
    (bank & 0x80)
        | (0..7)
            .map(|bit| ((bank >> bit) & 1) << (6 - bit))
            .sum::<i64>()
}

/// Mappers 209 (the standard board), 35 (the same, WRAM in the header), 211
/// (a duplicate of 209) and 90 (a jumper inhibits ROM nametables and the
/// extended mirroring). The base class's state; also the plain board.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct JyCompany {
    pub b: Banked,
    pub nametables_allowed: bool,
    pub prg_regs: [u8; 4],
    /// low | high << 8
    pub chr_regs: [u16; 8],
    pub nt_regs: [u16; 4],
    pub mode: u8,
    pub arrangement: u8,
    pub ppu_config: u8,
    pub outer: u8,
    pub latches: [u8; 2],
    pub irq_enabled: bool,
    pub irq_mode: u8,
    pub prescaler: u8,
    pub counter: u8,
    pub xor: u8,
    pub multiplicand: u8,
    pub multiplier: u8,
    pub accumulator: u8,
    pub test: u8,
    pub a12: u16,
    pub rom_bank_6000: Option<i64>,
    pub irq: bool,
}

impl JyCompany {
    /// `JYCompany.__init__` up to (not including) its `_update()`, which the
    /// board's constructor calls as [`jy_update`].
    pub fn state(image: RomImage, cpu_cycle_ticks: i64) -> JyCompany {
        let nametables_allowed = image.board.mapper != 90;
        JyCompany {
            b: Banked::new(image, cpu_cycle_ticks),
            nametables_allowed,
            prg_regs: [0; 4],
            chr_regs: [0; 8],
            nt_regs: [0; 4],
            mode: 0,
            arrangement: 0,
            ppu_config: 0,
            outer: 0,
            latches: [0, 0],
            irq_enabled: false,
            irq_mode: 0,
            prescaler: 0,
            counter: 0,
            xor: 0,
            multiplicand: 0,
            multiplier: 0,
            accumulator: 0,
            test: 0,
            a12: 0,
            rom_bank_6000: None,
            irq: false,
        }
    }

    /// The whole `JYCompany.__init__`: the plain board.
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> JyCompany {
        let mut jy = JyCompany::state(image, cpu_cycle_ticks);
        jy_update(&mut jy);
        jy
    }

    /// `_chr_outer`: the outer CHR bank's size and base, in bytes.
    pub fn chr_outer(&self) -> (i64, i64) {
        let o = self.outer as i64;
        if o & 0x20 != 0 {
            return (0x80000, ((o >> 3) & 3) * 0x80000);
        }
        (0x40000, ((o >> 3) & 3) * 0x80000 + (o & 1) * 0x40000)
    }

    /// `_rom_nametable`: the ROM bank of a nametable address, or None if CIRAM answers.
    pub fn rom_nametable(&self, address: u16) -> Option<u16> {
        if !(self.nametables_allowed && self.mode & 0x20 != 0) {
            return None;
        }
        let reg = self.nt_regs[((address >> 10) & 3) as usize];
        if self.mode & 0x40 != 0 || (reg >> 7) & 1 != ((self.ppu_config >> 7) & 1) as u16 {
            return Some(reg);
        }
        None
    }
}

/// The ASIC's overridable methods. Default bodies: the base's.
pub trait JyLike {
    fn jy(&self) -> &JyCompany;
    fn jy_mut(&mut self) -> &mut JyCompany;

    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.jy_mut().b.map_prg(window, size_kib, bank);
    }
    /// `_map_chr`: CHR through the outer bank.
    fn map_chr_outer(&mut self, window: usize, size_kib: usize, bank: i64) {
        jy_base_map_chr(self.jy_mut(), window, size_kib, bank);
    }
}

/// `JYCompany._map_chr`.
pub fn jy_base_map_chr(jy: &mut JyCompany, window: usize, size_kib: usize, bank: i64) {
    let (size, base) = jy.chr_outer();
    let start = base + (bank * size_kib as i64 * 0x400).rem_euclid(size);
    let len = jy.b.chr.len() as i64;
    for i in 0..size_kib {
        jy.b.chr_offsets[window + i] = (start + i as i64 * 0x400).rem_euclid(len) as usize;
    }
}

/// `JYCompany._update`.
pub fn jy_update<T: JyLike + ?Sized>(this: &mut T) {
    jy_update_prg(this);
    jy_update_chr(this);
    let jy = this.jy_mut();
    jy.b.set_mirroring(ARRANGEMENTS[(jy.arrangement & 3) as usize]);
}

/// `JYCompany._update_prg`.
pub fn jy_update_prg<T: JyLike + ?Sized>(this: &mut T) {
    let jy = this.jy();
    let mode = jy.mode & 3;
    let r = jy.prg_regs.map(|x| x as i64);
    let last = jy.mode & 4 != 0;
    let outer8 = (((jy.outer >> 1) & 3) as i64) << 6;
    let banks: [i64; 4];
    let bank6000: i64;
    if mode == 0 {
        let bank32 = if last { r[3] & 0x0F } else { 0x0F };
        banks = [0, 1, 2, 3].map(|i| bank32 * 4 + i);
        bank6000 = (r[3] << 2 | 3) & 0x3F;
    } else if mode == 1 {
        let low = r[1] & 0x1F;
        let high = if last { r[3] & 0x1F } else { 0x1F };
        banks = [low * 2, low * 2 + 1, high * 2, high * 2 + 1];
        bank6000 = (r[3] << 1 | 1) & 0x3F;
    } else {
        let fix = |x: i64| if mode == 3 { reverse7(x) } else { x };
        banks = [
            fix(r[0]),
            fix(r[1]),
            fix(r[2]),
            if last { fix(r[3]) } else { 0x3F },
        ]
        .map(|b| b & 0x3F);
        bank6000 = fix(r[3]) & 0x3F;
    }
    let rom_6000 = jy.mode & 0x80 != 0;
    for (window, bank) in banks.into_iter().enumerate() {
        this.map_prg(window, 8, outer8 | bank);
    }
    this.jy_mut().rom_bank_6000 = if rom_6000 {
        Some(outer8 | bank6000)
    } else {
        None
    };
}

/// `JYCompany._update_chr`.
#[allow(clippy::needless_range_loop)] // i is the window and the register
pub fn jy_update_chr<T: JyLike + ?Sized>(this: &mut T) {
    let jy = this.jy();
    let r = jy.chr_regs.map(|x| x as i64);
    let mode = (jy.mode >> 3) & 3;
    if mode == 0 {
        this.map_chr_outer(0, 8, r[0]);
    } else if mode == 1 {
        let mmc4 = jy.outer & 0x80 != 0;
        let latches = jy.latches;
        this.map_chr_outer(0, 4, if mmc4 && latches[0] != 0 { r[2] } else { r[0] });
        this.map_chr_outer(4, 4, if mmc4 && latches[1] != 0 { r[6] } else { r[4] });
    } else if mode == 2 {
        for i in 0..4 {
            this.map_chr_outer(i * 2, 2, r[i * 2]);
        }
    } else {
        for i in 0..8 {
            this.map_chr_outer(i, 1, r[i]);
        }
    }
}

/// `JYCompany.cpu_read`.
pub fn jy_cpu_read(jy: &JyCompany, address: u16, t: i64) -> Option<u16> {
    if address == 0x5000 || address == 0x5400 || address == 0x5C00 {
        return Some(0x3F << 8); // the solder pads (D7-D6): 0
    }
    let product = jy.multiplicand as u16 * jy.multiplier as u16;
    match address & 0xF803 {
        0x5800 => return Some(product & 0xFF),
        0x5801 => return Some(product >> 8),
        0x5802 => return Some(jy.accumulator as u16),
        0x5803 => return Some(jy.test as u16),
        _ => {}
    }
    if (0x6000..0x8000).contains(&address) {
        if let Some(bank) = jy.rom_bank_6000 {
            let prg = &jy.b.prg;
            let offset = (bank * 0x2000 + (address as i64 & 0x1FFF)).rem_euclid(prg.len() as i64);
            return Some(prg[offset as usize] as u16);
        }
    }
    jy.b.base_cpu_read(address, t)
}

/// `JYCompany.cpu_write`.
pub fn jy_cpu_write<T: JyLike + ?Sized>(this: &mut T, address: u16, value: u8, t: i64) {
    if this.jy().irq_mode & 3 == SOURCE_CPU_WRITES {
        jy_clock_irq(this.jy_mut());
    }
    let jy = this.jy_mut();
    if address < 0x8000 {
        let register = address & 0xF803;
        if register == 0x5800 {
            jy.multiplicand = value;
        } else if register == 0x5801 {
            jy.multiplier = value;
        } else if register == 0x5802 {
            jy.accumulator = jy.accumulator.wrapping_add(value);
        } else if register == 0x5803 {
            jy.accumulator = 0;
            jy.test = value;
        } else if jy.rom_bank_6000.is_none() {
            jy.b.base_cpu_write(address, value, t);
        }
        return;
    }
    let region = address & 0xF000;
    if region == 0xC000 {
        jy_irq_register(jy, (address & 7) as u8, value);
        return;
    }
    if address & 0x0800 != 0 {
        return; // A11 set: the other registers ignore the write
    }
    if region == 0x8000 {
        jy.prg_regs[(address & 3) as usize] = value;
    } else if region == 0x9000 || region == 0xA000 {
        let i = (address & 7) as usize;
        let shift = if region == 0xA000 { 8 } else { 0 };
        jy.chr_regs[i] = (jy.chr_regs[i] & (0xFF00 >> shift)) | (value as u16) << shift;
    } else if region == 0xB000 {
        let i = (address & 3) as usize;
        let shift = if address & 4 != 0 { 8 } else { 0 };
        jy.nt_regs[i] = (jy.nt_regs[i] & (0xFF00 >> shift)) | (value as u16) << shift;
    } else if region == 0xD000 {
        match address & 3 {
            0 => jy.mode = value,
            1 => jy.arrangement = value,
            2 => jy.ppu_config = value,
            _ => jy.outer = value,
        }
    }
    jy_update(this);
}

/// `JYCompany._irq_register`.
pub fn jy_irq_register(jy: &mut JyCompany, register: u8, value: u8) {
    match register {
        0 | 2 | 3 => {
            let enable = if register == 0 {
                value & 1 != 0
            } else {
                register == 3
            };
            if !enable {
                jy.irq = false;
                jy.prescaler = 0;
            }
            jy.irq_enabled = enable;
        }
        1 => jy.irq_mode = value,
        4 => jy.prescaler = value ^ jy.xor,
        5 => jy.counter = value ^ jy.xor,
        6 => jy.xor = value,
        _ => {}
    }
}

/// `JYCompany._clock_irq`.
pub fn jy_clock_irq(jy: &mut JyCompany) {
    let direction = jy.irq_mode >> 6;
    if !jy.irq_enabled || direction == 0 || direction == 3 {
        return;
    }
    let mask = if jy.irq_mode & 4 != 0 { 0x07 } else { 0xFF };
    let up = direction == 1;
    jy.prescaler = if up {
        jy.prescaler.wrapping_add(1)
    } else {
        jy.prescaler.wrapping_sub(1)
    };
    if jy.prescaler & mask != (if up { 0 } else { mask }) {
        return;
    }
    jy.counter = if up {
        jy.counter.wrapping_add(1)
    } else {
        jy.counter.wrapping_sub(1)
    };
    if jy.counter == (if up { 0 } else { 0xFF }) {
        jy.irq = true;
    }
}

/// `JYCompany.m2`.
pub fn jy_m2(jy: &mut JyCompany) {
    if jy.irq_mode & 3 == SOURCE_M2 {
        jy_clock_irq(jy);
    }
}

/// `JYCompany.ppu_address`.
pub fn jy_ppu_address(jy: &mut JyCompany, address: u16) {
    let source = jy.irq_mode & 3;
    let a12 = (address >> 12) & 1;
    if (source == SOURCE_A12 && a12 != 0 && jy.a12 == 0) || source == SOURCE_PPU_READS {
        jy_clock_irq(jy);
    }
    jy.a12 = a12;
}

/// `JYCompany.ciram_a10`.
pub fn jy_ciram_a10(jy: &JyCompany, address: u16) -> Option<u8> {
    if jy.rom_nametable(address).is_some() {
        return None;
    }
    let extended = jy.nametables_allowed && (jy.arrangement & 8 != 0 || jy.mode & 0x20 != 0);
    if extended {
        return Some((jy.nt_regs[((address >> 10) & 3) as usize] & 1) as u8);
    }
    jy.b.base_ciram_a10(address)
}

/// `JYCompany.ppu_read`.
pub fn jy_ppu_read<T: JyLike + ?Sized>(this: &mut T, address: u16, t: i64) -> Option<u8> {
    let jy = this.jy();
    if address >= 0x2000 {
        let bank = jy.rom_nametable(address)? as usize;
        let chr = &jy.b.chr;
        return Some(chr[(bank * 0x400 + (address as usize & 0x3FF)) % chr.len()]);
    }
    let value = jy.b.base_ppu_read(address, t);
    if jy.outer & 0x80 != 0 {
        let low = address & 0xFF8;
        if low == 0xFD8 || low == 0xFE8 {
            this.jy_mut().latches[(address >> 12) as usize] = if low == 0xFE8 { 1 } else { 0 };
            jy_update_chr(this);
        }
    }
    value
}

/// `JYCompany.ppu_write`.
pub fn jy_ppu_write(jy: &mut JyCompany, address: u16, value: u8, t: i64) {
    if address < 0x2000 && !(jy.b.chr_writable && jy.ppu_config & 0x40 != 0) {
        return;
    }
    jy.b.base_ppu_write(address, value, t);
}

/// The `Cartridge` methods a J.Y. board takes from the base class: `banked`,
/// `banked_mut`, `counts_m2`, `watches_ppu_address` and `irq`, and each named
/// one of `cpu_read`, `cpu_write`, `m2`, `ppu_address`, `ciram_a10`,
/// `ppu_read` and `ppu_write`.
#[macro_export]
macro_rules! jy_cartridge {
    ($($method:ident),* $(,)?) => {
        fn banked(&self) -> Option<&$crate::cartridge::Banked> {
            Some(&$crate::cartridge::jycompany::JyLike::jy(self).b)
        }
        fn banked_mut(&mut self) -> Option<&mut $crate::cartridge::Banked> {
            Some(&mut $crate::cartridge::jycompany::JyLike::jy_mut(self).b)
        }
        fn counts_m2(&self) -> bool {
            true
        }
        fn watches_ppu_address(&self) -> bool {
            true
        }
        fn irq(&self) -> bool {
            $crate::cartridge::jycompany::JyLike::jy(self).irq
        }
        $( $crate::jy_cartridge!(@ $method); )*
    };
    (@ cpu_read) => {
        fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
            $crate::cartridge::jycompany::jy_cpu_read(
                $crate::cartridge::jycompany::JyLike::jy(self), address, t)
        }
    };
    (@ cpu_write) => {
        fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
            $crate::cartridge::jycompany::jy_cpu_write(self, address, value, t);
        }
    };
    (@ m2) => {
        fn m2(&mut self, _t: i64) {
            $crate::cartridge::jycompany::jy_m2(
                $crate::cartridge::jycompany::JyLike::jy_mut(self));
        }
    };
    (@ ppu_address) => {
        fn ppu_address(&mut self, address: u16, _t: i64) {
            $crate::cartridge::jycompany::jy_ppu_address(
                $crate::cartridge::jycompany::JyLike::jy_mut(self), address);
        }
    };
    (@ ciram_a10) => {
        fn ciram_a10(&mut self, address: u16) -> Option<u8> {
            $crate::cartridge::jycompany::jy_ciram_a10(
                $crate::cartridge::jycompany::JyLike::jy(self), address)
        }
    };
    (@ ppu_read) => {
        fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
            $crate::cartridge::jycompany::jy_ppu_read(self, address, t)
        }
    };
    (@ ppu_write) => {
        fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
            $crate::cartridge::jycompany::jy_ppu_write(
                $crate::cartridge::jycompany::JyLike::jy_mut(self), address, value, t);
        }
    };
}

impl JyLike for JyCompany {
    fn jy(&self) -> &JyCompany {
        self
    }
    fn jy_mut(&mut self) -> &mut JyCompany {
        self
    }
}

impl Cartridge for JyCompany {
    jy_cartridge!(
        cpu_read,
        cpu_write,
        m2,
        ppu_address,
        ciram_a10,
        ppu_read,
        ppu_write
    );
}

// -- JY830623C ---------------------------------------------------------------------

/// Mapper 91: J.Y. Company's JY830623C/YY840238C (91.0, an A12 counter) and
/// EJ-006-1 (91.1, a cycle counter) (INES_Mapper_091.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Jy830623c {
    pub b: Banked,
    pub cycle_counter: bool,
    pub mask: u16,
    pub chr_regs: [u8; 4],
    pub prg_regs: [u8; 2],
    pub outer: u16,
    pub counter: i64,
    pub counting: bool,
    pub prescaler: u8,
    pub a12: u16,
    pub irq: bool,
}

impl Jy830623c {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Jy830623c {
        let cycle_counter = image.board.submapper == 1;
        let mut j = Jy830623c {
            b: Banked::new(image, cpu_cycle_ticks),
            cycle_counter,
            mask: if cycle_counter { 0xF007 } else { 0xF003 },
            chr_regs: [0; 4],
            prg_regs: [0, 0],
            outer: 0,
            counter: 0,
            counting: false,
            prescaler: 0,
            a12: 0,
            irq: false,
        };
        j.update();
        j
    }

    fn update(&mut self) {
        let prg_outer = (((self.outer >> 1) & 3) << 4) as i64; // 8 KiB units of 128 KiB
        let last = (self.b.prg.len() / 0x2000) as i64 - 1;
        self.b
            .map_prg(0, 8, prg_outer | (self.prg_regs[0] & 0x0F) as i64);
        self.b
            .map_prg(1, 8, prg_outer | (self.prg_regs[1] & 0x0F) as i64);
        self.b.map_prg(2, 8, (prg_outer | 0x0E) & last);
        self.b.map_prg(3, 8, (prg_outer | 0x0F) & last);
        let chr_outer = ((self.outer & 1) << 8) as i64; // 2 KiB units of 512 KiB
        for i in 0..4 {
            self.b
                .map_chr(i * 2, 2, chr_outer | self.chr_regs[i] as i64);
        }
    }
}

impl Cartridge for Jy830623c {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if !self.cycle_counter && (0x8000..0xA000).contains(&address) {
            self.outer = address & 7;
            self.update();
            return;
        }
        let register = address & self.mask;
        if (0x6000..=0x6003).contains(&register) {
            self.chr_regs[(register & 3) as usize] = value;
        } else if register == 0x7000 || register == 0x7001 {
            self.prg_regs[(register & 1) as usize] = value;
        } else if register == 0x7002 || register == 0x7006 {
            self.counting = false;
            self.irq = false;
        } else if register == 0x7003 || register == 0x7007 {
            self.counting = true;
            if !self.cycle_counter {
                self.counter = 0;
            }
        } else if self.cycle_counter && (register == 0x6004 || register == 0x6005) {
            self.b.set_mirroring(if register == 0x6004 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        } else if self.cycle_counter && register == 0x6006 {
            self.counter = (self.counter & 0xFF00) | value as i64;
        } else if self.cycle_counter && register == 0x6007 {
            self.counter = (self.counter & 0x00FF) | (value as i64) << 8;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a12 = (address >> 12) & 1;
        if a12 != 0 && self.a12 == 0 && self.counting && !self.cycle_counter {
            self.counter += 1;
            if self.counter == 64 {
                self.irq = true;
            }
        }
        self.a12 = a12;
    }
    fn m2(&mut self, _t: i64) {
        if !(self.cycle_counter && self.counting) {
            return;
        }
        self.prescaler = (self.prescaler + 1) & 3;
        if self.prescaler == 0 {
            self.counter -= 5;
            if self.counter < 0 {
                self.counter &= 0xFFFF;
                self.counting = false;
                self.irq = true;
            }
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
        35 | 90 | 209 | 211 => Box::new(JyCompany::new(image, t)),
        91 => Box::new(Jy830623c::new(image, t)),
        _ => return Err(image),
    })
}
