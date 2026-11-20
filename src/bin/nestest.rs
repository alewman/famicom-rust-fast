// SPDX-License-Identifier: MIT
//! Rung 1: kevtris's nestest in automation mode against nestest.log, as the
//! reference's tests/test_nestest.py runs it: power on, the reset sequence,
//! PC = $C000, then per instruction PC, A, X, Y, P & $CF, SP, the CPU cycle
//! count and the PPU's line and dot; then the result codes at $02/$03.
//!
//!     nestest ROMS_DIR   (the directory holding other/nestest.nes and .log)

use famicom_rust::{Machine, System};
use std::path::PathBuf;

fn field(line: &str, key: &str) -> u32 {
    let at = line
        .find(key)
        .unwrap_or_else(|| panic!("{key} missing in {line}"))
        + key.len();
    let rest = line[at..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(rest.len());
    u32::from_str_radix(&rest[..end], if key == "CYC:" { 10 } else { 16 }).unwrap()
}

fn decimal_after(line: &str, key: &str) -> (u32, u32) {
    let at = line.find(key).unwrap() + key.len();
    let rest = &line[at..];
    let mut parts = rest.split(',');
    let a = parts.next().unwrap().trim().parse().unwrap();
    let b: String = parts
        .next()
        .unwrap()
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (a, b.parse().unwrap())
}

fn main() {
    let roms = PathBuf::from(std::env::args().nth(1).expect("usage: nestest ROMS_DIR"));
    let rom = std::fs::read(roms.join("other/nestest.nes")).expect("nestest.nes");
    let log = std::fs::read_to_string(roms.join("other/nestest.log")).expect("nestest.log");
    let mut system = System::from_rom(&rom, Machine::nes_001(), None).expect("load");
    system.step();
    system.cpu.pc = 0xC000;
    let mut failures = 0;
    let mut lines = 0;
    for (number, text) in log.lines().enumerate() {
        lines += 1;
        let cpu = &system.cpu;
        let ppu = &cpu.bus.ppu;
        let expected = (
            u32::from_str_radix(&text[..4], 16).unwrap(),
            field(text, "A:"),
            field(text, "X:"),
            field(text, "Y:"),
            field(text, "P:") & 0xCF,
            field(text, "SP:"),
            field(text, "CYC:"),
            decimal_after(text, "PPU:"),
        );
        let actual = (
            cpu.pc as u32,
            cpu.a as u32,
            cpu.x as u32,
            cpu.y as u32,
            cpu.p as u32 & 0xCF,
            cpu.s as u32,
            cpu.bus.cycles() as u32,
            (ppu.line as u32, ppu.dot as u32),
        );
        if expected != actual {
            failures += 1;
            if failures <= 5 {
                println!(
                    "line {}: expected {expected:?}\n          got {actual:?}",
                    number + 1
                );
            }
        }
        system.step();
    }
    let codes = (system.cpu.bus.ram[2], system.cpu.bus.ram[3]);
    println!("{lines} lines, {failures} differ; result codes {codes:02X?}");
    if failures != 0 || codes != (0, 0) || lines != 8991 {
        std::process::exit(1);
    }
}
