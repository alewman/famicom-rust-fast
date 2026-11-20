// SPDX-License-Identifier: MIT
//! Reading test ROMs' verdicts (`testrom.py`): blargg's $6000 protocol, the
//! screen, Holy Mapperel, values in the test ROMs' fonts, RAM, pixel spans,
//! and AccuracyCoin.
//!
//! The cases themselves live in the reference (its RomCase tables); the
//! gate exports them with the board the reference's `run_case` builds
//! (scripts/testrom_outcomes.py), so [`run_case`] takes that board as given.
//! Peeking reads the cartridge and memories directly, with no bus cycle, as
//! the reference does (the cartridge's `cpu_read` is called, side effects
//! and all).

use crate::cartridge::{self, BoardInfo};
use crate::joypad::StandardController;
use crate::machine::Machine;
use crate::system::System;
use std::path::Path;

pub const SIGNATURE: [u16; 3] = [0xDE, 0xB0, 0x61];
pub const RUNNING: u16 = 0x80;
pub const RESET_REQUEST: u16 = 0x81;

/// The byte at a CPU address in RAM or on the cartridge, without a bus cycle.
pub fn peek(system: &mut System, address: u16) -> u16 {
    let board = &mut system.cpu.bus;
    if address < 0x2000 {
        return board.ram[address as usize & 0x7FF] as u16;
    }
    let t = board.t;
    board.ppu.cart.cpu_read(address, t).unwrap_or(0)
}

/// Python's `chr` of a peeked value.
fn chr(value: u32) -> char {
    char::from_u32(value).unwrap_or('\u{FFFD}')
}

pub fn blargg_text(system: &mut System) -> String {
    let mut out = String::new();
    for address in 0x6004u16..0x8000 {
        let value = peek(system, address);
        if value == 0 {
            break;
        }
        out.push(chr(value as u32));
    }
    out
}

/// Python's `"\n".join(rows).strip("\n")`.
fn join_rows(rows: &[String]) -> String {
    rows.join("\n").trim_matches('\n').to_string()
}

/// Python's `str.rstrip()` (whitespace).
fn rstrip(text: &str) -> String {
    text.trim_end_matches(char::is_whitespace).to_string()
}

fn nametable_text(system: &mut System, base: u16, tile_char: impl Fn(u8) -> char) -> String {
    let ppu = &mut system.cpu.bus.ppu;
    let mut rows = Vec::new();
    for row in 0..30u16 {
        let line: String = (0..32u16)
            .map(|column| tile_char(ppu.bus_read(base + row * 32 + column)))
            .collect();
        rows.push(rstrip(&line));
    }
    join_rows(&rows)
}

/// Nametable `nametable` (0-3) as 30 lines of text: printable tiles as ASCII.
pub fn screen_text(system: &mut System, nametable: u16) -> String {
    nametable_text(system, 0x2000 + 0x400 * nametable, ascii_char)
}

fn ascii_char(t: u8) -> char {
    if (0x20..0x7F).contains(&t) {
        t as char
    } else {
        ' '
    }
}

