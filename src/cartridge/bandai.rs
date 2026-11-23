// SPDX-License-Identifier: MIT
//! Bandai's FCG family (`bandai.py`): FCG-1/2 and LZ93D50 (16), the Datach
//! Joint ROM System (157), and the LZ93D50 with an X24C01 (159) or 8 KiB of
//! RAM (153); the discrete 74161/7432 boards (70, 152); the Oeka Kids board
//! (96); and Karaoke Studio's M60001 (188). INES_Mapper_016.wiki, _157,
//! _159, _153, _070, _152, _096 and _188. No judge ROM exists; the claims
//! rest on sweeps against Mesen2.
//!
//! The reference's `BandaiFCGRAM`, `BandaiX24C01` and `Datach` subclass
//! `BandaiFCG` and override `_eeproms` (which its `__init__` calls),
//! `register`, `chr_register`, `clock_eeproms`, `cpu_read`, `cpu_write`,
//! `battery_ram` and `_restore`; here one struct carries a [`Kind`].

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::eeprom::Eeprom;
use super::ines::RomImage;
use crate::banked_field;

const ARRANGEMENTS: [Mirroring; 4] = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// Mapper 16 (and the LZ93D50 base of the others).
    Fcg,
    /// Mapper 153: the LZ93D50 with 8 KiB of battery-backed RAM (Famicom Jump II).
    FcgRam { outer: i64, inner: i64 },
    /// Mapper 159: the LZ93D50 with Xicor's 128-byte X24C01.
    X24c01,
    /// Mapper 157: the Datach Joint ROM System: unbanked CHR-RAM, a barcode
    /// reader (idle here), two EEPROMs.
    Datach,
}

/// Mapper 16: 16 KiB PRG and the last fixed, eight 1 KiB CHR banks, a 16-bit
/// M2 counter (registers at $6000 on the FCG-1/2, $8000 on the LZ93D50). While
/// counting, each M2 cycle the counter is tested and then decremented, a zero
/// asserting /IRQ. DOC: the wiki's order (INES_Mapper_016.wiki: "When it holds
/// a value of zero, an IRQ is generated", and enabling it while it holds zero
/// generates one "immediately"). OBSERVED closed-box: Mesen2 2.1.1 gives the
/// immediate IRQ too, but asserts /IRQ one cycle sooner after counts of
/// $100-$108 (the reference's `scripts/provenance_probes.py
/// fcg-test-then-decrement`); MAME's bandai.cpp decrements first. That cycle is
/// open: no hardware test.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BandaiFcg {
    pub b: Banked,
    pub kind: Kind,
    pub lz93d50: bool,
    /// $6000-$7FFF.
    pub ports_low: bool,
    /// $8000-$FFFF.
    pub ports_high: bool,
    pub eeproms: Vec<Eeprom>,
    pub scl: u8,
    pub sda: u8,
    pub releasing: bool,
    pub counter: u16,
    pub latch: u16,
    pub counting: bool,
    pub irq: bool,
}

