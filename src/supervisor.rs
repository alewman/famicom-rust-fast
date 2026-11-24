// SPDX-License-Identifier: MIT AND BSD-3-Clause
// The BIOS SHA-1s and the screen's weights are MAME's playch10.cpp (BSD-3-Clause, Ernesto
// Corvi, Brad Oliver); the RP5H01 its rp5h01.cpp (BSD-3-Clause, Nicola Salmoria): NOTICE.md.
//! The PlayChoice-10's supervisor: a Z80, its BIOS, its own screen, and the
//! cabinet (`supervisor.py`).
//!
//! Sources, by tier (`playchoice.rs` has the game board's): Nintendo's PCH1-CPU
//! schematic (the reference's `reference/docs/Playchoice.pdf`, manufacturer's
//! documentation, read by eye: "sheet N" below, with the part's board
//! position); MAME's `playch10.cpp` and `rp5h01.cpp` (BSD-3, pinned by the
//! reference under `reference/mame-nes/`; emulator-derived); PC10_ROM-Images.wiki
//! (specification). Where the schematic and MAME differ the schematic is
//! followed and the difference is a numbered contradiction (the reference's
//! docs/undocumented-behavior.md C82-C86).
//!
//! **The Z80** (sheet 1): a Z80A on an 8 MHz crystal halved, 4 MHz (z80-rust,
//! the transcription of the family's core z80-python). Its memory:
//!
//! ```text
//! $0000-$3FFF  the BIOS (8T, 16 KiB)
//! $8000-$87FF  RAM (8V)
//! $8800-$8FFF  RAM (8W), kept by a capacitor when the power is off; its
//!              A10 is ANDed with the latch's "BUP KEY", so the upper half
//!              is the lower's mirror until the BIOS turns the key
//! $9000-$97FF  the screen's RAM (8R): the Z80's only while SDCS is low
//! $C000-$DFFF  the selected channel's INST ROM
//! $E000-$FFFF  the selected channel's security PROM (/ID SEL)
//! ```
//!
//! and its ports (sheet 3; reads decode A0-A1, writes A0-A3):
//!
//! ```text
//! read 0   CH Select, Enter, Reset, INT DETECT, 0, Coin 2, Service, Coin 1
//!          (bits 0 to 7, through a 74LS240)
//! read 1   DIP switches A-H (SW1)      read 2   DIP switches I-P (SW2)
//! read 3   /DETECT CLR: clears INT DETECT
//! write 0-7, 8-F   two 74LS259 addressable latches, bit 0 of the data:
//!     0 SDCS          1 /CNTRL MASK    2 /DISP MASK    3 /SOUND MASK
//!     4 /GAME RES     5 /GAME STOP     6 -             7 -
//!     8 NMI EI        9 DOG DI         A /PPU RES      B-E CSEL0-3
//!     F BUP KEY ("backup key"; MAME reads the label as "8UP KEY")
//! ```
//!
//! Both latches are cleared by the Z80's reset: the game held in reset, the PPU
//! held in reset, picture, sound and Start masked, channel 1.
//!
//! **Its screen** (sheet 2): 32 x 32 characters of 8 x 8 pixels from the screen
//! RAM, two bytes each (the character's low 8 bits; then bits 2-0 its high bits
//! and bits 7-3 its colour), three bitplanes in EPROMs 8P, 8M and 8K (bits 0, 1,
//! 2 of the pixel), and three 82S129 PROMs (6F red, 6E green, 6D blue) addressed
//! by colour and pixel, four bits a channel through resistor networks (the
//! weights are MAME's: 0x0E, 0x1F, 0x43, 0x8F, on the PROM's inverted bits). The
//! counters (74LS161s from a 20.160 MHz crystal, a quarter of it the dot clock)
//! count 256 dots and then reload to 185: 327 dots a line; and 256 lines: 60.21
//! frames a second, nothing to do with the game's 60.10. The vertical blank
//! flip-flop (5M) sets as line 240 begins and clears as line 16 does, and it
//! blanks the picture: 224 lines show, rows 2 to 29 (C83: MAME shows rows 0 to
//! 29 at 60 Hz). While SDCS is low the Z80 has the RAM and the screen is blank.
//!
//! **The vertical blank** clocks two things (sheet 1). A flip-flop (74LS109, 4Y)
//! pulls the Z80's /NMI low when NMI EI is high, and holds it until NMI EI goes
//! low: the BIOS re-arms it each time. A counter (74LS161, 4R) is held at zero
//! while NMI EI or DOG DI is high and counts blanks otherwise; its eighth resets
//! the Z80 (C84: MAME resets on the first).
//!
//! **The game's lines** (sheets 5 and 6). /GAME RES is the 2A03's reset, /PPU RES
//! the 2C03's, separately. /GAME STOP goes to the 2A03E's pin 30 (its /RDY)
//! through two flip-flops clocked by the PPU's /INT (its vertical blank
//! interrupt, a few M2 cycles delayed): the game stops, and starts again, only
//! just after one of its own NMIs, and never if it has not enabled them (C85:
//! MAME halts at once). The same pulse sets INT DETECT, which the BIOS reads and
//! clears: "the game is running and interrupting". /DISP MASK and /SOUND MASK
//! are analogue switches (4066s) in the game's RGB and sound; /CNTRL MASK is
//! `playchoice::PlayChoicePad::masked`.
//!
//! **The security PROM** ([`Rp5h01`]; sheet 6, MAME's rp5h01.cpp): a write to
//! $E000-$FFFF latches D0, D3 and D4 (74LS175, 4S) as the chip's /RESET
//! (inverted), clock and TEST; a read returns its data on D3 and its counter
//! output on D4, both through inverting buffers, and 1 on the other six lines
//! (pull-ups). The BIOS clocks the bits out and checks them against the INST
//! ROM; nothing here knows the check: the BIOS runs it.
//!
//! **Ten channels, one game.** The cabinet has ten sockets (CSEL0-3 through a
//! 74LS42); this model has one game, in channel 1, and nine empty sockets whose
//! INST ROM and PROM read as pulled-up lines (MAME does the same).
//!
//! **Two processors, three crystals** ([`PlayChoice`]). The game board is a
//! [`System`] on the master clock; the Z80 counts its own T-states. They are
//! interleaved an instruction at a time, the one behind stepping next, with the
//! crystals taken at their nominal ratios (a Z80 T-state is 945/176 master
//! ticks; a screen dot 50/63 T-states). A latch write reaches the game within
//! one instruction of either processor, never on its cycle: the limit of this
//! model (and of z80-python, which reports an instruction's length, not where in
//! it a port is written). No source here measures better, and the two sides
//! share nothing faster than a latch bit and a serial PROM.
//!
//! Differences forced by Rust:
//!
//! - the reference's `Supervisor` hands its Z80 four bound methods as the
//!   memory and I/O buses; z80-rust's `Z80` owns its [`Bus`], so what those
//!   methods touch (the ROMs, the RAMs, the latches, the DIP switches, the
//!   buttons, the coins, INT DETECT, the time digits and the T-state count) is
//!   a [`SupervisorBus`], `Supervisor::cpu.bus`, and the rest (the blank, the
//!   watchdog, the screen's cache) is the [`Supervisor`]'s;
//! - the reference's `on_line(number, level)` callable, called inside the Z80's
//!   port write, is a queue here: the bus appends each change
//!   ([`LineChange`], with SDCS as it stood) and the cabinet applies them in
//!   order after the Z80's instruction ([`PlayChoice::supervisor_step`]). The
//!   game does not run during a Z80 instruction and nothing the Z80 reads
//!   depends on what the lines do to it, so the order of everything either
//!   processor sees is the reference's;
//! - z80-python raises on the conditions z80-rust returns as a `Fault`; the
//!   BIOS meets none, and a fault here panics, as the reference's run would
//!   end on the exception;
//! - `Bios` loads from a zip or a folder through the `zip` crate.

