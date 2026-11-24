// SPDX-License-Identifier: MIT
//! iNES 1.0 and NES 2.0 files: the header and the ROM it describes (`ines.py`).

pub const MAGIC: &[u8; 4] = b"NES\x1a";

/// What a cartridge's circuit is: everything a mapper needs besides the ROM.
///
/// `mirroring` is nes20db's letter: 'H', 'V', '4' or '1'.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardInfo {
    pub mapper: u16,
    pub submapper: u8,
    pub mirroring: char,
    pub battery: bool,
    pub prg_ram: usize,
    pub prg_nvram: usize,
    pub chr_ram: usize,
    pub chr_nvram: usize,
    /// 0 NTSC, 1 PAL, 2 multi-region, 3 Dendy.
    pub timing: u8,
    /// 0 NES/Famicom, 1 Vs., 2 PlayChoice-10, 3 extended.
    pub console: u8,
    /// The Default Expansion Device (NES 2.0 byte 15).
    pub expansion: u8,
    /// A Vs. System game's (console 1) NES 2.0 byte 13, or nes20db's `<vs>`:
    /// the RGB PPU its colours and checks expect (0 RP2C03, 2-5 RP2C04-0001 to
    /// -0004, 8-11 RC2C05-01 to -04) and its hardware type (0 UniSystem, 1-4 a
    /// UniSystem with a protection, 5 DualSystem, 6 DualSystem with Raid on
    /// Bungeling Bay's protection) (NES_2.0.wiki "Vs. System Type").
    pub vs_ppu: u8,
    pub vs_hardware: u8,
}

impl BoardInfo {
    pub fn new(mapper: u16) -> BoardInfo {
        BoardInfo {
            mapper,
            submapper: 0,
            mirroring: 'H',
            battery: false,
            prg_ram: 0,
            prg_nvram: 0,
            chr_ram: 0,
            chr_nvram: 0,
            timing: 0,
            console: 0,
            expansion: 0,
            vs_ppu: 0,
            vs_hardware: 0,
        }
    }
}

/// A cartridge file, split: header, trainer, PRG, CHR and anything after.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RomImage {
    pub header: Vec<u8>,
    pub trainer: Vec<u8>,
    pub prg: Vec<u8>,
    pub chr: Vec<u8>,
    pub misc: Vec<u8>,
    pub board: BoardInfo,
    pub nes2: bool,
}

fn nes2_rom_size(low: u8, high_nibble: u8, unit: usize) -> usize {
    if high_nibble == 0x0F {
        (1usize << (low >> 2)) * ((low as usize & 3) * 2 + 1)
    } else {
        (((high_nibble as usize) << 8) | low as usize) * unit
    }
}

fn shift_size(count: u8) -> usize {
    if count == 0 {
        0
    } else {
        64 << count
    }
}

/// Split an iNES or NES 2.0 file into its parts, believing its header.
pub fn parse(data: &[u8]) -> Result<RomImage, String> {
    if data.len() < 16 || &data[..4] != MAGIC {
        return Err("not an iNES file (no NES<EOF> magic)".into());
    }
    let h = &data[..16];
    let nes2 = (h[7] & 0x0C) == 0x08;
    let flags6 = h[6];
    let mirroring = if flags6 & 0x08 != 0 {
        '4'
    } else if flags6 & 0x01 != 0 {
        'V'
    } else {
        'H'
    };
    let battery = flags6 & 0x02 != 0;
    let mut mapper = ((flags6 >> 4) | (h[7] & 0xF0)) as u16;
    let (board, prg_size, chr_size);
    if nes2 {
        mapper |= ((h[8] & 0x0F) as u16) << 8;
        board = BoardInfo {
            mapper,
            submapper: h[8] >> 4,
            mirroring,
            battery,
            prg_ram: shift_size(h[10] & 0x0F),
            prg_nvram: shift_size(h[10] >> 4),
            chr_ram: shift_size(h[11] & 0x0F),
            chr_nvram: shift_size(h[11] >> 4),
            timing: h[12] & 0x03,
            console: h[7] & 0x03,
            expansion: h[15] & 0x3F,
            vs_ppu: if h[7] & 0x03 == 1 { h[13] & 0x0F } else { 0 },
            vs_hardware: if h[7] & 0x03 == 1 { h[13] >> 4 } else { 0 },
        };
        prg_size = nes2_rom_size(h[4], h[9] & 0x0F, 0x4000);
        chr_size = nes2_rom_size(h[5], h[9] >> 4, 0x2000);
    } else {
        if h[12..16].iter().any(|&b| b != 0) {
            mapper &= 0x0F;
        }
        prg_size = h[4] as usize * 0x4000;
        chr_size = h[5] as usize * 0x2000;
        board = BoardInfo {
            mapper,
            mirroring,
            battery,
            prg_ram: if battery { 0 } else { 0x2000 },
            prg_nvram: if battery { 0x2000 } else { 0 },
            chr_ram: if chr_size != 0 { 0 } else { 0x2000 },
            timing: if h[9] & 0x01 != 0 { 1 } else { 0 },
            console: h[7] & 0x03,
            ..BoardInfo::new(mapper)
        };
    }
    let mut offset = 16;
    let slice = |from: usize, len: usize| -> Vec<u8> {
        let from = from.min(data.len());
        let to = (from + len).min(data.len());
        data[from..to].to_vec()
    };
    let trainer = if flags6 & 0x04 != 0 {
        slice(offset, 512)
    } else {
        Vec::new()
    };
    offset += trainer.len();
    let prg = slice(offset, prg_size);
    offset += prg_size;
    let chr = slice(offset, chr_size);
    offset += chr_size;
    if prg.len() != prg_size || chr.len() != chr_size {
        return Err(format!(
            "file too short: header says {} PRG + {} CHR bytes, file has {}",
            prg_size,
            chr_size,
            data.len() - 16 - trainer.len()
        ));
    }
    let misc = if offset < data.len() {
        data[offset..].to_vec()
    } else {
        Vec::new()
    };
    Ok(RomImage {
        header: h.to_vec(),
        trainer,
        prg,
        chr,
        misc,
        board,
        nes2,
    })
}
