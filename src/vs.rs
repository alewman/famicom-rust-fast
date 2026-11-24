// SPDX-License-Identifier: MIT
//! The Nintendo Vs. System's mainboard: what it adds around a 2A03 and an RGB
//! PPU (`vs.py`).
//!
//! Vs._System.wiki (NESdev wiki; specification tier) is the source of every
//! statement here not marked otherwise; MAME's driver (`vsnes.cpp`, BSD-3,
//! pinned by the reference as reference/mame-nes/vsnes.cpp; emulator-derived)
//! is the second.
//!
//! The MDS mainboard carries two consoles side by side, each a letterless RP2A03,
//! an RGB PPU, 2 KiB of work RAM, 4 KiB of nametable RAM (always four screens)
//! and six EPROM sockets. A UniSystem cabinet uses one side. Against a Famicom,
//! each side has:
//!
//! - **$4016/$4017 reads** through 74LS240 buffers that drive all eight data
//!   lines, so nothing here is open bus:
//!
//!   ```text
//!   $4016   P C C D  D S 0 B      $4017   D D D D  D D 0 B
//!           | | | |  | | | serial data, right stick     | | | | | | | serial data, left stick
//!           | | | |  | | always 0                       | | | | | | always 0
//!           | | | |  | service button                   DIP switches 8 .. 3
//!           | | | DIP switches 2, 1
//!           | coin acceptors 2, 1
//!           0: the primary side's CPU, 1: the secondary's
//!   ```
//!
//!   The serial data are 4021 shift registers on the mainboard, read exactly as
//!   a controller is: the board's two `pads`.
//! - **$4016 writes**: OUT0 strobes the sticks; OUT1 requests an interrupt on the
//!   other side and, from the primary side, hands the shared RAM over (the
//!   DualSystem); OUT2 goes to the ROM sockets (the CHR bank of mapper 99).
//! - **$4020-$5FFF**: one latch bit, the coin counter's electromagnet. "Reading
//!   from the register effectively writes the value of open bus."
//! - **$6000-$7FFF**: 2 KiB of RAM, mirrored, shared by the two sides and owned
//!   by one at a time; battery-backed.
//!
//! On the [`Board`](crate::board::Board) this object stands where a Famicom's
//! expansion-port device stands (`set_out`, `read`, `clock`: the same three
//! signals), and the board asks it for $4020-$7FFF.
//!
//! **Coins.** "The coin collectors will read as true for approximately 40 to
//! 70ms"; MAME holds them 3 frames (50 ms) after hardware tests of 2.5 to 4 frames
//! (vsnes.cpp `VS_COINS_AND_SERVICE`). Games reject a contact that lasts too
//! long or too short, so a coin is a pulse on the board's clock
//! ([`VsMainboard::insert_coin`]), 50 ms by default. "The default UniSystem
//! harness wiring connects both coin acceptors in parallel, and only to the $20s
//! bit", and "emulators should NOT set both bits high when a single virtual coin
//! has been inserted": acceptor 1 is D5, acceptor 2 is D6.
//!
//! Not modelled: the service button's RC debounce (3-6 ms to read released; the
//! bit follows the button here), the coin counter's mechanics (the latch is kept
//! and its pulses counted), and the watchdog, which resets everything when the
//! secondary CPU has not read $4017 for about 1.2 s (no game that runs trips it).
//!
//! Differences forced by Rust:
//!
//! - the reference's mainboard holds three callables: `now` (the board's
//!   clock), `cartridge_out` (the sockets' OUT2) and, on a DualSystem,
//!   `other_irq` (the other side's /IRQ input). Here the board hands
//!   [`VsMainboard::read`] and [`VsMainboard::insert_coin`] the time and
//!   [`VsMainboard::set_out`] the cartridge, and the other side's /IRQ is a
//!   level [`VsMainboard::set_out`] leaves in `pending_irq` for the DualSystem
//!   (`dual.rs`) to pass on before the other side runs again;
//! - the reference's DualSystem shares one [`SharedRam`] object between the two
//!   sides; here each side holds one, and `dual.rs` keeps them one (the owner
//!   alike on both sides, the bytes with whichever side owns them);
//! - the reference's DualSystem watches the secondary side's bus for $4017
//!   reads (a bus observer); here the board writes the read's time in
//!   [`VsMainboard::fed`].

