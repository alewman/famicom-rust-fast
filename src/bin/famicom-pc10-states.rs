// SPDX-License-Identifier: MIT
//! PlayChoice-10 save states, proven as `famicom-state-roundtrip` proves the
//! console's: a cabinet run saved between two steps and loaded, into a fresh
//! cabinet and into one that has run elsewhere first, continues as the run that
//! was not interrupted: every bus access of both processors, both pictures, the
//! audio, the registers and both clocks.
//!
//!     famicom-pc10-states IMAGE... [--bios NAME] [--at A] [--to B] [--input FILE] [--seed S]
//!
//! FILE is a `--pc10` input script of the lockstep tracer (`FRAME coin SLOT`,
//! `FRAME button NAME 0|1`, `FRAME dip HEX HEX`, `FRAME PAD BUTTONS`); the gun's
//! lines are not taken. The save lands at frame A plus a number of the cabinet's
//! steps that depends on the seed and the image. The reference saves cabinets
//! too (`state.save_state` of a `supervisor.PlayChoice`); its test
//! `test_a_save_state_keeps_both_processors` is transcribed in tests/playchoice.rs.

use famicom_rust::playchoice::{PlayChoiceGun, PlayChoicePad};
use famicom_rust::state::{load_cabinet_state, save_cabinet_state};
use famicom_rust::supervisor::{self as sv, Bios, PlayChoice};
use std::collections::HashMap;
use std::path::Path;

type Inputs = HashMap<u64, Vec<Vec<String>>>;

fn digest(cabinet: &mut PlayChoice) -> u64 {
    let mut hasher = crc32fast::Hasher::new();
    let samples = cabinet.take_samples();
    for packed in cabinet.supervisor.cpu.bus.bus_log.as_ref().unwrap() {
        hasher.update(&packed.to_le_bytes());
    }
    for packed in cabinet.game.cpu.bus.bus_log.as_ref().unwrap() {
        hasher.update(&packed.to_le_bytes());
    }
    for value in cabinet.game_picture() {
        hasher.update(&value.to_le_bytes());
    }
    hasher.update(&cabinet.supervisor.picture());
    for sample in &samples {
        hasher.update(&sample.to_le_bytes());
    }
    let cpu = &cabinet.game.cpu;
    hasher.update(&[cpu.a, cpu.x, cpu.y, cpu.s, cpu.p]);
    hasher.update(&cpu.pc.to_le_bytes());
    hasher.update(&cabinet.game.cpu.bus.t.to_le_bytes());
    let z80 = cabinet.supervisor.cpu.capture_state();
    hasher.update(format!("{z80:?}").as_bytes());
    hasher.update(&cabinet.supervisor.t().to_le_bytes());
    hasher.update(cabinet.supervisor.latch());
    hasher.update(&[
        cabinet.in_reset as u8,
        cabinet.stopped as u8,
        cabinet.nmi as u8,
        cabinet.monitor_on_game as u8,
    ]);
    ((samples.len() as u64) << 32) | hasher.finalize() as u64
}

fn apply(cabinet: &mut PlayChoice, lines: &[Vec<String>]) {
    for line in lines {
        let rest = &line[1..];
        match line[0].as_str() {
            "coin" => cabinet.insert_coin(rest[0].parse().unwrap()),
            "button" => {
                let bit = match rest[0].as_str() {
                    "CHANNEL" => sv::CHANNEL,
                    "ENTER" => sv::ENTER,
                    "RESET" => sv::RESET,
                    _ => sv::SERVICE,
                };
                let bus = &mut cabinet.supervisor.cpu.bus;
                if rest[1] == "1" {
                    bus.buttons |= bit;
                } else {
                    bus.buttons &= !bit;
                }
            }
            "dip" => {
                cabinet.supervisor.cpu.bus.dip = [
                    u8::from_str_radix(&rest[0], 16).unwrap(),
                    u8::from_str_radix(&rest[1], 16).unwrap(),
                ]
            }
            pad => {
                let held = u8::from_str_radix(&rest[0], 16).unwrap();
                let device = cabinet.game.board().pads[pad.parse::<usize>().unwrap()].as_any_mut();
                if let Some(gun) = device.downcast_mut::<PlayChoiceGun>() {
                    gun.pad.buttons = held;
                } else if let Some(pad) = device.downcast_mut::<PlayChoicePad>() {
                    pad.buttons = held;
                }
            }
        }
    }
}