use crate::cartridge::{self, database::sha1_hex, Loaded};
use crate::machine::Machine;
use crate::playchoice::{game_roms, is_playchoice, GameRoms, PlayChoiceGun, PlayChoicePad};
use crate::ppu::{HEIGHT, WIDTH};
use crate::system::System;
use crate::video::rgb_palette;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use z80_rust::{Bus, Z80};

// The latches' outputs, by port number (module notes).
pub const SDCS: usize = 0;
pub const CNTRL_MASK: usize = 1;
pub const DISP_MASK: usize = 2;
pub const SOUND_MASK: usize = 3;
pub const GAME_RES: usize = 4;
pub const GAME_STOP: usize = 5;
pub const NMI_EI: usize = 8;
pub const DOG_DI: usize = 9;
pub const PPU_RES: usize = 10;
pub const CSEL0: usize = 11;
pub const CSEL1: usize = 12;
pub const CSEL2: usize = 13;
pub const CSEL3: usize = 14;
pub const BUP_KEY: usize = 15;

// Port 0's bits.
pub const CHANNEL: u8 = 1 << 0;
pub const ENTER: u8 = 1 << 1;
pub const RESET: u8 = 1 << 2;
pub const INT_DETECT: u8 = 1 << 3;
pub const COIN_2: u8 = 1 << 5;
pub const SERVICE: u8 = 1 << 6;
pub const COIN_1: u8 = 1 << 7;

pub const Z80_HZ: i64 = 4_000_000;
/// The screen's time is counted in units of 1/63 T-state: a dot is 50 of them
/// (5.04 MHz against 4 MHz).
pub const UNITS_PER_T: i64 = 63;
pub const UNITS_PER_DOT: i64 = 50;
/// 256, then the counters reload to 185 and run to 255 (sheet 2, 6L and 6K).
pub const DOTS_PER_LINE: i64 = 327;
pub const LINES: i64 = 256;
/// The flip-flop 5M: set at line 240, cleared at line 16.
pub const VBLANK_FROM: usize = 240;
pub const VBLANK_TO: usize = 16;
pub const FRAME_UNITS: i64 = DOTS_PER_LINE * LINES * UNITS_PER_DOT;
/// The counter 4R's QD.
pub const WATCHDOG_BLANKS: u32 = 8;
/// A coin's contact, as the Vs. System's (vs::COIN_SECONDS): unmeasured here.
pub const COIN_SECONDS: f64 = 0.050;