use crate::cartridge::Cartridge;
use crate::joypad::{StandardController, A, B, DOWN, LEFT, RIGHT, SELECT, START, UP};
use crate::peripherals::{PortDevice, Zapper};
use crate::ppu::Ppu;
use std::any::Any;

/// NES 2.0 "Default Expansion Device" numbers of the Vs. System (NES_2.0.wiki):
/// the sticks with player 1 on $4016 or on $4017, and the Vs. Zapper.
pub const VS_1P_4016: u8 = 0x04;
pub const VS_1P_4017: u8 = 0x05;
pub const VS_ZAPPER: u8 = 0x07;
pub const VS_DEVICES: [u8; 3] = [VS_1P_4016, VS_1P_4017, VS_ZAPPER];

/// The panel's four numbered buttons, where a controller has Select and Start:
/// "Buttons for right stick (A, B, 1, 3, Up, Down, Left, Right)" on $4016 and
/// "left stick (A, B, 2, 4, ...)" on $4017. As (port, bit) by button number.
pub const BUTTONS: [(u8, (usize, u8)); 4] = [
    (1, (0, SELECT)),
    (2, (1, SELECT)),
    (3, (0, START)),
    (4, (1, START)),
];

pub const COIN_SECONDS: f64 = 0.050;

/// A side of the mainboard: the reference's strings "primary" and
/// "secondary" (a Rust enum, so that a state can hold it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Primary,
    Secondary,
}

impl Side {
    /// The reference's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Side::Primary => "primary",
            Side::Secondary => "secondary",
        }
    }

    /// From the reference's name (`Machine::vs_side`).
    pub fn from_name(name: &str) -> Side {
        match name {
            "primary" => Side::Primary,
            "secondary" => Side::Secondary,
            other => panic!("no side {other:?}"),
        }
    }
}

/// One stick and its buttons on the mainboard's 4021: a controller's report.
///
/// `jumper` is bits that always read pressed. *Vs. Ice Climber*'s Japanese
/// board and *Vs. Raid on Bungeling Bay* expect the fourth bit of both reports
/// set (NES 2.0 Vs. hardware types 4 and 6; MAME's `iceclmbj` and `bnglngby`
/// ports, "protection? START on a NES": emulator-derived, the wiring itself is
/// not documented).
///
/// The reference subclasses `StandardController`; here the stick has the
/// controller's fields and its own `jumper`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct VsStick {
    /// Pressed buttons, the controller's bits.
    pub buttons: u8,
    pub shift: u8,
    pub strobe: bool,
    pub jumper: u8,
}

impl VsStick {
    pub fn new(jumper: u8) -> VsStick {
        VsStick {
            jumper,
            ..VsStick::default()
        }
    }

    pub fn set_strobe(&mut self, high: bool) {
        if high || self.strobe {
            self.shift = self.buttons | self.jumper;
        }
        self.strobe = high;
    }

    pub fn read(&self) -> u8 {
        if self.strobe {
            return (self.buttons | self.jumper) & 1;
        }
        self.shift & 1
    }

    pub fn clock(&mut self) {
        if self.strobe {
            self.shift = self.buttons | self.jumper;
        } else {
            self.shift = (self.shift >> 1) | 0x80;
        }
    }
}

impl PortDevice for VsStick {
    fn set_strobe(&mut self, high: bool, _: &Ppu) {
        VsStick::set_strobe(self, high);
    }
    fn read(&mut self, _: &Ppu) -> u8 {
        VsStick::read(self)
    }
    fn clock(&mut self, _: &Ppu) {
        VsStick::clock(self);
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The Vs. System's light gun: a report on $4016's 4021, not two port lines.
///
/// Zapper.wiki: "0, 0, 0, 0, 1, 0, Light sense (inverted), Trigger ... the
/// light sense status corresponds to Left and the trigger to Right, and Up is
/// always pressed. Unlike the NES/Famicom Zapper, the Vs. Zapper's light sense
/// is 1 when detecting". The sensor is the NES Zapper's model
/// ([`Zapper::light`]); the trigger is taken as a plain switch (MAME's port;
/// the NES gun's half-pull timing is that gun's capacitor).
///
/// The reference subclasses `Zapper`; here the gun is the field `zapper`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VsZapper {
    pub zapper: Zapper,
    pub shift: u8,
    pub strobe: bool,
}

impl VsZapper {
    pub fn new(ppu: &Ppu, palette: &[[u8; 3]]) -> VsZapper {
        VsZapper {
            zapper: Zapper::new(ppu, palette),
            shift: 0,
            strobe: false,
        }
    }

