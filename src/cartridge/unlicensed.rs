// SPDX-License-Identifier: MIT
//! Unlicensed North American discrete boards (`unlicensed.py`): Color Dreams
//! (11), Camerica/Codemasters (71, 232), AVE's NINA-03/06 (79) with HES's
//! multicart extension (113), and the other small unlicensed boards (38, 41,
//! 46, 104, 144, 167, 168, 216, 228, 487).
//!
//! Each from its NESdev wiki page; no judge ROM exists for any of them (the
//! reference's docstrings have the details and docs/claims.md the claims).
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 11: `ColorDreams`
//! - 38: `BitCorp38`
//! - 41: `Caltron41`
//! - 46: `RumbleStation46`
//! - 71: `Camerica`
//! - 79: `NINA03`
//! - 104: `Pegasus104`
//! - 113: `NINA03`
//! - 144: `DeathRace`
//! - 167: `Subor167`
//! - 168: `Racermate`
//! - 216: `RCM216`
//! - 228: `Action52`
//! - 232: `Camerica`
//! - 487: `NINA08`

use super::banked::{Banked, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        11 => Box::new(ColorDreams::new(image, t)),
        38 => Box::new(BitCorp38::new(image, t)),
        41 => Box::new(Caltron41::new(image, t)),
        46 => Box::new(RumbleStation46::new(image, t)),
        71 | 232 => Box::new(Camerica::new(image, t)),
        79 | 113 => Box::new(Nina03::new(image, t)),
        104 => Box::new(Pegasus104::new(image, t)),
        144 => Box::new(DeathRace {
            c: ColorDreams::new(image, t),
        }),
        167 => Box::new(Subor167::new(image, t)),
        168 => Box::new(Racermate::new(image, t)),
        216 => Box::new(Rcm216::new(image, t)),
        228 => Box::new(Action52::new(image, t)),
        487 => Box::new(Nina08::new(image, t)),
        _ => return Err(image),
    })
}

/// Mapper 11: a 74377 latch: `CCCCLLPP`, the 32 KiB PRG and 8 KiB CHR banks
/// (Color_Dreams.wiki). Submapper 2 has AND-type bus conflicts.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ColorDreams {
    pub b: Banked,
    pub bus_conflicts: bool,
}

impl ColorDreams {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> ColorDreams {
        let bus_conflicts = image.board.submapper == 2;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        ColorDreams { b, bus_conflicts }
    }
}

impl Cartridge for ColorDreams {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let value = if self.bus_conflicts {
            self.b.bus_conflict(address, value)
        } else {
            value
        };
        self.b.map_prg(0, 32, (value & 3) as i64);
        self.b.map_chr(0, 8, (value >> 4) as i64);
    }
}

/// Mappers 71 (BF9093/BF9097) and 232 (BF9096, the Quattro multicarts):
/// UNROM-like (INES_Mapper_071.wiki, _232.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Camerica {
    pub b: Banked,
    pub quattro: bool,
    pub aladdin: bool,
    pub outer: u8,
    pub inner: u8,
}

impl Camerica {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Camerica {
        let board = image.board.clone();
        let quattro = board.mapper == 232;
        let mut c = Camerica {
            b: Banked::new(image, cpu_cycle_ticks),
            quattro,
            aladdin: quattro && board.submapper == 1,
            outer: 0,
            inner: 0,
        };
        if board.mapper == 71 && board.submapper == 1 {
            c.b.set_mirroring(SCREEN_A);
        }
        c.b.map_chr(0, 8, 0);
        c.update();
        c
    }

    fn update(&mut self) {
        if self.quattro {
            self.b
                .map_prg(0, 16, ((self.outer << 2) | (self.inner & 3)) as i64);
            self.b.map_prg(2, 16, ((self.outer << 2) | 3) as i64);
        } else {
            self.b.map_prg(0, 16, self.inner as i64);
            let last = (self.b.prg.len() / 0x4000) as i64 - 1;
            self.b.map_prg(2, 16, last);
        }
    }
}

impl Cartridge for Camerica {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if address >= 0xC000 {
            self.inner = value & 0x0F;
        } else if self.quattro {
            let outer = (value >> 3) & 3;
            self.outer = if self.aladdin {
                (outer >> 1) | ((outer & 1) << 1)
            } else {
                outer
            };
        } else if address & 0xF000 == 0x9000 {
            self.b.set_mirroring(if value & 0x10 != 0 {
                SCREEN_B
            } else {
                SCREEN_A
            });
        }
        self.update();
    }
}

