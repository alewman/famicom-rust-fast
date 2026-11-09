//! The Famicom Disk System: the RAM adapter (RP2C33) and its disk drive
//! (`fds.py`; the registers follow FDS_Registers.wiki, the drive's model
//! and its unmeasured spin-up are described there).

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::fds_disk::{crc16, disk_sides, fds_side, raw_side, SIDE_BYTES};
use super::fds_sound::{gain, FdsSound};
use super::ines::RomImage;
use crate::banked_field;

/// Master-clock ticks per disk byte.
pub const BYTE_TICKS: i64 = 1792;
/// NTSC: the RP2C33 is a Famicom part.
pub fn master_hz() -> f64 {
    236_250_000.0 / 11.0
}
pub fn spin_up_bytes() -> i64 {
    (50_000.0f64 * 12.0 / BYTE_TICKS as f64).round_ties_even() as i64
}
pub fn revolution_bytes() -> i64 {
    (7.0 * master_hz() / BYTE_TICKS as f64).round_ties_even() as i64
}
/// CPU cycles the DRAM refresh watchdog allows.
pub const WATCHDOG_RELOAD: i64 = 3245;
/// A save: the sides written since the image was made, each as a .fds side.
pub const SAVE_MAGIC: &[u8; 4] = b"FDSW";

// $4025 bits
pub const SCAN: u8 = 1;
pub const MOTOR_STOP: u8 = 2;
pub const READ: u8 = 4;
pub const ARRANGEMENT: u8 = 8;
pub const CRC_SEND: u8 = 16;
pub const CRC_ENABLE: u8 = 64;
pub const DISK_IRQ: u8 = 128;

/// The RAM adapter with a disk image in its drive (`image.misc`: the `.fds` data).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fds {
    pub b: Banked,
    pub bios: Vec<u8>,
    pub sound: FdsSound,
    pub sides: Vec<Vec<u8>>,
    pub written: Vec<bool>,
    /// The side in the drive.
    pub side: Option<usize>,
    pub write_protected: bool,
    pub reload: u16,
    pub counter: u16,
    pub timer_enabled: bool,
    pub timer_repeat: bool,
    pub timer_irq: bool,
    pub disk_io: bool,
    pub sound_io: bool,
    pub watchdog_enabled: bool,
    pub refresh: u8,
    pub watchdog: i64,
    pub watchdog_irq: bool,
    prg_ram_cycle: bool,
    pub control: u8,
    pub write_latch: u8,
    pub read_latch: u8,
    pub external: u8,
    pub byte_flag: bool,
    pub crc: u16,
    pub crc_error: bool,
    pub synced: bool,
    pub crc_sent: u32,
    byte_ticks: i64,
    /// None while returning or spinning up, else its byte on the side.
    pub head: Option<usize>,
    pub spin_up: i64,
    pub irq: bool,
    /// The level last driven (the reference's `audio_out.expansion`).
    pub expansion: f64,
    gain: f64,
    revolution: usize,
}

impl Fds {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fds {
        let bios = image.prg.clone();
        let sides = disk_sides(&image.misc);
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram = vec![0; 0x8000];
        b.chr = vec![0; 0x2000];
        b.chr_writable = true;
        b.map_chr(0, 8, 0);
        let side = if sides.is_empty() { None } else { Some(0) };
        let mut fds = Fds {
            b,
            bios,
            sound: FdsSound::new(),
            written: vec![false; sides.len()],
            sides,
            side,
            write_protected: false,
            reload: 0,
            counter: 0,
            timer_enabled: false,
            timer_repeat: false,
            timer_irq: false,
            disk_io: false,
            sound_io: false,
            watchdog_enabled: false,
            refresh: 0,
            watchdog: 4095,
            watchdog_irq: false,
            prg_ram_cycle: false,
            control: 0x06,
            write_latch: 0,
            read_latch: 0,
            external: 0x7F,
            byte_flag: false,
            crc: 0,
            crc_error: false,
            synced: false,
            crc_sent: 0,
            byte_ticks: 0,
            head: None,
            spin_up: spin_up_bytes(),
            irq: false,
            expansion: 0.0,
            gain: gain(),
            revolution: revolution_bytes() as usize,
        };
        fds.apply_control();
        fds
    }

    // -- the disk ------------------------------------------------------------

    /// Put disk side `side` in the drive (None: eject).
    pub fn insert(&mut self, side: Option<usize>) {
        self.side = side;
        self.head = None;
        self.spin_up = spin_up_bytes();
    }

