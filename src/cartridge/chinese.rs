// SPDX-License-Identifier: MIT
//! Chinese, Taiwanese and Korean discrete-logic and simple ASIC boards, each
//! from its NESdev wiki page (`chinese.py`).
//!
//! No judge ROM exists for any of them; the claims rest on sweeps against
//! Mesen2, a detector, or on looking where it refuses them (docs/mappers.md).
//! The TMS5220-compatible speech chips some educational cartridges mount are
//! not modelled: their status reads are open bus.
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 14: `SL1632`
//! - 27: `MiHunChe27`
//! - 107: `MagicDragon107`
//! - 108: `FDSConversion108`
//! - 112: `Asder112`
//! - 116: `Huang116`
//! - 117: `FutureMedia117`
//! - 156: `Daou156`
//! - 177: `Hengge`
//! - 178: `Education178`
//! - 190: `Zemina190`
//! - 193: `TC112`
//! - 240: `Latch240`
//! - 241: `Hengge`
//! - 244: `Decathlon244`
//! - 246: `Fengshenbang246`
//! - 328: `RT01`
//! - 330: `Sangokushi330`
//! - 417: `FineStudio417`
//! - 515: `Noraebang515`
//! - 518: `Dance2000`
//! - 551: `Jncota551`
//!
//! Subclasses: `Jncota551` (of `Education178`, overriding `_update`) is a
//! flag of [`Education178`]; `SL1632` (of `Huang116`, overriding `active`,
//! `cpu_write` and `ppu_read`) one of [`Huang116`]; `HuangMMC1` is mmc1.rs's
//! `Kind::Huang`.

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use super::konami::{rewired, Vrc24};
use super::mmc1::Mmc1;
use super::mmc3::Mmc3Board;
use crate::banked_field;

// -- Hengge --------------------------------------------------------------------------

/// Mappers 241 (BxROM with WRAM) and 177 (Hénggé Diànzǐ's, with mirroring)
/// (INES_Mapper_241.wiki, _177.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hengge {
    pub b: Banked,
    pub mirroring_control: bool,
    pub chr_ram_mode: bool,
    pub latch: u8,
}

impl Hengge {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Hengge {
        let mirroring_control = image.board.mapper == 177;
        let chr_ram_mode = mirroring_control && image.board.submapper == 1;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        if chr_ram_mode && b.chr.len() < 0x4000 {
            b.chr = vec![0; 0x4000];
        }
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        if chr_ram_mode {
            b.map_chr(4, 4, 3);
        }
        Hengge {
            b,
            mirroring_control,
            chr_ram_mode,
            latch: 0,
        }
    }
}

impl Cartridge for Hengge {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.latch = value;
        if self.mirroring_control {
            self.b.map_prg(0, 32, (value & 0x1F) as i64);
            self.b.prg_ram_offset = ((value as usize >> 6) * 0x2000) % self.b.prg_ram.len().max(1);
            if !self.chr_ram_mode {
                self.b.set_mirroring(if value & 0x20 != 0 {
                    HORIZONTAL
                } else {
                    VERTICAL
                });
            }
        } else {
            self.b.map_prg(0, 32, value as i64);
        }
    }
    fn ppu_address(&mut self, address: u16, _t: i64) {
        if self.chr_ram_mode && address & 0x3000 == 0x2000 {
            self.b.map_chr(0, 4, ((address >> 8) & 3) as i64);
        }
    }
}

// -- Education178, Jncota551 ------------------------------------------------------------

/// Mapper 178: the educational-computer and Waixing FS305 board
/// (INES_Mapper_178.wiki). Also mapper 551 (`Jncota551`, `jncota`): Jncota's
/// re-releases on a variant of the board (NES_2.0_Mapper_551.wiki): the
/// mirroring hard-wired, $4803 the 8 KiB CHR-ROM bank.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Education178 {
    pub b: Banked,
    pub jncota: bool,
    pub solder_pads: bool,
    pub pads_shown: bool,
    pub regs: [u8; 4],
}

