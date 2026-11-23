// SPDX-License-Identifier: MIT
//! Sunsoft's mappers (`sunsoft.py`): Sunsoft-3 (67), Sunsoft-4 (68), FME-7,
//! 5A and 5B (69), and the discrete Sunsoft-1 (184) and Sunsoft-2 (89, 93).
//! 69 is judged by Holy Mapperel and tepples's `fme7acktest` and
//! `fme7ramtest`; the 5B's sound is ay8910-python's YM2149 variant (here
//! ay8910-rust, its transcription), judged there against the AY-3-8910's die
//! model; the others rest on sweeps against Mesen2, a detector.
//!
//! The reference's `Sunsoft4Dual` subclasses `Sunsoft4`; here one struct
//! carries the dual cartridge's state as an `Option`.

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;
use ay8910_rust::{Levels, PSG, SUNSOFT_5B};

/// One 5B channel's DAC level (ay8910-rust's `mix()`: 0.0-1.0 a channel) in
/// the APU mixer's units. rainwarrior's `db_5b` on an AV Famicom measured the
/// 5B's square at volume 12 as 1.3 dB below a full-volume APU square
/// (nesdev-forum/17741.html, hardware-captured on one console and one
/// cartridge); fitted so db_5b measures that through the board
/// (the reference's scripts/expansion_levels.py). The amplifier's compression
/// above one channel at volume 14 and its filtering (Sunsoft_5B_audio.wiki
/// "Output": "ongoing") are not modelled: no measurement gives the curve.
pub const SUNSOFT_5B_GAIN: f64 = 0.364;

const ARRANGEMENTS: [Mirroring; 4] = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];

/// Mapper 69: a command register ($8000) and its parameter ($A000)
/// (Sunsoft_FME-7.wiki).
///
/// The 5B adds a YM2149F (Sunsoft_5B_audio.wiki): $C000 selects its register,
/// $E000 writes it, and it is clocked by M2, one input clock a CPU cycle
/// (the variant halves it). Neither the header nor nes20db tells a 5B from an
/// FME-7 or 5A, which have no sound and whose games never write there, so
/// every mapper 69 cartridge carries the chip.
///
/// Forced by Rust: the reference sends a new level to the mixer when the
/// chip's `levels()` is a different object from the last it sent (`is not`);
/// [`Levels`] is a value here, so the test is `!=`. The chip makes a new
/// object only when it recomputes its levels, and the level sent is a function
/// of them, so the mixer gets the same level on the same cycle either way.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fme7 {
    pub b: Banked,
    pub command: u8,
    /// Command 8.
    pub low_bank: u8,
    pub counter: u16,
    pub counting: bool,
    pub irq_enabled: bool,
    pub irq: bool,
    pub sound: PSG,
    /// The levels last sent to the mixer (the reference's `_levels`).
    pub levels: Levels,
    /// The level on the cartridge's audio pin, in the mixer's units.
    pub expansion: f64,
}

impl Fme7 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fme7 {
        let mut m = Fme7 {
            b: Banked::new(image, cpu_cycle_ticks),
            command: 0,
            low_bank: 0,
            counter: 0,
            counting: false,
            irq_enabled: false,
            irq: false,
            sound: PSG::with_record(SUNSOFT_5B, false),
            levels: [0; 3],
            expansion: 0.0,
        };
        m.levels = m.sound.levels();
        let last = (m.b.prg.len() / 0x2000) as i64 - 1;
        m.b.map_prg(3, 8, last);
        m
    }

    fn parameter(&mut self, command: u8, value: u8) {
        if command < 8 {
            self.b.map_chr(command as usize, 1, value as i64);
        } else if command == 8 {
            self.low_bank = value;
        } else if command < 0x0C {
            self.b
                .map_prg((command - 9) as usize, 8, (value & 0x3F) as i64);
        } else if command == 0x0C {
            self.b.set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
        } else if command == 0x0D {
            self.irq_enabled = value & 1 != 0;
            self.counting = value & 0x80 != 0;
            self.irq = false;
        } else if command == 0x0E {
            self.counter = (self.counter & 0xFF00) | value as u16;
        } else {
            self.counter = (self.counter & 0x00FF) | (value as u16) << 8;
        }
    }
}

