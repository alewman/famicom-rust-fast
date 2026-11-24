// SPDX-License-Identifier: MIT
//! A whole console: 6502-rust's RP2A03 on a [`Board`] (`system.py`).

use crate::board::Board;
use crate::cartridge::{self, BoardInfo, Loaded, RomImage};
use crate::joypad::START;
use crate::machine::Machine;
use crate::peripherals;
use crate::vs::{VsStick, VsZapper, VS_DEVICES, VS_ZAPPER};
use sixfiveohtwo::Mos6502;
use std::path::Path;

/// Machine + cartridge, powered on.
pub struct System {
    pub cpu: Mos6502<Board>,
    pub instructions: u64,
    /// How [`System::from_rom`] identified the cartridge.
    pub image: Option<RomImage>,
    pub source: &'static str,
    pub disagreements: Vec<String>,
}

impl System {
    pub fn new(cartridge: Box<dyn cartridge::Cartridge>, machine: Machine) -> System {
        let ane = machine.ane_constant;
        let lxa = machine.lxa_constant;
        let board = Board::new(machine, cartridge);
        let mut cpu = Mos6502::rp2a03(board);
        cpu.ane_constant = ane;
        cpu.lxa_constant = lxa;
        let mut system = System {
            cpu,
            instructions: 0,
            image: None,
            source: "",
            disagreements: Vec::new(),
        };
        system.power();
        system
    }

    /// The console a cartridge asks for: a Vs. UniSystem with the game's PPU
    /// if the NES 2.0 header or nes20db says console type 1 (and which PPU:
    /// machine::VS_PPUS); else the NES 2.0 header's or nes20db's timing (1
    /// PAL, 3 Dendy), else a Famicom (NES-only expansion devices pick the
    /// NES). The reference's PlayChoice-10 row (console type 2) is not carried.
    pub fn machine_for(data: &[u8]) -> Result<Machine, String> {
        let loaded = cartridge::load(data, 12, None, true)?;
        let board = &loaded.image.board;
        if board.console == 1 {
            return Ok(Machine::vs_machine(board.vs_ppu));
        }
        Ok(match board.timing {
            1 => Machine::pal_nes(),
            3 => Machine::dendy(),
            _ if crate::peripherals_nes_only(board.expansion) => Machine::nes_001(),
            _ => Machine::famicom(),
        })
    }

    /// Load `data` (identified as [`cartridge::load`] does, or as `override_board`).
    pub fn from_rom(
        data: &[u8],
        machine: Machine,
        override_board: Option<BoardInfo>,
    ) -> Result<System, String> {
        let Loaded {
            cartridge,
            image,
            source,
            disagreements,
        } = cartridge::load(data, machine.timing.cpu_divider, override_board, true)?;
        let mut system = System::new(cartridge, machine);
        let vs_hardware = image.board.vs_hardware;
        system.image = Some(image);
        system.source = source;
        system.disagreements = disagreements;
        if system.cpu.bus.vs.is_some() && matches!(vs_hardware, 4 | 6) {
            // Vs. Ice Climber (Japan) and Vs. Raid on Bungeling Bay: the fourth
            // bit of both sticks' reports is wired on (vs::VsStick).
            for pad in system.cpu.bus.pads.iter_mut() {
                if let Some(pad) = pad.as_any_mut().downcast_mut::<VsStick>() {
                    pad.jumper = START;
                }
            }
        }
        Ok(system)
    }

    pub fn from_path(path: &Path, machine: Machine) -> Result<System, String> {
        let data = cartridge::read_rom_file(path, cartridge::ROM_SUFFIXES)?;
        System::from_rom(&data, machine, None)
    }

    pub fn board(&mut self) -> &mut Board {
        &mut self.cpu.bus
    }