impl Education178 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64, jncota: bool) -> Education178 {
        let solder_pads = image.board.submapper == 3;
        let mut e = Education178 {
            b: Banked::new(image, cpu_cycle_ticks),
            jncota,
            solder_pads,
            pads_shown: false,
            regs: [0; 4],
        };
        e.b.map_chr(0, 8, 0);
        e.update();
        e
    }

    /// `_update`, through `Jncota551`'s.
    fn update(&mut self) {
        if self.jncota {
            let mirroring = self.b.mirroring;
            self.education_update();
            self.b.set_mirroring(mirroring);
            self.b.prg_ram_offset = 0;
            self.b.map_chr(0, 8, self.regs[3] as i64);
        } else {
            self.education_update();
        }
    }

    /// `Education178._update`.
    fn education_update(&mut self) {
        let (mode, low, high) = (
            self.regs[0] as i64,
            self.regs[1] as i64,
            self.regs[2] as i64,
        );
        self.b
            .set_mirroring(if mode & 1 != 0 { HORIZONTAL } else { VERTICAL });
        let base = high << 3 | low & 7; // 16 KiB banks
        let kind = (mode >> 1) & 3;
        if kind == 0 {
            self.b.map_prg(0, 32, base >> 1);
        } else if kind == 2 {
            self.b.map_prg(0, 16, base);
            self.b.map_prg(2, 16, base);
        } else {
            self.b.map_prg(0, 16, base);
            let fixed = if kind == 1 { 7 } else { 6 | low & 1 };
            self.b.map_prg(2, 16, high << 3 | fixed);
        }
        if !self.b.prg_ram.is_empty() {
            self.b.prg_ram_offset = (self.regs[3] as usize * 0x2000) % self.b.prg_ram.len();
        }
    }
}

impl Cartridge for Education178 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.pads_shown {
            return Some(0);
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x4800..=0x4FFF).contains(&address) {
            self.regs[(address & 3) as usize] = value;
            self.update();
            return;
        }
        if self.solder_pads && (0x6000..0x8000).contains(&address) {
            self.pads_shown = value & 1 != 0;
        }
        self.b.base_cpu_write(address, value, t);
    }
}

// -- Asder112 ---------------------------------------------------------------------------

/// Mapper 112: Asder's Namco 108-like board with its own register layout
/// (INES_Mapper_112.wiki; power-on PPU A10 whatever the header says).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Asder112 {
    pub b: Banked,
    pub select: u8,
}

impl Asder112 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Asder112 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(2, 8, last - 1);
        b.map_prg(3, 8, last);
        b.set_mirroring(VERTICAL);
        Asder112 { b, select: 0 }
    }
}

impl Cartridge for Asder112 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let register = address & 0xE001;
        if register == 0x8000 {
            self.select = value & 7;
        } else if register == 0xA000 {
            let index = self.select as usize;
            if index < 2 {
                self.b.map_prg(index, 8, value as i64);
            } else if index < 4 {
                self.b.map_chr((index - 2) * 2, 2, (value >> 1) as i64);
            } else {
                self.b.map_chr(index, 1, value as i64);
            }
        } else if register == 0xE000 {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

// -- Huang116, SL1632 -------------------------------------------------------------------

/// Mapper 116: Gouder's SOMARI-P board, whose Huang ASIC acts as a VRC2, an
/// MMC3 or an MMC1 (INES_Mapper_116.wiki): three mappers over the one image,
/// the active one answering. Also mapper 14 (`SL1632`, `sl1632`): Gouder's
/// SL-1632, the Huang-1 as VRC2b or MMC3, its supervisor at $A131
/// (INES_Mapper_014.wiki, the mirroring from Nestopia's source as the page says).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Huang116 {
    pub b: Banked,
    pub sl1632: bool,
    pub submapper: u8,
    pub vrc2: Vrc24,
    pub mmc3: Mmc3Board,
    pub mmc1: Mmc1,
    pub supervisor: u8,
    pub irq: bool,
}

