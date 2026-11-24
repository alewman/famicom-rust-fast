// SPDX-License-Identifier: MIT AND BSD-3-Clause
// The Vs. protection bytes (vs_sequence, vs_xevious) are MAME's vsnes.cpp (BSD-3-Clause,
// Pierpaolo Prazzoli, Brad Oliver, Fabio Priuli), by way of the reference: notice in NOTICE.md.
//! Namco's mappers (`namco.py`): the 108 family (206, and its boards 76, 88,
//! 95 and 154), the 129/163 (19) and the 175/340 (210). No judge ROM exists
//! for any of them; they rest on sweeps against Mesen2. The 163's sound level
//! is the board's submapper (its mixing resistor), from the wiki's
//! measurements.
//!
//! The reference's 108 boards subclass `Namco108` and override `_map_chr`
//! (which its `__init__` reaches through `_update`), `cpu_write` and
//! `ciram_a10`; here one struct carries a [`Kind`].

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::{expansion_gain, Cartridge};
use super::ines::RomImage;
use crate::banked_field;

/// A full-volume N163 square, one channel, against a full-volume APU square
/// (dB), by NES 2.0 submapper (INES_Mapper_019/Submapper_table, the middle
/// of each range; submapper 0 the median of the nine games measured,
/// Namco_163_audio.wiki "Mixing"). Submappers 1 and 2: no sound.
pub fn n163_db(submapper: u8) -> Option<f64> {
    match submapper {
        0 => Some(18.0),
        3 => Some(12.0),
        4 => Some(16.5),
        5 => Some(18.75),
        _ => None,
    }
}

/// CPU cycles per channel update (Namco_163_audio.wiki).
pub const N163_UPDATE: u32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// Mapper 206.
    Namco108,
    /// Mapper 76: NAMCOT-3446 (Megami Tensei): four 2 KiB CHR banks from R2-R5.
    Namco3446,
    /// Mappers 88 and 154: PPU A12 as CHR A16; 154 adds the one-screen select.
    Namco3433 { one_screen: bool },
    /// Mapper 95: NAMCOT-3425 (Dragon Buster): CHR A15 drives CIRAM A10.
    Namco3425,
}

/// Mapper 206: Namco 108/109/118/119 and Tengen MIMIC-1, the MMC3's
/// predecessor (INES_Mapper_206.wiki), and its boards 76, 88, 95, 154
/// (INES_Mapper_076.wiki, _088, _154, _095).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Namco108 {
    pub b: Banked,
    pub kind: Kind,
    pub unbanked: bool,
    pub bank_select: usize,
    pub registers: [u8; 8],
}

impl Namco108 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64, kind: Kind) -> Namco108 {
        let unbanked = image.board.submapper == 1;
        let mut m = Namco108 {
            b: Banked::new(image, cpu_cycle_ticks),
            kind,
            unbanked,
            bank_select: 0,
            registers: [0; 8],
        };
        m.b.prg_ram_enabled = false;
        m.update();
        if let Kind::Namco3433 { one_screen: true } = m.kind {
            m.b.set_mirroring(SCREEN_A);
        }
        m
    }

    fn update(&mut self) {
        let r = self.registers;
        if self.unbanked {
            self.b.map_prg(0, 32, 0);
        } else {
            let last = (self.b.prg.len() / 0x2000) as i64 - 1;
            self.b.map_prg(0, 8, (r[6] & 0x0F) as i64);
            self.b.map_prg(1, 8, (r[7] & 0x0F) as i64);
            self.b.map_prg(2, 8, last - 1);
            self.b.map_prg(3, 8, last);
        }
        self.map_chr(&r);
    }

    fn map_chr(&mut self, r: &[u8; 8]) {
        match self.kind {
            Kind::Namco3446 => {
                for i in 0..4 {
                    self.b.map_chr(2 * i, 2, (r[2 + i] & 0x3F) as i64);
                }
            }
            Kind::Namco3433 { .. } => {
                self.b.map_chr(0, 2, ((r[0] & 0x3E) >> 1) as i64);
                self.b.map_chr(2, 2, ((r[1] & 0x3E) >> 1) as i64);
                for i in 0..4 {
                    self.b.map_chr(4 + i, 1, ((r[2 + i] & 0x3F) | 0x40) as i64);
                }
            }
            Kind::Namco108 | Kind::Namco3425 => {
                self.b.map_chr(0, 2, ((r[0] & 0x3E) >> 1) as i64);
                self.b.map_chr(2, 2, ((r[1] & 0x3E) >> 1) as i64);
                for i in 0..4 {
                    self.b.map_chr(4 + i, 1, (r[2 + i] & 0x3F) as i64);
                }
            }
        }
    }
}