/// The BIOS versions, as MAME names them, and the file of each (playch10.cpp
/// `BIOS_CPU`): the SHA-1s are MAME's. "dual" is the two-monitor cabinet the
/// schematic draws; "single" is the one-monitor board's (PCK1), which has no
/// schematic here.
pub const BIOS_PROGRAMS: [(&str, (&str, &str)); 2] = [
    (
        "dual",
        (
            "pch1-c__8t_e-2.8t",
            "55cabf52ae10c050c2229081a80b9fe5454ab8c5",
        ),
    ),
    (
        "single",
        ("pck1-c.8t", "3bd20bc71cac742d1b8c1430a6426d0a19db7ad0"),
    ),
];
/// The screen's EPROMs (pixel bits 2, 1, 0) and colour PROMs (red, green, blue).
pub const BIOS_CHARACTERS: [(&str, &str); 3] = [
    ("pch1-c__8k.8k", "b814f10ef23f2ca445fabafcbf7f25e2d454ba8c"),
    (
        "pch1-c__8m_e-1.8m",
        "beaf9fa2d091a3c7f70c51e966d885b1f9f0935f",
    ),
    (
        "pch1-c__8p_e-1.8p",
        "69166afdb2fe827c7f1919cdf4197caccbd961fa",
    ),
];
pub const BIOS_PROMS: [(&str, &str); 3] = [
    (
        "pch1-c-6f.82s129an.6f",
        "d2878411cda84ffe0afb2e538a67457f51bebffb",
    ),
    (
        "pch1-c-6e.82s129an.6e",
        "a448b47c9289902e26a3d3c4c7d5a7968c385e81",
    ),
    (
        "pch1-c-6d.82s129an.6d",
        "0ad386fc3eab5e53c0288ad1de33639a9e461b7c",
    ),
];

/// The supervisor's ROMs: its program, its characters, its colours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bios {
    pub name: String,
    /// 16 KiB.
    pub program: Vec<u8>,
    /// 8K, 8M, 8P: pixel bits 2, 1, 0.
    pub characters: [Vec<u8>; 3],
    /// Red, green, blue: 256 x 4 bits.
    pub proms: [Vec<u8>; 3],
}

impl Bios {
    pub fn single_monitor(&self) -> bool {
        self.name == "single"
    }
}

/// Where the BIOS is: the zip (or folder) FAMICOM_PC10_BIOS names, else MAME's
/// `playch10.zip` in the directory MAME_ROMPATH names (the family's variable
/// for MAME's sets; default `roms`).
pub fn bios_path() -> PathBuf {
    if let Ok(named) = std::env::var("FAMICOM_PC10_BIOS") {
        if !named.is_empty() {
            return PathBuf::from(named);
        }
    }
    PathBuf::from(std::env::var("MAME_ROMPATH").unwrap_or_else(|_| "roms".into()))
        .join("playch10.zip")
}

/// The BIOS `name` from MAME's `playch10` set at `path` (default
/// [`bios_path`]), each file checked against MAME's SHA-1. Never shipped: it
/// is Nintendo's.
pub fn load_bios(name: &str, path: Option<&Path>) -> Result<Bios, String> {
    let path = path.map_or_else(bios_path, Path::to_path_buf);
    if !path.exists() {
        return Err(format!(
            "PlayChoice-10 BIOS not found at {}: set MAME_ROMPATH or FAMICOM_PC10_BIOS",
            path.display()
        ));
    }
    let mut archive = if path.is_dir() {
        None
    } else {
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        Some(zip::ZipArchive::new(file).map_err(|e| e.to_string())?)
    };
    let mut checked = |(member, sha1): (&str, &str)| -> Result<Vec<u8>, String> {
        let missing = || format!("{} has no {member}", path.display());
        let data = match archive.as_mut() {
            None => std::fs::read(path.join(member)).map_err(|_| missing())?,
            Some(archive) => {
                let mut entry = archive.by_name(member).map_err(|_| missing())?;
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
                data
            }
        };
        if !sha1_hex(&data).eq_ignore_ascii_case(sha1) {
            return Err(format!(
                "{}: {member} is not the file MAME's playch10 set names",
                path.display()
            ));
        }
        Ok(data)
    };
    let mut characters: [Vec<u8>; 3] = Default::default();
    for (slot, entry) in characters.iter_mut().zip(BIOS_CHARACTERS) {
        *slot = checked(entry)?;
    }
    let mut proms: [Vec<u8>; 3] = Default::default();
    for (slot, entry) in proms.iter_mut().zip(BIOS_PROMS) {
        *slot = checked(entry)?;
    }
    let program = BIOS_PROGRAMS
        .iter()
        .find(|(version, _)| *version == name)
        .ok_or_else(|| format!("no BIOS {name:?}"))?
        .1;
    Ok(Bios {
        name: name.to_string(),
        program: checked(program)?,
        characters,
        proms,
    })
}