impl Huang116 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64, sl1632: bool) -> Huang116 {
        let t = cpu_cycle_ticks;
        let submapper = image.board.submapper;
        let mut vrc2 = Vrc24::new(rewired(image.clone(), 23, 3), t);
        vrc2.chr_registers = [0xFF; 8];
        for bank in 0..8 {
            vrc2.b.map_chr(bank, 1, 0xFF);
        }
        let mmc3 = Mmc3Board::new(image.clone(), t);
        let mmc1 = if submapper == 2 {
            Mmc1::huang(image.clone(), t)
        } else {
            Mmc1::new(image.clone(), t)
        };
        Huang116 {
            b: Banked::new(image, t),
            sl1632,
            submapper,
            vrc2,
            mmc3,
            mmc1,
            supervisor: if sl1632 { 0 } else { 1 },
            irq: false,
        }
    }

    /// Which chip `active` is: 0 the VRC2, 1 the MMC3, 2 the MMC1.
    fn active_index(&self) -> u8 {
        if self.sl1632 {
            return if self.supervisor & 2 != 0 { 1 } else { 0 };
        }
        match self.supervisor & 3 {
            0 => 0,
            1 => 1,
            _ => 2,
        }
    }

    fn active(&mut self) -> &mut dyn Cartridge {
        match self.active_index() {
            0 => &mut self.vrc2,
            1 => &mut self.mmc3,
            _ => &mut self.mmc1,
        }
    }

    fn active_banked(&self) -> &Banked {
        match self.active_index() {
            0 => &self.vrc2.b,
            1 => &self.mmc3.m.b,
            _ => &self.mmc1.b,
        }
    }
}

impl Cartridge for Huang116 {
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
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.active().cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.sl1632 {
            if address == 0xA131 {
                self.supervisor = value;
            }
        } else if address & 0xE100 == 0x4100 {
            let was_mmc1 = self.supervisor & 2 != 0;
            self.supervisor = value;
            if value & 2 != 0 && !was_mmc1 && self.submapper != 1 {
                self.mmc1.cpu_write(0x8000, 0x80, t);
            }
            return;
        }
        self.active().cpu_write(address, value, t);
        self.irq = self.mmc3.irq();
    }
    fn m2(&mut self, _t: i64) {
        self.irq = self.mmc3.irq();
    }
    fn ppu_address(&mut self, address: u16, t: i64) {
        if self.active_index() == 1 {
            self.mmc3.ppu_address(address, t);
            self.irq = self.mmc3.irq();
        }
    }
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        self.active().ciram_a10(address)
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let s = self.supervisor as usize;
            let a18 = if !self.sl1632 {
                (s >> 2) & 1
            } else if address < 0x1000 {
                (s >> 3) & 1
            } else if address < 0x1800 {
                (s >> 5) & 1
            } else {
                s >> 7
            };
            let chip = self.active_banked();
            let offset = chip.chr_offsets[(address >> 10) as usize] + (address as usize & 0x3FF);
            return Some(chip.chr[(offset + a18 * 0x40000) % chip.chr.len()]);
        }
        self.active().ppu_read(address, t)
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        self.active().ppu_write(address, value, t);
    }
}

// -- FDSConversion108 -------------------------------------------------------------------

/// Mapper 108: Whirlwind Manu's and others' FDS-to-cartridge conversions
/// (INES_Mapper_108.wiki, its disambiguation without a submapper).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FdsConversion108 {
    pub b: Banked,
    pub submapper: u8,
    pub bank6000: i64,
}

