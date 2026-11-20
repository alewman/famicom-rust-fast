// SPDX-License-Identifier: MIT
//! Homebrew boards (`homebrew.py`): Action 53 (28), Sealie's RET-CUFROM (29),
//! the NSF subset (31), GTROM (111), Magic Floor (218), Drip (284) and
//! BATMAP-SRR-X (413). Each from its NESdev wiki page (or MAME, as its class
//! says) and judged by its own community test ROMs (docs/claims.md).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 28: `Action53`
//! - 29: `SealieRET`
//! - 31: `NSF31`
//! - 111: `GTROM`
//! - 218: `MagicFloor`
//! - 284: `DripGame`
//! - 413: `BATMAPSRR`

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::flash::Flash;
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
        28 => Box::new(Action53::new(image, t)),
        29 => Box::new(SealieRet::new(image, t)),
        31 => Box::new(Nsf31::new(image, t)),
        111 => Box::new(Gtrom::new(image, t)),
        218 => Box::new(MagicFloor::new(image, t)),
        284 => Box::new(DripGame::new(image, t)),
        413 => Box::new(BatmapSrr::new(image, t)),
        _ => return Err(image),
    })
}

/// Mapper 31: PRG-ROM in eight 4 KiB slots at $8000-$FFFF, as an NSF banks
/// it (INES_Mapper_031.wiki); $F000's register powers on as $FF.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Nsf31 {
    pub b: Banked,
    /// Byte offsets into PRG of each 4 KiB slot.
    pub slots: [usize; 8],
}

impl Nsf31 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Nsf31 {
        let mut n = Nsf31 {
            b: Banked::new(image, cpu_cycle_ticks),
            slots: [0; 8],
        };
        n.select(7, 0xFF);
        n.b.map_chr(0, 8, 0);
        n
    }

    fn select(&mut self, slot: usize, bank: u8) {
        self.slots[slot] = ((bank as usize) << 12) % self.b.prg.len();
    }
}

impl Cartridge for Nsf31 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 {
            let a = address as usize;
            return Some(self.b.prg[self.slots[(a >> 12) & 7] + (a & 0x0FFF)] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            self.select((address & 7) as usize, value);
            return;
        }
        self.b.base_cpu_write(address, value, t);
    }
}

/// Mapper 28: Damian Yerrick's multicart mapper, which simulates NROM, CNROM,
/// UxROM, BNROM and AxROM (Action_53_mapper.wiki). Power-on: the wiki's
/// suggested boot state, $81 = $FF and $80 = $02.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Action53 {
    pub b: Banked,
    pub select: u8,
    /// The registers $00, $01, $80, $81.
    pub regs: [u8; 4],
}

fn action53_slot(register: u8) -> usize {
    match register {
        0x00 => 0,
        0x01 => 1,
        0x80 => 2,
        _ => 3,
    }
}

impl Action53 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Action53 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        if b.chr_writable && b.chr.len() < 0x8000 {
            b.chr = vec![0; 0x8000];
        }
        let mut a = Action53 {
            b,
            select: 0,
            regs: [0, 0, 0x02, 0xFF],
        };
        a.update();
        a
    }

    fn update(&mut self) {
        let regs = self.regs.map(|r| r as i64);
        let mode = regs[2];
        let (size, prg_mode, mirroring) = ((mode >> 4) & 3, (mode >> 2) & 3, mode & 3);
        let (outer, inner) = (regs[3], regs[1]);
        let (low, high);
        if prg_mode < 2 {
            // 32 KiB: the low outer bits give way to inner ones
            let mask = (1 << size) - 1;
            let bank32 = (outer & !mask) | (inner & mask);
            low = bank32 << 1;
            high = (bank32 << 1) | 1;
        } else {
            // UxROM: one 16 KiB half switchable, the other fixed to the outer bank
            let mask = (2 << size) - 1;
            let switched = ((outer << 1) & !mask) | (inner & mask);
            if prg_mode == 2 {
                // fixed $8000 (mapper 180)
                low = outer << 1;
                high = switched;
            } else {
                // fixed $C000 (mapper 2)
                low = switched;
                high = (outer << 1) | 1;
            }
        }
        self.b.map_prg(0, 16, low);
        self.b.map_prg(2, 16, high);
        self.b.map_chr(0, 8, regs[0] & 3);
        self.b
            .set_mirroring([SCREEN_A, SCREEN_B, VERTICAL, HORIZONTAL][mirroring as usize]);
    }
}

impl Cartridge for Action53 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x5000..0x6000).contains(&address) {
            self.select = value & 0x81;
            return;
        }
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = self.select;
        self.regs[action53_slot(register)] = value;
        if (register == 0x00 || register == 0x01) && self.regs[2] & 2 == 0 {
            // A 1-screen mode: D4 is the screen, as AxROM's (the wiki's "M").
            self.regs[2] = (self.regs[2] & !1) | ((value >> 4) & 1);
        }
        self.update();
    }
}

