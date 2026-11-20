// SPDX-License-Identifier: MIT
//! Kaiser's and others' FDS-to-cartridge conversions (`kaiser.py`): 8 KiB
//! PRG banks at $6000 and simple cycle counters standing in for the disk
//! system's timer. Each from its NESdev wiki page; no judge ROM exists, the
//! claims rest on sweeps against Mesen2.
//!
//! The reference's `KaiserSMB3` subclasses `KS7032`; here [`Ks7032`] carries
//! the SMB3 board's registers as an `Option`.

use super::banked::{Banked, HORIZONTAL, VERTICAL};
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// `self.prg[offset % len(self.prg)]`.
fn rom(b: &Banked, offset: i64) -> u8 {
    b.prg[offset.rem_euclid(b.prg.len() as i64) as usize]
}

/// `self.prg[index]` without a modulo: Python counts a negative index from the end.
fn rom_index(b: &Banked, index: i64) -> u8 {
    let index = if index < 0 {
        index + b.prg.len() as i64
    } else {
        index
    };
    b.prg[index as usize]
}

/// The last bank of `size` bytes: `len(self.prg) // size - 1`.
fn last(b: &Banked, size: usize) -> i64 {
    (b.prg.len() / size) as i64 - 1
}

/// Mapper 42 (Ai Senshi Nicol, Bio Miracle Bokutte Upa): $6000 banked, the
/// last 32 KiB fixed (INES_Mapper_042.wiki; 42.2, Green Beret, as its dumps
/// decide, C65).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser42 {
    pub b: Banked,
    pub green_beret: bool,
    pub bank6000: i64,
    pub counter: u16,
    pub counting: bool,
    pub irq: bool,
}

impl Kaiser42 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser42 {
        let green_beret = image.board.submapper == 2;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let bank = last(&b, 0x8000);
        b.map_prg(0, 32, bank);
        b.map_chr(0, 8, 0);
        Kaiser42 {
            b,
            green_beret,
            bank6000: 0,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for Kaiser42 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let register = address & 0xE003;
        if self.green_beret {
            if register == 0x8001 {
                self.bank6000 = ((value >> 1) & 0x0F) as i64;
            }
        } else if register == 0x8000 && !self.b.chr_writable {
            self.b.map_chr(0, 8, (value & 0x0F) as i64);
        } else if register == 0xE000 {
            self.bank6000 = (value & 0x0F) as i64;
        } else if register == 0xE001 {
            self.b
                .set_mirroring(if value & 8 != 0 { HORIZONTAL } else { VERTICAL });
        } else if register == 0xE002 {
            self.counting = value & 2 != 0;
            if !self.counting {
                self.counter = 0;
                self.irq = false;
            }
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = (self.counter + 1) & 0x7FFF;
            self.irq = self.counter & 0x6000 == 0x6000;
        }
    }
}

/// Mapper 326 (a bootleg Contra): 8 KiB PRG, 1 KiB CHR, a CIRAM bit per
/// nametable (krzysiobal's dump notes, forum t=17352).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Contra326 {
    pub b: Banked,
    pub nametables: [u8; 4],
}

impl Contra326 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Contra326 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let bank = last(&b, 0x2000);
        b.map_prg(3, 8, bank);
        Contra326 {
            b,
            nametables: [0, 0, 1, 1],
        }
    }
}

impl Cartridge for Contra326 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
        } else if address & 0x10 != 0 {
            let register = address & 0x0F;
            if register < 8 {
                self.b.map_chr(register as usize, 1, value as i64);
            } else if register < 12 {
                self.nametables[(register & 3) as usize] = value & 1;
            }
        } else if address < 0xE000 {
            self.b
                .map_prg(((address >> 13) & 3) as usize, 8, value as i64);
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.nametables[((address >> 10) & 3) as usize])
    }
}

/// The SMB3 board's registers on the KS202 (mapper 56).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Smb3 {
    pub low: [i64; 4],
    pub upper: [i64; 4],
}

