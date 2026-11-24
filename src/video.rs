// SPDX-License-Identifier: MIT
//! From the PPU's 9-bit pixels to RGB, and a PNG (`video.py`).
//!
//! The 2C02 has no RGB: colour is the frontend's choice. The default is
//! pally's `2C02G_U_wiki.pal`, read at run time from the reference's
//! `reference/nesdev-wiki/images` (never committed). The PNG's compressed
//! bytes are flate2's, not zlib's; its pixels are the reference's.

use crate::machine::Timing;
use crate::ppu::{HEIGHT, WIDTH};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the reference keeps pally's palettes.
pub fn images() -> PathBuf {
    crate::reference_checkout().join("reference/nesdev-wiki/images")
}

/// The RGB PPUs' colour ROMs, as PPU_palettes.wiki prints them ("2C03 and 2C05",
/// "RP2C04-0001" to "-0004"): one octal digit each for red, green and blue per
/// palette index. The four RP2C04s hold one 64-colour master palette in four
/// permutations, a copy protection; the RC2C05s have the RP2C03's (the
/// reference's tests/test_vs_system.py reads the pinned page and its
/// 2C03_wiki.pal against these). Specification tier: the wiki's tables, from
/// dumped chips (nesdev-forum p=98955, not pinned).
#[rustfmt::skip]
pub const RGB_DAC: [(&str, [u16; 64]); 5] = [
    ("RP2C03", [
        0o333, 0o014, 0o006, 0o326, 0o403, 0o503, 0o510, 0o420,
        0o320, 0o120, 0o031, 0o040, 0o022, 0o000, 0o000, 0o000,
        0o555, 0o036, 0o027, 0o407, 0o507, 0o704, 0o700, 0o630,
        0o430, 0o140, 0o040, 0o053, 0o044, 0o000, 0o000, 0o000,
        0o777, 0o357, 0o447, 0o637, 0o707, 0o737, 0o740, 0o750,
        0o660, 0o360, 0o070, 0o276, 0o077, 0o000, 0o000, 0o000,
        0o777, 0o567, 0o657, 0o757, 0o747, 0o755, 0o764, 0o772,
        0o773, 0o572, 0o473, 0o276, 0o467, 0o000, 0o000, 0o000,
    ]),
    ("RP2C04-0001", [
        0o755, 0o637, 0o700, 0o447, 0o044, 0o120, 0o222, 0o704,
        0o777, 0o333, 0o750, 0o503, 0o403, 0o660, 0o320, 0o777,
        0o357, 0o653, 0o310, 0o360, 0o467, 0o657, 0o764, 0o027,
        0o760, 0o276, 0o000, 0o200, 0o666, 0o444, 0o707, 0o014,
        0o003, 0o567, 0o757, 0o070, 0o077, 0o022, 0o053, 0o507,
        0o000, 0o420, 0o747, 0o510, 0o407, 0o006, 0o740, 0o000,
        0o000, 0o140, 0o555, 0o031, 0o572, 0o326, 0o770, 0o630,
        0o020, 0o036, 0o040, 0o111, 0o773, 0o737, 0o430, 0o473,
    ]),
    ("RP2C04-0002", [
        0o000, 0o750, 0o430, 0o572, 0o473, 0o737, 0o044, 0o567,
        0o700, 0o407, 0o773, 0o747, 0o777, 0o637, 0o467, 0o040,
        0o020, 0o357, 0o510, 0o666, 0o053, 0o360, 0o200, 0o447,
        0o222, 0o707, 0o003, 0o276, 0o657, 0o320, 0o000, 0o326,
        0o403, 0o764, 0o740, 0o757, 0o036, 0o310, 0o555, 0o006,
        0o507, 0o760, 0o333, 0o120, 0o027, 0o000, 0o660, 0o777,
        0o653, 0o111, 0o070, 0o630, 0o022, 0o014, 0o704, 0o140,
        0o000, 0o077, 0o420, 0o770, 0o755, 0o503, 0o031, 0o444,
    ]),
    ("RP2C04-0003", [
        0o507, 0o737, 0o473, 0o555, 0o040, 0o777, 0o567, 0o120,
        0o014, 0o000, 0o764, 0o320, 0o704, 0o666, 0o653, 0o467,
        0o447, 0o044, 0o503, 0o027, 0o140, 0o430, 0o630, 0o053,
        0o333, 0o326, 0o000, 0o006, 0o700, 0o510, 0o747, 0o755,
        0o637, 0o020, 0o003, 0o770, 0o111, 0o750, 0o740, 0o777,
        0o360, 0o403, 0o357, 0o707, 0o036, 0o444, 0o000, 0o310,
        0o077, 0o200, 0o572, 0o757, 0o420, 0o070, 0o660, 0o222,
        0o031, 0o000, 0o657, 0o773, 0o407, 0o276, 0o760, 0o022,
    ]),
    ("RP2C04-0004", [
        0o430, 0o326, 0o044, 0o660, 0o000, 0o755, 0o014, 0o630,
        0o555, 0o310, 0o070, 0o003, 0o764, 0o770, 0o040, 0o572,
        0o737, 0o200, 0o027, 0o747, 0o000, 0o222, 0o510, 0o740,
        0o653, 0o053, 0o447, 0o140, 0o403, 0o000, 0o473, 0o357,
        0o503, 0o031, 0o420, 0o006, 0o407, 0o507, 0o333, 0o704,
        0o022, 0o666, 0o036, 0o020, 0o111, 0o773, 0o444, 0o707,
        0o757, 0o777, 0o320, 0o700, 0o760, 0o276, 0o777, 0o467,
        0o000, 0o750, 0o637, 0o567, 0o360, 0o657, 0o077, 0o120,
    ]),
];

