// SPDX-License-Identifier: MIT
//! The PlayChoice-10 on synthetic images: the reference's tests/test_playchoice.py
//! (famicom-python `c52a6d3`), transcribed: its game board's row and panel, the
//! security PROM, the supervisor's memory, ports, screen, interrupt and watchdog,
//! and the cabinet's lines to the game. The cases with the real BIOS and a game
//! are the lockstep gate's (scripts/cases-pc10.tsv), and two here that skip
//! without MAME's `playch10` set (`MAME_ROMPATH`) and the images
//! (`PC10_IMAGES_DIR`).

use famicom_rust::cartridge;
use famicom_rust::joypad::{A, B, DOWN, LEFT, RIGHT, SELECT, START, UP};
use famicom_rust::machine::{Machine, NTSC};
use famicom_rust::peripherals::Plugged;
use famicom_rust::playchoice::{game_roms, PlayChoiceGun, PlayChoicePad, COUNTER_OUT};
use famicom_rust::state::{load_cabinet_state, save_cabinet_state};
use famicom_rust::supervisor::*;
use famicom_rust::System;
use std::path::PathBuf;

const PROM: [u8; 16] = [
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
];

fn inst() -> Vec<u8> {
    let mut inst = vec![0xC3, 0x00, 0xC0];
    inst.resize(0x2000, 0);
    inst
}

fn misc_default() -> Vec<u8> {
    [inst(), PROM.to_vec(), COUNTER_OUT.to_vec()].concat()
}

/// An NES 2.0 PlayChoice-10 image (NROM): `program` at $F000, the reset vector
/// on it, NMI and IRQ on an RTI at $F800.
fn pc10_image(program: &[u8], misc: Option<Vec<u8>>, device: u8) -> Vec<u8> {
    let mut prg = vec![0u8; 0x8000];
    prg[0x7000..0x7000 + program.len()].copy_from_slice(program);
    prg[0x7800] = 0x40; // RTI
    prg[0x7FFA..].copy_from_slice(&[0x00, 0xF8, 0x00, 0xF0, 0x00, 0xF8]);
    let misc = misc.unwrap_or_else(misc_default);
    let header = [
        0x4E, 0x45, 0x53, 0x1A, 2, 1, 0, 0x0A, 0, 0, 0, 0, 0, 0, 3, device,
    ];
    [header.to_vec(), prg, vec![0; 0x2000], misc].concat()
}

const JUMP: &[u8] = &[0x4C, 0x00, 0xF0];

fn load(data: &[u8]) -> cartridge::Loaded {
    cartridge::load(data, 12, None, false).unwrap()
}

/// A BIOS of a few bytes: `program` at 0, `nmi` at $0066; character 1 has every
/// pixel 7, character 2 every pixel 1; each PROM holds its index's low nibble,
/// inverted (so a colour's channel is the index's weights).
fn bios(program: &[u8], nmi: &[u8]) -> Bios {
    let mut rom = vec![0u8; 0x4000];
    rom[..program.len()].copy_from_slice(program);
    rom[0x66..0x66 + nmi.len()].copy_from_slice(nmi);
    let mut solid = vec![0u8; 0x2000];
    solid[8..16].fill(0xFF);
    let mut low = vec![0u8; 0x2000];
    low[8..24].fill(0xFF);
    let prom: Vec<u8> = (0..256).map(|i: u32| (!i & 0x0F) as u8).collect();
    Bios {
        name: "dual".into(),
        program: rom,
        characters: [solid.clone(), solid, low],
        proms: [prom.clone(), prom.clone(), prom],
    }
}

const SPIN: &[u8] = &[0x18, 0xFE];
const RETN: &[u8] = &[0xED, 0x45];

fn cabinet(z80: &[u8], game: &[u8]) -> PlayChoice {
    PlayChoice::new(load(&pc10_image(game, None, 1)), bios(z80, RETN), None)
}

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

// -- the game board ----------------------------------------------------------