impl Cartridge for Fme7 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn expansion_audio(&self) -> bool {
        true
    }

    fn expansion(&self) -> f64 {
        self.expansion
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn m2(&mut self, _t: i64) {
        let sound = &mut self.sound;
        sound.tick();
        let levels = sound.levels();
        if levels != self.levels {
            self.levels = levels;
            self.expansion = sound.mix() * SUNSOFT_5B_GAIN;
        }
        if self.counting {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter == 0xFFFF && self.irq_enabled {
                self.irq = true;
            }
        }
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            let bank = self.low_bank as usize;
            if bank & 0x40 == 0 {
                let a = address as usize;
                return Some(
                    self.b.prg[(bank & 0x3F) * 0x2000 % self.b.prg.len() + (a & 0x1FFF)] as u16,
                );
            }
            if bank & 0x80 == 0 || self.b.prg_ram.is_empty() {
                return None;
            }
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            if self.low_bank & 0xC0 == 0xC0 {
                self.b.base_cpu_write(address, value, t);
            }
            return;
        }
        let region = address & 0xE000;
        if region == 0x8000 {
            self.command = value & 0x0F;
        } else if region == 0xA000 {
            let command = self.command;
            self.parameter(command, value);
        } else if region == 0xC000 {
            self.sound.select(value);
        } else {
            self.sound.write(value);
        }
    }
}

/// Mappers 89 (Sunsoft-2 on the Sunsoft-3 board) and 93 (on the Sunsoft-3R)
/// (INES_Mapper_089.wiki, _093.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sunsoft2 {
    pub b: Banked,
    pub sunsoft_3r: bool,
    pub chr_enabled: bool,
}

impl Sunsoft2 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sunsoft2 {
        let sunsoft_3r = image.board.mapper == 93;
        let mut m = Sunsoft2 {
            b: Banked::new(image, cpu_cycle_ticks),
            sunsoft_3r,
            chr_enabled: true,
        };
        m.b.map_prg(0, 16, 0);
        let last = (m.b.prg.len() / 0x4000) as i64 - 1;
        m.b.map_prg(2, 16, last);
        m.b.map_chr(0, 8, 0);
        if !m.sunsoft_3r {
            m.b.set_mirroring(SCREEN_A);
        }
        m
    }
}

impl Cartridge for Sunsoft2 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = self.b.bus_conflict(address, value);
        self.b.map_prg(0, 16, ((value >> 4) & 7) as i64);
        if self.sunsoft_3r {
            self.chr_enabled = value & 1 != 0;
        } else {
            self.b
                .map_chr(0, 8, (((value >> 4) & 8) | (value & 7)) as i64);
            self.b
                .set_mirroring(if value & 8 != 0 { SCREEN_B } else { SCREEN_A });
        }
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && !self.chr_enabled {
            return None;
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x2000 || self.chr_enabled {
            self.b.base_ppu_write(address, value, t);
        }
    }
}

/// Mapper 184: Sunsoft-1 on the Sunsoft-K board: two 4 KiB CHR banks from
/// $6000-$7FFF (INES_Mapper_184.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sunsoft1 {
    pub b: Banked,
}

impl Sunsoft1 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sunsoft1 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 32, 0);
        b.map_chr(0, 4, 0);
        b.map_chr(4, 4, 4);
        Sunsoft1 { b }
    }
}

impl Cartridge for Sunsoft1 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.b.map_chr(0, 4, (value & 7) as i64);
            self.b.map_chr(4, 4, (4 | ((value >> 4) & 3)) as i64);
        }
    }
}

/// Mapper 67: Sunsoft-3: 16 KiB PRG and the last fixed, four 2 KiB CHR
/// banks, a cycle counter (INES_Mapper_067.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sunsoft3 {
    pub b: Banked,
    pub counter: u16,
    pub counting: bool,
    /// The $C800 toggle.
    pub high_next: bool,
    pub irq: bool,
}

impl Sunsoft3 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sunsoft3 {
        let mut m = Sunsoft3 {
            b: Banked::new(image, cpu_cycle_ticks),
            counter: 0,
            counting: false,
            high_next: true,
            irq: false,
        };
        let last = (m.b.prg.len() / 0x4000) as i64 - 1;
        m.b.map_prg(2, 16, last);
        m
    }
}

impl Cartridge for Sunsoft3 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter == 0xFFFF {
                self.counting = false;
                self.irq = true;
            }
        }
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0xF800;
        if register & 0x0800 == 0 {
            self.irq = false; // $8000 and its mirrors
        } else if register <= 0xB800 {
            self.b
                .map_chr((((register >> 12) - 8) * 2) as usize, 2, value as i64);
        } else if register == 0xC800 {
            if self.high_next {
                self.counter = (self.counter & 0x00FF) | (value as u16) << 8;
            } else {
                self.counter = (self.counter & 0xFF00) | value as u16;
            }
            self.high_next = !self.high_next;
        } else if register == 0xD800 {
            self.counting = value & 0x10 != 0;
            self.high_next = true;
        } else if register == 0xE800 {
            self.b.set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
        } else {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        }
    }
}

