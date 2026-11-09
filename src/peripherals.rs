//! Controllers besides the standard pad, on the controller ports and the
//! Famicom expansion port (`peripherals.py`; Input_devices.wiki). A device
//! answers one of two protocols:
//!
//! - a **port device** (`board.pads[0]` or `[1]`), like the standard
//!   controller: `set_strobe` with OUT0, `read` D0-D4 at its /OE, `clock` at
//!   the end of the /OE pulse;
//! - an **expansion device** (`board.expansion`): `set_out` with OUT0-2,
//!   `read(port)` and `clock(port)` for either /OE.
//!
//! [`plug`] puts in the devices that NES 2.0's Default Expansion Device byte
//! names (NES_2.0.wiki; nes20db gives the same number).
//!
//! Differences forced by Rust:
//!
//! - the reference's devices that look at the picture or the clock keep a
//!   reference to the PPU (the Zapper) or a `now()` callable reading
//!   `ppu.time` (the Vaus, the Data Recorder); here every protocol call is
//!   handed the PPU, and the device reads `ppu.time`, `ppu.line`, `ppu.dot`
//!   and `ppu.picture` from it;
//! - where the reference's host-facing setters read the PPU (the Zapper's
//!   `trigger` property, the Data Recorder's `play` and `record`), the Rust
//!   setter takes the PPU time: [`Zapper::set_trigger`], [`DataRecorder::play`];
//! - objects the reference shares between an adapter and its ports (the Four
//!   Score's pads, the port test controller's pads, the Famicom Zapper's
//!   Zapper, the keyboard's recorder) are owned by the port or the device
//!   that uses them; [`plug`] returns a [`Plugged`] that says where they are,
//!   and the host reaches them through `as_any_mut` (downcasting), as
//!   [`Board::pad_as`] and [`Board::expansion_as`] do;
//! - `SerialReport`'s subclasses hold a [`SerialReport`] and pass it their
//!   report at the strobe's fall; `_Pointer` is the [`Pointer`] field `pointer`.
//!
//! The float arithmetic (the Zapper's fit and luminance, the Vaus's step and
//! timers) is the reference's, in its order.

use crate::board::Board;
use crate::joypad::StandardController;
use crate::ppu::Ppu;
use std::any::Any;
use std::collections::HashSet;

pub trait PortDevice: crate::state::Snapshot {
    fn set_strobe(&mut self, high: bool, ppu: &Ppu);
    fn read(&mut self, ppu: &Ppu) -> u8;
    fn clock(&mut self, ppu: &Ppu);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub trait ExpansionDevice: crate::state::Snapshot {
    fn set_out(&mut self, value: u8, ppu: &Ppu);
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8;
    fn clock(&mut self, port: usize, ppu: &Ppu);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl PortDevice for StandardController {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        StandardController::set_strobe(self, high);
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        StandardController::read(self)
    }
    fn clock(&mut self, _: &Ppu) {
        StandardController::clock(self);
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

macro_rules! any_mut {
    () => {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    };
}

impl Board {
    /// The device on controller port `port`, as a `T`.
    pub fn pad_as<T: 'static>(&mut self, port: usize) -> Option<&mut T> {
        self.pads[port].as_any_mut().downcast_mut::<T>()
    }

    /// The expansion port's device, as a `T`.
    pub fn expansion_as<T: 'static>(&mut self) -> Option<&mut T> {
        self.expansion.as_mut()?.as_any_mut().downcast_mut::<T>()
    }
}

// -- four players ------------------------------------------------------------

/// One port of a four-player adapter: two pads' reports, then a signature
/// (Four_player_adapters.wiki). The Four Score answers on D0, the Hori 4
/// Players Adapter on D1.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TapPort {
    pub first: StandardController,
    pub second: StandardController,
    /// Reads 16-23, the first in bit 0.
    pub signature: u8,
    pub bit: u8,
    pub count: u32,
    pub strobe: bool,
}

impl TapPort {
    pub fn new(signature: u8, bit: u8) -> TapPort {
        TapPort {
            first: StandardController::default(),
            second: StandardController::default(),
            signature,
            bit,
            count: 0,
            strobe: false,
        }
    }

    pub fn set_strobe(&mut self, high: bool) {
        self.first.set_strobe(high);
        self.second.set_strobe(high);
        self.strobe = high;
        if high {
            self.count = 0;
        }
    }

    pub fn read(&self) -> u8 {
        let count = self.count;
        let value = if count < 8 {
            self.first.read()
        } else if count < 16 {
            self.second.read()
        } else if count < 24 {
            (self.signature >> (count - 16)) & 1
        } else {
            1
        };
        value << self.bit
    }

    pub fn clock(&mut self) {
        if self.strobe {
            self.first.clock();
            self.second.clock();
            return;
        }
        let count = self.count;
        if count < 8 {
            self.first.clock();
        } else if count < 16 {
            self.second.clock();
        }
        if count < 24 {
            self.count = count + 1;
        }
    }

    /// Player `n` of the adapter (0-3): ports alternate, the first pads first.
    pub fn player(ports: &mut [TapPort; 2], n: usize) -> &mut StandardController {
        let port = &mut ports[n % 2];
        if n < 2 {
            &mut port.first
        } else {
            &mut port.second
        }
    }
}

impl PortDevice for TapPort {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        TapPort::set_strobe(self, high);
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        TapPort::read(self)
    }
    fn clock(&mut self, _: &Ppu) {
        TapPort::clock(self);
    }
    any_mut!();
}

/// The NES Four Score (or NES Satellite) in its 4-player mode, on both ports:
/// players 1 and 3 on $4016 D0, 2 and 4 on $4017 D0; signatures: $4016's
/// read 19 is 1, $4017's read 18. Its ports own the pads (player `n` is
/// `pads[n % 2]`, first for `n < 2`).
pub fn four_score() -> [TapPort; 2] {
    [TapPort::new(0x08, 0), TapPort::new(0x04, 0)]
}

/// The Hori 4 Players Adapter in 4-player mode, on the Famicom expansion port:
/// the Four Score's protocol on D1, the signatures swapped. Player `n` of its
/// own ports is [`TapPort::player`]`(&mut ports, n)`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HoriAdapter {
    pub ports: [TapPort; 2],
}

impl HoriAdapter {
    pub fn new() -> HoriAdapter {
        HoriAdapter {
            ports: [TapPort::new(0x04, 1), TapPort::new(0x08, 1)],
        }
    }
}

impl Default for HoriAdapter {
    fn default() -> Self {
        HoriAdapter::new()
    }
}

impl ExpansionDevice for HoriAdapter {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        for port in self.ports.iter_mut() {
            port.set_strobe(value & 1 != 0);
        }
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        self.ports[port].read()
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        self.ports[port].clock();
    }
    any_mut!();
}

/// Two pads on the Famicom expansion port's D1 of $4016 and $4017: players 3
/// and 4 of the "simple" adapters (JoyPair, Twin Adapter).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExpansionPads {
    pub pads: [StandardController; 2],
}

impl ExpansionDevice for ExpansionPads {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        for pad in self.pads.iter_mut() {
            pad.set_strobe(value & 1 != 0);
        }
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        self.pads[port].read() << 1
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        self.pads[port].clock();
    }
    any_mut!();
}

// -- the Zapper ---------------------------------------------------------------

/// Zap Ruder (Zapper.wiki "Output"): lines the photodiode stays on for
/// palette entries $00, $10 and $20. **Fitted** (see the reference).
pub const ZAP_RUDER: [(usize, f64); 3] = [(0x00, 19.0), (0x10, 24.0), (0x20, 26.0)];

/// A stand-in for the palette when its file is absent: each 9-bit value as
/// the grey of its row's luma, columns $D-$F black.
pub fn grey_palette() -> Vec<[u8; 3]> {
    let levels = [102u8, 173, 255, 255];
    (0..512usize)
        .map(|value| {
            let (column, row) = (value & 0x0F, (value >> 4) & 3);
            let grey = if column >= 0x0D { 0 } else { levels[row] };
            [grey, grey, grey]
        })
        .collect()
}

pub fn luminance(rgb: &[u8; 3]) -> f64 {
    0.299 * rgb[0] as f64 + 0.587 * rgb[1] as f64 + 0.114 * rgb[2] as f64
}