impl Cartridge for Namco108 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x8000..0xA000).contains(&address) {
            if address & 1 != 0 {
                self.registers[self.bank_select] = value;
                self.update();
            } else {
                self.bank_select = (value & 7) as usize;
            }
        }
        if let Kind::Namco3433 { one_screen: true } = self.kind {
            if address >= 0x8000 {
                self.b.set_mirroring(if value & 0x40 != 0 {
                    SCREEN_B
                } else {
                    SCREEN_A
                });
            }
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        if self.kind == Kind::Namco3425 {
            return Some((self.registers[((address >> 11) & 1) as usize] >> 5) & 1);
        }
        self.b.base_ciram_a10(address)
    }
}

/// The protection chips' decoding: (the addresses decoded, as a mask and a
/// value; the 32 bytes), by NES 2.0 Vs. hardware type.
pub fn vs_sequence(protection: u8) -> (u16, u16, [u8; 32]) {
    match protection {
        // Vs. Atari R.B.I. Baseball: $5600-$5601 and $5E00-$5E01 ("address
        // fully decoded except A11").
        1 => (
            0xF7FE,
            0x5600,
            [
                0xff, 0xfd, 0xf5, 0xf4, 0xb4, 0xb4, 0xa6, 0x2e, 0x2f, 0x6f, 0x6f, 0x7d, 0xd5, 0xd4,
                0x94, 0x94, 0x86, 0x2e, 0x2f, 0x6f, 0x6b, 0x79, 0xd1, 0xd0, 0x92, 0x92, 0x8d, 0x65,
                0x64, 0x34, 0xb0, 0xa2,
            ],
        ),
        // Vs. T.K.O. Boxing (chip "128"): $5E00-$5E01.
        2 => (
            0xFFFE,
            0x5E00,
            [
                0xff, 0xbf, 0xb7, 0x97, 0x97, 0x17, 0x57, 0x4f, 0x6f, 0x6b, 0xeb, 0xa9, 0xb1, 0x90,
                0x94, 0x14, 0x56, 0x4e, 0x6f, 0x6b, 0xeb, 0xa9, 0xb1, 0x90, 0xd4, 0x5c, 0x3e, 0x26,
                0x87, 0x83, 0x13, 0x00,
            ],
        ),
        _ => panic!("no protection sequence for hardware type {protection}"),
    }
}

/// Vs. Super Xevious: $54FF returns $05 and flips the bit; the others answer by it.
pub fn vs_xevious(address: u16) -> Option<(u8, u8)> {
    match address {
        0x5678 => Some((0x00, 0x01)),
        0x578F => Some((0xD1, 0x89)),
        0x5567 => Some((0x3E, 0x37)),
        _ => None,
    }
}

/// A Namco 108 daughterboard of the Vs. System with a protection chip in
/// $4020-$5FFF (NES 2.0 Vs. hardware types 1 to 3; Vs._System.wiki "Copy
/// Protection": "a few third-party games added an extra IC as well").
///
/// What the chips answer is known only as the bytes each game expects:
/// MAME's vsnes.cpp (BSD-3; emulator-derived) holds them, and says of chip
/// "127" that it "uses a LFSR with taps: 23, 18, 0" whose read-back shuffle
/// is unknown. Types 1 and 2 are a sequence of 32 bytes: a read of an even
/// address returns 0 and rewinds it, a read of an odd address returns the
/// next byte. Type 3 is four fixed addresses, one of which flips a bit that
/// picks between two answers at the other three.
///
/// The reference subclasses `Namco108`; here the board is a field, and every
/// method the subclass does not override goes to it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct VsProtected108 {
    pub namco: Namco108,
    pub protection: u8,
    pub protection_index: u64,
}

