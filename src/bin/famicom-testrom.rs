// SPDX-License-Identifier: MIT
//! Rungs 3-4's Rust side: run the reference's test ROM cases on this board.
//!
//! Reads the JSON lines scripts/testrom_outcomes.py writes (each case with
//! the board the reference's `run_case` builds) and writes one JSON line per
//! case with this board's outcome; scripts/testrom_compare.py compares them.
//!
//!     famicom-testrom CASES.jsonl --roms DIR --out FILE [--jobs N] [--only TEXT]

use famicom_rust::cartridge::BoardInfo;
use famicom_rust::testrom::{run_case, RomCase};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn board(value: &Value) -> BoardInfo {
    let n = |k: &str| value[k].as_u64().unwrap_or(0);
    BoardInfo {
        mapper: n("mapper") as u16,
        submapper: n("submapper") as u8,
        mirroring: value["mirroring"]
            .as_str()
            .unwrap_or("H")
            .chars()
            .next()
            .unwrap(),
        battery: value["battery"].as_bool().unwrap_or(false),
        prg_ram: n("prg_ram") as usize,
        prg_nvram: n("prg_nvram") as usize,
        chr_ram: n("chr_ram") as usize,
        chr_nvram: n("chr_nvram") as usize,
        timing: n("timing") as u8,
        console: n("console") as u8,
        expansion: n("expansion") as u8,
    }
}

fn case(value: &Value) -> RomCase {
    let s = |k: &str| value[k].as_str().unwrap_or("").to_string();
    RomCase {
        path: s("path"),
        protocol: s("protocol"),
        seconds: value["seconds"].as_f64().unwrap(),
        buttons: value["buttons"].as_u64().unwrap_or(0) as u8,
        passed: s("passed"),
        failed: s("failed"),
        expect: value["expect"]
            .as_array()
            .map(|a| a.iter().map(|e| e.as_str().unwrap().to_string()).collect())
            .unwrap_or_default(),
        font: s("font"),
        sprites: value["sprites"].as_bool().unwrap_or(false),
        machine: s("machine"),
        board: board(&value["resolved_board"]),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut cases = None;
    let mut roms = None;
    let mut out = None;
    let mut jobs = 4usize;
    let mut only = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--roms" => roms = args.next(),
            "--out" => out = args.next(),
            "--jobs" => jobs = args.next().unwrap().parse().unwrap(),
            "--only" => only = args.next(),
            _ => cases = Some(arg),
        }
    }
    let rows: Vec<Value> = std::fs::read_to_string(cases.expect("CASES.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|r: &Value| {
            r["resolved_board"].get("error").is_none()
                && only
                    .as_ref()
                    .is_none_or(|o| r["label"].as_str().unwrap().contains(o.as_str()))
        })
        .collect();
    let roms = PathBuf::from(roms.expect("--roms"));
    let out = Arc::new(Mutex::new(
        std::fs::File::create(out.expect("--out")).unwrap(),
    ));
    let queue = Arc::new(Mutex::new(rows.into_iter().enumerate().collect::<Vec<_>>()));
    let mut handles = Vec::new();
    for _ in 0..jobs {
        let queue = Arc::clone(&queue);
        let out = Arc::clone(&out);
        let roms = roms.clone();
        handles.push(std::thread::spawn(move || loop {
            let Some((index, row)) = queue.lock().unwrap().pop() else {
                break;
            };
            let result = run_case(&case(&row), &roms);
            let line = match result {
                Ok(o) => json!({"index": index, "label": row["label"], "suite": row["suite"],
                    "protocol": row["protocol"],
                    "outcome": {"passed": o.passed, "detail": o.detail, "cycles": o.cycles,
                                "instructions": o.instructions}}),
                Err(e) => json!({"index": index, "label": row["label"], "error": e}),
            };
            let mut out = out.lock().unwrap();
            writeln!(out, "{line}").unwrap();
            out.flush().unwrap();
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
}