/// Lines the light bit stays on after the beam passes each 9-bit pixel value.
pub fn zapper_lines(palette: &[[u8; 3]]) -> Vec<f64> {
    let points: Vec<(f64, f64)> = ZAP_RUDER
        .iter()
        .map(|&(c, lines)| (luminance(&palette[c]).max(1.0).ln(), lines))
        .collect();
    // Least squares for lines = tau * (ln Y - ln Y0); sums as Python's sum().
    let n = points.len() as f64;
    let sum = |f: &dyn Fn(&(f64, f64)) -> f64| points.iter().fold(0.0f64, |s, p| s + f(p));
    let mean_x = sum(&|p| p.0) / n;
    let mean_y = sum(&|p| p.1) / n;
    let tau = sum(&|p| (p.0 - mean_x) * (p.1 - mean_y)) / sum(&|p| (p.0 - mean_x) * (p.0 - mean_x));
    let log_y0 = mean_x - mean_y / tau;
    palette
        .iter()
        .map(|rgb| {
            let lines = tau * (luminance(rgb).max(1e-6).ln() - log_y0);
            if lines > 0.0 {
                lines
            } else {
                0.0
            }
        })
        .collect()
}

/// The NES Zapper on a controller port: D3 = light (0: sensed), D4 = trigger
/// (Zapper.wiki). The trigger's release holds the bit for 100 ms. `aim` is
/// the picture pixel (x, y) the gun points at, or None off the screen.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Zapper {
    pub aim: Option<(i64, i64)>,
    trigger: bool,
    dots_per_line: i64,
    frame_dots: i64,
    hold_ticks: f64,
    released_at: Option<i64>,
    lines: Vec<f64>,
}

impl Zapper {
    pub fn new(ppu: &Ppu, palette: &[[u8; 3]]) -> Zapper {
        let timing = &ppu.timing;
        Zapper {
            aim: None,
            trigger: false,
            dots_per_line: 341,
            frame_dots: timing.lines as i64 * 341,
            hold_ticks: timing.master_hz.div_euclid(10.0),
            released_at: None,
            lines: zapper_lines(palette),
        }
    }

    pub fn trigger(&self) -> bool {
        self.trigger
    }

    /// The reference's `trigger` setter; `time` is `ppu.time` now.
    pub fn set_trigger(&mut self, pulled: bool, time: i64) {
        if self.trigger && !pulled {
            self.released_at = Some(time);
        }
        self.trigger = pulled;
    }

    /// Whether the sensor sees light now, as the PPU's beam stands.
    pub fn light(&self, ppu: &Ppu) -> bool {
        let Some((x, y)) = self.aim else {
            return false;
        };
        if !((0..256).contains(&x) && (0..240).contains(&y)) {
            return false;
        }
        let lines = self.lines[ppu.picture[(y * 256 + x) as usize] as usize];
        if lines == 0.0 {
            return false;
        }
        // Dots since the beam drew the pixel (dot x + 1 of line y).
        let since = (ppu.line as i64 * 341 + ppu.dot as i64 - (y * 341 + x + 1))
            .rem_euclid(self.frame_dots);
        (since as f64) < lines * self.dots_per_line as f64
    }

    fn trigger_bit(&self, ppu: &Ppu) -> bool {
        if self.trigger {
            return true;
        }
        match self.released_at {
            Some(released) => ((ppu.time - released) as f64) < self.hold_ticks,
            None => false,
        }
    }

    pub fn bits(&self, ppu: &Ppu) -> u8 {
        (if self.light(ppu) { 0 } else { 0x08 }) | (if self.trigger_bit(ppu) { 0x10 } else { 0 })
    }
}

impl PortDevice for Zapper {
    fn set_strobe(&mut self, _: bool, _: &Ppu) {}
    fn read(&mut self, ppu: &Ppu) -> u8 {
        self.bits(ppu)
    }
    fn clock(&mut self, _: &Ppu) {}
    any_mut!();
}

/// The Famicom's Zapper, on the expansion port: the NES Zapper's bits on $4017 only.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FamicomZapper {
    pub zapper: Zapper,
}

impl ExpansionDevice for FamicomZapper {
    fn set_out(&mut self, _: u8, _: &Ppu) {}
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8 {
        if port != 0 {
            self.zapper.bits(ppu)
        } else {
            0
        }
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

// -- the Arkanoid (Vaus) controller --------------------------------------------

/// Arkanoid_controller.wiki "Conversion": the counter's step, 7 ms over 512
/// counts (**fitted**).
pub const VAUS_STEP_SECONDS: f64 = 0.007 / 512.0;

/// The Vaus's knob and button, as both versions share them. `position` is
/// the knob as the game reads it, 8 bits. Times are `ppu.time`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Vaus {
    pub position: i64,
    pub fire: bool,
    step: f64,
    pub shift: i64,
    pub serial_in: i64,
    out0: bool,
    /// When the conversion in progress ends.
    end: Option<f64>,
    /// OUT0 last fell, or the start.
    counting_since: Option<i64>,
}

impl Vaus {
    pub fn new(ticks_per_second: f64) -> Vaus {
        Vaus {
            position: 0x98,
            fire: false,
            step: ticks_per_second * VAUS_STEP_SECONDS,
            shift: 0,
            serial_in: 0,
            out0: false,
            end: None,
            counting_since: None,
        }
    }

    fn counter(&self, time: f64) -> i64 {
        match self.counting_since {
            None => 0,
            Some(since) => (((time - since as f64) / self.step) as i64).max(0),
        }
    }

    /// `_counter` at an integer time: Python subtracts the ints exactly first.
    fn counter_at(&self, time: i64) -> i64 {
        match self.counting_since {
            None => 0,
            Some(since) => ((((time - since) as f64) / self.step) as i64).max(0),
        }
    }

    fn settle(&mut self, time: i64) {
        let Some(end) = self.end else {
            return;
        };
        if (time as f64) < end || self.out0 {
            return; // running, or held by OUT0 at 1 until it falls
        }
        let counter = self.counter(end);
        self.shift = (counter >> 1) & 0xFF;
        self.serial_in = counter & 1;
        self.end = None;
    }

    pub fn set_out0(&mut self, high: bool, time: i64) {
        if high == self.out0 {
            return;
        }
        if high {
            self.settle(time);
            if self.end.is_none() {
                // A rise starts a conversion; one in progress runs on.
                self.end = Some(time as f64 + ((self.position << 1) | 1) as f64 * self.step);
            }
            self.counting_since = None; // the counter clears while OUT0 is 1
            self.out0 = true;
        } else {
            self.out0 = false;
            self.counting_since = Some(time);
            if let Some(end) = self.end {
                if end < time as f64 {
                    self.end = Some(time as f64); // held past its end: it ends at the fall
                }
            }
            self.settle(time);
        }
    }

    /// The shift register's output, inverted: the next knob bit, MSB first.
    pub fn data(&mut self, time: i64) -> u8 {
        self.settle(time);
        ((!self.shift >> 7) & 1) as u8
    }

    pub fn shift_out(&mut self, time: i64) {
        self.settle(time);
        let serial = if self.end.is_some() {
            self.counter_at(time) & 1
        } else {
            self.serial_in
        };
        self.shift = ((self.shift << 1) | serial) & 0xFF;
    }
}

/// The NES Vaus on a controller port: D3 = fire (1: pressed), D4 = knob data.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VausNes {
    pub vaus: Vaus,
}

impl PortDevice for VausNes {
    fn set_strobe(&mut self, high: bool, ppu: &Ppu) {
        self.vaus.set_out0(high, ppu.time);
    }
    fn read(&mut self, ppu: &Ppu) -> u8 {
        let fire = if self.vaus.fire { 0x08 } else { 0 };
        fire | self.vaus.data(ppu.time) << 4
    }
    fn clock(&mut self, ppu: &Ppu) {
        self.vaus.shift_out(ppu.time);
    }
    any_mut!();
}

/// The Famicom Vaus on the expansion port: $4016 D1 = fire, $4017 D1 = knob
/// data. `second`: the *Arkanoid II* controller's own expansion port's Vaus,
/// on $4017 D3 (fire) and D4 (knob).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VausFamicom {
    pub vaus: Vaus,
    pub second: Option<Vaus>,
}

impl ExpansionDevice for VausFamicom {
    fn set_out(&mut self, value: u8, ppu: &Ppu) {
        self.vaus.set_out0(value & 1 != 0, ppu.time);
        if let Some(second) = self.second.as_mut() {
            second.set_out0(value & 1 != 0, ppu.time);
        }
    }
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8 {
        if port == 0 {
            return if self.vaus.fire { 0x02 } else { 0 };
        }
        let mut value = self.vaus.data(ppu.time) << 1;
        if let Some(second) = self.second.as_mut() {
            let fire = if second.fire { 0x08 } else { 0 };
            value |= fire | second.data(ppu.time) << 4;
        }
        value
    }
    fn clock(&mut self, port: usize, ppu: &Ppu) {
        if port != 0 {
            self.vaus.shift_out(ppu.time);
            if let Some(second) = self.second.as_mut() {
                second.shift_out(ppu.time);
            }
        }
    }
    any_mut!();
}

// -- mats -----------------------------------------------------------------------

