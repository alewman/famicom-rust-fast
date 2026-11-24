// SPDX-License-Identifier: MIT
//! The console board: the CPU bus, open bus, the ports, DMA, and the one
//! clock (`board.py`; its module notes give the design).
//!
//! The board is the 6502's [`Bus`]: each call is one CPU cycle, which moves
//! the clock on by `cpu_divider` ticks, runs the PPU's dots up to the access,
//! clocks the APU once, performs the access, and updates the CPU's interrupt
//! inputs as they stand at the cycle's end. DMA (OAM and DMC) runs inside
//! the CPU's stalled read.
//!
//! Differences forced by Rust: the reference's `cpu` and its interrupt
//! callables are reached through the [`Frame`] each bus call receives; the
//! cartridge is owned by the PPU (`ppu.cart`); the bus observers are one
//! optional log of packed cycles ([`Board::bus_log`]), the lockstep gate's
//! tap. The reference's Vs. mainboard is both its `vs` and its `expansion`;
//! here it is [`Board::vs`] alone, called where the expansion device would be
//! while no other device is plugged there (as the reference's `expansion` is
//! the mainboard until something replaces it), and saved in the expansion's
//! place in a state.

use crate::apu::{AnalogueStage, Apu};
use crate::cartridge::Cartridge;
use crate::fds_auto::AutoDisk;
use crate::joypad::StandardController;
use crate::machine::Machine;
use crate::peripherals::{ExpansionDevice, PortDevice};
use crate::ppu::Ppu;
use crate::vs::{Side, VsMainboard, VsStick};
use sixfiveohtwo::{Bus, Frame};

/// When the PPU sees a register read or write: two dots into its CPU cycle.
pub const PPU_ACCESS_TICKS: i64 = 8;

/// One CPU-bus cycle as the observers see it, packed as the gate's
/// reference script packs it: `t << 25 | write << 24 | address << 8 | value`.
pub fn pack_cycle(write: bool, address: u16, value: u8, t: i64) -> u64 {
    ((t as u64) << 25) | ((write as u64) << 24) | ((address as u64) << 8) | value as u64
}

pub struct Board {
    pub machine: Machine,
    pub cycle_ticks: i64,
    pub ram: [u8; 0x800],
    pub ppu: Ppu,
    pub apu: Apu,
    pub analogue: AnalogueStage,
    pub pads: [Box<dyn PortDevice>; 2],
    pub expansion: Option<Box<dyn ExpansionDevice>>,
    /// A Vs. System mainboard (vs.rs): it stands where the expansion-port
    /// device does, drives every line of $4016/$4017, and answers
    /// $4020-$7FFF before the ROM sockets do. None on any other console.
    pub vs: Option<Box<VsMainboard>>,
    pub microphone: bool,
    has_microphone: bool,
    open_lines: [u8; 2],
    /// Master ticks at the end of the last CPU cycle.
    pub t: i64,
    pub data_bus: u8,
    pub internal_bus: u8,
    undriven: bool,
    pad_oe: i32,
    oe_per_cycle: bool,
    oam_dma_page: Option<u8>,
    halts_on_opcode: bool,
    pub opcode_read: bool,
    strobe: Option<u8>,
    irq: bool,
    nmi: bool,
    /// One more source on the wired-OR /IRQ line: the other side's OUT1 on
    /// a Vs. DualSystem (dual.rs). False on every other machine.
    pub external_irq: bool,
    ppu_access: i64,
    snoop_reads: bool,
    latches_reads: bool,
    counts_m2: bool,
    route_audio: bool,
    /// Every CPU-bus cycle (not the DMA's), packed by [`pack_cycle`], while Some.
    pub bus_log: Option<Vec<u64>>,
    /// Disk changes as the program asks for them, while Some (`fds_auto.py`'s
    /// observer): called after each CPU-bus cycle, after `bus_log`.
    pub auto_disk: Option<Box<AutoDisk>>,
}

