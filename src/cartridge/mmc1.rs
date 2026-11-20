// SPDX-License-Identifier: MIT
//! MMC1 (mappers 1 and 155) and NES-EVENT (105): five serial writes load a
//! register (`mmc1.py`; the boards' wirings are described there).
//!
//! The reference's `NESEvent` and `MMC1A` subclass `MMC1` and override
//! `_update` and `_update_prg`; here one struct carries a [`Kind`] and the
//! two methods dispatch on it. multicart.py's `ResetMMC1` (mapper 374)
//! overrides `map_prg`, `map_chr` and `chr_bit_is_prg_a18`: [`Kind::Reset`],
//! and every bank the MMC1 maps goes through [`Mmc1::map_prg`] and
//! [`Mmc1::map_chr`], which apply that override. chinese.py's `HuangMMC1`
//! (116.2) overrides `_update_prg`: [`Kind::Huang`]; multiasic.py's
//! `HostMMC1` overrides `map_prg` and `map_chr` with its host board's
//! translation: [`Kind::Translated`].

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

const MIRRORING: [super::banked::Mirroring; 4] = [SCREEN_A, SCREEN_B, VERTICAL, HORIZONTAL];

/// The NES-EVENT timer's four DIP switches, D C B A (C closed).
pub const EVENT_DIP_SWITCHES: u32 = 0b0100;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub unlocked: bool,
    pub saw_running: bool,
    pub timer_held: bool,
    pub counter: u32,
    pub target: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Mmc1,
    /// Mapper 155: $E000 D4 leaves PRG-RAM enabled.
    Mmc1a,
    /// Mapper 105: NES-EVENT.
    Event(Event),
    /// Mapper 374: the reference's `ResetMMC1` (multicart.py), each game an
    /// MMC1 in 128 KiB of PRG and of CHR; a reset steps `outer`.
    Reset {
        outer: i64,
    },
    /// chinese.py's `HuangMMC1` (116.2): the PRG register in 8 KiB units.
    Huang,
    /// multiasic.py's `HostMMC1`: banks through the host board's translation.
    Translated(super::multiasic::Host),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Mmc1 {
    pub b: Banked,
    pub kind: Kind,
    pub fixed_32k: bool,
    pub ram_bank_shift: u32,
    pub ram_bank_mask: usize,
    pub snrom: bool,
    pub shift: u8,
    pub count: u8,
    pub last_write_t: i64,
    pub control: u8,
    pub chr0: u8,
    pub chr1: u8,
    pub prg_bank: u8,
    pub irq: bool,
}