/// Power_Pad.wiki: button numbers as on side B, D3's reads 0-7.
pub const POWER_PAD_LOW: [u32; 8] = [2, 1, 5, 9, 6, 10, 11, 7];
/// D4's reads 0-3, then 1s.
pub const POWER_PAD_HIGH: [u32; 4] = [4, 3, 12, 8];

/// The Power Pad (Family Fun Fitness) on a controller port: two 4021s, on D3
/// and D4. `buttons`: bit n-1 for button n (side B's numbers); pressed reads 1.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PowerPad {
    pub buttons: u32,
    pub strobe: bool,
    low: u32,
    high: u32,
}

impl PowerPad {
    fn latch(&mut self) {
        let buttons = self.buttons;
        self.low = POWER_PAD_LOW
            .iter()
            .enumerate()
            .map(|(i, &n)| ((buttons >> (n - 1)) & 1) << i)
            .sum();
        let high: u32 = POWER_PAD_HIGH
            .iter()
            .enumerate()
            .map(|(i, &n)| ((buttons >> (n - 1)) & 1) << i)
            .sum();
        self.high = high | 0xF0;
    }
}

impl PortDevice for PowerPad {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        if high || self.strobe {
            self.latch();
        }
        self.strobe = high;
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        if self.strobe {
            self.latch();
        }
        ((self.low & 1) << 3 | (self.high & 1) << 4) as u8
    }
    fn clock(&mut self, _: &Ppu) {
        if self.strobe {
            self.latch();
        } else {
            self.low = self.low >> 1 | 0x80;
            self.high = self.high >> 1 | 0x80;
        }
    }
    any_mut!();
}

/// The Family Trainer Mat (and the Tap-tap Mat) on the expansion port
/// (Power_Pad.wiki "Family Trainer Mat"): OUT2-OUT0 = 1 leave out the top,
/// middle and bottom rows; $4017 D4-D1 the columns, 0 when pressed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FamilyTrainer {
    pub buttons: u32,
    out: u8,
}

impl Default for FamilyTrainer {
    fn default() -> Self {
        FamilyTrainer { buttons: 0, out: 7 }
    }
}

impl ExpansionDevice for FamilyTrainer {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        self.out = value & 7;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        let mut pressed = 0u32;
        for (row, ignore) in [4u8, 2, 1].into_iter().enumerate() {
            if self.out & ignore == 0 {
                pressed |= (self.buttons >> (4 * row)) & 0x0F;
            }
        }
        // Button 1 (left column) on D4, button 4 on D1.
        let columns: u32 = (0..4).map(|c| ((pressed >> c) & 1) << (4 - c)).sum();
        (0x1E & !columns) as u8
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

// -- the Family BASIC Keyboard ----------------------------------------------------

/// Family_BASIC_Keyboard.wiki "Matrix": per row, column 0 then column 1, each
/// from $4017 D4 down to D1.
pub const KEYBOARD_MATRIX: [[[&str; 4]; 2]; 9] = [
    [
        ["]", "[", "RETURN", "F8"],
        ["STOP", "YEN", "RSHIFT", "KANA"],
    ],
    [[";", ":", "@", "F7"], ["^", "-", "/", "_"]],
    [["K", "L", "O", "F6"], ["0", "P", ",", "."]],
    [["J", "U", "I", "F5"], ["8", "9", "N", "M"]],
    [["H", "G", "Y", "F4"], ["6", "7", "V", "B"]],
    [["D", "R", "T", "F3"], ["4", "5", "C", "F"]],
    [["A", "S", "W", "F2"], ["3", "E", "Z", "X"]],
    [["CTR", "Q", "ESC", "F1"], ["2", "1", "GRPH", "LSHIFT"]],
    [
        ["LEFT", "RIGHT", "UP", "CLR"],
        ["INS", "DEL", "SPACE", "DOWN"],
    ],
];

/// The Family BASIC Keyboard (HVC-007) on the expansion port: OUT0 resets to
/// row 0, OUT1 selects the column and its fall moves to the next row, OUT2
/// enables the matrix. $4017 D4-D1: the selected keys, 0 = held. `keys`: the
/// held keys' names. `recorder`: a Data Recorder on its jacks.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FamilyBasicKeyboard {
    pub keys: HashSet<String>,
    pub recorder: Option<DataRecorder>,
    pub row: usize,
    column: usize,
    enabled: bool,
}

impl FamilyBasicKeyboard {
    pub fn new(recorder: Option<DataRecorder>) -> FamilyBasicKeyboard {
        FamilyBasicKeyboard {
            recorder,
            ..Default::default()
        }
    }
}

impl ExpansionDevice for FamilyBasicKeyboard {
    fn set_out(&mut self, value: u8, ppu: &Ppu) {
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.output(value & 1, ppu.time);
        }
        let column = ((value >> 1) & 1) as usize;
        if value & 1 != 0 {
            self.row = 0;
        } else if self.column != 0 && column == 0 {
            self.row = (self.row + 1) % 10;
        }
        self.column = column;
        self.enabled = value & 4 != 0;
    }
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8 {
        if !self.enabled {
            return 0;
        }
        if port == 0 {
            return match self.recorder.as_ref() {
                Some(recorder) => recorder.input(ppu.time) << 1,
                None => 0,
            };
        }
        if self.row >= KEYBOARD_MATRIX.len() {
            return 0x1E;
        }
        let names = &KEYBOARD_MATRIX[self.row][self.column];
        let held: u8 = names
            .iter()
            .enumerate()
            .filter(|(_, name)| self.keys.contains(**name))
            .map(|(i, _)| 1 << (4 - i))
            .sum();
        0x1E & !held
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

/// Konami's Hyper Shot pair on the expansion port (Konami_Hyper_Shot.wiki):
/// OUT1/OUT2 = 0 enable players 1/2; $4017 D1-D4: player 1 Run, Jump, player
/// 2 Run, Jump. `buttons`: those four bits.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KonamiHyperShot {
    pub buttons: u8,
    out: u8,
}

impl Default for KonamiHyperShot {
    fn default() -> Self {
        KonamiHyperShot { buttons: 0, out: 6 }
    }
}

impl ExpansionDevice for KonamiHyperShot {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        self.out = value & 6;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        let enabled =
            (if self.out & 2 != 0 { 0 } else { 0x3 }) | (if self.out & 4 != 0 { 0 } else { 0xC });
        (self.buttons & enabled) << 1
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

// -- serial reports: mice, SNES pads, the Hori Track --------------------------------

/// The reference's `SerialReport`: latches a report when OUT0 falls and shifts
/// it out, then `fill`; while OUT0 is high every read returns `fill`. `bit`
/// is the data line. A device holds one and hands it the report at the fall.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SerialReport {
    pub strobe: bool,
    pub bits: Vec<u8>,
    pub index: usize,
    pub bit: u8,
    pub fill: u8,
}

impl SerialReport {
    pub fn new(bit: u8, fill: u8) -> SerialReport {
        SerialReport {
            strobe: false,
            bits: Vec::new(),
            index: 0,
            bit,
            fill,
        }
    }

    /// Whether this `set_strobe` is OUT0's fall, when a report is latched.
    pub fn falls(&self, high: bool) -> bool {
        self.strobe && !high
    }

    pub fn latch(&mut self, bits: Vec<u8>) {
        self.bits = bits;
        self.index = 0;
    }

    pub fn read(&self) -> u8 {
        if self.strobe {
            return self.fill << self.bit;
        }
        let value = self.bits.get(self.index).copied().unwrap_or(self.fill);
        value << self.bit
    }

    pub fn clock(&mut self) {
        if !self.strobe {
            self.index += 1;
        }
    }
}

fn msb_first(value: i64, width: u32) -> Vec<u8> {
    (0..width)
        .map(|i| ((value >> (width - 1 - i)) & 1) as u8)
        .collect()
}

/// Motion a host feeds in (`move_by`), taken whole at each report.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Pointer {
    pub left: bool,
    pub right: bool,
    dx: i64,
    dy: i64,
}

impl Pointer {
    /// The reference's `move`.
    pub fn move_by(&mut self, dx: i64, dy: i64) {
        self.dx += dx;
        self.dy += dy;
    }

    /// The motion not yet reported (the reference's `_dx`, `_dy`).
    pub fn pending(&self) -> (i64, i64) {
        (self.dx, self.dy)
    }

    fn take(&mut self, limit: i64) -> (i64, i64) {
        let dx = self.dx.clamp(-limit, limit);
        let dy = self.dy.clamp(-limit, limit);
        self.dx -= dx;
        self.dy -= dy;
        (dx, dy)
    }
}

/// A Super NES controller through an adapter (SNES_controller.wiki): 16 bits,
/// B first, the last four 0, then 1s. `buttons`: B Y Select Start Up Down Left
/// Right A X L R in bits 0-11.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SnesController {
    pub serial: SerialReport,
    pub buttons: u16,
}