    pub fn side_name(side: Option<usize>) -> String {
        match side {
            None => "no disk".into(),
            Some(side) => format!(
                "disk {} side {}",
                side / 2 + 1,
                if side & 1 != 0 { 'B' } else { 'A' }
            ),
        }
    }

    /// The motor turns and the head scans: $4025 D1 clear and D0 set, a disk in.
    pub fn scanning(&self) -> bool {
        self.side.is_some() && self.control & (SCAN | MOTOR_STOP) == SCAN
    }

    /// $4030 D6: the head past the side's data, on its way back.
    pub fn end_of_head(&self) -> bool {
        match (self.head, self.side) {
            (Some(head), Some(side)) => head >= self.sides[side].len(),
            _ => false,
        }
    }

    /// One byte time of the serial interface.
    fn byte(&mut self) {
        let control = self.control;
        let mut value: Option<u8> = None;
        let mut position: Option<usize> = None;
        let scanning = self.scanning();
        match self.side {
            Some(side) if scanning => {
                let raw = &self.sides[side];
                match self.head {
                    None => {
                        self.spin_up -= 1;
                        if self.spin_up <= 0 {
                            self.head = Some(0);
                        }
                    }
                    Some(head) => {
                        if head < raw.len() {
                            value = Some(raw[head]);
                            position = Some(head);
                        } else {
                            value = Some(0);
                        }
                        self.head = Some(if head + 1 < raw.len().max(self.revolution) {
                            head + 1
                        } else {
                            0
                        });
                    }
                }
            }
            _ => {
                self.head = None;
                self.spin_up = spin_up_bytes();
            }
        }
        if control & READ != 0 {
            let Some(value) = value else { return };
            if control & CRC_ENABLE == 0 {
                return;
            }
            if !self.synced {
                if value != 0 {
                    self.synced = true;
                    self.crc = crc16(&[value], 0);
                }
                return;
            }
            self.read_latch = value;
            self.crc = crc16(&[value], self.crc);
            if control & CRC_SEND != 0 {
                self.crc_error = self.crc != 0;
            }
            self.byte_flag = true;
        } else {
            let out = if control & CRC_ENABLE == 0 {
                0
            } else if control & CRC_SEND != 0 {
                let out = if self.crc_sent < 2 {
                    ((self.crc >> (8 * self.crc_sent)) & 0xFF) as u8
                } else {
                    0
                };
                self.crc_sent += 1;
                out
            } else {
                let out = self.write_latch;
                self.crc = crc16(&[out], self.crc);
                out
            };
            if control & CRC_ENABLE != 0 {
                self.byte_flag = true;
            }
            if let (Some(side), Some(position)) = (self.side, position) {
                if !self.write_protected {
                    self.sides[side][position] = out;
                    self.written[side] = true;
                }
            }
        }
    }

    fn write_io_enable(&mut self, value: u8) {
        self.disk_io = value & 1 != 0;
        self.sound_io = value & 2 != 0;
        self.sound.set_enabled(self.sound_io);
        self.watchdog_enabled = value & 0x80 != 0;
        if !self.watchdog_enabled {
            self.watchdog_irq = false;
        }
        if !self.disk_io {
            self.control = 0x06;
            self.apply_control();
            self.timer_enabled = false;
            self.timer_irq = false;
            self.byte_flag = false;
            self.watchdog = 4095;
            self.refresh = 0;
            self.watchdog_irq = false;
            self.external = 0x7F;
        }
    }

    fn apply_control(&mut self) {
        let control = self.control;
        self.b.set_mirroring(if control & ARRANGEMENT != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        });
        if !self.scanning() {
            self.head = None;
            self.spin_up = spin_up_bytes();
        }
        if control & CRC_ENABLE == 0 {
            self.synced = false;
            self.crc = 0;
            self.crc_error = false;
            self.byte_flag = false;
        }
        if control & CRC_SEND == 0 {
            self.crc_sent = 0;
        }
    }

    fn update_irq(&mut self) {
        self.irq = self.timer_irq
            || (self.watchdog_irq && self.watchdog_enabled)
            || (self.byte_flag && self.control & DISK_IRQ != 0);
    }
}

impl Cartridge for Fds {
    banked_field!(b);
    fn fds(&self) -> Option<&Fds> {
        Some(self)
    }
    fn fds_mut(&mut self) -> Option<&mut Fds> {
        Some(self)
    }

