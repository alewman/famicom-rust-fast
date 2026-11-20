// SPDX-License-Identifier: MIT
//! The MMC5's sound: two pulses and an 8-bit PCM DAC (`mmc5_sound.py`;
//! MMC5_audio.wiki, the measured levels and their sources are there).
//!
//! Difference forced by Rust: the reference's sound object keeps its
//! cartridge to set `audio_out.expansion`; here it keeps the level itself
//! (`expansion`), which the MMC5 reports.

use super::connector::APU_SQUARE;

pub const LENGTHS: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

pub const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [0, 0, 0, 0, 0, 0, 1, 1],
    [0, 0, 0, 0, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 0, 0],
];

/// M2 cycles per envelope and length clock (the die).
pub const FRAME_CYCLES: u32 = 7424;
/// A full-volume square against the APU's (rainwarrior, AV Famicom).
pub const MMC5_DB: f64 = 1.0;

pub fn pulse_gain() -> f64 {
    APU_SQUARE * 10f64.powf(MMC5_DB / 20.0) / 15.0
}

pub fn pcm_gain() -> f64 {
    let pcm_per_pulse_step = 10f64.powf(-(4.28 + 5.09) / 2.0 / 20.0) * 15.0 / 32.0;
    pulse_gain() * pcm_per_pulse_step
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Mmc5Pulse {
    pub duty: usize,
    /// Also the envelope's loop.
    pub halt: bool,
    pub constant: bool,
    /// Also the envelope's period.
    pub volume: u8,
    pub period: u16,
    pub timer: u16,
    pub step: usize,
    pub length: u8,
    pub enabled: bool,
    pub start: bool,
    pub divider: u8,
    pub decay: u8,
}

impl Mmc5Pulse {
    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.duty = (value >> 6) as usize;
                self.halt = value & 0x20 != 0;
                self.constant = value & 0x10 != 0;
                self.volume = value & 0x0F;
            }
            2 => self.period = (self.period & 0x700) | value as u16,
            3 => {
                self.period = (self.period & 0xFF) | ((value as u16 & 7) << 8);
                if self.enabled {
                    self.length = LENGTHS[(value >> 3) as usize];
                }
                self.step = 0;
                self.start = true;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length = 0;
        }
    }

    /// One APU cycle; true when the sequencer stepped.
    pub fn clock_timer(&mut self) -> bool {
        if self.timer != 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period;
        self.step = (self.step + 7) & 7;
        true
    }

    /// The envelope's clock and the length counter's, together.
    pub fn clock_frame(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider != 0 {
            self.divider -= 1;
        } else {
            self.divider = self.volume;
            if self.decay != 0 {
                self.decay -= 1;
            } else if self.halt {
                self.decay = 15;
            }
        }
        if self.length != 0 && !self.halt {
            self.length -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if self.length == 0 || DUTY[self.duty][self.step] == 0 {
            return 0;
        }
        if self.constant {
            self.volume
        } else {
            self.decay
        }
    }
}

/// The registers at $5000-$5015 and the level the chip drives.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Mmc5Sound {
    pub pulses: [Mmc5Pulse; 2],
    pub read_mode: bool,
    pub pcm_irq_enabled: bool,
    pub pcm_tripped: bool,
    pub pcm: u8,
    /// The pulse timers count APU cycles: every other M2.
    pub put: bool,
    pub frame: u32,
    /// The level last driven (the reference's `audio_out.expansion`).
    pub expansion: f64,
    pulse_gain: f64,
    pcm_gain: f64,
}

impl Default for Mmc5Sound {
    fn default() -> Self {
        Mmc5Sound::new()
    }
}

impl Mmc5Sound {
    pub fn new() -> Mmc5Sound {
        Mmc5Sound {
            pulses: [Mmc5Pulse::default(), Mmc5Pulse::default()],
            read_mode: false,
            pcm_irq_enabled: false,
            pcm_tripped: false,
            pcm: 0,
            put: false,
            frame: FRAME_CYCLES,
            expansion: 0.0,
            pulse_gain: pulse_gain(),
            pcm_gain: pcm_gain(),
        }
    }

    pub fn irq(&self) -> bool {
        self.pcm_tripped && self.pcm_irq_enabled
    }

    /// A write to $5000-$5015.
    pub fn write(&mut self, address: u16, value: u8) {
        if address < 0x5008 {
            if address & 3 != 1 {
                self.pulses[((address >> 2) & 1) as usize].write(address & 3, value);
            }
        } else if address == 0x5010 {
            self.read_mode = value & 1 != 0;
            self.pcm_irq_enabled = value & 0x80 != 0;
        } else if address == 0x5011 {
            if !self.read_mode {
                self.dac(value);
            }
        } else if address == 0x5015 {
            self.pulses[0].set_enabled(value & 1 != 0);
            self.pulses[1].set_enabled(value & 2 != 0);
        }
        self.mix();
    }

    pub fn read(&mut self, address: u16) -> Option<u8> {
        if address == 0x5010 {
            let value = if self.irq() { 0x80 } else { 0 };
            self.pcm_tripped = false;
            return Some(value);
        }
        if address == 0x5015 {
            return Some(
                (self.pulses[0].length > 0) as u8 | (((self.pulses[1].length > 0) as u8) << 1),
            );
        }
        None
    }

    /// A byte for the DAC: written to $5011, or read from $8000-$BFFF in read mode.
    pub fn dac(&mut self, value: u8) {
        if value != 0 {
            self.pcm = value;
            self.pcm_tripped = false;
            self.mix();
        } else {
            self.pcm_tripped = true;
        }
    }

    pub fn m2(&mut self) {
        let mut moved = false;
        self.put = !self.put;
        if self.put {
            let a = self.pulses[0].clock_timer();
            let b = self.pulses[1].clock_timer();
            moved = a | b;
        }
        self.frame -= 1;
        if self.frame == 0 {
            self.frame = FRAME_CYCLES;
            for pulse in self.pulses.iter_mut() {
                pulse.clock_frame();
            }
            moved = true;
        }
        if moved {
            self.mix();
        }
    }

    fn mix(&mut self) {
        let pulses = self.pulses[0].output() as i32 + self.pulses[1].output() as i32;
        let level = pulses as f64 * self.pulse_gain + self.pcm as f64 * self.pcm_gain;
        self.expansion = -level;
    }
}
