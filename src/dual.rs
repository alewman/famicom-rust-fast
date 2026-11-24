// SPDX-License-Identifier: MIT
//! The Vs. DualSystem: both sides of the mainboard running, on one clock
//! (`dual.py`).
//!
//! Vs._System.wiki (specification tier): "The Vs. DualSystem has a main CPU, main
//! PPU, sub CPU, and sub PPU. All four are fed by the same clock, and all four are
//! released from reset by the watchdog at the same time. The RGB PPUs ... never
//! have the missing dot ... so it is never possible for the two PPUs to not be
//! anything but perfectly synchronized." Each side is a whole [`System`] on a Vs.
//! row of `machine.rs`, and this module is the wiring between the two:
//!
//! - **The shared RAM** at $6000-$7FFF: one [`SharedRam`]. "On the primary CPU
//!   only, [OUT1] controls which CPU can access [it]. When 1: the primary CPU
//!   has access and the secondary CPU sees open bus. When 0: the secondary CPU
//!   has access." OUT1 is 0 out of reset, so the secondary side starts with it.
//! - **The cross interrupt**: each side's OUT1 is the other side's /IRQ, "when 0:
//!   asserts an IRQ request. When 1: releases" it. Both are asserted out of reset
//!   until each program writes a 1.
//! - **The watchdog**: "the secondary CPU ... must be instructed to read from
//!   $4017 at least every 1.2 seconds, or else the watchdog timer will reset both
//!   CPUs, both PPUs, and both CPUs' bit at $4020". MAME measured "between 1.23s
//!   and 1.33s in hardware tests, exact timing unknown" and uses 1.3 s
//!   (vsnes.cpp), as this does.
//!
//! **One clock, an instruction at a time.** 6502-rust runs whole instructions,
//! so the two CPUs are interleaved by instruction: the side whose clock is behind
//! steps next (the primary on a tie). A write on one side therefore reaches the
//! other within one instruction's length, at most 7 CPU cycles plus any DMA, never
//! on the exact cycle. MAME interleaves the same way (`set_perfect_quantum`).
//! The PPUs need no such care: each is exact on its own side, and the two frames
//! begin on the same master tick.
//!
//! **The image** (NES_2.0.wiki "PRG-ROM Area", "CHR-ROM Area"): "the first half
//! block of the specified PRG-ROM size belongs to the first unit, and the second
//! half block ... to the second unit"; 32 KiB of CHR are 16 KiB each, and with 16
//! KiB or less "both units use the same" CHR. The first unit is the primary side
//! (MAME's main CPU, sockets 1A-1D and 2A-2B). *Vs. Raid on Bungeling Bay*
//! (hardware type 6) is the exception both ways: 32 KiB for the first unit and 8
//! KiB for the second, at $E000, and its first unit, the game, is the *secondary*
//! side: it expects $4016 D7 = 1, and MAME's set names its EPROMs for sockets
//! 6A-6D and the 8 KiB one for 1A.
//!
//! Differences forced by Rust (see also `vs.rs`): the reference's two sides
//! share one `SharedRAM` object and call each other's `set_external_irq`. Here
//! each side's mainboard holds its own [`SharedRam`] and leaves the level for
//! the other side's /IRQ in `pending_irq`; after every instruction,
//! [`VsDualSystem::wire`] passes the level on and makes the two RAMs one: the
//! owner is the primary side's (only its OUT1 moves it), and the bytes go to
//! whichever side owns them. The side not running cannot look in between, so
//! both sides see what they would see sharing one object. The reference's
//! bus observer on the secondary side is the board's `vs.fed`. The image's
//! cartridges are built by the loader (`cartridge::build`) where the reference
//! repeats the whole image's class: the same class for every DualSystem image,
//! all of them mapper 99.

use crate::cartridge::{self, ines, BoardInfo, Loaded, RomImage};
use crate::joypad::START;
use crate::machine::Machine;
use crate::system::System;
use crate::vs::{SharedRam, Side, VsStick};
use std::path::Path;

pub const WATCHDOG_SECONDS: f64 = 1.3;

/// NES 2.0 Vs. hardware types that are DualSystem boards.
pub const DUAL_HARDWARE: [u8; 2] = [5, 6];

