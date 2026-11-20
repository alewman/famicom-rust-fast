// SPDX-License-Identifier: MIT
//! Run a cartridge headless (the reference's `python -m famicom_python`):
//! prints how the cartridge was identified, runs it, and reports what a test
//! ROM says. `--png` saves the last frame; `--wav` the run's sound.
//!
//!     famicom ROM [--machine auto|KEY] [--frames N] [--blargg] [--png OUT] [--wav OUT] [--palette PAL]

use famicom_rust::testrom::{blargg_text, peek, run_blargg, screen_text, SIGNATURE};
use famicom_rust::video::{load_palette, palette_for, to_rgb, write_png};
use famicom_rust::{Machine, System};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Samples (about -1.0 to 1.0 after the analogue stage) as 16-bit PCM, clipped.
fn write_wav(path: &Path, samples: &[f64], rate: u32) -> std::io::Result<()> {
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|&s| {
            ((s * 32767.0).round_ties_even().clamp(-32768.0, 32767.0) as i16).to_le_bytes()
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    std::fs::write(path, out)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut rom, mut machine, mut frames, mut blargg) = (None, "auto".to_string(), 600u64, false);
    let (mut png, mut wav, mut palette) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--machine" => machine = args.next().unwrap(),
            "--frames" => frames = args.next().unwrap().parse().unwrap(),
            "--blargg" => blargg = true,
            "--png" => png = args.next().map(PathBuf::from),
            "--wav" => wav = args.next().map(PathBuf::from),
            "--palette" => palette = args.next().map(PathBuf::from),
            _ => rom = Some(PathBuf::from(arg)),
        }
    }
    let rom = rom.expect("usage: famicom ROM [options]");
    let data = famicom_rust::cartridge::read_rom_file(&rom, famicom_rust::cartridge::ROM_SUFFIXES)
        .unwrap();
    let machine = if machine == "auto" {
        System::machine_for(&data).unwrap()
    } else {
        Machine::by_key(&machine).expect("machine")
    };
    let mut system = System::from_rom(&data, machine.clone(), None).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let image = system.image.clone().unwrap();
    let board = &image.board;
    println!(
        "mapper {}.{}, mirroring {}, PRG {} KiB, CHR {} KiB ({})",
        board.mapper,
        board.submapper,
        board.mirroring,
        image.prg.len() / 1024,
        image.chr.len() / 1024,
        system.source
    );
    for line in system.disagreements.clone() {
        println!("  disagreement: {line}");
    }
    let start = Instant::now();
    if blargg {
        let result = run_blargg(&mut system, 120.0);
        let status = result
            .status
            .map_or("no result".to_string(), |s| format!("status {s}"));
        println!("{status}\n{}", result.text);
    } else {
        if let Some(wav) = &wav {
            let mut samples = Vec::new();
            for _ in 0..frames {
                system.run_frames(1);
                samples.extend(system.take_samples());
            }
            let rate = system.cpu.bus.apu.sample_rate() as u32;
            write_wav(wav, &samples, rate).unwrap();
            println!("{} samples written to {}", samples.len(), wav.display());
        } else {
            system.run_frames(frames);
        }
        if (0..3).all(|i| peek(&mut system, 0x6001 + i) == SIGNATURE[i as usize]) {
            println!(
                "$6000 = {:02X}\n{}",
                peek(&mut system, 0x6000),
                blargg_text(&mut system)
            );
        } else {
            println!("{}", screen_text(&mut system, 0));
        }
    }
    println!(
        "{} CPU cycles, {} instructions, {:.1} s",
        system.cycles(),
        system.instructions,
        start.elapsed().as_secs_f64()
    );
    if let Some(png) = png {
        let timing = &machine.timing;
        let path = palette.unwrap_or_else(|| palette_for(timing));
        let colours = load_palette(&path, timing.swapped_emphasis).unwrap();
        write_png(
            &png,
            &to_rgb(&system.cpu.bus.ppu.picture, &colours),
            256,
            240,
        )
        .unwrap();
        println!(
            "frame {} written to {}",
            system.cpu.bus.ppu.frame,
            png.display()
        );
    }
}
