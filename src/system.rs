// SPDX-License-Identifier: MIT
//! A whole console: 6502-rust's RP2A03 on a [`Board`] (`system.py`).

use crate::board::Board;
use crate::cartridge::{self, BoardInfo, Loaded, RomImage};
use crate::machine::Machine;
use crate::peripherals;
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

    /// The console a cartridge asks for: the NES 2.0 header's or nes20db's
    /// timing (1 PAL, 3 Dendy), else a Famicom (NES-only expansion devices
    /// pick the NES).
    pub fn machine_for(data: &[u8]) -> Result<Machine, String> {
        let loaded = cartridge::load(data, 12, None, true)?;
        let board = &loaded.image.board;
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
        system.image = Some(image);
        system.source = source;
        system.disagreements = disagreements;
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
        let famicom = self.cpu.bus.machine.famicom_expansion_port;
        peripherals::plug(&mut self.cpu.bus, device, famicom, None)
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

    // -- running -----------------------------------------------------------

    pub fn cycles(&self) -> i64 {
        self.cpu.bus.cycles()
    }

    #[inline]
    fn cpu_step(&mut self) {
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
