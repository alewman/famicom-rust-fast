//! Save states, proven complete as the reference proves its own
//! (famicom-python `scripts/state_roundtrip.py`): a run saved at an
//! instruction boundary and loaded, into a fresh system and into one that has
//! run elsewhere first, continues as the run that was not interrupted: every
//! CPU-bus cycle, the picture, the audio, the registers, the master clock.
//!
//!     famicom-state-roundtrip ROM... [--machine KEY] [--at A] [--to B] [--input FILE] [--seed S]

use famicom_rust::joypad::StandardController;
use famicom_rust::state::{load_snapshot, load_state, save_snapshot, save_state};
use famicom_rust::{Machine, System};
use std::collections::HashMap;
use std::path::Path;

fn digest(system: &mut System) -> u64 {
    let mut hasher = crc32fast::Hasher::new();
    let samples = system.take_samples();
    let board = &system.cpu.bus;
    for packed in board.bus_log.as_ref().unwrap() {
        hasher.update(&packed.to_le_bytes());
    }
    for value in &board.ppu.picture {
        hasher.update(&value.to_le_bytes());
    }
    for sample in &samples {
        hasher.update(&sample.to_le_bytes());
    }
    let cpu = &system.cpu;
    hasher.update(&[cpu.a, cpu.x, cpu.y, cpu.s, cpu.p]);
    hasher.update(&cpu.pc.to_le_bytes());
    hasher.update(&board.ppu.v.to_le_bytes());
    hasher.update(&[board.ppu.status]);
    hasher.update(&board.t.to_le_bytes());
    ((samples.len() as u64) << 32) | hasher.finalize() as u64
}

fn run(
    system: &mut System,
    frames: std::ops::Range<u64>,
    inputs: &HashMap<u64, Vec<(usize, u8)>>,
) -> Vec<u64> {
    let mut out = Vec::new();
    for frame in frames {
        for &(pad, buttons) in inputs.get(&frame).into_iter().flatten() {
            if let Some(pad) = system.board().pads[pad]
                .as_any_mut()
                .downcast_mut::<StandardController>()
            {
                pad.buttons = buttons;
            }
        }
        system.board().bus_log.as_mut().unwrap().clear();
        system.run_frames(1);
        out.push(digest(system));
    }
    out
}

fn fresh(data: &[u8], machine: &Machine) -> Result<System, String> {
    let mut system = System::from_rom(data, machine.clone(), None)?;
    system.board().bus_log = Some(Vec::new());
    Ok(system)
}

/// A small deterministic generator for the save point (not the reference's).
fn spread(seed: u64, rom: &str) -> i64 {
    let mut x = seed
        ^ rom.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01B3)
        });
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x % 20_000) as i64
}

fn check(
    rom: &str,
    machine: &Machine,
    at: u64,
    to: u64,
    inputs: &HashMap<u64, Vec<(usize, u8)>>,
    seed: u64,
) -> Result<String, String> {
    let data = famicom_rust::cartridge::read_rom_file(
        Path::new(rom),
        famicom_rust::cartridge::ROM_SUFFIXES,
    )?;
    let extra = spread(seed, rom);
    let mut first = fresh(&data, machine)?;
    run(&mut first, 0..at, inputs);
    first.run_cycles(extra);
    let state = save_state(&first);
    let expected = run(&mut first, at + 1..to, inputs);

    let mut second = fresh(&data, machine)?;
    load_state(&mut second, &state)?;
    let got = run(&mut second, at + 1..to, inputs);

    let mut third = fresh(&data, machine)?;
    run(&mut third, 0..at / 2, inputs);
    load_state(&mut third, &state)?;
    let got3 = run(&mut third, at + 1..to, inputs);

    // Running ahead, as a frontend does it with the snapshot pair: before every
    // frame, snapshot, run two frames nobody keeps, and come back.
    let mut fourth = fresh(&data, machine)?;
    load_state(&mut fourth, &state)?;
    let mut got4 = Vec::new();
    for frame in at + 1..to {
        let snapshot = save_snapshot(&fourth);
        fourth.run_frames(2);
        fourth.take_samples();
        load_snapshot(&mut fourth, &snapshot)?;
        got4.extend(run(&mut fourth, frame..frame + 1, inputs));
    }
    if let Some(i) = expected.iter().zip(&got4).position(|(a, b)| a != b) {
        return Ok(format!(
            "differs at frame {} (snapshot, two frames ahead and back, every frame)",
            at + 1 + i as u64
        ));
    }

    for (i, ((a, b), c)) in expected.iter().zip(&got).zip(&got3).enumerate() {
        let frame = at + 1 + i as u64;
        if a != b {
            return Ok(format!(
                "differs at frame {frame} (loaded into a fresh system)"
            ));
        }
        if a != c {
            return Ok(format!(
                "differs at frame {frame} (loaded into a system that had run)"
            ));
        }
    }
    Ok(format!(
        "identical: frames {}-{} after a save at frame {at} + {extra} cycles ({} bytes)",
        at + 1,
        to - 1,
        state.len()
    ))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut roms, mut machine, mut at, mut to, mut input, mut seed) =
        (Vec::new(), "nes".to_string(), 120u64, 240u64, None, 1u64);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--machine" => machine = args.next().unwrap(),
            "--at" => at = args.next().unwrap().parse().unwrap(),
            "--to" => to = args.next().unwrap().parse().unwrap(),
            "--input" => input = args.next(),
            "--seed" => seed = args.next().unwrap().parse().unwrap(),
            _ => roms.push(arg),
        }
    }
    let mut inputs: HashMap<u64, Vec<(usize, u8)>> = HashMap::new();
    if let Some(path) = input {
        for text in std::fs::read_to_string(path).unwrap().lines() {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 3 && !text.starts_with('#') {
                inputs.entry(parts[0].parse().unwrap()).or_default().push((
                    parts[1].parse().unwrap(),
                    u8::from_str_radix(parts[2], 16).unwrap(),
                ));
            }
        }
    }
    let machine = Machine::by_key(&machine).expect("machine");
    let mut failed = 0;
    for rom in &roms {
        let verdict =
            check(rom, &machine, at, to, &inputs, seed).unwrap_or_else(|e| format!("error: {e}"));
        if !verdict.starts_with("identical") {
            failed += 1;
        }
        println!("{rom}: {verdict}");
    }
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