impl Default for SnesController {
    fn default() -> Self {
        SnesController {
            serial: SerialReport::new(0, 1),
            buttons: 0,
        }
    }
}

impl SnesController {
    fn report(&self) -> Vec<u8> {
        let mut bits: Vec<u8> = (0..12).map(|i| ((self.buttons >> i) & 1) as u8).collect();
        bits.extend([0, 0, 0, 0]);
        bits
    }
}

impl PortDevice for SnesController {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        if self.serial.falls(high) {
            let bits = self.report();
            self.serial.latch(bits);
        }
        self.serial.strobe = high;
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        if self.serial.strobe {
            return (self.buttons & 1) as u8;
        }
        self.serial.read()
    }
    fn clock(&mut self, _: &Ppu) {
        self.serial.clock();
    }
    any_mut!();
}

/// The Super NES Mouse through an adapter (Super_NES_Mouse.wiki): 32 bits MSB
/// first: $00; right, left, sensitivity, signature 0001; then Y and X as sign
/// and magnitude. A clock while OUT0 is high steps the sensitivity.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SnesMouse {
    pub serial: SerialReport,
    pub pointer: Pointer,
    pub sensitivity: i64,
    up: i64,
    leftward: i64,
}

impl Default for SnesMouse {
    fn default() -> Self {
        SnesMouse {
            serial: SerialReport::new(0, 1),
            pointer: Pointer::default(),
            sensitivity: 0,
            up: 0,
            leftward: 0,
        }
    }
}

impl SnesMouse {
    pub const TABLES: [[i64; 8]; 3] = [
        [0, 1, 2, 3, 4, 5, 6, 7],
        [0, 1, 2, 3, 8, 10, 12, 21],
        [0, 1, 4, 9, 12, 20, 24, 28],
    ];

    fn magnitude(&self, value: i64) -> i64 {
        let mut size = value.abs().min(63);
        if self.sensitivity != 0 {
            let table = &Self::TABLES[self.sensitivity as usize];
            size = table[size.min(7) as usize];
        }
        size
    }

    fn report(&mut self) -> Vec<u8> {
        let (dx, dy) = self.pointer.take(63);
        if dy != 0 {
            self.up = (dy < 0) as i64;
        }
        if dx != 0 {
            self.leftward = (dx < 0) as i64;
        }
        let second = (self.pointer.right as i64) << 7
            | (self.pointer.left as i64) << 6
            | self.sensitivity << 4
            | 0x1;
        let third = self.up << 7 | self.magnitude(dy);
        let fourth = self.leftward << 7 | self.magnitude(dx);
        let mut bits = msb_first(0, 8);
        bits.extend(msb_first(second, 8));
        bits.extend(msb_first(third, 8));
        bits.extend(msb_first(fourth, 8));
        bits
    }
}

impl PortDevice for SnesMouse {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        if self.serial.falls(high) {
            let bits = self.report();
            self.serial.latch(bits);
        }
        self.serial.strobe = high;
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        self.serial.read()
    }
    fn clock(&mut self, _: &Ppu) {
        if self.serial.strobe {
            self.sensitivity = (self.sensitivity + 1) % 3;
        } else {
            self.serial.clock();
        }
    }
    any_mut!();
}

/// The Subor Mouse (Subor_Mouse.wiki): 24 bits, bit 23 first: left, right, E,
/// then up, down, left, right movement flags, then |X| and |Y| when E is 1.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SuborMouse {
    pub serial: SerialReport,
    pub pointer: Pointer,
}

impl Default for SuborMouse {
    fn default() -> Self {
        SuborMouse {
            serial: SerialReport::new(0, 1),
            pointer: Pointer::default(),
        }
    }
}

impl SuborMouse {
    fn report(&mut self) -> Vec<u8> {
        let (dx, dy) = self.pointer.take(127);
        let big = dx.abs() > 1 || dy.abs() > 1;
        let mut value = (self.pointer.left as i64) << 23
            | (self.pointer.right as i64) << 22
            | (big as i64) << 21
            | ((dy < 0) as i64) << 19
            | ((dy > 0) as i64) << 18
            | ((dx < 0) as i64) << 17
            | ((dx > 0) as i64) << 16;
        if big {
            value |= dx.abs() << 8 | dy.abs();
        }
        msb_first(value, 24)
    }
}

/// The Macro Winners (GameStar) Mouse, and the Mega Book's
/// (Macro_Winners_Mouse.wiki): one byte per strobe, MSB first; motion beyond
/// one unit takes three.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MacroWinnersMouse {
    pub serial: SerialReport,
    pub pointer: Pointer,
    pub mega_book: bool,
    pending: Vec<i64>,
}

impl MacroWinnersMouse {
    pub fn new(mega_book: bool) -> MacroWinnersMouse {
        MacroWinnersMouse {
            serial: SerialReport::new(0, 1),
            pointer: Pointer::default(),
            mega_book,
            pending: Vec::new(),
        }
    }

    fn report(&mut self) -> Vec<u8> {
        if self.pending.is_empty() {
            let (dx, dy) = self.pointer.take(31);
            let buttons = (self.pointer.left as i64) << 7 | (self.pointer.right as i64) << 6;
            if dx.abs() <= 1 && dy.abs() <= 1 {
                let x = if dx < 0 { 3 } else { dx };
                let y = if dy < 0 { 3 } else { dy };
                let byte = buttons
                    | if self.mega_book {
                        x << 2 | y
                    } else {
                        x << 4 | y << 2
                    };
                self.pending = vec![byte];
            } else {
                let (sx, sy) = ((dx < 0) as i64, (dy < 0) as i64);
                let (ax, ay) = (dx.abs(), dy.abs());
                if self.mega_book {
                    let first = buttons | 0x10 | sx << 3 | (ax >> 4) << 2 | sy << 1 | ay >> 4;
                    self.pending = vec![first, 0x20 | (ax & 15), 0x30 | (ay & 15)];
                } else {
                    let first = buttons | sx << 5 | (ax >> 4) << 4 | sy << 3 | (ay >> 4) << 2 | 1;
                    self.pending = vec![first, (ax & 15) << 2 | 2, (ay & 15) << 2 | 3];
                }
            }
        }
        let byte = self.pending.remove(0);
        msb_first(byte, 8)
    }
}

/// The Yuxing Mouse (Yuxing_Mouse.wiki): a serial mouse with X and Y swapped,
/// strobed by writing 0 then 1, its data inverted; three 9-bit packets. The
/// data bits' order is taken as MSB first (UNVERIFIED).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct YuxingMouse {
    pub serial: SerialReport,
    pub pointer: Pointer,
}

impl Default for YuxingMouse {
    fn default() -> Self {
        YuxingMouse {
            serial: SerialReport::new(0, 0),
            pointer: Pointer::default(),
        }
    }
}

impl YuxingMouse {
    fn report(&mut self) -> Vec<u8> {
        let (dx, dy) = self.pointer.take(127);
        let (x, y) = (dx & 0xFF, dy & 0xFF);
        let first = 1 << 6
            | (!self.pointer.left as i64) << 5
            | (self.pointer.right as i64) << 4
            | (x >> 6) << 2
            | y >> 6;
        let mut bits = Vec::new();
        for data in [first, y & 0x3F, x & 0x3F] {
            bits.push(0);
            bits.extend(msb_first(data, 7));
            bits.push(1);
        }
        bits
    }
}

macro_rules! serial_port_device {
    ($t:ty) => {
        impl PortDevice for $t {
            fn set_strobe(&mut self, high: bool, _: &Ppu) {
                if self.serial.falls(high) {
                    let bits = self.report();
                    self.serial.latch(bits);
                }
                self.serial.strobe = high;
            }
            fn read(&mut self, _: &Ppu) -> u8 {
                self.serial.read()
            }
            fn clock(&mut self, _: &Ppu) {
                self.serial.clock();
            }
            any_mut!();
        }
    };
}

serial_port_device!(SuborMouse);
serial_port_device!(MacroWinnersMouse);

impl PortDevice for YuxingMouse {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        if high && !self.serial.strobe {
            // The inverted strobe: a rise latches.
            let bits = self.report();
            self.serial.latch(bits);
        }
        self.serial.strobe = high;
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        let serial = &self.serial;
        let value = serial.bits.get(serial.index).copied().unwrap_or(1);
        (value ^ 1) << serial.bit
    }
    fn clock(&mut self, _: &Ppu) {
        self.serial.index += 1;
    }
    any_mut!();
}

/// The Hori Track (Hori_Track.wiki) on the Famicom expansion port, $4016 D1
/// (UNVERIFIED): its pad, A first; the two axes, signed 4 bits XOR $F; rotation
/// mode, speed, ID; then 1s (UNVERIFIED). `pad`: its buttons.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HoriTrack {
    pub serial: SerialReport,
    pub pointer: Pointer,
    pub pad: StandardController,
    pub rotation_l: bool,
    pub slow: bool,
}