/// Whether `data` is a Vs. DualSystem image (by its header or nes20db).
pub fn is_dual(data: &[u8]) -> Result<bool, String> {
    let board = cartridge::load(data, 12, None, true)?.image.board;
    Ok(board.console == 1 && DUAL_HARDWARE.contains(&board.vs_hardware))
}

/// The (primary, secondary) sides' images of a DualSystem image (module notes).
pub fn split(image: &RomImage) -> (RomImage, RomImage) {
    let (prg, chr) = (&image.prg, &image.chr);
    let (first_prg, second_prg) = if image.board.vs_hardware == 6 && prg.len() == 0xA000 {
        (prg[..0x8000].to_vec(), prg[0x8000..].to_vec())
    } else {
        let half = prg.len() / 2;
        (prg[..half].to_vec(), prg[half..].to_vec())
    };
    let (first_chr, second_chr) = if chr.len() == 0x8000 {
        (chr[..0x4000].to_vec(), chr[0x4000..].to_vec())
    } else {
        (chr.clone(), chr.clone())
    };
    let first = RomImage {
        prg: first_prg,
        chr: first_chr,
        ..image.clone()
    };
    let second = RomImage {
        prg: second_prg,
        chr: second_chr,
        ..image.clone()
    };
    if image.board.vs_hardware == 6 {
        return (second, first); // the game is the secondary side's
    }
    (first, second)
}

/// A DualSystem cabinet: `primary` and `secondary`, each a System.
pub struct VsDualSystem {
    pub machine: Machine,
    pub primary: System,
    pub secondary: System,
    pub watchdog_resets: u64,
    watchdog_ticks: i64,
    /// The side whose mainboard holds the shared RAM's bytes (module notes).
    holder: Side,
}

