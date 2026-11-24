// SPDX-License-Identifier: MIT AND BSD-3-Clause
// The RC2C05-01's identifier ($1B) is MAME's ppu2c0x.cpp (BSD-3-Clause, Ernesto Corvi,
// Brad Oliver, Fabio Priuli), by way of the reference: notice in NOTICE.md.
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
    /// An RGB PPU (2C03, 2C04, 2C05): `palette` names a DAC table of
    /// `video::RGB_DAC` instead of a file, and an emphasis bit drives its
    /// channel to full brightness instead of dimming the others
    /// (PPU_palettes.wiki "2C03 and 2C05"; Errata.wiki l.17).
    pub rgb: bool,
    /// Rendering switched off mid-line corrupts an OAM row when it next starts
    /// (`ppu.oam_corrupt`): "OAM corruption added in revision E"
    /// (PPU_variants.wiki, RC2C05-99). The arcade RGB PPUs are 2C02B, C and D
    /// cores (the same table), and "in the 2C03, 2C04, 2C05 ... OAMADDR works as
    /// intended" (PPU_registers.wiki "OAMADDR precautions", lidnariq on a Vs.
    /// System, forum p=179676): specification tier, no test ROM run on one.
    pub oam_corruption: bool,
    /// The RC2C05's copy protection (PPU_registers.wiki "2C05 identifier",
    /// NES_2.0.wiki "Vs. System Type", Vs._System.wiki): $2000 and $2001 change
    /// places, and a $2002 read returns `identifier` on the bits of
    /// `identifier_mask` instead of the I/O latch. The values are what the
    /// games expect: "we haven't yet collected data directly from the PPUs".
    pub swapped_control: bool,
    pub identifier: u8,
    pub identifier_mask: u8,
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
    rgb: false,
    oam_corruption: true,
    swapped_control: false,
    identifier: 0,
    identifier_mask: 0,
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
    rgb: false,
    oam_corruption: true,
    swapped_control: false,
    identifier: 0,
    identifier_mask: 0,
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
    rgb: false,
    oam_corruption: true,
    swapped_control: false,
    identifier: 0,
    identifier_mask: 0,
};

/// The arcade RGB PPUs: "All official RGB PPUs are NTSC and expect a 21.477272
/// MHz master clock" (PPU_variants.wiki "RGB"), and "never have the missing dot
/// that the 2C02 does" (Vs._System.wiki "Synchronization"; PPU_variants.wiki:
/// the 2C04 "keeps 2C03 timing (no skipped dot)"). Not modelled, each on the
/// wiki's "believed" only: OAMDATA and the palette unreadable (the 2C03B as the
/// 2C02B), and the 2C02A/B's stray sprite pixels at x = 255 (the reference's
/// docs/undocumented-behavior.md C76).
pub const RGB: Timing = Timing {
    odd_frame_skip: false,
    rgb: true,
    oam_corruption: false,
    palette: "RP2C03",
    ..NTSC
};

const fn rc2c05(identifier: u8, mask: u8) -> Timing {
    Timing {
        swapped_control: true,
        identifier,
        identifier_mask: mask,
        ..RGB
    }
}

const fn rp2c04(palette: &'static str) -> Timing {
    Timing { palette, ..RGB }
}