/// Holy Mapperel's screen: ' '-'?' at their ASCII codes, letters at `code & $3F`.
pub fn holy_mapperel_text(system: &mut System) -> String {
    nametable_text(system, 0x2000, |t| {
        if (0x20..0x40).contains(&t) {
            t as char
        } else if 0 < t && t < 0x20 {
            (t | 0x40) as char
        } else {
            ' '
        }
    })
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn rainwarrior_char(t: u8) -> char {
    if (0x20..0x7F).contains(&t) {
        return t as char;
    }
    if (0xA0..0xB0).contains(&t) {
        HEX[(t & 15) as usize] as char
    } else {
        ' '
    }
}

fn tall_char(t: u8) -> char {
    if t != 0 && t & 1 == 0 && (0x20..0x7F).contains(&(t >> 1)) {
        (t >> 1) as char
    } else {
        ' '
    }
}

fn mmc1atest_char(t: u8) -> char {
    if (0x10..0x40).contains(&t) {
        return HEX[(t & 15) as usize] as char;
    }
    if t == 0x06 {
        ':'
    } else {
        ' '
    }
}

fn vrctest_char(t: u8) -> char {
    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-!?.,;:()/= ";
    if t < 0x30 {
        CHARS[t as usize] as char
    } else {
        ' '
    }
}

fn font(name: &str) -> fn(u8) -> char {
    match name {
        "rainwarrior" => rainwarrior_char,
        "tall" => tall_char,
        "mmc1atest" => mmc1atest_char,
        "vrctest" => vrctest_char,
        _ => ascii_char,
    }
}

/// Nametable 0 as text in one of the test ROMs' fonts; with `sprites`, the
/// sprites' tiles too.
pub fn font_text(system: &mut System, font_name: &str, sprites: bool) -> String {
    let decode = font(font_name);
    let text = nametable_text(system, 0x2000, decode);
    if !sprites {
        return text;
    }
    let mut grid: Vec<Vec<char>> = text
        .lines()
        .map(|line| {
            let mut row: Vec<char> = line.chars().collect();
            while row.len() < 32 {
                row.push(' ');
            }
            row
        })
        .collect();
    while grid.len() < 30 {
        grid.push(vec![' '; 32]);
    }
    let ppu = &system.cpu.bus.ppu;
    let scroll = ((ppu.t as usize >> 5) & 31) * 8 + ((ppu.t as usize >> 12) & 7);
    for i in (0..256).step_by(4) {
        let y = ppu.oam[i] as usize + 1;
        let tile = ppu.oam[i + 1];
        let x = ppu.oam[i + 3] as usize;
        if y < 240 && decode(tile) != ' ' {
            grid[(y + scroll) / 8 % 30][x / 8] = decode(tile);
        }
    }
    let rows: Vec<String> = grid
        .iter()
        .map(|row| rstrip(&row.iter().collect::<String>()))
        .collect();
    join_rows(&rows)
}

/// A finished blargg run, or the state at its time limit.
pub struct BlarggResult {
    pub status: Option<u16>,
    pub text: String,
    pub cycles: i64,
    pub instructions: u64,
}

fn signature(system: &mut System) -> bool {
    (0..3).all(|i| peek(system, 0x6001 + i as u16) == SIGNATURE[i])
}

fn finished(system: &mut System) -> bool {
    signature(system) && peek(system, 0x6000) < RUNNING
}

fn reset_requested(system: &mut System) -> bool {
    signature(system) && peek(system, 0x6000) == RESET_REQUEST
}

fn cpu_hz(system: &System) -> f64 {
    let timing = &system.cpu.bus.machine.timing;
    timing.master_hz / timing.cpu_divider as f64
}

/// Run a $6000-protocol ROM to its result, pressing reset when it asks.
pub fn run_blargg(system: &mut System, max_seconds: f64) -> BlarggResult {
    let cps = cpu_hz(system);
    let max_cycles = (max_seconds * cps) as i64;
    let start = system.cycles();
    while system.cycles() - start < max_cycles {
        let remaining = max_cycles - (system.cycles() - start);
        system.run_until(|s| finished(s) || reset_requested(s), remaining, 2000);
        if finished(system) {
            return BlarggResult {
                status: Some(peek(system, 0x6000)),
                text: blargg_text(system),
                cycles: system.cycles(),
                instructions: system.instructions,
            };
        }
        if reset_requested(system) {
            system.run_cycles((0.1 * cps) as i64 + 1);
            system.reset();
            let remaining = max_cycles - (system.cycles() - start);
            system.run_until(|s| !reset_requested(s), remaining.max(0), 2000);
            continue;
        }
        break;
    }
    BlarggResult {
        status: None,
        text: blargg_text(system),
        cycles: system.cycles(),
        instructions: system.instructions,
    }
}

/// Run a screen-only ROM until its nametable shows `passed` or `failed`.
pub fn run_screen(
    system: &mut System,
    passed: &str,
    failed: &str,
    max_seconds: f64,
    nametable: u16,
) -> (Option<bool>, String) {
    let mut frames = 0;
    let limit = (max_seconds * 60.0) as i64;
    let mut text = String::new();
    while frames < limit {
        system.run_frames(30);
        frames += 30;
        text = screen_text(system, nametable);
        if text.contains(passed) {
            return (Some(true), text);
        }
        if text.contains(failed) {
            return (Some(false), text);
        }
    }
    (None, text)
}

/// One test ROM case as the reference's table gives it, with its board.
#[derive(Clone, Debug)]
pub struct RomCase {
    pub path: String,
    pub protocol: String,
    pub seconds: f64,
    pub buttons: u8,
    pub passed: String,
    pub failed: String,
    pub expect: Vec<String>,
    pub font: String,
    pub sprites: bool,
    pub machine: String,
    pub board: BoardInfo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RomOutcome {
    pub passed: Option<bool>,
    pub detail: String,
    pub cycles: i64,
    pub instructions: u64,
}

/// A test ROM's bytes; a path through a `.7z` or `.zip` reads the member in place.
pub fn rom_bytes(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let parts: Vec<&str> = path.split('/').collect();
    for i in 0..parts.len() {
        let part = parts[i];
        let archive = root.join(parts[..=i].join("/"));
        if (part.ends_with(".7z") || part.ends_with(".zip")) && archive.is_file() {
            let member = parts[i + 1..].join("/");
            if part.ends_with(".zip") {
                let file = std::fs::File::open(&archive).map_err(|e| e.to_string())?;
                let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                let mut entry = zip.by_name(&member).map_err(|e| e.to_string())?;
                let mut data = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| e.to_string())?;
                return Ok(data);
            }
            let output = std::process::Command::new("7z")
                .arg("e")
                .arg("-so")
                .arg(&archive)
                .arg(&member)
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(format!("7z failed on {}", archive.display()));
            }
            return Ok(output.stdout);
        }
    }
    std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))
}