impl FdsConversion108 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> FdsConversion108 {
        let mut sub = image.board.submapper;
        if sub == 0 {
            sub = if image.chr.is_empty() {
                if image.board.mirroring == 'H' {
                    1
                } else {
                    3
                }
            } else if image.chr.len() > 0x4000 {
                2
            } else {
                4
            };
        }
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last32 = (b.prg.len() / 0x8000) as i64 - 1;
        b.map_prg(0, 32, last32);
        b.map_chr(0, 8, 0);
        let bank6000 = (b.prg.len() / 0x2000) as i64 - 1;
        FdsConversion108 {
            b,
            submapper: sub,
            bank6000,
        }
    }
}

impl Cartridge for FdsConversion108 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            let prg = &self.b.prg;
            let offset =
                (self.bank6000 * 0x2000 + (address as i64 & 0x1FFF)).rem_euclid(prg.len() as i64);
            return Some(prg[offset as usize] as u16);
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let sub = self.submapper;
        if (sub == 1 && address >= 0xF000) || (sub == 3 && address >= 0x8000) {
            self.bank6000 = value as i64;
        } else if sub == 2 && address >= 0xE000 {
            self.bank6000 = value as i64;
            self.b.map_chr(0, 8, value as i64);
        } else if sub == 4 && address >= 0x8000 {
            self.b.map_chr(0, 8, value as i64);
        }
    }
}

// -- Daou156 ----------------------------------------------------------------------------

/// Mapper 156: Daou Infosys' DIS23C01 (Metal Force, Buzz & Waldog)
/// (INES_Mapper_156.wiki, from FCEUX's implementation, fully decoded).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Daou156 {
    pub b: Banked,
    pub chr_regs: [u16; 8],
}

impl Daou156 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Daou156 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.set_mirroring(SCREEN_A);
        Daou156 {
            b,
            chr_regs: [0; 8],
        }
    }
}

impl Cartridge for Daou156 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0xC000..=0xC00F).contains(&address) {
            let window = ((address & 3) | (address >> 1) & 4) as usize;
            let value = value as u16;
            if address & 4 != 0 {
                self.chr_regs[window] = (self.chr_regs[window] & 0xFF) | (value & 1) << 8;
            } else {
                self.chr_regs[window] = (self.chr_regs[window] & 0x100) | value;
            }
            self.b.map_chr(window, 1, self.chr_regs[window] as i64);
        } else if address == 0xC010 {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        } else if address == 0xC014 {
            self.b
                .set_mirroring([VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_A][(value & 3) as usize]);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

// -- Latch240 ---------------------------------------------------------------------------

/// Mapper 240 (Jing Ke Xin Zhuan, Sheng Huo Lie Zhuan): a register in
/// $4020-$5FFF (INES_Mapper_240.wiki; both boards' decodings taken).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Latch240 {
    pub b: Banked,
}

impl Latch240 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Latch240 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Latch240 { b }
    }
}

impl Cartridge for Latch240 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE800 == 0x4800 || address & 0xE100 == 0x4100 {
            self.b.map_prg(0, 32, (value >> 4) as i64);
            self.b.map_chr(0, 8, (value & 0x0F) as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

// -- TC112 ------------------------------------------------------------------------------

/// Mapper 193: NTDEC's TC-112 (Fighting Hero, War in the Gulf) (INES_Mapper_193.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tc112 {
    pub b: Banked,
}

impl Tc112 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Tc112 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        for window in 1..4 {
            b.map_prg(window, 8, last - 3 + window as i64);
        }
        Tc112 { b }
    }
}

impl Cartridge for Tc112 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if !(0x6000..0x8000).contains(&address) {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0xE007;
        if register == 0x6000 {
            self.b.map_chr(0, 4, (value >> 2) as i64);
        } else if register == 0x6001 || register == 0x6002 {
            self.b
                .map_chr(4 + (register - 0x6001) as usize * 2, 2, (value >> 1) as i64);
        } else if register == 0x6003 {
            self.b.map_prg(0, 8, (value & 0x0F) as i64);
        } else if register == 0x6004 {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        }
    }
}