fn run(cabinet: &mut PlayChoice, frames: std::ops::Range<u64>, inputs: &Inputs) -> Vec<u64> {
    let mut out = Vec::new();
    for frame in frames {
        if let Some(lines) = inputs.get(&frame) {
            apply(cabinet, lines);
        }
        cabinet.supervisor.cpu.bus.bus_log.as_mut().unwrap().clear();
        cabinet.game.board().bus_log.as_mut().unwrap().clear();
        cabinet.run_frames(1);
        out.push(digest(cabinet));
    }
    out
}

fn fresh(data: &[u8], bios: &Bios) -> Result<PlayChoice, String> {
    let mut cabinet = PlayChoice::from_rom(data, Some(bios.clone()), None)?;
    cabinet.supervisor.cpu.bus.bus_log = Some(Vec::new());
    cabinet.game.board().bus_log = Some(Vec::new());
    Ok(cabinet)
}

/// A small deterministic generator for the save point.
fn spread(seed: u64, rom: &str) -> u64 {
    let mut x = seed
        ^ rom.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01B3)
        });
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x % 20_000
}

fn check(
    rom: &str,
    bios: &Bios,
    at: u64,
    to: u64,
    inputs: &Inputs,
    seed: u64,
) -> Result<String, String> {
    let data = famicom_rust::cartridge::read_rom_file(
        Path::new(rom),
        famicom_rust::cartridge::ROM_SUFFIXES,
    )?;
    let extra = spread(seed, rom);
    let mut first = fresh(&data, bios)?;
    run(&mut first, 0..at, inputs);
    for _ in 0..extra {
        first.step();
    }
    let state = save_cabinet_state(&first);
    let expected = run(&mut first, at + 1..to, inputs);

    let mut second = fresh(&data, bios)?;
    load_cabinet_state(&mut second, &state)?;
    let got = run(&mut second, at + 1..to, inputs);

    let mut third = fresh(&data, bios)?;
    run(&mut third, 0..at / 2, inputs);
    load_cabinet_state(&mut third, &state)?;
    let got3 = run(&mut third, at + 1..to, inputs);

    for (i, ((a, b), c)) in expected.iter().zip(&got).zip(&got3).enumerate() {
        let frame = at + 1 + i as u64;
        if a != b {
            return Ok(format!(
                "differs at frame {frame} (loaded into a fresh cabinet)"
            ));
        }
        if a != c {
            return Ok(format!(
                "differs at frame {frame} (loaded into a cabinet that had run)"
            ));
        }
    }
    Ok(format!(
        "identical: frames {}-{} after a save at frame {at} + {extra} steps ({} bytes)",
        at + 1,
        to - 1,
        state.len()
    ))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut roms, mut bios, mut at, mut to, mut input, mut seed) =
        (Vec::new(), "dual".to_string(), 900u64, 1000u64, None, 1u64);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bios" => bios = args.next().unwrap(),
            "--at" => at = args.next().unwrap().parse().unwrap(),
            "--to" => to = args.next().unwrap().parse().unwrap(),
            "--input" => input = args.next(),
            "--seed" => seed = args.next().unwrap().parse().unwrap(),
            _ => roms.push(arg),
        }
    }
    let mut inputs: Inputs = HashMap::new();
    if let Some(path) = input {
        for text in std::fs::read_to_string(path).unwrap().lines() {
            let parts: Vec<String> = text.split_whitespace().map(str::to_string).collect();
            if parts.len() < 3
                || text.starts_with('#')
                || matches!(parts[1].as_str(), "aim" | "trigger")
            {
                continue;
            }
            inputs
                .entry(parts[0].parse().unwrap())
                .or_default()
                .push(parts[1..].to_vec());
        }
    }
    let bios = sv::load_bios(&bios, None).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let mut failed = 0;
    for rom in &roms {
        let verdict =
            check(rom, &bios, at, to, &inputs, seed).unwrap_or_else(|e| format!("error: {e}"));
        if !verdict.starts_with("identical") {
            failed += 1;
        }
        println!("{rom}: {verdict}");
    }
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