/// Ricoh's serial PROM: 64 bits and a counter, read a bit at a time.
///
/// MAME's rp5h01.cpp, as pinned (emulator-derived; the datasheet is not
/// pinned): RESET high holds the counter at zero; a falling edge of the clock
/// while RESET is low advances it; TEST chooses whether the data pin follows
/// six bits of the counter or seven ("in 7-bit counter mode, from 64 to 127
/// ... the dummy bits are read"); the counter output is the counter's bit 5.
/// The image holds the two pins as they were recorded over 128 clocks
/// (playchoice::GameRoms), and this plays the recordings back: the first
/// clock's bit is bit 0 of the first byte.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rp5h01 {
    pub data: Vec<u8>,
    pub counter_out: Vec<u8>,
    pub counter: u8,
    pub reset: bool,
    pub clock: bool,
    pub test: bool,
}

impl Rp5h01 {
    pub fn new(data: Vec<u8>, counter_out: Vec<u8>) -> Rp5h01 {
        Rp5h01 {
            data,
            counter_out,
            counter: 0,
            reset: false,
            clock: false,
            test: false,
        }
    }

    /// The three input pins, as the board's latch drives them.
    pub fn write(&mut self, reset: bool, clock: bool, test: bool) {
        if reset {
            self.counter = 0;
        } else if self.clock && !clock {
            self.counter = (self.counter + 1) & 0x7F;
        }
        self.reset = reset;
        self.clock = clock;
        self.test = test;
    }

    pub fn data_out(&self) -> u8 {
        let index = self.counter & if self.test { 0x7F } else { 0x3F };
        (self.data[(index >> 3) as usize] >> (index & 7)) & 1
    }

    pub fn counter_output(&self) -> u8 {
        (self.counter_out[(self.counter >> 3) as usize] >> (self.counter & 7)) & 1
    }
}

/// A latch output that changed, as the reference's `on_line(number, level)`
/// is told it, with SDCS as it stood at that moment (the cabinet's /DISP MASK
/// rule reads it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub number: usize,
    pub level: u8,
    pub sdcs: u8,
}

/// What the Z80's buses reach: the reference's `Supervisor` attributes that its
/// `read_byte`, `write_byte`, `read_port` and `write_port` touch (module notes,
/// "Differences forced by Rust").
#[derive(Clone, Debug)]
pub struct SupervisorBus {
    pub rom: Vec<u8>,
    pub single_monitor: bool,
    pub inst: Option<Vec<u8>>,
    pub prom: Option<Rp5h01>,
    /// 8V.
    pub ram: Vec<u8>,
    /// Battery-backed.
    pub ram_8w: Vec<u8>,
    pub video_ram: Vec<u8>,
    pub latch: [u8; 16],
    /// The latch changes not yet applied by the cabinet (the reference's `on_line`).
    pub lines: Vec<LineChange>,
    /// DIP switches A-H and I-P as bits 0-7, 1 = on (MAME's SW1 and SW2).
    pub dip: [u8; 2],
    /// The cabinet's buttons held now: CHANNEL, ENTER, RESET, SERVICE.
    pub buttons: u8,
    /// Coins: each slot's contacts, as the T-state at which one closes.
    pub coins: [Vec<i64>; 2],
    pub int_detect: bool,
    /// The single-monitor BIOS's four time digits (MAME's `time_w`).
    pub digits: [u8; 4],
    /// T-states since power.
    pub t: i64,
    /// The flip-flop 4Y: /NMI high.
    pub nmi_released: bool,
    /// Every access of the Z80's buses while Some, packed by [`pack_z80`]:
    /// the lockstep gate's tap (the reference's tracer wraps the four callables).
    pub bus_log: Option<Vec<u64>>,
}

/// One Z80 bus access as the gate packs it: `t << 26 | kind << 24 | address << 8
/// | value`, kind 0 a memory read, 1 a memory write, 2 a port read, 3 a port
/// write; `t` the T-state count at the instruction's start.
pub fn pack_z80(kind: u64, address: u16, value: u8, t: i64) -> u64 {
    ((t as u64) << 26) | (kind << 24) | ((address as u64) << 8) | value as u64
}

impl SupervisorBus {
    /// The selected game socket, 0 for channel 1 (CSEL0-3).
    pub fn channel(&self) -> u8 {
        let latch = &self.latch;
        latch[CSEL0] | latch[CSEL1] << 1 | latch[CSEL2] << 2 | latch[CSEL3] << 3
    }

    fn read_memory(&mut self, address: u16) -> u8 {
        let address = address as usize;
        if address < 0x4000 {
            return self.rom[address];
        }
        if address >= 0xE000 {
            // Six lines pulled up; D3 and D4 from the selected channel's PROM,
            // inverted by the buffers (an empty socket's lines float high).
            let mut value = 0xE7;
            if let Some(prom) = self.prom.as_ref() {
                if self.channel() == 0 {
                    value |= (prom.data_out() ^ 1) << 3 | (prom.counter_output() ^ 1) << 4;
                }
            }
            return value;
        }
        if address >= 0xC000 {
            if let Some(inst) = self.inst.as_ref() {
                if self.channel() == 0 {
                    return inst[address & 0x1FFF];
                }
            }
            return 0xFF;
        }
        if address < 0x8000 {
            return 0xFF;
        }
        if address < 0x8800 {
            return self.ram[address & 0x7FF];
        }
        if address < 0x9000 {
            let mask = if self.latch[BUP_KEY] != 0 {
                0x7FF
            } else {
                0x3FF
            };
            return self.ram_8w[address & mask];
        }
        if address < 0x9800 {
            return self.video_ram[address & 0x7FF];
        }
        0xFF
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        let address = address as usize;
        if address >= 0xE000 {
            if self.channel() == 0 {
                if let Some(prom) = self.prom.as_mut() {
                    prom.write(value & 1 == 0, value & 0x08 != 0, value & 0x10 != 0);
                }
            }
        } else if (0x8000..0x8800).contains(&address) {
            self.ram[address & 0x7FF] = value;
        } else if (0x8800..0x9000).contains(&address) {
            let mask = if self.latch[BUP_KEY] != 0 {
                0x7FF
            } else {
                0x3FF
            };
            self.ram_8w[address & mask] = value;
        } else if (0x9000..0x9800).contains(&address) && self.latch[SDCS] == 0 {
            self.video_ram[address & 0x7FF] = value;
        }
    }