// -- Fengshenbang246 --------------------------------------------------------------------

/// Mapper 246 (Fengshenbang): four 8 KiB PRG and four 2 KiB CHR banks at
/// $6000-$6007, the vectors' reads forcing PRG A17, 2 KiB of battery-backed
/// RAM at $6800 (INES_Mapper_246.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fengshenbang246 {
    pub b: Banked,
    pub ram: Vec<u8>,
    pub prg_regs: [u8; 4],
}

impl Fengshenbang246 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fengshenbang246 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let prg_regs = [0, 0, 0, 0xFF];
        b.prg_ram_enabled = false;
        for (window, bank) in prg_regs.into_iter().enumerate() {
            b.map_prg(window, 8, (bank & 0x3F) as i64);
        }
        Fengshenbang246 {
            b,
            ram: vec![0; 0x800],
            prg_regs,
        }
    }
}

impl Cartridge for Fengshenbang246 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6800..0x7000).contains(&address) {
            return Some(self.ram[address as usize & 0x7FF] as u16);
        }
        if address >= 0xFFE0 && address & 4 != 0 {
            let bank = ((self.prg_regs[3] | 0x10) & 0x3F) as usize;
            let prg = &self.b.prg;
            return Some(prg[(bank * 0x2000 + (address as usize & 0x1FFF)) % prg.len()] as u16);
        }
        self.b.base_cpu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6800..0x7000).contains(&address) {
            self.ram[address as usize & 0x7FF] = value;
        } else if address & 0xFFF8 == 0x6000 {
            let register = (address & 7) as usize;
            if register < 4 {
                self.prg_regs[register] = value;
                self.b.map_prg(register, 8, (value & 0x3F) as i64);
            } else {
                self.b.map_chr((register - 4) * 2, 2, value as i64);
            }
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.b.battery {
            Some(self.ram.clone())
        } else {
            None
        }
    }
    fn restore(&mut self, data: &[u8]) {
        self.ram.copy_from_slice(data);
    }
}

// -- MiHunChe27, MagicDragon107, Zemina190 -----------------------------------------------

/// Mapper 27 as FCEUX r2947 reassigned it ("Mi Hun Che"; INES_Mapper_027.wiki):
/// one latch, the 4 KiB CHR bank (at $0000 and $1000) and the one-screen nametable.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MiHunChe27 {
    pub b: Banked,
}

impl MiHunChe27 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> MiHunChe27 {
        let mut board = MiHunChe27 {
            b: Banked::new(image, cpu_cycle_ticks),
        };
        for window in 0..4 {
            board.b.map_prg(window, 8, window as i64);
        }
        board.latch(0);
        board
    }

    fn latch(&mut self, bit: u8) {
        self.b.map_chr(0, 4, bit as i64);
        self.b.map_chr(4, 4, bit as i64);
        self.b
            .set_mirroring(if bit != 0 { SCREEN_B } else { SCREEN_A });
    }
}

impl Cartridge for MiHunChe27 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address == 0x8000 {
            self.latch(value & 1);
        } else if address > 0x8000 {
            self.latch((address & 1) as u8);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 107 (Magicseries' Magic Dragon): one register for both banks
/// (INES_Mapper_107.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MagicDragon107 {
    pub b: Banked,
}

impl MagicDragon107 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> MagicDragon107 {
        let mut board = MagicDragon107 {
            b: Banked::new(image, cpu_cycle_ticks),
        };
        board.latch(0);
        board
    }

    fn latch(&mut self, value: u8) {
        self.b.map_prg(0, 32, (value >> 1) as i64);
        self.b.map_chr(0, 8, value as i64);
    }
}

impl Cartridge for MagicDragon107 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 190 (Zemina's Magic Kid Googoo): the first 16 KiB fixed at $C000
/// (INES_Mapper_190.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Zemina190 {
    pub b: Banked,
}