/// Mapper 142: Kaiser KS-7032 (SMB2J and other FDS conversions;
/// INES_Mapper_142.wiki). With `smb3`, mapper 56 (Kaiser's Super Mario Bros.
/// 3, INES_Mapper_056.wiki): the KS202 plus CHR and mirroring.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7032 {
    pub b: Banked,
    pub select: u8,
    pub bank6000: i64,
    pub reload: u32,
    pub counter: u32,
    pub counting: bool,
    pub irq: bool,
    pub smb3: Option<Smb3>,
}

impl Ks7032 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7032 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let bank = last(&b, 0x2000);
        b.map_prg(3, 8, bank);
        b.map_chr(0, 8, 0);
        Ks7032 {
            b,
            select: 0,
            bank6000: 0,
            reload: 0,
            counter: 0,
            counting: false,
            irq: false,
            smb3: None,
        }
    }

    pub fn kaiser_smb3(image: RomImage, cpu_cycle_ticks: i64) -> Ks7032 {
        let mut m = Ks7032::new(image, cpu_cycle_ticks);
        m.smb3 = Some(Smb3 {
            low: [0, 0, 0, 0x0F],
            upper: [1, 1, 1, 1],
        });
        m.b.prg_ram = vec![0; 0x2000];
        m.b.prg_ram_mask = 0x1FFF;
        m.b.prg_ram_enabled = true;
        m.smb3_map();
        m
    }

    /// `KaiserSMB3._map`.
    fn smb3_map(&mut self) {
        if let Some(s) = &self.smb3 {
            let banks: [i64; 4] = std::array::from_fn(|w| s.upper[w] << 4 | s.low[w]);
            for (window, bank) in banks.into_iter().enumerate() {
                self.b.map_prg(window, 8, bank);
            }
        }
    }

    /// `KS7032.cpu_write`.
    fn ks7032_write(&mut self, address: u16, value: u8) {
        let region = address & 0xF000;
        if (0x8000..0xC000).contains(&region) {
            let shift = ((region - 0x8000) >> 10) as u32;
            self.reload = (self.reload & !(0xF << shift)) | ((value & 0x0F) as u32) << shift;
        } else if region == 0xC000 {
            self.counting = value & 2 != 0;
            if self.counting {
                self.counter = self.reload;
            }
            self.irq = false;
        } else if region == 0xD000 {
            self.irq = false;
        } else if region == 0xE000 {
            self.select = value & 7;
        } else if region == 0xF000 {
            if (1..=3).contains(&self.select) {
                self.b
                    .map_prg((self.select - 1) as usize, 8, (value & 0x0F) as i64);
            } else if self.select == 4 {
                self.bank6000 = (value & 0x0F) as i64;
            }
        }
    }
}

impl Cartridge for Ks7032 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if self.smb3.is_none() && (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.smb3.is_none() {
            self.ks7032_write(address, value);
            return;
        }
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        if address < 0xF000 {
            self.ks7032_write(address, value); // the counter and the bank select
            return;
        }
        let select = self.select;
        let s = self.smb3.as_mut().unwrap();
        if (1..=3).contains(&select) {
            s.low[(select - 1) as usize] = (value & 0x0F) as i64;
        }
        let region = address & 0xFC00;
        if region == 0xF000 {
            s.upper[(address & 3) as usize] = ((value >> 4) & 1) as i64;
        } else if region == 0xF800 {
            self.b
                .set_mirroring(if value & 1 != 0 { VERTICAL } else { HORIZONTAL });
        } else if region == 0xFC00 {
            self.b
                .map_chr((address & 7) as usize, 1, (value & 0x7F) as i64);
        }
        self.smb3_map();
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter += 1;
            if self.counter > 0xFFFF {
                self.counter = self.reload;
                self.irq = true;
            }
        }
    }
}

/// NES 2.0 mapper 303: Kaiser's conversion keeping the disk system's timer
/// and mirroring (NES_2.0_Mapper_303.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser303 {
    pub b: Banked,
    pub latched: i64,
    pub reload: u16,
    pub counter: u16,
    pub timer_enabled: bool,
    pub timer_repeat: bool,
    pub irq: bool,
}

impl Kaiser303 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser303 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        b.map_prg(2, 16, 2);
        b.map_chr(0, 8, 0);
        Kaiser303 {
            b,
            latched: 0,
            reload: 0,
            counter: 0,
            timer_enabled: false,
            timer_repeat: false,
            irq: false,
        }
    }
}