impl VsProtected108 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> VsProtected108 {
        let protection = image.board.vs_hardware;
        VsProtected108 {
            namco: Namco108::new(image, cpu_cycle_ticks, Kind::Namco108),
            protection,
            protection_index: 0,
        }
    }
}

impl Cartridge for VsProtected108 {
    fn banked(&self) -> Option<&Banked> {
        Some(&self.namco.b)
    }
    fn banked_mut(&mut self) -> Option<&mut Banked> {
        Some(&mut self.namco.b)
    }

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        let b = &self.namco.b;
        if address >= 0x8000 {
            let a = address as usize;
            return Some(b.prg[b.prg_offsets[(a >> 13) & 3] + (a & 0x1FFF)] as u16);
        }
        if self.protection == 3 {
            if address == 0x54FF {
                self.protection_index ^= 1;
                return Some(0x05);
            }
            return vs_xevious(address).map(|answers| {
                (if self.protection_index != 0 {
                    answers.1
                } else {
                    answers.0
                }) as u16
            });
        }
        let (mask, base, data) = vs_sequence(self.protection);
        if address & mask != base {
            return None;
        }
        if address & 1 == 0 {
            self.protection_index = 0;
            return Some(0);
        }
        let value = data[(self.protection_index & 0x1F) as usize];
        self.protection_index += 1;
        Some(value as u16)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        self.namco.cpu_write(address, value, t);
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        self.namco.ciram_a10(address)
    }
}

/// Mapper 206: the plain board, or a Vs. System daughterboard with its chip.
pub fn namco108_for(image: RomImage, cpu_cycle_ticks: i64) -> Box<dyn Cartridge> {
    if image.board.console == 1 && matches!(image.board.vs_hardware, 1..=3) {
        return Box::new(VsProtected108::new(image, cpu_cycle_ticks));
    }
    Box::new(Namco108::new(image, cpu_cycle_ticks, Kind::Namco108))
}

/// Mapper 19: Namco 129/163: 1 KiB CHR banks, nametables from CIRAM or
/// CHR-ROM, a cycle IRQ, and the wavetable sound (INES_Mapper_019.wiki,
/// Namco_163_audio.wiki). The wiki does not say whether the IRQ counter counts
/// while disabled; here only while enabled, as MAME's BSD-3 namcot.cpp does and
/// as Mesen2 2.1.1 does, OBSERVED closed-box (the reference's
/// `scripts/provenance_probes.py n163-counter-disabled`). Two emulators, no
/// hardware test.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Namco163 {
    pub b: Banked,
    #[serde(with = "serde_big_array::BigArray")]
    pub chip_ram: [u8; 0x80],
    pub chip_address: u8,
    pub protect: u8,
    pub counter: u16,
    pub irq_enabled: bool,
    pub nametables: [u8; 4],
    pub nametable_offsets: [usize; 4],
    pub expansion_audio: bool,
    pub gain: f64,
    /// $E000 D6 clear.
    pub sound_enabled: bool,
    pub update: u32,
    /// The next to update, 7 (channel 8) downwards.
    pub channel: usize,
    pub irq: bool,
    /// The reference's `audio_out.expansion`.
    pub expansion: f64,
}