impl Zemina190 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Zemina190 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        b.map_prg(2, 16, 0);
        Zemina190 { b }
    }
}

impl Cartridge for Zemina190 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let region = address & 0xE000;
        if region == 0x8000 || region == 0xC000 {
            self.b
                .map_prg(0, 16, ((address >> 11) & 8 | (value & 7) as u16) as i64);
        } else if region == 0xA000 {
            self.b.map_chr((address & 3) as usize * 2, 2, value as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

// -- RT01 -------------------------------------------------------------------------------

/// NES 2.0 mapper 328 (UNL-RT-01: a TV test cartridge): 16 KiB PRG, 2 KiB
/// CHR, weak bits from a fixed 16-bit LFSR (NES_2.0_Mapper_328.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rt01 {
    pub b: Banked,
    pub lfsr: u16,
}

impl Rt01 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Rt01 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        for window in 0..4 {
            b.map_prg(window, 8, window as i64);
        }
        for window in 0..8 {
            b.map_chr(window, 1, window as i64);
        }
        Rt01 { b, lfsr: 0xACE1 }
    }
}

impl Cartridge for Rt01 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xCF80 == 0xCE80 {
            let l = self.lfsr;
            let bit = (l ^ l >> 2 ^ l >> 3 ^ l >> 5) & 1;
            self.lfsr = l >> 1 | bit << 15;
            let random = self.lfsr & 0x0D;
            return Some(0xF2 | random);
        }
        self.b.base_cpu_read(address, t)
    }
}

// -- Dance2000 --------------------------------------------------------------------------

/// NES 2.0 mapper 518 (UNL-DANCE2000: Subor 999, Subor V, Dance 2000
/// 12-in-1) (NES_2.0_Mapper_518.wiki; the speech chip and the floppy
/// controller not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Dance2000 {
    pub b: Banked,
    pub prg_reg: u8,
    pub mode: u8,
    pub a13: u16,
    pub latched_a10: u8,
}

impl Dance2000 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Dance2000 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x2000];
        b.chr_writable = true;
        let mut board = Dance2000 {
            b,
            prg_reg: 0,
            mode: 0,
            a13: 0,
            latched_a10: 0,
        };
        board.update();
        board
    }

    fn update(&mut self) {
        let bank = (self.prg_reg & 0x7F) as i64;
        if self.mode & 4 != 0 {
            self.b.map_prg(0, 32, bank);
        } else {
            self.b.map_prg(0, 16, bank);
            self.b.map_prg(2, 16, 0);
        }
        self.b.set_mirroring(if self.mode & 1 != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        });
        self.b.map_chr(0, 4, 0);
        self.b.map_chr(4, 4, 1);
    }
}

impl Cartridge for Dance2000 {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a13 = (address >> 13) & 1;
        if a13 != 0 && self.a13 == 0 {
            self.latched_a10 = self.b.base_ciram_a10(address).unwrap_or(0);
        }
        self.a13 = a13;
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x1000 && self.mode & 2 != 0 {
            return Some(self.b.chr[(self.latched_a10 as usize) << 12 | address as usize]);
        }
        self.b.base_ppu_read(address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address == 0x5000 {
            self.prg_reg = value;
        } else if address == 0x5200 {
            self.mode = value;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

// -- Noraebang515 -----------------------------------------------------------------------

/// NES 2.0 mapper 515: Family Noraebang (Korean karaoke): UNROM-style with an
/// expansion ROM (NES_2.0_Mapper_515.wiki; the YM2413 and the microphone's
/// ADC not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Noraebang515 {
    pub b: Banked,
    pub internal_base: i64,
}

impl Noraebang515 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Noraebang515 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let internal_base = (b.prg.len().saturating_sub(0x100000) / 0x4000) as i64;
        b.map_prg(0, 16, internal_base);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 8, 0);
        b.prg_ram_enabled = false;
        Noraebang515 { b, internal_base }
    }
}