impl Board {
    pub fn new(machine: Machine, cartridge: Box<dyn Cartridge>) -> Board {
        let power = machine.power_on.clone();
        let timing = machine.timing.clone();
        let snoop_reads = cartridge.snoops_cpu_reads();
        let latches_reads = cartridge.latches_reads();
        let counts_m2 = cartridge.counts_m2();
        let route_audio = machine.famicom_audio_through_cart && cartridge.expansion_audio();
        let mut cartridge = cartridge;
        if route_audio {
            cartridge.connect_audio();
        }
        let mut ppu = Ppu::new(timing.clone(), cartridge, machine.ppu_latch_decay_seconds);
        ppu.palette = power.palette;
        ppu.status = power.ppustatus & 0xE0;
        ppu.dot = power.ppu_dot;
        ppu.time = power.ppu_ticks;
        let mut apu = Apu::new(
            power.apu_put_first,
            44100,
            machine.apu_tables,
            machine.dmc_unexpected_reload_bug,
            machine.dmc_late_stop,
        );
        apu.dmc.timer = power.dmc_timer;
        apu.set_cpu_rate(timing.master_hz / timing.cpu_divider as f64);
        let analogue = AnalogueStage::new(
            apu.sample_rate(),
            &machine.audio_high_pass_hz,
            machine.audio_low_pass_hz,
        );
        let mut pads: [Box<dyn PortDevice>; 2] = [
            Box::new(StandardController::default()),
            Box::new(StandardController::default()),
        ];
        let mut open_lines = [0xE0 | machine.pad_open_bus.0, 0xE0 | machine.pad_open_bus.1];
        let mut vs = None;
        if let Some(side) = machine.vs_side {
            vs = Some(Box::new(VsMainboard::new(
                Side::from_name(side),
                timing.master_hz,
                None,
            )));
            pads = [Box::new(VsStick::default()), Box::new(VsStick::default())];
            open_lines = [0, 0];
        }
        Board {
            cycle_ticks: timing.cpu_divider,
            ram: [power.ram_fill; 0x800],
            ppu,
            apu,
            analogue,
            pads,
            expansion: None,
            vs,
            microphone: false,
            has_microphone: machine.microphone,
            open_lines,
            t: 0,
            data_bus: 0,
            internal_bus: 0,
            undriven: false,
            pad_oe: -1,
            oe_per_cycle: machine.joypad_oe_per_cycle,
            oam_dma_page: None,
            halts_on_opcode: machine.dma_halts_on_opcode,
            opcode_read: false,
            strobe: None,
            irq: false,
            nmi: false,
            external_irq: false,
            ppu_access: timing.ppu_access_ticks - timing.cpu_divider,
            snoop_reads,
            latches_reads,
            counts_m2,
            route_audio,
            bus_log: None,
            auto_disk: None,
            machine,
        }
    }

    /// CPU cycles since power, DMA cycles included.
    pub fn cycles(&self) -> i64 {
        self.t / self.cycle_ticks
    }

    pub fn cart(&mut self) -> &mut dyn Cartridge {
        self.ppu.cart.as_mut()
    }

    pub fn power(&mut self) {
        self.apu.power();
        self.ppu.resetting = self.machine.ppu_reset_at_power;
    }

    /// The reset button: the APU always, the PPU only where the console wires
    /// it, and the cartridge sees M2 stop.
    pub fn reset(&mut self) {
        self.apu.reset();
        if self.machine.reset_resets_ppu {
            self.ppu.reset();
        }
        if let Some(vs) = self.vs.as_mut() {
            vs.reset();
        }
        let t = self.t;
        self.ppu.cart.m2_gap(t);
    }

    /// The mainboard pulls /IRQ low, or lets go (the CPU sees it at the
    /// next cycle's end).
    pub fn set_external_irq(&mut self, asserted: bool) {
        self.external_irq = asserted;
    }

    /// The host rate for [`Board::take_samples`] (44,100 Hz by default).
    pub fn set_sample_rate(&mut self, rate: i64) {
        self.apu.set_sample_rate(rate);
        self.analogue = AnalogueStage::new(
            rate,
            &self.machine.audio_high_pass_hz,
            self.machine.audio_low_pass_hz,
        );
    }

    /// Audio since the last call, through the console's fitted analogue stage.
    pub fn take_samples(&mut self) -> Vec<f64> {
        let samples = self.apu.take_samples();
        self.analogue.process(&samples)
    }

    // -- one CPU cycle -----------------------------------------------------