impl Cartridge for Kaiser303 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address == 0x4030 {
            let value = self.irq as u16;
            self.irq = false;
            return Some(value | 0xFE << 8);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xFF00 == 0x4A00 {
            self.latched = (((address >> 2) & 3) | ((address >> 4) & 4)) as i64;
        } else if address & 0xFF00 == 0x5100 {
            self.b.map_prg(0, 16, self.latched);
        } else if address == 0x4020 {
            self.reload = (self.reload & 0xFF00) | value as u16;
        } else if address == 0x4021 {
            self.reload = (self.reload & 0x00FF) | (value as u16) << 8;
        } else if address == 0x4022 {
            self.timer_repeat = value & 1 != 0;
            self.timer_enabled = value & 2 != 0;
            if self.timer_enabled {
                self.counter = self.reload;
            } else {
                self.irq = false;
            }
        } else if address == 0x4025 {
            self.b
                .set_mirroring(if value & 8 != 0 { HORIZONTAL } else { VERTICAL });
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.timer_enabled {
            if self.counter == 0 {
                self.irq = true;
                self.counter = self.reload;
                self.timer_enabled = self.timer_repeat;
            } else {
                self.counter -= 1;
            }
        }
    }
}

/// NES 2.0 mapper 304: the 09-034A conversions (Whirlwind Manu's SMB2J,
/// Volleyball, Zanac; NES_2.0_Mapper_304.wiki; no counter, as on the cartridge).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Board09034A {
    pub b: Banked,
    pub bank6000: i64,
}

impl Board09034A {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Board09034A {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Board09034A { b, bank6000: 4 }
    }
}

impl Cartridge for Board09034A {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x4042..=0x4055).contains(&address) {
            return Some(0xFF);
        }
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address == 0x4027 {
            self.bank6000 = (4 | (value & 1)) as i64;
        }
    }
}

/// Mapper 40: NTDEC 2722 (SMB2J conversions) and 40.1, the 2752 multicart
/// (INES_Mapper_040.wiki, from FCEUX's and Nestopia's source).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ntdec2722 {
    pub b: Banked,
    pub multicart: bool,
    pub outer: u16,
    pub inner: i64,
    pub counting: bool,
    pub counter: u16,
    pub irq: bool,
}

impl Ntdec2722 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ntdec2722 {
        let multicart = image.board.submapper == 1;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let mut m = Ntdec2722 {
            b,
            multicart,
            outer: 0,
            inner: 0,
            counting: false,
            counter: 0,
            irq: false,
        };
        m.update();
        m
    }

    fn update(&mut self) {
        let o = self.outer;
        if self.multicart && o & 8 != 0 {
            let bank16 = ((((o >> 3) & 1) << 2) | ((o >> 5) & 3)) as i64;
            if o & 0x10 != 0 {
                self.b.map_prg(0, 32, bank16 >> 1);
            } else {
                self.b.map_prg(0, 16, bank16);
                self.b.map_prg(2, 16, bank16);
            }
        } else {
            for (window, bank) in [4, 5, self.inner, 7].into_iter().enumerate() {
                self.b.map_prg(window, 8, bank);
            }
        }
        self.b.map_chr(0, 8, ((o >> 1) & 3) as i64);
        if self.multicart {
            self.b
                .set_mirroring(if o & 1 != 0 { HORIZONTAL } else { VERTICAL });
        }
    }
}

impl Cartridge for Ntdec2722 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, 6 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let region = address & 0xE000;
        if region == 0x8000 {
            self.counting = false;
            self.counter = 0;
            self.irq = false;
        } else if region == 0xA000 {
            self.counting = true;
        } else if region == 0xC000 && self.multicart {
            self.outer = address & 0x7F;
            self.update();
        } else if region == 0xE000 {
            self.inner = (value & 7) as i64;
            self.update();
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = (self.counter + 1) & 0x1FFF;
            self.irq = self.counter & 0x1000 != 0;
        }
    }
}