impl Cartridge for Noraebang515 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0xE00F == 0x8000 {
            let bank = (value & 0x7F) as i64;
            self.b.map_prg(
                0,
                16,
                if value & 0x80 != 0 {
                    self.internal_base + bank
                } else {
                    bank
                },
            );
        }
    }
}

// -- FineStudio417 ----------------------------------------------------------------------

/// Mapper 417: the Fine Studio bootlegs of Batman (0) and RoboCop (1)
/// (NES_2.0_Mapper_417.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FineStudio417 {
    pub b: Banked,
    pub robocop: bool,
    pub ciram: [u8; 4],
    pub counter: u16,
    pub irq_enabled: bool,
    pub irq: bool,
}

impl FineStudio417 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> FineStudio417 {
        let robocop = image.board.submapper == 1;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let ciram = if b.mirroring == VERTICAL {
            [0, 1, 0, 1]
        } else {
            [0, 0, 1, 1]
        };
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        FineStudio417 {
            b,
            robocop,
            ciram,
            counter: 0,
            irq_enabled: false,
            irq: false,
        }
    }
}

impl Cartridge for FineStudio417 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let register = address & 0x8073;
        let low = (register & 3) as usize;
        if register < 0x8003 && address >= 0x8000 {
            self.b.map_prg(low, 8, value as i64);
        } else if (0x8010..0x8014).contains(&register) {
            self.b.map_chr(low, 1, value as i64);
            if self.robocop {
                self.ciram[low] = value >> 7;
            }
        } else if (0x8020..0x8024).contains(&register) {
            self.b.map_chr(4 + low, 1, value as i64);
        } else if register & 0x8070 == 0x8030 {
            self.counter = 0;
            self.irq_enabled = true;
        } else if register & 0x8070 == 0x8040 {
            self.irq_enabled = false;
            self.irq = false;
        } else if (0x8050..0x8054).contains(&register) && !self.robocop {
            self.ciram[low] = value & 1;
        }
    }
    fn m2(&mut self, _t: i64) {
        self.counter = (self.counter + 1) & if self.robocop { 0xFFF } else { 0x3FF };
        if self.counter == 0 && self.irq_enabled {
            self.irq = true;
        }
    }
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.ciram[((address >> 10) & 3) as usize])
    }
}

// -- Sangokushi330 ----------------------------------------------------------------------

/// Mapper 330 (a bootleg Sangokushi II): a subset of the Namco 163 (from the
/// forum's analysis, t=17469; the wiki has none).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sangokushi330 {
    pub b: Banked,
    pub nametables: [u8; 4],
    pub counter: u16,
    pub counting: bool,
    pub irq: bool,
}

impl Sangokushi330 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sangokushi330 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        Sangokushi330 {
            b,
            nametables: [0, 0, 1, 1], // not given: horizontal
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Sangokushi330 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0xFC00;
        if address & 0xE400 == 0x8400 {
            self.counter = (self.counter & 0x7F00) | value as u16;
        } else if address & 0xE400 == 0xA400 {
            self.counter = (self.counter & 0xFF) | ((value & 0x7F) as u16) << 8;
            self.counting = value & 0x80 != 0;
            self.irq = false;
        } else if register < 0xC000 {
            if register & 0x400 == 0 {
                self.b
                    .map_chr(((register - 0x8000) >> 11) as usize, 1, value as i64);
            }
        } else if register < 0xE000 {
            if register & 0x400 == 0 {
                self.nametables[((register >> 11) & 3) as usize] = value & 1;
            }
        } else if register < 0xF800 && register & 0x400 == 0 {
            self.b.map_prg(
                ((register - 0xE000) >> 11) as usize,
                8,
                (value & 0x1F) as i64,
            );
        }
    }
    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = (self.counter + 1) & 0x7FFF;
            if self.counter == 0 {
                self.counting = false;
                self.irq = true;
            }
        }
    }
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.nametables[((address >> 10) & 3) as usize])
    }
}

