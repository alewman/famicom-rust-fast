// SPDX-License-Identifier: MIT
//! The consoles, as data: one board, many machines (`machine.py`).
//!
//! Every console is the same board with different chips and quirks, so each
//! is a [`Machine`] and the board reads its fields. Each field's source and
//! tier is in the reference's comments (famicom-python `machine.py`); the
//! short forms here name them.

/// A video standard's clocks, in master-clock ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    pub name: &'static str,
    pub master_hz: f64,
    /// Master ticks per CPU cycle.
    pub cpu_divider: i64,
    /// Master ticks per PPU dot.
    pub dot_divider: i64,
    /// Scanlines per frame, the pre-render line included.
    pub lines: i32,
    /// The line on whose dot 1 the VBL flag sets.
    pub vblank_line: i32,
    /// With rendering on, odd frames are one dot short.
    pub odd_frame_skip: bool,
    /// PAL's black border: line 0 and two pixels at each side.
    pub border: bool,
    /// The 2C07's forced OAM refresh lines (first, last).
    pub oam_refresh: Option<(i32, i32)>,
    /// A $2003 write reaches OAMADDR a few dots late (2C07, UA6538).
    pub delayed_oamaddr: bool,
    pub palette: &'static str,
    pub swapped_emphasis: bool,
    /// Master ticks into a CPU cycle at which the PPU sees a register access.
    pub ppu_access_ticks: i64,
}

pub const NTSC: Timing = Timing {
    name: "NTSC",
    master_hz: 236.25e6 / 11.0,
    cpu_divider: 12,
    dot_divider: 4,
    lines: 262,
    vblank_line: 241,
    odd_frame_skip: true,
    border: false,
    oam_refresh: None,
    delayed_oamaddr: false,
    palette: "2C02G_U_wiki.pal",
    swapped_emphasis: false,
    ppu_access_ticks: 8,
};

pub const PAL: Timing = Timing {
    name: "PAL",
    master_hz: 26.601712e6,
    cpu_divider: 16,
    dot_divider: 5,
    lines: 312,
    vblank_line: 241,
    odd_frame_skip: false,
    border: true,
    oam_refresh: Some((265, 310)),
    delayed_oamaddr: true,
    palette: "2C07_wiki.pal",
    swapped_emphasis: true,
    ppu_access_ticks: 9,
};

pub const DENDY: Timing = Timing {
    name: "Dendy",
    master_hz: 26.601712e6,
    cpu_divider: 15,
    dot_divider: 5,
    lines: 312,
    vblank_line: 291,
    odd_frame_skip: false,
    border: true,
    oam_refresh: None,
    delayed_oamaddr: true,
    palette: "2C07_wiki.pal",
    swapped_emphasis: true,
    ppu_access_ticks: 8,
};

/// What a console holds when it is switched on (`machine.PowerOn`).
#[derive(Clone, Debug, PartialEq)]
pub struct PowerOn {
    pub apu_put_first: bool,
    pub ram_fill: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub palette: [u8; 32],
    pub ppustatus: u8,
    pub ppu_dot: i32,
    pub ppu_ticks: i64,
    pub dmc_timer: i32,
}

/// blargg's console's palette at power (`power_up_palette.asm`).
pub const POWER_PALETTE: [u8; 32] = [
    0x09, 0x01, 0x00, 0x01, 0x00, 0x02, 0x02, 0x0D, 0x08, 0x10, 0x08, 0x24, 0x00, 0x00, 0x04, 0x2C,
    0x09, 0x01, 0x34, 0x03, 0x00, 0x04, 0x00, 0x14, 0x08, 0x3A, 0x00, 0x02, 0x00, 0x20, 0x2C, 0x08,
];

impl Default for PowerOn {
    fn default() -> Self {
        PowerOn {
            apu_put_first: false,
            ram_fill: 0x00,
            a: 0,
            x: 0,
            y: 0,
            palette: POWER_PALETTE,
            ppustatus: 0x20,
            ppu_dot: 0,
            ppu_ticks: 0,
            dmc_timer: 0,
        }
    }
}

/// The APU's noise, DMC and frame-counter tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApuTables {
    Ntsc,
    Pal,
}

