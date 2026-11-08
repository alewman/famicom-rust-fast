//! The 2A03's sound unit: five channels, the frame counter, $4015, the DMC's
//! DMA request (`apu.py`; its module notes give the sources and the clocks).
//!
//! The board calls [`Apu::clock`] once per CPU cycle, before the cycle's
//! access; [`Apu::put`] says whether the current cycle is an APU put. The
//! floating-point mixer and filters do the reference's operations in its
//! order, so the samples are the same doubles.

use crate::machine::ApuTables;
use std::f64::consts::PI;

/// The box filter's rate, in host samples.
pub const OVERSAMPLE: usize = 4;
/// The decimating low-pass's length, at the box filter's rate.
pub const TAPS: usize = 96;

/// A Blackman-windowed sinc low-pass at `cutoff` Hz for samples at `rate`,
/// unity gain at DC.
pub fn lowpass_taps(cutoff: f64, rate: f64, count: usize) -> Vec<f64> {
    let middle = (count - 1) as f64 / 2.0;
    let fc = cutoff / rate;
    let mut taps = Vec::with_capacity(count);
    for i in 0..count {
        let x = i as f64 - middle;
        let sinc = if x == 0.0 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let phase = 2.0 * PI * i as f64 / (count - 1) as f64;
        taps.push(sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()));
    }
    // Python's sum() on PyPy 3.11: plain left-to-right addition from 0.
    let mut total = 0.0;
    for &t in &taps {
        total += t;
    }
    taps.iter().map(|t| t / total).collect()
}

/// APU_Length_Counter.wiki.
pub const LENGTHS: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// APU_Pulse.wiki "Duty Cycle Sequences".
pub const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [0, 0, 0, 0, 0, 0, 1, 1],
    [0, 0, 0, 0, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 0, 0],
];

/// APU_Triangle.wiki: the 32-step sequence.
pub const TRIANGLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

pub const NOISE_PERIODS: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
pub const DMC_RATES: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];
pub const PAL_NOISE_PERIODS: [u16; 16] = [
    4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778,
];
pub const PAL_DMC_RATES: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

pub const QUARTER: u8 = 1;
pub const HALF: u8 = 2;
pub const IRQ: u8 = 4;
pub const WRAP: u8 = 8;

const STEPS_4: [(u32, u8); 6] = [
    (7457, QUARTER),
    (14913, QUARTER | HALF),
    (22371, QUARTER),
    (29828, IRQ),
    (29829, QUARTER | HALF | IRQ),
    (29830, IRQ | WRAP),
];
const STEPS_5: [(u32, u8); 5] = [
    (7457, QUARTER),
    (14913, QUARTER | HALF),
    (22371, QUARTER),
    (37281, QUARTER | HALF),
    (37282, WRAP),
];
const PAL_STEPS_4: [(u32, u8); 6] = [
    (8313, QUARTER),
    (16627, QUARTER | HALF),
    (24939, QUARTER),
    (33252, IRQ),
    (33253, QUARTER | HALF | IRQ),
    (33254, IRQ | WRAP),
];
const PAL_STEPS_5: [(u32, u8); 5] = [
    (8313, QUARTER),
    (16627, QUARTER | HALF),
    (24939, QUARTER),
    (41565, QUARTER | HALF),
    (41566, WRAP),
];

/// The tables that differ between the RP2A03 and the RP2A07.
#[derive(Clone, Copy, Debug)]
pub struct Tables {
    pub noise_periods: &'static [u16; 16],
    pub dmc_rates: &'static [u16; 16],
    pub steps_4: &'static [(u32, u8)],
    pub steps_5: &'static [(u32, u8)],
}

pub fn tables(which: ApuTables) -> Tables {
    match which {
        ApuTables::Ntsc => Tables {
            noise_periods: &NOISE_PERIODS,
            dmc_rates: &DMC_RATES,
            steps_4: &STEPS_4,
            steps_5: &STEPS_5,
        },
        ApuTables::Pal => Tables {
            noise_periods: &PAL_NOISE_PERIODS,
            dmc_rates: &PAL_DMC_RATES,
            steps_4: &PAL_STEPS_4,
            steps_5: &PAL_STEPS_5,
        },
    }
}