    // -- the Z80's ports -------------------------------------------------

    fn read_io(&mut self, port: u16) -> u8 {
        let port = port & 3;
        if port == 0 {
            let mut value = self.buttons & (CHANNEL | ENTER | RESET | SERVICE);
            if !self.int_detect {
                value |= INT_DETECT;
            }
            for (bit, coins) in [COIN_1, COIN_2].into_iter().zip(self.coins.iter_mut()) {
                if !coins.is_empty() {
                    let length = (COIN_SECONDS * Z80_HZ as f64).round() as i64;
                    let now = self.t;
                    if coins[0] + length <= now {
                        coins.retain(|&start| now < start + length);
                    }
                    if coins.iter().any(|&start| start <= now) {
                        value |= bit;
                    }
                }
            }
            return value;
        }
        if port == 3 {
            self.int_detect = false;
            return 0xFF; // nothing drives the bus (MAME returns 0)
        }
        self.dip[port as usize - 1]
    }

    fn write_io(&mut self, port: u16, value: u8) {
        let port = port & 0xFF;
        if self.single_monitor && (0x10..=0x13).contains(&port) {
            self.digits[port as usize - 0x10] = value & 0x0F;
            return;
        }
        let number = (port & 0x0F) as usize;
        let level = value & 1;
        if self.latch[number] == level {
            return;
        }
        self.latch[number] = level;
        if number == NMI_EI && level == 0 {
            self.nmi_released = true; // the flip-flop's preset
        }
        self.on_line(number, level);
    }

    fn on_line(&mut self, number: usize, level: u8) {
        self.lines.push(LineChange {
            number,
            level,
            sdcs: self.latch[SDCS],
        });
    }

    fn log(&mut self, kind: u64, address: u16, value: u8) {
        if let Some(log) = self.bus_log.as_mut() {
            log.push(pack_z80(kind, address, value, self.t));
        }
    }
}

impl Bus for SupervisorBus {
    fn read_byte(&mut self, addr: u16) -> u8 {
        let value = self.read_memory(addr);
        self.log(0, addr, value);
        value
    }
    fn write_byte(&mut self, addr: u16, value: u8) {
        self.log(1, addr, value);
        self.write_memory(addr, value);
    }
    fn read_port(&mut self, addr: u16) -> u8 {
        let value = self.read_io(addr);
        self.log(2, addr, value);
        value
    }
    fn write_port(&mut self, addr: u16, value: u8) {
        self.log(3, addr, value);
        self.write_io(addr, value);
    }
}

/// The Z80 and what it alone touches (module notes). The latch outputs that
/// change are queued in `cpu.bus.lines`; the cabinet ([`PlayChoice`]) owns what
/// the game's lines do.
pub struct Supervisor {
    pub bios: Bios,
    pub cpu: Z80<SupervisorBus>,
    /// The screen's counters start at line 0, dot 0; the first blank begins at
    /// line 240.
    pub next_blank: i64,
    pub blanks: u64,
    pub watchdog: u32,
    pub watchdog_resets: u64,
    pub instructions: u64,
    tiles: HashMap<(u16, u8), Vec<u8>>,
    colours: Vec<[u8; 3]>,
}

impl Supervisor {
    pub fn new(bios: Bios, roms: Option<GameRoms>) -> Supervisor {
        let (inst, prom) = match roms {
            Some(roms) => {
                let prom = roms.prom.map(|data| Rp5h01::new(data, roms.counter_out));
                (Some(roms.inst), prom)
            }
            None => (None, None),
        };
        let bus = SupervisorBus {
            rom: bios.program.clone(),
            single_monitor: bios.single_monitor(),
            inst,
            prom,
            ram: vec![0; 0x800],
            ram_8w: vec![0; 0x800],
            video_ram: vec![0; 0x800],
            latch: [0; 16],
            lines: Vec::new(),
            dip: [0, 0],
            buttons: 0,
            coins: [Vec::new(), Vec::new()],
            int_detect: false,
            digits: [0; 4],
            t: 0,
            nmi_released: true,
            bus_log: None,
        };
        let colours = palette(&bios);
        Supervisor {
            bios,
            cpu: Z80::new(bus),
            next_blank: VBLANK_FROM as i64 * DOTS_PER_LINE * UNITS_PER_DOT,
            blanks: 0,
            watchdog: 0,
            watchdog_resets: 0,
            instructions: 0,
            tiles: HashMap::new(),
            colours,
        }
    }