    fn report(&self, ppu: &Ppu) -> u8 {
        UP | (if self.zapper.light(ppu) { LEFT } else { 0 })
            | (if self.zapper.trigger() { RIGHT } else { 0 })
    }
}

impl PortDevice for VsZapper {
    fn set_strobe(&mut self, high: bool, ppu: &Ppu) {
        if high || self.strobe {
            self.shift = self.report(ppu);
        }
        self.strobe = high;
    }
    fn read(&mut self, ppu: &Ppu) -> u8 {
        (if self.strobe {
            self.report(ppu)
        } else {
            self.shift
        }) & 1
    }
    fn clock(&mut self, ppu: &Ppu) {
        if self.strobe {
            self.shift = self.report(ppu);
        } else {
            self.shift = (self.shift >> 1) | 0x80;
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The 2 KiB at $6000-$7FFF and which side holds it.
///
/// "In the DualSystem it can be swapped between CPUs; in the UniSystem it is
/// either ignored or a special jumper '2A04' was installed to give dedicated
/// access." A UniSystem's one side always has it here (`owner` never moves);
/// on a DualSystem the primary side's OUT1 chooses.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SharedRam {
    pub data: Vec<u8>,
    /// "primary" or "secondary".
    pub owner: Side,
    /// The primary side's OUT1 moves it.
    pub switched: bool,
}

impl SharedRam {
    pub fn new(owner: Side, switched: bool) -> SharedRam {
        SharedRam {
            data: vec![0; 0x800],
            owner,
            switched,
        }
    }
}

/// One side of the mainboard (see the module notes).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VsMainboard {
    pub side: Side,
    coin_ticks: i64,
    pub ram: SharedRam,
    /// DIP switches 1-8 as bits 0-7, 1 = on.
    pub dip: u8,
    pub service: bool,
    /// Each acceptor's coins: the master ticks at which a contact closes.
    pub coins: [Vec<i64>; 2],
    pub out: u8,
    /// The latch.
    pub coin_counter: u8,
    /// Its 1 -> 0 edges: what the meter shows.
    pub coins_counted: u64,
    /// The other side's /IRQ is wired to this side's OUT1 (a DualSystem; the
    /// reference's `other_irq` is not None).
    pub irq_wired: bool,
    /// The level OUT1 last asked of the other side's /IRQ, for the DualSystem
    /// to pass on (true asserts it).
    #[serde(skip)]
    pub pending_irq: Option<bool>,
    /// The master tick of this side's last $4017 read as the CPU's bus sees it
    /// (the reference's DualSystem's `_fed`, kept by a bus observer there).
    #[serde(skip)]
    pub fed: i64,
}

impl VsMainboard {
    pub fn new(side: Side, ticks_per_second: f64, ram: Option<SharedRam>) -> VsMainboard {
        VsMainboard {
            side,
            coin_ticks: (COIN_SECONDS * ticks_per_second).round_ties_even() as i64,
            ram: ram.unwrap_or_else(|| SharedRam::new(side, false)),
            dip: 0,
            service: false,
            coins: [Vec::new(), Vec::new()],
            out: 0,
            coin_counter: 0,
            coins_counted: 0,
            irq_wired: false,
            pending_irq: None,
            fed: 0,
        }
    }

    // -- the operator and the player ---------------------------------------

    /// A coin through acceptor 0 or 1: its contact closes at master tick `at`
    /// (the reference's default: now) for [`COIN_SECONDS`].
    pub fn insert_coin(&mut self, acceptor: usize, at: i64) {
        self.coins[acceptor].push(at);
    }

    // -- the expansion-device protocol (board.rs) --------------------------

    /// OUT0-2 moved (the 2A03 updates them on a put cycle).
    pub fn set_out(&mut self, value: u8, cartridge: &mut dyn Cartridge) {
        let changed = self.out ^ value;
        self.out = value;
        if changed & 2 != 0 {
            if self.irq_wired {
                // "When 0: asserts an IRQ request. When 1: releases" it.
                self.pending_irq = Some(value & 2 == 0);
            }
            if self.side == Side::Primary && self.ram.switched {
                // "When 1: the primary CPU has access and the secondary CPU
                // sees open bus. When 0: the secondary CPU has access."
                self.ram.owner = if value & 2 != 0 {
                    Side::Primary
                } else {
                    Side::Secondary
                };
            }
        }
        if changed & 4 != 0 {
            cartridge.out(value);
        }
    }