/// Mappers 79 (AVE NINA-03/06) and 113 (HES NTD-8), the register at
/// $4100-$5FFF (NINA-003-006.wiki, INES_Mapper_113.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Nina03 {
    pub b: Banked,
    pub multicart: bool,
}

impl Nina03 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Nina03 {
        let multicart = image.board.mapper == 113;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Nina03 { b, multicart }
    }
}

impl Cartridge for Nina03 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE100 != 0x4100 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if self.multicart {
            self.b.map_prg(0, 32, ((value >> 3) & 7) as i64);
            self.b
                .map_chr(0, 8, (((value >> 3) & 8) | (value & 7)) as i64);
            self.b.set_mirroring(if value & 0x80 != 0 {
                VERTICAL
            } else {
                HORIZONTAL
            });
        } else {
            self.b.map_prg(0, 32, ((value >> 3) & 1) as i64);
            self.b.map_chr(0, 8, (value & 7) as i64);
        }
    }
}

/// Mapper 168: Racermate Challenge II: UNROM with 64 KiB of CHR-RAM in 4 KiB
/// banks, an M2 counter driving /IRQ and a battery half (INES_Mapper_168.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Racermate {
    pub b: Banked,
    pub held: bool,
    pub protected: bool,
    pub counter: u16,
    pub irq: bool,
}

impl Racermate {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Racermate {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.chr = vec![0; 0x10000];
        b.chr_writable = true;
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 4, 0);
        b.map_chr(4, 4, 0);
        Racermate {
            b,
            held: false,
            protected: true,
            counter: 0,
            irq: false,
        }
    }

    fn battery_bank(&self, address: u16) -> bool {
        self.b.chr_offsets[address as usize >> 10] >= 0x8000
    }
}

impl Cartridge for Racermate {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x8000..0xC000).contains(&address) {
            self.b.map_prg(0, 16, (value >> 6) as i64);
            self.b.map_chr(4, 4, (value & 0x0F) as i64);
        } else if address >= 0xC000 {
            let held = value & 4 != 0;
            if self.held && !held {
                self.protected = false;
            }
            self.held = held;
            if held {
                self.counter = 0;
                self.irq = false;
            }
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }

    fn m2(&mut self, _t: i64) {
        if !self.held {
            self.counter = (self.counter + 1) & 0x7FF;
            self.irq = self.counter & 0x400 != 0;
        }
    }

    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 && self.protected && self.battery_bank(address) {
            return None;
        }
        self.b.base_ppu_read(address, t)
    }

    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x2000 && self.protected && self.battery_bank(address) {
            return;
        }
        self.b.base_ppu_write(address, value, t);
    }

    fn battery_ram(&self) -> Option<Vec<u8>> {
        Some(self.b.chr[0x8000..].to_vec())
    }

    fn restore(&mut self, data: &[u8]) {
        self.b.chr[0x8000..].copy_from_slice(data);
    }
}

/// Mapper 228: Active Enterprises' Action 52 and Cheetahmen II: the address
/// and data of a write latched (INES_Mapper_228.wiki); chip 2 of Action 52
/// does not exist and reads open bus.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Action52 {
    pub b: Banked,
    pub three_chips: bool,
    pub open_bus: bool,
}

impl Action52 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Action52 {
        let b = Banked::new(image, cpu_cycle_ticks);
        let mut a = Action52 {
            three_chips: b.prg.len() == 0x180000,
            b,
            open_bus: false,
        };
        a.latch(0, 0);
        a
    }

    fn latch(&mut self, address: u16, value: u8) {
        let (mut chip, page) = ((address as i64 >> 11) & 3, (address as i64 >> 6) & 0x1F);
        self.open_bus = self.three_chips && chip == 2;
        if self.three_chips && chip == 3 {
            chip = 2;
        }
        let bank = (chip << 5) | page;
        if address & 0x20 != 0 {
            self.b.map_prg(0, 16, bank);
            self.b.map_prg(2, 16, bank);
        } else {
            self.b.map_prg(0, 16, bank & !1);
            self.b.map_prg(2, 16, bank | 1);
        }
        self.b
            .map_chr(0, 8, (((address & 0x0F) << 2) | (value as u16 & 3)) as i64);
        self.b.set_mirroring(if address & 0x2000 != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        });
    }
}

