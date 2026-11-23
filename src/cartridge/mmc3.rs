// SPDX-License-Identifier: MIT
//! MMC3 and its relatives (mapper 4): MMC6, MC-ACC, the NEC and Sharp IRQs;
//! its boards TxSROM (118), TQROM (119), the AA6023 (268), the multicarts 37
//! and 47, Kǎshèng's clone (115) and NES-EVENT2 (555) (`mmc3.py`; the
//! sources and the A12 filter are described there).
//!
//! **The class hierarchy.** The reference's MMC3 subclasses override the
//! helpers the base calls on `self` (`map_prg`, `map_chr`, `_update`,
//! `ram_protect`, `clock_counter`). Here those are the methods of
//! [`Mmc3Like`], whose default bodies are the free `base_*` functions, so an
//! override calls `base_*` where the Python calls `super()`. Every MMC3
//! board is a struct holding an [`Mmc3`] (the base class's state) and
//! implementing [`Mmc3Like`] and [`Cartridge`]; [`mmc3_cartridge!`] writes the
//! `Cartridge` methods a board does not override.

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;

/// The MMC3's own state (the reference's `MMC3` attributes) and its `Banked`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc3 {
    pub b: Banked,
    pub nec_irq: bool,
    pub fixed_mirroring: bool,
    pub bank_select: u8,
    pub registers: [u8; 8],
    pub latch: u8,
    pub counter: u8,
    pub reload: bool,
    pub irq_enabled: bool,
    pub a12: u8,
    /// The time of A12's third M2 fall while low.
    pub a12_ready: i64,
    pub irq: bool,
}

impl Mmc3 {
    /// The base class's `__init__` up to (not including) its `_update()`,
    /// which the board's constructor calls through [`Mmc3Like::update`].
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc3 {
        let submapper = image.board.submapper;
        Mmc3 {
            b: Banked::new(image, cpu_cycle_ticks),
            nec_irq: submapper == 4,
            fixed_mirroring: submapper == 2,
            bank_select: 0,
            registers: [0; 8],
            latch: 0,
            counter: 0,
            reload: false,
            irq_enabled: false,
            a12: 0,
            a12_ready: 0,
            irq: false,
        }
    }
}

/// The MMC3's overridable methods. Default bodies: the `base_*` functions.
pub trait Mmc3Like {
    fn mmc3(&self) -> &Mmc3;
    fn mmc3_mut(&mut self) -> &mut Mmc3;

    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.mmc3_mut().b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.mmc3_mut().b.map_chr(window, size_kib, bank);
    }
    fn update(&mut self) {
        base_update(self);
    }
    fn ram_protect(&mut self, value: u8) {
        base_ram_protect(self.mmc3_mut(), value);
    }
    fn clock_counter(&mut self) {
        base_clock_counter(self.mmc3_mut());
    }
}

/// `MMC3._update`: the PRG and CHR windows from the bank registers.
pub fn base_update<T: Mmc3Like + ?Sized>(this: &mut T) {
    let m = this.mmc3();
    let r = m.registers;
    let bank_select = m.bank_select;
    let second_last = (m.b.prg.len() / 0x2000) as i64 - 2;
    if bank_select & 0x40 != 0 {
        this.map_prg(0, 8, second_last);
        this.map_prg(2, 8, r[6] as i64);
    } else {
        this.map_prg(0, 8, r[6] as i64);
        this.map_prg(2, 8, second_last);
    }
    this.map_prg(1, 8, r[7] as i64);
    this.map_prg(3, 8, second_last + 1);
    let low = if bank_select & 0x80 != 0 { 4 } else { 0 };
    this.map_chr(low, 2, (r[0] >> 1) as i64);
    this.map_chr(low + 2, 2, (r[1] >> 1) as i64);
    let high = low ^ 4;
    for i in 0..4 {
        this.map_chr(high + i, 1, r[2 + i] as i64);
    }
}

/// `MMC3.ram_protect`: $A001 D7 enables PRG-RAM, D6 denies writes.
pub fn base_ram_protect(m: &mut Mmc3, value: u8) {
    m.b.prg_ram_enabled = !m.b.prg_ram.is_empty() && value & 0x80 != 0;
    m.b.prg_ram_writable = value & 0x40 == 0;
}

