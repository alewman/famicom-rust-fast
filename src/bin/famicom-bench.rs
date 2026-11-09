//! The benchmark: run cartridges headless for N frames each and print the
//! frames per second, with a checksum of every picture, every audio sample
//! and the CPU's cycle count, so a faster build that changes the output is
//! caught here too.
//!
//!     famicom-bench ROM [...] [--frames N] [--input FILE]
//!
//! Each runs on the console it asks for (`System::machine_for`), with the
//! lockstep gate's start-mash input (Start, then A, now and then) so a game
//! leaves its title screen. The time is this thread's time on the CPU
//! (`/proc/thread-self/schedstat`, nanoseconds run), not the wall clock: on a
//! shared machine the wall clock counts the time other processes had the CPU.

use famicom_rust::joypad::StandardController;
use famicom_rust::System;
use std::collections::HashMap;
use std::path::Path;

/// Nanoseconds this thread has run on a CPU.
fn cpu_ns() -> u64 {
    let text = std::fs::read_to_string("/proc/thread-self/schedstat").expect("schedstat");
    text.split_whitespace().next().unwrap().parse().unwrap()
}

fn main() {
    let mut frames = 600u64;
    let mut input = format!(
        "{}/scripts/inputs/start-mash.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut roms = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().expect("--frames N").parse().expect("a number"),
            "--input" => input = args.next().expect("--input FILE"),
            _ => roms.push(arg),
        }
    }
    let mut presses: HashMap<u64, u8> = HashMap::new();
    for line in std::fs::read_to_string(&input).unwrap().lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 3 && !line.starts_with('#') && parts[1] == "0" {
            presses.insert(
                parts[0].parse().unwrap(),
                u8::from_str_radix(parts[2], 16).unwrap(),
            );
        }
    }
    for rom in &roms {
        let data = famicom_rust::cartridge::read_rom_file(
            Path::new(rom),
            famicom_rust::cartridge::ROM_SUFFIXES,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let machine = System::machine_for(&data).unwrap_or_else(|e| panic!("{e}"));
        let mut system = System::from_rom(&data, machine, None).unwrap_or_else(|e| panic!("{e}"));
        let mut check = crc32fast::Hasher::new();
        let mut bytes: Vec<u8> = Vec::with_capacity(1 << 18);
        let began = cpu_ns();
        for frame in 0..frames {
            if let Some(&buttons) = presses.get(&frame) {
                if let Some(pad) = system.board().pads[0]
                    .as_any_mut()
                    .downcast_mut::<StandardController>()
                {
                    pad.buttons = buttons;
                }
            }
            system.run_frames(1);
            // The picture and the samples as bytes, one CRC update each (an
            // update per value cost about a tenth of the run).
            bytes.clear();
            for value in &system.cpu.bus.ppu.picture {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for sample in system.take_samples() {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            check.update(&bytes);
        }
        let seconds = (cpu_ns() - began) as f64 / 1e9;
        check.update(&system.cycles().to_le_bytes());
        let name = Path::new(rom).file_stem().unwrap().to_string_lossy();
        println!(
            "{name}: {frames} frames in {seconds:.3} s, {:.1} frames/s, check {:08X}",
            frames as f64 / seconds,
            check.finalize()
        );
    }
}