impl Mmc1 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Mmc1 {
        Mmc1::build(image, cpu_cycle_ticks, Kind::Mmc1)
    }

    pub fn mmc1a(image: RomImage, cpu_cycle_ticks: i64) -> Mmc1 {
        Mmc1::build(image, cpu_cycle_ticks, Kind::Mmc1a)
    }

    /// Mapper 374 (multicart.py's `ResetMMC1`).
    pub fn reset_multicart(image: RomImage, cpu_cycle_ticks: i64) -> Mmc1 {
        Mmc1::build(image, cpu_cycle_ticks, Kind::Reset { outer: 0 })
    }

    /// chinese.py's `HuangMMC1`.
    pub fn huang(image: RomImage, cpu_cycle_ticks: i64) -> Mmc1 {
        Mmc1::build(image, cpu_cycle_ticks, Kind::Huang)
    }

    /// multiasic.py's `HostMMC1`, banking through `host`.
    pub fn translated(image: RomImage, cpu_cycle_ticks: i64, host: super::multiasic::Host) -> Mmc1 {
        Mmc1::build(image, cpu_cycle_ticks, Kind::Translated(host))
    }

    pub fn nes_event(image: RomImage, cpu_cycle_ticks: i64) -> Mmc1 {
        let kind = Kind::Event(Event {
            unlocked: false,
            saw_running: false,
            timer_held: false,
            counter: 0,
            target: 0x20000000 + EVENT_DIP_SWITCHES * 0x2000000,
        });
        let mut mmc1 = Mmc1::build(image, cpu_cycle_ticks, kind);
        mmc1.snrom = false;
        mmc1.update();
        mmc1
    }

    fn build(image: RomImage, cpu_cycle_ticks: i64, kind: Kind) -> Mmc1 {
        let board = image.board.clone();
        let b = Banked::new(image, cpu_cycle_ticks);
        let ram = b.prg_ram.len();
        let szrom = board.prg_ram == 0x2000 && board.prg_nvram == 0x2000 && b.chr.len() >= 0x4000;
        let (ram_bank_shift, ram_bank_mask) = if ram == 0x8000 {
            (2, 3)
        } else if ram == 0x4000 {
            (if szrom { 4 } else { 3 }, 1)
        } else {
            (0, 0)
        };
        let snrom = ram == 0x2000 && b.chr_writable && b.prg.len() <= 0x40000;
        let mut mmc1 = Mmc1 {
            b,
            kind,
            fixed_32k: board.submapper == 5,
            ram_bank_shift,
            ram_bank_mask,
            snrom,
            shift: 0,
            count: 0,
            last_write_t: -1,
            control: 0x0C,
            chr0: 0,
            chr1: 0,
            prg_bank: 0,
            irq: false,
        };
        mmc1.update();
        mmc1
    }

    /// `_update` (multiasic's `HostMMC1.refresh`).
    pub fn update(&mut self) {
        if let Kind::Event(event) = &mut self.kind {
            let held = self.chr0 & 0x10 != 0;
            if held && !event.timer_held && event.saw_running {
                event.unlocked = true;
            }
            if held {
                event.counter = 0;
                self.irq = false;
            } else {
                event.saw_running = true;
            }
            event.timer_held = held;
            self.mmc1_update();
            self.map_chr(0, 8, 0);
        } else {
            self.mmc1_update();
        }
    }

    /// `MMC1._update`.
    fn mmc1_update(&mut self) {
        let control = self.control;
        self.b.set_mirroring(MIRRORING[(control & 3) as usize]);
        if control & 0x10 != 0 {
            self.map_chr(0, 4, self.chr0 as i64);
            self.map_chr(4, 4, self.chr1 as i64);
        } else {
            self.map_chr(0, 8, (self.chr0 >> 1) as i64);
        }
        if self.fixed_32k {
            self.map_prg(0, 32, 0);
        } else {
            self.update_prg(control);
        }
        self.b.prg_ram_offset =
            ((self.chr0 as usize >> self.ram_bank_shift) & self.ram_bank_mask) * 0x2000;
        let mut enabled = !self.b.prg_ram.is_empty();
        if !matches!(self.kind, Kind::Mmc1a) {
            enabled = enabled && self.prg_bank & 0x10 == 0;
        }
        if self.snrom {
            enabled = enabled && self.chr0 & 0x10 == 0;
        }
        self.b.prg_ram_enabled = enabled;
    }

    /// `MMC1.map_prg`, through `ResetMMC1`'s override.
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        if let Kind::Reset { outer } = self.kind {
            let banks = (128 / size_kib) as i64;
            self.b
                .map_prg(window, size_kib, bank.rem_euclid(banks) + outer * banks);
        } else if let Kind::Translated(host) = self.kind {
            super::multiasic::translate_prg(&mut self.b, &host, window, size_kib, bank);
        } else {
            self.b.map_prg(window, size_kib, bank);
        }
    }

    /// `MMC1.map_chr`, through `ResetMMC1`'s override.
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        if let Kind::Reset { outer } = self.kind {
            let banks = (128 / size_kib) as i64;
            self.b
                .map_chr(window, size_kib, bank.rem_euclid(banks) + outer * banks);
        } else if let Kind::Translated(host) = self.kind {
            super::multiasic::translate_chr(&mut self.b, &host, window, size_kib, bank);
        } else {
            self.b.map_chr(window, size_kib, bank);
        }
    }

    /// `MMC1.chr_bit_is_prg_a18` (`ResetMMC1`: never).
    fn chr_bit_is_prg_a18(&self) -> bool {
        !matches!(self.kind, Kind::Reset { .. }) && self.b.prg.len() > 0x40000
    }

    fn update_prg(&mut self, control: u8) {
        if let Kind::Event(event) = &self.kind {
            let unlocked = event.unlocked;
            if !unlocked {
                self.map_prg(0, 32, 0);
            } else if self.chr0 & 0x08 == 0 {
                self.map_prg(0, 32, ((self.chr0 >> 1) & 3) as i64);
            } else {
                let bank = (8 | (self.prg_bank & 7)) as i64;
                let mode = (control >> 2) & 3;
                if mode < 2 {
                    self.map_prg(0, 32, bank >> 1);
                } else if mode == 2 {
                    self.map_prg(0, 16, 8);
                    self.map_prg(2, 16, bank);
                } else {
                    self.map_prg(0, 16, bank);
                    self.map_prg(2, 16, 15);
                }
            }
            return;
        }
        if matches!(self.kind, Kind::Huang) {
            let written = self.prg_bank;
            self.prg_bank = (written & 0x10) | (written & 0x0F) >> 1;
            self.mmc1_update_prg(control);
            self.prg_bank = written;
            return;
        }
        self.mmc1_update_prg(control);
    }

    /// `MMC1._update_prg`.
    fn mmc1_update_prg(&mut self, control: u8) {
        let mmc1a = matches!(self.kind, Kind::Mmc1a);
        let outer = if self.chr_bit_is_prg_a18() {
            (self.chr0 & 0x10) as i64
        } else {
            0
        };
        let bank = (self.prg_bank & 0x0F) as i64 | outer;
        let mode = (control >> 2) & 3;
        if mode < 2 {
            self.map_prg(0, 32, bank >> 1);
            return;
        }
        let mut fixed: i64 = if mode == 2 { 0 } else { 0x0F };
        if mmc1a && self.prg_bank & 0x10 != 0 {
            fixed = (fixed & 0x07) | (self.prg_bank & 0x08) as i64;
        }
        if mode == 2 {
            self.map_prg(0, 16, outer | fixed);
            self.map_prg(2, 16, bank);
        } else {
            self.map_prg(0, 16, bank);
            self.map_prg(2, 16, outer | fixed);
        }
    }
}