#[test]
fn console_type_2_is_the_playchoice_row() {
    let machine = System::machine_for(&pc10_image(JUMP, None, 1)).unwrap();
    assert_eq!(machine, Machine::playchoice());
    assert!(machine.playchoice);
    assert_eq!(
        (machine.cpu_variant, machine.ppu_variant),
        ("RP2A03E", "RP2C03B")
    );
    let timing = &machine.timing;
    assert!(timing.rgb && !timing.odd_frame_skip && timing.palette == "RP2C03");
    assert_eq!(
        (timing.master_hz, timing.cpu_divider, timing.lines),
        (NTSC.master_hz, NTSC.cpu_divider, NTSC.lines)
    );
}

#[test]
fn the_image_keeps_the_inst_rom_and_the_prom() {
    let roms = game_roms(&load(&pc10_image(JUMP, None, 1)).image).unwrap();
    assert_eq!(
        (roms.inst, roms.prom, roms.counter_out),
        (inst(), Some(PROM.to_vec()), COUNTER_OUT.to_vec())
    );
    // An older dump: the INST ROM alone.
    let old = game_roms(&load(&pc10_image(JUMP, Some(inst()), 1)).image).unwrap();
    assert!(old.prom.is_none() && old.inst == inst());
    assert!(game_roms(&load(&pc10_image(JUMP, Some(Vec::new()), 1)).image).is_none());
}

fn report(pad: &mut PlayChoicePad, reads: usize) -> Vec<u8> {
    pad.set_strobe(true);
    pad.set_strobe(false);
    let mut bits = Vec::new();
    for _ in 0..reads {
        bits.push(pad.read() & 1);
        pad.clock();
    }
    bits
}

#[test]
fn the_panels_registers_report_as_controllers_and_then_ones() {
    let mut pad = PlayChoicePad::new(true);
    for (bit, button) in [A, B, SELECT, START, UP, DOWN, LEFT, RIGHT]
        .into_iter()
        .enumerate()
    {
        pad.buttons = button;
        let mut expected: Vec<u8> = (0..8).map(|n| (n == bit) as u8).collect();
        expected.extend([1, 1]);
        assert_eq!(report(&mut pad, 10), expected);
    }
}

#[test]
fn player_2_has_no_select_or_start() {
    let mut pad = PlayChoicePad::new(false);
    pad.buttons = 0xFF;
    assert_eq!(report(&mut pad, 8), vec![1, 1, 0, 0, 1, 1, 1, 1]);
}

#[test]
fn cntrl_mask_holds_start_and_select_released() {
    let mut pad = PlayChoicePad::new(true);
    pad.buttons = 0xFF;
    pad.masked = true;
    assert_eq!(report(&mut pad, 8), vec![1, 1, 0, 0, 1, 1, 1, 1]);
    pad.masked = false;
    assert_eq!(report(&mut pad, 8), vec![1; 8]);
}

#[test]
fn the_ports_drive_d0_to_d4_and_leave_the_rest_to_the_bus() {
    // LDA #1; STA $4016; LSR; STA $4016; LDA $4016; STA $00; LDA $4017; STA $01; JMP
    let program = hex("a9 01 8d 16 40 4a 8d 16 40 ad 16 40 85 00 ad 17 40 85 01 4c 13 f0");
    let mut system =
        System::from_rom(&pc10_image(&program, None, 1), Machine::playchoice(), None).unwrap();
    system.board().pad_as::<PlayChoicePad>(0).unwrap().buttons = A;
    system.run_cycles(200);
    assert_eq!((system.cpu.bus.ram[0], system.cpu.bus.ram[1]), (0x41, 0x40));
}

