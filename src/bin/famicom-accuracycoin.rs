// SPDX-License-Identifier: MIT
//! Rung 4: all of AccuracyCoin, as the reference's scripts/accuracycoin.py
//! runs it (Start on the page index, every result byte read), with the test
//! list the reference reads from the pinned .asm (scripts/accuracycoin_tests.py).
//!
//!     famicom-accuracycoin ACCURACYCOIN.nes TESTS.json [--failures] [--seconds S]

use famicom_rust::testrom::run_accuracycoin;
use famicom_rust::{Machine, System};
use serde_json::Value;

fn verdict(value: u8) -> String {
    if value == 0xFF {
        return "skipped".into();
    }
    const CODES: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let code = if ((value >> 2) as usize) < CODES.len() {
        CODES[(value >> 2) as usize] as char
    } else {
        '?'
    };
    match value & 3 {
        1 => "pass".into(),
        2 => format!("FAIL {code}"),
        _ => format!("no result (${value:02X})"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let failures = args.iter().any(|a| a == "--failures");
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .map_or(300.0, |i| args[i + 1].parse().unwrap());
    let tests: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let addresses: Vec<u16> = tests
        .iter()
        .map(|t| t["address"].as_u64().unwrap() as u16)
        .collect();
    let mut system = System::from_path(std::path::Path::new(&args[0]), Machine::nes_001()).unwrap();
    let results = run_accuracycoin(&mut system, &addresses, seconds);
    println!("| Page | Test | Result |");
    println!("| --- | --- | --- |");
    let mut passed = 0;
    for (test, &value) in tests.iter().zip(&results) {
        let text = verdict(value);
        if value != 0xFF && value & 3 == 1 {
            passed += 1;
        }
        if !failures || text != "pass" {
            println!(
                "| {} | {} | {text} |",
                test["page"].as_str().unwrap(),
                test["name"].as_str().unwrap()
            );
        }
    }
    println!(
        "\n{passed} of {} pass; {} CPU cycles, {} frames",
        tests.len(),
        system.cycles(),
        system.cpu.bus.ppu.frame
    );
    std::process::exit(if passed == tests.len() { 0 } else { 1 });
}