    fn begin_cycle(&mut self, address: u16, reading: bool) -> i64 {
        let t = self.t + self.cycle_ticks;
        self.t = t;
        if self.route_audio {
            self.apu.expansion = self.ppu.cart.expansion();
        }
        self.apu.clock();
        if self.apu.put {
            if let Some(out) = self.strobe.take() {
                for pad in self.pads.iter_mut() {
                    pad.set_strobe(out & 1 != 0, &self.ppu);
                }
                if let Some(expansion) = self.expansion.as_mut() {
                    expansion.set_out(out, &self.ppu);
                } else if self.vs.is_some() {
                    self.vs_set_out(out);
                }
            }
        }
        let oe = self.pad_oe;
        if oe >= 0 && (self.oe_per_cycle || !reading || address != 0x4016 + oe as u16) {
            self.pads[oe as usize].clock(&self.ppu);
            if let Some(expansion) = self.expansion.as_mut() {
                expansion.clock(oe as usize, &self.ppu);
            }
            // (Fork: famicom-rust calls the Vs. mainboard's `clock` here when no
            // expansion device is plugged; it does nothing.)
            self.pad_oe = -1;
        }
        t
    }

    /// M2 falls: the cartridge's cycle count, then /IRQ and /NMI as the CPU samples them.
    fn end_cycle(&mut self, frame: &mut Frame) {
        if self.counts_m2 {
            let t = self.t;
            self.ppu.cart.m2(t);
        }
        // Fork: the external line tested before the cartridge's (a call);
        // both are reads without effects, so the order changes nothing.
        let irq = self.apu.irq || self.external_irq || self.ppu.cart.irq();
        if irq != self.irq {
            self.irq = irq;
            if irq {
                frame.request_maskable_interrupt();
            } else {
                frame.clear_maskable_interrupt();
            }
        }
        let nmi = self.ppu.nmi;
        if nmi != self.nmi {
            self.nmi = nmi;
            if nmi {
                frame.request_non_maskable_interrupt();
            }
        }
    }

    // -- the CPU's bus -----------------------------------------------------

    pub fn read_byte(&mut self, address: u16, frame: &mut Frame) -> u8 {
        let mut stalled = false;
        if self.apu.dmc_abort {
            self.apu.dmc_abort = false;
            frame.note_stall();
            self.repeat_read(address, frame);
            stalled = true;
        }
        if (self.oam_dma_page.is_some() || self.apu.dmc_request)
            && (!self.halts_on_opcode || self.opcode_read)
        {
            if !stalled {
                frame.note_stall();
            }
            self.dma(address, frame);
            stalled = true;
        }
        if frame.dma_stalled != stalled {
            frame.dma_stalled = stalled;
        }
        self.opcode_read = false;
        let t = self.begin_cycle(address, true);
        let value = self.read(address, t);
        self.internal_bus = value;
        self.end_cycle(frame);
        if let Some(log) = self.bus_log.as_mut() {
            log.push(pack_cycle(false, address, value, t));
        }
        if let Some(mut auto) = self.auto_disk.take() {
            auto.observe(self, false, address, value);
            self.auto_disk = Some(auto);
        }
        value
    }

    pub fn write_byte(&mut self, address: u16, value: u8, frame: &mut Frame) {
        self.apu.dmc_abort = false;
        let t = self.begin_cycle(address, false);
        let early = self.data_bus;
        self.data_bus = value;
        self.internal_bus = value;
        if (0x2000..0x4000).contains(&address) {
            let register = address & 7;
            let access = t + self.ppu_access - self.ppu.write_lead(register);
            self.ppu.run_to(access);
            let phase = self.ppu.time - access;
            self.ppu.write_register(register, value, Some(early), phase);
        }
        self.ppu.run_to(t);
        if address < 0x2000 {
            self.ram[address as usize & 0x7FF] = value;
        } else if (0x4000..0x4020).contains(&address) {
            self.write_io(address, value);
        } else if (0x4020..0x8000).contains(&address) && self.vs.is_some() {
            self.vs_write(address, value);
        }
        self.ppu.cart.cpu_write(address, value, t);
        self.end_cycle(frame);
        if let Some(log) = self.bus_log.as_mut() {
            log.push(pack_cycle(true, address, value, t));
        }
        if let Some(mut auto) = self.auto_disk.take() {
            auto.observe(self, true, address, value);
            self.auto_disk = Some(auto);
        }
    }