/// `MMC3.clock_counter`.
pub fn base_clock_counter(m: &mut Mmc3) {
    let was = m.counter;
    let reload = m.reload;
    if was == 0 || reload {
        m.counter = m.latch;
    } else {
        m.counter = was - 1;
    }
    m.reload = false;
    if m.counter == 0 && m.irq_enabled && (!m.nec_irq || was != 0 || reload) {
        m.irq = true;
    }
}

/// `MMC3.cpu_write`.
pub fn base_cpu_write<T: Mmc3Like + ?Sized>(this: &mut T, address: u16, value: u8, t: i64) {
    if address < 0x8000 {
        this.mmc3_mut().b.base_cpu_write(address, value, t);
        return;
    }
    let odd = address & 1 != 0;
    let region = address & 0xE000;
    if region == 0x8000 {
        let m = this.mmc3_mut();
        if odd {
            m.registers[(m.bank_select & 7) as usize] = value;
        } else {
            m.bank_select = value;
        }
        this.update();
    } else if region == 0xA000 {
        if odd {
            this.ram_protect(value);
        } else {
            let m = this.mmc3_mut();
            if !m.fixed_mirroring && !m.b.four_screen {
                m.b.set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
            }
        }
    } else if region == 0xC000 {
        let m = this.mmc3_mut();
        if odd {
            m.counter = 0;
            m.reload = true;
        } else {
            m.latch = value;
        }
    } else if odd {
        this.mmc3_mut().irq_enabled = true;
    } else {
        let m = this.mmc3_mut();
        m.irq_enabled = false;
        m.irq = false;
    }
}

/// `MMC3.A12_LOW_FALLS`: M2 falls A12 must stay low through before a rise
/// clocks the counter. TEST ROM: blargg's `2-details` and `4-scanline_timing`
/// fail with one or two falls on both revisions and every MMC3 judge passes from
/// three to sixteen (the reference's sweep of 2026-11-19 and its seeded faults,
/// its docs/claims.md); three is a CHOICE at that lower bound.
pub const A12_LOW_FALLS: i64 = 3;

/// `MMC3.ppu_address`: the A12 filter.
pub fn base_ppu_address<T: Mmc3Like + ?Sized>(this: &mut T, address: u16, t: i64) {
    let m = this.mmc3_mut();
    if address & 0x1000 != 0 {
        if m.a12 == 0 {
            m.a12 = 1;
            if t >= m.a12_ready {
                this.clock_counter();
            }
        }
    } else if m.a12 != 0 {
        m.a12 = 0;
        let ticks = m.b.cpu_cycle_ticks;
        m.a12_ready = (t.div_euclid(ticks) + A12_LOW_FALLS) * ticks;
    }
}

/// The `Cartridge` methods an MMC3 board takes from the base class:
/// `mmc3_cartridge!(Type; method, ...)` inside `impl Cartridge for Type`
/// writes each named one.
#[macro_export]
macro_rules! mmc3_cartridge {
    ($($method:ident),* $(,)?) => {
        fn banked(&self) -> Option<&$crate::cartridge::Banked> {
            Some(&$crate::cartridge::mmc3::Mmc3Like::mmc3(self).b)
        }
        fn banked_mut(&mut self) -> Option<&mut $crate::cartridge::Banked> {
            Some(&mut $crate::cartridge::mmc3::Mmc3Like::mmc3_mut(self).b)
        }
        fn watches_ppu_address(&self) -> bool {
            true
        }
        fn irq(&self) -> bool {
            $crate::cartridge::mmc3::Mmc3Like::mmc3(self).irq
        }
        $( $crate::mmc3_cartridge!(@ $method); )*
    };
    (@ cpu_write) => {
        fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
            $crate::cartridge::mmc3::base_cpu_write(self, address, value, t);
        }
    };
    (@ ppu_address) => {
        fn ppu_address(&mut self, address: u16, t: i64) {
            $crate::cartridge::mmc3::base_ppu_address(self, address, t);
        }
    };
}

// -- MMC3 itself, MC-ACC -------------------------------------------------------

/// Mapper 4 (submappers 0, 2, 4): the MMC3.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc3Board {
    pub m: Mmc3,
}