impl Default for HoriTrack {
    fn default() -> Self {
        HoriTrack {
            serial: SerialReport::new(1, 1),
            pointer: Pointer::default(),
            pad: StandardController::default(),
            rotation_l: false,
            slow: false,
        }
    }
}

impl HoriTrack {
    fn report(&mut self) -> Vec<u8> {
        let (dx, dy) = self.pointer.take(7);
        let (axis1, axis2) = if self.rotation_l {
            (-dy, dx)
        } else {
            (-dx, dy)
        };
        let axes = ((axis1 & 15) ^ 15) << 4 | ((axis2 & 15) ^ 15);
        let third = (self.rotation_l as i64) << 7 | (self.slow as i64) << 6 | 1 << 4;
        let mut bits: Vec<u8> = (0..8).map(|i| (self.pad.buttons >> i) & 1).collect();
        bits.extend(msb_first(axes, 8));
        bits.extend(msb_first(third, 8));
        bits
    }
}

impl ExpansionDevice for HoriTrack {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        let high = value & 1 != 0;
        if self.serial.falls(high) {
            let bits = self.report();
            self.serial.latch(bits);
        }
        self.serial.strobe = high;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            self.serial.read()
        } else {
            0
        }
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        if port == 0 {
            self.serial.clock();
        }
    }
    any_mut!();
}

// -- the Subor Keyboard ---------------------------------------------------------------

/// MAME's src/devices/bus/nes_ctrl/suborkey.cpp (BSD-3-Clause, Fabio Priuli):
/// 13 rows of 8 keys, bit 0 first; names as PC keys.
pub const SUBOR_MATRIX: [[&str; 8]; 13] = [
    ["4", "G", "F", "C", "F2", "E", "5", "V"],
    ["2", "D", "S", "END", "F1", "W", "3", "X"],
    [
        "INSERT",
        "BACKSPACE",
        "PAGEDOWN",
        "RIGHT",
        "F8",
        "PAGEUP",
        "DELETE",
        "HOME",
    ],
    ["9", "I", "L", ",", "F5", "O", "0", "."],
    ["]", "ENTER", "UP", "LEFT", "F7", "[", "\\", "DOWN"],
    ["Q", "CAPSLOCK", "Z", "TAB", "ESC", "A", "1", "LCTRL"],
    ["7", "Y", "K", "M", "F4", "U", "8", "J"],
    ["-", ";", "'", "/", "F6", "P", "=", "LSHIFT"],
    ["T", "H", "N", "SPACE", "F3", "R", "6", "B"],
    ["", "", "", "", "", "", "", ""],
    ["LALT", "KP4", "KP7", "F11", "F12", "KP1", "KP2", "KP8"],
    ["KP-", "KP+", "KP*", "KP9", "F10", "KP5", "KP/", "NUMLOCK"],
    ["`", "KP6", "PAUSE", "SPACE2", "F9", "KP3", "KP.", "KP0"],
];

/// The Subor (Xiao Ba Wang) Keyboard on the expansion port, as MAME's
/// suborkey: while OUT2 is 1, OUT1 selects the row's low or high four keys,
/// its fall moves to the next row, OUT0 resets to row 0. $4017 D1-D4: the
/// selected four keys, 0 = held.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SuborKeyboard {
    pub keys: HashSet<String>,
    pub row: usize,
    high: usize,
}

impl ExpansionDevice for SuborKeyboard {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        if value & 4 == 0 {
            return;
        }
        let high = ((value >> 1) & 1) as usize;
        if self.high != 0 && high == 0 {
            self.row = if self.row < 12 { self.row + 1 } else { 0 };
        }
        self.high = high;
        if value & 1 != 0 {
            self.row = 0;
        }
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        if self.row >= SUBOR_MATRIX.len() {
            return 0x1E;
        }
        let names = &SUBOR_MATRIX[self.row][4 * self.high..4 * self.high + 4];
        let held: u8 = names
            .iter()
            .enumerate()
            .filter(|(_, name)| !name.is_empty() && self.keys.contains(**name))
            .map(|(i, _)| 1 << i)
            .sum();
        !(held << 1) & 0x1E
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

// -- storage: the Turbo File and the Data Recorder ----------------------------------------

/// ASCII's Turbo File (AS-TFO2) on the expansion port: 8 KiB of battery RAM a
/// bit at a time (Turbo_File.wiki; MAME's turbofile.cpp, BSD-3). OUT1 = 0
/// resets; a fall of OUT2 stores OUT0 and steps. $4017 D2: the current bit.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TurboFile {
    pub data: Vec<u8>,
    pub locked: bool,
    address: usize,
    bit: u32,
    out: u8,
}

impl TurboFile {
    pub fn new(data: Option<&[u8]>) -> TurboFile {
        let data = match data {
            Some(bytes) if !bytes.is_empty() => bytes.to_vec(),
            _ => vec![0; 0x2000],
        };
        TurboFile {
            data,
            locked: false,
            address: 0,
            bit: 0,
            out: 0,
        }
    }
}

impl ExpansionDevice for TurboFile {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        if value & 2 == 0 {
            self.address = 0;
            self.bit = 0;
        }
        if self.out & 4 != 0 && value & 4 == 0 {
            if !self.locked {
                let mask = 1u8 << self.bit;
                let byte = self.data[self.address] & !mask;
                self.data[self.address] = byte | (value & 1) << self.bit;
            }
            self.bit = (self.bit + 1) & 7;
            if self.bit == 0 {
                self.address = (self.address + 1) & 0x1FFF;
            }
        }
        self.out = value;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        ((self.data[self.address] >> self.bit) & 1) << 2
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

/// The Data Recorder's keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TapeMode {
    Stop,
    Play,
    Record,
}

/// The Family BASIC Data Recorder (HVC-008), or any cassette deck on its jacks
/// (Family_BASIC_Data_Recorder.wiki): the tape is the list of level changes at
/// master-tick positions. Times are `ppu.time`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DataRecorder {
    pub tape: Vec<(i64, u8)>,
    pub mode: TapeMode,
    start: i64,
    last: i32,
}

impl DataRecorder {
    pub fn new(tape: Vec<(i64, u8)>) -> DataRecorder {
        DataRecorder {
            tape,
            mode: TapeMode::Stop,
            start: 0,
            last: 0,
        }
    }

    pub fn play(&mut self, now: i64) {
        self.mode = TapeMode::Play;
        self.start = now;
    }

    pub fn record(&mut self, now: i64) {
        self.mode = TapeMode::Record;
        self.start = now;
        self.tape = Vec::new();
        self.last = -1;
    }

    pub fn stop(&mut self) {
        self.mode = TapeMode::Stop;
    }

    pub fn output(&mut self, level: u8, now: i64) {
        if self.mode == TapeMode::Record && level as i32 != self.last {
            self.tape.push((now - self.start, level));
            self.last = level as i32;
        }
    }

    pub fn input(&self, now: i64) -> u8 {
        if self.mode != TapeMode::Play || self.tape.is_empty() {
            return 0;
        }
        let position = now - self.start;
        if position > self.tape[self.tape.len() - 1].0 + 1_000_000 {
            return 0;
        }
        // bisect_right(tape, (position, 2)) - 1: the last change at or before.
        let index = self.tape.partition_point(|&(at, _)| at <= position);
        if index > 0 {
            self.tape[index - 1].1
        } else {
            0
        }
    }
}

// -- the tail: light guns, push buttons, dials, tablets, glasses, robots ---------------------

/// Bandai's Hyper Shot on the expansion port (Bandai_Hyper_Shot.wiki): a pad
/// on $4016 D1 (its A never reads pressed) and a Zapper on $4017 D3-D4 whose
/// trigger reads 1 while fully pulled. OUT1 and OUT2: `rumble`, `sound`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BandaiHyperShot {
    pub pad: StandardController,
    pub zapper: Zapper,
    pub rumble: bool,
    pub sound: bool,
}

impl BandaiHyperShot {
    pub fn new(ppu: &Ppu, palette: &[[u8; 3]]) -> BandaiHyperShot {
        BandaiHyperShot {
            pad: StandardController::default(),
            zapper: Zapper::new(ppu, palette),
            rumble: false,
            sound: false,
        }
    }
}

impl ExpansionDevice for BandaiHyperShot {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        self.pad.set_strobe(value & 1 != 0);
        self.rumble = value & 2 != 0;
        self.sound = value & 4 != 0;
    }
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8 {
        if port == 0 {
            let buttons = self.pad.buttons;
            self.pad.buttons = buttons & !1; // no A
            let value = self.pad.read() << 1;
            self.pad.buttons = buttons;
            return value;
        }
        let zapper = &self.zapper;
        (if zapper.light(ppu) { 0 } else { 0x08 }) | (if zapper.trigger() { 0x10 } else { 0 })
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        if port == 0 {
            self.pad.clock();
        }
    }
    any_mut!();
}

