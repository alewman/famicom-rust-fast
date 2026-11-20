// SPDX-License-Identifier: MIT
//! MMC5 (mapper 5): its banking, ExRAM, scanline detection and vertical
//! split (`mmc5.py`; MMC5.wiki throughout; the sound is `mmc5_sound.rs`).
//!
//! The MMC5 watches the PPU's reads (/RD), not its address phases: every
//! read reaches the cartridge, nametable-space reads through `ciram_a10`
//! and pattern reads through `ppu_read`.

use super::banked::Banked;
use super::connector::Cartridge;
use super::ines::RomImage;
use super::mmc5_sound::Mmc5Sound;
use crate::banked_field;

fn sprite_slot(slot: i64) -> bool {
    (32..40).contains(&slot)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc5 {
    pub b: Banked,
    #[serde(with = "serde_big_array::BigArray")]
    pub exram: [u8; 0x400],
    pub prg_mode: u8,
    /// $5113-$5117.
    pub prg_registers: [u8; 5],
    pub protect: [u8; 2],
    pub exram_mode: u8,
    pub nametables: [u8; 4],
    pub fill_tile: u8,
    pub fill_attribute: u8,
    pub chr_mode: u8,
    /// $5130.
    pub chr_high: u16,
    /// 10-bit, $5120-$512B.
    pub chr_registers: [u16; 12],
    pub last_chr_set_b: bool,
    pub sprites_8x16: bool,
    pub substitutions: bool,
    pub split_control: u8,
    pub split_scroll: u8,
    pub split_bank: u8,
    pub irq_target: u8,
    pub irq_enabled: bool,
    pub irq_pending: bool,
    pub in_frame: bool,
    pub scanline: i64,
    pub multiplicand: u8,
    pub multiplier: u8,
    pub last_read: i64,
    pub matches: u32,
    /// Reads since the scanline's third matching read.
    pub fetch: i64,
    pub read_since_m2: bool,
    pub idle: u32,
    pub ext_tile: u8,
    pub split_tile: usize,
    pub in_split: bool,
    /// $6000-$FFFF in 8 KiB windows: (is ROM, offset), or None (open bus).
    pub prg_windows: [Option<(bool, usize)>; 5],
    pub chr_a: [usize; 8],
    pub chr_b: [usize; 8],
    pub sound: Mmc5Sound,
    pub irq: bool,
}

impl Mmc5 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc5 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let mut mmc5 = Mmc5 {
            b,
            exram: [0; 0x400],
            prg_mode: 3,
            prg_registers: [0, 0, 0, 0, 0xFF],
            protect: [1, 2],
            exram_mode: 3,
            nametables: [0; 4],
            fill_tile: 0,
            fill_attribute: 0,
            chr_mode: 3,
            chr_high: 0,
            chr_registers: [0; 12],
            last_chr_set_b: false,
            sprites_8x16: false,
            substitutions: false,
            split_control: 0,
            split_scroll: 0,
            split_bank: 0,
            irq_target: 0,
            irq_enabled: false,
            irq_pending: false,
            in_frame: false,
            scanline: 0,
            multiplicand: 0xFF,
            multiplier: 0xFF,
            last_read: -1,
            matches: 0,
            fetch: 0,
            read_since_m2: false,
            idle: 0,
            ext_tile: 0,
            split_tile: 0,
            in_split: false,
            prg_windows: [None; 5],
            chr_a: [0; 8],
            chr_b: [0; 8],
            sound: Mmc5Sound::new(),
            irq: false,
        };
        mmc5.update_prg();
        mmc5.update_chr();
        mmc5
    }

    // -- PRG -----------------------------------------------------------------

    /// PRG-RAM by bank: D2 picks one of two chips, D1-D0 an 8 KiB page.
    fn ram_offset(&self, bank: usize) -> Option<usize> {
        let size = self.b.prg_ram.len();
        if size == 0 {
            return None;
        }
        let chip = (bank >> 2) & 1;
        let two_chips = size == 0x4000 || size == 0x10000;
        let chip_size = if two_chips { size / 2 } else { size };
        if chip != 0 && !two_chips {
            return None;
        }
        Some(chip * chip_size + ((bank & 3) * 0x2000) % chip_size)
    }

    fn update_prg(&mut self) {
        let r = self.prg_registers;
        let mut windows = [None; 5];
        windows[0] = self
            .ram_offset((r[0] & 0x0F) as usize)
            .map(|ram| (false, ram));
        let prg_len = self.b.prg.len();
        let layout: [(usize, usize); 4] = match self.prg_mode {
            0 => [(4, 4); 4],
            1 => [(2, 2), (2, 2), (4, 2), (4, 2)],
            2 => [(2, 2), (2, 2), (3, 1), (4, 1)],
            _ => [(1, 1), (2, 1), (3, 1), (4, 1)],
        };
        for (window, &(register, units)) in layout.iter().enumerate() {
            let value = r[register] as usize;
            let bank = ((value & 0x7F) & !(units - 1)) | (window & (units - 1));
            if register == 4 || value & 0x80 != 0 {
                windows[window + 1] = Some((true, bank * 0x2000 % prg_len));
            } else {
                windows[window + 1] = self.ram_offset(bank & 0x0F).map(|ram| (false, ram));
            }
        }
        self.prg_windows = windows;
    }

    fn snoop(&mut self, address: u16, value: u8) {
        if address == 0x2000 {
            self.sprites_8x16 = value & 0x20 != 0;
        } else if address == 0x2001 {
            let enabled = value & 0x18 != 0;
            if !enabled {
                self.leave_frame();
            }
            self.substitutions = enabled;
        } else if address == 0x4014 {
            self.leave_frame();
        }
    }

    fn register(&mut self, address: u16, value: u8) {
        match address {
            0x5100 => {
                self.prg_mode = value & 3;
                self.update_prg();
            }
            0x5101 => {
                self.chr_mode = value & 3;
                self.update_chr();
            }
            0x5102 | 0x5103 => self.protect[(address - 0x5102) as usize] = value & 3,
            0x5104 => self.exram_mode = value & 3,
            0x5105 => self.nametables = [0, 2, 4, 6].map(|shift| (value >> shift) & 3),
            0x5106 => self.fill_tile = value,
            0x5107 => self.fill_attribute = (value & 3) * 0x55,
            0x5113..=0x5117 => {
                self.prg_registers[(address - 0x5113) as usize] = value;
                self.update_prg();
            }
            0x5120..=0x512B => {
                self.chr_registers[(address - 0x5120) as usize] = self.chr_register(value);
                self.last_chr_set_b = address >= 0x5128;
                self.update_chr();
            }
            0x5130 => self.chr_high = (value & 3) as u16,
            0x5200 => self.split_control = value,
            0x5201 => self.split_scroll = value,
            0x5202 => self.split_bank = value,
            0x5203 => self.irq_target = value,
            0x5204 => {
                self.irq_enabled = value & 0x80 != 0;
                self.update_irq();
            }
            0x5205 => self.multiplicand = value,
            0x5206 => self.multiplier = value,
            _ => {}
        }
    }

    // -- CHR -----------------------------------------------------------------

    /// The 10 bits a $512x write stores, by CHR mode.
    fn chr_register(&self, value: u8) -> u16 {
        let value = value as u16;
        match self.chr_mode {
            3 => (self.chr_high << 8) | value,
            2 => ((self.chr_high & 1) << 9) | (value << 1) | (value & 1),
            1 => (value << 2) | (value & 1),
            _ => ((value & 0x7F) << 3) | (value & 1),
        }
    }

    fn update_chr(&mut self) {
        let mode = self.chr_mode;
        let span = 8usize >> mode;
        let size = self.b.chr.len();
        let regs = self.chr_registers;
        for window in 0..8 {
            let first = window - window % span;
            let a = regs[first + span - 1] as usize;
            let b = regs[8 + ((first + span - 1) & 3)] as usize;
            let low = window % span;
            self.chr_a[window] = (((a & !(span - 1)) | low) << 10) % size;
            self.chr_b[window] = (((b & !(span - 1)) | low) << 10) % size;
        }
    }

    // -- nametables ------------------------------------------------------------

    fn nametable_byte(&self, address: u16) -> u8 {
        let attribute = address & 0x3FF >= 0x3C0;
        if self.in_frame && self.substitutions {
            if self.in_split {
                if attribute {
                    let tile = self.split_tile;
                    let shift = ((tile >> 4) & 4) | (tile & 2);
                    let at = 0x3C0 | ((tile & 0x380) >> 4) | ((tile & 0x1F) >> 2);
                    return ((self.exram[at] >> shift) & 3) * 0x55;
                }
                return self.exram[self.split_tile];
            }
            if self.exram_mode == 1 && attribute {
                return (self.ext_tile >> 6) * 0x55;
            }
        }
        let source = self.nametables[((address >> 10) & 3) as usize];
        if source == 2 {
            return if self.exram_mode < 2 {
                self.exram[address as usize & 0x3FF]
            } else {
                0
            };
        }
        if attribute {
            self.fill_attribute
        } else {
            self.fill_tile
        }
    }

    // -- scanline detection, the fetch count, the split ------------------------

    /// One PPU /RD (the wiki's pseudo-code, with the fetch count).
    fn read(&mut self, address: u16) {
        self.read_since_m2 = true;
        self.fetch += 1;
        let nametable = (0x2000..0x3000).contains(&address);
        if nametable && address as i64 == self.last_read {
            self.matches += 1;
            if self.matches == 2 {
                self.fetch = 0;
            }
        } else if self.matches >= 2 {
            self.matches = 0;
            self.scanline_detected();
        } else {
            self.matches = 0;
        }
        self.last_read = address as i64;
        if nametable && self.fetch & 3 == 0 {
            self.tile(address);
        }
    }

    fn scanline_detected(&mut self) {
        if !self.in_frame {
            self.in_frame = true;
            self.scanline = 0;
        } else {
            self.scanline += 1;
            if self.scanline == self.irq_target as i64 && self.irq_target != 0 {
                self.irq_pending = true;
                self.update_irq();
            }
        }
    }

    /// A tile's nametable read: its extended attribute, and whether it is in the split.
    fn tile(&mut self, address: u16) {
        let slot = self.fetch >> 2;
        if self.exram_mode == 1 {
            self.ext_tile = self.exram[address as usize & 0x3FF];
        }
        let control = self.split_control;
        if control & 0x80 == 0 || self.exram_mode >= 2 || (32..40).contains(&slot) {
            self.in_split = false;
            return;
        }
        let column = if slot < 32 { slot + 2 } else { slot - 40 };
        let threshold = (control & 0x1F) as i64;
        let right = control & 0x40 != 0;
        self.in_split = if right {
            column >= threshold
        } else {
            column < threshold
        };
        if self.in_split {
            let line = (self.scanline + if slot >= 40 { 1 } else { 0 } + self.split_scroll as i64)
                .rem_euclid(240);
            self.split_tile = (((line & 0xF8) << 2) | column) as usize;
        }
    }

    fn leave_frame(&mut self) {
        self.in_frame = false;
        self.last_read = -1;
        self.matches = 0;
    }

    fn update_irq(&mut self) {
        self.irq = (self.irq_pending && self.irq_enabled) || self.sound.irq();
    }
}

