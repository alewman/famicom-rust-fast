// SPDX-License-Identifier: MIT
//! Cartridge conversions of Famicom Disk System games (`conversions.py`):
//! ROM and a few KiB of RAM quilted across the CPU's space. Most from their
//! NESdev wiki pages (NES_2.0_Mapper_*.wiki), others as their class says;
//! none has a judge ROM (docs/mappers.md).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 347: `KS7030`
//! - 368: `Yung08`
//! - 415: `Board0353`
//! - 418: `LH42`
//! - 535: `LH53`
//! - 538: `SoccerChampion538`
//! - 539: `Palthena539`
//! - 548: `CTC15`
//! - 549: `KS7016B`
//! - 554: `KS7010`
//! - 557: `LH42`

use super::banked::{Banked, Mirroring, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        347 => Box::new(Ks7030::new(image, t)),
        368 => Box::new(Yung08::new(image, t)),
        415 => Box::new(Board0353::new(image, t)),
        418 | 557 => Box::new(Lh42::new(image, t)),
        535 => Box::new(Lh53::new(image, t)),
        538 => Box::new(SoccerChampion538::new(image, t)),
        539 => Box::new(Palthena539::new(image, t)),
        548 => Box::new(Ctc15::new(image, t)),
        549 => Box::new(Ks7016b::new(image, t)),
        554 => Box::new(Ks7010::new(image, t)),
        _ => return Err(image),
    })
}

/// What these boards share: a ROM window at $6000 and hard-wired mirroring
/// (the reference's `Conversion`; its class attribute `mirroring_wired` is
/// the constructor's argument).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Conversion {
    pub b: Banked,
    pub bank6000: usize,
}

impl Conversion {
    pub fn new(
        image: RomImage,
        cpu_cycle_ticks: i64,
        mirroring_wired: Option<Mirroring>,
    ) -> Conversion {
        let mut c = Conversion {
            b: Banked::new(image, cpu_cycle_ticks),
            bank6000: 0,
        };
        if let Some(mode) = mirroring_wired {
            c.b.set_mirroring(mode);
        }
        c.b.map_chr(0, 8, 0);
        c
    }

    pub fn read_6000(&self, address: u16) -> u8 {
        self.b.prg[(self.bank6000 * 0x2000 + (address as usize & 0x1FFF)) % self.b.prg.len()]
    }

    /// `Conversion.cpu_read`.
    pub fn cpu_read(&self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(self.read_6000(address) as u16);
        }
        self.b.base_cpu_read(address, t)
    }
}

/// Mapper 415: the 0353 board (Lucky Rabbit, the FDS's Roger Rabbit): a latch
/// at $8000-$FFFF, the $6000 bank and the mirroring (NES_2.0_Mapper_415.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Board0353 {
    pub c: Conversion,
}

impl Board0353 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Board0353 {
        let mut c = Conversion::new(image, cpu_cycle_ticks, None);
        let last = (c.b.prg.len() / 0x8000) as i64 - 1;
        c.b.map_prg(0, 32, last);
        Board0353 { c }
    }
}

impl Cartridge for Board0353 {
    banked_field!(c.b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.c.cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.c.bank6000 = (value & 0x0F) as usize;
            self.c.b.set_mirroring(if value & 0x10 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        } else {
            self.c.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 554: Kaiser's KS-7010 (Akumajou Dracula): banks switched by reads
/// of magic addresses.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7010 {
    pub c: Conversion,
}

impl Ks7010 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7010 {
        let mut c = Conversion::new(image, cpu_cycle_ticks, Some(VERTICAL));
        for (window, bank) in [10, 11, 6, 7].into_iter().enumerate() {
            c.b.map_prg(window, 8, bank);
        }
        Ks7010 { c }
    }
}

impl Cartridge for Ks7010 {
    banked_field!(c.b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let value = self.c.cpu_read(address, t);
        if (0xCAB6..=0xCAD6).contains(&address) || matches!(address, 0xEBE2 | 0xEE32 | 0xFFFC) {
            self.c.bank6000 = ((address >> 2) & 0x0F) as usize;
            let bank = self.c.bank6000 as i64;
            self.c.b.map_chr(0, 8, bank);
        }
        value
    }
}

/// Mapper 548: Co Tung's CTC-15 (Almana no Kiseki): a latch in the address of
/// $4800 writes, applied at $5000, and an M2 counter.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ctc15 {
    pub b: Banked,
    pub latch: i64,
    pub counter: i64,
    pub counting: bool,
    pub irq: bool,
}