#[test]
fn the_gun_shares_4017_with_player_2() {
    let mut system =
        System::from_rom(&pc10_image(JUMP, None, 8), Machine::playchoice(), None).unwrap();
    assert!(matches!(system.plug(None), Plugged::Pad(1)));
    let board = system.board();
    let (ppu, pads) = (&board.ppu, &mut board.pads);
    let gun = pads[1]
        .as_any_mut()
        .downcast_mut::<PlayChoiceGun>()
        .unwrap();
    gun.pad.buttons = A;
    gun.pad.set_strobe(true);
    assert_eq!(gun.read(ppu), 0x08 | 1); // no light, no trigger, A
    gun.zapper.set_trigger(true, ppu.time);
    assert_ne!(gun.read(ppu) & 0x10, 0);
    gun.set_masked(true);
    assert_eq!(gun.read(ppu) & 0x10, 0);
}

// -- the security PROM -------------------------------------------------------

#[test]
fn the_prom_counts_falling_clocks_and_reads_out_serially() {
    let mut prom = Rp5h01::new(PROM.to_vec(), COUNTER_OUT.to_vec());
    prom.write(true, false, false);
    prom.write(false, false, false);
    let mut bits = Vec::new();
    for _ in 0..64 {
        bits.push(prom.data_out());
        prom.write(false, true, false);
        assert_eq!(prom.data_out(), *bits.last().unwrap()); // nothing moves on the rising edge
        prom.write(false, false, false);
    }
    let expected: Vec<u8> = (0..64).map(|i| (PROM[i >> 3] >> (i & 7)) & 1).collect();
    assert_eq!(bits, expected);
    assert!(prom.counter == 64 && prom.data_out() == bits[0]); // six bits: around again
    prom.write(false, false, true);
    assert_eq!(prom.data_out(), PROM[8] & 1); // TEST: the seventh bit counts too
}

#[test]
fn the_proms_counter_output_is_bit_5_and_reset_holds_it() {
    let mut prom = Rp5h01::new(PROM.to_vec(), COUNTER_OUT.to_vec());
    let mut seen = Vec::new();
    for _ in 0..128 {
        seen.push(prom.counter_output());
        prom.write(false, true, false);
        prom.write(false, false, false);
    }
    let expected: Vec<u8> = (0..128).map(|n| (n >> 5) & 1).collect();
    assert_eq!(seen, expected);
    prom.write(true, true, false);
    prom.write(true, false, false);
    assert_eq!(prom.counter, 0);
}

// -- the supervisor ----------------------------------------------------------

fn supervisor(program: &[u8], nmi: &[u8]) -> Supervisor {
    let roms = game_roms(&load(&pc10_image(JUMP, None, 1)).image);
    let mut sup = Supervisor::new(bios(program, nmi), roms);
    sup.power();
    sup
}

#[test]
fn the_z80s_memory() {
    let mut sup = supervisor(SPIN, RETN);
    assert!(sup.read_byte(0x0000) == 0x18 && sup.read_byte(0xC000) == 0xC3);
    sup.write_byte(0x8000, 0x12);
    sup.write_byte(0x8801, 0x34);
    assert_eq!(sup.read_byte(0x8000), 0x12);
    // 8W's upper half mirrors the lower until the key is turned.
    assert_eq!(sup.read_byte(0x8C01), 0x34);
    sup.write_port(BUP_KEY as u16, 1);
    assert_eq!(sup.read_byte(0x8C01), 0x00);
    sup.write_byte(0x8C01, 0x56);
    assert_eq!((sup.read_byte(0x8801), sup.read_byte(0x8C01)), (0x34, 0x56));
}

#[test]
fn the_screens_ram_is_the_z80s_only_while_sdcs_is_low() {
    let mut sup = supervisor(SPIN, RETN);
    sup.write_byte(0x9000, 0x01);
    sup.write_port(SDCS as u16, 1);
    sup.write_byte(0x9000, 0x02);
    assert!(sup.cpu.bus.video_ram[0] == 0x01 && !sup.screen_blank());
}