/// M2 cycles the Nantettatte!! Baseball licensing timer runs
/// (INES_Mapper_068.wiki: "most likely exactly 107520 = 1024 x 105").
pub const LICENSING_CYCLES: u32 = 1024 * 105;

/// Mapper 68.1's state: the Sunsoft Dual Cartridge System (Nantettatte!!
/// Baseball; INES_Mapper_068.wiki, NES_2.0_submappers.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Dual {
    pub internal_banks: i64,
    pub cassette_banks: i64,
    pub from_cassette: bool,
    pub timer: u32,
}

/// Mapper 68: Sunsoft-4: 16 KiB PRG and the last fixed, 2 KiB CHR banks,
/// CHR-ROM nametables (INES_Mapper_068.wiki). With `dual`, 68.1.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sunsoft4 {
    pub b: Banked,
    pub rom_nametables: bool,
    /// 1 KiB banks.
    pub nametable_banks: [usize; 2],
    pub dual: Option<Dual>,
}

impl Sunsoft4 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Sunsoft4 {
        let mut m = Sunsoft4 {
            b: Banked::new(image, cpu_cycle_ticks),
            rom_nametables: false,
            nametable_banks: [0x80, 0x80],
            dual: None,
        };
        m.b.prg_ram_enabled = false;
        let last = (m.b.prg.len() / 0x4000) as i64 - 1;
        m.b.map_prg(2, 16, last);
        m
    }

    pub fn dual(image: RomImage, cpu_cycle_ticks: i64) -> Sunsoft4 {
        let internal_banks = (image.prg.len().min(0x20000) / 0x4000) as i64;
        let cassette_banks = (image.prg.len() / 0x4000) as i64 - internal_banks;
        let mut m = Sunsoft4::new(image, cpu_cycle_ticks);
        m.dual = Some(Dual {
            internal_banks,
            cassette_banks,
            from_cassette: false,
            timer: 0,
        });
        m.b.map_prg(2, 16, internal_banks - 1);
        m
    }

    /// `Sunsoft4.cpu_write`.
    fn sunsoft4_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        if region <= 0xB000 {
            self.b
                .map_chr(((region - 0x8000) >> 11) as usize, 2, value as i64);
        } else if region <= 0xD000 {
            self.nametable_banks[((region >> 12) & 1) as usize] = (value | 0x80) as usize;
        } else if region == 0xE000 {
            self.b.set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
            self.rom_nametables = value & 0x10 != 0;
        } else {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
            self.b.prg_ram_enabled = value & 0x10 != 0 && !self.b.prg_ram.is_empty();
        }
    }
}

impl Cartridge for Sunsoft4 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        self.dual.is_some()
    }

    fn m2(&mut self, _t: i64) {
        if let Some(dual) = &mut self.dual {
            if dual.timer != 0 {
                dual.timer -= 1;
            }
        }
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if let Some(dual) = &self.dual {
            if (0x8000..0xC000).contains(&address) && dual.from_cassette && dual.timer == 0 {
                return None;
            }
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.dual.is_none() {
            self.sunsoft4_write(address, value, t);
            return;
        }
        if (0x6000..0x8000).contains(&address) && !self.b.prg_ram_enabled {
            self.dual.as_mut().unwrap().timer = LICENSING_CYCLES;
        }
        self.sunsoft4_write(address, value, t);
        if address & 0xF000 == 0xF000 {
            let dual = self.dual.as_mut().unwrap();
            dual.from_cassette = value & 0x08 == 0;
            let bank = if dual.from_cassette && dual.cassette_banks != 0 {
                dual.internal_banks + ((value & 7) as i64).rem_euclid(dual.cassette_banks)
            } else {
                (value & 7) as i64
            };
            self.b.map_prg(0, 16, bank);
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        let a10 = self.b.base_ciram_a10(address);
        if self.rom_nametables {
            None
        } else {
            a10
        }
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address >= 0x2000 {
            // Only with ROM nametables: CIRAM answers otherwise.
            let a10 = self.b.base_ciram_a10(address).unwrap_or(0) as usize;
            let bank = self.nametable_banks[a10];
            let chr = &self.b.chr;
            return Some(chr[(bank * 0x400 + (address as usize & 0x3FF)) % chr.len()]);
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 {
            self.b.base_ppu_write(address, value, t);
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
        67 => Box::new(Sunsoft3::new(image, t)),
        68 => {
            // `mapper_68`: the board by submapper.
            if image.board.submapper == 1 {
                Box::new(Sunsoft4::dual(image, t))
            } else {
                Box::new(Sunsoft4::new(image, t))
            }
        }
        69 => Box::new(Fme7::new(image, t)),
        89 | 93 => Box::new(Sunsoft2::new(image, t)),
        184 => Box::new(Sunsoft1::new(image, t)),
        _ => return Err(image),
    })
}