impl Ctc15 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ctc15 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram = vec![0; b.prg_ram.len().max(0x2000)];
        b.prg_ram_enabled = true;
        b.prg_ram_mask = 0x1FFF;
        b.set_mirroring(HORIZONTAL);
        b.map_prg(0, 16, 7);
        b.map_prg(2, 16, 3);
        b.map_chr(0, 8, 0);
        Ctc15 {
            b,
            latch: 7,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Ctc15 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF800 == 0x4800 {
            let a = address as i64;
            self.latch = (((a >> 5) & 1) << 2) | ((a >> 2) & 3);
            self.counting = address & 4 == 0;
            if !self.counting {
                self.counter = 0;
                self.irq = false;
            }
        } else if address & 0xF800 == 0x5000 {
            self.b.map_prg(0, 16, self.latch ^ 5);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter += 1;
            self.irq = (self.counter.div_euclid(640)) & 37 == 37;
        }
    }
}

/// Mapper 539's RAM over the CPU's space: (first, last, offset in the 8 KiB).
pub const PALTHENA_RAM: [(u16, u16, usize); 6] = [
    (0xC000, 0xD1FF, 0x0000),
    (0x6000, 0x60FF, 0x1200),
    (0x6200, 0x62FF, 0x1300),
    (0x6400, 0x65FF, 0x1400),
    (0x8200, 0x82FF, 0x1600),
    (0xDF00, 0xDFFF, 0x1700),
];

/// Mapper 539: a bootleg of the disk Kid Icarus: 8 KiB of RAM laid over ROM
/// in slices.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Palthena539 {
    pub c: Conversion,
}

impl Palthena539 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Palthena539 {
        let mut c = Conversion::new(image, cpu_cycle_ticks, None);
        c.b.prg_ram = vec![0; 0x2000];
        c.bank6000 = 0x0D;
        c.b.map_prg(0, 8, 0x0C);
        c.b.map_prg(1, 8, 0);
        c.b.map_prg(2, 8, 0x0E);
        c.b.map_prg(3, 8, 0x0F);
        Palthena539 { c }
    }

    fn ram(address: u16) -> Option<usize> {
        for (first, last, offset) in PALTHENA_RAM {
            if first <= address && address <= last {
                return Some(offset + (address - first) as usize);
            }
        }
        None
    }
}

impl Cartridge for Palthena539 {
    banked_field!(c.b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        match Palthena539::ram(address) {
            None => self.c.cpu_read(address, t),
            Some(ram) => Some(self.c.b.prg_ram[ram] as u16),
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if let Some(ram) = Palthena539::ram(address) {
            self.c.b.prg_ram[ram] = value;
        } else if address & 0xE000 == 0xA000 {
            self.c.b.map_prg(1, 8, (value & 0x0F) as i64);
        } else if address == 0xFFFF {
            self.c
                .b
                .set_mirroring(if value & 8 != 0 { HORIZONTAL } else { VERTICAL });
        }
    }
}

/// Mapper 535: Whirlwind Manu's LH53 (Nazo no Murasamejou): 8 KiB of RAM at
/// $B800-$D7FF and an M2 counter reaching 7560 (the number from FCEUX).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Lh53 {
    pub c: Conversion,
    pub counter: i64,
    pub counting: bool,
    pub irq: bool,
}