#[test]
fn the_prom_through_e000() {
    let mut sup = supervisor(SPIN, RETN);
    assert!(sup.cpu.bus.prom.is_some());
    sup.write_byte(0xE000, 0x00); // D0 low: RESET
    sup.write_byte(0xE000, 0x01);
    let mut bits = Vec::new();
    for _ in 0..16 {
        let value = sup.read_byte(0xFFFF);
        assert!(value & 0xE7 == 0xE7 && value & 0x10 != 0); // counter bit 5 is 0, inverted
        bits.push((value >> 3) & 1);
        sup.write_byte(0xE000, 0x09);
        sup.write_byte(0xE000, 0x01);
    }
    let expected: Vec<u8> = (0..16)
        .map(|i| ((PROM[i >> 3] >> (i & 7)) & 1) ^ 1)
        .collect();
    assert_eq!(bits, expected);
    // Another channel: an empty socket.
    sup.write_port(CSEL0 as u16, 1);
    assert!(sup.read_byte(0xE000) == 0xE7 && sup.read_byte(0xC000) == 0xFF);
}

#[test]
fn port_0_coins_buttons_and_int_detect() {
    let mut sup = supervisor(SPIN, RETN);
    assert_eq!(sup.read_port(0), INT_DETECT); // nothing detected reads 1
    sup.cpu.bus.int_detect = true;
    assert_eq!(sup.read_port(0x10), 0); // reads decode two address lines
    sup.read_port(3);
    assert_eq!(sup.read_port(0), INT_DETECT);
    sup.cpu.bus.buttons = CHANNEL | ENTER;
    sup.insert_coin(0, None);
    let at = sup.t() + Z80_HZ / 10;
    sup.insert_coin(1, Some(at));
    assert_eq!(sup.read_port(0), INT_DETECT | CHANNEL | ENTER | COIN_1);
    sup.cpu.bus.t += Z80_HZ / 10;
    assert_eq!(sup.read_port(0), INT_DETECT | CHANNEL | ENTER | COIN_2);
    sup.cpu.bus.t += Z80_HZ / 10;
    assert_eq!(sup.read_port(0), INT_DETECT | CHANNEL | ENTER);
    sup.cpu.bus.dip = [0x40, 0x3F];
    assert_eq!((sup.read_port(1), sup.read_port(2)), (0x40, 0x3F));
}

#[test]
fn the_watchdog_resets_on_the_eighth_blank() {
    let mut sup = supervisor(SPIN, RETN);
    while sup.blanks < 7 {
        sup.step();
    }
    assert_eq!(sup.watchdog_resets, 0);
    while sup.blanks < 8 {
        sup.step();
    }
    assert_eq!(sup.watchdog_resets, 1);
    // DOG DI holds the counter: LD A,1; OUT (9),A; JR $
    let mut sup = supervisor(&hex("3e 01 d3 09 18 fe"), RETN);
    while sup.blanks < 20 {
        sup.step();
    }
    assert!(sup.watchdog_resets == 0 && sup.latch()[DOG_DI] != 0);
}

#[test]
fn the_blank_interrupt_fires_once_until_nmi_ei_is_dropped() {
    let start = hex("31 00 88 3e 01 d3 08 18 fe"); // LD SP,$8800; LD A,1; OUT (8),A; JR $
    let count = hex("21 00 80 34"); // LD HL,$8000; INC (HL)
    let mut sup = supervisor(&start, &[count.clone(), hex("ed 45")].concat());
    while sup.blanks < 5 {
        sup.step();
    }
    sup.step();
    assert_eq!(sup.cpu.bus.ram[0], 1);
    // XOR A; OUT (8),A; INC A; OUT (8),A; RETN: armed again each time.
    let mut sup = supervisor(&start, &[count, hex("af d3 08 3c d3 08 ed 45")].concat());
    while sup.blanks < 5 {
        sup.step();
    }
    for _ in 0..8 {
        sup.step();
    }
    assert_eq!(sup.cpu.bus.ram[0], 5);
}

#[test]
fn the_blank_is_60_21_times_a_second() {
    let mut sup = supervisor(&hex("3e 01 d3 09 18 fe"), RETN);
    while sup.t() < 10 * Z80_HZ {
        sup.step();
    }
    // 5.04 MHz / (327 x 256), the first at line 240.
    assert_eq!(sup.blanks, 602);
}