/// Yonezawa's Partytap on the expansion port (Partytap.wiki): six 1-button
/// pads; after a strobe $4017 D2-D4 give buttons 1-3, then 4-6, then $14.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Partytap {
    pub buttons: u8,
    pub strobe: bool,
    reads: Vec<u8>,
    index: usize,
}

impl ExpansionDevice for Partytap {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        let high = value & 1 != 0;
        if self.strobe && !high {
            let b = self.buttons;
            self.reads = vec![(b & 7) << 2, ((b >> 3) & 7) << 2, 0x14];
            self.index = 0;
        }
        self.strobe = high;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 || self.strobe {
            return 0;
        }
        let reads = &self.reads;
        if reads.is_empty() {
            0
        } else {
            reads[self.index.min(reads.len() - 1)]
        }
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        if port != 0 && !self.strobe {
            self.index += 1;
        }
    }
    any_mut!();
}

/// Konami's Exciting Boxing punching bag on the expansion port
/// (Exciting_Boxing_Punching_Bag.wiki; active low, as MAME's konamibag.cpp):
/// OUT1 picks the sensor group; `sensors`: bits 0-3 the first, 4-7 the second.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExcitingBoxing {
    pub sensors: u8,
    group: u8,
}

impl ExpansionDevice for ExcitingBoxing {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        self.group = (value >> 1) & 1;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        !(((self.sensors >> (4 * self.group)) & 15) << 1) & 0x1E
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

/// Coconuts Japan's pachinko controller (Hori CJPC-102) on the expansion port,
/// $4016 D1 (Coconuts_Japan_Pachinko_Controller.wiki): its pad's 8 bits, a 1,
/// then the dial inverted, MSB first, then 1s. `dial`: $03 (rest) to about $73.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CoconutsPachinko {
    pub serial: SerialReport,
    pub pad: StandardController,
    pub dial: i64,
}

impl Default for CoconutsPachinko {
    fn default() -> Self {
        CoconutsPachinko {
            serial: SerialReport::new(1, 1),
            pad: StandardController::default(),
            dial: 0x04,
        }
    }
}

impl CoconutsPachinko {
    fn report(&self) -> Vec<u8> {
        let dial = self.dial.clamp(0, 0x7F);
        let mut bits: Vec<u8> = (0..8).map(|i| (self.pad.buttons >> i) & 1).collect();
        bits.push(1);
        bits.extend(msb_first(dial ^ 0x7F, 7));
        bits
    }
}

impl ExpansionDevice for CoconutsPachinko {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        let high = value & 1 != 0;
        if self.serial.falls(high) {
            let bits = self.report();
            self.serial.latch(bits);
        }
        self.serial.strobe = high;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port != 0 {
            return 0;
        }
        if self.serial.strobe {
            return (self.pad.buttons & 1) << 1;
        }
        self.serial.read()
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        if port == 0 {
            self.serial.clock();
        }
    }
    any_mut!();
}

pub const JISSEN_MAHJONG_ROWS: [[&str; 8]; 3] = [
    ["", "", "N", "M", "L", "K", "J", "I"],
    ["H", "G", "F", "E", "D", "C", "B", "A"],
    ["", "RON", "RIICHI", "CHII", "PON", "KAN", "START", "SELECT"],
];

/// Capcom's Jissen Mahjong controller on the expansion port
/// (Jissen_Mahjong_controller.wiki): while OUT0 is 1, OUT2-OUT1 pick the row
/// (0: rows 1 and 2 ORed); after the fall $4017 D1 gives 8 bits, then 1s.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JissenMahjong {
    pub keys: HashSet<String>,
    pub strobe: bool,
    row: usize,
    shift: u32,
}

impl Default for JissenMahjong {
    fn default() -> Self {
        JissenMahjong {
            keys: HashSet::new(),
            strobe: false,
            row: 0,
            shift: 0xFF,
        }
    }
}

impl JissenMahjong {
    fn load(&mut self) {
        let rows: &[[&str; 8]] = if self.row == 0 {
            &JISSEN_MAHJONG_ROWS[0..2]
        } else {
            &JISSEN_MAHJONG_ROWS[self.row - 1..self.row]
        };
        let mut value = 0u32;
        for row in rows {
            value |= row
                .iter()
                .enumerate()
                .filter(|(_, name)| !name.is_empty() && self.keys.contains(**name))
                .map(|(i, _)| 1u32 << i)
                .sum::<u32>();
        }
        self.shift = value;
    }
}

impl ExpansionDevice for JissenMahjong {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        self.row = ((value >> 1) & 3) as usize;
        let high = value & 1 != 0;
        if high || self.strobe {
            self.load();
        }
        self.strobe = high;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        if self.strobe {
            self.load();
        }
        ((self.shift & 1) << 1) as u8
    }
    fn clock(&mut self, port: usize, _: &Ppu) {
        if port != 0 && !self.strobe {
            self.shift = self.shift >> 1 | 0x80;
        }
    }
    any_mut!();
}

/// Bandai's Oeka Kids tablet on the expansion port (Oeka_Kids_tablet.wiki):
/// OUT0 = 0 latches a report; with OUT0 = 1 a rise of OUT1 moves to the next
/// bit. $4017 D2: the acknowledgement; D3: the current bit inverted. The
/// report, MSB first: X (0-239), Y (0-255), touching, clicking.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct OekaKidsTablet {
    pub x: i64,
    pub y: i64,
    pub touching: bool,
    pub click: bool,
    out: u8,
    bits: Vec<u8>,
    index: i64,
}

impl ExpansionDevice for OekaKidsTablet {
    fn set_out(&mut self, value: u8, _: &Ppu) {
        let (strobe, advance) = (value & 1, (value >> 1) & 1);
        if strobe == 0 {
            let x = self.x.clamp(0, 239);
            let y = self.y.clamp(0, 255);
            let mut bits = msb_first(x, 8);
            bits.extend(msb_first(y, 8));
            bits.push(self.touching as u8);
            bits.push(self.click as u8);
            self.bits = bits;
            self.index = -1;
        } else if advance != 0 && (self.out >> 1) & 1 == 0 {
            self.index += 1;
        }
        self.out = value & 3;
    }
    fn read(&mut self, port: usize, _: &Ppu) -> u8 {
        if port == 0 {
            return 0;
        }
        let (strobe, advance) = (self.out & 1, (self.out >> 1) & 1);
        let ack = if strobe != 0 && advance != 0 { 0 } else { 0x04 };
        if strobe == 0 {
            return ack;
        }
        let index = self.index;
        let bit = if 0 <= index && (index as usize) < self.bits.len() {
            self.bits[index as usize]
        } else {
            0
        };
        ack | (bit ^ 1) << 3
    }
    fn clock(&mut self, _: usize, _: &Ppu) {}
    any_mut!();
}

/// The Famicom 3D System's control box (Famicom_3D_System.wiki): OUT1 says
/// which eye the next field is for (`eye`); it passes the expansion port
/// through, here to `through` (players 3 and 4).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Famicom3DSystem {
    pub eye: u8,
    pub through: ExpansionPads,
}

impl ExpansionDevice for Famicom3DSystem {
    fn set_out(&mut self, value: u8, ppu: &Ppu) {
        self.eye = (value >> 1) & 1;
        self.through.set_out(value, ppu);
    }
    fn read(&mut self, port: usize, ppu: &Ppu) -> u8 {
        self.through.read(port, ppu)
    }
    fn clock(&mut self, port: usize, ppu: &Ppu) {
        self.through.clock(port, ppu);
    }
    any_mut!();
}

/// One port of the factory port test controller (Port_test_controller.wiki):
/// a pad on D0, and a signature on D4 from a shift register that D3 taps two
/// bits later. Port 1's signature is 1, 0, 1, 0... then 1s; port 2's the inverse.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PortTestPort {
    pub pad: StandardController,
    pub strobe: bool,
    pattern: [u8; 8],
    fill: u8,
    index: usize,
}

impl PortTestPort {
    pub fn new(second: bool) -> PortTestPort {
        let flip = if second { 0 } else { 1 };
        PortTestPort {
            pad: StandardController::default(),
            strobe: false,
            pattern: std::array::from_fn(|i| (i as u8 & 1) ^ flip),
            fill: if second { 0 } else { 1 },
            index: 0,
        }
    }

    fn bit(&self, index: usize) -> u8 {
        if index < 8 {
            self.pattern[index]
        } else {
            self.fill
        }
    }
}