impl Cartridge for Action52 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0x8000 && self.open_bus {
            return None;
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.latch(address, value);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 144 (Death Race): a Color Dreams board where the ROM's D0 always
/// wins the conflict (INES_Mapper_144.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DeathRace {
    pub c: ColorDreams,
}

impl Cartridge for DeathRace {
    banked_field!(c.b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.c.cpu_write(address, value, t);
            return;
        }
        let b = &mut self.c.b;
        let rom = b.prg[b.prg_offsets[(address as usize >> 13) & 3] + (address as usize & 0x1FFF)];
        let value = rom & (value | 1);
        b.map_prg(0, 32, (value & 3) as i64);
        b.map_chr(0, 8, (value >> 4) as i64);
    }
}

/// Mapper 41 (Caltron 6-in-1): an outer register in the address of
/// $6000-$67FF writes (INES_Mapper_041.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Caltron41 {
    pub b: Banked,
    pub outer: u16,
    pub inner: u8,
}

impl Caltron41 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Caltron41 {
        let mut c = Caltron41 {
            b: Banked::new(image, cpu_cycle_ticks),
            outer: 0,
            inner: 0,
        };
        c.b.prg_ram_enabled = false;
        c.update();
        c
    }

    fn update(&mut self) {
        self.b.map_prg(0, 32, (self.outer & 7) as i64);
        self.b.map_chr(
            0,
            8,
            ((((self.outer >> 3) & 3) << 2) | (self.inner as u16 & 3)) as i64,
        );
        self.b.set_mirroring(if self.outer & 0x20 != 0 {
            HORIZONTAL
        } else {
            VERTICAL
        });
    }
}

impl Cartridge for Caltron41 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = 0;
        self.inner = 0;
        self.update();
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x6800).contains(&address) {
            self.outer = address & 0x3F;
            self.update();
        } else if address >= 0x8000 && self.outer & 4 != 0 {
            self.inner = self.b.bus_conflict(address, value);
            self.update();
        }
    }
}

/// Mapper 38 (Bit Corp.'s Crime Busters): GNROM with the register at
/// $7000-$7FFF, `....CCPP` (INES_Mapper_038.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BitCorp38 {
    pub b: Banked,
}

impl BitCorp38 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> BitCorp38 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        BitCorp38 { b }
    }
}

impl Cartridge for BitCorp38 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x7000..0x8000).contains(&address) {
            self.b.map_prg(0, 32, (value & 3) as i64);
            self.b.map_chr(0, 8, ((value >> 2) & 3) as i64);
        }
    }
}

/// NES 2.0 mapper 487: AVE's NINA-08 (the unreleased Maxivision 30-in-1),
/// an outer register at $4180 over NINA-03's or Color Dreams' inner one
/// (NES_2.0_Mapper_487.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Nina08 {
    pub b: Banked,
    pub outer: u8,
    pub inner_prg: u8,
    pub inner_chr: u8,
}

impl Nina08 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Nina08 {
        let mut n = Nina08 {
            b: Banked::new(image, cpu_cycle_ticks),
            outer: 0,
            inner_prg: 0,
            inner_chr: 0,
        };
        n.update();
        n
    }

    fn update(&mut self) {
        let o = self.outer as i64;
        let big = o & 0x40 != 0;
        let bits = o & (if o & 0x20 != 0 { 0x1F } else { 0x0F }); // A15-A19 (A19 only past the first chip)
        let mut base32 = if big { bits & !1 } else { bits }; // in 32 KiB units
        if o & 0x20 != 0 {
            base32 += 16; // the second and third chips follow the first 512 KiB
        }
        let prg32 = base32 | if big { self.inner_prg as i64 & 1 } else { 0 };
        let chr8 = (base32 << 2) | (self.inner_chr as i64 & if big { 7 } else { 3 });
        self.b.map_prg(0, 32, prg32);
        self.b.map_chr(0, 8, chr8);
        self.b
            .set_mirroring(if o & 0x80 != 0 { HORIZONTAL } else { VERTICAL });
    }
}

impl Cartridge for Nina08 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xE180 == 0x4180 {
            self.outer = value;
        } else if address & 0xE180 == 0x4100 && self.outer & 0x20 == 0 {
            self.inner_prg = (value >> 3) & 1;
            self.inner_chr = value & 7;
        } else if address >= 0x8000 && self.outer & 0x20 != 0 {
            self.inner_prg = value & 1;
            self.inner_chr = (value >> 4) & 7;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// Mapper 104 (Pegasus 5-in-1 / Golden Five): a write-protectable game
/// register and an inner 16 KiB bank (PEGASUS_5_IN_1.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Pegasus104 {
    pub b: Banked,
    pub outer: u8,
    pub inner: u8,
}