    fn snoops_cpu_reads(&self) -> bool {
        true
    }
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
        self.expansion
    }

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        if address >= 0xE000 {
            return Some(self.bios[(address - 0xE000) as usize] as u16);
        }
        if address >= 0x6000 {
            self.prg_ram_cycle = true;
            return Some(self.b.prg_ram[(address - 0x6000) as usize] as u16);
        }
        if address == 0x4030 {
            let value = self.timer_irq as u16
                | (self.watchdog_irq as u16) << 1
                | (self.control & ARRANGEMENT) as u16
                | (self.crc_error as u16) << 4
                | (self.end_of_head() as u16) << 6
                | (self.byte_flag as u16) << 7;
            self.timer_irq = false;
            self.watchdog_irq = false;
            self.update_irq();
            return Some(value | 0x24 << 8);
        }
        if address == 0x4031 {
            self.byte_flag = false;
            self.update_irq();
            return Some(self.read_latch as u16);
        }
        if address == 0x4032 {
            let status = if self.side.is_none() {
                0x07
            } else {
                let ready = self.head.is_some() && self.scanning() && !self.end_of_head();
                ((!ready) as u16) << 1 | (self.write_protected as u16) << 2
            };
            return Some(status | 0xF8 << 8);
        }
        if address == 0x4033 {
            let battery = if self.side.is_some() && self.control & MOTOR_STOP == 0 {
                0x80
            } else {
                0
            };
            return Some(battery | (self.external as u16 & 0x7F));
        }
        if address >= 0x4040 {
            return self.sound.read(address);
        }
        None
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x6000 {
            if address < 0xE000 {
                self.prg_ram_cycle = true;
                self.b.prg_ram[(address - 0x6000) as usize] = value;
            }
            return;
        }
        if (0x4040..=0x408A).contains(&address) {
            self.sound.write(address, value);
            return;
        }
        match address {
            0x4020 => self.reload = (self.reload & 0xFF00) | value as u16,
            0x4021 => self.reload = (self.reload & 0x00FF) | (value as u16) << 8,
            0x4023 => self.write_io_enable(value),
            _ if !self.disk_io => return,
            0x4022 => {
                self.timer_repeat = value & 1 != 0;
                self.timer_enabled = value & 2 != 0;
                if self.timer_enabled {
                    self.counter = self.reload;
                } else {
                    self.timer_irq = false;
                }
            }
            0x4024 => {
                self.write_latch = value;
                self.byte_flag = false;
            }
            0x4025 => {
                self.control = value;
                self.apply_control();
            }
            0x4026 => self.external = value,
            _ => return,
        }
        self.update_irq();
    }

    fn m2(&mut self, _t: i64) {
        if self.timer_enabled {
            if self.counter == 0 {
                self.timer_irq = true;
                self.counter = self.reload;
                if !self.timer_repeat {
                    self.timer_enabled = false;
                }
            } else {
                self.counter -= 1;
            }
        }
        if self.disk_io {
            let mut reload = false;
            if !self.prg_ram_cycle {
                self.refresh = (self.refresh + 1) & 0x7F;
                reload = self.refresh == 0;
            }
            if reload {
                self.watchdog = WATCHDOG_RELOAD;
            } else if self.watchdog != 0 {
                self.watchdog -= 1;
            } else {
                self.watchdog = 4095;
                if self.watchdog_enabled {
                    self.watchdog_irq = true;
                }
            }
        }
        self.prg_ram_cycle = false;
        self.expansion = self.sound.clock() * self.gain;
        self.byte_ticks += self.b.cpu_cycle_ticks;
        if self.byte_ticks >= BYTE_TICKS {
            self.byte_ticks -= BYTE_TICKS;
            self.byte();
        }
        self.update_irq();
    }

    // -- saves: the written sides beside the image, never the image itself --

    fn battery_ram(&self) -> Option<Vec<u8>> {
        let written: Vec<usize> = (0..self.written.len())
            .filter(|&i| self.written[i])
            .collect();
        if written.is_empty() {
            return None;
        }
        let mut out = SAVE_MAGIC.to_vec();
        out.push(written.len() as u8);
        for side in written {
            out.push(side as u8);
            out.extend_from_slice(&fds_side(&self.sides[side]));
        }
        Some(out)
    }

    fn restore_battery(&mut self, data: &[u8]) -> bool {
        if data.len() < 5 || &data[..4] != SAVE_MAGIC {
            return false;
        }
        let record = 1 + SIDE_BYTES;
        let records: Vec<&[u8]> = data[5..].chunks(record).collect();
        if records.len() != data[4] as usize
            || records
                .iter()
                .any(|r| r.len() != record || r[0] as usize >= self.sides.len())
        {
            return false;
        }
        for r in records {
            self.sides[r[0] as usize] = raw_side(&r[1..]);
            self.written[r[0] as usize] = true;
        }
        true
    }
}