/// One console model (`machine.Machine`).
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: &'static str,
    pub timing: Timing,
    pub cpu_variant: &'static str,
    pub ppu_variant: &'static str,
    pub reset_resets_ppu: bool,
    pub ppu_reset_at_power: bool,
    pub joypad_oe_per_cycle: bool,
    pub famicom_audio_through_cart: bool,
    pub dmc_unexpected_reload_bug: bool,
    pub power_on: PowerOn,
    pub ane_constant: u8,
    pub lxa_constant: u8,
    pub ppu_latch_decay_seconds: f64,
    pub audio_high_pass_hz: Vec<f64>,
    pub audio_low_pass_hz: Option<f64>,
    pub microphone: bool,
    pub pad_open_bus: (u8, u8),
    pub apu_tables: ApuTables,
    pub dma_halts_on_opcode: bool,
    pub dmc_late_stop: bool,
    pub famicom_expansion_port: bool,
    pub certified: bool,
}

impl Machine {
    /// The fields every row shares by default (the dataclass defaults).
    #[allow(clippy::too_many_arguments)]
    fn row(
        name: &'static str,
        timing: Timing,
        cpu_variant: &'static str,
        ppu_variant: &'static str,
        reset_resets_ppu: bool,
        ppu_reset_at_power: bool,
        joypad_oe_per_cycle: bool,
        famicom_audio_through_cart: bool,
        dmc_unexpected_reload_bug: bool,
    ) -> Machine {
        Machine {
            name,
            timing,
            cpu_variant,
            ppu_variant,
            reset_resets_ppu,
            ppu_reset_at_power,
            joypad_oe_per_cycle,
            famicom_audio_through_cart,
            dmc_unexpected_reload_bug,
            power_on: PowerOn::default(),
            ane_constant: 0xEE,
            lxa_constant: 0xFF,
            ppu_latch_decay_seconds: 0.6,
            audio_high_pass_hz: vec![90.0, 440.0],
            audio_low_pass_hz: Some(14_000.0),
            microphone: false,
            pad_open_bus: (0, 0),
            apu_tables: ApuTables::Ntsc,
            dma_halts_on_opcode: false,
            dmc_late_stop: false,
            famicom_expansion_port: false,
            certified: true,
        }
    }

    pub fn nes_001() -> Machine {
        Machine::row(
            "NES-001", NTSC, "RP2A03G", "RP2C02G", true, true, false, false, false,
        )
    }

    pub fn famicom() -> Machine {
        Machine {
            audio_high_pass_hz: vec![37.0],
            audio_low_pass_hz: None,
            microphone: true,
            pad_open_bus: (0x18, 0),
            famicom_expansion_port: true,
            ..Machine::row(
                "Famicom HVC-001",
                NTSC,
                "RP2A03G",
                "RP2C02G",
                false,
                false,
                true,
                true,
                false,
            )
        }
    }

    pub fn av_famicom() -> Machine {
        Machine {
            pad_open_bus: (0x18, 0),
            famicom_expansion_port: true,
            ..Machine::row(
                "AV Famicom HVC-101",
                NTSC,
                "RP2A03G",
                "RP2C02G",
                false,
                false,
                false,
                true,
                false,
            )
        }
    }

    pub fn nes_101() -> Machine {
        Machine {
            pad_open_bus: (0x04, 0),
            ..Machine::row(
                "NES-101", NTSC, "RP2A03G", "RP2C02G", false, false, false, false, false,
            )
        }
    }

    pub fn pal_nes() -> Machine {
        Machine {
            apu_tables: ApuTables::Pal,
            dma_halts_on_opcode: true,
            certified: false,
            ..Machine::row(
                "PAL NES", PAL, "RP2A07", "RP2C07", true, true, false, false, false,
            )
        }
    }

    pub fn dendy() -> Machine {
        Machine {
            dmc_late_stop: true,
            certified: false,
            ..Machine::row(
                "Dendy", DENDY, "UA6527P", "UA6538", false, false, true, false, true,
            )
        }
    }

    /// `MACHINES[key]`.
    pub fn by_key(key: &str) -> Option<Machine> {
        Some(match key {
            "nes" => Machine::nes_001(),
            "famicom" => Machine::famicom(),
            "av-famicom" => Machine::av_famicom(),
            "nes-101" => Machine::nes_101(),
            "pal" => Machine::pal_nes(),
            "dendy" => Machine::dendy(),
            _ => return None,
        })
    }
}
