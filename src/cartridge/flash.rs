// SPDX-License-Identifier: MIT
//! The SST39SF040 flash, the self-flashing homebrew boards' PRG (`flash.py`;
//! the commands are described there, from Microchip's DS20005022C).
//!
//! Difference forced by Rust: the reference's flash keeps a reference to the
//! cartridge's PRG bytes; here each call is handed them.

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Flash {
    pub manufacturer: u8,
    pub device: u8,
    pub unlock_mask: usize,
    pub sector_size: usize,
    /// How many writes of a sequence have matched (6: program's).
    pub step: u8,
    pub id_mode: bool,
    /// The PRG differs from the image: the save.
    pub dirty: bool,
}

impl Flash {
    /// The SST39SF040 (Table 2).
    pub fn sst39sf040() -> Flash {
        Flash {
            manufacturer: 0xBF,
            device: 0xB7,
            unlock_mask: 0x7FFF,
            sector_size: 0x1000,
            step: 0,
            id_mode: false,
            dirty: false,
        }
    }

    /// The 29F040 family: the same commands, 64 KiB sectors, unlocked on A10-A0.
    pub fn flash_29f040(manufacturer: u8, device: u8) -> Flash {
        Flash {
            manufacturer,
            device,
            unlock_mask: 0x7FF,
            sector_size: 0x10000,
            ..Flash::sst39sf040()
        }
    }

    /// The byte at chip address `offset` (only differs in ID mode).
    pub fn read(&self, data: &[u8], offset: usize) -> u8 {
        if self.id_mode && offset >> 1 == 0 {
            return if offset & 1 != 0 {
                self.device
            } else {
                self.manufacturer
            };
        }
        data[offset]
    }

    /// One write cycle at chip address `offset`.
    pub fn write(&mut self, data: &mut [u8], offset: usize, value: u8) {
        let unlock = offset & self.unlock_mask;
        let first = 0x5555 & self.unlock_mask;
        let second = 0x2AAA & self.unlock_mask;
        let step = self.step;
        self.step = 0;
        if (matches!(step, 0 | 3) && unlock == first && value == 0xAA)
            || (matches!(step, 1 | 4) && unlock == second && value == 0x55)
        {
            self.step = step + 1;
        } else if step == 2 && unlock == first && matches!(value, 0xA0 | 0x80) {
            self.step = if value == 0xA0 { 6 } else { 3 };
        } else if step == 2 && unlock == first && matches!(value, 0x90 | 0xF0) {
            self.id_mode = value == 0x90;
        } else if step == 0 && value == 0xF0 {
            self.id_mode = false;
        } else if step == 5 && value == 0x30 {
            let sector = offset & !(self.sector_size - 1);
            let end = (sector + self.sector_size).min(data.len());
            // Python's slice assignment of a full sector past the end grows the
            // bytearray; the chips' sizes are multiples of their sectors.
            data[sector..end].fill(0xFF);
            self.dirty = true;
        } else if step == 5 && unlock == first && value == 0x10 {
            data.fill(0xFF);
            self.dirty = true;
        } else if step == 6 {
            data[offset] &= value;
            self.dirty = true;
        }
    }
}
