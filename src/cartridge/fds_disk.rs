// SPDX-License-Identifier: MIT
//! Famicom Disk System disks: a `.fds` or `.qd` file's sides as the drive's
//! head meets them (`fds_disk.py`; FDS_file_format.wiki, FDS_disk_format.wiki,
//! QD.wiki: the stream's layout is described there).

pub const SIDE_BYTES: usize = 65_500;
pub const QD_SIDE_BYTES: usize = 65_536;
pub const HEADER: &[u8; 4] = b"FDS\x1a";
pub const VERIFY: &[u8; 14] = b"*NINTENDO-HVC*";
/// The 1 bit that ends a gap.
pub const MARK: u8 = 0x80;
/// Bytes of zeros before the first block (typical).
pub const PREGAP: usize = 28_300 / 8;
/// Bytes of zeros between blocks (typical).
pub const GAP: usize = 976 / 8;

/// Block lengths without the CRC, by block code; a file data block (4) is 1 +
/// the size in the file header before it.
fn block_length(code: u8) -> Option<usize> {
    match code {
        1 => Some(56),
        2 => Some(2),
        3 => Some(16),
        _ => None,
    }
}

const CRC_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut crc = byte as u16;
        let mut i = 0;
        while i < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
            i += 1;
        }
        table[byte] = crc;
        byte += 1;
    }
    table
};

/// CRC-16/KERMIT (reflected 0x1021, initial 0), as the RP2C33 computes it.
pub fn crc16(data: &[u8], crc: u16) -> u16 {
    let mut crc = crc;
    for &byte in data {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ byte as u16) & 0xFF) as usize];
    }
    crc
}

fn all_zero(data: &[u8]) -> bool {
    data.iter().all(|&b| b == 0)
}

/// A `.fds` file, with or without its header.
pub fn is_fds(data: &[u8]) -> bool {
    if data.len() >= 4 && &data[..4] == HEADER {
        return true;
    }
    data.len() >= SIDE_BYTES && data.len() % SIDE_BYTES == 0 && &data[1..15] == VERIFY
}

/// A `.qd` file: whole 65,536-byte sides starting with a disk info block.
pub fn is_qd(data: &[u8]) -> bool {
    data.len() >= QD_SIDE_BYTES && data.len() % QD_SIDE_BYTES == 0 && &data[1..15] == VERIFY
}

/// Each side of a `.fds` or `.qd` file as the stream on the disk.
pub fn disk_sides(data: &[u8]) -> Vec<Vec<u8>> {
    if is_qd(data) && !is_fds(data) {
        return data
            .chunks(QD_SIDE_BYTES)
            .filter(|side| !all_zero(side))
            .map(|side| {
                let (found, crcs) = qd_blocks(side);
                raw(&found, &crcs)
            })
            .collect();
    }
    split_sides(data)
        .iter()
        .map(|side| raw_side(side))
        .collect()
}

/// The 65,500-byte sides of a `.fds` file; a short last side is padded with zeros.
pub fn split_sides(data: &[u8]) -> Vec<Vec<u8>> {
    let data = if data.len() >= 4 && &data[..4] == HEADER {
        &data[16.min(data.len())..]
    } else {
        data
    };
    data.chunks(SIDE_BYTES)
        .filter(|side| !all_zero(side))
        .map(|side| {
            let mut side = side.to_vec();
            side.resize(SIDE_BYTES, 0);
            side
        })
        .collect()
}

fn expected_code(found: &[Vec<u8>]) -> u8 {
    match found.len() {
        0 => 1,
        1 => 2,
        _ => {
            if found[found.len() - 1][0] == 3 {
                4
            } else {
                3
            }
        }
    }
}

/// The blocks of a `.fds` side in order (without CRCs).
pub fn blocks(side: &[u8]) -> Vec<Vec<u8>> {
    let mut found: Vec<Vec<u8>> = Vec::new();
    let (mut pos, mut size) = (0usize, 0usize);
    while pos < side.len() {
        let code = side[pos];
        if code != expected_code(&found) {
            break;
        }
        let length = if code == 4 {
            1 + size
        } else {
            block_length(code).unwrap()
        };
        if pos + length > side.len() {
            break;
        }
        let block = side[pos..pos + length].to_vec();
        if code == 3 {
            size = block[13] as usize | (block[14] as usize) << 8;
        }
        found.push(block);
        pos += length;
    }
    found
}

/// A `.fds` side as the stream on the disk: gaps, marks, blocks and CRCs,
/// then blank disk to the side's capacity.
pub fn raw_side(side: &[u8]) -> Vec<u8> {
    let found = blocks(side);
    let crcs: Vec<Vec<u8>> = found
        .iter()
        .map(|block| {
            let crc = crc16(block, crc16(&[MARK], 0));
            vec![(crc & 0xFF) as u8, (crc >> 8) as u8]
        })
        .collect();
    raw(&found, &crcs)
}

fn raw(found: &[Vec<u8>], crcs: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = vec![0u8; PREGAP];
    for (block, crc) in found.iter().zip(crcs) {
        raw.push(MARK);
        raw.extend_from_slice(block);
        raw.extend_from_slice(crc);
        raw.extend(std::iter::repeat_n(0u8, GAP));
    }
    let capacity = PREGAP + SIDE_BYTES + found.len() * (3 + GAP);
    if capacity > raw.len() {
        raw.resize(capacity, 0);
    }
    raw
}

fn qd_blocks(side: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut found: Vec<Vec<u8>> = Vec::new();
    let mut crcs = Vec::new();
    let (mut pos, mut size) = (0usize, 0usize);
    while pos < side.len() {
        let code = side[pos];
        if code != expected_code(&found) {
            break;
        }
        let length = if code == 4 {
            1 + size
        } else {
            block_length(code).unwrap()
        };
        if pos + length + 2 > side.len() {
            break;
        }
        let block = side[pos..pos + length].to_vec();
        if code == 3 {
            size = block[13] as usize | (block[14] as usize) << 8;
        }
        found.push(block);
        crcs.push(side[pos + length..pos + length + 2].to_vec());
        pos += length + 2;
    }
    (found, crcs)
}

/// A disk stream back as a `.fds` side, padded with zeros to 65,500 bytes.
pub fn fds_side(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut pos, mut size, mut previous) = (0usize, 0usize, 0u8);
    loop {
        while pos < raw.len() && raw[pos] == 0 {
            pos += 1;
        }
        if pos >= raw.len() || raw[pos] != MARK {
            break;
        }
        pos += 1;
        let code = if pos < raw.len() { raw[pos] } else { 0 };
        let length = if code == 4 && previous == 3 {
            1 + size
        } else if let Some(length) = block_length(code) {
            length
        } else {
            break;
        };
        let end = (pos + length).min(raw.len());
        let block = &raw[pos..end];
        if code == 3 {
            size = block[13] as usize | (block[14] as usize) << 8;
        }
        out.extend_from_slice(block);
        previous = code;
        pos += length + 2;
    }
    out.truncate(SIDE_BYTES);
    out.resize(SIDE_BYTES, 0);
    out
}