#[test]
fn the_screen_draws_characters_through_the_proms() {
    let mut sup = supervisor(SPIN, RETN);
    sup.write_byte(0x9000 + 2 * 64 + 2, 0x01); // row 2, column 1: character 1 ...
    sup.write_byte(0x9000 + 2 * 64 + 3, 0x05 << 3); // ... in colour 5
    sup.write_byte(0x9000, 0x02); // row 0: in the vertical blank
    assert_eq!(sup.picture(), vec![0u8; 256 * 240 * 3]); // SDCS low: blank
    sup.write_port(SDCS as u16, 1);
    let picture = sup.picture();
    // Index (5 << 3 | 7) & 15 = 15: every weight, 0x0E + 0x1F + 0x43 + 0x8F.
    let at = (16 * 256 + 8) * 3;
    assert_eq!(&picture[at..at + 3], &[0xFF, 0xFF, 0xFF]);
    assert_eq!(&picture[..3], &[0, 0, 0]); // row 0 is not shown
                                           // MAME's thirty rows: row 0's character 2, pixel 1, colour 0: weight 0x0E.
    assert_eq!(&sup.picture_lines((0, 240))[..3], &[0x0E, 0x0E, 0x0E]);
}

// -- the cabinet -------------------------------------------------------------

/// LD A,1; OUT (n),A for /PPU RES, /GAME RES, /GAME STOP, DOG DI; JR $
const RELEASE: &str = "3e 01 d3 0a d3 04 d3 05 d3 09 18 fe";
/// LDA #$80; STA $2000; INC $10; JMP $F002 (the write repeated: the first falls
/// in the PPU's window after its reset)
const COUNTING: &str = "a9 80 8d 00 20 e6 10 4c 02 f0";

fn ram10(machine: &PlayChoice) -> u8 {
    machine.game.cpu.bus.ram[0x10]
}

#[test]
fn the_game_is_held_in_reset_until_the_z80_lets_go() {
    let mut held = cabinet(&hex("3e 01 d3 09 18 fe"), &hex(COUNTING));
    held.run_frames(3);
    assert!(held.in_reset && held.game.instructions == 0 && ram10(&held) == 0);
    assert!(held.game_picture() == vec![0x0F; 256 * 240] && !held.sound_on());
    let mut running = cabinet(&hex(RELEASE), &hex(COUNTING));
    running.run_frames(3);
    assert!(!running.in_reset && ram10(&running) != 0);
    running.supervisor_write_port(DISP_MASK as u16, 1);
    assert_eq!(running.game_picture(), running.game.cpu.bus.ppu.picture);
}

#[test]
fn the_two_clocks_stay_within_an_instruction() {
    let mut machine = cabinet(&hex(RELEASE), &hex(COUNTING));
    for _ in 0..20_000 {
        machine.step();
        let gap = machine.supervisor.t() * 945 - machine.game.cpu.bus.t * 176;
        assert!(gap.abs() <= 23 * 945); // the longest Z80 instruction; a 2A03's is shorter
    }
}

#[test]
fn game_stop_waits_for_the_games_interrupt() {
    let mut machine = cabinet(&hex(RELEASE), &hex(COUNTING));
    machine.run_frames(3);
    machine.supervisor_read_port(3);
    machine.run_frames(1);
    assert!(machine.supervisor.cpu.bus.int_detect); // the PPU's /INT was seen
    machine.supervisor_write_port(GAME_STOP as u16, 0);
    assert!(!machine.stopped);
    machine.run_frames(2);
    assert!(machine.stopped);
    let count = ram10(&machine);
    machine.run_frames(2);
    assert_eq!(ram10(&machine), count);
    machine.supervisor_write_port(GAME_STOP as u16, 1);
    machine.run_frames(2);
    assert!(!machine.stopped && ram10(&machine) != count);
    // A game that never enables its NMI is never stopped (nor detected).
    let mut quiet = cabinet(&hex(RELEASE), &hex("e6 10 4c 00 f0"));
    quiet.run_frames(3);
    quiet.supervisor_write_port(GAME_STOP as u16, 0);
    quiet.run_frames(3);
    assert!(!quiet.stopped && !quiet.supervisor.cpu.bus.int_detect);
}