impl Pegasus104 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Pegasus104 {
        let mut p = Pegasus104 {
            b: Banked::new(image, cpu_cycle_ticks),
            outer: 0,
            inner: 0,
        };
        p.b.map_chr(0, 8, 0);
        p.update();
        p
    }

    fn update(&mut self) {
        let game = (self.outer & 7) as i64;
        self.b.map_prg(0, 16, (game << 4) | self.inner as i64);
        self.b.map_prg(2, 16, (game << 4) | 0x0F);
    }
}

impl Cartridge for Pegasus104 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x8000..0xC000).contains(&address) {
            if self.outer & 8 == 0 {
                self.outer = value & 0x0F;
            }
        } else if address >= 0xC000 {
            self.inner = value & 0x0F;
        } else {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        self.update();
    }
}

/// Mapper 46 (Rumble Station 15-in-1): Color Dreams inside 64 KiB outer
/// banks (INES_Mapper_046.wiki). No PRG-RAM; power-on and reset clear both.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct RumbleStation46 {
    pub b: Banked,
    pub outer: u8,
    pub inner: u8,
}

impl RumbleStation46 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> RumbleStation46 {
        let mut r = RumbleStation46 {
            b: Banked::new(image, cpu_cycle_ticks),
            outer: 0,
            inner: 0,
        };
        r.b.prg_ram_enabled = false;
        r.update();
        r
    }

    fn update(&mut self) {
        let (outer, inner) = (self.outer as i64, self.inner as i64);
        self.b.map_prg(0, 32, ((outer & 0x0F) << 1) | (inner & 1));
        self.b
            .map_chr(0, 8, ((outer >> 4) << 3) | ((inner >> 4) & 7));
    }
}

impl Cartridge for RumbleStation46 {
    banked_field!(b);
    fn m2_gap(&mut self, _t: i64) {
        self.outer = 0;
        self.inner = 0;
        self.update();
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.outer = value;
        } else if address >= 0x8000 {
            self.inner = value;
        } else {
            return;
        }
        self.update();
    }
}

/// Mapper 167 (Subor's computer learning cartridges): four registers, banks
/// XORed in pairs (INES_Mapper_167.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Subor167 {
    pub b: Banked,
    pub regs: [u8; 4],
}

impl Subor167 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Subor167 {
        let mut s = Subor167 {
            b: Banked::new(image, cpu_cycle_ticks),
            regs: [0; 4],
        };
        s.b.map_chr(0, 8, 0);
        s.update();
        s
    }

    fn update(&mut self) {
        let r = self.regs.map(|x| x as i64);
        let bank = ((((r[0] ^ r[1]) >> 4) & 1) << 5) | ((r[2] ^ r[3]) & 0x1F);
        let mode = (r[1] >> 2) & 3;
        if mode == 0 {
            self.b.map_prg(0, 16, bank);
            self.b.map_prg(2, 16, 0x20);
        } else if mode == 1 {
            self.b.map_prg(0, 16, 0x1F);
            self.b.map_prg(2, 16, bank);
        } else {
            self.b.map_prg(0, 16, bank | 1);
            self.b.map_prg(2, 16, bank & !1);
        }
        self.b
            .set_mirroring(if r[0] & 1 != 0 { HORIZONTAL } else { VERTICAL });
    }
}

impl Cartridge for Subor167 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.regs[(address as usize >> 13) & 3] = value;
            self.update();
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 216 (Bonza, Magic Jewelry II): a latch of the address; $5000 the
/// SIM-card port (no card), $6000-$7FFF reads 0 (MAME's rcm.cpp, BSD-3).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Rcm216 {
    pub b: Banked,
}

impl Rcm216 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Rcm216 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Rcm216 { b }
    }
}

impl Cartridge for Rcm216 {
    banked_field!(b);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address & 0xF000 == 0x5000 {
            return Some(0x80 | (0x7E << 8));
        }
        if (0x6000..0x8000).contains(&address) {
            return Some(0);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            self.b.map_prg(0, 32, (address & 1) as i64);
            self.b.map_chr(0, 8, ((address >> 1) & 7) as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}