/// Holy Mapperel's screen against the board it ran on.
pub fn holy_mapperel_verdict(system: &mut System) -> (Option<bool>, String) {
    let text = holy_mapperel_text(system);
    if !text.contains("DETAILED TEST RESULT:") {
        return (None, text);
    }
    let image = system.image.clone().expect("a loaded image");
    let board = &image.board;
    let ram = board.prg_ram + board.prg_nvram;
    let expected = [
        format!("{:03} ", board.mapper),
        format!("{}K PRG ROM", image.prg.len() / 1024),
        if ram != 0 {
            format!("{}K PRG RAM OK", ram / 1024)
        } else {
            "PRG RAM MISSING".to_string()
        },
        if image.chr.is_empty() {
            "CHR RAM OK".to_string()
        } else {
            "CHR ROM OK".to_string()
        },
        "DETAILED TEST RESULT: 0000".to_string(),
    ];
    let passed =
        !text.contains("PROBLEM") && expected.iter().all(|line| text.contains(line.as_str()));
    (Some(passed), text)
}

/// Python's `repr()` of a str.
pub fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn py_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| py_repr(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Power on with `case`'s buttons held and read its verdict (`run_case`).
pub fn run_case(case: &RomCase, root: &Path) -> Result<RomOutcome, String> {
    let data = rom_bytes(root, &case.path)?;
    let machine = Machine::by_key(&case.machine).ok_or("unknown machine")?;
    let mut system = System::from_rom(&data, machine, Some(case.board.clone()))?;
    if let Some(pad) = system.board().pads[0]
        .as_any_mut()
        .downcast_mut::<StandardController>()
    {
        pad.buttons = case.buttons;
    }
    let (passed, detail) = match case.protocol.as_str() {
        "6000" => {
            let result = run_blargg(&mut system, case.seconds);
            let status = match result.status {
                None => "None".to_string(),
                Some(s) => s.to_string(),
            };
            (
                result.status.map(|s| s == 0),
                format!("status {status}: {}", result.text.trim()),
            )
        }
        "values" => {
            system.run_frames((case.seconds * 60.0) as u64);
            let text = font_text(&mut system, &case.font, case.sprites);
            let lines: Vec<&str> = text.lines().map(str::trim).collect();
            let lines = format!("\n{}\n", lines.join("\n"));
            let missing: Vec<String> = case
                .expect
                .iter()
                .filter(|block| !lines.contains(&format!("\n{block}\n")))
                .cloned()
                .collect();
            (
                Some(missing.is_empty()),
                format!("missing {}:\n{text}", py_list(&missing)),
            )
        }
        "ram" => {
            system.run_frames((case.seconds * 60.0) as u64);
            let ram = system.cpu.bus.ram;
            let wrong: Vec<String> = case
                .expect
                .iter()
                .filter(|e| {
                    let address = usize::from_str_radix(&e[..4], 16).unwrap();
                    let value = u8::from_str_radix(&e[5..], 16).unwrap();
                    ram[address] != value
                })
                .cloned()
                .collect();
            let hex: Vec<String> = ram[..0x100].iter().map(|b| format!("{b:02x}")).collect();
            (
                Some(wrong.is_empty()),
                format!("wrong {}: {}", py_list(&wrong), hex.join(" ")),
            )
        }
        "spans" => {
            // The reference's detail prints sets, whose order Python's hashing
            // decides; only the verdict and the counts are compared for spans.
            system.run_frames((case.seconds * 60.0) as u64);
            let mut want: Vec<(usize, Vec<String>)> = Vec::new();
            for e in &case.expect {
                let (line, spans) = e.split_once('=').unwrap();
                let mut spans: Vec<String> = spans.split(',').map(String::from).collect();
                spans.sort();
                spans.dedup();
                want.push((line.parse().unwrap(), spans));
            }
            let mut seen: Vec<Vec<String>> = vec![Vec::new(); want.len()];
            for _ in 0..4 {
                system.run_frames(1);
                let picture = &system.cpu.bus.ppu.picture;
                for (k, (line, _)) in want.iter().enumerate() {
                    let row = &picture[line * 256..line * 256 + 256];
                    let xs: Vec<usize> = (0..256).filter(|&x| row[x] != row[0]).collect();
                    let span = match (xs.first(), xs.last()) {
                        (Some(a), Some(b)) => format!("{a}-{b}"),
                        _ => "none".to_string(),
                    };
                    if !seen[k].contains(&span) {
                        seen[k].push(span);
                    }
                }
            }
            for s in seen.iter_mut() {
                s.sort();
            }
            let passed = want.iter().zip(&seen).all(|((_, w), s)| w == s);
            (Some(passed), format!("seen {seen:?}"))
        }
        "holy" => {
            let mut result = (None, String::new());
            for _ in 0..case.seconds as i64 {
                system.run_frames(60);
                result = holy_mapperel_verdict(&mut system);
                if result.0.is_some() {
                    break;
                }
            }
            result
        }
        _ => run_screen(&mut system, &case.passed, &case.failed, case.seconds, 0),
    };
    Ok(RomOutcome {
        passed,
        detail,
        cycles: system.cycles(),
        instructions: system.instructions,
    })
}

/// The loader's board for `data`, as `run_case` would start from it.
pub fn identified_board(data: &[u8]) -> Result<BoardInfo, String> {
    Ok(cartridge::load(data, 12, None, true)?.image.board)
}

// -- AccuracyCoin -------------------------------------------------------------

/// One AccuracyCoin test: its page, name, result byte and place in the menu.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccuracyCoinTest {
    pub page: String,
    pub name: String,
    pub address: u16,
    pub page_number: u32,
    pub row: u32,
}

/// A result byte that is final: pass (1) or fail (2) in the low bits, or $FF.
pub fn result_finished(value: u8) -> bool {
    matches!(value & 3, 1 | 2) || value == 0xFF
}

/// Press Start on the page index (run all), then read every result byte.
pub fn run_accuracycoin(system: &mut System, addresses: &[u16], max_seconds: f64) -> Vec<u8> {
    let set_buttons = |system: &mut System, buttons: u8| {
        if let Some(pad) = system.board().pads[0]
            .as_any_mut()
            .downcast_mut::<StandardController>()
        {
            pad.buttons = buttons;
        }
    };
    system.run_frames(60);
    set_buttons(system, crate::joypad::START);
    system.run_frames(4);
    set_buttons(system, 0);
    let mut frames = 0;
    let limit = (max_seconds * 60.0) as i64;
    let done = |system: &System| {
        addresses
            .iter()
            .all(|&a| result_finished(system.cpu.bus.ram[a as usize & 0x7FF]))
    };
    while frames < limit && !done(system) {
        system.run_frames(30);
        frames += 30;
    }
    addresses
        .iter()
        .map(|&a| system.cpu.bus.ram[a as usize & 0x7FF])
        .collect()
}