fn step_event(steps: &[(u32, u8)], cycle: u32) -> u8 {
    for &(at, event) in steps {
        if at == cycle {
            return event;
        }
    }
    0
}

/// APU_Envelope.wiki: a divider and a decay level, clocked on quarter frames.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    pub start: bool,
    pub looping: bool,
    pub constant: bool,
    /// Also the constant volume.
    pub period: u8,
    pub divider: u8,
    pub decay: u8,
}

impl Envelope {
    /// Bits 5-0 of $4000/$4004/$400C.
    pub fn set(&mut self, value: u8) {
        self.looping = value & 0x20 != 0;
        self.constant = value & 0x10 != 0;
        self.period = value & 0x0F;
    }

    pub fn quarter(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.period;
        } else if self.divider != 0 {
            self.divider -= 1;
        } else {
            self.divider = self.period;
            if self.decay != 0 {
                self.decay -= 1;
            } else if self.looping {
                self.decay = 15;
            }
        }
    }

    pub fn volume(&self) -> u8 {
        if self.constant {
            self.period
        } else {
            self.decay
        }
    }
}

/// APU_Length_Counter.wiki, with blargg's two races.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LengthCounter {
    pub value: u8,
    pub halt: bool,
    pub enabled: bool,
    pub new_halt: bool,
    /// A length to load at the end of this cycle; 0: none.
    pub load: u8,
    /// The counter when the load was written.
    pub before: u8,
}

impl LengthCounter {
    pub fn write_halt(&mut self, halt: bool) {
        self.new_halt = halt;
    }

    pub fn write_load(&mut self, index: u8) {
        if self.enabled {
            self.load = LENGTHS[index as usize];
            self.before = self.value;
        }
    }

    pub fn half(&mut self) {
        if self.value != 0 && !self.halt {
            self.value -= 1;
        }
    }

    /// The end of a CPU cycle with a write to this channel.
    pub fn settle(&mut self) {
        if self.load != 0 {
            if self.value == self.before {
                self.value = self.load;
            }
            self.load = 0;
        }
        self.halt = self.new_halt;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.value = 0;
            self.load = 0;
        }
    }
}

/// APU_Pulse.wiki and APU_Sweep.wiki.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Pulse {
    pub ones_complement: bool,
    pub envelope: Envelope,
    pub length: LengthCounter,
    pub duty: usize,
    pub step: usize,
    pub period: i32,
    pub timer: i32,
    pub sweep_enabled: bool,
    pub sweep_period: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_divider: u8,
    pub sweep_reload: bool,
    pub target: i32,
    pub output: u8,
}

impl Pulse {
    pub fn new(ones_complement: bool) -> Pulse {
        Pulse {
            ones_complement,
            envelope: Envelope::default(),
            length: LengthCounter::default(),
            duty: 0,
            step: 0,
            period: 0,
            timer: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_divider: 0,
            sweep_reload: false,
            target: 0,
            output: 0,
        }
    }

    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.duty = (value >> 6) as usize;
                self.envelope.set(value);
                self.length.write_halt(value & 0x20 != 0);
            }
            1 => {
                self.sweep_enabled = value & 0x80 != 0;
                self.sweep_period = (value >> 4) & 7;
                self.sweep_negate = value & 0x08 != 0;
                self.sweep_shift = value & 7;
                self.sweep_reload = true;
            }
            2 => self.period = (self.period & 0x700) | value as i32,
            _ => {
                self.period = (self.period & 0xFF) | ((value as i32 & 7) << 8);
                self.length.write_load(value >> 3);
                self.step = 0;
                self.envelope.start = true;
            }
        }
        self.update_target();
    }

    fn update_target(&mut self) {
        let mut change = self.period >> self.sweep_shift;
        if self.sweep_negate {
            change = -change - self.ones_complement as i32;
        }
        self.target = (self.period + change).max(0);
        self.update_output();
    }

    /// One APU cycle; true when the sequencer stepped while it can be heard.
    pub fn clock_timer(&mut self) -> bool {
        if self.timer != 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period;
        self.step = (self.step + 7) & 7;
        self.period >= 8 && self.length.value > 0
    }

    pub fn half(&mut self) {
        if self.sweep_divider == 0
            && self.sweep_enabled
            && self.sweep_shift != 0
            && self.period >= 8
            && self.target <= 0x7FF
        {
            self.period = self.target;
            self.update_target();
        }
        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    pub fn update_output(&mut self) {
        if self.period < 8 || self.target > 0x7FF || self.length.value == 0 {
            self.output = 0;
        } else {
            self.output = if DUTY[self.duty][self.step] != 0 {
                self.envelope.volume()
            } else {
                0
            };
        }
    }
}