/// Mapper 43's $4022 values and the 8 KiB banks they select (INES_Mapper_043.wiki).
pub const TONY_I_BANKS: [i64; 8] = [4, 3, 4, 4, 4, 7, 5, 6];

/// Mapper 43 (SMB2J conversions on the TONY-I and YS-612 boards): three PRG
/// chips (INES_Mapper_043.wiki; the file layout is Mesen2's).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TonyI43 {
    pub b: Banked,
    pub counter: u32,
    pub counting: bool,
    pub irq: bool,
}

impl TonyI43 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> TonyI43 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        for (window, bank) in [1, 0, 4, 9].into_iter().enumerate() {
            b.map_prg(window, 8, bank);
        }
        b.map_chr(0, 8, 0);
        TonyI43 {
            b,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for TonyI43 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x5000..0x6000).contains(&address) {
            return Some(rom(&self.b, 0x10000 + (address & 0x7FF) as i64) as u16);
        }
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, 2 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let register = address & 0x71FF;
        if register == 0x4022 {
            self.b.map_prg(2, 8, TONY_I_BANKS[(value & 7) as usize]);
        } else if register == 0x4122 || address & 0xF1FF == 0x8122 {
            self.counting = value & 1 != 0;
            self.counter = 0;
            self.irq = false;
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter += 1;
            if self.counter >= 0x1000 {
                self.counting = false;
                self.irq = true;
            }
        }
    }
}

/// NES 2.0 mapper 309: Whirlwind Manu's Ai Senshi Nicol conversion
/// (NES_2.0_Mapper_309.wiki, masks taken as $E000).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Whirlwind309 {
    pub b: Banked,
}

impl Whirlwind309 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Whirlwind309 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        for (window, bank) in [0, 13, 14, 15].into_iter().enumerate() {
            b.map_prg(window, 8, bank);
        }
        b.map_chr(0, 8, 0);
        Whirlwind309 { b }
    }
}

impl Cartridge for Whirlwind309 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let region = address & 0xE000;
        if region == 0x8000 {
            self.b.map_prg(0, 8, (value & 0x0F) as i64);
        } else if region == 0xE000 {
            self.b
                .set_mirroring(if value & 8 != 0 { HORIZONTAL } else { VERTICAL });
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 305: a Kaiser conversion in 2 KiB PRG banks
/// (NES_2.0_Mapper_305.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser305 {
    pub b: Banked,
    pub low_banks: [i64; 4],
}

impl Kaiser305 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser305 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_chr(0, 8, 0);
        b.set_mirroring(VERTICAL);
        Kaiser305 {
            b,
            low_banks: [0; 4],
        }
    }

    fn bank(&self, bank: i64, address: u16) -> u8 {
        rom(&self.b, bank * 0x800 + (address & 0x7FF) as i64)
    }
}

impl Cartridge for Kaiser305 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, _t: i64) -> Option<u16> {
        if address >= 0x8000 {
            return Some(self.bank(15 - ((address - 0x8000) >> 11) as i64, address) as u16);
        }
        if address >= 0x6000 {
            let bank = self.low_banks[((address >> 11) & 3) as usize];
            return Some(self.bank(bank, address) as u16);
        }
        None
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x8000..0xA000).contains(&address) {
            self.low_banks[((address >> 11) & 3) as usize] = value as i64;
        }
    }
}

/// NES 2.0 mapper 306: a Kaiser conversion banking $6000 by address
/// (NES_2.0_Mapper_306.wiki, mask $D903, FCEUX's).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser306 {
    pub b: Banked,
    pub bank6000: i64,
}

impl Kaiser306 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser306 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.set_mirroring(VERTICAL);
        b.map_prg(0, 32, 3);
        b.map_chr(0, 8, 0);
        // Not given: A6-A2 clear.
        Kaiser306 { b, bank6000: 8 }
    }
}

impl Cartridge for Kaiser306 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        if address & 0xD903 == 0xD903 {
            let high = if address & 0x40 != 0 {
                (address >> 4) & 3
            } else {
                2
            };
            self.bank6000 = ((high << 2 | ((address >> 2) & 3)) as i64).min(0x0B);
        }
    }
}

