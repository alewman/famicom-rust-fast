// SPDX-License-Identifier: MIT
//! Multicarts that switch whole mapper ASICs: 394, 351 and Mindkids' FPGA
//! (446) (`multiasic.py`). Each from its NESdev wiki page; no judge ROM
//! exists, the claims rest on sweeps against Mesen2, a detector, and on
//! looking (docs/claims.md).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 351: `Techline351`
//! - 394: `HSK007`
//! - 446: `Mindkids446`
//!
//! Differences forced by Rust:
//!
//! - **The host.** The reference's `Translated` ASICs call back into their
//!   host board (`self.host.prg_bank(self, ...)`). Here the host's
//!   translating state is a [`Host`] value each ASIC holds a copy of; the
//!   host writes it into its ASICs ([`sync`](Hsk007::sync)) whenever that state
//!   changes, before any ASIC maps a bank, so every bank an ASIC maps sees
//!   the host as the reference's call would. The ASICs are the base classes'
//!   `*Like` traits with `map_prg`/`map_chr` overridden (`mmc3.rs`,
//!   `jycompany.rs`, `irem.rs`, `konami.rs`); the MMC1 is mmc1.rs's
//!   [`Kind::Translated`](super::mmc1::Kind::Translated).
//! - **Shared memory.** Where the reference hands several objects one
//!   `bytearray` (Techline's CHR-RAM to its three ASICs, Mindkids' CHR-RAM and
//!   PRG-RAM to each ASIC it makes), each Rust object owns a copy. Only the
//!   ASIC in charge can write it, so the host takes the copy back from that
//!   ASIC before handing the memory to another.

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::irem::{h3001_init, H3001Like, H3001};
use super::jycompany::{jy_update, JyCompany, JyLike};
use super::konami::{rewired, vrc24_update_prg, Vrc24, Vrc24Like};
use super::mmc1::{Kind, Mmc1};
use super::mmc3::{Mmc3, Mmc3Like};
use crate::{h3001_cartridge, jy_cartridge, mmc3_cartridge, vrc24_cartridge};

/// The host board's state its `prg_bank` and `chr_bank` read, as an ASIC sees it.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Host {
    /// `HSK007`; `jy` is `asic is self.jy`.
    Hsk007 {
        submapper: u8,
        mode_register: u8,
        outer: u8,
        jy: bool,
    },
    Techline351 {
        registers: [u8; 3],
    },
    Mindkids446 {
        submapper: u8,
        mask: u8,
        outer: u16,
        flags: u8,
        chr_outer: u8,
    },
}

impl Host {
    /// The host's `prg_bank(asic, window, bank)`.
    pub fn prg_bank(&self, window: usize, bank: i64) -> i64 {
        match *self {
            Host::Hsk007 {
                mode_register,
                outer,
                jy,
                ..
            } => {
                let high = hsk007_high_prg(mode_register);
                if jy {
                    return bank & 0x1F | high;
                }
                let outer = outer as i64;
                let a17 = if outer & 0x10 != 0 {
                    bank & 0x10
                } else {
                    (outer & 8) << 1
                };
                bank & 0x0F | a17 | high
            }
            Host::Techline351 { registers } => {
                let base8 = (registers[1] as i64 >> 2) * 2;
                let mode = registers[2] as i64;
                let size = (mode >> 2) & 3;
                let window = window as i64;
                if mode & 0x10 != 0 {
                    if size == 0 {
                        return (base8 & !3) | window;
                    }
                    return if size == 1 {
                        (base8 & !1) | window & 1
                    } else {
                        base8
                    };
                }
                let mask = if size == 0 { 0x1F } else { 0x0F };
                (base8 & !mask) | bank & mask
            }
            Host::Mindkids446 {
                submapper,
                mask,
                outer,
                ..
            } => {
                // The mask covers A20-A13: the outer bank's high byte always applies.
                let mask = mask as i64;
                let mask = if submapper == 2 {
                    !mask & 0xFFFF
                } else {
                    mask | 0xFF00
                };
                (bank & !mask & 0xFFFF) | (outer as i64 & mask)
            }
        }
    }