impl BandaiFcg {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64, kind: Kind) -> BandaiFcg {
        let board = image.board.clone();
        let submapper = board.submapper;
        let b = Banked::new(image, cpu_cycle_ticks);
        let eeproms = BandaiFcg::eeproms_for(kind, board.prg_nvram, submapper);
        let mut m = BandaiFcg {
            b,
            kind,
            lz93d50: submapper == 5 || board.mapper != 16,
            ports_low: board.mapper == 16 && submapper != 5,
            ports_high: board.mapper != 16 || submapper != 4,
            eeproms,
            scl: 1,
            sda: 1,
            releasing: false,
            counter: 0,
            latch: 0,
            counting: false,
            irq: false,
        };
        m.b.prg_ram_enabled = false;
        let last = (m.b.prg.len() / 0x4000) as i64 - 1;
        m.b.map_prg(2, 16, last);
        match kind {
            Kind::FcgRam { .. } => {
                m.b.map_chr(0, 8, 0);
                m.map_prg_ram_board();
            }
            Kind::Datach => m.b.map_chr(0, 8, 0), // unbanked: the 8 KiB straight through
            _ => {}
        }
        m
    }

    /// `_eeproms`, per class.
    fn eeproms_for(kind: Kind, nvram: usize, submapper: u8) -> Vec<Eeprom> {
        match kind {
            Kind::Fcg => {
                if nvram == 256 || submapper == 0 {
                    vec![Eeprom::new(256)]
                } else {
                    vec![]
                }
            }
            Kind::FcgRam { .. } => vec![],
            Kind::X24c01 => vec![Eeprom::x24c01()],
            Kind::Datach => {
                if nvram == 128 {
                    vec![Eeprom::new(256), Eeprom::x24c01()]
                } else {
                    vec![Eeprom::new(256)]
                }
            }
        }
    }

    /// `BandaiFCGRAM._map_prg`.
    fn map_prg_ram_board(&mut self) {
        if let Kind::FcgRam { outer, inner } = self.kind {
            self.b.map_prg(0, 16, outer << 4 | inner);
            self.b.map_prg(2, 16, outer << 4 | 0x0F);
        }
    }

    fn sda_line(&self) -> u8 {
        let mut line = if self.releasing { 1 } else { self.sda };
        for eeprom in &self.eeproms {
            line &= eeprom.output;
        }
        line
    }

    /// `BandaiFCG.cpu_read`.
    fn fcg_cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 {
            return self.b.base_cpu_read(address, t);
        }
        if address >= 0x6000 && !self.eeproms.is_empty() {
            return Some((self.sda_line() as u16) << 4 | 0xEF << 8);
        }
        None
    }

    /// `BandaiFCG.cpu_write`.
    fn fcg_cpu_write(&mut self, address: u16, value: u8) {
        let decodes = if address >= 0x8000 {
            self.ports_high
        } else {
            address >= 0x6000 && self.ports_low
        };
        if decodes {
            self.register((address & 0x0F) as u8, value);
        }
    }

    fn register(&mut self, number: u8, value: u8) {
        if let Kind::FcgRam { inner, .. } = &mut self.kind {
            if number == 8 {
                *inner = (value & 0x0F) as i64;
                self.map_prg_ram_board();
                return;
            }
            if number == 0x0D {
                self.b.prg_ram_enabled = value & 0x20 != 0 && !self.b.prg_ram.is_empty();
                return;
            }
        }
        self.fcg_register(number, value);
    }

    /// `BandaiFCG.register`.
    fn fcg_register(&mut self, number: u8, value: u8) {
        if number < 8 {
            self.chr_register(number, value);
        } else if number == 8 {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        } else if number == 9 {
            self.b.set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
        } else if number == 0x0A {
            self.counting = value & 1 != 0;
            self.irq = false;
            if self.lz93d50 {
                self.counter = self.latch;
            }
        } else if number == 0x0B || number == 0x0C {
            let shift = if number == 0x0C { 8 } else { 0 };
            if self.lz93d50 {
                self.latch = (self.latch & (0xFF00 >> shift)) | (value as u16) << shift;
            } else {
                self.counter = (self.counter & (0xFF00 >> shift)) | (value as u16) << shift;
            }
        } else if number == 0x0D && self.lz93d50 {
            self.releasing = value & 0x80 != 0;
            self.sda = (value >> 6) & 1;
            self.scl = (value >> 5) & 1;
            self.clock_eeproms();
        }
    }

    fn chr_register(&mut self, number: u8, value: u8) {
        match &mut self.kind {
            Kind::FcgRam { outer, .. } => {
                if number < 4 {
                    *outer = (value & 1) as i64;
                    self.map_prg_ram_board();
                }
            }
            Kind::Datach => {
                if number < 4 && self.eeproms.len() == 2 {
                    let sda = self.sda_line();
                    self.eeproms[1].lines((value >> 3) & 1, sda);
                }
            }
            _ => self.b.map_chr(number as usize, 1, value as i64),
        }
    }

    fn clock_eeproms(&mut self) {
        // SDA is open drain: a part pulling it low holds it there when the
        // master releases it, so releasing with SCL high is no STOP.
        let sda = self.sda_line();
        if self.kind == Kind::Datach {
            let scl = self.scl;
            self.eeproms[0].lines(scl, sda);
            if self.eeproms.len() == 2 {
                let external = &mut self.eeproms[1];
                let scl = external.scl;
                external.lines(scl, sda);
            }
            return;
        }
        let scl = self.scl;
        for eeprom in self.eeproms.iter_mut() {
            eeprom.lines(scl, sda);
        }
    }
}

impl Cartridge for BandaiFcg {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            if self.counter == 0 {
                self.irq = true;
            }
            self.counter = self.counter.wrapping_sub(1);
        }
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        match self.kind {
            Kind::FcgRam { .. } => {
                if (0x6000..0x8000).contains(&address) && self.b.prg_ram_enabled {
                    return Some(self.b.prg_ram[address as usize & self.b.prg_ram_mask] as u16);
                }
                self.fcg_cpu_read(address, t)
            }
            Kind::Datach => {
                if address >= 0x8000 {
                    return self.b.base_cpu_read(address, t);
                }
                if address >= 0x6000 {
                    return Some((self.sda_line() as u16) << 4 | 0xE7 << 8);
                }
                None
            }
            _ => self.fcg_cpu_read(address, t),
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if let Kind::FcgRam { .. } = self.kind {
            if (0x6000..0x8000).contains(&address) {
                if self.b.prg_ram_enabled {
                    let mask = self.b.prg_ram_mask;
                    self.b.prg_ram[address as usize & mask] = value;
                }
                return;
            }
        }
        self.fcg_cpu_write(address, value);
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if let Kind::FcgRam { .. } = self.kind {
            return self.b.base_battery_ram();
        }
        let data: Vec<u8> = self
            .eeproms
            .iter()
            .flat_map(|e| e.memory.iter().copied())
            .collect();
        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    }

    fn restore(&mut self, data: &[u8]) {
        if let Kind::FcgRam { .. } = self.kind {
            self.b.prg_ram.copy_from_slice(data);
            return;
        }
        let mut data = data;
        for eeprom in self.eeproms.iter_mut() {
            let size = eeprom.memory.len();
            eeprom.memory.copy_from_slice(&data[..size]);
            data = &data[size..];
        }
    }
}

