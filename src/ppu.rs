//! The RP2C02 picture processor, dot by dot, every fetch on its bus (`ppu.py`;
//! its module notes give the schedule and the sources).
//!
//! Differences forced by Rust: the reference schedules register effects as
//! closures; here they are [`Event`]s, kept in the same order (a stable sort
//! by due time). The PPU owns the cartridge, since every one of its bus
//! accesses goes through the connector; the board reaches it as `ppu.cart`.
//! The set of leaking lanes is a bit mask.

use crate::cartridge::Cartridge;
use crate::machine::Timing;

/// Dots after the access before a register write's effect (`ppu.DELAYS`).
pub const DELAY_2006_COPY: i64 = 2;
pub const DELAY_2007_WRITE: i64 = 3;
pub const DELAY_2007_INCREMENT: i64 = 4;
pub const DELAY_2007_READ_ALE: i64 = 2;
pub const DELAY_2003_DELAYED: i64 = 3;
pub const DELAY_2004_REFRESH: i64 = 3;

pub const DOTS: i32 = 341;
pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;
pub const BORDER: u16 = 0x0F;
const COPY_WRAPPED: u8 = 4;
pub const OAMADDR_LEAD_TICKS: i64 = 8;
pub const VBL: u8 = 0x80;
pub const SPRITE0: u8 = 0x40;
pub const OVERFLOW: u8 = 0x20;
const BEHIND: u8 = 0x20;
const IS_SPRITE0: u8 = 0x40;

/// A byte's bits mirrored.
pub const REVERSE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = (i as u8).reverse_bits();
        i += 1;
    }
    table
};

/// A register effect that lands some dots after the access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    WakeUp,
    TurnOff,
    LatchMux(u8, u8),
    SetOverflow,
    Read2007Ale,
    Complete2007Read,
    CopyT,
    Write2007(u8),
    Increment2007,
    SetOamAddr(u8),
    WriteOamRefreshing(u8),
}