    /// The access of one read cycle: decode, drive, update open bus.
    fn read(&mut self, address: u16, t: i64) -> u8 {
        if (0x2000..0x4000).contains(&address) {
            let register = address & 7;
            self.ppu.run_to(t + self.ppu_access);
            let value = self.ppu.read_register(register);
            self.ppu.run_to(t);
            let value = self.ppu.end_read(register, value);
            if self.snoop_reads {
                self.ppu.cart.cpu_read(address, t);
            }
            self.data_bus = value;
            return value;
        }
        self.ppu.run_to(t);
        if address >= 0x4020 {
            // Fork: the address tested first and the Vs. mainboard's answer out
            // of line (`vs_read`), so a home console's cartridge read has one
            // call site and one cheap test.
            let read = if address < 0x8000 && self.vs.is_some() {
                self.vs_read(address, t)
            } else {
                self.ppu.cart.cpu_read(address, t)
            };
            let value = match read {
                None => {
                    self.undriven = true;
                    self.data_bus
                }
                Some(value) => {
                    let value = if value > 0xFF {
                        (value as u8) | (self.data_bus & (value >> 8) as u8)
                    } else {
                        value as u8
                    };
                    self.data_bus = value;
                    value
                }
            };
            if self.latches_reads {
                self.ppu.cart.read_data(address, value, t);
            }
            return value;
        }
        if self.snoop_reads {
            self.ppu.cart.cpu_read(address, t);
        }
        let value = if address < 0x2000 {
            self.ram[address as usize & 0x7FF]
        } else if address == 0x4015 {
            return self.apu.read_status() | (self.internal_bus & 0x20);
        } else if address == 0x4016 || address == 0x4017 {
            let port = (address - 0x4016) as usize;
            self.pad_oe = port as i32;
            if port == 1 {
                // Fork: the DualSystem's watchdog (dual.rs) noted here, off
                // read_byte's every cycle. Besides the CPU's own read, only a
                // DMA's repeats of a stalled $4017 read come here, and the
                // CPU's read that ends the stall writes over them before the
                // instruction ends; the watchdog is checked between
                // instructions, so it sees famicom-rust's value.
                if let Some(vs) = self.vs.as_mut() {
                    vs.fed = t;
                }
            }
            (self.data_bus & self.open_lines[port]) | self.port_bits(port)
        } else {
            return self.data_bus;
        };
        self.data_bus = value;
        value
    }

    /// Fork: the Vs. mainboard's OUT lines, out of the cycle's line.
    #[cold]
    #[inline(never)]
    fn vs_set_out(&mut self, out: u8) {
        self.vs
            .as_mut()
            .unwrap()
            .set_out(out, self.ppu.cart.as_mut());
    }

    /// Fork: a write to the Vs. mainboard's $4020-$7FFF, out of line.
    #[cold]
    #[inline(never)]
    fn vs_write(&mut self, address: u16, value: u8) {
        let vs = self.vs.as_mut().unwrap();
        if address < 0x6000 {
            vs.write_coin_counter(value);
        } else {
            vs.write_ram(address, value);
        }
    }

    /// The Vs. mainboard: its RAM at $6000 (open bus when the other side
    /// holds it); below, a read clocks the coin counter with whatever the bus
    /// holds, and a daughterboard's chip may answer (famicom-rust's `read`).
    #[cold]
    #[inline(never)]
    fn vs_read(&mut self, address: u16, t: i64) -> Option<u16> {
        let vs = self.vs.as_mut().unwrap();
        if address >= 0x6000 {
            vs.read_ram(address).map(u16::from)
        } else {
            vs.write_coin_counter(self.data_bus);
            self.ppu.cart.cpu_read(address, t)
        }
    }

    /// D0-D4 of $4016 (port 0) or $4017: the port's device, the expansion's, the microphone.
    fn port_bits(&mut self, port: usize) -> u8 {
        let mut bits = self.pads[port].read(&self.ppu);
        if let Some(expansion) = self.expansion.as_mut() {
            bits |= expansion.read(port, &self.ppu);
        } else if let Some(vs) = self.vs.as_mut() {
            bits |= vs.read(port, self.t);
        }
        if port == 0 && self.microphone && self.has_microphone {
            bits |= 4;
        }
        bits
    }

    fn write_io(&mut self, address: u16, value: u8) {
        if address == 0x4014 {
            self.oam_dma_page = Some(value);
        } else if address == 0x4016 {
            self.strobe = Some(value & 7);
        } else {
            self.apu.write(address, value);
        }
    }

    // -- DMA ---------------------------------------------------------------