impl Namco163 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Namco163 {
        let submapper = image.board.submapper;
        let mut m = Namco163 {
            b: Banked::new(image, cpu_cycle_ticks),
            chip_ram: [0; 0x80],
            chip_address: 0,
            protect: 0,
            counter: 0,
            irq_enabled: false,
            nametables: [0xE0, 0xE1, 0xE0, 0xE1],
            nametable_offsets: [0; 4],
            expansion_audio: n163_db(submapper).is_some(),
            // A full-volume square: samples 0 and 15 at volume 15, -120 to +105.
            gain: expansion_gain(n163_db(submapper).unwrap_or(0.0), 225),
            sound_enabled: true,
            update: N163_UPDATE,
            channel: 7,
            irq: false,
            expansion: 0.0,
        };
        let last = (m.b.prg.len() / 0x2000) as i64 - 1;
        m.b.map_prg(3, 8, last);
        m
    }

    fn update_channel(&mut self) {
        let ram = &mut self.chip_ram;
        let channel = self.channel;
        let base = 0x40 + 8 * channel;
        let frequency =
            ((ram[base + 4] as u32 & 3) << 16) | (ram[base + 2] as u32) << 8 | ram[base] as u32;
        let phase =
            (ram[base + 5] as u32) << 16 | (ram[base + 3] as u32) << 8 | ram[base + 1] as u32;
        let length = 256 - (ram[base + 4] as u32 & 0xFC);
        let phase = (phase + frequency) % (length << 16);
        ram[base + 5] = (phase >> 16) as u8;
        ram[base + 3] = ((phase >> 8) & 0xFF) as u8;
        ram[base + 1] = (phase & 0xFF) as u8;
        let index = (((phase >> 16) + ram[base + 6] as u32) & 0xFF) as usize;
        let sample = ((ram[index >> 1] >> ((index & 1) << 2)) & 0x0F) as i64;
        let volume = (ram[base + 7] & 0x0F) as i64;
        self.expansion = ((sample - 8) * volume) as f64 * self.gain;
        let lowest = 7 - ((ram[0x7F] as usize >> 4) & 7);
        self.channel = if channel > lowest { channel - 1 } else { 7 };
    }

    fn step_chip_address(&mut self) {
        if self.chip_address & 0x80 != 0 {
            self.chip_address = 0x80 | (self.chip_address.wrapping_add(1) & 0x7F);
        }
    }
}

impl Cartridge for Namco163 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn expansion_audio(&self) -> bool {
        self.expansion_audio
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn expansion(&self) -> f64 {
        self.expansion
    }

    fn m2(&mut self, _t: i64) {
        if self.irq_enabled && self.counter < 0x7FFF {
            self.counter += 1;
            if self.counter == 0x7FFF {
                self.irq = true;
            }
        }
        self.update -= 1;
        if self.update == 0 {
            self.update = N163_UPDATE;
            if self.sound_enabled {
                self.update_channel();
            }
        }
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x6000 {
            return self.b.base_cpu_read(address, t);
        }
        let region = address & 0xF800;
        if region == 0x4800 {
            let value = self.chip_ram[(self.chip_address & 0x7F) as usize];
            self.step_chip_address();
            return Some(value as u16);
        }
        if region == 0x5000 {
            return Some(self.counter & 0xFF);
        }
        if region == 0x5800 {
            return Some((self.counter >> 8) | if self.irq_enabled { 0x80 } else { 0 });
        }
        None
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            if address >= 0x6000 {
                let window = (address >> 11) & 3;
                if self.protect & 0xF0 == 0x40 && self.protect & (1 << window) == 0 {
                    self.b.base_cpu_write(address, value, t);
                }
                return;
            }
            let region = address & 0xF800;
            if region == 0x4800 {
                self.chip_ram[(self.chip_address & 0x7F) as usize] = value;
                self.step_chip_address();
            } else if region == 0x5000 {
                self.counter = (self.counter & 0x7F00) | value as u16;
                self.irq = false;
            } else if region == 0x5800 {
                self.counter = (self.counter & 0xFF) | ((value & 0x7F) as u16) << 8;
                self.irq_enabled = value & 0x80 != 0;
                self.irq = false;
            }
            return;
        }
        let region = address & 0xF800;
        if region < 0xC000 {
            self.b
                .map_chr(((region - 0x8000) >> 11) as usize, 1, value as i64);
        } else if region < 0xE000 {
            let slot = ((region - 0xC000) >> 11) as usize;
            self.nametables[slot] = value;
            self.nametable_offsets[slot] = (value as usize * 0x400) % self.b.chr.len();
        } else if region == 0xF800 {
            self.protect = value;
            self.chip_address = value;
        } else {
            if region == 0xE000 {
                self.sound_enabled = value & 0x40 == 0;
                if !self.sound_enabled {
                    self.expansion = 0.0;
                }
            }
            self.b
                .map_prg(((region - 0xE000) >> 11) as usize, 8, (value & 0x3F) as i64);
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        let value = self.nametables[((address >> 10) & 3) as usize];
        if value >= 0xE0 {
            Some(value & 1)
        } else {
            None
        }
    }