    /// The host's `chr_bank(asic, bank)`.
    pub fn chr_bank(&self, bank: i64) -> i64 {
        match *self {
            Host::Hsk007 {
                submapper,
                mode_register,
                outer,
                jy,
            } => {
                let high = hsk007_high_chr(submapper, mode_register);
                if jy {
                    return bank & 0xFF | high;
                }
                let outer = outer as i64;
                let a17 = if outer & 0x80 != 0 {
                    bank & 0x80
                } else {
                    (outer & 0x40) << 1
                };
                bank & 0x7F | a17 | high
            }
            Host::Techline351 { registers } => {
                let size = registers[2] >> 5;
                let mask: i64 = if size & 4 != 0 {
                    0x07
                } else if size & 2 != 0 {
                    0x7F
                } else if size & 1 != 0 {
                    0x1F
                } else {
                    0xFF
                };
                (((registers[0] as i64 >> 2) * 8) & !mask) | bank & mask
            }
            Host::Mindkids446 {
                flags, chr_outer, ..
            } => {
                let inner: i64 = match (flags >> 4) & 3 {
                    2 => 0x7F,
                    3 => 0x1F,
                    _ => 0xFF,
                };
                (bank & inner) | ((chr_outer as i64) << 3 & !inner)
            }
        }
    }
}

/// `Translated.map_prg`.
pub fn translate_prg(b: &mut Banked, host: &Host, window: usize, size_kib: usize, bank: i64) {
    let count = size_kib / 8;
    let len = b.prg.len() as i64;
    for i in 0..count {
        let eighth = host.prg_bank(window + i, bank * count as i64 + i as i64);
        b.prg_offsets[window + i] = (eighth * 0x2000).rem_euclid(len) as usize;
    }
}

/// `Translated.map_chr`.
pub fn translate_chr(b: &mut Banked, host: &Host, window: usize, size_kib: usize, bank: i64) {
    let len = b.chr.len() as i64;
    for i in 0..size_kib {
        let one = host.chr_bank(bank * size_kib as i64 + i as i64);
        b.chr_offsets[window + i] = (one * 0x400).rem_euclid(len) as usize;
    }
}

// -- the hosted ASICs ------------------------------------------------------------

/// `HostMMC3`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct HostMmc3 {
    pub m: Mmc3,
    pub host: Host,
}

impl HostMmc3 {
    pub fn new(host: Host, image: RomImage, cpu_cycle_ticks: i64) -> HostMmc3 {
        let mut asic = HostMmc3 {
            m: Mmc3::new(image, cpu_cycle_ticks),
            host,
        };
        asic.update();
        asic
    }
}

impl Mmc3Like for HostMmc3 {
    fn mmc3(&self) -> &Mmc3 {
        &self.m
    }
    fn mmc3_mut(&mut self) -> &mut Mmc3 {
        &mut self.m
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_prg(&mut self.m.b, &self.host, window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_chr(&mut self.m.b, &self.host, window, size_kib, bank);
    }
}

impl Cartridge for HostMmc3 {
    mmc3_cartridge!(cpu_write, ppu_address);
}

/// `HostJY`: `_map_chr` is `Translated.map_chr`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct HostJy {
    pub jy: JyCompany,
    pub host: Host,
}

impl HostJy {
    pub fn new(host: Host, image: RomImage, cpu_cycle_ticks: i64) -> HostJy {
        let mut asic = HostJy {
            jy: JyCompany::state(image, cpu_cycle_ticks),
            host,
        };
        jy_update(&mut asic);
        asic
    }
}

impl JyLike for HostJy {
    fn jy(&self) -> &JyCompany {
        &self.jy
    }
    fn jy_mut(&mut self) -> &mut JyCompany {
        &mut self.jy
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_prg(&mut self.jy.b, &self.host, window, size_kib, bank);
    }
    fn map_chr_outer(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_chr(&mut self.jy.b, &self.host, window, size_kib, bank);
    }
}

