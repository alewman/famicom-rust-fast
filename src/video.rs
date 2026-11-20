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
    let reference = match std::env::var("FAMICOM_REFERENCE") {
        Ok(path) => PathBuf::from(path),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("external/famicom-python"),
    };
    reference.join("reference/nesdev-wiki/images")
}

/// The pally palette of a video standard.
pub fn palette_for(timing: &Timing) -> PathBuf {
    images().join(timing.palette)
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