impl Lh53 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Lh53 {
        let mut c = Conversion::new(image, cpu_cycle_ticks, None);
        c.b.prg_ram = vec![0; 0x2000];
        let last = (c.b.prg.len() / 0x8000) as i64 - 1;
        c.b.map_prg(0, 32, last);
        Lh53 {
            c,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Lh53 {
    banked_field!(c.b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0xB800..0xD800).contains(&address) {
            return Some(self.c.b.prg_ram[(address - 0xB800) as usize] as u16);
        }
        self.c.cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0xB800..0xD800).contains(&address) {
            self.c.b.prg_ram[(address - 0xB800) as usize] = value;
        } else if address & 0xF000 == 0xE000 {
            self.irq = false;
            self.counter = 0;
            self.counting = value & 2 != 0;
        } else if address & 0xF000 == 0xF000 {
            self.c.bank6000 = value as usize;
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter += 1;
            if self.counter == 7560 {
                self.irq = true;
            }
        }
    }
}

/// Mapper 368's bank at $C000 for each value of D2-D0.
pub const YUNG08_BANKS: [i64; 8] = [4, 3, 5, 3, 6, 3, 7, 3];

/// Mapper 368: the Yung-08 conversion of the disk Super Mario Bros. 2: a
/// table-banked $C000 and a 12-bit M2 counter.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Yung08 {
    pub c: Conversion,
    pub latch: u8,
    pub counter: u16,
    pub counting: bool,
    pub irq: bool,
}

impl Yung08 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Yung08 {
        let mut c = Conversion::new(image, cpu_cycle_ticks, None);
        c.bank6000 = 2;
        c.b.map_prg(0, 8, 1);
        c.b.map_prg(1, 8, 0);
        c.b.map_prg(2, 8, YUNG08_BANKS[0]);
        c.b.map_prg(3, 8, 8);
        Yung08 {
            c,
            latch: 0,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Yung08 {
    banked_field!(c.b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xF1FF == 0x4122 {
            return Some((0x8A | (self.latch & 0x35)) as u16);
        }
        self.c.cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0xF1FF == 0x4022 {
            self.c.b.map_prg(2, 8, YUNG08_BANKS[(value & 7) as usize]);
        } else if address & 0xF1FF == 0x4122 {
            self.latch = value;
            self.counting = value & 1 != 0;
            if !self.counting {
                self.counter = 0;
                self.irq = false;
            }
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = (self.counter + 1) & 0xFFF;
            if self.counter == 0 {
                self.irq = true;
            }
        }
    }
}

/// Mapper 538: the 60-1064-16L board (Super Soccer Champion, the disk
/// Exciting Soccer): one write sets the banks at $6000 and $8000.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SoccerChampion538 {
    pub c: Conversion,
}

impl SoccerChampion538 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> SoccerChampion538 {
        let mut s = SoccerChampion538 {
            c: Conversion::new(image, cpu_cycle_ticks, Some(VERTICAL)),
        };
        s.select(0);
        for (window, bank) in [(1, 13), (2, 14), (3, 15)] {
            s.c.b.map_prg(window, 8, bank);
        }
        s
    }

    fn select(&mut self, value: u8) {
        self.c.bank6000 = (value | 1) as usize;
        let bank = if value & 1 != 0 && value & 8 == 0 {
            10
        } else {
            (value & !1) as i64
        };
        self.c.b.map_prg(0, 8, bank);
    }
}

impl Cartridge for SoccerChampion538 {
    banked_field!(c.b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.c.cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0xE000 == 0xC000 {
            self.select(value & 0x0F);
        }
    }
}

/// Mapper 347: Kaiser's KS-7030 (UNL-KS7030, Yume Koujou: Doki Doki Panic):
/// 8 KiB of RAM and banked ROM quilted over $6000-$FFFF, banks set by the
/// address of $8000/$9000 writes.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7030 {
    pub b: Banked,
    pub bank7000: i64,
    pub bank_c000: i64,
    pub fixed: i64,
}