/// An RGB PPU's 512 colours: "C = 255 * DAC / 7" per channel, and each
/// emphasis bit "sets the corresponding channel to full brightness" rather
/// than dimming the others (PPU_palettes.wiki "2C03 and 2C05"; the picture's
/// bits 6, 7 and 8 are $2001's red, green and blue). Greyscale is the PPU's
/// own AND $30 on the index, already in the picture.
pub fn rgb_palette(chip: &str) -> Vec<[u8; 3]> {
    let dac = &RGB_DAC
        .iter()
        .find(|(name, _)| *name == chip)
        .unwrap_or_else(|| panic!("no colour ROM {chip}"))
        .1;
    let mut colours = Vec::with_capacity(512);
    for emphasis in 0..8 {
        for &value in dac {
            let mut channels = [(value >> 6) & 7, (value >> 3) & 7, value & 7];
            for (channel, level) in channels.iter_mut().enumerate() {
                if (emphasis >> channel) & 1 != 0 {
                    *level = 7;
                }
            }
            colours
                .push(channels.map(|level| (255.0 * level as f64 / 7.0).round_ties_even() as u8));
        }
    }
    colours
}

/// The pally palette of a video standard.
pub fn palette_for(timing: &Timing) -> PathBuf {
    images().join(timing.palette)
}

/// A machine's 512 colours: the file `path` if given, else an RGB PPU's own
/// ([`rgb_palette`]), else the pally palette of the video standard.
pub fn machine_palette(timing: &Timing, path: Option<&Path>) -> Result<Vec<[u8; 3]>, String> {
    if path.is_none() && timing.rgb {
        return Ok(rgb_palette(timing.palette));
    }
    match path {
        Some(path) => load_palette(path, timing.swapped_emphasis),
        None => load_palette(&palette_for(timing), timing.swapped_emphasis),
    }
}

/// 512 RGB triples, one per 9-bit pixel value (a 64-entry file ignores emphasis).
pub fn load_palette(path: &Path, swapped_emphasis: bool) -> Result<Vec<[u8; 3]>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if data.len() != 64 * 3 && data.len() != 512 * 3 {
        return Err(format!(
            "{}: {} bytes, not a 64- or 512-colour .pal file",
            path.display(),
            data.len()
        ));
    }
    let mut colours: Vec<[u8; 3]> = data.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    if colours.len() == 64 {
        colours = colours.repeat(8);
    }
    if swapped_emphasis {
        let swap = |i: usize| (i & !0xC0) | ((i & 0x40) << 1) | ((i & 0x80) >> 1);
        colours = (0..512).map(|i| colours[swap(i)]).collect();
    }
    Ok(colours)
}

/// A frame of 9-bit values as packed 8-bit RGB.
pub fn to_rgb(picture: &[u16], palette: &[[u8; 3]]) -> Vec<u8> {
    picture
        .iter()
        .flat_map(|&value| palette[value as usize])
        .collect()
}

/// Write packed RGB as an 8-bit truecolour PNG.
pub fn write_png(path: &Path, rgb: &[u8], width: usize, height: usize) -> std::io::Result<()> {
    let stride = width * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height);
    for y in 0..height {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&raw)?;
    let idat = encoder.finish()?;
    let chunk = |kind: &[u8], body: &[u8]| -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(kind);
        hasher.update(body);
        out.extend_from_slice(&hasher.finalize().to_be_bytes());
        out
    };
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &idat));
    png.extend(chunk(b"IEND", b""));
    std::fs::write(path, png)
}

/// The picture's size.
pub const SIZE: (usize, usize) = (WIDTH, HEIGHT);