/// A sprite FIFO lane: pattern low, pattern high, attribute, X counter, and
/// the time the lane starts drawing.
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Lane {
    pub lo: u8,
    pub hi: u8,
    pub attr: u8,
    pub x: u8,
    pub zero: i64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ppu {
    #[serde(skip, default = "crate::state::live_timing")]
    pub timing: Timing,
    #[serde(skip, default = "crate::state::empty_cart")]
    pub cart: Box<dyn Cartridge>,
    pub prerender_line: i32,
    pub dot_ticks: i64,
    #[serde(with = "serde_big_array::BigArray")]
    pub ciram: [u8; 0x800],
    pub palette: [u8; 0x20],
    #[serde(with = "serde_big_array::BigArray")]
    pub oam: [u8; 0x100],
    pub oam2: [u8; 32],
    /// The picture: 240 rows of 256 9-bit values.
    pub picture: Vec<u16>,
    /// The next dot runs at master tick `time`.
    pub time: i64,
    pub line: i32,
    pub dot: i32,
    nt_340_time: i64,
    pub frame: u64,
    watch: bool,
    pub latch_decay_ticks: i64,
    latch_times: [i64; 8],
    events: Vec<(i64, Event)>,
    read_in_flight_until: i64,
    oam_corrupt: Option<usize>,
    refill_from_fetch: bool,
    octal: Option<u16>,
    bus_data: u8,
    last_pixel: i64,
    last_value: u8,
    last_pixel_time: i64,
    prev_pixel: i64,
    prev_value: u8,
    prev_pixel_time: i64,
    // power()
    pub status: u8,
    pub oam_addr: u8,
    pub v: u16,
    pub nt_latch: u8,
    pub at_byte: u8,
    pub at_latch: u8,
    pub lo_latch: u8,
    pub hi_latch: u8,
    pub bg_lo: u16,
    pub bg_hi: u16,
    pub at_lo: u16,
    pub at_hi: u16,
    pub oam_bus: u8,
    oam2_index: usize,
    oam2_fetch: usize,
    fetch_y: u8,
    copying: u8,
    eval_hold: i32,
    step_from: u8,
    eval_done: bool,
    sprite_count: usize,
    sprite0_next: bool,
    sprite0_line: bool,
    fetch_tile: u8,
    fetch_attr: u8,
    garbage: u16,
    oam2_frozen: bool,
    lanes: [Lane; 8],
    plan: [usize; 8],
    zh: bool,
    leaking: u8,
    #[serde(with = "serde_big_array::BigArray")]
    pub sprite_line: [u8; WIDTH],
    vbl_suppress: bool,
    skip_sample: u8,
    // reset()
    pub ctrl: u8,
    pub mask: u8,
    mux: u8,
    on: bool,
    target: bool,
    waking: bool,
    dark: bool,
    fading: u8,
    reload_pending: bool,
    pub t: u16,
    pub x: u8,
    pub w: u8,
    pub buffer: u8,
    pub latch: u8,
    pub resetting: bool,
    pub nmi: bool,
}

impl Ppu {
    pub fn new(timing: Timing, cart: Box<dyn Cartridge>, latch_decay_seconds: f64) -> Ppu {
        let watch = cart.watches_ppu_address();
        let mut ppu = Ppu {
            prerender_line: timing.lines - 1,
            dot_ticks: timing.dot_divider,
            latch_decay_ticks: (latch_decay_seconds * timing.master_hz) as i64,
            timing,
            cart,
            ciram: [0; 0x800],
            palette: [0; 0x20],
            oam: [0; 0x100],
            oam2: [0xFF; 32],
            picture: vec![0; WIDTH * HEIGHT],
            time: 0,
            line: 0,
            dot: 0,
            nt_340_time: -1,
            frame: 0,
            watch,
            latch_times: [0; 8],
            events: Vec::new(),
            read_in_flight_until: -1,
            oam_corrupt: None,
            refill_from_fetch: false,
            octal: None,
            bus_data: 0,
            last_pixel: -1,
            last_value: 0,
            last_pixel_time: -1,
            prev_pixel: -1,
            prev_value: 0,
            prev_pixel_time: -1,
            status: 0,
            oam_addr: 0,
            v: 0,
            nt_latch: 0,
            at_byte: 0,
            at_latch: 0,
            lo_latch: 0,
            hi_latch: 0,
            bg_lo: 0,
            bg_hi: 0,
            at_lo: 0,
            at_hi: 0,
            oam_bus: 0xFF,
            oam2_index: 0,
            oam2_fetch: 0,
            fetch_y: 0,
            copying: 0,
            eval_hold: -1,
            step_from: 0,
            eval_done: false,
            sprite_count: 0,
            sprite0_next: false,
            sprite0_line: false,
            fetch_tile: 0xFF,
            fetch_attr: 0xFF,
            garbage: 0x2000,
            oam2_frozen: false,
            lanes: [Lane::default(); 8],
            plan: [WIDTH; 8],
            zh: false,
            leaking: 0,
            sprite_line: [0; WIDTH],
            vbl_suppress: false,
            skip_sample: 0,
            ctrl: 0,
            mask: 0,
            mux: 0,
            on: false,
            target: false,
            waking: false,
            dark: false,
            fading: 0,
            reload_pending: false,
            t: 0,
            x: 0,
            w: 0,
            buffer: 0,
            latch: 0,
            resetting: true,
            nmi: false,
        };
        ppu.power();
        ppu
    }

    // -- power and reset ---------------------------------------------------

    pub fn power(&mut self) {
        self.status = OVERFLOW;
        self.oam_addr = 0;
        self.v = 0;
        self.nt_latch = 0;
        self.at_byte = 0;
        self.at_latch = 0;
        self.lo_latch = 0;
        self.hi_latch = 0;
        self.bg_lo = 0;
        self.bg_hi = 0;
        self.at_lo = 0;
        self.at_hi = 0;
        self.oam_bus = 0xFF;
        self.oam2_index = 0;
        self.oam2_fetch = 0;
        self.fetch_y = 0;
        self.copying = 0;
        self.eval_hold = -1;
        self.step_from = 0;
        self.eval_done = false;
        self.sprite_count = 0;
        self.sprite0_next = false;
        self.sprite0_line = false;
        self.fetch_tile = 0xFF;
        self.fetch_attr = 0xFF;
        self.garbage = 0x2000;
        self.oam2_frozen = false;
        self.lanes = [Lane::default(); 8];
        self.plan = [WIDTH; 8];
        self.zh = false;
        self.leaking = 0;
        self.sprite_line = [0; WIDTH];
        self.vbl_suppress = false;
        self.skip_sample = 0;
        self.reset();
    }

    /// The reset line: PPUCTRL, PPUMASK, t, x, w and the buffer clear; v does not.
    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.mux = 0;
        self.on = false;
        self.target = false;
        self.waking = false;
        self.dark = false;
        self.fading = 0;
        self.reload_pending = false;
        self.t = 0;
        self.x = 0;
        self.w = 0;
        self.buffer = 0;
        self.latch = 0;
        self.resetting = true;
        self.nmi = false;
    }

    // -- the clock ---------------------------------------------------------

    /// Run every dot that begins before master tick `time`.
    pub fn run_to(&mut self, time: i64) {
        let ticks = self.dot_ticks;
        while self.time < time {
            let dot = self.dot;
            let line = self.line;
            if (1..DOTS - 4).contains(&dot)
                && (!(self.on || self.mask & 0x18 != 0 || self.fading != 0)
                    || (self.timing.vblank_line - 1 <= line && line < self.prerender_line))
            {
                let mut dots =
                    ((time - self.time + ticks - 1).div_euclid(ticks)).min((DOTS - 4 - dot) as i64);
                if let Some(&(due, _)) = self.events.first() {
                    dots = dots.min((due - self.time).div_euclid(ticks) - 1);
                    if dots <= 0 {
                        self.step();
                        continue;
                    }
                }
                let dots32 = dots as i32;
                if (line as usize) < HEIGHT {
                    self.fill_backdrop(line, dot + 1, dot + dots32);
                } else if let Some(refresh) = self.timing.oam_refresh {
                    if refresh.0 <= line && line <= refresh.1 {
                        let steps = (dot + dots32) / 2 - dot / 2;
                        self.oam_addr = self.oam_addr.wrapping_add(steps as u8);
                    }
                }
                self.dot = dot + dots32;
                self.time += dots * ticks;
                continue;
            }
            self.step();
        }
    }

    fn backdrop_index(&self) -> usize {
        let v = self.v & 0x3FFF;
        if v >= 0x3F00 {
            palette_index(v)
        } else {
            0
        }
    }

    /// The pixels of dots `first`-`last` with rendering off.
    fn fill_backdrop(&mut self, line: i32, first: i32, last: i32) {
        let first = first.max(2);
        let last = last.min(WIDTH as i32 + 1);
        if first > last {
            return;
        }
        let raw = self.palette[self.backdrop_index()];
        let mut value = raw as u16;
        if self.mask & 0x01 != 0 {
            value &= 0x30;
        }
        value |= ((self.mask & 0xE0) as u16) << 1;
        let base = line as i64 * WIDTH as i64 - 2;
        for index in base + first as i64..=base + last as i64 {
            self.picture[index as usize] = value;
        }
        let last_time = self.time + (last - self.dot) as i64 * self.dot_ticks;
        if last > first {
            self.prev_pixel = base + last as i64 - 1;
            self.prev_value = raw;
            self.prev_pixel_time = last_time - self.dot_ticks;
        } else {
            self.prev_pixel = self.last_pixel;
            self.prev_value = self.last_value;
            self.prev_pixel_time = self.last_pixel_time;
        }
        self.last_pixel = base + last as i64;
        self.last_value = raw;
        self.last_pixel_time = last_time;
    }

    /// Run `event` before the work of the `dots`-th dot after the next.
    fn schedule(&mut self, dots: i64, event: Event) {
        let due = self.time + (dots + 1) * self.dot_ticks;
        // Appended then stably sorted by due time, as the reference does.
        let at = self.events.partition_point(|&(d, _)| d <= due);
        self.events.insert(at, (due, event));
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::WakeUp => self.wake_up(),
            Event::TurnOff => self.turn_off(),
            Event::LatchMux(bits, value) => self.latch_mux(bits, value),
            Event::SetOverflow => self.status |= OVERFLOW,
            Event::Read2007Ale => self.read_2007_ale(),
            Event::Complete2007Read => self.complete_2007_read(),
            Event::CopyT => self.copy_t(),
            Event::Write2007(value) => {
                let v = self.v & 0x3FFF;
                self.write_2007(v, value);
            }
            Event::Increment2007 => self.increment_after_2007(),
            Event::SetOamAddr(value) => self.oam_addr = value,
            Event::WriteOamRefreshing(value) => self.write_oam_refreshing(value),
        }
    }

    /// Advance one dot and do its work.
    fn step(&mut self) {
        self.time += self.dot_ticks;
        while let Some(&(due, event)) = self.events.first() {
            if due > self.time {
                break;
            }
            self.events.remove(0);
            self.apply(event);
        }
        let mut dot = self.dot + 1;
        let mut line = self.line;
        let rendering = self.on;
        // Fork: the common dot, a visible line's pixel dot (2-256) with
        // rendering on and settled. dot_work would take the same path:
        // dot is neither 1 nor DOTS - 3, nor the line's end; no wake, fade,
        // OAM corruption, darkness or OAM refresh (`render`'s dot >= 2 arm).
        if (2..=WIDTH as i32).contains(&dot)
            && (line as usize) < HEIGHT
            && rendering
            && !self.waking
            && self.fading == 0
            && !self.dark
            && self.oam_corrupt.is_none()
            && self.timing.oam_refresh.is_none()
        {
            self.dot = dot;
            if self.mask & 0x18 != 0 {
                self.evaluate(line, dot);
            }
            self.pixel(line, (dot - 2) as usize);
            self.shift();
            if dot == 63 || dot == 255 {
                self.oam2_frozen = false;
            }
            self.background_fetch(dot);
            if dot == WIDTH as i32 {
                self.increment_y();
            }
            return;
        }
        if dot == DOTS {
            line = self.next_line(line);
            self.dot = 0;
            self.line_start(line, false);
            return self.dot_work(line, 0, rendering);
        } else if dot == DOTS - 1
            && line == self.prerender_line
            && self.frame & 1 != 0
            && self.skip_sample != 0
            && self.timing.odd_frame_skip
        {
            self.render(line, dot);
            dot = 0;
            line = self.next_line(line);
            self.dot = dot;
            self.line_start(line, true);
            return;
        }
        self.dot = dot;
        self.dot_work(line, dot, rendering);
    }

    /// The rest of `_step` once the dot and line are known.
    fn dot_work(&mut self, line: i32, dot: i32, rendering: bool) {
        if dot == DOTS - 3 {
            self.skip_sample = self.mask & 0x18;
        }
        if let Some(refresh) = self.timing.oam_refresh {
            if dot & 1 == 0 && dot != 0 && refresh.0 <= line && line <= refresh.1 {
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
        }
        if dot == 1 {
            if line == self.timing.vblank_line {
                if !self.vbl_suppress {
                    self.status |= VBL;
                    self.nmi = self.ctrl & 0x80 != 0;
                }
                self.vbl_suppress = false;
            } else if line == self.prerender_line {
                self.status &= !(VBL | SPRITE0 | OVERFLOW);
                self.nmi = false;
                self.resetting = false;
            }
        }
        let visible = (line as usize) < HEIGHT;
        if visible || line == self.prerender_line {
            if visible && (1..=WIDTH as i32).contains(&dot) && self.mask & 0x18 != 0 {
                self.evaluate(line, dot);
            }
            if self.waking {
                self.waking = false;
                self.wake(line, dot);
            } else if self.fading != 0 {
                self.fading -= 1;
                if self.fading != 0 {
                    self.fade(line, dot);
                } else {
                    self.after_fade(line, dot);
                }
            } else if rendering {
                if self.oam_corrupt.is_some() && self.mask & 0x18 != 0 {
                    self.corrupt_oam();
                }
                self.render(line, dot);
                if self.dark {
                    self.dark = false;
                }
            } else if visible && (2..=WIDTH as i32 + 1).contains(&dot) {
                self.fill_backdrop(line, dot, dot);
            }
        } else {
            self.waking = false;
            self.fading = 0;
        }
    }

    fn next_line(&mut self, line: i32) -> i32 {
        if (line as usize) < HEIGHT && self.timing.border {
            self.blank_border(line);
        }
        let mut line = line + 1;
        if line == self.timing.lines {
            line = 0;
            self.frame += 1;
        }
        self.line = line;
        line
    }

    fn blank_border(&mut self, line: i32) {
        let base = line as usize * WIDTH;
        let picture = &mut self.picture;
        if line == 0 {
            picture[base..base + WIDTH].fill(BORDER);
        } else {
            picture[base] = BORDER;
            picture[base + 1] = BORDER;
            picture[base + WIDTH - 2] = BORDER;
            picture[base + WIDTH - 1] = BORDER;
        }
    }

    fn line_start(&mut self, line: i32, skipped: bool) {
        self.leaking = 0;
        if self.zh {
            self.zh = false;
            let now = self.time;
            let ticks = self.dot_ticks;
            for n in 0..8 {
                let lane = &mut self.lanes[n];
                if lane.zero <= now && lane.x != 0 {
                    lane.zero = now + lane.x as i64 * ticks;
                    lane.x = 0;
                    if skipped {
                        self.leaking |= 1 << n;
                    }
                }
            }
        }
        if (line as usize) < HEIGHT && self.on {
            self.plan_sprites(0);
        }
    }

    // -- rendering ---------------------------------------------------------

    fn render(&mut self, line: i32, dot: i32) {
        let visible = (line as usize) < HEIGHT;
        if dot <= WIDTH as i32 + 1 {
            if dot == 0 {
                if self.watch && self.time - self.nt_340_time == self.dot_ticks {
                    let address = self.bg_pattern(self.v);
                    self.ale(address);
                }
                return;
            }
            if dot >= 2 {
                if visible {
                    if self.dark {
                        self.fill_backdrop(line, dot, dot);
                    } else {
                        self.pixel(line, (dot - 2) as usize);
                    }
                }
                if !self.dark {
                    self.shift();
                } else if self.reload_pending {
                    self.reload_pending = false;
                    self.reload();
                }
            }
            if dot == 63 || dot == 255 {
                self.oam2_frozen = false;
            }
            if dot <= WIDTH as i32 {
                self.background_fetch(dot);
                if dot == WIDTH as i32 {
                    self.increment_y();
                }
            } else {
                if !self.dark {
                    self.reload();
                }
                self.oam_addr = 0;
                if visible {
                    self.lanes_shifted(WIDTH);
                }
                self.sprite0_line = self.sprite0_next;
                self.sprite_fetch(line, dot, visible);
                self.v = (self.v & !0x041F) | (self.t & 0x041F);
            }
        } else if dot <= 320 {
            self.oam_addr = 0;
            if line == self.prerender_line && (280..=304).contains(&dot) {
                self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
            }
            self.sprite_fetch(line, dot, visible);
        } else if dot <= 336 {
            if dot >= 322 {
                if !self.dark {
                    self.shift();
                } else if self.reload_pending {
                    self.reload_pending = false;
                    self.reload();
                }
            }
            self.background_fetch(dot);
            if dot == 321 && !self.oam2_frozen {
                self.oam2_fetch += 1;
                if self.oam2_fetch >= 32 {
                    self.oam2_fetch = 0;
                    self.oam2_frozen = true;
                }
            }
            self.oam_bus = self.oam2[self.oam2_fetch & 31];
        } else {
            if dot == 337 && !self.dark {
                self.shift();
                self.reload();
            } else if dot == 339 {
                self.zh = true;
                self.oam2_frozen = false;
            }
            let address = 0x2000 | (self.v & 0x0FFF);
            if dot & 1 == 0 {
                self.nt_latch = self.fetch(address);
                if dot == 340 {
                    self.nt_340_time = self.time;
                }
            } else if self.watch {
                self.ale(address);
            }
        }
    }

    fn shift(&mut self) {
        self.bg_lo <<= 1;
        self.bg_hi = (self.bg_hi << 1) | 1;
        let at = self.at_latch as u16;
        self.at_lo = (self.at_lo << 1) | (at & 1);
        self.at_hi = (self.at_hi << 1) | (at >> 1);
    }

    fn reload(&mut self) {
        self.bg_lo = (self.bg_lo & 0xFF00) | self.lo_latch as u16;
        self.bg_hi = (self.bg_hi & 0xFF00) | self.hi_latch as u16;
        let at = self.at_latch;
        self.at_lo = (self.at_lo & 0xFF00) | if at & 1 != 0 { 0xFF } else { 0 };
        self.at_hi = (self.at_hi & 0xFF00) | if at & 2 != 0 { 0xFF } else { 0 };
    }

    fn attribute_address(v: u16) -> u16 {
        0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)
    }

    fn background_fetch(&mut self, dot: i32) {
        let phase = (dot - 1) & 7;
        let v = self.v;
        match phase {
            0 => {
                if dot >= 9 && !self.dark {
                    self.reload();
                }
                if self.watch {
                    self.ale(0x2000 | (v & 0x0FFF));
                }
            }
            1 => self.nt_latch = self.fetch(0x2000 | (v & 0x0FFF)),
            2 => {
                if self.watch {
                    self.ale(Self::attribute_address(v));
                }
            }
            3 => self.at_byte = self.fetch(Self::attribute_address(v)),
            4 => {
                if self.watch {
                    let address = self.bg_pattern(v);
                    self.ale(address);
                }
            }
            5 => {
                let address = self.bg_pattern(v);
                self.lo_latch = self.fetch(address);
            }
            6 => {
                if self.watch {
                    let address = self.bg_pattern(v) | 8;
                    self.ale(address);
                }
            }
            _ => {
                let address = self.bg_pattern(v) | 8;
                self.hi_latch = self.fetch(address);
                self.at_latch = (self.at_byte >> (((v >> 4) & 4) | (v & 2))) & 3;
                if v & 0x1F == 31 {
                    self.v = (v & !0x1F) ^ 0x0400;
                } else {
                    self.v = v + 1;
                }
            }
        }
    }

    fn bg_pattern(&self, v: u16) -> u16 {
        (((self.ctrl & 0x10) as u16) << 8) | ((self.nt_latch as u16) << 4) | (v >> 12)
    }

    fn increment_y(&mut self) {
        let mut v = self.v;
        if v & 0x7000 != 0x7000 {
            self.v = v + 0x1000;
            return;
        }
        v &= !0x7000;
        let mut y = (v >> 5) & 31;
        if y == 29 {
            y = 0;
            v ^= 0x0800;
        } else if y == 31 {
            y = 0;
        } else {
            y += 1;
        }
        self.v = (v & !0x03E0) | (y << 5);
    }

    fn pixel(&mut self, line: i32, x: usize) {
        let mux = self.mux;
        let mut bg: u8 = 0;
        if mux & 0x08 != 0 && (x >= 8 || mux & 0x02 != 0) {
            let shift = 15 - self.x as u16;
            bg = ((((self.bg_hi >> shift) & 1) << 1) | ((self.bg_lo >> shift) & 1)) as u8;
            if bg != 0 {
                bg |=
                    ((((self.at_hi >> shift) & 1) << 3) | (((self.at_lo >> shift) & 1) << 2)) as u8;
            }
        }
        let sprite = self.sprite_line[x];
        let index = if sprite != 0 && mux & 0x10 != 0 && (x >= 8 || mux & 0x04 != 0) {
            if bg != 0 && sprite & IS_SPRITE0 != 0 && x != 255 {
                self.status |= SPRITE0;
            }
            if bg != 0 && sprite & BEHIND != 0 {
                bg
            } else {
                sprite & 0x1F
            }
        } else {
            bg
        };
        let value = self.palette[index as usize];
        self.set_pixel(line, x, value);
    }

    /// Record pixel `x` of `line` as computed now, and draw it.
    fn set_pixel(&mut self, line: i32, x: usize, value: u8) {
        self.prev_pixel = self.last_pixel;
        self.prev_value = self.last_value;
        self.prev_pixel_time = self.last_pixel_time;
        let index = line as usize * WIDTH + x;
        self.last_pixel = index as i64;
        self.last_value = value;
        self.last_pixel_time = self.time;
        let mask = self.mask;
        let mut value = value as u16;
        if mask & 0x01 != 0 {
            value &= 0x30;
        }
        self.picture[index] = value | (((mask & 0xE0) as u16) << 1);
    }

    // -- sprites -----------------------------------------------------------

    fn in_range(line: i32, value: u8, height: i32) -> bool {
        let d = line - value as i32;
        0 <= d && d < height
    }

    fn evaluate(&mut self, line: i32, dot: i32) {
        if dot <= 64 {
            self.oam_bus = 0xFF;
            if dot & 1 == 0 {
                self.oam2[((dot >> 1) - 1) as usize] = 0xFF;
            }
            return;
        }
        if dot & 1 != 0 {
            if dot == 65 {
                self.oam2_index = 0;
                self.copying = 0;
                self.eval_done = false;
                self.sprite0_next = false;
                self.eval_hold = -1;
            }
            if self.eval_hold >= 0 {
                self.oam_addr = self.eval_hold as u8;
                self.eval_hold = -1;
            }
            self.oam_bus = self.oam[self.oam_addr as usize];
            return;
        }
        let value = self.oam_bus;
        let mut address = self.oam_addr;
        self.step_from = address;
        let index = self.oam2_index;
        let height = if self.ctrl & 0x20 != 0 { 16 } else { 8 };
        if self.eval_done {
            self.oam_bus = self.oam2[index & 31];
            self.oam_addr = address.wrapping_add(4) & 0xFC;
            return;
        }
        if index < 32 {
            self.oam2[index] = value;
            if self.copying != 0 {
                self.copying -= 1;
                self.oam2_index = index + 1;
                if index == 31 {
                    self.oam2_frozen = true;
                }
                if address == 0xFF {
                    self.copying |= COPY_WRAPPED;
                }
                address = address.wrapping_add(1);
                if self.copying & 3 == 0 {
                    if !Self::in_range(line, value, height) {
                        address &= 0xFC;
                    }
                    if self.copying & COPY_WRAPPED != 0 {
                        self.eval_done = true;
                    }
                    self.copying = 0;
                }
            } else if Self::in_range(line, value, height) {
                if dot == 66 {
                    self.sprite0_next = true;
                }
                self.copying = 3;
                self.oam2_index = index + 1;
                address = address.wrapping_add(1);
            } else {
                address = address.wrapping_add(4) & 0xFC;
                if address < 4 {
                    self.eval_done = true;
                }
            }
            self.oam_addr = address;
            return;
        }
        self.oam_bus = self.oam2[0];
        if self.copying != 0 {
            self.copying -= 1;
            address = address.wrapping_add(1);
            if self.copying == 0 {
                if !Self::in_range(line, value, height) {
                    address &= 0xFC;
                }
                self.eval_done = true;
            }
        } else if Self::in_range(line, value, height) {
            self.schedule(0, Event::SetOverflow);
            self.copying = 3;
            address = address.wrapping_add(1);
        } else {
            let n = ((address >> 2) + 1) & 0x3F;
            address = (n << 2) | (address.wrapping_add(1) & 3);
            if n == 0 {
                self.eval_done = true;
            }
        }
        self.oam_addr = address;
    }

    fn last_eval_step(&mut self) {
        if self.dot & 1 == 0 {
            self.oam_addr = self.step_from.wrapping_add(1);
        }
        self.oam2_index = (self.oam2_index + (self.copying & 3) as usize).min(32);
        self.copying = 0;
    }

    fn sprite_fetch(&mut self, line: i32, dot: i32, visible: bool) {
        let line = if visible { line } else { line & 0xFF };
        let slot = ((dot - 257) >> 3) as usize;
        let phase = (dot - 257) & 7;
        if dot == 257 {
            self.oam2_fetch = 0;
        } else if phase <= 3 && !self.oam2_frozen {
            self.oam2_fetch += 1;
        }
        let address = self.oam2_fetch & 31;
        match phase {
            0 => {
                if slot == 0 {
                    self.sprite_count = if visible {
                        self.oam2_index.min(32) >> 2
                    } else {
                        8
                    };
                }
                self.fetch_y = self.oam2[address];
                self.oam_bus = self.fetch_y;
                self.garbage = 0x2000 | (self.v & 0x0FFF);
                if self.watch {
                    let g = self.garbage;
                    self.ale(g);
                }
            }
            1 => {
                self.fetch_tile = self.oam2[address];
                self.oam_bus = self.fetch_tile;
                let a = (0x2000 | (self.v & 0x0F00)) | (self.garbage & 0xFF);
                self.nt_latch = self.fetch(a);
            }
            2 => {
                let attr = self.oam2[address];
                self.fetch_attr = attr;
                self.oam_bus = attr;
                self.lanes[slot].attr = attr;
                if self.watch {
                    self.ale(0x2000 | (self.v & 0x0FFF));
                }
            }
            3 => {
                let x = self.oam2[address];
                self.oam_bus = x;
                self.fetch(0x2000 | (self.v & 0x0FFF));
                let now = self.time;
                let ticks = self.dot_ticks;
                let lane = &mut self.lanes[slot];
                lane.x = x;
                if lane.zero > now {
                    lane.zero = now + x as i64 * ticks;
                }
            }
            4 => {
                if self.watch {
                    let a = self.sprite_pattern(line, self.fetch_y);
                    self.ale(a);
                }
            }
            5 => {
                let a = self.sprite_pattern(line, self.fetch_y);
                let lo = self.fetch(a);
                self.lanes[slot].lo = self.lane_bits(slot, visible, line, self.fetch_y, lo);
            }
            6 => {
                if self.watch {
                    let a = self.sprite_pattern(line, self.fetch_y) | 8;
                    self.ale(a);
                }
            }
            _ => {
                let a = self.sprite_pattern(line, self.fetch_y) | 8;
                let hi = self.fetch(a);
                self.lanes[slot].hi = self.lane_bits(slot, visible, line, self.fetch_y, hi);
            }
        }
        if phase >= 3 {
            self.oam_bus = self.oam2[address];
        }
    }

    fn lane_bits(&self, slot: usize, visible: bool, line: i32, y: u8, bits: u8) -> u8 {
        let height = if self.ctrl & 0x20 != 0 { 16 } else { 8 };
        if slot >= self.sprite_count || !(visible || Self::in_range(line, y, height)) {
            return 0;
        }
        if self.fetch_attr & 0x40 != 0 {
            REVERSE[bits as usize]
        } else {
            bits
        }
    }

    fn sprite_pattern(&self, line: i32, y: u8) -> u16 {
        let tile = self.fetch_tile as u16;
        let mut row = ((line - y as i32) & 0xFF) as u16;
        if self.ctrl & 0x20 != 0 {
            if self.fetch_attr & 0x80 != 0 {
                row = 15 - (row & 15);
            }
            return ((tile & 1) << 12) | ((tile & 0xFE) << 4) | ((row & 8) << 1) | (row & 7);
        }
        if self.fetch_attr & 0x80 != 0 {
            row = 7 - (row & 7);
        }
        (((self.ctrl & 0x08) as u16) << 9) | (tile << 4) | (row & 7)
    }

    #[allow(clippy::needless_range_loop)] // n indexes the lanes, the plan and the leak mask
    fn plan_sprites(&mut self, x1: usize) {
        self.sprite_line = [0; WIDTH];
        let now = self.time;
        let ticks = self.dot_ticks;
        let sprite0 = self.sprite0_line;
        let mut plan = [0usize; 8];
        for n in 0..8 {
            let Lane {
                lo, hi, attr, zero, ..
            } = self.lanes[n];
            let start = if zero <= now {
                x1
            } else {
                x1 + ((zero - now) / ticks) as usize
            };
            plan[n] = start;
            if start >= WIDTH || (lo | hi) == 0 {
                continue;
            }
            let mut flags = 0x10 | ((attr & 3) << 2) | if attr & 0x20 != 0 { BEHIND } else { 0 };
            if n == 0 && sprite0 {
                flags |= IS_SPRITE0;
            }
            let mut x = start;
            for bit in (0..8).rev() {
                if x >= WIDTH {
                    break;
                }
                let pixel = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
                let at = if bit == 7 && self.leaking & (1 << n) != 0 {
                    x1
                } else {
                    x
                };
                if pixel != 0 && self.sprite_line[at] == 0 {
                    self.sprite_line[at] = flags | pixel;
                }
                x += 1;
            }
        }
        self.plan = plan;
    }

    fn lanes_shifted(&mut self, x0: usize) {
        for (lane, &start) in self.lanes.iter_mut().zip(self.plan.iter()) {
            if start < x0 {
                let n = (x0 - start).min(8) as u32;
                lane.lo = ((lane.lo as u16) << n) as u8;
                lane.hi = ((lane.hi as u16) << n) as u8;
            }
        }
        self.plan = [WIDTH; 8];
    }

    // -- the PPU bus -------------------------------------------------------

    fn ale(&mut self, address: u16) {
        let time = self.time;
        self.cart.ppu_address(address, time);
    }

    /// A rendering fetch (/RD), at this dot's time.
    fn fetch(&mut self, address: u16) -> u8 {
        let mut address = address;
        if let Some(octal) = self.octal.take() {
            address = (address & 0x3F00) | octal;
        }
        if address >= 0x2000 {
            if let Some(a10) = self.cart.ciram_a10(address) {
                let value = self.ciram[((a10 as usize) << 10) | (address as usize & 0x3FF)];
                self.bus_data = value;
                if self.refill_from_fetch {
                    self.refill_from_fetch = false;
                    self.buffer = value;
                }
                return value;
            }
        }
        let time = self.time;
        let value = self
            .cart
            .ppu_read(address, time)
            .unwrap_or((address & 0xFF) as u8);
        self.bus_data = value;
        if self.refill_from_fetch {
            self.refill_from_fetch = false;
            self.buffer = value;
        }
        value
    }

    fn fetch_address(&self, dot: i32) -> u16 {
        let v = self.v;
        if dot <= WIDTH as i32 || (321..=336).contains(&dot) {
            let phase = (dot - 1) & 7;
            if phase == 1 {
                return 0x2000 | (v & 0x0FFF);
            }
            if phase == 3 {
                return Self::attribute_address(v);
            }
            return self.bg_pattern(v) | if phase == 7 { 8 } else { 0 };
        }
        if dot <= 320 {
            let phase = (dot - 257) & 7;
            if phase == 1 {
                return self.garbage;
            }
            if phase == 3 {
                return 0x2000 | (v & 0x0FFF);
            }
            let line = if (self.line as usize) < HEIGHT {
                self.line
            } else {
                self.line & 0xFF
            };
            return self.sprite_pattern(line, self.fetch_y) | if phase == 7 { 8 } else { 0 };
        }
        0x2000 | (v & 0x0FFF)
    }

    fn ale_address(&self, dot: i32) -> u16 {
        if dot > 340 {
            return self.bg_pattern(self.v);
        }
        if (257..=320).contains(&dot) && matches!((dot - 257) & 7, 0 | 2) {
            return 0x2000 | (self.v & 0x0FFF);
        }
        self.fetch_address(dot + 1)
    }

    fn settle(&mut self, high: u16, low: u16, steps: u32) -> u16 {
        let mut low = low;
        for _ in 0..steps {
            low = self.bus_read(high | low) as u16;
        }
        low
    }

    pub fn bus_read(&mut self, address: u16) -> u8 {
        let address = address & 0x3FFF;
        if address >= 0x2000 {
            if let Some(a10) = self.cart.ciram_a10(address) {
                return self.ciram[((a10 as usize) << 10) | (address as usize & 0x3FF)];
            }
        }
        let time = self.time;
        self.cart
            .ppu_read(address, time)
            .unwrap_or((address & 0xFF) as u8)
    }

    pub fn bus_write(&mut self, address: u16, value: u8) {
        let address = address & 0x3FFF;
        if address >= 0x2000 {
            if let Some(a10) = self.cart.ciram_a10(address) {
                self.ciram[((a10 as usize) << 10) | (address as usize & 0x3FF)] = value;
                return;
            }
        }
        let time = self.time;
        self.cart.ppu_write(address, value, time);
    }

    fn oam2_row(&self) -> usize {
        let dot = self.dot;
        if (1..=64).contains(&dot) {
            return ((dot - 1) >> 1) as usize;
        }
        if (65..=256).contains(&dot) {
            let index = self.oam2_index;
            return if index >= 32 {
                0
            } else {
                ((index + 3) & !3) & 31
            };
        }
        self.oam2_fetch & 31
    }

    /// Master ticks before the access point at which a write to `register`
    /// takes effect: $2003's load, on NTSC.
    pub fn write_lead(&self, register: u16) -> i64 {
        if register == 3 && !self.timing.delayed_oamaddr {
            OAMADDR_LEAD_TICKS
        } else {
            0
        }
    }

    fn write_oam_addr(&mut self, value: u8) {
        self.oam_addr = value;
        let dot = self.dot;
        if dot & 1 != 0
            && (65..WIDTH as i32).contains(&dot)
            && (self.line as usize) < HEIGHT
            && self.mask & 0x18 != 0
        {
            self.eval_hold = value as i32;
        }
    }

    fn write_oam_refreshing(&mut self, value: u8) {
        let address = self.oam_addr;
        self.write_oam(value);
        if (self.dot + 1) & 1 == 0 {
            self.oam_addr = address;
        }
    }

    fn write_oam(&mut self, value: u8) {
        let address = self.oam_addr;
        self.oam[address as usize] = if address & 3 == 2 {
            value & 0xE3
        } else {
            value
        };
        self.oam_addr = address.wrapping_add(1);
    }

    fn corrupt_oam(&mut self) {
        let row = self.oam_corrupt.take().expect("a row to corrupt");
        let source = (self.oam_addr & 0xF8) as usize;
        let copy: [u8; 8] = self.oam[source..source + 8].try_into().unwrap();
        self.oam[row * 8..row * 8 + 8].copy_from_slice(&copy);
        self.oam2[row] = self.oam2[0];
    }

    fn wake(&mut self, line: i32, dot: i32) {
        let visible = (line as usize) < HEIGHT;
        if visible && (2..=WIDTH as i32 + 1).contains(&dot) {
            self.fill_backdrop(line, dot, dot);
        }
        if dot & 1 != 0 && dot <= 339 {
            let address;
            if dot <= WIDTH as i32 || dot >= 321 {
                let phase = (dot - 1) & 7;
                if phase <= 2 || dot >= 337 {
                    address = 0x2000 | (self.v & 0x0FFF);
                } else {
                    address = self.fetch_address(dot + 1);
                }
                if phase == 2 {
                    self.octal = Some(address & 0xFF);
                }
            } else {
                address = self.fetch_address(dot + 1);
            }
            if self.watch {
                self.ale(address);
            }
        }
        if visible && dot < WIDTH as i32 {
            self.plan_sprites(dot as usize);
        }
        self.dark = true;
    }

    fn latch_mux(&mut self, bits: u8, value: u8) {
        self.mux = (self.mux & !bits) | (value & bits);
    }

    /// PPUMASK as if written long ago: rendering state follows at once.
    pub fn set_mask(&mut self, value: u8) {
        self.mask = value;
        self.mux = value;
        self.on = value & 0x18 != 0;
        self.target = self.on;
        self.waking = false;
        self.dark = false;
        self.fading = 0;
    }

    fn wake_up(&mut self) {
        self.on = true;
        self.waking = true;
        self.fading = 0;
    }

    fn fade(&mut self, line: i32, dot: i32) {
        if (line as usize) < HEIGHT && (2..=WIDTH as i32 + 1).contains(&dot) {
            let x = (dot - 2) as usize;
            let sprite = self.sprite_line[x];
            let mux = self.mux;
            if sprite != 0 && mux & 0x10 != 0 && (x >= 8 || mux & 0x04 != 0) {
                let value = self.palette[(sprite & 0x1F) as usize];
                self.set_pixel(line, x, value);
            } else {
                self.fill_backdrop(line, dot, dot);
            }
        }
        if (2..=WIDTH as i32 + 1).contains(&dot) || (322..=337).contains(&dot) {
            self.shift();
        }
        if (1..=WIDTH as i32).contains(&dot) || (321..=336).contains(&dot) {
            let phase = (dot - 1) & 7;
            let v = self.v;
            match phase {
                0 => {
                    if dot >= 9 {
                        self.reload();
                    }
                }
                1 => self.nt_latch = (v & 0xFF) as u8,
                3 => self.at_byte = (0xC0 | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)) as u8,
                5 => self.lo_latch = (v & 0xFF) as u8,
                7 => {
                    self.hi_latch = (v & 0xFF) as u8;
                    self.at_latch = (self.at_byte >> (((v >> 4) & 4) | (v & 2))) & 3;
                    self.v = if v & 0x1F == 31 {
                        (v & !0x1F) ^ 0x0400
                    } else {
                        v + 1
                    };
                    self.reload_pending = true;
                }
                _ => {}
            }
            if dot == WIDTH as i32 {
                self.increment_y();
            }
        } else if dot == WIDTH as i32 + 1 {
            self.reload();
            self.v = (self.v & !0x041F) | (self.t & 0x041F);
        } else if WIDTH as i32 + 1 < dot && dot <= 320 {
            self.sprite_fade(line, dot);
        } else if dot == 337 {
            self.reload();
        }
    }

    fn after_fade(&mut self, line: i32, dot: i32) {
        if (line as usize) < HEIGHT && (2..=WIDTH as i32 + 1).contains(&dot) {
            self.fill_backdrop(line, dot, dot);
        }
        if (2..=WIDTH as i32 + 1).contains(&dot) || (322..=337).contains(&dot) {
            self.shift();
            if self.reload_pending {
                self.reload_pending = false;
                self.reload();
            }
        }
    }

    fn sprite_fade(&mut self, line: i32, dot: i32) {
        let visible = (line as usize) < HEIGHT;
        let line = if visible { line } else { line & 0xFF };
        let slot = ((dot - 257) >> 3) as usize;
        let phase = (dot - 257) & 7;
        if dot == 257 {
            self.oam2_fetch = 0;
        } else if phase <= 3 && !self.oam2_frozen {
            self.oam2_fetch += 1;
        }
        let address = self.oam2_fetch & 31;
        match phase {
            0 => self.fetch_y = self.oam2[address],
            1 => self.fetch_tile = self.oam2[address],
            2 => {
                self.fetch_attr = self.oam2[address];
                self.lanes[slot].attr = self.fetch_attr;
            }
            3 => {
                let x = self.oam2[address];
                let now = self.time;
                let ticks = self.dot_ticks;
                let lane = &mut self.lanes[slot];
                lane.x = x;
                if lane.zero > now {
                    lane.zero = now + x as i64 * ticks;
                }
            }
            5 => {
                let bits = (self.v & 0xFF) as u8;
                self.lanes[slot].lo = self.lane_bits(slot, visible, line, self.fetch_y, bits);
            }
            7 => {
                let bits = (self.v & 0xFF) as u8;
                self.lanes[slot].hi = self.lane_bits(slot, visible, line, self.fetch_y, bits);
            }
            _ => {}
        }
    }

    fn turn_off(&mut self) {
        self.on = false;
        self.waking = false;
        self.fading = 2;
        if (self.line as usize) < HEIGHT && self.dot <= WIDTH as i32 {
            self.lanes_shifted(((self.dot + 1) as usize).min(WIDTH));
        }
    }

    /// Rendering is on and this is a line on which the PPU fetches.
    pub fn rendering_now(&self) -> bool {
        self.on && ((self.line as usize) < HEIGHT || self.line == self.prerender_line)
    }

    // -- the CPU's registers -----------------------------------------------

    pub fn read_register(&mut self, register: u16) -> u8 {
        if register == 2 {
            let value = (self.status & 0xE0) | (self.decayed_latch() & 0x1F);
            if self.line == self.timing.vblank_line && self.dot == 0 {
                self.vbl_suppress = true;
            }
            self.status &= !VBL;
            self.nmi = false;
            self.w = 0;
            self.drive(value, 0xE0);
            return value;
        }
        if register == 4 {
            let value = if !self.rendering_now() {
                self.oam[self.oam_addr as usize]
            } else if self.line == self.prerender_line && (1..=64).contains(&self.dot) {
                0xFF
            } else {
                self.oam_bus
            };
            self.drive(value, 0xFF);
            return value;
        }
        if register == 7 {
            let v = self.v & 0x3FFF;
            let value;
            if v >= 0x3F00 {
                let mut p = self.palette[palette_index(v)];
                if self.mask & 0x01 != 0 {
                    p &= 0x30;
                }
                value = p | (self.decayed_latch() & 0xC0);
                self.drive(value, 0x3F);
            } else {
                value = self.buffer;
                self.drive(value, 0xFF);
            }
            if self.time >= self.read_in_flight_until {
                let dots = DELAY_2007_INCREMENT;
                self.read_in_flight_until = self.time + (dots + 1) * self.dot_ticks;
                if self.rendering_now() {
                    self.schedule(DELAY_2007_READ_ALE, Event::Read2007Ale);
                }
                self.schedule(dots, Event::Complete2007Read);
            }
            return value;
        }
        self.decayed_latch()
    }

    // -- the I/O latch -----------------------------------------------------

    fn drive(&mut self, value: u8, bits: u8) {
        self.latch = (self.latch & !bits) | (value & bits);
        let now = self.time;
        for bit in 0..8 {
            if (bits >> bit) & 1 != 0 {
                self.latch_times[bit] = now;
            }
        }
    }

    fn decayed_latch(&mut self) -> u8 {
        let mut latch = self.latch;
        if latch != 0 {
            let limit = self.time - self.latch_decay_ticks;
            for bit in 0..8 {
                if (latch >> bit) & 1 != 0 && self.latch_times[bit] < limit {
                    latch &= !(1 << bit);
                }
            }
            self.latch = latch;
        }
        latch
    }

    /// A register read's value as M2 falls, the cycle's end.
    pub fn end_read(&self, register: u16, value: u8) -> u8 {
        if register == 2 {
            return (value & 0x9F) | (self.status & 0x60);
        }
        value
    }

    fn read_2007_ale(&mut self) {
        let dot = self.dot + 1;
        if self.rendering_now() && dot & 1 == 0 && (2..=340).contains(&dot) {
            let address = self.fetch_address(dot);
            let low = self.octal.unwrap_or(address & 0xFF);
            self.octal = Some(self.settle(address & 0x3F00, low, 7));
        }
    }

    fn complete_2007_read(&mut self) {
        if self.rendering_now() {
            let dot = self.dot + 1;
            if dot & 1 == 0 && (2..=340).contains(&dot) {
                if self.octal.is_none() {
                    self.octal = Some(self.fetch_address(dot) & 0xFF);
                }
                self.refill_from_fetch = true;
            } else if dot & 1 != 0 {
                let high = self.fetch_address(dot + 1) & 0x3F00;
                let low = self.settle(high, self.bus_data as u16, 3);
                self.buffer = self.bus_read(high | low);
                self.octal = Some(low);
            } else {
                self.refill_from_fetch = true;
            }
        } else {
            let v = self.v & 0x3FFF;
            let value = self.bus_read(if v >= 0x3F00 { v - 0x1000 } else { v });
            if !self.resetting {
                self.buffer = value;
            }
        }
        self.increment_after_2007();
    }

    fn increment_after_2007(&mut self) {
        if self.rendering_now() {
            let v = self.v;
            self.v = if v & 0x1F == 31 {
                (v & !0x1F) ^ 0x0400
            } else {
                v + 1
            };
            self.increment_y();
        } else {
            self.v = (self.v + if self.ctrl & 0x04 != 0 { 32 } else { 1 }) & 0x7FFF;
            if self.watch {
                let a = self.v & 0x3FFF;
                self.ale(a);
            }
        }
    }

    /// A CPU write; `early` is the open-bus value the data bus holds at the
    /// start of the write cycle (None: valid for the whole cycle); `phase`:
    /// master ticks from the access to the start of the dot the PPU is in.
    pub fn write_register(&mut self, register: u16, value: u8, early: Option<u8>, phase: i64) {
        self.drive(value, 0xFF);
        match register {
            0 => {
                if self.resetting {
                    return;
                }
                self.ctrl = value;
                self.t = (self.t & !0x0C00) | (((value & 0x03) as u16) << 10);
                self.nmi = value & 0x80 != 0 && self.status & VBL != 0;
            }
            1 => {
                if self.resetting {
                    return;
                }
                if self.rendering_now() && value & 0x18 == 0 {
                    if (self.line as usize) < HEIGHT
                        && (65..WIDTH as i32).contains(&self.dot)
                        && self.mask & 0x18 != 0
                    {
                        self.last_eval_step();
                    }
                    self.oam_corrupt = Some(self.oam2_row());
                } else if value & 0x18 != 0 && (self.line as usize) < HEIGHT {
                    self.oam_corrupt = None;
                }
                self.mask = value;
                let target = value & 0x18 != 0;
                if target != self.target {
                    self.target = target;
                    self.schedule(
                        1,
                        if target {
                            Event::WakeUp
                        } else {
                            Event::TurnOff
                        },
                    );
                }
                self.schedule(1, Event::LatchMux(0x0A, value));
                self.schedule(2, Event::LatchMux(0x14, value));
                let grey: u8 = if value & 0x01 != 0 { 0x30 } else { 0x3F };
                let first: u8 = if early.unwrap_or(value) & 0x01 != 0 {
                    0x30
                } else {
                    0x3F
                };
                let ticks = self.dot_ticks;
                if self.prev_pixel_time == self.time - ticks {
                    let index = self.prev_pixel as usize;
                    self.picture[index] =
                        (self.prev_value & first) as u16 | (self.picture[index] & 0x1C0);
                }
                if self.last_pixel_time == self.time {
                    let colour = (self.last_value & grey) as u16;
                    self.picture[self.last_pixel as usize] =
                        colour | (((value & 0xE0) as u16) << 1);
                } else if self.last_pixel_time == self.time - ticks {
                    let index = self.last_pixel as usize;
                    self.picture[index] =
                        (self.last_value & first) as u16 | (self.picture[index] & 0x1C0);
                }
            }
            3 => {
                if !self.timing.delayed_oamaddr {
                    self.write_oam_addr(value);
                } else {
                    let lost = match self.timing.oam_refresh {
                        Some(refresh) => {
                            refresh.0 <= self.line && self.line <= refresh.1 && phase == 1
                        }
                        None => false,
                    };
                    if !lost {
                        self.schedule(DELAY_2003_DELAYED, Event::SetOamAddr(value));
                    }
                }
            }
            4 => {
                if self.rendering_now() {
                    self.oam_addr = self.oam_addr.wrapping_add(4) & 0xFC;
                } else if self
                    .timing
                    .oam_refresh
                    .is_some_and(|r| r.0 <= self.line && self.line <= r.1)
                {
                    self.schedule(DELAY_2004_REFRESH, Event::WriteOamRefreshing(value));
                } else {
                    self.write_oam(value);
                }
            }
            5 => {
                if self.resetting {
                    return;
                }
                if self.w == 0 {
                    self.t = (self.t & !0x001F) | (value >> 3) as u16;
                    self.x = value & 0x07;
                } else {
                    self.t = (self.t & !0x73E0)
                        | (((value & 0x07) as u16) << 12)
                        | (((value & 0xF8) as u16) << 2);
                }
                self.w ^= 1;
            }
            6 => {
                if self.resetting {
                    return;
                }
                if self.w == 0 {
                    self.t = (self.t & 0x00FF) | (((value & 0x3F) as u16) << 8);
                } else {
                    self.t = (self.t & 0x7F00) | value as u16;
                    self.schedule(DELAY_2006_COPY, Event::CopyT);
                }
                self.w ^= 1;
            }
            7 => {
                self.schedule(DELAY_2007_WRITE, Event::Write2007(value));
                self.schedule(DELAY_2007_INCREMENT, Event::Increment2007);
            }
            _ => {}
        }
    }

    fn copy_t(&mut self) {
        if self.rendering_now() {
            let dot = self.dot + 1;
            if dot & 1 == 0 && (2..=340).contains(&dot) && self.octal.is_none() {
                self.octal = Some(self.fetch_address(dot) & 0xFF);
            }
        }
        self.v = self.t;
        if self.watch && !self.rendering_now() {
            let a = self.v & 0x3FFF;
            self.ale(a);
        }
    }

    fn write_2007(&mut self, v: u16, value: u8) {
        let mut v = v;
        let mut value = value;
        if self.rendering_now() {
            let dot = self.dot + 1;
            if dot & 1 != 0 {
                v = self.ale_address(dot);
                value = (v & 0xFF) as u8;
            } else {
                v = (self.ale_address(dot + 1) & 0x3F00) | value as u16;
            }
        }
        if v >= 0x3F00 {
            self.palette[palette_index(v)] = value & 0x3F;
        } else {
            self.bus_write(v, value);
        }
    }

    /// A DMA write to $2004.
    pub fn oam_dma_write(&mut self, value: u8) {
        self.write_register(4, value, None, 0);
    }
}

/// $3F10/$14/$18/$1C are the cells of $3F00/$04/$08/$0C.
pub fn palette_index(address: u16) -> usize {
    let index = (address & 0x1F) as usize;
    if index & 0x13 == 0x10 {
        index & 0x0F
    } else {
        index
    }
}
