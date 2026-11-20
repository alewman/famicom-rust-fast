// SPDX-License-Identifier: MIT
//! The FDS's sound: the RP2C33's wavetable channel and its modulator
//! (`fds_sound.py`; FDS_audio.wiki and loopy's measurements, the DAC, the
//! filter and the measured level are described there).

use super::connector::expansion_gain;
use std::f64::consts::PI;

pub const FDS_DB: f64 = 6.6;
pub const CUTOFF_HZ: f64 = 2000.0;
pub const FILTER_LOSS_DB: f64 = 0.61;

pub fn cpu_hz() -> f64 {
    236_250_000.0 / 11.0 / 12.0
}

pub fn lowpass() -> f64 {
    1.0 - (-2.0 * PI * CUTOFF_HZ / cpu_hz()).exp()
}

/// The level per DAC step (`fds_sound.GAIN`).
pub fn gain() -> f64 {
    expansion_gain(FDS_DB + FILTER_LOSS_DB, 63)
}

/// $4089 D1-D0, the ladder's grounding resistors.
pub const MASTER: [f64; 4] = [1.0, 2.0 / 3.0, 1.0 / 2.0, 2.0 / 5.0];
/// Mod table entries as mod counter steps (None: reset to 0).
pub const MOD_STEPS: [Option<i64>; 8] = [
    Some(0),
    Some(1),
    Some(2),
    Some(4),
    None,
    Some(-4),
    Some(-2),
    Some(-1),
];
/// $4095's nibble, as loopy read it.
pub const MOD_READ: [u8; 8] = [0x0, 0x1, 0x2, 0x4, 0xC, 0xC, 0xE, 0xF];

/// The volume ($4080) or mod depth ($4084) envelope.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    pub speed: i64,
    pub increase: bool,
    pub off: bool,
    pub gain: i64,
    pub timer: i64,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope {
            speed: 0,
            increase: false,
            off: true,
            gain: 0,
            timer: 0,
        }
    }
}

impl Envelope {
    pub fn write(&mut self, value: u8, master_speed: i64) {
        self.speed = (value & 0x3F) as i64;
        self.increase = value & 0x40 != 0;
        self.off = value & 0x80 != 0;
        if self.off {
            self.gain = (value & 0x3F) as i64;
        }
        self.timer = self.period(master_speed);
    }

    pub fn period(&self, master_speed: i64) -> i64 {
        8 * (self.speed + 1) * (master_speed + 1)
    }

    pub fn tick(&mut self) {
        if self.increase {
            if self.gain < 32 {
                self.gain += 1;
            }
        } else if self.gain != 0 {
            self.gain -= 1;
        }
    }
}

/// $4040-$408A and the readable $4090-$4097, clocked every CPU cycle.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FdsSound {
    pub enabled: bool,
    #[serde(with = "serde_big_array::BigArray")]
    pub wave: [u8; 64],
    pub wave_write: bool,
    pub master: usize,
    pub volume: Envelope,
    pub depth: Envelope,
    pub master_speed: i64,
    pub pitch: i64,
    pub wave_reset: bool,
    pub envelopes_off: bool,
    /// 24 bits: the wave position in bits 23-18.
    pub accumulator: i64,
    pub mod_frequency: i64,
    pub mod_halt: bool,
    pub mod_force: bool,
    /// 18 bits: the table address in bits 17-13.
    pub mod_accumulator: i64,
    pub mod_table: [u8; 32],
    /// 7-bit signed.
    pub mod_counter: i64,
    pub prescaler: i64,
    pub pwm: i64,
    pub out_volume: i64,
    pub sample: i64,
    /// After the low-pass.
    pub level: f64,
    lowpass: f64,
}

impl Default for FdsSound {
    fn default() -> Self {
        FdsSound::new()
    }
}

impl FdsSound {
    pub fn new() -> FdsSound {
        FdsSound {
            enabled: false,
            wave: [0; 64],
            wave_write: false,
            master: 0,
            volume: Envelope::default(),
            depth: Envelope::default(),
            master_speed: 0,
            pitch: 0,
            wave_reset: true,
            envelopes_off: false,
            accumulator: 0,
            mod_frequency: 0,
            mod_halt: true,
            mod_force: false,
            mod_accumulator: 0,
            mod_table: [0; 32],
            mod_counter: 0,
            prescaler: 0,
            pwm: 0,
            out_volume: 0,
            sample: 0,
            level: 0.0,
            lowpass: lowpass(),
        }
    }