impl Ks7030 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7030 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram = vec![0; 0x2000];
        let fixed = b.prg.len() as i64 - 0x8000;
        b.map_chr(0, 8, 0);
        Ks7030 {
            b,
            bank7000: 0,
            bank_c000: 0,
            fixed,
        }
    }

    /// (is RAM, offset) for a CPU address, or None for the unmapped.
    fn place(&self, address: u16) -> Option<(bool, i64)> {
        let a = address as i64;
        if address < 0x6000 {
            return None;
        }
        if address < 0x6C00 {
            return Some((true, a - 0x6000));
        }
        if address < 0x7000 {
            return Some((false, self.bank_c000 * 0x1000 + 0xC00 + (a & 0x3FF)));
        }
        if address < 0x8000 {
            return Some((false, 0x10000 + self.bank7000 * 0x1000 + (a & 0xFFF)));
        }
        if (0xB800..0xC000).contains(&address) {
            return Some((true, 0xC00 + a - 0xB800));
        }
        if (0xC000..0xCC00).contains(&address) {
            return Some((false, self.bank_c000 * 0x1000 + (a & 0xFFF)));
        }
        if (0xCC00..0xD800).contains(&address) {
            return Some((true, 0x1400 + a - 0xCC00));
        }
        Some((false, self.fixed + (a & 0x7FFF)))
    }
}

impl Cartridge for Ks7030 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        let (ram, offset) = self.place(address)?;
        Some(if ram {
            self.b.prg_ram[offset as usize]
        } else {
            self.b.prg[offset.rem_euclid(self.b.prg.len() as i64) as usize]
        } as u16)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0xF000 == 0x8000 {
            self.bank7000 = (address & 7) as i64;
            self.b.set_mirroring(if address & 8 != 0 {
                HORIZONTAL
            } else {
                VERTICAL
            });
        } else if address & 0xF000 == 0x9000 {
            self.bank_c000 = (address & 0x0F) as i64;
        } else if let Some((true, offset)) = self.place(address) {
            self.b.prg_ram[offset as usize] = value;
        }
    }
}

/// Mappers 418 (LH42, Highway Star) and 557 (LG25, NTDEC 2718, Moero
/// TwinBee): a cut-down MMC3's two ports (MAME's bootleg.cpp, BSD-3).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Lh42 {
    pub b: Banked,
    pub mirror_bit: u8,
    pub select: u8,
}

impl Lh42 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Lh42 {
        let mirror_bit = if image.board.mapper == 557 { 5 } else { 0 };
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 8, 0);
        Lh42 {
            b,
            mirror_bit,
            select: 0,
        }
    }
}

impl Cartridge for Lh42 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
        } else if address & 0xE001 == 0x8000 {
            self.select = value & 7;
        } else if address & 0xE001 == 0x8001 {
            if self.select == 5 {
                self.b
                    .set_mirroring(if (value >> self.mirror_bit) & 1 != 0 {
                        HORIZONTAL
                    } else {
                        VERTICAL
                    });
            } else if self.select >= 6 {
                self.b
                    .map_prg((self.select & 1) as usize, 8, (value & 0x0F) as i64);
            }
        }
    }
}

/// Mapper 549 (Meikyuu Jiin Dababa): Kaiser's KS-7016B: the address of a
/// write banks $6000 (krzysiobal, forum t=19303; MAME's kaiser.cpp, BSD-3).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7016b {
    pub c: Conversion,
}

impl Ks7016b {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7016b {
        let mut c = Conversion::new(image, cpu_cycle_ticks, None);
        c.b.map_prg(0, 32, 2);
        Ks7016b { c }
    }
}

impl Cartridge for Ks7016b {
    banked_field!(c.b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.c.cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            let bank = ((address >> 2) & 0x0F) as usize;
            self.c.bank6000 = if bank & 8 != 0 { bank | 4 } else { bank };
        } else {
            self.c.b.base_cpu_write(address, value, t);
        }
    }
}
