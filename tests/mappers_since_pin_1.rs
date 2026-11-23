// SPDX-License-Identifier: MIT
//! The reference's unit tests for what it added after `famicom-rust-pin-1`, on
//! synthetic images: the Sunsoft 5B's sound (tests/test_expansion_audio.py),
//! mapper 559 and mapper 208's protection against the game's own jumps
//! (tests/test_mappers_remaining.py), transcribed.

use famicom_rust::cartridge::chinese::Subor559;
use famicom_rust::cartridge::mmc3_clones::Gouder208;
use famicom_rust::cartridge::sunsoft::{Fme7, SUNSOFT_5B_GAIN};
use famicom_rust::cartridge::{BoardInfo, Cartridge, RomImage};

const TICKS: i64 = 12; // master ticks per CPU cycle

/// PRG and CHR whose every 1 KiB starts with its own bank number (test_mappers.py's `image`).
fn image(mapper: u16, prg_kib: usize, chr_kib: usize) -> RomImage {
    RomImage {
        header: Vec::new(),
        trainer: Vec::new(),
        prg: (0..prg_kib * 1024).map(|i| (i / 0x400) as u8).collect(),
        chr: (0..chr_kib * 1024).map(|i| (i / 0x400) as u8).collect(),
        misc: Vec::new(),
        board: BoardInfo {
            prg_ram: 0x2000,
            ..BoardInfo::new(mapper)
        },
        nes2: true,
    }
}

fn offset(cart: &Subor559, address: u16) -> usize {
    cart.b.prg_offsets[((address >> 13) & 3) as usize] + (address as usize & 0x1FFF)
}

/// Sunsoft_5B_audio.wiki: a tone flips every 16 CPU cycles times its period,
/// and volume 12 is the DAC's code 25 (2v + 1).
#[test]
fn test_5b_tone_period_and_volume() {
    let mut cart = Fme7::new(image(69, 128, 128), TICKS);
    for (register, value) in [(0, 2), (1, 0), (7, 0x3E), (8, 12)] {
        cart.cpu_write(0xC000, register, 0);
        cart.cpu_write(0xE000, value, 0);
    }
    let mut seen = Vec::new();
    for i in 0..400 {
        cart.m2(i * TICKS);
        seen.push(cart.expansion());
    }
    let high = ay8910_rust::SUNSOFT_5B.dac[25] * SUNSOFT_5B_GAIN;
    assert!(seen[100..].iter().all(|&v| v == 0.0 || v == high));
    assert!(seen[100..].contains(&0.0) && seen[100..].contains(&high));
    let flips: Vec<usize> = (101..400).filter(|&i| seen[i] != seen[i - 1]).collect();
    assert!(flips.windows(2).all(|w| w[1] - w[0] == 32), "{flips:?}");
}

/// A $C000 byte with its high nibble set selects nothing: the write is lost.
#[test]
fn test_5b_select_above_15_deselects_the_chip() {
    let mut cart = Fme7::new(image(69, 128, 128), TICKS);
    cart.cpu_write(0xC000, 0x18, 0);
    cart.cpu_write(0xE000, 15, 0);
    cart.cpu_write(0xC000, 0x07, 0);
    cart.cpu_write(0xE000, 0x3F, 0); // all tones and noise off: the levels show
    for i in 0..64 {
        cart.m2(i * TICKS);
    }
    assert_eq!(cart.expansion(), 0.0);
}

#[test]
fn test_mapper_559_banks_and_nametables() {
    let mut cart = Subor559::new(image(559, 256, 256), TICKS);
    assert_eq!(offset(&cart, 0xE000), 31 * 0x2000); // the last bank fixed
    cart.cpu_write(0x8000, 5, 0);
    cart.cpu_write(0xA000, 0xC0 | 23, 0); // the game's ORed value
    cart.cpu_write(0x9C00, 13, 0);
    let banks: Vec<usize> = cart.b.prg_offsets.iter().map(|o| o / 0x2000).collect();
    assert_eq!(banks, [5, 23, 13, 31]);
    for slot in 0..8u16 {
        // each bank has two addresses, either sets it
        cart.cpu_write(0xB000 + slot * 0x800 + (slot & 1) * 0x400, 0x40 + slot as u8, 0);
    }
    let chr: Vec<usize> = cart.b.chr_offsets.iter().map(|o| o / 0x400).collect();
    assert_eq!(chr, (0..8).map(|s| 0x40 + s).collect::<Vec<usize>>());
    for (i, value) in [0xE1, 0xE0, 0xE1, 0xE0].into_iter().enumerate() {
        cart.cpu_write(0x9C0C + i as u16, value, 0);
    }
    let a10: Vec<Option<u8>> = (0..4).map(|i| cart.ciram_a10(0x2000 + 0x400 * i)).collect();
    assert_eq!(a10, [Some(1), Some(0), Some(1), Some(0)]);
    cart.cpu_write(0x9C01, 7, 0); // an unused $9C0x
    assert_eq!(cart.b.prg_offsets[2], 13 * 0x2000);
    assert_eq!(cart.nametables, [1, 0, 1, 0]);
}

#[test]
fn test_mapper_559_counter_steps_once_a_scanline() {
    let mut cart = Subor559::new(image(559, 256, 256), TICKS);
    cart.cpu_write(0xF000, 0xFE, 0); // two steps to the wrap
    cart.cpu_write(0xF400, 0xFE, 0);
    cart.cpu_write(0xF800, 2, 0);
    let mut cycles = 0;
    while !cart.irq() {
        cart.m2(0);
        cycles += 1;
        assert!(cycles < 1000);
    }
    assert_eq!(cycles, 228); // two steps of 341/3: 113.67 then 227.33, so the 228th cycle
    assert_eq!(cart.counter, 0);
    cart.cpu_write(0xF800, 0, 0); // stop and acknowledge
    assert!(!cart.irq());
    for _ in 0..500 {
        cart.m2(0);
    }
    assert!(!cart.irq() && cart.counter == 0);
}

/// Kuai Da Chuanshuo - Street Fighter IV's jumps through mapper 208's protection, read
/// from the game running (the reference, 2026-11-14): the index it writes to $5000, its
/// writes to $5800-$5802, and what it then executes at $5800.
#[allow(clippy::type_complexity)]
const GAME_208_JUMPS: &[(u8, &[(u16, u8)], &[(u16, u8)])] = &[
    (0x8E, &[(0, 0x54)], &[(0, 0x4C)]),                     // the opcode: JMP
    (0xB3, &[(1, 0x6D), (2, 0x98)], &[(1, 0x34), (2, 0xC1)]), // JMP $C134
    (0x09, &[(1, 0x44), (2, 0xAD)], &[(1, 0x0D), (2, 0xE4)]), // JMP $E40D
    (0x62, &[(1, 0x55), (2, 0xB4)], &[(1, 0x15), (2, 0xF4)]), // JMP $F415
    (0x0D, &[(1, 0x62), (2, 0xAA)], &[(1, 0x2B), (2, 0xE3)]), // JMP $E32B
];

#[test]
fn test_mapper_208_protection_gives_the_games_jumps() {
    let mut cart = Gouder208::new(image(208, 128, 128), TICKS);
    for &(index, written, expected) in GAME_208_JUMPS {
        cart.cpu_write(0x5000, index, 0);
        for &(register, value) in written {
            cart.cpu_write(0x5800 + register, value, 0);
        }
        for &(register, value) in expected {
            assert_eq!(cart.cpu_read(0x5800 + register, 0), Some(value as u16));
        }
    }
}