impl Mmc3Board {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc3Board {
        let mut board = Mmc3Board {
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        board
    }
}

impl Mmc3Like for Mmc3Board {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
}

impl Cartridge for Mmc3Board {
    mmc3_cartridge!(cpu_write, ppu_address);
}

/// Submapper 4.3: MC-ACC, clocked on every eighth fall of A12, unfiltered.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct McAcc {
    pub m: Mmc3,
    pub prescaler: u8,
}

impl McAcc {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> McAcc {
        let mut board = McAcc {
            m: Mmc3::new(image, cpu_cycle_ticks),
            prescaler: 0,
        };
        board.update();
        board
    }
}

impl Mmc3Like for McAcc {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
}

impl Cartridge for McAcc {
    mmc3_cartridge!(cpu_write);
    fn ppu_address(&mut self, address: u16, _t: i64) {
        if address & 0x1000 != 0 {
            self.m.a12 = 1;
        } else if self.m.a12 != 0 {
            self.m.a12 = 0;
            self.prescaler = (self.prescaler + 1) & 7;
            if self.prescaler == 0 {
                self.clock_counter();
            }
        }
    }
}

// -- MMC6 ----------------------------------------------------------------------

/// Submapper 4.1: MMC6, 1 KiB of PRG-RAM at $7000-$7FFF in two halves.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc6 {
    pub m: Mmc3,
    pub protect: u8,
}

impl Mmc6 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc6 {
        let mut board = Mmc6 {
            m: Mmc3::new(image, cpu_cycle_ticks),
            protect: 0,
        };
        board.update();
        board.m.b.prg_ram = vec![0; 0x400];
        board.protect = 0;
        board
    }
}

impl Mmc3Like for Mmc6 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn ram_protect(&mut self, value: u8) {
        if self.m.bank_select & 0x20 != 0 {
            self.protect = value;
        }
    }
    fn update(&mut self) {
        base_update(self);
        if self.m.bank_select & 0x20 == 0 {
            self.protect = 0;
        }
    }
}

impl Cartridge for Mmc6 {
    mmc3_cartridge!(ppu_address);

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        let b = &self.m.b;
        let a = address as usize;
        if address >= 0x8000 {
            return Some(b.prg[b.prg_offsets[(a >> 13) & 3] + (a & 0x1FFF)] as u16);
        }
        if address < 0x7000 {
            return None;
        }
        let protect = self.protect;
        if protect & 0xA0 == 0 {
            return None;
        }
        let high = address & 0x200 != 0;
        if protect & (if high { 0x80 } else { 0x20 }) == 0 {
            return Some(0);
        }
        Some(b.prg_ram[a & 0x3FF] as u16)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            base_cpu_write(self, address, value, t);
            return;
        }
        if address < 0x7000 {
            return;
        }
        let enables = (self.protect >> if address & 0x200 != 0 { 6 } else { 4 }) & 3;
        if enables == 3 {
            self.m.b.prg_ram[address as usize & 0x3FF] = value;
        }
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.m.b.battery {
            Some(self.m.b.prg_ram.clone())
        } else {
            None
        }
    }
}

// -- TxSROM ----------------------------------------------------------------------

/// Mapper 118 (TKSROM, TLSROM): CHR A17 drives CIRAM A10.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TxSrom {
    pub m: Mmc3,
    pub nametable_a10: [u8; 4],
}

impl TxSrom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> TxSrom {
        let mut board = TxSrom {
            m: Mmc3::new(image, cpu_cycle_ticks),
            nametable_a10: [0; 4],
        };
        board.update();
        board.m.fixed_mirroring = true;
        board
    }
}

impl Mmc3Like for TxSrom {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn update(&mut self) {
        base_update(self);
        let r = self.m.registers;
        let banks = if self.m.bank_select & 0x80 != 0 {
            [r[2], r[3], r[4], r[5]]
        } else {
            [r[0], r[0], r[1], r[1]]
        };
        self.nametable_a10 = banks.map(|bank| bank >> 7);
    }
}

impl Cartridge for TxSrom {
    mmc3_cartridge!(cpu_write, ppu_address);
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.nametable_a10[((address >> 10) & 3) as usize])
    }
}

// -- AA6023 ----------------------------------------------------------------------

/// Mapper 268 (submappers 0/1, 8/9) and 224: the AA6023's outer banks.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Aa6023 {
    pub m: Mmc3,
    pub outer_base: u16,
    pub wide: bool,
    pub outer: [u8; 6],
}