/// The Vs. System's PPUs by NES 2.0 "Vs. PPU type" (header byte 13's low
/// nibble, nes20db's `<vs ppu=>`): the chip's name and its row. The RC2C05-01's
/// identifier is unknown (its one game, Ninja Jajamaru-kun, never looks:
/// NES_2.0.wiki); $1B is MAME's choice (ppu2c0x.cpp, emulator-derived). The
/// reference's `_rc2c05`'s default mask is $1F.
pub const VS_PPUS: [(u8, &str, Timing); 9] = [
    (0, "RP2C03B", RGB),
    (2, "RP2C04-0001", rp2c04("RP2C04-0001")),
    (3, "RP2C04-0002", rp2c04("RP2C04-0002")),
    (4, "RP2C04-0003", rp2c04("RP2C04-0003")),
    (5, "RP2C04-0004", rp2c04("RP2C04-0004")),
    (8, "RC2C05-01", rc2c05(0x1B, 0x1F)),
    (9, "RC2C05-02", rc2c05(0x3D, 0x3F)),
    (10, "RC2C05-03", rc2c05(0x1C, 0x1F)),
    (11, "RC2C05-04", rc2c05(0x1B, 0x1F)),
];

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
    /// The letterless RP2A03 of the first Famicoms and every Vs. System board.
    Rp2a03,
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
    /// A Vs. System mainboard (Vs._System.wiki, vs.rs): $4016/$4017 carry coins,
    /// the service button and DIP switches on D2-D7, driven, not open bus;
    /// $4020-$5FFF is the coin counter; 2 KiB of RAM answers at $6000-$7FFF;
    /// OUT2 reaches the ROM sockets. "primary" or "secondary" is the side of
    /// the board the CPU sits on ($4016 D7; the reference's C75), None any other console.
    pub vs_side: Option<&'static str>,
    /// A PlayChoice-10's game board (playchoice.rs): the control panel's two
    /// 74LS165 shift registers stand where the controller ports are, and a Z80
    /// supervisor (supervisor.rs) holds the reset, stop and mask lines.
    pub playchoice: bool,
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
            vs_side: None,
            playchoice: false,
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

    /// The Vs. UniSystem (Vs._System.wiki; MAME's vsnes.cpp, BSD-3, emulator-derived).
    ///
    /// - "The CPUs are always the letterless revision, and the arcade board is
    ///   incompatible with a 2A03G, so tonal noise is not supported" (apu
    ///   [`ApuTables::Rp2a03`]; CPU_variants.wiki's row adds the frame counter
    ///   that a reset leaves alone, and an M2 duty of 17/24, not modelled: the
    ///   board's $2002 races are the RP2A03G's 15/24).
    /// - Reset is the watchdog's: it "will reset both CPUs, both PPUs", and "all
    ///   four are released from reset by the watchdog at the same time", so the
    ///   PPU has the NES-001's power-on window.
    /// - UNVERIFIED, taken as the NES-001's for want of a source: how
    ///   back-to-back reads clock the mainboard's 4021s, and the audio
    ///   amplifier's filter.
    /// - The side: a UniSystem game's CPU is in the secondary socket ($4016 D7 =
    ///   1), the one whose $4017 reads hold the watchdog off; MAME and Mesen2
    ///   both answer 0 there, and the RAM and /IRQ that follow from the socket
    ///   are the reference's C75.
    ///
    /// One row per PPU, since the game's ROMs name the chip they need:
    /// [`Machine::vs_machine`] picks the row.
    pub fn vs_unisystem() -> Machine {
        Machine {
            apu_tables: ApuTables::Rp2a03,
            vs_side: Some("secondary"),
            certified: false,
            ..Machine::row(
                "Vs. UniSystem (RP2C03B)",
                RGB,
                "RP2A03",
                "RP2C03B",
                true,
                true,
                false,
                false,
                false,
            )
        }
    }

    /// The PlayChoice-10's game side (Nintendo's PCH1-CPU schematic, 1986, sheets 5
    /// and 6, pinned by the reference as reference/docs/Playchoice.pdf:
    /// manufacturer's documentation; MAME's playch10.cpp, BSD-3, emulator-derived).
    ///
    /// - The CPU is an RP2A03E (the schematic's 1H; CPU_variants.wiki: "pin 30 is
    ///   /RDY", which the supervisor uses to stop the game), on the 21.47727 MHz
    ///   crystal it shares with the PPU. Nothing pinned tells an E's APU from a G's:
    ///   the tonal noise and the tables are taken as the G's.
    /// - The PPU is an RP2C03B (3H), "used in ... all PlayChoice games"
    ///   (PPU_palettes.wiki): the Vs. row's RGB timing, no skipped dot, the 2C03's
    ///   colours (the reference's C76 for what its core is).
    /// - /PPU RES and /GAME RES are two outputs of the supervisor's latch, so the
    ///   PPU has a reset of its own and the NES-001's power-on window after it.
    /// - UNVERIFIED, taken as the NES-001's: whether a run of back-to-back reads
    ///   clocks the shift registers once (their clocks are the 2A03E's /INP0 and
    ///   /INP1), and the audio amplifier's filter (sheet 4, not analysed).
    pub fn playchoice() -> Machine {
        Machine {
            playchoice: true,
            certified: false,
            ..Machine::row(
                "PlayChoice-10",
                RGB,
                "RP2A03E",
                "RP2C03B",
                true,
                true,
                false,
                false,
                false,
            )
        }
    }

    /// `VS_MACHINES[ppu]`: the UniSystem with the PPU of NES 2.0 "Vs. PPU type"
    /// `ppu`, None for a reserved number.
    pub fn vs_machines(ppu: u8) -> Option<Machine> {
        let (_, chip, timing) = VS_PPUS.iter().find(|row| row.0 == ppu)?;
        if ppu == 0 {
            return Some(Machine::vs_unisystem());
        }
        Some(Machine {
            name: vs_name(chip),
            timing: timing.clone(),
            ppu_variant: chip,
            ..Machine::vs_unisystem()
        })
    }

    /// The Vs. UniSystem with the PPU of NES 2.0 "Vs. PPU type" `ppu`; a
    /// reserved number gets the RP2C03B, whose colours are the home consoles'.
    pub fn vs_machine(ppu: u8) -> Machine {
        Machine::vs_machines(ppu).unwrap_or_else(Machine::vs_unisystem)
    }

    /// `MACHINES[key]`: the home consoles, `vs`, `playchoice`, and `vs-` with the PPU's name
    /// in lower case for every Vs. row but the RP2C03B's.
    pub fn by_key(key: &str) -> Option<Machine> {
        Some(match key {
            "nes" => Machine::nes_001(),
            "famicom" => Machine::famicom(),
            "av-famicom" => Machine::av_famicom(),
            "nes-101" => Machine::nes_101(),
            "pal" => Machine::pal_nes(),
            "dendy" => Machine::dendy(),
            "vs" => Machine::vs_unisystem(),
            "playchoice" => Machine::playchoice(),
            _ => {
                let chip = key.strip_prefix("vs-")?;
                let (number, _, _) = VS_PPUS
                    .iter()
                    .find(|row| row.0 != 0 && row.1.to_lowercase() == chip)?;
                return Machine::vs_machines(*number);
            }
        })
    }

    /// The keys of `MACHINES`, in the reference's order.
    pub fn keys() -> Vec<String> {
        let mut keys: Vec<String> = [
            "nes",
            "famicom",
            "av-famicom",
            "nes-101",
            "pal",
            "dendy",
            "vs",
            "playchoice",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect();
        for (number, chip, _) in VS_PPUS {
            if number != 0 {
                keys.push(format!("vs-{}", chip.to_lowercase()));
            }
        }
        keys
    }
}

/// The reference's `f"Vs. UniSystem ({chip})"`, as a static name.
fn vs_name(chip: &str) -> &'static str {
    match chip {
        "RP2C04-0001" => "Vs. UniSystem (RP2C04-0001)",
        "RP2C04-0002" => "Vs. UniSystem (RP2C04-0002)",
        "RP2C04-0003" => "Vs. UniSystem (RP2C04-0003)",
        "RP2C04-0004" => "Vs. UniSystem (RP2C04-0004)",
        "RC2C05-01" => "Vs. UniSystem (RC2C05-01)",
        "RC2C05-02" => "Vs. UniSystem (RC2C05-02)",
        "RC2C05-03" => "Vs. UniSystem (RC2C05-03)",
        "RC2C05-04" => "Vs. UniSystem (RC2C05-04)",
        _ => "Vs. UniSystem (RP2C03B)",
    }
}