impl Cartridge for HostJy {
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

/// `HostH3001`: remembers each PRG window's bank, to re-map them on a refresh.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct HostH3001 {
    pub h: H3001,
    pub host: Host,
    pub prg_banks: [Option<i64>; 4],
}

impl HostH3001 {
    pub fn new(host: Host, image: RomImage, cpu_cycle_ticks: i64) -> HostH3001 {
        let mut asic = HostH3001 {
            h: H3001::state(image, cpu_cycle_ticks),
            host,
            prg_banks: [None; 4],
        };
        h3001_init(&mut asic);
        asic
    }

    fn refresh(&mut self) {
        for window in 0..4 {
            if let Some(bank) = self.prg_banks[window] {
                self.map_prg(window, 8, bank);
            }
        }
    }
}

impl H3001Like for HostH3001 {
    fn h3001(&self) -> &H3001 {
        &self.h
    }
    fn h3001_mut(&mut self) -> &mut H3001 {
        &mut self.h
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.prg_banks[window] = Some(bank);
        translate_prg(&mut self.h.b, &self.host, window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_chr(&mut self.h.b, &self.host, window, size_kib, bank);
    }
}

impl Cartridge for HostH3001 {
    h3001_cartridge!(cpu_write, m2);
}

/// `HostVRC4`: the XB's VRC4: A0, A1 from CPU A2, A3, exchanged when A11 is set.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct HostVrc4 {
    pub v: Vrc24,
    pub host: Host,
}

impl HostVrc4 {
    pub fn new(host: Host, image: RomImage, cpu_cycle_ticks: i64) -> HostVrc4 {
        let mut asic = HostVrc4 {
            v: Vrc24::state(rewired(image, 21, 1), cpu_cycle_ticks),
            host,
        };
        asic.update_prg();
        asic
    }

    fn refresh(&mut self) {
        self.update_prg();
        for window in 0..8 {
            let bank = self.v.chr_registers[window] as i64;
            self.map_chr(window, 1, bank);
        }
    }
}

impl Vrc24Like for HostVrc4 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_prg(&mut self.v.b, &self.host, window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        translate_chr(&mut self.v.b, &self.host, window, size_kib, bank);
    }
    fn register(&self, address: u16) -> u8 {
        let low = ((address >> 2) & 1) as u8;
        let high = ((address >> 3) & 1) as u8;
        if address & 0x800 != 0 {
            high | low << 1
        } else {
            low | high << 1
        }
    }
    fn update_prg(&mut self) {
        vrc24_update_prg(self);
    }
}

impl Cartridge for HostVrc4 {
    vrc24_cartridge!(cpu_read, cpu_write, m2);
}

/// Any hosted ASIC (Mindkids' FPGA makes them as the game asks).
#[derive(serde::Serialize, serde::Deserialize)]
pub enum Asic {
    Mmc3(HostMmc3),
    Mmc1(Mmc1),
    Jy(HostJy),
    H3001(HostH3001),
    Vrc4(HostVrc4),
}

impl Asic {
    fn cart(&mut self) -> &mut dyn Cartridge {
        match self {
            Asic::Mmc3(a) => a,
            Asic::Mmc1(a) => a,
            Asic::Jy(a) => a,
            Asic::H3001(a) => a,
            Asic::Vrc4(a) => a,
        }
    }

    fn b(&mut self) -> &mut Banked {
        self.cart().banked_mut().expect("every ASIC is Banked")
    }

    fn set_host(&mut self, host: Host) {
        match self {
            Asic::Mmc3(a) => a.host = host,
            Asic::Mmc1(a) => a.kind = Kind::Translated(host),
            Asic::Jy(a) => a.host = host,
            Asic::H3001(a) => a.host = host,
            Asic::Vrc4(a) => a.host = host,
        }
    }