/// NES 2.0 mapper 522 (Fūun Shōrinken): an MMC3 clone wired for two PRG
/// banks and RAM at $C000 (NES_2.0_Mapper_522.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser522 {
    pub b: Banked,
    pub ram: Vec<u8>,
    pub select: u8,
    pub second_last: i64,
}

impl Kaiser522 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser522 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let last = last(&b, 0x2000);
        b.map_prg(3, 8, last);
        b.map_chr(0, 8, 0);
        Kaiser522 {
            b,
            ram: vec![0; 0x2000],
            select: 0,
            second_last: last - 1,
        }
    }
}

impl Cartridge for Kaiser522 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom_index(
                &self.b,
                self.second_last * 0x2000 + (address & 0x1FFF) as i64,
            ) as u16);
        }
        if (0xC000..0xE000).contains(&address) {
            return Some(self.ram[(address & 0x1FFF) as usize] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0xC000..0xE000).contains(&address) {
            self.ram[(address & 0x1FFF) as usize] = value;
        } else if (0x8000..0xA000).contains(&address) {
            if address & 1 != 0 {
                if self.select == 6 || self.select == 7 {
                    self.b.map_prg((self.select - 6) as usize, 8, value as i64);
                }
            } else {
                self.select = value & 7;
            }
        }
    }
}

/// NES 2.0 mapper 525 (UNL-KS7021A: Kaiser's VRC2 mapper hacks of Contra,
/// Getsu Fūma Den; NES_2.0_Mapper_525.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7021A {
    pub b: Banked,
}

impl Ks7021A {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7021A {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let bank = last(&b, 0x4000);
        b.map_prg(2, 16, bank);
        Ks7021A { b }
    }
}

impl Cartridge for Ks7021A {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let region = address & 0xF000;
        if region == 0x8000 {
            self.b.map_prg(0, 16, ((value >> 1) & 7) as i64);
        } else if region == 0x9000 {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if address & 0xF000 == 0xB000 && address & 0x0FF8 == 0 {
            self.b.map_chr((address & 7) as usize, 1, value as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// NES 2.0 mapper 302 (Kaiser KS7057): eight 2 KiB PRG banks set by nibbles
/// (NES_2.0_Mapper_302.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7057 {
    pub b: Banked,
    /// 2 KiB banks: $8000-$9FFF (0-3), $6000-$7FFF (4-7).
    pub small: [u8; 8],
}

impl Ks7057 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7057 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(1, 8, 0x0D);
        b.map_prg(2, 16, 7);
        b.map_chr(0, 8, 0);
        Ks7057 { b, small: [0; 8] }
    }

    fn small(&self, index: usize, address: u16) -> u8 {
        rom(
            &self.b,
            self.small[index] as i64 * 0x800 + (address & 0x7FF) as i64,
        )
    }
}

impl Cartridge for Ks7057 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(self.small(4 + ((address >> 11) & 3) as usize, address) as u16);
        }
        if (0x8000..0xA000).contains(&address) {
            return Some(self.small(((address >> 11) & 3) as usize, address) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x8000..0xA000).contains(&address) {
            self.b
                .set_mirroring(if value & 1 != 0 { VERTICAL } else { HORIZONTAL });
            return;
        }
        let register = address & 0xF003;
        if (0xB000..=0xE003).contains(&register) {
            let index = (((register >> 12) - 0x0B) * 2 + ((register >> 1) & 1)) as usize;
            let shift = if register & 1 != 0 { 4 } else { 0 };
            self.small[index] = (self.small[index] & (0xF0 >> shift)) | (value & 0x0F) << shift;
        }
    }
}

/// NES 2.0 mapper 312 (UNL-KS7013B: Kaiser's Highway Star;
/// NES_2.0_Mapper_312.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7013B {
    pub b: Banked,
}

impl Ks7013B {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7013B {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 16, 0);
        b.map_prg(2, 16, 7);
        b.map_chr(0, 8, 0);
        Ks7013B { b }
    }
}

impl Cartridge for Ks7013B {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address >= 0x8000 {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if address >= 0x6000 {
            self.b.map_prg(0, 16, (value & 7) as i64);
        }
    }
}