#[test]
fn game_res_resets_the_2a03_and_ppu_res_the_ppu() {
    let mut machine = cabinet(&hex(RELEASE), &hex(COUNTING));
    machine.run_frames(3);
    assert_eq!(machine.game.cpu.bus.ppu.ctrl, 0x80);
    machine.supervisor_write_port(PPU_RES as u16, 0);
    assert_eq!(machine.game.cpu.bus.ppu.ctrl, 0);
    machine.supervisor_write_port(GAME_RES as u16, 0);
    assert!(machine.in_reset);
    let count = ram10(&machine);
    machine.run_frames(2);
    assert_eq!(ram10(&machine), count);
    machine.supervisor_write_port(PPU_RES as u16, 1);
    machine.supervisor_write_port(GAME_RES as u16, 1);
    machine.run_frames(3);
    assert!(ram10(&machine) != count && machine.game.cpu.bus.ppu.ctrl == 0x80);
}

/// A supervisor program from its listing (scripts/inputs/pc10/watchdog.hex,
/// the reference's tests/pc10_watchdog.hex), as the reference's `read_program`.
fn read_program(path: &std::path::Path) -> Vec<u8> {
    let mut rom = vec![0xFF; 0x4000];
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let fields: Vec<&str> = line.split('#').next().unwrap().split_whitespace().collect();
        if let Some((first, bytes)) = fields.split_first() {
            let address = usize::from_str_radix(first.trim_end_matches(':'), 16).unwrap();
            let data = hex(&bytes.join(" "));
            rom[address..address + data.len()].copy_from_slice(&data);
        }
    }
    rom
}

#[test]
fn our_supervisor_program_lets_the_watchdog_run_out() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/inputs/pc10/watchdog.hex");
    let mut ours = bios(SPIN, RETN);
    ours.program = read_program(&path);
    let mut machine = PlayChoice::new(load(&pc10_image(&hex(COUNTING), None, 1)), ours, None);
    let mut drops = Vec::new(); // the blanks seen when each boot's hold dropped
    for boot in 1..=3u64 {
        let hold = if boot % 2 == 1 { NMI_EI } else { DOG_DI };
        while machine.supervisor.latch()[hold] == 0 {
            machine.step();
        }
        // Lines up, the program's own flip done: once a boot.
        let latch = *machine.supervisor.latch();
        assert!(latch[SDCS] != 0 && latch[DISP_MASK] != 0 && !machine.in_reset);
        assert_eq!(machine.monitor_on_game, boot % 2 == 1);
        assert_eq!(latch[GAME_STOP] as u64, boot % 2);
        let bus = &machine.supervisor.cpu.bus;
        assert!(bus.ram_8w[0] as u64 == boot && bus.video_ram[0] as u64 == boot);
        while machine.supervisor.latch()[hold] != 0 {
            machine.step();
        }
        drops.push(machine.supervisor.blanks);
        while machine.supervisor.watchdog_resets < boot {
            machine.step();
        }
        assert_eq!(machine.supervisor.blanks, drops[drops.len() - 1] + 8);
        assert!(*machine.supervisor.latch() == [0; 16] && machine.in_reset);
        assert!(machine.supervisor.screen_blank());
        assert_eq!(machine.monitor_on_game, boot % 2 == 1); // the reset flipped nothing
    }
    // Each boot is the hold (three or four blanks) and the eight after it.
    assert!(drops.windows(2).all(|w| (11..=12).contains(&(w[1] - w[0]))));
    assert!(ram10(&machine) != 0); // the game ran between resets
}