impl Cartridge for Mmc5 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }
    fn expansion_audio(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn expansion(&self) -> f64 {
        self.sound.expansion
    }

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        if address >= 0x6000 {
            if (0xFFFA..0xFFFC).contains(&address) {
                self.leave_frame();
                self.irq_pending = false;
                self.update_irq();
            }
            let (rom, offset) = self.prg_windows[((address - 0x6000) >> 13) as usize]?;
            let a = offset + (address as usize & 0x1FFF);
            let value = if rom {
                self.b.prg[a]
            } else {
                self.b.prg_ram[a]
            };
            if self.sound.read_mode && (0x8000..0xC000).contains(&address) {
                self.sound.dac(value);
                self.update_irq();
            }
            return Some(value as u16);
        }
        if address >= 0x5C00 {
            return if self.exram_mode >= 2 {
                Some(self.exram[(address - 0x5C00) as usize] as u16)
            } else {
                None
            };
        }
        if address == 0x5204 {
            let value =
                (if self.irq_pending { 0x80 } else { 0 }) | (if self.in_frame { 0x40 } else { 0 });
            self.irq_pending = false;
            self.update_irq();
            return Some(value);
        }
        if address == 0x5010 || address == 0x5015 {
            let value = self.sound.read(address);
            self.update_irq();
            return value.map(u16::from);
        }
        if address == 0x5205 {
            return Some((self.multiplicand as u16 * self.multiplier as u16) & 0xFF);
        }
        if address == 0x5206 {
            return Some((self.multiplicand as u16 * self.multiplier as u16) >> 8);
        }
        None
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x6000 {
            if let Some((false, offset)) = self.prg_windows[((address - 0x6000) >> 13) as usize] {
                if self.protect == [2, 1] {
                    self.b.prg_ram[offset + (address as usize & 0x1FFF)] = value;
                }
            }
            return;
        }
        if address < 0x5000 {
            self.snoop(address, value);
        } else if address >= 0x5C00 {
            if self.exram_mode != 3 {
                self.exram[(address - 0x5C00) as usize] = if self.exram_mode == 2 || self.in_frame {
                    value
                } else {
                    0
                };
            }
        } else if address <= 0x5015 {
            self.sound.write(address, value);
            self.update_irq();
        } else if (0x5100..=0x5206).contains(&address) {
            self.register(address, value);
        }
    }

    fn ppu_read(&mut self, address: u16, _t: i64) -> Option<u8> {
        if address >= 0x2000 {
            return Some(self.nametable_byte(address));
        }
        self.read(address);
        let slot = self.fetch >> 2;
        let a = address as usize;
        let chr = &self.b.chr;
        if self.in_frame && self.substitutions {
            if self.in_split && !sprite_slot(slot) {
                return Some(chr[(self.split_bank as usize * 0x1000 + (a & 0xFFF)) % chr.len()]);
            }
            if self.exram_mode == 1 && !sprite_slot(slot) {
                let bank = ((self.chr_high as usize) << 6) | (self.ext_tile as usize & 0x3F);
                return Some(chr[(bank * 0x1000 + (a & 0xFFF)) % chr.len()]);
            }
            if self.sprites_8x16 {
                let offsets = if sprite_slot(slot) {
                    &self.chr_a
                } else {
                    &self.chr_b
                };
                return Some(chr[offsets[a >> 10] + (a & 0x3FF)]);
            }
        } else if self.sprites_8x16 && self.last_chr_set_b {
            return Some(chr[self.chr_b[a >> 10] + (a & 0x3FF)]);
        }
        Some(chr[self.chr_a[a >> 10] + (a & 0x3FF)])
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        self.read(address);
        if self.in_frame && self.substitutions {
            if self.in_split {
                return None;
            }
            if self.exram_mode == 1 && address & 0x3FF >= 0x3C0 {
                return None;
            }
        }
        let source = self.nametables[((address >> 10) & 3) as usize];
        if source < 2 {
            Some(source)
        } else {
            None
        }
    }

    fn ppu_write(&mut self, address: u16, value: u8, _t: i64) {
        let a = address as usize;
        if address < 0x2000 {
            if self.b.chr_writable {
                self.b.chr[self.chr_a[a >> 10] + (a & 0x3FF)] = value;
            }
        } else if self.nametables[(a >> 10) & 3] == 2 && self.exram_mode < 2 {
            self.exram[a & 0x3FF] = value;
        }
    }

    fn m2(&mut self, _t: i64) {
        self.sound.m2();
        if self.read_since_m2 {
            self.read_since_m2 = false;
            self.idle = 0;
        } else {
            self.idle += 1;
            if self.idle == 3 {
                self.leave_frame();
            }
        }
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        if self.b.battery {
            Some(self.b.prg_ram.clone())
        } else {
            None
        }
    }
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    match mapper {
        5 => Ok(Box::new(Mmc5::new(image, cpu_cycle_ticks))),
        _ => Err(image),
    }
}