    pub fn bus(&mut self) -> &mut SupervisorBus {
        &mut self.cpu.bus
    }

    pub fn t(&self) -> i64 {
        self.cpu.bus.t
    }

    pub fn latch(&self) -> &[u8; 16] {
        &self.cpu.bus.latch
    }

    // -- the Z80's memory and ports, as the reference's methods ----------

    pub fn read_byte(&mut self, address: u16) -> u8 {
        self.cpu.bus.read_byte(address)
    }

    pub fn write_byte(&mut self, address: u16, value: u8) {
        self.cpu.bus.write_byte(address, value);
    }

    pub fn read_port(&mut self, port: u16) -> u8 {
        self.cpu.bus.read_port(port)
    }

    pub fn write_port(&mut self, port: u16, value: u8) {
        self.cpu.bus.write_port(port, value);
    }

    // -- the operator and the player -------------------------------------

    /// A coin through slot 0 (Coin 1) or 1: its contact closes at T-state `at`
    /// (default: now) for [`COIN_SECONDS`].
    pub fn insert_coin(&mut self, slot: usize, at: Option<i64>) {
        let bus = &mut self.cpu.bus;
        let at = at.unwrap_or(bus.t);
        bus.coins[slot].push(at);
    }

    // -- running ---------------------------------------------------------

    pub fn power(&mut self) {
        self.reset();
    }

    /// The Z80's /RESET: the CPU, and both latches cleared.
    fn reset(&mut self) {
        let bus = &mut self.cpu.bus;
        for number in 0..16 {
            if bus.latch[number] != 0 {
                bus.latch[number] = 0;
                bus.on_line(number, 0);
            }
        }
        bus.nmi_released = true;
        self.watchdog = 0;
        let cpu = &mut self.cpu;
        cpu.clear_non_maskable_interrupt();
        cpu.request_reset();
        let t_states = cpu.step().expect("the Z80's reset");
        cpu.bus.t += t_states as i64;
        cpu.clear_reset();
    }

    /// One Z80 instruction; returns its T-states.
    pub fn step(&mut self) -> u32 {
        let t_states = self
            .cpu
            .step()
            .unwrap_or_else(|fault| panic!("the supervisor's Z80: {fault}"));
        self.cpu.bus.t += t_states as i64;
        self.instructions += 1;
        if self.cpu.bus.t * UNITS_PER_T >= self.next_blank {
            self.next_blank += FRAME_UNITS;
            self.vertical_blank();
        }
        t_states
    }

    /// The blank flip-flop's rising edge: the NMI and the watchdog.
    fn vertical_blank(&mut self) {
        self.blanks += 1;
        let latch = self.cpu.bus.latch;
        if latch[NMI_EI] != 0 && self.cpu.bus.nmi_released {
            self.cpu.bus.nmi_released = false;
            self.cpu.request_non_maskable_interrupt();
        }
        if latch[NMI_EI] != 0 || latch[DOG_DI] != 0 {
            self.watchdog = 0;
        } else {
            self.watchdog += 1;
            if self.watchdog == WATCHDOG_BLANKS {
                self.watchdog_resets += 1;
                self.reset();
            }
        }
    }

    // -- the screen ------------------------------------------------------

    /// A character's 8 rows of 8 RGB pixels.
    fn tile(&mut self, code: u16, colour: u8) -> &[u8] {
        let (characters, colours) = (&self.bios.characters, &self.colours);
        self.tiles.entry((code, colour)).or_insert_with(|| {
            let [high, middle, low] = characters;
            let base = (colour as usize) << 3;
            let mut rows = Vec::with_capacity(8 * 24);
            for row in 0..8 {
                let index = code as usize * 8 + row;
                let (a, b, c) = (high[index], middle[index], low[index]);
                for bit in (0..8).rev() {
                    let pixel = ((a >> bit) & 1) << 2 | ((b >> bit) & 1) << 1 | ((c >> bit) & 1);
                    rows.extend_from_slice(&colours[base | pixel as usize]);
                }
            }
            rows
        })
    }

    /// SDCS low: the Z80 has the screen's RAM and the shift registers' colour
    /// is held clear.
    pub fn screen_blank(&self) -> bool {
        self.cpu.bus.latch[SDCS] == 0
    }

    /// The screen as packed RGB, 256 x 240 (the game screen's size): the
    /// counters' lines 0 to 239, of which only `lines` (first, end) are outside
    /// the vertical blank and drawn; the rest, and everything while SDCS is
    /// low, is black. `(0, 240)` draws all thirty rows, as MAME does
    /// ([`Supervisor::picture`] passes `(VBLANK_TO, VBLANK_FROM)`).
    pub fn picture_lines(&mut self, lines: (usize, usize)) -> Vec<u8> {
        let mut out = vec![0u8; WIDTH * HEIGHT * 3];
        if self.screen_blank() {
            return out;
        }
        let (first, end) = lines;
        for line in 0..HEIGHT {
            if !(first <= line && line < end) {
                continue;
            }
            let (row, y) = (line >> 3, (line & 7) * 24);
            let base = row * 64;
            for column in 0..32 {
                let low = self.cpu.bus.video_ram[base + column * 2];
                let high = self.cpu.bus.video_ram[base + column * 2 + 1];
                let tile = self.tile(low as u16 | ((high as u16 & 7) << 8), high >> 3);
                let at = (line * WIDTH + column * 8) * 3;
                out[at..at + 24].copy_from_slice(&tile[y..y + 24]);
            }
        }
        out
    }