    /// Put in the devices of NES 2.0 expansion number `device` (None: the
    /// cartridge's, from its header or nes20db); where the one to drive is,
    /// as [`peripherals::plug`] returns it.
    pub fn plug(&mut self, device: Option<u8>) -> peripherals::Plugged {
        let device = match device {
            Some(device) => device,
            None => self.image.as_ref().map_or(0, |image| image.board.expansion),
        };
        let board = &mut self.cpu.bus;
        if board.vs.is_some() {
            // A Vs. cabinet's controls are its own: the two sticks are on the
            // mainboard already (vs::panel says whose is whose), and its light
            // gun reports through $4016's shift register.
            if device == VS_ZAPPER {
                let palette = peripherals::palette_of(board);
                board.pads[0] = Box::new(VsZapper::new(&board.ppu, &palette));
                return peripherals::Plugged::Pad(0);
            }
            return peripherals::Plugged::None;
        }
        if VS_DEVICES.contains(&device) {
            return peripherals::Plugged::None; // a Vs. game on a home console: standard pads
        }
        let famicom = board.machine.famicom_expansion_port;
        peripherals::plug(board, device, famicom, None)
    }

    // -- power and reset ---------------------------------------------------

    /// Switch on: registers from the machine's power-on parameters, then the reset sequence.
    pub fn power(&mut self) {
        let power = self.cpu.bus.machine.power_on.clone();
        self.cpu.a = power.a;
        self.cpu.x = power.x;
        self.cpu.y = power.y;
        self.cpu.s = 0;
        self.cpu.bus.power();
        self.cpu.frame.request_reset();
    }

    /// The reset button.
    pub fn reset(&mut self) {
        self.cpu.bus.reset();
        self.cpu.frame.request_reset();
    }

    pub fn take_samples(&mut self) -> Vec<f64> {
        self.cpu.bus.take_samples()
    }

    /// Drop a coin into a Vs. System's coin acceptor 0 or 1 (vs::VsMainboard).
    pub fn insert_coin(&mut self, acceptor: usize) -> Result<(), String> {
        let board = &mut self.cpu.bus;
        let now = board.t;
        match board.vs.as_mut() {
            None => Err(format!("the {} takes no coins", board.machine.name)),
            Some(vs) => {
                vs.insert_coin(acceptor, now);
                Ok(())
            }
        }
    }

    // -- running -----------------------------------------------------------

    pub fn cycles(&self) -> i64 {
        self.cpu.bus.cycles()
    }

    #[inline]
    pub(crate) fn cpu_step(&mut self) {
        if self.cpu.bus.machine.dma_halts_on_opcode {
            self.cpu.bus.opcode_read = true;
        }
        self.cpu.step();
    }

    /// One instruction, interrupt entry or reset sequence; returns CPU cycles, DMA included.
    pub fn step(&mut self) -> i64 {
        let before = self.cpu.bus.t;
        self.cpu_step();
        self.instructions += 1;
        (self.cpu.bus.t - before) / self.cpu.bus.cycle_ticks
    }

    /// Run whole instructions until at least `cycles` more CPU cycles have passed.
    pub fn run_cycles(&mut self, cycles: i64) {
        let end = self.cpu.bus.t + cycles * self.cpu.bus.cycle_ticks;
        while self.cpu.bus.t < end {
            self.cpu_step();
            self.instructions += 1;
        }
    }

    /// Run until the PPU has started `frames` more frames.
    pub fn run_frames(&mut self, frames: u64) {
        let target = self.cpu.bus.ppu.frame + frames;
        while self.cpu.bus.ppu.frame < target {
            self.cpu_step();
            self.instructions += 1;
        }
    }

    /// Run until `done()` (checked every `check_every` instructions); false on timeout.
    pub fn run_until(
        &mut self,
        mut done: impl FnMut(&mut System) -> bool,
        max_cycles: i64,
        check_every: u64,
    ) -> bool {
        let end = self.cpu.bus.t + max_cycles * self.cpu.bus.cycle_ticks;
        while self.cpu.bus.t < end {
            for _ in 0..check_every {
                self.cpu_step();
            }
            self.instructions += check_every;
            if done(self) {
                return true;
            }
        }
        false
    }
}