    /// OAM and DMC DMA, run inside the CPU's stalled read of `address`.
    fn dma(&mut self, address: u16, frame: &mut Frame) {
        let page = self.oam_dma_page.take();
        let mut oam_next: u32 = if page.is_some() { 0 } else { 256 };
        let mut oam_value: Option<u8> = None;
        let mut dmc_age: Option<u32> = if self.apu.dmc_request { Some(0) } else { None };
        self.repeat_read(address, frame);
        loop {
            if let Some(age) = dmc_age.as_mut() {
                *age += 1;
            } else if self.apu.dmc_request {
                dmc_age = Some(0);
            }
            if oam_next == 256 && oam_value.is_none() && dmc_age.is_none() {
                return;
            }
            let get = self.apu.put;
            if get && dmc_age.is_some_and(|age| age >= 2) {
                let dmc_address = self.apu.dmc_address();
                let t = self.begin_cycle(decoded(dmc_address, address), true);
                let value = self.dma_read(dmc_address, address, t);
                self.apu.dmc_fill(value);
                self.end_cycle(frame);
                frame.note_stall();
                dmc_age = None;
            } else if get && oam_value.is_none() && oam_next < 256 {
                let source = ((page.unwrap_or(0) as u16) << 8) | oam_next as u16;
                let t = self.begin_cycle(decoded(source, address), true);
                oam_value = Some(self.dma_read(source, address, t));
                self.end_cycle(frame);
                frame.note_stall();
                oam_next += 1;
            } else if !get && oam_value.is_some() {
                let value = oam_value.take().unwrap();
                let t = self.begin_cycle(0x2004, false);
                self.data_bus = value;
                self.ppu.run_to(t + self.ppu_access);
                self.ppu.oam_dma_write(value);
                self.ppu.run_to(t);
                self.ppu.cart.cpu_write(0x2004, value, t);
                self.end_cycle(frame);
                frame.note_stall();
            } else {
                self.repeat_read(address, frame);
            }
        }
    }

    /// A DMA unit's read of `address` while the CPU is halted reading `halted`.
    fn dma_read(&mut self, address: u16, halted: u16, t: i64) -> u8 {
        self.undriven = (0x4000..0x4020).contains(&address);
        let external = if self.undriven {
            self.data_bus
        } else {
            self.read(address, t)
        };
        if halted & 0xFFE0 != 0x4000 {
            if (0x4000..0x4020).contains(&address) {
                self.ppu.run_to(t);
            }
            return external;
        }
        let register = 0x4000 | (address & 0x1F);
        if register == 0x4015 {
            return self.apu.read_status() | (external & 0x20);
        }
        if register == 0x4016 || register == 0x4017 {
            let port = (register - 0x4016) as usize;
            self.pad_oe = port as i32;
            let mixed = (external & self.open_lines[port]) | self.port_bits(port);
            self.data_bus = mixed;
            return if self.undriven { mixed } else { external };
        }
        external
    }

    fn repeat_read(&mut self, address: u16, frame: &mut Frame) {
        let t = self.begin_cycle(address, true);
        self.internal_bus = self.read(address, t);
        self.end_cycle(frame);
        frame.note_stall();
    }
}

/// The address the 2A03's register decode sees during a DMA access.
fn decoded(address: u16, halted: u16) -> u16 {
    if halted & 0xFFE0 == 0x4000 {
        0x4000 | (address & 0x1F)
    } else {
        address
    }
}

impl Bus for Board {
    #[inline]
    fn read(&mut self, address: u16, frame: &mut Frame) -> u8 {
        self.read_byte(address, frame)
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8, frame: &mut Frame) {
        self.write_byte(address, value, frame)
    }
}

// -- save states (state.rs) ---------------------------------------------------------

/// The board's own state: what changes as it runs. Its configuration (the
/// machine and the flags derived from it) stays as the system was built.
#[derive(serde::Serialize, serde::Deserialize)]
struct BoardFields {
    #[serde(with = "serde_big_array::BigArray")]
    ram: [u8; 0x800],
    ppu: Vec<u8>,
    apu: Vec<u8>,
    analogue: Vec<u8>,
    cart: Vec<u8>,
    pads: [Vec<u8>; 2],
    expansion: Option<Vec<u8>>,
    microphone: bool,
    t: i64,
    data_bus: u8,
    internal_bus: u8,
    undriven: bool,
    pad_oe: i32,
    oam_dma_page: Option<u8>,
    opcode_read: bool,
    strobe: Option<u8>,
    irq: bool,
    nmi: bool,
}