/// NES 2.0 mapper 307: a Kaiser conversion with RAM and ROM in 4 KiB pieces
/// (NES_2.0_Mapper_307.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Kaiser307 {
    pub b: Banked,
    pub ram: Vec<u8>,
    pub select: u8,
    /// By nametable: $2000, $2400, $2800, $2C00.
    pub pages: [u8; 4],
}

impl Kaiser307 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Kaiser307 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(3, 8, 15);
        b.map_chr(0, 8, 0);
        Kaiser307 {
            b,
            ram: vec![0; 0x2000],
            select: 0,
            pages: [0; 4],
        }
    }

    fn rom4k(&self, bank: i64, address: u16) -> u8 {
        rom(&self.b, bank * 0x1000 + (address & 0xFFF) as i64)
    }
}

impl Cartridge for Kaiser307 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x7000).contains(&address) {
            return Some(self.ram[(address & 0xFFF) as usize] as u16);
        }
        if (0x7000..0x8000).contains(&address) {
            return Some(self.rom4k(15, address) as u16);
        }
        if (0xA000..0xB000).contains(&address) {
            return Some(self.rom4k(28, address) as u16);
        }
        if (0xB000..0xC000).contains(&address) {
            return Some(self.ram[0x1000 | (address & 0xFFF) as usize] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x7000).contains(&address) {
            self.ram[(address & 0xFFF) as usize] = value;
        } else if (0xB000..0xC000).contains(&address) {
            self.ram[0x1000 | (address & 0xFFF) as usize] = value;
        } else if (0x8000..0xA000).contains(&address) {
            if address & 1 == 0 {
                self.select = value & 7;
            } else if self.select == 6 {
                self.b.map_prg(0, 8, value as i64);
            } else if self.select == 7 {
                self.b.map_prg(2, 8, value as i64);
            } else if (2..=5).contains(&self.select) {
                let quadrant = [0, 2, 1, 3][(self.select - 2) as usize];
                self.pages[quadrant] = value & 1;
            }
        }
    }

    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.pages[((address >> 10) & 3) as usize])
    }
}

/// NES 2.0 mapper 521 (Korean Igo): a 16 KiB bank register at $5020 (mask
/// $F030; NES_2.0_Mapper_521.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Board521 {
    pub b: Banked,
    pub last: i64,
}

impl Board521 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Board521 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 16, 0);
        let last = b.prg.len() as i64 - 0x4000;
        b.map_chr(0, 8, 0);
        Board521 { b, last }
    }
}

impl Cartridge for Board521 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if address >= 0xC000 {
            return Some(rom_index(&self.b, self.last + (address & 0x3FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF030 == 0x5020 {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// Mapper 125 (Whirlwind Manu's Monty no Doki Doki Daisassou): $6000
/// banked, RAM at $C000 (INES_Mapper_125.wiki, mask taken as $E000).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Whirlwind125 {
    pub b: Banked,
    pub ram: Vec<u8>,
    pub bank6000: i64,
}

impl Whirlwind125 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Whirlwind125 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        for (window, bank) in [(0, 12), (1, 13), (3, 15)] {
            b.map_prg(window, 8, bank);
        }
        b.map_chr(0, 8, 0);
        Whirlwind125 {
            b,
            ram: vec![0; 0x2000],
            bank6000: 0,
        }
    }
}

impl Cartridge for Whirlwind125 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        if (0xC000..0xE000).contains(&address) {
            return Some(self.ram[(address & 0x1FFF) as usize] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.bank6000 = (value & 0x0F) as i64;
        } else if (0xC000..0xE000).contains(&address) {
            self.ram[(address & 0x1FFF) as usize] = value;
        }
    }
}

/// NES 2.0 mapper 526 (Sangoku III, BJ-56): direct 1 KiB CHR and 8 KiB PRG
/// registers (NES_2.0_Mapper_526.wiki, mask taken as $800F).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Board526 {
    pub b: Banked,
    pub counter: u16,
    pub irq: bool,
}

impl Board526 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Board526 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let bank = last(&b, 0x2000);
        b.map_prg(3, 8, bank);
        Board526 {
            b,
            counter: 0,
            irq: false,
        }
    }
}