    /// The ASIC's `refresh`.
    fn refresh(&mut self) {
        match self {
            Asic::Mmc3(a) => a.update(),
            Asic::Mmc1(a) => a.update(),
            Asic::Jy(a) => jy_update(a),
            Asic::H3001(a) => a.refresh(),
            Asic::Vrc4(a) => a.refresh(),
        }
    }
}

// -- the composite boards ----------------------------------------------------------

/// `Composite`: the board's own `Banked`, the ASIC in charge (or none), the
/// /IRQ it last copied from it.
trait Composite {
    fn parts(&mut self) -> (Option<&mut Asic>, &mut Banked, &mut bool);
}

/// `Composite.forward_write`.
fn forward_write<T: Composite>(this: &mut T, address: u16, value: u8, t: i64) {
    if let (Some(asic), _, irq) = this.parts() {
        let cart = asic.cart();
        cart.cpu_write(address, value, t);
        *irq = cart.irq();
    }
}

/// The `Cartridge` methods `Composite` defines.
macro_rules! composite_cartridge {
    () => {
        fn banked(&self) -> Option<&Banked> {
            Some(&self.b)
        }
        fn banked_mut(&mut self) -> Option<&mut Banked> {
            Some(&mut self.b)
        }
        fn counts_m2(&self) -> bool {
            true
        }
        fn watches_ppu_address(&self) -> bool {
            true
        }
        fn irq(&self) -> bool {
            self.irq
        }
        fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
            match self.parts() {
                (Some(asic), _, _) => asic.cart().cpu_read(address, t),
                (None, b, _) => b.base_cpu_read(address, t),
            }
        }
        fn ciram_a10(&mut self, address: u16) -> Option<u8> {
            match self.parts() {
                (Some(asic), _, _) => asic.cart().ciram_a10(address),
                (None, b, _) => b.base_ciram_a10(address),
            }
        }
        fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
            match self.parts() {
                (Some(asic), _, _) => asic.cart().ppu_read(address, t),
                (None, b, _) => b.base_ppu_read(address, t),
            }
        }
        fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
            match self.parts() {
                (Some(asic), _, _) => asic.cart().ppu_write(address, value, t),
                (None, b, _) => b.base_ppu_write(address, value, t),
            }
        }
        fn ppu_address(&mut self, address: u16, t: i64) {
            if let (Some(asic), _, irq) = self.parts() {
                let cart = asic.cart();
                cart.ppu_address(address, t);
                *irq = cart.irq();
            }
        }
        fn m2(&mut self, t: i64) {
            if let (Some(asic), _, irq) = self.parts() {
                let cart = asic.cart();
                cart.m2(t);
                *irq = cart.irq();
            }
        }
    };
}

fn hsk007_high_prg(mode_register: u8) -> i64 {
    ((mode_register & 3) as i64) << 5
}

fn hsk007_high_chr(submapper: u8, r: u8) -> i64 {
    let a18 = if submapper != 0 { (r >> 2) & 1 } else { r & 1 };
    ((a18 | (r & 2)) as i64) << 8
}

/// Mapper 394: Realtec's HSK007: BNROM, an MMC3 clone or a J.Y. Company ASIC
/// (NES_2.0_Mapper_394.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hsk007 {
    pub b: Banked,
    pub submapper: u8,
    pub mode_register: u8,
    pub outer: u8,
    /// The ASICs: `self.mmc3`, `self.jy`.
    pub asics: [Asic; 2],
    /// `self.asic`: None, or 0 (the MMC3) or 1 (the J.Y.).
    pub active: Option<usize>,
    pub irq: bool,
}