impl Board {
    /// The board's state (state.rs), the cartridge and devices included.
    pub fn save_state(&self) -> Vec<u8> {
        let fields = BoardFields {
            ram: self.ram,
            ppu: bincode::serialize(&self.ppu).expect("the PPU serializes"),
            apu: bincode::serialize(&self.apu).expect("the APU serializes"),
            analogue: bincode::serialize(&self.analogue).expect("the analogue stage serializes"),
            cart: self.ppu.cart.save_state(),
            pads: [self.pads[0].save_state(), self.pads[1].save_state()],
            // The Vs. mainboard is the reference's `expansion`: saved in its place.
            expansion: match (&self.expansion, &self.vs) {
                (Some(device), _) => Some(device.save_state()),
                (None, Some(vs)) => Some(crate::state::Snapshot::save_state(vs.as_ref())),
                (None, None) => None,
            },
            microphone: self.microphone,
            t: self.t,
            data_bus: self.data_bus,
            internal_bus: self.internal_bus,
            undriven: self.undriven,
            pad_oe: self.pad_oe,
            oam_dma_page: self.oam_dma_page,
            opcode_read: self.opcode_read,
            strobe: self.strobe,
            irq: self.irq,
            nmi: self.nmi,
        };
        bincode::serialize(&fields).expect("the board serializes")
    }

    /// Load a state [`Board::save_state`] wrote, inside `state::load_state`'s
    /// context. The cartridge and devices must be of the types saved.
    ///
    /// All or nothing: on an error the board is as it was. The parts held by
    /// value are decoded into temporaries first; the cartridge and the devices,
    /// trait objects that load in place (each one atomically, [`crate::state::Snapshot`]), go
    /// devices first with their old states kept, the cartridge last, and a failure
    /// puts the devices back.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let fields: BoardFields = bincode::deserialize(data).map_err(|e| e.to_string())?;
        let mut ppu: Ppu = bincode::deserialize(&fields.ppu).map_err(|e| e.to_string())?;
        let apu: Apu = bincode::deserialize(&fields.apu).map_err(|e| e.to_string())?;
        let analogue: AnalogueStage =
            bincode::deserialize(&fields.analogue).map_err(|e| e.to_string())?;
        let plugged = self.expansion.is_some() || self.vs.is_some();
        if fields.expansion.is_some() != plugged {
            return Err("a state with other devices plugged".into());
        }
        let kept_pads = [self.pads[0].save_state(), self.pads[1].save_state()];
        let kept_expansion = self.expansion.as_ref().map(|device| device.save_state());
        let kept_vs = self.vs.clone();
        let loaded = (|| {
            for (pad, saved) in self.pads.iter_mut().zip(&fields.pads) {
                pad.load_state(saved)?;
            }
            if let (Some(device), Some(saved)) = (self.expansion.as_mut(), &fields.expansion) {
                device.load_state(saved)?;
            } else if let (Some(vs), Some(saved)) = (self.vs.as_mut(), &fields.expansion) {
                crate::state::Snapshot::load_state(vs.as_mut(), saved)?;
            }
            // The cartridge stays this system's own object; its state is loaded into it.
            self.ppu.cart.load_state(&fields.cart)
        })();
        if let Err(error) = loaded {
            for (pad, kept) in self.pads.iter_mut().zip(&kept_pads) {
                pad.load_state(kept)
                    .expect("a device takes back its own state");
            }
            if let (Some(device), Some(kept)) = (self.expansion.as_mut(), &kept_expansion) {
                device
                    .load_state(kept)
                    .expect("a device takes back its own state");
            }
            self.vs = kept_vs;
            return Err(error);
        }
        std::mem::swap(&mut ppu.cart, &mut self.ppu.cart);
        self.ppu = ppu;
        self.apu = apu;
        self.analogue = analogue;
        self.ram = fields.ram;
        self.microphone = fields.microphone;
        self.t = fields.t;
        self.data_bus = fields.data_bus;
        self.internal_bus = fields.internal_bus;
        self.undriven = fields.undriven;
        self.pad_oe = fields.pad_oe;
        self.oam_dma_page = fields.oam_dma_page;
        self.opcode_read = fields.opcode_read;
        self.strobe = fields.strobe;
        self.irq = fields.irq;
        self.nmi = fields.nmi;
        Ok(())
    }
}