impl VsDualSystem {
    pub fn new(primary: Loaded, secondary: Loaded, machine: Machine) -> VsDualSystem {
        let side = |loaded: Loaded, vs_side: &'static str| {
            let mut system = System::new(
                loaded.cartridge,
                Machine {
                    vs_side: Some(vs_side),
                    ..machine.clone()
                },
            );
            system.image = Some(loaded.image);
            system.source = loaded.source;
            system.disagreements = loaded.disagreements;
            system
        };
        let primary = side(primary, "primary");
        let secondary = side(secondary, "secondary");
        let watchdog_ticks = (WATCHDOG_SECONDS * machine.timing.master_hz).round_ties_even() as i64;
        let mut dual = VsDualSystem {
            machine,
            primary,
            secondary,
            watchdog_resets: 0,
            watchdog_ticks,
            holder: Side::Secondary,
        };
        for system in [&mut dual.primary, &mut dual.secondary] {
            let vs = system.cpu.bus.vs.as_mut().expect("a Vs. row");
            vs.ram = SharedRam::new(Side::Secondary, true);
            vs.irq_wired = true;
        }
        dual.out_of_reset();
        dual
    }

    /// Both sides of the DualSystem image `data`, with the PPUs its header or
    /// nes20db names.
    pub fn from_rom(data: &[u8]) -> Result<VsDualSystem, String> {
        let whole = cartridge::load(data, 12, None, true)?;
        let board: BoardInfo = whole.image.board.clone();
        if board.console != 1 || !DUAL_HARDWARE.contains(&board.vs_hardware) {
            return Err("not a Vs. DualSystem image".into());
        }
        let image = RomImage {
            board: board.clone(),
            ..ines::parse(data)?
        };
        let (first, second) = split(&image);
        let loaded = |side: RomImage| -> Result<Loaded, String> {
            Ok(Loaded {
                cartridge: cartridge::build(side.clone(), 12)?,
                image: side,
                source: whole.source,
                disagreements: whole.disagreements.clone(),
            })
        };
        let (first, second) = (loaded(first)?, loaded(second)?);
        let mut system = VsDualSystem::new(first, second, Machine::vs_machine(board.vs_ppu));
        if board.vs_hardware == 6 {
            // Raid on Bungeling Bay's jumpers (vs::VsStick)
            for side in [&mut system.primary, &mut system.secondary] {
                for pad in side.cpu.bus.pads.iter_mut() {
                    if let Some(pad) = pad.as_any_mut().downcast_mut::<VsStick>() {
                        pad.jumper = START;
                    }
                }
            }
        }
        Ok(system)
    }

    pub fn from_path(path: &Path) -> Result<VsDualSystem, String> {
        VsDualSystem::from_rom(&cartridge::read_rom_file(path, cartridge::ROM_SUFFIXES)?)
    }

    /// Side 0 (the primary) or 1.
    pub fn side(&mut self, side: usize) -> &mut System {
        if side == 0 {
            &mut self.primary
        } else {
            &mut self.secondary
        }
    }

    // -- the wiring --------------------------------------------------------

    /// Both OUT1s are 0: each side requests the other's interrupt, and the
    /// secondary side has the RAM.
    fn out_of_reset(&mut self) {
        for side in [&mut self.primary, &mut self.secondary] {
            side.cpu.bus.set_external_irq(true);
        }
        self.own(Side::Secondary);
        let t = self.secondary.cpu.bus.t;
        self.secondary.cpu.bus.vs.as_mut().unwrap().fed = t;
    }

    /// The shared RAM is `owner`'s on both sides, its bytes with it.
    fn own(&mut self, owner: Side) {
        let first = self.primary.cpu.bus.vs.as_mut().unwrap();
        let second = self.secondary.cpu.bus.vs.as_mut().unwrap();
        first.ram.owner = owner;
        second.ram.owner = owner;
        if owner != self.holder {
            std::mem::swap(&mut first.ram.data, &mut second.ram.data);
            self.holder = owner;
        }
    }

    /// After an instruction on either side: each OUT1 that moved reaches the
    /// other side's /IRQ, and the RAM goes where the primary side's OUT1 put it.
    fn wire(&mut self) {
        let first = self.primary.cpu.bus.vs.as_mut().unwrap();
        let to_second = first.pending_irq.take();
        let owner = first.ram.owner;
        let second = self.secondary.cpu.bus.vs.as_mut().unwrap();
        let to_first = second.pending_irq.take();
        if let Some(level) = to_second {
            self.secondary.cpu.bus.set_external_irq(level);
        }
        if let Some(level) = to_first {
            self.primary.cpu.bus.set_external_irq(level);
        }
        if owner != self.holder || self.secondary.cpu.bus.vs.as_ref().unwrap().ram.owner != owner {
            self.own(owner);
        }
    }

    /// The watchdog's reset (there is no button): both CPUs, both PPUs.
    pub fn reset(&mut self) {
        self.primary.reset();
        self.secondary.reset();
        self.out_of_reset();
    }

    /// A coin into side 0's (the primary's) or side 1's acceptor 0 or 1.
    pub fn insert_coin(&mut self, side: usize, acceptor: usize) -> Result<(), String> {
        self.side(side).insert_coin(acceptor)
    }

    // -- running -----------------------------------------------------------

    /// One instruction on the side whose clock is behind.
    pub fn step(&mut self) {
        if self.primary.cpu.bus.t <= self.secondary.cpu.bus.t {
            self.primary.step();
        } else {
            self.secondary.step();
        }
        self.wire();
    }

    /// Run until both PPUs have started `frames` more frames.
    pub fn run_frames(&mut self, frames: u64) {
        let target_1 = self.primary.cpu.bus.ppu.frame + frames;
        let target_2 = self.secondary.cpu.bus.ppu.frame + frames;
        let (mut count_1, mut count_2) = (0u64, 0u64);
        let limit = self.watchdog_ticks;
        while self.primary.cpu.bus.ppu.frame < target_1
            || self.secondary.cpu.bus.ppu.frame < target_2
        {
            if self.primary.cpu.bus.t <= self.secondary.cpu.bus.t {
                self.primary.cpu_step();
                count_1 += 1;
                self.wire();
            } else {
                self.secondary.cpu_step();
                count_2 += 1;
                self.wire();
                let board = &self.secondary.cpu.bus;
                if board.t - board.vs.as_ref().unwrap().fed > limit {
                    self.watchdog_resets += 1;
                    self.reset();
                }
            }
        }
        self.primary.instructions += count_1;
        self.secondary.instructions += count_2;
    }

    /// The two screens, the primary side's first.
    pub fn pictures(&self) -> (&[u16], &[u16]) {
        (
            &self.primary.cpu.bus.ppu.picture,
            &self.secondary.cpu.bus.ppu.picture,
        )
    }
}