impl Hsk007 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Hsk007 {
        let submapper = image.board.submapper;
        let b = Banked::new(image.clone(), cpu_cycle_ticks);
        let (mode_register, outer) = (0x0F, 0x90);
        // Each ASIC is made before `self.jy` names it: the J.Y.'s first
        // banks are translated as the MMC3's.
        let host = Host::Hsk007 {
            submapper,
            mode_register,
            outer,
            jy: false,
        };
        let mmc3 = HostMmc3::new(host, image.clone(), cpu_cycle_ticks);
        let jy = HostJy::new(host, image, cpu_cycle_ticks);
        let mut board = Hsk007 {
            b,
            submapper,
            mode_register,
            outer,
            asics: [Asic::Mmc3(mmc3), Asic::Jy(jy)],
            active: None,
            irq: false,
        };
        board.sync();
        board.select();
        board
    }

    /// Hand the ASICs the host's state.
    fn sync(&mut self) {
        for (i, asic) in self.asics.iter_mut().enumerate() {
            asic.set_host(Host::Hsk007 {
                submapper: self.submapper,
                mode_register: self.mode_register,
                outer: self.outer,
                jy: i == 1,
            });
        }
    }

    fn select(&mut self) {
        let mode = (self.mode_register >> 3) & 3;
        self.active = match mode {
            0 => None,
            1 => Some(0),
            _ => Some(1),
        };
        if let Some(i) = self.active {
            self.asics[i].refresh();
            return;
        }
        let outer = self.outer as i64;
        let a17 = if outer & 0x10 != 0 {
            0
        } else {
            (outer >> 3) & 1
        };
        self.b.map_prg(
            0,
            32,
            (outer >> 1) & 3 | a17 << 2 | hsk007_high_prg(self.mode_register) >> 2,
        );
        self.b.map_chr(
            0,
            8,
            hsk007_high_chr(self.submapper, self.mode_register) >> 3,
        );
    }
}

impl Composite for Hsk007 {
    fn parts(&mut self) -> (Option<&mut Asic>, &mut Banked, &mut bool) {
        let asic = match self.active {
            Some(i) => Some(&mut self.asics[i]),
            None => None,
        };
        (asic, &mut self.b, &mut self.irq)
    }
}

impl Cartridge for Hsk007 {
    composite_cartridge!();
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF00F == 0x5009 {
            self.mode_register = value;
            self.sync();
            self.select();
        } else if address & 0xF00F == 0x500B {
            self.outer = value;
            self.sync();
            self.select();
        } else {
            forward_write(self, address, value, t);
        }
    }
}

/// Mapper 351: Techline's XB multicarts: an MMC3, an MMC1 or a VRC4 on one
/// die (NES_2.0_Mapper_351.wiki; the reading of $5002 is the reference's).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Techline351 {
    pub b: Banked,
    #[serde(with = "crate::state::rom_bytes::chr")]
    pub rom: Vec<u8>,
    /// The CHR-RAM; while `shared`, the ASIC in charge holds its live copy.
    pub chr_ram: Vec<u8>,
    pub shared: bool,
    pub registers: [u8; 3],
    /// `self.asic`, as `registers[0] & 3` was at the last `_select`.
    pub selected: u8,
    /// `self.asics`: 0 (and 1) the MMC3, 2 the MMC1, 3 the VRC4.
    pub mmc3: Asic,
    pub mmc1: Asic,
    pub vrc4: Asic,
    pub irq: bool,
}

impl Techline351 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Techline351 {
        let rom = image.chr.clone();
        let b = Banked::new(image.clone(), cpu_cycle_ticks);
        let registers = [0, 0, 0];
        let host = Host::Techline351 { registers };
        let t = cpu_cycle_ticks;
        let mut board = Techline351 {
            b,
            rom,
            chr_ram: vec![0; 0x2000],
            shared: false,
            registers,
            selected: 0,
            mmc3: Asic::Mmc3(HostMmc3::new(host, image.clone(), t)),
            mmc1: Asic::Mmc1(Mmc1::translated(image.clone(), t, host)),
            vrc4: Asic::Vrc4(HostVrc4::new(host, image, t)),
            irq: false,
        };
        board.select();
        board
    }

    fn asic(&mut self) -> &mut Asic {
        match self.selected {
            0 | 1 => &mut self.mmc3,
            2 => &mut self.mmc1,
            _ => &mut self.vrc4,
        }
    }

    /// `_select`, after handing the ASICs the host's state.
    fn select(&mut self) {
        let host = Host::Techline351 {
            registers: self.registers,
        };
        if self.shared {
            // The ASIC that was in charge wrote the CHR-RAM last.
            self.chr_ram = self.asic().b().chr.clone();
        }
        let ram = self.registers[2] & 1 != 0;
        let prg = if ram {
            [self.b.prg.as_slice(), self.rom.as_slice()].concat()
        } else {
            self.b.prg.clone()
        };
        let chr = if ram {
            self.chr_ram.clone()
        } else {
            self.rom.clone()
        };
        for asic in [&mut self.mmc3, &mut self.mmc1, &mut self.vrc4] {
            asic.set_host(host);
            let b = asic.b();
            b.prg = prg.clone();
            b.chr = chr.clone();
            b.chr_writable = ram;
        }
        self.shared = ram;
        self.selected = self.registers[0] & 3;
        self.asic().refresh();
    }
}