/// Mapper 111: Membler Industries' Cheapocabra (GTROM.wiki): one 74377
/// register (latched on reads too), the SST39SF040 as PRG, 32 KiB of RAM for
/// CHR and four nametables. The PRG is flash, so it is the save.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Gtrom {
    pub b: Banked,
    pub flash: Flash,
    pub register: u8,
    /// Where the selected nametable page begins.
    pub nametables: usize,
}

impl Gtrom {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Gtrom {
        let prg = image.prg.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg = prg;
        b.chr = vec![0; 0x8000];
        b.chr_writable = true;
        let mut g = Gtrom {
            b,
            flash: Flash::sst39sf040(),
            register: 0,
            nametables: 0x4000,
        };
        g.latch(0);
        g
    }

    fn latch(&mut self, value: u8) {
        self.register = value;
        self.b.map_prg(0, 32, (value & 0x0F) as i64);
        self.b.map_chr(0, 8, ((value >> 4) & 1) as i64);
        self.nametables = 0x4000 + ((value as usize >> 5) & 1) * 0x2000;
    }

    fn flash_offset(&self, address: u16) -> usize {
        (((self.register as usize & 0x0F) << 15) | (address as usize & 0x7FFF)) % self.b.prg.len()
    }
}

impl Cartridge for Gtrom {
    banked_field!(b);
    fn latches_reads(&self) -> bool {
        true
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if self.flash.id_mode && address >= 0x8000 {
            let offset = self.flash_offset(address);
            return Some(self.flash.read(&self.b.prg, offset) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn read_data(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0x5000 == 0x5000 && address < 0x8000 {
            self.latch(value);
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x8000 {
            let offset = self.flash_offset(address);
            self.flash.write(&mut self.b.prg, offset, value);
        } else if address & 0x5000 == 0x5000 {
            self.latch(value);
        }
    }

    fn ciram_a10(&mut self, _address: u16) -> Option<u8> {
        None
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address >= 0x2000 {
            return Some(self.b.chr[self.nametables + (address as usize & 0x1FFF)]);
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x2000 {
            self.b.chr[self.nametables + (address as usize & 0x1FFF)] = value;
            return;
        }
        self.b.base_ppu_write(address, value, t);
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        Some(self.b.prg.clone())
    }

    fn restore(&mut self, data: &[u8]) {
        self.b.prg.copy_from_slice(data);
    }
}

/// Mapper 218 (Magic Floor, Starfight): no CHR memory: the console's CIRAM is
/// the CHR-RAM, its A10 wired to PPU A10, A11, A12 or A13
/// (INES_Mapper_218.wiki); the board answers with a 2 KiB RAM of its own.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MagicFloor {
    pub b: Banked,
    pub a10_line: u32,
    pub ciram: Vec<u8>,
}

impl MagicFloor {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> MagicFloor {
        let board = image.board.clone();
        let flags6 = if image.header.len() > 6 {
            image.header[6]
        } else {
            0
        };
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let a10_line = if flags6 & 8 != 0 {
            if flags6 & 1 != 0 {
                13
            } else {
                12
            }
        } else if board.mirroring == 'V' {
            10
        } else {
            11
        };
        b.four_screen = false;
        b.prg = b.prg.repeat(0x8000 / b.prg.len()); // Magic Floor is 4 KiB
        for window in 0..4 {
            b.map_prg(window, 8, window as i64);
        }
        MagicFloor {
            b,
            a10_line,
            ciram: vec![0; 0x800],
        }
    }

    fn index(&self, address: u16) -> usize {
        ((((address as usize) >> self.a10_line) & 1) << 10) | (address as usize & 0x3FF)
    }
}

impl Cartridge for MagicFloor {
    banked_field!(b);
    fn ciram_a10(&mut self, _address: u16) -> Option<u8> {
        None
    }

    fn ppu_read(&mut self, address: u16, _t: i64) -> Option<u8> {
        Some(self.ciram[self.index(address)])
    }

    fn ppu_write(&mut self, address: u16, value: u8, _t: i64) {
        let i = self.index(address);
        self.ciram[i] = value;
    }
}

/// NES 2.0 mapper 284: Quietust's FPGA for the port of Drip
/// (UNIF_UNL-DripGame.wiki): banks, an extended attribute table and an M2
/// down-counter; the sound channels are not modelled.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DripGame {
    pub b: Banked,
    pub control: u8,
    pub reload: i64,
    pub counter: i64,
    pub counting: bool,
    pub ext: Vec<u8>,
    /// CIRAM index of the last nametable fetch.
    pub last_tile: usize,
    pub irq: bool,
}

impl DripGame {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> DripGame {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.prg_ram_writable = false;
        DripGame {
            b,
            control: 0,
            reload: 0,
            counter: 0,
            counting: false,
            ext: vec![0; 0x800],
            last_tile: 0,
            irq: false,
        }
    }
}

impl Cartridge for DripGame {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x4800..0x5000).contains(&address) {
            return Some(0x64);
        }
        if (0x5000..0x6000).contains(&address) {
            return Some(0x40 | (0x3F << 8)); // buffer empty, not full
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0xC000 {
            self.ext[address as usize & 0x7FF] = value & 3;
            return;
        }
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0x0F;
        if register == 8 {
            self.reload = (self.reload & 0x7F00) | value as i64;
        } else if register == 9 {
            self.reload = (self.reload & 0xFF) | ((value as i64 & 0x7F) << 8);
            self.counter = self.reload;
            self.counting = value & 0x80 != 0;
            self.irq = false;
        } else if register == 0x0A {
            self.control = value;
            self.b
                .set_mirroring([VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B][(value & 3) as usize]);
            self.b.prg_ram_writable = value & 8 != 0;
        } else if register == 0x0B {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        } else if register >= 0x0C {
            self.b
                .map_chr((register as usize - 0x0C) * 2, 2, (value & 0x0F) as i64);
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter -= 1;
            if self.counter < 0 {
                self.counter = 0;
                self.counting = false;
                self.irq = true;
            }
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        let a10 = self.b.base_ciram_a10(address);
        if address & 0x3FF >= 0x3C0 {
            if self.control & 4 != 0 {
                return None;
            }
        } else if let Some(a10) = a10 {
            self.last_tile = ((a10 as usize) << 10) | (address as usize & 0x3FF);
        }
        a10
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address >= 0x2000 && self.control & 4 != 0 {
            return Some(self.ext[self.last_tile] * 0x55);
        }
        self.b.base_ppu_read(address, t)
    }
}

/// Mapper 29: Sealie Computing's RET-CUFROM (Glider): UNROM-like with 32 KiB
/// of CHR-RAM (INES_Mapper_029.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SealieRet {
    pub b: Banked,
}

impl SealieRet {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> SealieRet {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x8000];
        b.chr_writable = true;
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        let mut s = SealieRet { b };
        s.latch(0);
        s
    }

    fn latch(&mut self, value: u8) {
        self.b.map_prg(0, 16, ((value >> 2) & 7) as i64);
        self.b.map_chr(0, 8, (value & 3) as i64);
    }
}

impl Cartridge for SealieRet {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 413 (Super Russian Roulette): BATMAP-SRR-X, a CPLD and a speech ROM
/// (MAME's batlab.cpp, BSD-3): banks, an MMC3-like IRQ on PPU A12 (our guess
/// at its clock) and a serial pointer into the Misc ROM.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BatmapSrr {
    pub b: Banked,
    pub speech: Vec<u8>,
    /// Unbounded in the reference; only its bits below the ROM's size are read.
    pub pointer: u64,
    pub bank6000: usize,
    pub counter: u8,
    pub latch: u8,
    pub irq_enabled: bool,
    pub a12: u8,
    pub a12_ready: i64,
    pub irq: bool,
}

impl BatmapSrr {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> BatmapSrr {
        let speech = image.misc.clone();
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(2, 8, 3);
        b.map_prg(3, 8, 4);
        b.map_chr(0, 4, 0);
        b.map_chr(4, 4, 0x3D);
        BatmapSrr {
            b,
            speech,
            pointer: 0,
            bank6000: 0,
            counter: 0,
            latch: 0,
            irq_enabled: false,
            a12: 0,
            a12_ready: 0,
            irq: false,
        }
    }

