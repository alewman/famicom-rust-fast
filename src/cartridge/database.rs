//! nes20db: who made this cartridge, and how (`database.py`).
//!
//! The pinned 7z (`reference/docs/nes20db-2026-09-26.7z` under the
//! reference checkout, or the file `FAMICOM_NES20DB` names) is read at run
//! time through the `7z` program, as the reference does, or, named by
//! `FAMICOM_NES20DB`, as the `nes20db.xml` unpacked from it. The XML is NewRisingSun's
//! regular layout, read with a small scanner (no XML crate): `<game>` blocks
//! whose children are empty elements with attributes. MAME's software list
//! (the reference's second, informing-only lookup) is not read: it
//! overrides nothing.

use super::ines::{BoardInfo, RomImage};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Nes20Db {
    /// SHA-1 of PRG + CHR (+ misc ROM).
    pub by_rom: HashMap<String, BoardInfo>,
    /// (PRG SHA-1, CHR SHA-1 or "").
    pub by_parts: HashMap<(String, String), BoardInfo>,
}

pub fn sha1_hex(data: &[u8]) -> String {
    let digest = Sha1::digest(data);
    digest.iter().map(|b| format!("{b:02X}")).collect()
}

/// Where the pinned database is: `FAMICOM_NES20DB`, else the reference
/// checkout's `reference/docs`.
pub fn nes20db_path() -> PathBuf {
    if let Ok(path) = std::env::var("FAMICOM_NES20DB") {
        return PathBuf::from(path);
    }
    let reference = match std::env::var("FAMICOM_REFERENCE") {
        Ok(path) => PathBuf::from(path),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("external/famicom-python"),
    };
    reference.join("reference/docs/nes20db-2026-09-26.7z")
}

/// The attributes of the first `<tag .../>` child in `game`.
fn element<'a>(game: &'a str, tag: &str) -> Option<HashMap<&'a str, &'a str>> {
    let open = format!("<{tag} ");
    let start = game.find(&open)? + open.len();
    let end = start + game[start..].find('>')?;
    let mut attributes = HashMap::new();
    let mut rest = &game[start..end];
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq].trim();
        let value_start = eq + 2;
        let value_end = value_start + rest[value_start..].find('"')?;
        attributes.insert(name, &rest[value_start..value_end]);
        rest = &rest[value_end + 1..];
    }
    Some(attributes)
}

fn number<T: std::str::FromStr + Default>(
    attributes: &HashMap<&str, &str>,
    name: &str,
    default: &str,
) -> T {
    attributes
        .get(name)
        .copied()
        .unwrap_or(default)
        .parse()
        .unwrap_or_default()
}

fn size(game: &str, tag: &str) -> usize {
    element(game, tag).map_or(0, |a| number(&a, "size", "0"))
}

fn board(game: &str) -> Option<BoardInfo> {
    let pcb = element(game, "pcb")?;
    let console = element(game, "console")?;
    let expansion = element(game, "expansion");
    Some(BoardInfo {
        mapper: number(&pcb, "mapper", "0"),
        submapper: number(&pcb, "submapper", "0"),
        mirroring: pcb
            .get("mirroring")
            .and_then(|m| m.chars().next())
            .unwrap_or('H'),
        battery: pcb.get("battery").copied() == Some("1"),
        prg_ram: size(game, "prgram"),
        prg_nvram: size(game, "prgnvram"),
        chr_ram: size(game, "chrram"),
        chr_nvram: size(game, "chrnvram"),
        timing: number(&console, "region", "0"),
        console: number(&console, "type", "0"),
        expansion: expansion.map_or(0, |e| number(&e, "type", "0")),
    })
}

fn strip_comments(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn parse_nes20db(xml: &str) -> Nes20Db {
    let xml = strip_comments(xml);
    let mut by_rom = HashMap::new();
    let mut by_parts = HashMap::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<game>") {
        let body_start = start + "<game>".len();
        let Some(end) = rest[body_start..].find("</game>") else {
            break;
        };
        let game = &rest[body_start..body_start + end];
        rest = &rest[body_start + end..];
        let Some(info) = board(game) else { continue };
        if let Some(rom) = element(game, "rom") {
            let sha = rom.get("sha1").copied().unwrap_or("").to_string();
            by_rom.entry(sha).or_insert_with(|| info.clone());
        }
        if let Some(prg) = element(game, "prgrom") {
            let chr = element(game, "chrrom").map_or(String::new(), |c| {
                c.get("sha1").copied().unwrap_or("").to_string()
            });
            let key = (prg.get("sha1").copied().unwrap_or("").to_string(), chr);
            by_parts.entry(key).or_insert(info);
        }
    }
    Nes20Db { by_rom, by_parts }
}

/// Parse the pinned nes20db once, or None without the file or a `7z` program.
pub fn load_nes20db() -> Option<&'static Nes20Db> {
    static DB: OnceLock<Option<Nes20Db>> = OnceLock::new();
    DB.get_or_init(|| {
        let path = nes20db_path();
        if !path.exists() {
            return None;
        }
        // The XML itself, unpacked from the pinned 7z (where no 7z program
        // runs: a handheld's libretro core).
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        {
            return std::fs::read_to_string(&path)
                .ok()
                .map(|xml| parse_nes20db(&xml));
        }
        for program in ["7z", "7za", "7zr"] {
            let output = std::process::Command::new(program)
                .arg("x")
                .arg("-so")
                .arg(&path)
                .arg("nes20db.xml")
                .output();
            if let Ok(output) = output {
                if output.status.success() {
                    return Some(parse_nes20db(&String::from_utf8_lossy(&output.stdout)));
                }
            }
        }
        None
    })
    .as_ref()
}

/// nes20db's description of `image`: by whole ROM, then by PRG and CHR.
pub fn lookup_nes20db(image: &RomImage, db: Option<&Nes20Db>) -> Option<BoardInfo> {
    let db = db?;
    let mut whole = image.prg.clone();
    whole.extend_from_slice(&image.chr);
    let with_misc = [whole.as_slice(), image.misc.as_slice()].concat();
    if let Some(board) = db.by_rom.get(&sha1_hex(&with_misc)) {
        return Some(board.clone());
    }
    if let Some(board) = db.by_rom.get(&sha1_hex(&whole)) {
        return Some(board.clone());
    }
    let chr = if image.chr.is_empty() {
        String::new()
    } else {
        sha1_hex(&image.chr)
    };
    db.by_parts.get(&(sha1_hex(&image.prg), chr)).cloned()
}