impl Aa6023 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Aa6023 {
        let submapper = image.board.submapper;
        let mut board = Aa6023 {
            outer_base: if submapper & 1 != 0 { 0x5000 } else { 0x6000 },
            wide: submapper < 8,
            outer: [0; 6],
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        board.m.nec_irq = true;
        board
    }

    fn prg_bank(&self, bank: i64) -> i64 {
        let o0 = self.outer[0] as i64;
        let o1 = self.outer[1] as i64;
        let mut mmc3: i64 = 0x0F;
        mmc3 |= if o0 & 0x40 != 0 { 0 } else { 0x10 };
        mmc3 |= if o1 & 0x80 != 0 { 0 } else { 0x20 };
        mmc3 |= if o1 & 0x40 != 0 { 0x40 } else { 0 };
        mmc3 |= if o1 & 0x20 != 0 { 0x80 } else { 0 };
        let mut offset = ((o0 & 7) << 4) | ((o1 & 0x10) << 3);
        if self.wide {
            offset |= (((o1 >> 2) & 1) << 8) | (((o1 >> 3) & 1) << 9) | (((o0 >> 4) & 3) << 10);
        }
        (bank & mmc3) | (offset & !mmc3)
    }
}

impl Mmc3Like for Aa6023 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let o1 = self.outer[1] as i64;
        let o3 = self.outer[3] as i64;
        let mut bank = self.prg_bank(bank);
        if o3 & 0x10 != 0 {
            bank &= !0x0F;
            if o1 & 0x02 != 0 {
                bank |= (((o3 >> 2) & 3) << 2) | (window as i64 & 3);
            } else {
                bank |= (((o3 >> 1) & 7) << 1) | (window as i64 & 1);
            }
        }
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let o0 = self.outer[0] as i64;
        let o3 = self.outer[3] as i64;
        for i in 0..size_kib {
            let mut one = bank * size_kib as i64 + i as i64;
            if o0 & 0x80 != 0 {
                one = (one & !0x80) | ((o0 & 8) << 4);
            }
            if o3 & 0x10 != 0 {
                one = (one & !0x7F)
                    | ((self.outer[2] as i64 & 0x0F) << 3)
                    | ((window + i) as i64 & 7);
            }
            self.m.b.map_chr(window + i, 1, one);
        }
    }
}

impl Cartridge for Aa6023 {
    mmc3_cartridge!(ppu_address);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let mut value = value;
        if self.outer_base <= address && (address as u32) < self.outer_base as u32 + 0x1000 {
            let register = (address & 7) as usize;
            let locked = self.outer[3] & 0x80 != 0 && self.outer[3] & 0x10 == 0;
            if register < 6 && (!locked || register == 2) {
                if register == 2 && self.outer[2] & 0x80 != 0 {
                    value = (self.outer[2] & 0xF0) | (value & 0x0F);
                }
                self.outer[register] = value;
                self.update();
            }
        }
        base_cpu_write(self, address, value, t);
    }
}

/// Mapper 224: Jncota's KT-008, the AA6023 as submapper 1.
pub fn mindkids_224(image: RomImage, cpu_cycle_ticks: i64) -> Aa6023 {
    let mut image = image;
    image.board.submapper = 1;
    Aa6023::new(image, cpu_cycle_ticks)
}

// -- the multicarts 37 and 47 ------------------------------------------------------

/// Mappers 37 and 47: Nintendo of Europe's MMC3 multicarts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc3Multicart {
    pub m: Mmc3,
    pub outer: u8,
    pub ram_control: u8,
    pub three_games: bool,
}

impl Mmc3Multicart {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc3Multicart {
        let three_games = image.board.mapper == 37;
        let mut board = Mmc3Multicart {
            m: Mmc3::new(image, cpu_cycle_ticks),
            outer: 0,
            ram_control: 0x80,
            three_games,
        };
        board.update();
        board
    }
}