impl PortDevice for PortTestPort {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        self.pad.set_strobe(high);
        self.strobe = high;
        if high {
            self.index = 0;
        }
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        let index = self.index;
        self.pad.read() | self.bit(index + 2) << 3 | self.bit(index) << 4
    }
    fn clock(&mut self, _: &Ppu) {
        self.pad.clock();
        if !self.strobe {
            self.index += 1;
        }
    }
    any_mut!();
}

/// R.O.B.'s commands (R.O.B..wiki): the four bits w x y z after 000101 in 000101w1x1y1z.
pub fn rob_command(bits: u32) -> Option<&'static str> {
    Some(match bits {
        0b0101 => "up 2",
        0b1100 => "up 1",
        0b0010 => "down 1",
        0b1101 => "down 2",
        0b0100 => "left",
        0b1000 => "right",
        0b0110 => "close arms",
        0b1010 => "open arms",
        0b1001 => "LED on",
        0b0001 => "reset",
        _ => return None,
    })
}

/// R.O.B. (the Family Computer Robot), which watches the TV, not the bus
/// (R.O.B..wiki): one bit per frame, bright 1 and black 0. `see` takes each
/// frame the board drew. The decoded state: `height` 0-5, `turn` -2 to 2,
/// `arms_closed`, `led`, `commands`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rob {
    bright: Vec<bool>,
    bits: u32,
    pub height: i32,
    pub turn: i32,
    pub arms_closed: bool,
    pub led: bool,
    pub commands: Vec<String>,
}

impl Rob {
    pub fn new(palette: &[[u8; 3]]) -> Rob {
        Rob {
            bright: palette.iter().map(|rgb| luminance(rgb) > 60.0).collect(),
            bits: 0,
            height: 2,
            turn: 0,
            arms_closed: false,
            led: false,
            commands: Vec::new(),
        }
    }

    pub fn see(&mut self, picture: &[u16]) -> Option<&'static str> {
        let mut lit = 0;
        for y in (80..160).step_by(8) {
            for x in (64..192).step_by(8) {
                lit += self.bright[picture[y * 256 + x] as usize] as u32;
            }
        }
        self.bits = ((self.bits << 1) | (lit > 80) as u32) & 0x1FFF;
        // 000101w1x1y1z, oldest bit highest: the fixed bits are 12-7, 5, 3 and 1.
        if self.bits & 0b1111110101010 != 0b0001010101010 {
            return None;
        }
        let bits = self.bits;
        let command =
            ((bits >> 6) & 1) << 3 | ((bits >> 4) & 1) << 2 | ((bits >> 2) & 1) << 1 | (bits & 1);
        let name = rob_command(command)?;
        self.bits = 0;
        self.act(name);
        self.commands.push(name.to_string());
        Some(name)
    }

    fn act(&mut self, name: &str) {
        if let Some(n) = name.strip_prefix("up ") {
            self.height = (self.height + n.parse::<i32>().unwrap()).min(5);
        } else if let Some(n) = name.strip_prefix("down ") {
            self.height = (self.height - n.parse::<i32>().unwrap()).max(0);
        } else if name == "left" {
            self.turn = (self.turn - 1).max(-2);
        } else if name == "right" {
            self.turn = (self.turn + 1).min(2);
        } else if name == "close arms" {
            self.arms_closed = true;
        } else if name == "open arms" {
            self.arms_closed = false;
        } else if name == "LED on" {
            self.led = true;
        } else if name == "reset" {
            self.led = false;
            self.arms_closed = false;
            self.height = 2;
            self.turn = 0;
        }
    }
}

// -- choosing devices -----------------------------------------------------------------

// NES_2.0.wiki "Default Expansion Device": the numbers built here.
pub const STANDARD: u8 = 0x01;
pub const FOUR_SCORE: u8 = 0x02;
pub const FAMICOM_FOUR_PLAYERS: u8 = 0x03;
pub const ZAPPER: u8 = 0x08;
pub const TWO_ZAPPERS: u8 = 0x09;
pub const POWER_PAD_A: u8 = 0x0B;
pub const POWER_PAD_B: u8 = 0x0C;
pub const FAMILY_TRAINER_A: u8 = 0x0D;
pub const FAMILY_TRAINER_B: u8 = 0x0E;
pub const VAUS_NES: u8 = 0x0F;
pub const VAUS_FAMICOM: u8 = 0x10;
/// Arkanoid II: its recorder needs the keyboard, not built here.
pub const VAUS_TWO_AND_RECORDER: u8 = 0x11;
pub const KONAMI_HYPER_SHOT: u8 = 0x12;
pub const TAPTAP_MAT: u8 = 0x1A;
pub const BANDAI_HYPER_SHOT: u8 = 0x0A;
pub const PACHINKO: u8 = 0x13;
pub const BOXING_BAG: u8 = 0x14;
pub const JISSEN_MAHJONG: u8 = 0x15;
pub const PARTYTAP: u8 = 0x16;
pub const OEKA_KIDS: u8 = 0x17;
pub const FAMICOM_3D: u8 = 0x1D;
pub const ROB_GYROMITE: u8 = 0x1F;
pub const ROB_STACK_UP: u8 = 0x2E;
pub const PORT_TEST: u8 = 0x37;
pub const DATA_RECORDER: u8 = 0x20;
pub const TURBO_FILE: u8 = 0x21;
pub const FAMILY_BASIC_KEYBOARD: u8 = 0x23;
pub const SUBOR_KEYBOARD: u8 = 0x26;
pub const SUBOR_KEYBOARD_MACRO_WINNERS: u8 = 0x27;
pub const SUBOR_KEYBOARD_MOUSE_4016: u8 = 0x28;
pub const SNES_MOUSE_4016: u8 = 0x29;
pub const SNES_CONTROLLERS: u8 = 0x2B;
pub const SUBOR_KEYBOARD_MOUSE_4017: u8 = 0x36;
pub const YUXING_MOUSE: u8 = 0x43;
pub const SUBOR_KEYBOARD_YUXING: u8 = 0x44;
pub const SNES_MOUSE_4017: u8 = 0x48;
pub const ZAPPER_4016: u8 = 0x49;
pub const SUBOR_KEYBOARD_MEGA_BOOK: u8 = 0x4F;
pub const HORI_TRACK: u8 = 0x50;

/// Devices only the NES's controller ports take (`peripherals.NES_ONLY`).
pub fn nes_only(device: u8) -> bool {
    matches!(
        device,
        FOUR_SCORE | POWER_PAD_A | POWER_PAD_B | VAUS_NES | TWO_ZAPPERS
    )
}

/// `DEVICE_NAMES`.
pub fn device_name(device: u8) -> Option<&'static str> {
    Some(match device {
        STANDARD => "standard controllers",
        FOUR_SCORE => "NES Four Score",
        FAMICOM_FOUR_PLAYERS => "Famicom four-player adapter (simple)",
        ZAPPER => "Zapper",
        TWO_ZAPPERS => "two Zappers",
        POWER_PAD_A => "Power Pad (side A)",
        POWER_PAD_B => "Power Pad (side B)",
        FAMILY_TRAINER_A => "Family Trainer Mat (side A)",
        FAMILY_TRAINER_B => "Family Trainer Mat (side B)",
        VAUS_NES => "Arkanoid Vaus (NES)",
        VAUS_FAMICOM => "Arkanoid Vaus (Famicom)",
        VAUS_TWO_AND_RECORDER => "two Arkanoid Vaus (Famicom; no data recorder)",
        KONAMI_HYPER_SHOT => "Konami Hyper Shot",
        TAPTAP_MAT => "Tap-tap Mat",
        DATA_RECORDER => "Family BASIC Data Recorder",
        TURBO_FILE => "ASCII Turbo File",
        FAMILY_BASIC_KEYBOARD => "Family BASIC Keyboard and Data Recorder",
        SUBOR_KEYBOARD => "Subor Keyboard",
        SUBOR_KEYBOARD_MACRO_WINNERS => "Subor Keyboard and Macro Winners Mouse",
        SUBOR_KEYBOARD_MOUSE_4016 => "Subor Keyboard and Subor Mouse ($4016)",
        SNES_MOUSE_4016 => "Super NES Mouse ($4016)",
        SNES_CONTROLLERS => "two Super NES controllers",
        SUBOR_KEYBOARD_MOUSE_4017 => "Subor Keyboard and Subor Mouse ($4017)",
        YUXING_MOUSE => "Yuxing Mouse ($4016)",
        SUBOR_KEYBOARD_YUXING => "Subor Keyboard and Yuxing Mouse ($4016)",
        SNES_MOUSE_4017 => "Super NES Mouse ($4017)",
        ZAPPER_4016 => "Zapper ($4016)",
        SUBOR_KEYBOARD_MEGA_BOOK => "Subor Keyboard and Mega Book Mouse",
        HORI_TRACK => "Hori Track",
        BANDAI_HYPER_SHOT => "Bandai Hyper Shot",
        PACHINKO => "Coconuts Japan Pachinko Controller",
        BOXING_BAG => "Exciting Boxing punching bag",
        JISSEN_MAHJONG => "Jissen Mahjong controller",
        PARTYTAP => "Partytap",
        OEKA_KIDS => "Oeka Kids tablet",
        FAMICOM_3D => "Famicom 3D System",
        ROB_GYROMITE => "R.O.B. (Gyromite)",
        ROB_STACK_UP => "R.O.B. (Stack-Up)",
        PORT_TEST => "port test controller",
        _ => return None,
    })
}