    /// D2-D7 of $4016 (port 0) or $4017; D0 is the stick's, D1 is 0. `now`:
    /// the board's clock.
    pub fn read(&mut self, port: usize, now: i64) -> u8 {
        let dip = self.dip;
        if port != 0 {
            return dip & 0xFC;
        }
        let mut bits = (dip & 3) << 3;
        if self.service {
            bits |= 0x04;
        }
        let length = self.coin_ticks;
        for (acceptor, coins) in self.coins.iter_mut().enumerate() {
            if !coins.is_empty() {
                if coins[0] + length <= now {
                    // fallen through: forget it
                    coins.retain(|&start| now < start + length);
                }
                if coins.iter().any(|&start| start <= now) {
                    bits |= 0x20 << acceptor;
                }
            }
        }
        if self.side == Side::Secondary {
            bits |= 0x80;
        }
        bits
    }

    /// The end of a read of `port`: the sticks shift themselves.
    pub fn clock(&mut self, _port: usize) {}

    // -- $4020-$7FFF (board.rs) --------------------------------------------

    /// $4020-$5FFF: D0 is the electromagnet; "writing 1 then 0 will
    /// increment the counter". A read lands here too, with open bus.
    pub fn write_coin_counter(&mut self, value: u8) {
        let value = value & 1;
        if self.coin_counter != 0 && value == 0 {
            self.coins_counted += 1;
        }
        self.coin_counter = value;
    }

    pub fn read_ram(&self, address: u16) -> Option<u8> {
        let ram = &self.ram;
        if ram.owner != self.side {
            return None; // the other side has it: open bus
        }
        Some(ram.data[address as usize & 0x7FF])
    }

    pub fn write_ram(&mut self, address: u16, value: u8) {
        let side = self.side;
        let ram = &mut self.ram;
        if ram.owner == side {
            ram.data[address as usize & 0x7FF] = value;
        }
    }

    /// Reset: "this register is explicitly cleared on powerup and reset".
    pub fn reset(&mut self) {
        self.coin_counter = 0;
    }
}

/// A stick's buttons, if `pad` is a controller (a [`VsStick`] or a
/// [`StandardController`]: the reference's `isinstance(pad, StandardController)`).
fn controller_buttons(pad: &mut dyn PortDevice) -> Option<&mut u8> {
    let any = pad.as_any_mut();
    if any.is::<VsStick>() {
        return any
            .downcast_mut::<VsStick>()
            .map(|stick| &mut stick.buttons);
    }
    any.downcast_mut::<StandardController>()
        .map(|pad| &mut pad.buttons)
}

/// Put the players' buttons (`joypad` bits, Select and Start standing for
/// the panel's numbered buttons) on the two sticks.
///
/// The cabinet wires "the left stick to $4017 and the right stick to $4016,
/// but some games expect the first player to use the right stick, while others
/// expect the first player to use the left stick": NES 2.0 device $04 says
/// player 1 is read through $4016, $05 through $4017. The numbered buttons do
/// not move with the sticks: buttons 1 and 3 are always on $4016, 2 and 4 on
/// $4017, and player 1's Select and Start are taken as buttons 1 and 3,
/// player 2's as 2 and 4.
pub fn panel(pads: &mut [Box<dyn PortDevice>; 2], players: &[u8], first_on_4017: bool) {
    let stick = A | B | UP | DOWN | LEFT | RIGHT;
    let numbered = SELECT | START;
    let one = players.first().copied().unwrap_or(0);
    let two = players.get(1).copied().unwrap_or(0);
    let (on_4016, on_4017) = if first_on_4017 {
        (two, one)
    } else {
        (one, two)
    };
    if let Some(buttons) = controller_buttons(pads[0].as_mut()) {
        *buttons = (on_4016 & stick) | (one & numbered);
    }
    if let Some(buttons) = controller_buttons(pads[1].as_mut()) {
        *buttons = (on_4017 & stick) | (two & numbered);
    }
}
