// SPDX-License-Identifier: MIT
//! A save state that fails to load leaves the console as it was, running.
//!
//! The failing state is made by the same ROM read as another board: its header
//! (format, version, the ROM's hash, the machine) passes, and the cartridge's part
//! does not decode as this cartridge's type. Before 2026-11-23 the board had already
//! moved its cartridge into the state's temporary PPU when that part failed, and
//! returned without it.

use famicom_rust::cartridge::BoardInfo;
use famicom_rust::machine::Machine;
use famicom_rust::state::{load_state, save_state, VERSION};
use famicom_rust::system::System;

/// A mapper 69 image whose program plays a 5B tone it keeps retuning, with
/// rendering on: 32 KiB of PRG (the last 8 KiB at $E000), 8 KiB of CHR.
fn image() -> Vec<u8> {
    let mut prg = vec![0xEAu8; 0x8000];
    #[rustfmt::skip]
    let program: &[u8] = &[
        0x78,                         // SEI
        0xA9, 0x07, 0x8D, 0x00, 0xC0, // LDA #7: STA $C000 (the mixer)
        0xA9, 0x3E, 0x8D, 0x00, 0xE0, // LDA #$3E: STA $E000 (tone A on)
        0xA9, 0x08, 0x8D, 0x00, 0xC0, // LDA #8: STA $C000
        0xA9, 0x0C, 0x8D, 0x00, 0xE0, // LDA #12: STA $E000 (volume 12)
        0xA9, 0x1E, 0x8D, 0x01, 0x20, // LDA #$1E: STA $2001 (rendering on)
        // loop:
        0xE6, 0x10,                   // INC $10
        0xA9, 0x00, 0x8D, 0x00, 0xC0, // LDA #0: STA $C000 (tone A's period)
        0xA5, 0x10, 0x8D, 0x00, 0xE0, // LDA $10: STA $E000
        0x4C, 0x1A, 0xE0,             // JMP loop
    ];
    prg[0x6000..0x6000 + program.len()].copy_from_slice(program);
    prg[0x7FFA..].copy_from_slice(&[0x00, 0xE0, 0x00, 0xE0, 0x00, 0xE0]);
    let mut data = vec![0x4E, 0x45, 0x53, 0x1A, 2, 1, 0x50, 0x40, 0, 0, 0, 0, 0, 0, 0, 0];
    data.extend(prg);
    data.extend((0..0x2000).map(|i| (i * 7) as u8));
    data
}

fn console(board: Option<BoardInfo>) -> System {
    let mut system = System::from_rom(&image(), Machine::famicom(), board).expect("the image loads");
    system.power();
    system
}

#[test]
fn a_failed_load_leaves_the_console_running_unchanged() {
    let mut system = console(None);
    let mut twin = console(None);
    system.run_frames(30);
    twin.run_frames(30);
    let mut other = console(Some(BoardInfo::new(0)));
    other.run_frames(10);
    let wrong = save_state(&other);

    let error = load_state(&mut system, &wrong).expect_err("an NROM state is not an FME-7's");
    assert!(!error.is_empty());
    assert_eq!(save_state(&system), save_state(&twin), "the failed load changed the console");
    system.run_frames(60);
    twin.run_frames(60);
    assert_eq!(system.take_samples(), twin.take_samples());
    assert_eq!(system.cycles(), twin.cycles());
    assert_eq!(save_state(&system), save_state(&twin));
    // It still takes a good state afterwards.
    let good = save_state(&twin);
    twin.run_frames(5);
    load_state(&mut twin, &good).expect("its own state loads");
    assert_eq!(save_state(&twin), save_state(&system));
}

#[test]
fn another_version_is_refused_before_decoding() {
    let mut system = console(None);
    system.run_frames(2);
    let mut state = save_state(&system);
    // The header follows the 8-byte magic: bincode's u32 version first.
    assert_eq!(state[8..12], VERSION.to_le_bytes());
    state[8..12].copy_from_slice(&1u32.to_le_bytes());
    let before = save_state(&system);
    let error = load_state(&mut system, &state).expect_err("version 1 is refused");
    assert!(error.contains("version 1"), "{error}");
    assert_eq!(save_state(&system), before);
}