impl Composite for Techline351 {
    fn parts(&mut self) -> (Option<&mut Asic>, &mut Banked, &mut bool) {
        let asic = match self.selected {
            0 | 1 => &mut self.mmc3,
            2 => &mut self.mmc1,
            _ => &mut self.vrc4,
        };
        (Some(asic), &mut self.b, &mut self.irq)
    }
}

impl Cartridge for Techline351 {
    composite_cartridge!();
    fn m2_gap(&mut self, _t: i64) {
        self.registers = [0, 0, 0];
        self.select();
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let register = address & 0xF003;
        if (0x5000..=0x5002).contains(&register) {
            self.registers[(address & 3) as usize] = value;
            self.select();
        } else if address == 0x4025 && self.registers[2] & 0x10 != 0 {
            let mode = if value & 8 != 0 { HORIZONTAL } else { VERTICAL };
            self.asic().b().set_mirroring(mode);
        } else {
            forward_write(self, address, value, t);
        }
    }
}

/// Mapper 446's (submapper, $5000) that are MMC1 and MMC3 boards.
const FPGA_MMC1: [(u8, u8); 7] = [(0, 1), (1, 5), (1, 6), (1, 7), (2, 5), (2, 6), (2, 7)];
const FPGA_MMC3: [(u8, u8); 4] = [(1, 1), (2, 1), (2, 9), (2, 0x0D)];

/// Mapper 446: Mindkids' SMD172B_FPGA: an FPGA that becomes the mapper the
/// game asks for (NES_2.0_Mapper_446.wiki; only the MMC1 boards, plain MMC3
/// boards and H3001, the ones the dumps ask for; the state before a mapper
/// is chosen is the games', C70).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mindkids446 {
    /// The board's own: `self.chr` is the 256 KiB of CHR-RAM (`self.chr_ram`);
    /// while an ASIC is in charge, it holds the live copy of that and of the
    /// PRG-RAM.
    pub b: Banked,
    pub submapper: u8,
    pub outer: u16,
    pub mask: u8,
    pub flags: u8,
    pub protect: u8,
    pub chr_outer: u8,
    pub locked: bool,
    pub asic: Option<Asic>,
    pub irq: bool,
}