impl Mmc3Like for Mmc3Multicart {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn ram_protect(&mut self, value: u8) {
        self.ram_control = value;
        base_ram_protect(&mut self.m, value);
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let q = self.outer as i64;
        let bank = if self.three_games {
            let a16 = (q & 3 == 3) || (q & 4 != 0 && bank & 8 != 0);
            (bank & 7) | ((a16 as i64) << 3) | ((q & 4) << 2)
        } else {
            (bank & 0x0F) | (q << 4)
        };
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        let high = (if self.three_games {
            self.outer >> 2
        } else {
            self.outer
        } & 1) as i64;
        for i in 0..size_kib {
            self.m.b.map_chr(
                window + i,
                1,
                ((bank * size_kib as i64 + i as i64) & 0x7F) | (high << 7),
            );
        }
    }
}

impl Cartridge for Mmc3Multicart {
    mmc3_cartridge!(ppu_address);
    fn m2_gap(&mut self, _t: i64) {
        if self.three_games {
            self.outer = 0;
            self.update();
        }
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            if self.ram_control & 0xC0 == 0x80 {
                self.outer = value & if self.three_games { 7 } else { 1 };
                self.update();
            }
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

// -- TQROM and NES-EVENT2 ------------------------------------------------------------

/// TQROM's fields (the reference's `TQROM` attributes).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TqFields {
    pub rom_size: usize,
    pub ram_window: [bool; 8],
}

/// `TQROM.map_chr`: D6 of a CHR bank picks the RAM after the ROM.
pub fn tqrom_map_chr(m: &mut Mmc3, tq: &mut TqFields, window: usize, size_kib: usize, bank: i64) {
    for i in 0..size_kib {
        let one = bank * size_kib as i64 + i as i64;
        let ram = one & 0x40 != 0;
        tq.ram_window[window + i] = ram;
        if ram {
            m.b.chr_offsets[window + i] = tq.rom_size + (one & 7) as usize * 0x400;
        } else {
            m.b.chr_offsets[window + i] =
                ((one & 0x3F) * 0x400).rem_euclid(tq.rom_size as i64) as usize;
        }
    }
}

/// `TQROM.ppu_write`.
pub fn tqrom_ppu_write(m: &mut Mmc3, tq: &TqFields, address: u16, value: u8, t: i64) {
    if address >= 0x2000 {
        m.b.base_ppu_write(address, value, t);
    } else if tq.ram_window[(address >> 10) as usize] {
        let window = (address >> 10) as usize;
        m.b.chr[m.b.chr_offsets[window] + (address as usize & 0x3FF)] = value;
    }
}

/// Mapper 119: TQROM, CHR-ROM and 8 KiB of CHR-RAM side by side.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TqRom {
    pub m: Mmc3,
    pub tq: TqFields,
}

impl TqRom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> TqRom {
        let chr = image.chr.clone();
        let mut board = TqRom {
            tq: TqFields {
                rom_size: image.chr.len(),
                ram_window: [false; 8],
            },
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        let mut ram = chr;
        ram.extend_from_slice(&[0; 0x2000]);
        board.m.b.chr = ram;
        board
    }
}

impl Mmc3Like for TqRom {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        tqrom_map_chr(&mut self.m, &mut self.tq, window, size_kib, bank);
    }
}

impl Cartridge for TqRom {
    mmc3_cartridge!(cpu_write, ppu_address);
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        tqrom_ppu_write(&mut self.m, &self.tq, address, value, t);
    }
}

/// Mapper 555: NES-EVENT2, MMC3, TQROM and a timer.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Event2 {
    pub m: Mmc3,
    pub tq: TqFields,
    pub tq_mode: bool,
    pub prg_base: i64,
    pub prg_mask: i64,
    pub chr_base: i64,
    pub timer: u64,
    pub timing: bool,
}

const EVENT2_DIP: u64 = 4;

impl Event2 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Event2 {
        let chr = image.chr.clone();
        let mut board = Event2 {
            tq: TqFields {
                rom_size: image.chr.len(),
                ram_window: [false; 8],
            },
            tq_mode: false,
            prg_base: 0,
            prg_mask: 7,
            chr_base: 0,
            timer: 0,
            timing: false,
            m: Mmc3::new(image, cpu_cycle_ticks),
        };
        board.update();
        let mut ram = chr;
        ram.extend_from_slice(&[0; 0x2000]);
        board.m.b.chr = ram;
        let size = board.m.b.prg_ram.len().max(0x4000);
        board.m.b.prg_ram = vec![0; size];
        board.m.b.prg_ram_mask = 0x1FFF;
        board
    }
}