/// Mappers 70 and 152: a 74161 and a 7432, UNROM's PRG with a CHR bank on
/// top (INES_Mapper_070.wiki, _152.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Bandai74161 {
    pub b: Banked,
    pub one_screen: bool,
}

impl Bandai74161 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Bandai74161 {
        let one_screen = image.board.mapper == 152;
        let mut m = Bandai74161 {
            b: Banked::new(image, cpu_cycle_ticks),
            one_screen,
        };
        let last = (m.b.prg.len() / 0x4000) as i64 - 1;
        m.b.map_prg(2, 16, last);
        m.latch(0);
        m
    }

    fn latch(&mut self, value: u8) {
        self.b.map_chr(0, 8, (value & 0x0F) as i64);
        if self.one_screen {
            self.b.map_prg(0, 16, ((value >> 4) & 7) as i64);
            self.b.set_mirroring(if value & 0x80 != 0 {
                SCREEN_B
            } else {
                SCREEN_A
            });
        } else {
            self.b.map_prg(0, 16, (value >> 4) as i64);
        }
    }
}

impl Cartridge for Bandai74161 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.latch(value);
    }
}

/// Mapper 96: the Oeka Kids board: CHR-RAM banked by the nametable the PPU
/// last moved to (INES_Mapper_096.wiki). The tablet itself is a device on
/// the console's expansion port, not the cartridge's.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct OekaKids {
    pub b: Banked,
    pub outer: i64,
    pub inner: i64,
    pub in_nametables: bool,
}

impl OekaKids {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> OekaKids {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x8000];
        b.chr_writable = true;
        let mut m = OekaKids {
            b,
            outer: 0,
            inner: 0,
            in_nametables: false,
        };
        m.b.map_prg(0, 32, 0);
        m.map_chr();
        m
    }

    fn map_chr(&mut self) {
        self.b.map_chr(0, 4, self.outer << 2 | self.inner);
        self.b.map_chr(4, 4, self.outer << 2 | 3);
    }
}

impl Cartridge for OekaKids {
    banked_field!(b);

    fn watches_ppu_address(&self) -> bool {
        true
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.b.map_prg(0, 32, (value & 3) as i64);
        self.outer = ((value >> 2) & 1) as i64;
        self.map_chr();
    }

    fn ppu_address(&mut self, address: u16, _t: i64) {
        let now = address & 0x3000 == 0x2000;
        if now && !self.in_nametables {
            self.inner = ((address >> 8) & 3) as i64;
            self.map_chr();
        }
        self.in_nametables = now;
    }
}

/// Mapper 188: Bandai's Karaoke Studio (M60001): 16 KiB PRG banks, internal
/// or expansion ROM (INES_Mapper_188.wiki). The microphone at $6000-$7FFF
/// reads idle: buttons released, the ADC silent.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Karaoke {
    pub b: Banked,
    pub internal_banks: i64,
    /// The reference's `expansion`: an expansion cartridge's ROM is present.
    pub has_expansion: bool,
    pub from_expansion: bool,
}

impl Karaoke {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Karaoke {
        let b = Banked::new(image, cpu_cycle_ticks);
        let internal_banks = (b.prg.len().min(0x20000) / 0x4000) as i64;
        let has_expansion = b.prg.len() > 0x20000;
        let mut m = Karaoke {
            b,
            internal_banks,
            has_expansion,
            from_expansion: false,
        };
        m.b.map_prg(2, 16, internal_banks - 1);
        m.b.map_prg(0, 16, 0);
        m.b.map_chr(0, 8, 0);
        m
    }
}

impl Cartridge for Karaoke {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            // The buttons released, the ADC low; D7-D3 undriven.
            return Some(0x03 | 0xF8 << 8);
        }
        if (0x8000..0xC000).contains(&address) && self.from_expansion && !self.has_expansion {
            return None;
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address < 0x8000 {
            return;
        }
        let value = self.b.bus_conflict(address, value);
        let mut bank = (value & 0x0F) as i64;
        self.from_expansion = value & 0x10 == 0;
        if self.from_expansion {
            bank = self.internal_banks + bank.rem_euclid(self.internal_banks);
        } else {
            bank = bank.rem_euclid(self.internal_banks);
        }
        self.b.map_prg(0, 16, bank);
        self.b.set_mirroring(if value & 0x20 != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        });
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
        16 => Box::new(BandaiFcg::new(image, t, Kind::Fcg)),
        70 | 152 => Box::new(Bandai74161::new(image, t)),
        96 => Box::new(OekaKids::new(image, t)),
        153 => Box::new(BandaiFcg::new(
            image,
            t,
            Kind::FcgRam { outer: 0, inner: 0 },
        )),
        157 => Box::new(BandaiFcg::new(image, t, Kind::Datach)),
        159 => Box::new(BandaiFcg::new(image, t, Kind::X24c01)),
        188 => Box::new(Karaoke::new(image, t)),
        _ => return Err(image),
    })
}