    fn speech_byte(&mut self) -> u8 {
        self.pointer &= (self.speech.len() as u64).wrapping_sub(1);
        let value = self.speech[self.pointer as usize];
        self.pointer += 1;
        value
    }
}

impl Cartridge for BatmapSrr {
    banked_field!(b);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x4800..0x5000).contains(&address) || (0xC000..0xD000).contains(&address) {
            return Some(self.speech_byte() as u16);
        }
        if (0x5000..0x6000).contains(&address) {
            return Some(self.b.prg[address as usize - 0x4000] as u16);
        }
        if (0x6000..0x8000).contains(&address) {
            let offset = (self.bank6000 * 0x2000 + (address as usize & 0x1FFF)) % self.b.prg.len();
            return Some(self.b.prg[offset] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        if region == 0x8000 {
            self.latch = value;
        } else if region == 0x9000 {
            self.counter = 0;
        } else if region == 0xA000 {
            self.irq_enabled = false;
            self.irq = false;
        } else if region == 0xB000 {
            self.irq_enabled = true;
        } else if region == 0xC000 {
            self.pointer = (self.pointer << 1) | (value >> 7) as u64;
        } else if region >= 0xE000 {
            let (kind, bank) = (value >> 6, value & 0x3F);
            if kind == 0 {
                self.bank6000 = bank as usize;
            } else if kind == 3 {
                self.b.map_chr(0, 4, bank as i64);
            } else {
                self.b.map_prg(kind as usize - 1, 8, bank as i64);
            }
        }
    }

    fn ppu_address(&mut self, address: u16, t: i64) {
        if address & 0x1000 != 0 {
            if self.a12 == 0 {
                self.a12 = 1;
                if t >= self.a12_ready {
                    self.counter = if self.counter != 0 {
                        self.counter - 1
                    } else {
                        self.latch
                    };
                    if self.counter == 0 && self.irq_enabled {
                        self.irq = true;
                    }
                }
            }
        } else if self.a12 != 0 {
            self.a12 = 0;
            let c = self.b.cpu_cycle_ticks;
            self.a12_ready = (t.div_euclid(c) + 3) * c;
        }
    }
}