    pub fn picture(&mut self) -> Vec<u8> {
        self.picture_lines((VBLANK_TO, VBLANK_FROM))
    }
}

/// The 256 colours of (colour, pixel): the PROMs' inverted bits through the
/// resistor weights (MAME's `playch10_palette`).
fn palette(bios: &Bios) -> Vec<[u8; 3]> {
    let weights: [u32; 4] = [0x0E, 0x1F, 0x43, 0x8F];
    let mut colours = Vec::with_capacity(256);
    for index in 0..256 {
        let mut channels = [0u8; 3];
        for (channel, prom) in channels.iter_mut().zip(&bios.proms) {
            let bits = !prom[index] & 0x0F;
            let sum: u32 = weights
                .iter()
                .enumerate()
                .filter(|(n, _)| (bits >> n) & 1 != 0)
                .map(|(_, w)| w)
                .sum();
            *channel = sum as u8;
        }
        colours.push(channels);
    }
    colours
}

/// The game screen while /DISP MASK is low: the RGB lines are switched off.
pub const MASKED_PIXEL: u16 = 0x0F;

/// A PlayChoice-10 with one game in channel 1: `game` (a System on the
/// `playchoice` row) under `supervisor` (module notes).
pub struct PlayChoice {
    pub machine: Machine,
    pub game: System,
    /// The 2A03 is held in reset (/GAME RES low), or stopped by its pin 30.
    pub in_reset: bool,
    pub stopped: bool,
    pub nmi: bool,
    /// A one-monitor cabinet's monitor is on the game (see `line`).
    pub monitor_on_game: bool,
    pub supervisor: Supervisor,
    palette: Vec<[u8; 3]>,
}

impl PlayChoice {
    /// The cabinet around `loaded` (identified for the `playchoice` row), with
    /// `bios`, on `machine` (default the `playchoice` row).
    pub fn new(loaded: Loaded, bios: Bios, machine: Option<Machine>) -> PlayChoice {
        let machine = machine.unwrap_or_else(Machine::playchoice);
        let Loaded {
            cartridge,
            image,
            source,
            disagreements,
        } = loaded;
        let roms = game_roms(&image);
        let mut game = System::new(cartridge, machine.clone());
        game.image = Some(image);
        game.source = source;
        game.disagreements = disagreements;
        game.plug(None);
        let palette = rgb_palette(machine.timing.palette);
        let mut cabinet = PlayChoice {
            machine,
            game,
            in_reset: true,
            stopped: false,
            nmi: false,
            monitor_on_game: false,
            supervisor: Supervisor::new(bios, roms),
            palette,
        };
        cabinet.mask_controls(true);
        cabinet.game.cpu.bus.ppu.reset(); // /PPU RES is low out of the Z80's reset
        cabinet.supervisor.power();
        cabinet.apply_lines();
        cabinet
    }

    /// The cabinet with the PlayChoice-10 image `data` in channel 1 and the
    /// BIOS `bios` (default: [`load_bios`]'s dual-monitor one).
    pub fn from_rom(
        data: &[u8],
        bios: Option<Bios>,
        machine: Option<Machine>,
    ) -> Result<PlayChoice, String> {
        let ticks = Machine::playchoice().timing.cpu_divider;
        let loaded = cartridge::load(data, ticks, None, true)?;
        if !is_playchoice(&loaded.image) {
            return Err("not a PlayChoice-10 image".into());
        }
        let bios = match bios {
            Some(bios) => bios,
            None => load_bios("dual", None)?,
        };
        Ok(PlayChoice::new(loaded, bios, machine))
    }

    // -- the lines from the supervisor's latches -------------------------

    fn mask_controls(&mut self, masked: bool) {
        for pad in self.game.cpu.bus.pads.iter_mut() {
            let any = pad.as_any_mut();
            if let Some(pad) = any.downcast_mut::<PlayChoicePad>() {
                pad.masked = masked;
            } else if let Some(gun) = any.downcast_mut::<PlayChoiceGun>() {
                gun.set_masked(masked);
            }
        }
    }