// -- FutureMedia117 ---------------------------------------------------------------------

/// Mapper 117 (Future Media's Crayon Shin-Chan, San Guo Zhi 4): 8 KiB PRG, 1
/// KiB CHR, the IRQ from PPU A12's rises filtered as an MMC3 does
/// (INES_Mapper_117.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FutureMedia117 {
    pub b: Banked,
    pub counter: u8,
    pub latch: u8,
    pub irq_enabled: bool,
    pub a12: u8,
    pub a12_ready: i64,
    pub irq: bool,
}

impl FutureMedia117 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> FutureMedia117 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        FutureMedia117 {
            b,
            counter: 0,
            latch: 0,
            irq_enabled: false,
            a12: 0,
            a12_ready: 0,
            irq: false,
        }
    }
}

impl Cartridge for FutureMedia117 {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
        } else if address < 0x8003 {
            self.b.map_prg((address & 3) as usize, 8, value as i64);
        } else if (0xA000..0xA008).contains(&address) {
            self.b.map_chr((address & 7) as usize, 1, value as i64);
        } else if (0xC000..0xC003).contains(&address) {
            self.counter = value;
            self.latch = value;
            self.irq = false;
        } else if address == 0xE000 {
            self.irq_enabled = value & 1 != 0;
        }
    }
    fn ppu_address(&mut self, address: u16, t: i64) {
        if address & 0x1000 != 0 {
            if self.a12 == 0 {
                self.a12 = 1;
                if t >= self.a12_ready && self.counter != 0 {
                    self.counter -= 1;
                    if self.counter == 0 && self.irq_enabled && self.latch != 0 {
                        self.irq = true;
                    }
                }
            }
        } else if self.a12 != 0 {
            self.a12 = 0;
            let ticks = self.b.cpu_cycle_ticks;
            self.a12_ready = (t.div_euclid(ticks) + 3) * ticks;
        }
    }
}

// -- Decathlon244 -----------------------------------------------------------------------

/// Mapper 244 (C&E's Decathlon): the address picks the banks (MAME's
/// cne.cpp, BSD-3; C69).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Decathlon244 {
    pub b: Banked,
}

impl Decathlon244 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Decathlon244 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Decathlon244 { b }
    }
}

impl Cartridge for Decathlon244 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
        } else if (0x8065..0x80A5).contains(&address) {
            self.b.map_prg(0, 32, ((address - 0x8065) & 3) as i64);
        } else if (0x80A5..0x80E5).contains(&address) {
            self.b.map_chr(0, 8, ((address - 0x80A5) & 7) as i64);
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
        14 => Box::new(Huang116::new(image, t, true)),
        27 => Box::new(MiHunChe27::new(image, t)),
        107 => Box::new(MagicDragon107::new(image, t)),
        108 => Box::new(FdsConversion108::new(image, t)),
        112 => Box::new(Asder112::new(image, t)),
        116 => Box::new(Huang116::new(image, t, false)),
        117 => Box::new(FutureMedia117::new(image, t)),
        156 => Box::new(Daou156::new(image, t)),
        177 | 241 => Box::new(Hengge::new(image, t)),
        178 => Box::new(Education178::new(image, t, false)),
        190 => Box::new(Zemina190::new(image, t)),
        193 => Box::new(Tc112::new(image, t)),
        240 => Box::new(Latch240::new(image, t)),
        244 => Box::new(Decathlon244::new(image, t)),
        246 => Box::new(Fengshenbang246::new(image, t)),
        328 => Box::new(Rt01::new(image, t)),
        330 => Box::new(Sangokushi330::new(image, t)),
        417 => Box::new(FineStudio417::new(image, t)),
        515 => Box::new(Noraebang515::new(image, t)),
        518 => Box::new(Dance2000::new(image, t)),
        551 => Box::new(Education178::new(image, t, true)),
        _ => return Err(image),
    })
}