impl Mmc3Like for Event2 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let bank = (bank & self.prg_mask) | self.prg_base;
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        if self.tq_mode {
            tqrom_map_chr(&mut self.m, &mut self.tq, window, size_kib, bank);
            return;
        }
        for i in 0..size_kib {
            let one = ((bank * size_kib as i64 + i as i64) & 0x7F) | self.chr_base;
            self.tq.ram_window[window + i] = false;
            self.m.b.chr_offsets[window + i] =
                (one * 0x400).rem_euclid(self.tq.rom_size as i64) as usize;
        }
    }
}

impl Cartridge for Event2 {
    mmc3_cartridge!(ppu_address);
    fn counts_m2(&self) -> bool {
        true
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        tqrom_ppu_write(&mut self.m, &self.tq, address, value, t);
    }
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5800..0x6000).contains(&address) {
            return Some(if self.timer >= (0x10 | EVENT2_DIP) << 25 {
                0x80
            } else {
                0
            });
        }
        if (0x5000..0x5800).contains(&address) {
            return Some(self.m.b.prg_ram[0x2000 + (address as usize & 0x7FF)] as u16);
        }
        if (0x6000..0x8000).contains(&address) {
            return Some(self.m.b.prg_ram[address as usize & 0x1FFF] as u16);
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x5800).contains(&address) {
            self.m.b.prg_ram[0x2000 + (address as usize & 0x7FF)] = value;
        } else if (0x6000..0x8000).contains(&address) {
            self.m.b.prg_ram[address as usize & 0x1FFF] = value;
        } else if (0x5800..0x5C00).contains(&address) {
            self.tq_mode = value & 6 == 2;
            self.prg_base = ((value & 4) as i64) << 3;
            self.prg_mask = (((value & 3) as i64) << 3) | 7;
            self.chr_base = self.prg_base << 2;
            self.timing = value & 8 != 0;
            if !self.timing {
                self.timer = 0;
            }
            self.update();
        } else {
            base_cpu_write(self, address, value, t);
        }
    }
    fn m2(&mut self, _t: i64) {
        if self.timing {
            self.timer += 1;
        }
    }
}

// -- Kǎshèng -------------------------------------------------------------------------

/// Mapper 115: Kǎshèng's MMC3 clone with an NROM override and outer banks.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kasheng {
    pub m: Mmc3,
    pub mode: u8,
    pub chr_outer: u8,
}

impl Kasheng {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kasheng {
        let mut board = Kasheng {
            m: Mmc3::new(image, cpu_cycle_ticks),
            mode: 0,
            chr_outer: 0,
        };
        board.update();
        board.m.b.prg_ram_enabled = false;
        board
    }
}

impl Mmc3Like for Kasheng {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        let mode = self.mode as i64;
        let a18 = (mode >> 6) & 1;
        let bank = if mode & 0x80 != 0 {
            let mut sixteen = mode & 0x0F;
            if mode & 0x20 != 0 {
                sixteen = (sixteen & !1) | (window as i64 >> 1);
            }
            ((a18 << 5) | (sixteen << 1)) | (window as i64 & 1)
        } else {
            (bank & 0x1F) | (a18 << 5)
        };
        self.m.b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        for i in 0..size_kib {
            self.m.b.map_chr(
                window + i,
                1,
                (bank * size_kib as i64 + i as i64) | ((self.chr_outer as i64) << 8),
            );
        }
    }
}

impl Cartridge for Kasheng {
    mmc3_cartridge!(ppu_address);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return if address & 0xE003 == 0x6002 {
                Some(0xF8 << 8)
            } else {
                None
            };
        }
        self.m.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            let register = address & 0xE003;
            if register == 0x6000 {
                self.mode = value;
            } else if register == 0x6001 {
                self.chr_outer = value & 1;
            }
            self.update();
            return;
        }
        base_cpu_write(self, address, value, t);
    }
}

/// Mapper 4's board by submapper.
pub fn mmc3_for(image: RomImage, cpu_cycle_ticks: i64) -> Box<dyn Cartridge> {
    match image.board.submapper {
        1 => Box::new(Mmc6::new(image, cpu_cycle_ticks)),
        3 => Box::new(McAcc::new(image, cpu_cycle_ticks)),
        _ => Box::new(Mmc3Board::new(image, cpu_cycle_ticks)),
    }
}