/// APU_Triangle.wiki. The timer counts CPU cycles.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Triangle {
    pub length: LengthCounter,
    pub control: bool,
    pub linear_period: u8,
    pub linear: u8,
    pub linear_reload: bool,
    pub period: i32,
    pub timer: i32,
    pub step: usize,
    pub output: u8,
}

impl Triangle {
    pub fn new() -> Triangle {
        Triangle {
            length: LengthCounter::default(),
            control: false,
            linear_period: 0,
            linear: 0,
            linear_reload: false,
            period: 0,
            timer: 0,
            step: 0,
            output: TRIANGLE[0],
        }
    }

    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.control = value & 0x80 != 0;
                self.linear_period = value & 0x7F;
                self.length.write_halt(self.control);
            }
            2 => self.period = (self.period & 0x700) | value as i32,
            3 => {
                self.period = (self.period & 0xFF) | ((value as i32 & 7) << 8);
                self.length.write_load(value >> 3);
                self.linear_reload = true;
            }
            _ => {}
        }
    }

    /// One CPU cycle; true when the sequencer stepped.
    pub fn clock_timer(&mut self) -> bool {
        if self.timer != 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period;
        if self.linear != 0 && self.length.value != 0 {
            self.step = (self.step + 1) & 31;
            self.output = TRIANGLE[self.step];
            return true;
        }
        false
    }

    pub fn quarter(&mut self) {
        if self.linear_reload {
            self.linear = self.linear_period;
        } else if self.linear != 0 {
            self.linear -= 1;
        }
        if !self.control {
            self.linear_reload = false;
        }
    }
}

impl Default for Triangle {
    fn default() -> Self {
        Triangle::new()
    }
}

/// APU_Noise.wiki: a 15-bit LFSR clocked by a timer counting APU cycles.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Noise {
    #[serde(skip, default = "crate::state::live_noise_periods")]
    pub periods: &'static [u16; 16],
    pub envelope: Envelope,
    pub length: LengthCounter,
    /// The feedback tap: 1, or 6 in periodic mode.
    pub mode_bit: u8,
    pub period: i32,
    pub timer: i32,
    pub shift: u16,
    pub output: u8,
}

impl Noise {
    pub fn new(periods: &'static [u16; 16]) -> Noise {
        Noise {
            periods,
            envelope: Envelope::default(),
            length: LengthCounter::default(),
            mode_bit: 1,
            period: periods[0] as i32 / 2 - 1,
            timer: 0,
            shift: 1,
            output: 0,
        }
    }

    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.envelope.set(value);
                self.length.write_halt(value & 0x20 != 0);
            }
            2 => {
                self.mode_bit = if value & 0x80 != 0 { 6 } else { 1 };
                self.period = self.periods[(value & 0x0F) as usize] as i32 / 2 - 1;
            }
            3 => {
                self.length.write_load(value >> 3);
                self.envelope.start = true;
            }
            _ => {}
        }
        self.update_output();
    }

    /// One APU cycle; true when the shift register moved while it can be heard.
    pub fn clock_timer(&mut self) -> bool {
        if self.timer != 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period;
        let shift = self.shift;
        let feedback = (shift ^ (shift >> self.mode_bit)) & 1;
        self.shift = (shift >> 1) | (feedback << 14);
        self.length.value > 0
    }

    pub fn update_output(&mut self) {
        if self.shift & 1 != 0 || self.length.value == 0 {
            self.output = 0;
        } else {
            self.output = self.envelope.volume();
        }
    }
}