    /// The queued latch changes, in order (the reference's `_line` for each).
    fn apply_lines(&mut self) {
        if self.supervisor.cpu.bus.lines.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.supervisor.cpu.bus.lines);
        for change in lines {
            self.line(change);
        }
    }

    fn line(&mut self, change: LineChange) {
        let LineChange {
            number,
            level,
            sdcs,
        } = change;
        let game = &mut self.game;
        if number == GAME_RES {
            let board = &mut game.cpu.bus;
            if level != 0 {
                // Released: the 2A03 runs its reset sequence. The APU was
                // reset with it, and the game board saw M2 stop.
                board.apu.reset();
                let t = board.t;
                board.ppu.cart.m2_gap(t);
                game.cpu.frame.request_reset();
                self.in_reset = false;
            } else {
                self.in_reset = true;
                self.stopped = false; // /GAME RES presets the stop flip-flops
                board.apu.reset();
            }
        } else if number == PPU_RES {
            game.cpu.bus.ppu.reset(); // held while low; the power-on window from its release
        } else if number == CNTRL_MASK {
            self.mask_controls(level == 0);
        } else if number == DISP_MASK && level == 0 && sdcs != 0 {
            // The one-monitor board (PCK1; no schematic here): MAME's driver
            // flips its single screen between the supervisor's picture and
            // the game's each time /DISP MASK falls. Emulator-derived, and
            // kept out of the Z80's reset (which clears the latch with SDCS).
            self.monitor_on_game = !self.monitor_on_game;
        }
    }

    /// The supervisor's `write_port` as the reference's tests call it: the
    /// latch, and what its line does at once.
    pub fn supervisor_write_port(&mut self, port: u16, value: u8) {
        self.supervisor.write_port(port, value);
        self.apply_lines();
    }

    /// The supervisor's `read_port`.
    pub fn supervisor_read_port(&mut self, port: u16) -> u8 {
        self.supervisor.read_port(port)
    }

    // -- running ---------------------------------------------------------

    /// One Z80 instruction, and what the latch changes it made do.
    pub fn supervisor_step(&mut self) {
        self.supervisor.step();
        self.apply_lines();
    }

    fn game_step(&mut self) {
        let game = &mut self.game;
        if self.in_reset {
            game.cpu.bus.idle_cycle();
        } else if self.stopped {
            let cpu = &mut game.cpu;
            let pc = cpu.pc;
            cpu.bus.read_byte(pc, &mut cpu.frame); // RDY low: the 2A03 repeats its read
        } else {
            game.cpu_step();
            game.instructions += 1;
        }
        let nmi = game.cpu.bus.ppu.nmi;
        if nmi != self.nmi {
            self.nmi = nmi;
            if nmi {
                // The PPU's /INT fell: INT DETECT sets, and the stop flip-flop
                // takes /GAME STOP (unless /GAME RES holds it preset).
                self.supervisor.cpu.bus.int_detect = true;
                if !self.in_reset {
                    self.stopped = self.supervisor.cpu.bus.latch[GAME_STOP] == 0;
                }
            }
        }
    }

    /// Whether the Z80 is the side whose clock is behind: T-states x 945
    /// against master ticks x 176.
    #[inline]
    fn z80_behind(&self) -> bool {
        self.supervisor.cpu.bus.t * 945 <= self.game.cpu.bus.t * 176
    }

    /// One instruction (or one idle cycle of a held 2A03) on the side whose
    /// clock is behind.
    pub fn step(&mut self) {
        if self.z80_behind() {
            self.supervisor_step();
        } else {
            self.game_step();
        }
    }

    /// Run until the game's PPU has started `frames` more frames.
    pub fn run_frames(&mut self, frames: u64) {
        let target = self.game.cpu.bus.ppu.frame + frames;
        while self.game.cpu.bus.ppu.frame < target {
            if self.z80_behind() {
                self.supervisor_step();
            } else {
                self.game_step();
            }
        }
    }

    /// Run for `seconds` of the Z80's clock.
    pub fn run_seconds(&mut self, seconds: f64) {
        let end = self.supervisor.cpu.bus.t + (seconds * Z80_HZ as f64).round() as i64;
        while self.supervisor.cpu.bus.t < end {
            if self.z80_behind() {
                self.supervisor_step();
            } else {
                self.game_step();
            }
        }
    }

    // -- the cabinet -----------------------------------------------------

    pub fn insert_coin(&mut self, slot: usize) {
        self.supervisor.insert_coin(slot, None);
    }

    /// Hold a cabinet button (CHANNEL, ENTER, RESET, SERVICE) for `seconds`,
    /// running meanwhile.
    pub fn press(&mut self, button: u8, seconds: f64) {
        self.supervisor.cpu.bus.buttons |= button;
        self.run_seconds(seconds);
        self.supervisor.cpu.bus.buttons &= !button;
    }

    /// /DISP MASK is high: the game's picture reaches its monitor.
    pub fn game_shown(&self) -> bool {
        self.supervisor.cpu.bus.latch[DISP_MASK] != 0
    }

    pub fn sound_on(&self) -> bool {
        self.supervisor.cpu.bus.latch[SOUND_MASK] != 0
    }

    /// The game monitor's picture, as the PPU's pixel values: black while
    /// /DISP MASK is low.
    pub fn game_picture(&self) -> Vec<u16> {
        if self.game_shown() {
            return self.game.cpu.bus.ppu.picture.clone();
        }
        vec![MASKED_PIXEL; WIDTH * HEIGHT]
    }

    /// The game monitor's picture as packed RGB (the RP2C03's colours).
    pub fn game_rgb(&self) -> Vec<u8> {
        crate::video::to_rgb(&self.game_picture(), &self.palette)
    }

    /// The game's sound, silent while /SOUND MASK is low.
    pub fn take_samples(&mut self) -> Vec<f64> {
        let samples = self.game.take_samples();
        if !self.sound_on() {
            return vec![0.0; samples.len()];
        }
        samples
    }
}