impl Cartridge for Board526 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0x0F;
        if register < 8 {
            self.b.map_chr(register as usize, 1, value as i64);
        } else if register < 0x0C {
            self.b.map_prg((register - 8) as usize, 8, value as i64);
        } else if register == 0x0D || register == 0x0F {
            self.counter = 0;
            self.irq = false;
        }
    }

    fn m2(&mut self, _t: i64) {
        self.counter = (self.counter + 1) & 0x1FFF;
        self.irq = self.counter & 0x1000 != 0;
    }
}

/// Mapper 106: a discrete Super Mario Bros. 3 bootleg (two 128 KiB PRG
/// chips, combined here; INES_Mapper_106.wiki, mask $800F).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Smb3Bootleg106 {
    pub b: Banked,
    pub counter: u16,
    pub enabled: bool,
    pub irq: bool,
}

impl Smb3Bootleg106 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Smb3Bootleg106 {
        Smb3Bootleg106 {
            b: Banked::new(image, cpu_cycle_ticks),
            counter: 0,
            enabled: false,
            irq: false,
        }
    }
}

impl Cartridge for Smb3Bootleg106 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let register = address & 0x0F;
        if register < 4 {
            self.b.map_chr(
                register as usize,
                1,
                ((value & 0x7E) as u16 | (register & 1)) as i64,
            );
        } else if register < 8 {
            self.b.map_chr(register as usize, 1, (value & 0x7F) as i64);
        } else if register == 8 || register == 0x0B {
            self.b
                .map_prg((register - 8) as usize, 8, 16 + (value & 0x0F) as i64);
        } else if register == 9 || register == 0x0A {
            self.b
                .map_prg((register - 8) as usize, 8, (value & 0x1F) as i64);
        } else if register == 0x0C {
            self.b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if register == 0x0D {
            self.counter = 0;
            self.enabled = false;
            self.irq = false;
        } else if register == 0x0E {
            self.counter = (self.counter & 0xFF00) | value as u16;
        } else {
            self.counter = (self.counter & 0x00FF) | (value as u16) << 8;
            self.enabled = true;
        }
    }

    fn m2(&mut self, _t: i64) {
        self.counter = self.counter.saturating_add(1);
        self.irq = self.enabled && self.counter == 0xFFFF;
    }
}

/// Mapper 50 (the N-32 SMB2J conversion, "Romeo"): a scrambled bank
/// register and a counter (INES_Mapper_050.wiki, mask $4120; the 13-bit
/// counter the wiki expects).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct N32Mapper50 {
    pub b: Banked,
    pub counter: u16,
    pub counting: bool,
    pub irq: bool,
}

impl N32Mapper50 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> N32Mapper50 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        for (window, bank) in [8, 9, 0, 0x0B].into_iter().enumerate() {
            b.map_prg(window, 8, bank);
        }
        b.map_chr(0, 8, 0);
        N32Mapper50 {
            b,
            counter: 0,
            counting: false,
            irq: false,
        }
    }
}

impl Cartridge for N32Mapper50 {
    banked_field!(b);

    fn counts_m2(&self) -> bool {
        true
    }

    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, 0x0F * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let register = address & 0x4120;
        if address < 0x6000 && register == 0x4020 {
            let bank = (value & 8) | (value & 1) << 2 | ((value >> 1) & 3);
            self.b.map_prg(2, 8, bank as i64);
        } else if address < 0x6000 && register == 0x4120 {
            self.counting = value & 1 != 0;
            if !self.counting {
                self.counter = 0;
                self.irq = false;
            }
        }
    }

    fn m2(&mut self, _t: i64) {
        if self.counting {
            self.counter = (self.counter + 1) & 0x1FFF;
            self.irq = self.counter & 0x1000 != 0;
        }
    }
}

/// Mapper 120 (Whirlwind Manu's Tobidase Daisakusen): $6000 banked by a
/// $41FF register (INES_Mapper_120.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Lh15 {
    pub b: Banked,
    pub bank6000: i64,
}

impl Lh15 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Lh15 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 32, 2);
        b.map_chr(0, 8, 0);
        Lh15 { b, bank6000: 0 }
    }
}