/// APU_DMC.wiki: the memory reader, sample buffer, output unit and IRQ flag.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Dmc {
    #[serde(skip, default = "crate::state::live_dmc_rates")]
    pub rates: &'static [u16; 16],
    pub irq_enabled: bool,
    pub looping: bool,
    pub period: i32,
    pub timer: i32,
    /// The 7-bit output level.
    pub output: u8,
    pub sample_address: u16,
    pub sample_length: u16,
    pub address: u16,
    /// Bytes remaining.
    pub remaining: u16,
    /// The sample buffer; None: empty.
    pub buffer: Option<u8>,
    pub shift: u8,
    /// Bits remaining in the output cycle.
    pub bits: u8,
    pub silence: bool,
    pub irq: bool,
    /// The timer reloaded during the current CPU cycle.
    pub reloaded: bool,
}

impl Dmc {
    pub fn new(rates: &'static [u16; 16]) -> Dmc {
        Dmc {
            rates,
            irq_enabled: false,
            looping: false,
            period: rates[0] as i32 / 2 - 1,
            timer: 0,
            output: 0,
            sample_address: 0xC000,
            sample_length: 1,
            address: 0xC000,
            remaining: 0,
            buffer: None,
            shift: 0,
            bits: 8,
            silence: true,
            irq: false,
            reloaded: false,
        }
    }

    pub fn write(&mut self, register: u16, value: u8) {
        match register {
            0 => {
                self.irq_enabled = value & 0x80 != 0;
                if !self.irq_enabled {
                    self.irq = false;
                }
                self.looping = value & 0x40 != 0;
                self.period = self.rates[(value & 0x0F) as usize] as i32 / 2 - 1;
                if self.reloaded {
                    self.timer = self.period;
                }
            }
            1 => self.output = value & 0x7F,
            2 => self.sample_address = 0xC000 | ((value as u16) << 6),
            _ => self.sample_length = ((value as u16) << 4) | 1,
        }
    }

    pub fn restart(&mut self) {
        self.address = self.sample_address;
        self.remaining = self.sample_length;
    }