    // -- registers -------------------------------------------------------------

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.accumulator = 0;
            self.mod_counter = 0;
            self.volume.gain = 0;
            self.out_volume = 0;
        }
    }

    pub fn write(&mut self, address: u16, value: u8) {
        if address < 0x4080 {
            if self.wave_write {
                self.wave[(address & 0x3F) as usize] = value & 0x3F;
            }
            return;
        }
        match address {
            0x4080 => {
                if self.enabled {
                    self.volume.write(value, self.master_speed);
                    if self.volume.off && self.volume.gain == 0 {
                        self.out_volume = 0;
                    }
                }
            }
            0x4082 => self.pitch = (self.pitch & 0xF00) | value as i64,
            0x4083 => {
                self.pitch = (self.pitch & 0x0FF) | ((value as i64 & 0x0F) << 8);
                self.wave_reset = value & 0x80 != 0;
                self.envelopes_off = value & 0x40 != 0;
                if self.wave_reset {
                    self.accumulator = 0;
                    self.prescaler = 0;
                }
                if self.envelopes_off {
                    self.volume.timer = self.volume.period(self.master_speed);
                    self.depth.timer = self.depth.period(self.master_speed);
                }
            }
            0x4084 => self.depth.write(value, self.master_speed),
            0x4085 => {
                if self.enabled {
                    self.mod_counter = (value as i64 & 0x7F) - ((value as i64 & 0x40) << 1);
                }
            }
            0x4086 => self.mod_frequency = (self.mod_frequency & 0xF00) | value as i64,
            0x4087 => {
                self.mod_frequency = (self.mod_frequency & 0x0FF) | ((value as i64 & 0x0F) << 8);
                self.mod_halt = value & 0x80 != 0;
                self.mod_force = value & 0x40 != 0;
                if self.mod_halt {
                    self.mod_accumulator &= 0x3E000;
                }
            }
            0x4088 => {
                if self.enabled && self.mod_halt {
                    self.mod_table[((self.mod_accumulator >> 13) & 0x1F) as usize] = value & 7;
                    self.mod_accumulator = (self.mod_accumulator + 0x2000) & 0x3FFFF;
                }
            }
            0x4089 => {
                self.wave_write = value & 0x80 != 0;
                self.master = (value & 3) as usize;
            }
            0x408A => self.master_speed = value as i64,
            _ => {}
        }
    }

    /// The byte driven for a read of `address` (undriven bits as a mask in
    /// the high byte), or None.
    pub fn read(&self, address: u16) -> Option<u16> {
        if (0x4040..0x4080).contains(&address) {
            let position = if self.wave_write {
                (address & 0x3F) as usize
            } else {
                (self.accumulator >> 18) as usize
            };
            return Some(self.wave[position] as u16 | 0xC0 << 8);
        }
        Some(match address {
            0x4090 => self.volume.gain as u16 | 0xC0 << 8,
            0x4091 => ((self.accumulator >> 12) & 0xFF) as u16,
            0x4092 => self.depth.gain as u16 | 0xC0 << 8,
            0x4093 => ((self.mod_accumulator >> 5) & 0x7F) as u16 | 0x80 << 8,
            0x4094 => (((self.mod_counter * self.depth.gain) >> 4) & 0xFF) as u16,
            0x4095 => {
                MOD_READ[self.mod_table[((self.mod_accumulator >> 13) & 0x1F) as usize] as usize]
                    as u16
            }
            0x4096 => {
                let value = if self.pwm < self.out_volume {
                    self.wave[(self.accumulator >> 18) as usize]
                } else {
                    0
                };
                value as u16 | 0xC0 << 8
            }
            0x4097 => (self.mod_counter & 0x7F) as u16 | 0x80 << 8,
            _ => return None,
        })
    }

    // -- one CPU cycle -----------------------------------------------------------

    /// Advance a cycle; returns the low-passed output in DAC steps.
    pub fn clock(&mut self) -> f64 {
        self.pwm = (self.pwm + 1) & 31;
        if self.enabled {
            if self.master_speed != 0 && !self.envelopes_off {
                let step = if self.wave_reset { 4 } else { 1 };
                let master_speed = self.master_speed;
                for envelope in [&mut self.volume, &mut self.depth] {
                    if !envelope.off {
                        envelope.timer -= step;
                        if envelope.timer <= 0 {
                            envelope.timer += envelope.period(master_speed);
                            envelope.tick();
                        }
                    }
                }
            }
            if !self.wave_reset {
                self.prescaler = (self.prescaler + 1) & 15;
                if self.prescaler == 0 {
                    self.tick();
                }
            }
            let position = (self.accumulator >> 18) as usize;
            if position == 0 {
                self.out_volume = self.volume.gain.min(32);
            }
            if !self.wave_write {
                self.sample = self.wave[position] as i64 * self.out_volume;
            }
        } else {
            self.sample = 0;
        }
        let target = self.sample as f64 * MASTER[self.master] / 32.0;
        self.level += self.lowpass * (target - self.level);
        self.level
    }

    /// Every 16 cycles: the modulator, then the wave, steps.
    fn tick(&mut self) {
        if !self.mod_halt {
            let low = (self.mod_accumulator & 0xFFF) + self.mod_frequency;
            let carry = low > 0xFFF || self.mod_force;
            self.mod_accumulator = (self.mod_accumulator & 0x3F000) | (low & 0xFFF);
            if carry {
                let entry = self.mod_table[((self.mod_accumulator >> 13) & 0x1F) as usize];
                match MOD_STEPS[entry as usize] {
                    None => self.mod_counter = 0,
                    Some(step) => {
                        let counter = (self.mod_counter + step) & 0x7F;
                        self.mod_counter = counter - ((counter & 0x40) << 1);
                    }
                }
                self.mod_accumulator = (self.mod_accumulator + 0x1000) & 0x3FFFF;
            }
        }
        let mut temp = self.mod_counter * self.depth.gain;
        if temp & 0x0F != 0 && temp & 0x800 == 0 {
            temp += 0x20;
        }
        temp = ((temp + 0x400) >> 4) & 0xFF;
        self.accumulator = (self.accumulator + ((self.pitch * temp) & 0xFFFFF)) & 0xFFFFFF;
    }
}