impl Mindkids446 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mindkids446 {
        let submapper = image.board.submapper;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x40000];
        b.chr_writable = true;
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        let mut board = Mindkids446 {
            b,
            submapper,
            outer: 0,
            mask: 0,
            flags: 0,
            protect: 0,
            chr_outer: 0,
            locked: false,
            asic: None,
            irq: false,
        };
        board.map_outer();
        board
    }

    fn host(&self) -> Host {
        Host::Mindkids446 {
            submapper: self.submapper,
            mask: self.mask,
            outer: self.outer,
            flags: self.flags,
            chr_outer: self.chr_outer,
        }
    }

    fn map_outer(&mut self) {
        self.b.map_prg(0, 8, self.outer as i64);
        self.b.map_prg(1, 8, self.outer as i64 + 1);
    }

    fn choose(&mut self, value: u8) {
        let key = (self.submapper, value & 0x1F);
        let image = self.b.image.clone();
        let t = self.b.cpu_cycle_ticks;
        let host = self.host();
        let asic = if FPGA_MMC1.contains(&key) {
            let mut small = image;
            small.prg.truncate(0x40000);
            Asic::Mmc1(Mmc1::translated(small, t, host))
        } else if self.submapper == 3 && key.1 == 1 {
            Asic::H3001(HostH3001::new(host, image, t))
        } else if FPGA_MMC3.contains(&key) {
            Asic::Mmc3(HostMmc3::new(host, image, t))
        } else {
            return;
        };
        if let Some(old) = &mut self.asic {
            // The memory the old ASIC shared with the board.
            let old = old.b();
            self.b.chr = old.chr.clone();
            if !self.b.prg_ram.is_empty() {
                self.b.prg_ram = old.prg_ram.clone();
            }
        }
        let mut asic = asic;
        let b = asic.b();
        b.prg = self.b.prg.clone();
        b.chr = self.b.chr.clone();
        b.chr_writable = self.protect == 0;
        if !self.b.prg_ram.is_empty() {
            b.prg_ram = self.b.prg_ram.clone();
        }
        b.set_mirroring(if self.flags & 1 != 0 {
            VERTICAL
        } else {
            HORIZONTAL
        });
        asic.refresh();
        self.asic = Some(asic);
    }
}

impl Composite for Mindkids446 {
    fn parts(&mut self) -> (Option<&mut Asic>, &mut Banked, &mut bool) {
        (self.asic.as_mut(), &mut self.b, &mut self.irq)
    }
}

impl Cartridge for Mindkids446 {
    composite_cartridge!();
    fn m2_gap(&mut self, _t: i64) {
        self.locked = false; // the page: locked until reset; the chosen mapper stays
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x5007).contains(&address) && !self.locked {
            match address & 7 {
                0 => {
                    self.locked = value & 0x80 != 0;
                    self.choose(value);
                }
                1 => {
                    self.outer = (self.outer & 0xFF00) | value as u16;
                    if self.asic.is_none() {
                        self.map_outer();
                    }
                }
                2 => {
                    self.outer = (self.outer & 0x00FF) | (value as u16) << 8;
                    if self.asic.is_none() {
                        self.map_outer();
                    }
                }
                3 => self.mask = value,
                4 => {
                    self.flags = value;
                    if self.asic.is_none() {
                        self.b
                            .set_mirroring(if value & 1 != 0 { VERTICAL } else { HORIZONTAL });
                    }
                }
                5 => {
                    self.protect = value & 4;
                    self.b.chr_writable = self.protect == 0;
                }
                _ => {
                    self.chr_outer = value & 0x1F;
                    self.b.map_chr(0, 8, self.chr_outer as i64);
                }
            }
            let host = self.host();
            let writable = self.protect == 0;
            if let Some(asic) = &mut self.asic {
                asic.set_host(host);
                asic.b().chr_writable = writable;
                asic.refresh();
            }
        } else if self.asic.is_some() {
            forward_write(self, address, value, t);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
    fn battery_ram(&self) -> Option<Vec<u8>> {
        // The board's PRG-RAM, whose live copy an ASIC in charge holds.
        if !self.b.battery {
            return None;
        }
        match &self.asic {
            Some(asic) if !self.b.prg_ram.is_empty() => Some(asic_banked(asic).prg_ram.clone()),
            _ => Some(self.b.prg_ram.clone()),
        }
    }
}

fn asic_banked(asic: &Asic) -> &Banked {
    match asic {
        Asic::Mmc3(a) => &a.m.b,
        Asic::Mmc1(a) => &a.b,
        Asic::Jy(a) => &a.jy.b,
        Asic::H3001(a) => &a.h.b,
        Asic::Vrc4(a) => &a.v.b,
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
        351 => Box::new(Techline351::new(image, t)),
        394 => Box::new(Hsk007::new(image, t)),
        446 => Box::new(Mindkids446::new(image, t)),
        _ => return Err(image),
    })
}