/// The reference's `test_a_save_state_taken_while_the_watchdog_counts`
/// (famicom-python, 2026-11-25): tests/pc10_watchdog.hex saved while the
/// counter runs (its hold dropped, `counted` blanks of the eight seen; odd
/// boots with the game running, even with it stopped) and loaded into a fresh
/// cabinet: both reset on the same blank and continue identically into the
/// next boot's hold.
#[test]
fn a_save_state_taken_while_the_watchdog_counts() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/inputs/pc10/watchdog.hex");
    let program = read_program(&path);
    let fresh = || {
        let mut ours = bios(SPIN, RETN);
        ours.program = program.clone();
        PlayChoice::new(load(&pc10_image(&hex(COUNTING), None, 1)), ours, None)
    };
    for counted in [0u32, 4, 7] {
        for boot in [1u64, 2] {
            let mut first = fresh();
            let hold = if boot % 2 == 1 { NMI_EI } else { DOG_DI };
            while first.supervisor.watchdog_resets < boot - 1 {
                first.step();
            }
            while first.supervisor.latch()[hold] == 0 {
                first.step();
            }
            while first.supervisor.latch()[hold] != 0 {
                first.step();
            }
            let dropped = first.supervisor.blanks;
            while first.supervisor.blanks < dropped + counted as u64 {
                first.step();
            }
            for _ in 0..37 {
                first.step(); // off the blank's own step
            }
            assert!(first.supervisor.watchdog_resets == boot - 1);
            assert_eq!(first.supervisor.latch()[hold], 0);
            assert_eq!(first.supervisor.watchdog, counted);
            let mut second = fresh();
            load_cabinet_state(&mut second, &save_cabinet_state(&first)).unwrap();
            assert_eq!(save_cabinet_state(&second), save_cabinet_state(&first));
            for machine in [&mut first, &mut second] {
                while machine.supervisor.watchdog_resets < boot {
                    machine.step();
                }
            }
            assert_eq!(first.supervisor.blanks, dropped + 8);
            assert_eq!(second.supervisor.blanks, dropped + 8);
            assert_eq!(first.supervisor.t(), second.supervisor.t());
            let next = if (boot + 1) % 2 == 1 { NMI_EI } else { DOG_DI };
            for machine in [&mut first, &mut second] {
                while machine.supervisor.latch()[next] == 0 {
                    machine.step();
                }
                machine.run_frames(2);
            }
            assert_eq!(save_cabinet_state(&second), save_cabinet_state(&first));
            assert_eq!(first.supervisor.picture(), second.supervisor.picture());
            assert_eq!(first.game_picture(), second.game_picture());
        }
    }
}

#[test]
fn cntrl_mask_reaches_the_panel() {
    let mut machine = cabinet(SPIN, JUMP);
    assert!(
        machine
            .game
            .board()
            .pad_as::<PlayChoicePad>(0)
            .unwrap()
            .masked
    );
    machine.supervisor_write_port(1, 1);
    assert!(
        !machine
            .game
            .board()
            .pad_as::<PlayChoicePad>(0)
            .unwrap()
            .masked
    );
}

#[test]
fn a_save_state_keeps_both_processors() {
    let mut first = cabinet(&hex(RELEASE), &hex(COUNTING));
    first.run_frames(3);
    first.insert_coin(0);
    first.supervisor.cpu.bus.dip = [0x40, 0x3F];
    let mut second = cabinet(&hex(RELEASE), &hex(COUNTING));
    load_cabinet_state(&mut second, &save_cabinet_state(&first)).unwrap();
    for machine in [&mut first, &mut second] {
        machine.run_frames(3);
    }
    let (one, two) = (&first.supervisor, &second.supervisor);
    assert_eq!(
        (one.t(), one.latch(), one.cpu.bus.dip, one.blanks),
        (two.t(), two.latch(), two.cpu.bus.dip, two.blanks)
    );
    assert_eq!(one.cpu.capture_state(), two.cpu.capture_state());
    assert_eq!(first.game.cpu.bus.t, second.game.cpu.bus.t);
    assert_eq!(first.game.cpu.bus.ram, second.game.cpu.bus.ram);
    assert_eq!(
        (first.in_reset, first.stopped),
        (second.in_reset, second.stopped)
    );
}

// -- the real BIOS and a game ------------------------------------------------