impl Cartridge for Mmc1 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        matches!(self.kind, Kind::Event(_))
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let consecutive = t - self.last_write_t == self.b.cpu_cycle_ticks;
        self.last_write_t = t;
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if value & 0x80 != 0 {
            self.shift = 0;
            self.count = 0;
            self.control |= 0x0C;
            self.update();
            return;
        }
        if consecutive {
            return;
        }
        self.shift |= (value & 1) << self.count;
        self.count += 1;
        if self.count < 5 {
            return;
        }
        match (address >> 13) & 3 {
            0 => self.control = self.shift,
            1 => self.chr0 = self.shift,
            2 => self.chr1 = self.shift,
            _ => self.prg_bank = self.shift,
        }
        self.shift = 0;
        self.count = 0;
        self.update();
    }

    fn m2_gap(&mut self, _t: i64) {
        if let Kind::Event(event) = &mut self.kind {
            event.unlocked = false;
            event.saw_running = false;
            self.update();
        } else if let Kind::Reset { outer } = &mut self.kind {
            *outer = (*outer + 1) % (self.b.prg.len() as i64 / 0x20000).max(1);
            self.update();
        }
    }

    fn m2(&mut self, _t: i64) {
        if let Kind::Event(event) = &mut self.kind {
            if !event.timer_held {
                event.counter += 1;
                if event.counter == event.target {
                    self.irq = true;
                }
            }
        }
    }
}