/// What [`plug`] put in, and where the device the player drives is: the
/// reference's return value, as a place on the board.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Plugged {
    /// Standard pads, or a number not built here (the reference's None).
    None,
    /// The Four Score: `board.pads` are its two [`TapPort`]s; player `n` is
    /// [`TapPort::player`].
    FourScore,
    /// `board.pads[port]` itself.
    Pad(usize),
    /// Both `board.pads` (the reference's list of them).
    Pads,
    /// `board.expansion` itself.
    Expansion,
    /// The [`Zapper`] inside `board.expansion`'s [`FamicomZapper`].
    ExpansionZapper,
    /// The [`Vaus`] inside `board.pads[port]`'s [`VausNes`].
    PadVaus(usize),
    /// The first [`Vaus`] of `board.expansion`'s [`VausFamicom`].
    ExpansionVaus,
    /// Both [`Vaus`]es of `board.expansion`'s [`VausFamicom`] (`vaus`, `second`).
    VausPair,
    /// The [`DataRecorder`] of `board.expansion`'s [`FamilyBasicKeyboard`].
    Recorder,
    /// The [`SuborKeyboard`] on `board.expansion` and its mouse on `board.pads[port]`.
    SuborPair(usize),
    /// R.O.B.: nothing on the bus; the host shows it each frame.
    Rob(Rob),
}

/// The Subor Keyboard's sets: the mouse's port and kind (NES_2.0.wiki).
fn subor_mouse(device: u8) -> Option<(usize, Option<Box<dyn PortDevice>>)> {
    Some(match device {
        SUBOR_KEYBOARD => (0, None),
        SUBOR_KEYBOARD_MACRO_WINNERS => (1, Some(Box::new(MacroWinnersMouse::new(false)))),
        SUBOR_KEYBOARD_MOUSE_4016 => (0, Some(Box::new(SuborMouse::default()))),
        SUBOR_KEYBOARD_MOUSE_4017 => (1, Some(Box::new(SuborMouse::default()))),
        SUBOR_KEYBOARD_YUXING => (0, Some(Box::new(YuxingMouse::default()))),
        SUBOR_KEYBOARD_MEGA_BOOK => (1, Some(Box::new(MacroWinnersMouse::new(true)))),
        _ => return None,
    })
}

/// Put in the devices of NES 2.0 expansion number `device`; return where the
/// one the player drives is. `famicom`: the console has the Famicom's
/// expansion port, which takes the Famicom versions of the Zapper and the
/// Vaus; the NES takes them on port 2. `palette`: the light sensors' palette,
/// by default the timing's pally file or, without it, [`grey_palette`].
pub fn plug(
    board: &mut Board,
    device: u8,
    famicom: bool,
    palette: Option<Vec<[u8; 3]>>,
) -> Plugged {
    let ticks = board.ppu.timing.master_hz;
    let palette = palette.unwrap_or_else(|| {
        let timing = &board.ppu.timing;
        crate::video::load_palette(&crate::video::palette_for(timing), timing.swapped_emphasis)
            .unwrap_or_else(|_| grey_palette())
    });
    let ppu = &board.ppu;
    match device {
        FOUR_SCORE => {
            let [a, b] = four_score();
            board.pads = [Box::new(a), Box::new(b)];
            Plugged::FourScore
        }
        FAMICOM_FOUR_PLAYERS => {
            board.expansion = Some(Box::new(ExpansionPads::default()));
            Plugged::Expansion
        }
        ZAPPER => {
            let zapper = Zapper::new(ppu, &palette);
            if famicom {
                board.expansion = Some(Box::new(FamicomZapper { zapper }));
                return Plugged::ExpansionZapper;
            }
            board.pads[1] = Box::new(zapper);
            Plugged::Pad(1)
        }
        TWO_ZAPPERS => {
            board.pads = [
                Box::new(Zapper::new(ppu, &palette)),
                Box::new(Zapper::new(ppu, &palette)),
            ];
            Plugged::Pads
        }
        POWER_PAD_A | POWER_PAD_B => {
            board.pads[1] = Box::new(PowerPad::default());
            Plugged::Pad(1)
        }
        FAMILY_TRAINER_A | FAMILY_TRAINER_B | TAPTAP_MAT => {
            board.expansion = Some(Box::new(FamilyTrainer::default()));
            Plugged::Expansion
        }
        VAUS_TWO_AND_RECORDER => {
            board.expansion = Some(Box::new(VausFamicom {
                vaus: Vaus::new(ticks),
                second: Some(Vaus::new(ticks)),
            }));
            Plugged::VausPair
        }
        _ if device == VAUS_NES || (device == VAUS_FAMICOM && !famicom) => {
            board.pads[1] = Box::new(VausNes {
                vaus: Vaus::new(ticks),
            });
            Plugged::PadVaus(1)
        }
        VAUS_FAMICOM => {
            board.expansion = Some(Box::new(VausFamicom {
                vaus: Vaus::new(ticks),
                second: None,
            }));
            Plugged::ExpansionVaus
        }
        KONAMI_HYPER_SHOT => {
            board.expansion = Some(Box::new(KonamiHyperShot::default()));
            Plugged::Expansion
        }
        FAMILY_BASIC_KEYBOARD | DATA_RECORDER => {
            let recorder = DataRecorder::new(Vec::new());
            board.expansion = Some(Box::new(FamilyBasicKeyboard::new(Some(recorder))));
            if device == FAMILY_BASIC_KEYBOARD {
                Plugged::Expansion
            } else {
                Plugged::Recorder
            }
        }
        TURBO_FILE => {
            board.expansion = Some(Box::new(TurboFile::new(None)));
            Plugged::Expansion
        }
        ZAPPER_4016 => {
            board.pads[0] = Box::new(Zapper::new(ppu, &palette));
            Plugged::Pad(0)
        }
        SNES_MOUSE_4016 | SNES_MOUSE_4017 => {
            let port = if device == SNES_MOUSE_4016 { 0 } else { 1 };
            board.pads[port] = Box::new(SnesMouse::default());
            Plugged::Pad(port)
        }
        SNES_CONTROLLERS => {
            board.pads = [
                Box::new(SnesController::default()),
                Box::new(SnesController::default()),
            ];
            Plugged::Pads
        }
        YUXING_MOUSE => {
            board.pads[0] = Box::new(YuxingMouse::default());
            Plugged::Pad(0)
        }
        HORI_TRACK => {
            board.expansion = Some(Box::new(HoriTrack::default()));
            Plugged::Expansion
        }
        PACHINKO | BOXING_BAG | JISSEN_MAHJONG | PARTYTAP | OEKA_KIDS | FAMICOM_3D => {
            let device: Box<dyn ExpansionDevice> = match device {
                PACHINKO => Box::new(CoconutsPachinko::default()),
                BOXING_BAG => Box::new(ExcitingBoxing::default()),
                JISSEN_MAHJONG => Box::new(JissenMahjong::default()),
                PARTYTAP => Box::new(Partytap::default()),
                OEKA_KIDS => Box::new(OekaKidsTablet::default()),
                _ => Box::new(Famicom3DSystem::default()),
            };
            board.expansion = Some(device);
            Plugged::Expansion
        }
        BANDAI_HYPER_SHOT => {
            board.expansion = Some(Box::new(BandaiHyperShot::new(ppu, &palette)));
            Plugged::Expansion
        }
        // It watches the picture; nothing on the bus.
        ROB_GYROMITE | ROB_STACK_UP => Plugged::Rob(Rob::new(&palette)),
        PORT_TEST => {
            board.pads = [
                Box::new(PortTestPort::new(false)),
                Box::new(PortTestPort::new(true)),
            ];
            Plugged::Pads
        }
        _ => match subor_mouse(device) {
            Some((port, mouse)) => {
                board.expansion = Some(Box::new(SuborKeyboard::default()));
                match mouse {
                    None => Plugged::Expansion,
                    Some(mouse) => {
                        board.pads[port] = mouse;
                        Plugged::SuborPair(port)
                    }
                }
            }
            None => Plugged::None,
        },
    }
}