    fn ppu_read(&mut self, address: u16, _t: i64) -> Option<u8> {
        let a = address as usize;
        if address < 0x2000 {
            return Some(self.b.chr[self.b.chr_offsets[a >> 10] + (a & 0x3FF)]);
        }
        Some(self.b.chr[self.nametable_offsets[(a >> 10) & 3] + (a & 0x3FF)])
    }

    /// CHR-ROM ignores writes; the commercial boards have no CHR-RAM.
    fn ppu_write(&mut self, _address: u16, _value: u8, _t: i64) {}

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.b.battery {
            let mut data = self.b.prg_ram.clone();
            data.extend_from_slice(&self.chip_ram);
            Some(data)
        } else {
            None
        }
    }

    fn restore(&mut self, data: &[u8]) {
        let split = self.b.prg_ram.len();
        self.b.prg_ram.copy_from_slice(&data[..split]);
        self.chip_ram.copy_from_slice(&data[split..]);
    }
}

/// 0 one-screen A, 1 vertical, 2 one-screen B, 3 horizontal.
const ARRANGEMENTS_340: [Mirroring; 4] = [SCREEN_A, VERTICAL, SCREEN_B, HORIZONTAL];

/// Mapper 210: Namco 175 (210.1) and 340 (210.2), the 163's banking without
/// its extras (INES_Mapper_210.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Namco175 {
    pub b: Banked,
    pub is_340: bool,
}

impl Namco175 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Namco175 {
        let submapper = image.board.submapper;
        let is_340 = submapper == 2 || (submapper != 1 && !image.board.battery);
        let mut m = Namco175 {
            b: Banked::new(image, cpu_cycle_ticks),
            is_340,
        };
        m.b.prg_ram_enabled = false;
        let last = (m.b.prg.len() / 0x2000) as i64 - 1;
        m.b.map_prg(3, 8, last);
        m
    }
}

impl Cartridge for Namco175 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF800;
        if region < 0xC000 {
            self.b
                .map_chr(((region - 0x8000) >> 11) as usize, 1, value as i64);
        } else if region == 0xC000 {
            if !self.is_340 {
                self.b.prg_ram_enabled = !self.b.prg_ram.is_empty() && value & 1 != 0;
            }
        } else if (0xE000..0xF800).contains(&region) {
            self.b
                .map_prg(((region - 0xE000) >> 11) as usize, 8, (value & 0x3F) as i64);
            if region == 0xE000 && self.is_340 {
                self.b
                    .set_mirroring(ARRANGEMENTS_340[(value >> 6) as usize]);
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
        19 => Box::new(Namco163::new(image, t)),
        76 => Box::new(Namco108::new(image, t, Kind::Namco3446)),
        88 | 154 => {
            let one_screen = image.board.mapper == 154;
            Box::new(Namco108::new(image, t, Kind::Namco3433 { one_screen }))
        }
        95 => Box::new(Namco108::new(image, t, Kind::Namco3425)),
        206 => namco108_for(image, t),
        210 => Box::new(Namco175::new(image, t)),
        _ => return Err(image),
    })
}