impl Cartridge for Lh15 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            return Some(rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if address & 0xE100 == 0x4100 {
            self.bank6000 = (value & 7) as i64;
        }
    }
}

/// Mapper 103: a port of the FDS Doki Doki Panic, RAM over parts of the ROM
/// (INES_Mapper_103.wiki, mask $F000).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DokiDoki103 {
    pub b: Banked,
    pub low_ram: Vec<u8>,
    pub high_ram: Vec<u8>,
    pub rom_only: bool,
    pub bank6000: i64,
}

impl DokiDoki103 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> DokiDoki103 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        b.map_prg(0, 32, 3);
        b.map_chr(0, 8, 0);
        DokiDoki103 {
            b,
            low_ram: vec![0; 0x2000],
            high_ram: vec![0; 0x2000],
            rom_only: false,
            bank6000: 0,
        }
    }
}

impl Cartridge for DokiDoki103 {
    banked_field!(b);

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            if self.rom_only {
                return Some(
                    rom(&self.b, self.bank6000 * 0x2000 + (address & 0x1FFF) as i64) as u16,
                );
            }
            return Some(self.low_ram[(address & 0x1FFF) as usize] as u16);
        }
        if (0xB800..0xD800).contains(&address) && !self.rom_only {
            return Some(self.high_ram[(address - 0xB800) as usize] as u16);
        }
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.low_ram[(address & 0x1FFF) as usize] = value;
            return;
        }
        if (0xB800..0xD800).contains(&address) {
            self.high_ram[(address - 0xB800) as usize] = value;
        }
        let region = address & 0xF000;
        if region == 0x8000 {
            self.bank6000 = (value & 0x0F) as i64;
        } else if region == 0xE000 {
            self.b
                .set_mirroring(if value & 8 != 0 { HORIZONTAL } else { VERTICAL });
        } else if region == 0xF000 {
            self.rom_only = value & 0x10 != 0;
        }
    }
}

/// NES 2.0 mapper 346 (UNL-KS7012: Kaiser's Zanac): 32 KiB banks by magic
/// addresses (NES_2.0_Mapper_346.wiki; exact addresses here).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ks7012 {
    pub b: Banked,
}

impl Ks7012 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ks7012 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 1);
        b.map_chr(0, 8, 0);
        Ks7012 { b }
    }
}

impl Cartridge for Ks7012 {
    banked_field!(b);

    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address == 0xE0A0 {
            self.b.map_prg(0, 32, 0);
        } else if address == 0xEE36 {
            self.b.map_prg(0, 32, 1);
        } else {
            self.b.base_cpu_write(address, value, t);
        }
    }
}

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    let t = cpu_cycle_ticks;
    Ok(match mapper {
        40 => Box::new(Ntdec2722::new(image, t)),
        42 => Box::new(Kaiser42::new(image, t)),
        43 => Box::new(TonyI43::new(image, t)),
        50 => Box::new(N32Mapper50::new(image, t)),
        56 => Box::new(Ks7032::kaiser_smb3(image, t)),
        103 => Box::new(DokiDoki103::new(image, t)),
        106 => Box::new(Smb3Bootleg106::new(image, t)),
        120 => Box::new(Lh15::new(image, t)),
        125 => Box::new(Whirlwind125::new(image, t)),
        142 => Box::new(Ks7032::new(image, t)),
        302 => Box::new(Ks7057::new(image, t)),
        303 => Box::new(Kaiser303::new(image, t)),
        304 => Box::new(Board09034A::new(image, t)),
        305 => Box::new(Kaiser305::new(image, t)),
        306 => Box::new(Kaiser306::new(image, t)),
        307 => Box::new(Kaiser307::new(image, t)),
        309 => Box::new(Whirlwind309::new(image, t)),
        312 => Box::new(Ks7013B::new(image, t)),
        326 => Box::new(Contra326::new(image, t)),
        346 => Box::new(Ks7012::new(image, t)),
        521 => Box::new(Board521::new(image, t)),
        522 => Box::new(Kaiser522::new(image, t)),
        525 => Box::new(Ks7021A::new(image, t)),
        526 => Box::new(Board526::new(image, t)),
        _ => return Err(image),
    })
}