fn real() -> Option<PlayChoice> {
    if !bios_path().exists() {
        eprintln!("PlayChoice-10 BIOS not found (MAME_ROMPATH, FAMICOM_PC10_BIOS): skipped");
        return None;
    }
    let images = PathBuf::from(std::env::var("PC10_IMAGES_DIR").unwrap_or("pc10-images".into()));
    let path = images.join("Super Mario Bros..nes");
    if !path.exists() {
        eprintln!("PlayChoice-10 images not assembled (PC10_IMAGES_DIR): skipped");
        return None;
    }
    let data = std::fs::read(path).unwrap();
    let mut machine =
        PlayChoice::from_rom(&data, Some(load_bios("dual", None).unwrap()), None).unwrap();
    machine.supervisor.cpu.bus.dip = [0x40, 0x3F]; // MAME's defaults: 300 units a coin, one a second
    Some(machine)
}

#[test]
fn the_real_bios_starts_and_arms_its_interrupt() {
    let Some(mut machine) = real() else {
        return;
    };
    machine.run_seconds(2.5);
    let supervisor = &mut machine.supervisor;
    // "WAIT 10 Sec." is up, the blank interrupt has been re-armed every frame,
    // the watchdog has never fired, and the PROM has been clocked.
    assert!(supervisor.watchdog_resets == 0 && supervisor.blanks > 140);
    let picture = supervisor.picture();
    assert!(!supervisor.screen_blank() && picture.chunks(3).any(|p| p != &picture[..3]));
    assert!(supervisor
        .cpu
        .bus
        .prom
        .as_ref()
        .is_some_and(|prom| prom.counter != 0));
    assert!(machine.in_reset && !machine.game_shown());
}

#[test]
fn the_bios_checks_the_prom_runs_the_game_and_takes_it_away() {
    let Some(mut machine) = real() else {
        return;
    };
    machine.supervisor.cpu.bus.dip[1] = 0x05; // 60 units of time a second
    machine.run_seconds(11.7);
    // The PROM passed: the game is listed, released and showing its attract mode.
    assert!(!machine.in_reset && machine.game_shown() && machine.game.instructions > 1_000_000);
    machine.insert_coin(0);
    machine.run_seconds(1.5);
    assert!(machine.in_reset && !machine.game_shown()); // time bought: waiting for Enter
    machine.press(ENTER, 0.1);
    machine.run_seconds(2.0);
    let masked = |m: &mut PlayChoice| m.game.board().pad_as::<PlayChoicePad>(0).unwrap().masked;
    assert!(
        !machine.in_reset && machine.game_shown() && machine.sound_on() && !masked(&mut machine)
    );
    let before = machine.game.instructions;
    machine.run_seconds(1.0);
    assert!(machine.game.instructions > before && !machine.stopped);
    // 300 units at 60 a second: gone. Start is masked, the game stopped at one
    // of its own interrupts, its sound cut.
    machine.run_seconds(5.0);
    assert!(machine.stopped && masked(&mut machine) && !machine.sound_on());
    let frozen = machine.game.instructions;
    machine.run_seconds(1.0);
    assert!(machine.game.instructions == frozen && machine.supervisor.watchdog_resets == 0);
}

#[test]
fn the_bios_refuses_a_wrong_prom() {
    let Some(good) = real() else {
        return;
    };
    let images = PathBuf::from(std::env::var("PC10_IMAGES_DIR").unwrap_or("pc10-images".into()));
    let mut data = std::fs::read(images.join("Super Mario Bros..nes")).unwrap();
    let at = data.len() - 32;
    data[at] ^= 0x10; // one bit of the PROM's data
    let mut bad = PlayChoice::new(
        cartridge::load(&data, 12, None, false).unwrap(),
        good.supervisor.bios.clone(),
        None,
    );
    bad.supervisor.cpu.bus.dip = [0x40, 0x3F];
    bad.run_seconds(11.7);
    assert!(bad.in_reset && bad.game.instructions == 0 && !bad.game_shown());
}