    /// One APU cycle; true when the sample buffer was just emptied into the shifter.
    pub fn clock_timer(&mut self) -> bool {
        if self.timer != 0 {
            self.timer -= 1;
            return false;
        }
        self.timer = self.period;
        self.reloaded = true;
        if !self.silence {
            let level = self.output;
            if self.shift & 1 != 0 {
                if level <= 125 {
                    self.output = level + 2;
                }
            } else if level >= 2 {
                self.output = level - 2;
            }
        }
        self.shift >>= 1;
        self.bits -= 1;
        if self.bits != 0 {
            return false;
        }
        self.bits = 8;
        match self.buffer.take() {
            None => {
                self.silence = true;
                false
            }
            Some(buffer) => {
                self.silence = false;
                self.shift = buffer;
                true
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Apu {
    #[serde(skip, default = "crate::state::live_tables")]
    pub tables: Tables,
    /// Whether the current CPU cycle is an APU put; clock() flips it first.
    pub put: bool,
    pub pulse1: Pulse,
    pub pulse2: Pulse,
    pub triangle: Triangle,
    pub noise: Noise,
    pub dmc: Dmc,
    pub frame_irq: bool,
    frame_irq_clear: bool,
    pub irq: bool,
    pub mode5: bool,
    pub inhibit: bool,
    pub frame_cycle: u32,
    pub pending_frame_write: Option<u8>,
    pub pending_delay: u32,
    pub last_frame_write: u8,
    pub dmc_request: bool,
    load_countdown: u32,
    enable_hold: u32,
    reload_waiting: bool,
    pub dmc_abort: bool,
    apu_cycle: i64,
    stopped_in: i64,
    pub unexpected_reload: bool,
    empty_end: i64,
    pub late_stop: bool,
    late_reload: bool,
    ending: u32,
    /// Length counters written this cycle (bits: pulse 1, pulse 2, triangle, noise).
    unsettled: u8,
    cpu_hz: i64,
    sample_rate: i64,
    box_rate: i64,
    taps: Vec<f64>,
    level: f64,
    level_dirty: bool,
    /// A cartridge's expansion sound level, summed every cycle.
    pub expansion: f64,
    accumulated: f64,
    accumulated_cycles: i64,
    phase: i64,
    samples: Vec<f64>,
    history: Vec<f64>,
    /// A run of frame-counter cycles with no step in them, for one mode:
    /// (mode5, first, end), cycles first..end (fork cache: the step tables
    /// never change, so a run found once stays true; empty until found).
    #[serde(skip)]
    quiet: (bool, u32, u32),
}

impl Apu {
    pub fn new(
        put_first: bool,
        sample_rate: i64,
        which: ApuTables,
        unexpected_reload: bool,
        late_stop: bool,
    ) -> Apu {
        let tables = tables(which);
        let mut apu = Apu {
            tables,
            put: !put_first,
            pulse1: Pulse::new(true),
            pulse2: Pulse::new(false),
            triangle: Triangle::new(),
            noise: Noise::new(tables.noise_periods),
            dmc: Dmc::new(tables.dmc_rates),
            frame_irq: false,
            frame_irq_clear: false,
            irq: false,
            mode5: false,
            inhibit: false,
            frame_cycle: 0,
            pending_frame_write: None,
            pending_delay: 0,
            last_frame_write: 0x00,
            dmc_request: false,
            load_countdown: 0,
            enable_hold: 0,
            reload_waiting: false,
            dmc_abort: false,
            apu_cycle: 0,
            stopped_in: -2,
            unexpected_reload,
            empty_end: -2,
            late_stop,
            late_reload: late_stop && unexpected_reload,
            ending: 0,
            unsettled: 0,
            cpu_hz: 1_789_773,
            sample_rate: 0,
            box_rate: 0,
            taps: Vec::new(),
            level: 0.0,
            level_dirty: true,
            expansion: 0.0,
            accumulated: 0.0,
            accumulated_cycles: 0,
            phase: 0,
            samples: Vec::new(),
            history: vec![0.0; TAPS - 1],
            quiet: (false, 0, 0),
        };
        apu.set_sample_rate(sample_rate);
        apu
    }

    /// The host rate [`Apu::take_samples`] delivers.
    pub fn sample_rate(&self) -> i64 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, rate: i64) {
        self.sample_rate = rate;
        self.box_rate = rate * OVERSAMPLE as i64;
        let mut taps = lowpass_taps(
            20_000.0f64.min(0.45 * rate as f64),
            self.box_rate as f64,
            TAPS,
        );
        taps.reverse();
        self.taps = taps;
    }

    pub fn set_cpu_rate(&mut self, cpu_hz: f64) {
        self.cpu_hz = cpu_hz.round_ties_even() as i64;
    }

    /// Power: $4015 = 0; $4017 as if $00 were written just before the CPU starts.
    pub fn power(&mut self) {
        self.write(0x4015, 0);
        self.apply_frame_write(0x00);
    }

    /// Reset: $4015 = 0, the frame IRQ flag cleared, $4017 rewritten with its last value.
    pub fn reset(&mut self) {
        self.write(0x4015, 0);
        self.frame_irq = false;
        let last = self.last_frame_write;
        self.write(0x4017, last);
    }

    // -- the clock ---------------------------------------------------------

    /// One CPU cycle.
    pub fn clock(&mut self) {
        self.put = !self.put;
        let put = self.put;
        self.dmc.reloaded = false;
        if self.frame_irq_clear && !put {
            self.frame_irq_clear = false;
            self.frame_irq = false;
            self.update_irq();
        }
        if self.enable_hold != 0 {
            self.enable_hold -= 1;
            if self.enable_hold == 0 && self.reload_waiting {
                self.reload_waiting = false;
                if self.dmc.buffer.is_none() && self.dmc.remaining != 0 {
                    self.dmc_request = true;
                }
            }
        }
        if self.ending != 0 {
            self.ending -= 1;
        }
        if self.load_countdown != 0 {
            self.load_countdown -= 1;
            if self.load_countdown == 0 && self.dmc.buffer.is_none() && self.dmc.remaining != 0 {
                self.dmc_request = true;
            }
        }
        self.clock_frame_counter();
        if self.unsettled != 0 {
            let unsettled = self.unsettled;
            if unsettled & 1 != 0 {
                self.pulse1.length.settle();
            }
            if unsettled & 2 != 0 {
                self.pulse2.length.settle();
            }
            if unsettled & 4 != 0 {
                self.triangle.length.settle();
            }
            if unsettled & 8 != 0 {
                self.noise.length.settle();
            }
            self.unsettled = 0;
            self.level_dirty = true;
        }
        if self.triangle.clock_timer() {
            self.level_dirty = true;
        }
        if !put {
            let p1 = self.pulse1.clock_timer();
            let p2 = self.pulse2.clock_timer();
            let n = self.noise.clock_timer();
            if p1 | p2 | n {
                self.level_dirty = true;
            }
            self.apu_cycle += 1;
            if self.dmc.timer == 0 && !self.dmc.silence {
                self.level_dirty = true;
            }
            if self.dmc.clock_timer() {
                if self.dmc.remaining != 0 {
                    if self.enable_hold != 0 {
                        self.reload_waiting = true;
                    } else {
                        self.dmc_request = true;
                    }
                } else if self.late_reload && self.stopped_in == self.apu_cycle {
                    self.dmc_request = true;
                } else if self.stopped_in == self.apu_cycle - 1 {
                    self.dmc_abort = true;
                }
            } else if self.dmc.reloaded
                && self.dmc.bits == 8
                && self.dmc.buffer.is_none()
                && self.dmc.remaining != 0
            {
                self.empty_end = self.apu_cycle;
            }
        }
        if self.level_dirty {
            self.level = self.mix();
        }
        self.accumulated += self.level + self.expansion;
        self.accumulated_cycles += 1;
        self.phase += self.box_rate;
        if self.phase >= self.cpu_hz {
            self.phase -= self.cpu_hz;
            self.samples
                .push(self.accumulated / self.accumulated_cycles as f64);
            self.accumulated = 0.0;
            self.accumulated_cycles = 0;
        }
    }

    fn clock_frame_counter(&mut self) {
        if let Some(value) = self.pending_frame_write {
            self.pending_delay -= 1;
            if self.pending_delay == 0 {
                self.apply_frame_write(value);
                self.pending_frame_write = None;
                return;
            }
        }
        let cycle = self.frame_cycle + 1;
        self.frame_cycle = cycle;
        let (mode5, first, end) = self.quiet;
        if mode5 == self.mode5 && first <= cycle && cycle < end {
            return;
        }
        let steps = if self.mode5 {
            self.tables.steps_5
        } else {
            self.tables.steps_4
        };
        let event = step_event(steps, cycle);
        // The next step after this cycle (the tables are in order): none
        // falls in cycle + 1 .. next.
        let next = steps
            .iter()
            .map(|&(at, _)| at)
            .find(|&at| at > cycle)
            .unwrap_or(u32::MAX);
        self.quiet = (self.mode5, cycle + 1, next);
        if event != 0 {
            if event & IRQ != 0 {
                self.frame_irq = !(event & WRAP != 0 && self.inhibit);
                self.update_irq();
            }
            if event & QUARTER != 0 {
                self.quarter_frame();
            }
            if event & HALF != 0 {
                self.half_frame();
            }
            if event & WRAP != 0 {
                self.frame_cycle = 0;
            }
        }
    }

    fn quarter_frame(&mut self) {
        self.pulse1.envelope.quarter();
        self.pulse2.envelope.quarter();
        self.noise.envelope.quarter();
        self.triangle.quarter();
        self.level_dirty = true;
    }

    fn half_frame(&mut self) {
        self.pulse1.length.half();
        self.pulse2.length.half();
        self.triangle.length.half();
        self.noise.length.half();
        self.pulse1.half();
        self.pulse2.half();
        self.level_dirty = true;
    }

    fn apply_frame_write(&mut self, value: u8) {
        self.mode5 = value & 0x80 != 0;
        self.frame_cycle = 0;
        if self.mode5 {
            self.quarter_frame();
            self.half_frame();
        }
    }

    // -- the mixer ---------------------------------------------------------

    /// APU_Mixer.wiki, blargg's formula: 0.0 to about 1.0.
    fn mix(&mut self) -> f64 {
        self.level_dirty = false;
        self.pulse1.update_output();
        self.pulse2.update_output();
        self.noise.update_output();
        let pulses = self.pulse1.output as i32 + self.pulse2.output as i32;
        let mut level = if pulses != 0 {
            95.88 / (8128.0 / pulses as f64 + 100.0)
        } else {
            0.0
        };
        let tnd = self.triangle.output as f64 / 8227.0
            + self.noise.output as f64 / 12241.0
            + self.dmc.output as f64 / 22638.0;
        if tnd != 0.0 {
            level += 159.79 / (1.0 / tnd + 100.0);
        }
        level
    }

    /// The box filter's samples since the last call, before the low-pass
    /// (the lockstep gate's view of the mixer).
    pub fn boxed(&self) -> &[f64] {
        &self.samples
    }

    /// The mixer's output since the last call, at the sample rate.
    pub fn take_samples(&mut self) -> Vec<f64> {
        let boxed = std::mem::take(&mut self.samples);
        let keep = boxed.len() - boxed.len() % OVERSAMPLE;
        if keep < boxed.len() {
            self.samples = boxed[keep..].to_vec();
        }
        let mut window = std::mem::take(&mut self.history);
        window.extend_from_slice(&boxed[..keep]);
        let span = self.taps.len();
        let mut out = Vec::new();
        let mut end = span - 1 + OVERSAMPLE;
        while end <= window.len() {
            let mut total = 0.0;
            for (tap, x) in self.taps.iter().zip(&window[end - span..end]) {
                total += tap * x;
            }
            out.push(total);
            end += OVERSAMPLE;
        }
        self.history = window[window.len() - (span - 1)..].to_vec();
        out
    }

    // -- the DMC's DMA -----------------------------------------------------

    pub fn dmc_address(&self) -> u16 {
        self.dmc.address
    }

    /// The DMA's byte arrives: fill the buffer, advance the reader.
    pub fn dmc_fill(&mut self, value: u8) {
        self.dmc_request = false;
        let late = self.unexpected_reload
            && !self.late_stop
            && self.empty_end == self.apu_cycle
            && self.dmc.buffer.is_none();
        let dmc = &mut self.dmc;
        if late {
            self.empty_end = -2;
            dmc.shift = value;
            dmc.silence = false;
            self.dmc_request = true;
        } else {
            dmc.buffer = Some(value);
        }
        dmc.address = dmc.address.wrapping_add(1);
        if dmc.address == 0 {
            dmc.address = 0x8000;
        }
        if dmc.remaining == 0 {
            return;
        }
        dmc.remaining -= 1;
        if dmc.remaining == 0 {
            if dmc.looping {
                dmc.restart();
                return;
            }
            self.stopped_in = self.apu_cycle + self.late_stop as i64;
            if self.late_stop {
                self.ending = 3;
            }
            if dmc.irq_enabled {
                dmc.irq = true;
                self.irq = true;
            }
        }
    }

    // -- registers ---------------------------------------------------------

    pub fn write(&mut self, address: u16, value: u8) {
        let register = address & 3;
        if address < 0x4004 {
            self.pulse1.write(register, value);
            if register == 0 || register == 3 {
                self.unsettled |= 1;
            }
        } else if address < 0x4008 {
            self.pulse2.write(register, value);
            if register == 0 || register == 3 {
                self.unsettled |= 2;
            }
        } else if address < 0x400C {
            self.triangle.write(register, value);
            if register == 0 || register == 3 {
                self.unsettled |= 4;
            }
        } else if address < 0x4010 {
            self.noise.write(register, value);
            if register == 0 || register == 3 {
                self.unsettled |= 8;
            }
        } else if address < 0x4014 {
            self.dmc.write(register, value);
        } else if address == 0x4015 {
            self.write_status(value);
        } else if address == 0x4017 {
            self.last_frame_write = value;
            self.inhibit = value & 0x40 != 0;
            if self.inhibit {
                self.frame_irq = false;
            }
            self.pending_frame_write = Some(value);
            self.pending_delay = if self.put { 3 } else { 4 };
        }
        self.level_dirty = true;
        self.update_irq();
    }

    fn write_status(&mut self, value: u8) {
        self.pulse1.length.set_enabled(value & 1 != 0);
        self.pulse2.length.set_enabled(value & 2 != 0);
        self.triangle.length.set_enabled(value & 4 != 0);
        self.noise.length.set_enabled(value & 8 != 0);
        self.dmc.irq = false;
        if value & 0x10 == 0 {
            if self.dmc.remaining != 0 {
                self.stopped_in = self.apu_cycle;
            }
            self.dmc.remaining = 0;
            self.ending = 0;
            self.load_countdown = 0;
        } else if self.dmc.remaining == 0 {
            self.dmc.restart();
            self.enable_hold = 2;
            if self.dmc.buffer.is_none() {
                self.load_countdown = if self.put { 2 } else { 3 };
            }
        }
    }

    /// $4015: DMC IRQ, frame IRQ, DMC active, length > 0; clears the frame IRQ flag.
    pub fn read_status(&mut self) -> u8 {
        let value = ((self.dmc.irq as u8) << 7)
            | ((self.frame_irq as u8) << 6)
            | (((self.dmc.remaining > 0 || self.ending > 0) as u8) << 4)
            | ((self.noise.length.value > 0) as u8) << 3
            | ((self.triangle.length.value > 0) as u8) << 2
            | ((self.pulse2.length.value > 0) as u8) << 1
            | (self.pulse1.length.value > 0) as u8;
        self.frame_irq_clear = true;
        value
    }

    fn update_irq(&mut self) {
        self.irq = self.dmc.irq || (self.frame_irq && !self.inhibit);
    }
}

/// The console's filters after the DAC, **fitted** (`apu.AnalogueStage`).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AnalogueStage {
    highs: Vec<f64>,
    high_state: Vec<(f64, f64)>,
    low: Option<f64>,
    low_out: f64,
}

impl AnalogueStage {
    pub fn new(sample_rate: i64, high_passes: &[f64], low_pass: Option<f64>) -> AnalogueStage {
        let dt = 1.0 / sample_rate as f64;
        let highs = high_passes
            .iter()
            .map(|&f| {
                let rc = 1.0 / (2.0 * PI * f);
                rc / (rc + dt)
            })
            .collect();
        AnalogueStage {
            highs,
            high_state: vec![(0.0, 0.0); high_passes.len()],
            low: low_pass.map(|f| dt / (1.0 / (2.0 * PI * f) + dt)),
            low_out: 0.0,
        }
    }

    pub fn process(&mut self, samples: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(samples.len());
        for &sample in samples {
            let mut x = sample;
            if let Some(low) = self.low {
                self.low_out += low * (x - self.low_out);
                x = self.low_out;
            }
            for (i, &alpha) in self.highs.iter().enumerate() {
                let (last_in, last_out) = self.high_state[i];
                let y = alpha * (last_out + x - last_in);
                self.high_state[i] = (x, y);
                x = y;
            }
            out.push(x);
        }
        out
    }
}
