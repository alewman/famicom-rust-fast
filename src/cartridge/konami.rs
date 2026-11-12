//! Konami's VRC mappers: VRC1 (75), VRC2 and VRC4 (21, 22, 23, 25), VRC3
//! (73), VRC6 (24, 26), VRC7 (85), the VRC IRQ, and the VRC2/VRC4 clones 183,
//! 222, 266, 272, 298, 308, 529, 530 and 544 (`konami.py`).
//!
//! AWJ's `vrctest` judges the VRC2/VRC4 wiring and IRQ; the VRC1 rests on a
//! sweep against Mesen2; the VRC6's sound level is rainwarrior's `db_vrc6`.
//!
//! **The class hierarchy.** The reference's VRC2/VRC4 clones subclass
//! `VRC24` and override the helpers it calls on `self` (`map_prg`, `map_chr`,
//! `_register`, `_update_prg`, `irq_acknowledge`); here those are the methods
//! of [`Vrc24Like`], whose default bodies are the base's, as in `mmc3.rs`.
//! Every VRC2/VRC4 board is a struct holding a [`Vrc24`] (the base class's
//! state, itself the plain board) and implementing [`Vrc24Like`] and
//! [`Cartridge`]; [`vrc24_cartridge!`] writes the `Cartridge` methods a board
//! takes from the base. Other modules' subclasses (waixing's `WaixingVRC4`,
//! multiasic's `HostVRC4`, chinese's embedded VRC2b) build the same way from
//! [`Vrc24::state`] and [`vrc4e`].
//!
//! **The VRC7's sound is switched off for now**: the board has no model of
//! the DS1001 while the licence of the core it used (ym2413-rust, from
//! ym2413-python) is settled. Its two ports take writes and drop them.

use super::banked::{Banked, Mirroring, HORIZONTAL, SCREEN_A, SCREEN_B, VERTICAL};
use super::connector::{expansion_gain, Cartridge};
use super::eeprom::Microwire;
use super::ines::RomImage;
use crate::banked_field;

const ARRANGEMENTS: [Mirroring; 4] = [VERTICAL, HORIZONTAL, SCREEN_A, SCREEN_B];

/// `image.with_board(replace(image.board, mapper=..., submapper=...))`.
pub fn rewired(image: RomImage, mapper: u16, submapper: u8) -> RomImage {
    let mut image = image;
    image.board.mapper = mapper;
    image.board.submapper = submapper;
    image
}

/// `VRC4E`: VRC4e's wiring (23.2), which the clones start from.
pub fn vrc4e(image: RomImage) -> RomImage {
    rewired(image, 23, 2)
}

// -- VRC1 ------------------------------------------------------------------------

/// Mapper 75: three 8 KiB PRG banks and the last fixed; two 4 KiB CHR banks
/// of five bits (VRC1.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Vrc1 {
    pub b: Banked,
    pub chr_banks: [u8; 2],
}

impl Vrc1 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Vrc1 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        for window in 0..3 {
            b.map_prg(window, 8, 0);
        }
        let last = (b.prg.len() / 0x2000) as i64 - 1;
        b.map_prg(3, 8, last);
        let mut vrc1 = Vrc1 {
            b,
            chr_banks: [0, 0],
        };
        vrc1.update_chr();
        vrc1
    }

    fn update_chr(&mut self) {
        self.b.map_chr(0, 4, self.chr_banks[0] as i64);
        self.b.map_chr(4, 4, self.chr_banks[1] as i64);
    }
}

impl Cartridge for Vrc1 {
    banked_field!(b);
    fn cpu_write(&mut self, address: u16, value: u8, _t: i64) {
        let region = address & 0xF000;
        if region == 0x8000 || region == 0xA000 || region == 0xC000 {
            self.b
                .map_prg(((region - 0x8000) >> 13) as usize, 8, value as i64);
        } else if region == 0x9000 {
            if !self.b.four_screen {
                self.b
                    .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
            }
            self.chr_banks = [
                (self.chr_banks[0] & 0x0F) | (value & 2) << 3,
                (self.chr_banks[1] & 0x0F) | (value & 4) << 2,
            ];
            self.update_chr();
        } else if region == 0xE000 || region == 0xF000 {
            let half = ((region >> 12) & 1) as usize;
            self.chr_banks[half] = (self.chr_banks[half] & 0x10) | (value & 0x0F);
            self.update_chr();
        }
    }
}

// -- VRC3 ------------------------------------------------------------------------

/// Mapper 73: VRC3 (Salamander): 16 KiB PRG and the last fixed, 8 KiB
/// CHR-RAM, a cycle counter (VRC3.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Vrc3 {
    pub b: Banked,
    pub reload: u32,
    pub counter: u32,
    pub control: u8,
    pub irq: bool,
}

impl Vrc3 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Vrc3 {
        let mut b = Banked::new(image, cpu_cycle_ticks);
        let last = (b.prg.len() / 0x4000) as i64 - 1;
        b.map_prg(2, 16, last);
        b.map_chr(0, 8, 0);
        Vrc3 {
            b,
            reload: 0,
            counter: 0,
            control: 0,
            irq: false,
        }
    }
}

impl Cartridge for Vrc3 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }
    fn m2(&mut self, _t: i64) {
        if self.control & 2 == 0 {
            return;
        }
        if self.control & 4 != 0 {
            let mut low = (self.counter & 0xFF) + 1;
            if low > 0xFF {
                low = self.reload & 0xFF;
                self.irq = true;
            }
            self.counter = (self.counter & 0xFF00) | low;
        } else {
            self.counter += 1;
            if self.counter > 0xFFFF {
                self.counter = self.reload;
                self.irq = true;
            }
        }
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        if region < 0xC000 {
            let shift = ((region - 0x8000) >> 10) as u32; // $8000 0, $9000 4, $A000 8, $B000 12
            self.reload = (self.reload & !(0xF << shift)) | ((value as u32) & 0xF) << shift;
        } else if region == 0xC000 {
            self.control = value & 7;
            if value & 2 != 0 {
                self.counter = self.reload;
            }
            self.irq = false;
        } else if region == 0xD000 {
            self.control = (self.control & !2) | (self.control & 1) << 1;
            self.irq = false;
        } else if region == 0xF000 {
            self.b.map_prg(0, 16, (value & 7) as i64);
        }
    }
}

// -- the VRC IRQ -----------------------------------------------------------------

/// The VRC4/VRC6/VRC7 IRQ (VRC_IRQ.wiki): an 8-bit up-counter clocked by CPU
/// cycles, through a 114/114/113 prescaler in scanline mode. `irq` is the
/// mapper's /IRQ (the reference's `self.irq`, shared with the subclasses).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VrcIrq {
    pub irq: bool,
    pub latch: u8,
    pub counter: u8,
    pub prescaler: i32,
    pub enabled: bool,
    pub after_ack: bool,
    pub cycle_mode: bool,
}

impl Default for VrcIrq {
    fn default() -> Self {
        VrcIrq::new()
    }
}

impl VrcIrq {
    /// The class attribute `irq = False` and `irq_reset()`.
    pub fn new() -> VrcIrq {
        let mut irq = VrcIrq {
            irq: false,
            latch: 0,
            counter: 0,
            prescaler: 0,
            enabled: false,
            after_ack: false,
            cycle_mode: false,
        };
        irq.reset();
        irq
    }

    /// `irq_reset` (leaves /IRQ alone).
    pub fn reset(&mut self) {
        self.latch = 0;
        self.counter = 0;
        self.prescaler = 341;
        self.enabled = false;
        self.after_ack = false;
        self.cycle_mode = false;
    }

    /// `VRCIrq.m2`.
    pub fn m2(&mut self) {
        if !self.enabled {
            return;
        }
        if !self.cycle_mode {
            self.prescaler -= 3;
            if self.prescaler > 0 {
                return;
            }
            self.prescaler += 341;
        }
        if self.counter == 0xFF {
            self.counter = self.latch;
            self.irq = true;
        } else {
            self.counter += 1;
        }
    }

    /// `irq_control`.
    pub fn control(&mut self, value: u8) {
        self.irq = false;
        self.prescaler = 341;
        self.after_ack = value & 1 != 0;
        self.enabled = value & 2 != 0;
        self.cycle_mode = value & 4 != 0;
        if self.enabled {
            self.counter = self.latch;
        }
    }

    /// `irq_acknowledge`.
    pub fn acknowledge(&mut self) {
        self.irq = false;
        self.enabled = self.after_ack;
    }
}

// -- VRC2 and VRC4 ---------------------------------------------------------------

/// `VRC_WIRING`: the CPU address lines each board wires to the chip's A0 and
/// A1 (NES_2.0_submappers.wiki, "021/023/025"), by (mapper, submapper);
/// submapper 0 of 21/23/25 answers both of its mapper's wirings at once.
pub fn vrc_wiring(mapper: u16, submapper: u8) -> Option<(&'static [u32], &'static [u32])> {
    Some(match (mapper, submapper) {
        (21, 1) => (&[1], &[2]), // VRC4a
        (21, 2) => (&[6], &[7]), // VRC4c
        (21, 0) => (&[1, 6], &[2, 7]),
        (22, 0) => (&[1], &[0]), // VRC2a
        (23, 1) => (&[0], &[1]), // VRC4f
        (23, 2) => (&[2], &[3]), // VRC4e
        (23, 3) => (&[0], &[1]), // VRC2b
        (23, 0) => (&[0, 2], &[1, 3]),
        (25, 1) => (&[1], &[0]), // VRC4b
        (25, 2) => (&[3], &[2]), // VRC4d
        (25, 3) => (&[1], &[0]), // VRC2c
        (25, 0) => (&[1, 3], &[0, 2]),
        _ => return None,
    })
}

/// Mappers 21, 22, 23, 25: VRC2 and VRC4, told apart and wired by mapper and
/// submapper (VRC2_and_VRC4.wiki). The base class's state; also the plain board.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Vrc24 {
    pub b: Banked,
    pub vrc_irq: VrcIrq,
    pub vrc2: bool,
    /// An instance attribute in the reference: the VRC2 has no IRQ.
    pub counts_m2: bool,
    pub halve_chr: bool,
    pub lines0: Vec<u32>,
    pub lines1: Vec<u32>,
    pub prg_registers: [u8; 2],
    pub swap: bool,
    pub chr_registers: [u16; 8],
    pub latch_bit: u8,
}

impl Vrc24 {
    /// `VRC24.__init__` up to (not including) its `_update_prg()`, which the
    /// board's constructor calls through [`Vrc24Like::update_prg`].
    pub fn state(image: RomImage, cpu_cycle_ticks: i64) -> Vrc24 {
        let board = image.board.clone();
        let b = Banked::new(image, cpu_cycle_ticks);
        let vrc2 = board.mapper == 22 || board.submapper == 3;
        let (lines0, lines1) = vrc_wiring(board.mapper, board.submapper).unwrap_or_else(|| {
            panic!(
                "VRC_WIRING has no wiring for ({}, {})",
                board.mapper, board.submapper
            )
        });
        let prg_ram_enabled = vrc2 && !b.prg_ram.is_empty();
        let mut v = Vrc24 {
            b,
            vrc_irq: VrcIrq::new(),
            vrc2,
            counts_m2: !vrc2,
            halve_chr: board.mapper == 22,
            lines0: lines0.to_vec(),
            lines1: lines1.to_vec(),
            prg_registers: [0, 0],
            swap: false,
            chr_registers: [0; 8],
            latch_bit: 0,
        };
        v.b.prg_ram_enabled = prg_ram_enabled;
        v.vrc_irq.reset();
        v
    }

    /// The whole `VRC24.__init__`: the plain board.
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Vrc24 {
        let mut v = Vrc24::state(image, cpu_cycle_ticks);
        v.update_prg();
        v
    }
}

/// The VRC24's overridable methods. Default bodies: the base's.
pub trait Vrc24Like {
    fn vrc24(&self) -> &Vrc24;
    fn vrc24_mut(&mut self) -> &mut Vrc24;

    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.vrc24_mut().b.map_prg(window, size_kib, bank);
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.vrc24_mut().b.map_chr(window, size_kib, bank);
    }
    /// `_register`: the chip's A1-A0 from the CPU address.
    fn register(&self, address: u16) -> u8 {
        vrc24_register(self.vrc24(), address)
    }
    /// `_update_prg`.
    fn update_prg(&mut self) {
        vrc24_update_prg(self);
    }
    /// `irq_acknowledge` (`VRCIrq`'s).
    fn irq_acknowledge(&mut self) {
        self.vrc24_mut().vrc_irq.acknowledge();
    }
}

/// `VRC24._register`.
pub fn vrc24_register(v: &Vrc24, address: u16) -> u8 {
    let low = v.lines0.iter().any(|&line| (address >> line) & 1 != 0) as u8;
    let high = v.lines1.iter().any(|&line| (address >> line) & 1 != 0) as u8;
    low | high << 1
}

/// `VRC24._update_prg`.
pub fn vrc24_update_prg<T: Vrc24Like + ?Sized>(this: &mut T) {
    let v = this.vrc24();
    let last = (v.b.prg.len() / 0x2000) as i64 - 1;
    let (mut first, mut third) = (v.prg_registers[0] as i64, last - 1);
    if v.swap {
        std::mem::swap(&mut first, &mut third);
    }
    let second = v.prg_registers[1] as i64;
    this.map_prg(0, 8, first);
    this.map_prg(1, 8, second);
    this.map_prg(2, 8, third);
    this.map_prg(3, 8, last);
}

/// `VRC24.cpu_read`.
pub fn vrc24_cpu_read(v: &Vrc24, address: u16, t: i64) -> Option<u16> {
    if v.vrc2 && v.b.prg_ram.is_empty() && (0x6000..0x7000).contains(&address) {
        return Some(v.latch_bit as u16 | 0xFE << 8);
    }
    v.b.base_cpu_read(address, t)
}

/// `VRC24.cpu_write`.
pub fn vrc24_cpu_write<T: Vrc24Like + ?Sized>(this: &mut T, address: u16, value: u8, t: i64) {
    if address < 0x8000 {
        let v = this.vrc24_mut();
        if v.vrc2 && v.b.prg_ram.is_empty() {
            if (0x6000..0x7000).contains(&address) {
                v.latch_bit = value & 1;
            }
            return;
        }
        v.b.base_cpu_write(address, value, t);
        return;
    }
    let region = address & 0xF000;
    let register = this.register(address);
    let vrc2 = this.vrc24().vrc2;
    if region == 0x8000 || region == 0xA000 {
        this.vrc24_mut().prg_registers[(region == 0xA000) as usize] = value & 0x1F;
        this.update_prg();
    } else if region == 0x9000 {
        if vrc2 {
            this.vrc24_mut()
                .b
                .set_mirroring(if value & 1 != 0 { HORIZONTAL } else { VERTICAL });
        } else if register < 2 {
            this.vrc24_mut()
                .b
                .set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
        } else if register == 2 {
            let v = this.vrc24_mut();
            v.b.prg_ram_enabled = !v.b.prg_ram.is_empty() && value & 1 != 0;
            v.swap = value & 2 != 0;
            this.update_prg();
        }
    } else if region < 0xF000 {
        let v = this.vrc24_mut();
        let bank = (((region - 0xB000) >> 11) | (register >> 1) as u16) as usize;
        let mut old = v.chr_registers[bank];
        let value = value as u16;
        if register & 1 != 0 {
            old = (old & 0x0F) | (value & if v.vrc2 { 0x0F } else { 0x1F }) << 4;
        } else {
            old = (old & 0x1F0) | (value & 0x0F);
        }
        v.chr_registers[bank] = old;
        let one = if v.halve_chr { old >> 1 } else { old };
        this.map_chr(bank, 1, one as i64);
    } else if !vrc2 {
        if register == 0 {
            let irq = &mut this.vrc24_mut().vrc_irq;
            irq.latch = (irq.latch & 0xF0) | (value & 0x0F);
        } else if register == 1 {
            let irq = &mut this.vrc24_mut().vrc_irq;
            irq.latch = (irq.latch & 0x0F) | (value & 0x0F) << 4;
        } else if register == 2 {
            this.vrc24_mut().vrc_irq.control(value);
        } else {
            this.irq_acknowledge();
        }
    }
}

/// The `Cartridge` methods a VRC2/VRC4 board takes from the base class:
/// `vrc24_cartridge!(method, ...)` inside `impl Cartridge for Type` writes
/// `banked`, `banked_mut`, `counts_m2` and `irq`, and each named one of
/// `cpu_read`, `cpu_write` and `m2`.
#[macro_export]
macro_rules! vrc24_cartridge {
    ($($method:ident),* $(,)?) => {
        fn banked(&self) -> Option<&$crate::cartridge::Banked> {
            Some(&$crate::cartridge::konami::Vrc24Like::vrc24(self).b)
        }
        fn banked_mut(&mut self) -> Option<&mut $crate::cartridge::Banked> {
            Some(&mut $crate::cartridge::konami::Vrc24Like::vrc24_mut(self).b)
        }
        fn counts_m2(&self) -> bool {
            $crate::cartridge::konami::Vrc24Like::vrc24(self).counts_m2
        }
        fn irq(&self) -> bool {
            $crate::cartridge::konami::Vrc24Like::vrc24(self).vrc_irq.irq
        }
        $( $crate::vrc24_cartridge!(@ $method); )*
    };
    (@ cpu_read) => {
        fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
            $crate::cartridge::konami::vrc24_cpu_read(
                $crate::cartridge::konami::Vrc24Like::vrc24(self), address, t)
        }
    };
    (@ cpu_write) => {
        fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
            $crate::cartridge::konami::vrc24_cpu_write(self, address, value, t);
        }
    };
    (@ m2) => {
        fn m2(&mut self, _t: i64) {
            $crate::cartridge::konami::Vrc24Like::vrc24_mut(self).vrc_irq.m2();
        }
    };
}

impl Vrc24Like for Vrc24 {
    fn vrc24(&self) -> &Vrc24 {
        self
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        self
    }
}

impl Cartridge for Vrc24 {
    vrc24_cartridge!(cpu_read, cpu_write, m2);
}

// -- VRC6 ------------------------------------------------------------------------

/// VRC6_audio.wiki: a 12-bit divider clocking a 16-step duty generator, 15 down to 0.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Vrc6Pulse {
    /// MDDD VVVV
    pub control: u8,
    pub period: u16,
    pub enabled: bool,
    pub counter: u16,
    pub step: u8,
}

impl Vrc6Pulse {
    pub fn new() -> Vrc6Pulse {
        Vrc6Pulse {
            control: 0,
            period: 0,
            enabled: false,
            counter: 0,
            step: 15,
        }
    }

    pub fn output(&self) -> i32 {
        let control = self.control;
        if self.enabled && (control & 0x80 != 0 || self.step <= (control >> 4) & 7) {
            return (control & 0x0F) as i32;
        }
        0
    }

    /// One CPU cycle; true when the output may have moved.
    pub fn clock(&mut self, shift: u32) -> bool {
        if self.counter != 0 {
            self.counter -= 1;
            return false;
        }
        self.counter = self.period >> shift;
        self.step = self.step.wrapping_sub(1) & 15;
        true
    }
}

impl Default for Vrc6Pulse {
    fn default() -> Self {
        Vrc6Pulse::new()
    }
}

/// VRC6_audio.wiki: the same divider clocking an accumulator that adds its
/// rate on every second clock, six times, and clears on the fourteenth.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Vrc6Saw {
    pub rate: u8,
    pub period: u16,
    pub enabled: bool,
    pub counter: u16,
    pub step: u8,
    pub accumulator: u8,
}

impl Vrc6Saw {
    pub fn output(&self) -> i32 {
        if self.enabled {
            (self.accumulator >> 3) as i32
        } else {
            0
        }
    }

    pub fn clock(&mut self, shift: u32) -> bool {
        if self.counter != 0 {
            self.counter -= 1;
            return false;
        }
        self.counter = self.period >> shift;
        if !self.enabled {
            return false; // the divider runs on; the accumulator holds zero
        }
        let step = self.step + 1;
        if step == 14 {
            self.step = 0;
            self.accumulator = 0;
            return true;
        }
        self.step = step;
        if step & 1 == 0 {
            self.accumulator = self.accumulator.wrapping_add(self.rate);
            return true;
        }
        false
    }
}

/// The VRC6's level, measured: a full-volume square +0.2 dB against the
/// APU's (rainwarrior's db_vrc6 on an AV Famicom, nesdev-forum/17741.html).
pub const VRC6_DB: f64 = 0.2;

/// Mappers 24 (VRC6a) and 26 (VRC6b, A0 and A1 swapped), with the sound (VRC6.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Vrc6 {
    pub b: Banked,
    pub vrc_irq: VrcIrq,
    pub swap_lines: bool,
    pub banking: u8,
    pub chr_registers: [u8; 8],
    pub pulses: [Vrc6Pulse; 2],
    pub saw: Vrc6Saw,
    pub halt: bool,
    pub shift: u32,
    pub gain: f64,
    pub expansion: f64,
}

impl Vrc6 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Vrc6 {
        let swap_lines = image.board.mapper == 26;
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let mut vrc6 = Vrc6 {
            b,
            vrc_irq: VrcIrq::new(),
            swap_lines,
            banking: 0,
            chr_registers: [0; 8],
            pulses: [Vrc6Pulse::new(), Vrc6Pulse::new()],
            saw: Vrc6Saw::default(),
            halt: false,
            shift: 0,
            gain: expansion_gain(VRC6_DB, 15),
            expansion: 0.0,
        };
        vrc6.vrc_irq.reset();
        vrc6.b.map_prg(0, 16, 0);
        vrc6.b.map_prg(2, 8, 0);
        let last = (vrc6.b.prg.len() / 0x2000) as i64 - 1;
        vrc6.b.map_prg(3, 8, last);
        vrc6.update_chr();
        vrc6
    }

    fn sound_write(&mut self, region: u16, register: u16, value: u8) {
        if region == 0x9000 && register == 3 {
            self.halt = value & 1 != 0;
            self.shift = if value & 4 != 0 {
                8
            } else if value & 2 != 0 {
                4
            } else {
                0
            };
            return;
        }
        if region == 0xB000 {
            let saw = &mut self.saw;
            if register == 0 {
                saw.rate = value & 0x3F;
            } else if register == 1 {
                saw.period = (saw.period & 0xF00) | value as u16;
            } else {
                saw.period = (saw.period & 0x0FF) | ((value as u16) & 0x0F) << 8;
                saw.enabled = value & 0x80 != 0;
                if !saw.enabled {
                    // The saw's accumulator holds zero (its divider runs on).
                    saw.accumulator = 0;
                    saw.step = 0;
                }
            }
        } else {
            let pulse = &mut self.pulses[(region == 0xA000) as usize];
            if register == 0 {
                pulse.control = value;
            } else if register == 1 {
                pulse.period = (pulse.period & 0xF00) | value as u16;
            } else {
                pulse.period = (pulse.period & 0x0FF) | ((value as u16) & 0x0F) << 8;
                pulse.enabled = value & 0x80 != 0;
                if !pulse.enabled {
                    // The duty generator restarts from the beginning.
                    pulse.step = 15;
                }
            }
        }
        self.mix();
    }

    fn mix(&mut self) {
        let level = self.pulses[0].output() + self.pulses[1].output() + self.saw.output();
        self.expansion = level as f64 * self.gain;
    }

    #[allow(clippy::needless_range_loop)] // the window is the register's index
    fn update_chr(&mut self) {
        let r = self.chr_registers;
        let mode = self.banking & 3;
        if mode == 0 {
            for window in 0..8 {
                self.b.map_chr(window, 1, r[window] as i64);
            }
            return;
        }
        // 2 KiB banks: with D5 set PPU A10 picks the half (the register's
        // low bit ignored); clear, both halves are the register's 1 KiB bank.
        if mode == 1 {
            for i in 0..4 {
                self.two(2 * i, r[i]);
            }
        } else {
            for window in 0..4 {
                self.b.map_chr(window, 1, r[window] as i64);
            }
            self.two(4, r[4]);
            self.two(6, r[5]);
        }
    }

    /// `_update_chr`'s `two`: a 2 KiB bank.
    fn two(&mut self, window: usize, bank: u8) {
        if self.banking & 0x20 != 0 {
            self.b.map_chr(window, 2, (bank >> 1) as i64);
        } else {
            self.b.map_chr(window, 1, bank as i64);
            self.b.map_chr(window + 1, 1, bank as i64);
        }
    }
}

impl Cartridge for Vrc6 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn expansion_audio(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.vrc_irq.irq
    }
    fn expansion(&self) -> f64 {
        self.expansion
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        let mut register = address & 3;
        if self.swap_lines {
            register = (register & 1) << 1 | register >> 1;
        }
        if region == 0x8000 {
            self.b.map_prg(0, 16, (value & 0x0F) as i64);
        } else if region == 0xC000 {
            self.b.map_prg(2, 8, (value & 0x1F) as i64);
        } else if region == 0xD000 || region == 0xE000 {
            self.chr_registers[(((region == 0xE000) as u16) << 2 | register) as usize] = value;
            self.update_chr();
        } else if region == 0xF000 {
            if register == 0 {
                self.vrc_irq.latch = value;
            } else if register == 1 {
                self.vrc_irq.control(value);
            } else if register == 2 {
                self.vrc_irq.acknowledge();
            }
        } else if region == 0xB000 && register == 3 {
            self.b.prg_ram_enabled = !self.b.prg_ram.is_empty() && value & 0x80 != 0;
            self.banking = value & 0x3F;
            self.b
                .set_mirroring(ARRANGEMENTS[((value >> 2) & 3) as usize]);
            self.update_chr();
        } else if region == 0x9000 || region == 0xA000 || region == 0xB000 {
            self.sound_write(region, register, value);
        }
    }
    fn m2(&mut self, _t: i64) {
        self.vrc_irq.m2();
        if self.halt {
            return;
        }
        let shift = self.shift;
        let mut moved = self.pulses[0].enabled && self.pulses[0].clock(shift);
        if self.pulses[1].enabled && self.pulses[1].clock(shift) {
            moved = true;
        }
        if self.saw.clock(shift) {
            moved = true;
        }
        if moved {
            self.mix();
        }
    }
}

// -- VRC7 ------------------------------------------------------------------------

/// `VRC7_SELECT`: the address line that selects a VRC7 register's second
/// half, by submapper (VRC7.wiki): VRC7b A3, VRC7a A4; submapper 0 both.
pub fn vrc7_select(submapper: u8) -> u16 {
    match submapper {
        1 => 0x08,
        2 => 0x10,
        _ => 0x18,
    }
}

/// Mapper 85: the VRC7's banking and IRQ (VRC7.wiki). The sound chip, a
/// DS1001, is not modelled for now: its ports take writes and drop them.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Vrc7 {
    pub b: Banked,
    pub vrc_irq: VrcIrq,
    pub select: u16,
    pub sound_silenced: bool,
}

impl Vrc7 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Vrc7 {
        let select = vrc7_select(image.board.submapper);
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.prg_ram_enabled = false;
        let mut vrc7 = Vrc7 {
            b,
            vrc_irq: VrcIrq::new(),
            select,
            sound_silenced: false,
        };
        vrc7.vrc_irq.reset();
        for window in 0..3 {
            vrc7.b.map_prg(window, 8, 0);
        }
        let last = (vrc7.b.prg.len() / 0x2000) as i64 - 1;
        vrc7.b.map_prg(3, 8, last);
        for window in 0..8 {
            vrc7.b.map_chr(window, 1, 0);
        }
        vrc7
    }
}

impl Cartridge for Vrc7 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.vrc_irq.irq
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            self.b.base_cpu_write(address, value, t);
            return;
        }
        let region = address & 0xF000;
        let second = address & self.select != 0;
        if region == 0x8000 {
            self.b
                .map_prg(if second { 1 } else { 0 }, 8, (value & 0x3F) as i64);
        } else if region == 0x9000 {
            if !second && address & 0x30 == 0 {
                self.b.map_prg(2, 8, (value & 0x3F) as i64);
            }
            // $9010 and $9030, the sound chip's ports: no chip here for now.
        } else if (0xA000..=0xD000).contains(&region) {
            self.b.map_chr(
                (((region - 0xA000) >> 11) | second as u16) as usize,
                1,
                value as i64,
            );
        } else if region == 0xE000 {
            if second {
                self.vrc_irq.latch = value;
            } else {
                self.b.prg_ram_enabled = !self.b.prg_ram.is_empty() && value & 0x80 != 0;
                self.sound_silenced = value & 0x40 != 0;
                self.b.set_mirroring(ARRANGEMENTS[(value & 3) as usize]);
            }
        } else if region == 0xF000 {
            if second {
                self.vrc_irq.acknowledge();
            } else {
                self.vrc_irq.control(value);
            }
        }
    }
    fn m2(&mut self, _t: i64) {
        self.vrc_irq.m2();
    }
}

// -- the VRC2/VRC4 clones ----------------------------------------------------------

/// NES 2.0 mapper 308: the TH2131-1 board (NTDEC's Batman): a VRC2 with its
/// own IRQ (NES_2.0_Mapper_308.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Th2131 {
    pub v: Vrc24,
    pub low: u16,
    pub high: u8,
    pub counting: bool,
}

impl Th2131 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Th2131 {
        let mut board = Th2131 {
            v: Vrc24::state(rewired(image, 23, 3), cpu_cycle_ticks),
            low: 0,
            high: 0,
            counting: false,
        };
        board.update_prg();
        board.v.counts_m2 = true;
        board
    }
}

impl Vrc24Like for Th2131 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
}

impl Cartridge for Th2131 {
    vrc24_cartridge!(cpu_read);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let register = address & 0xF003;
        if register == 0xF000 {
            self.counting = false;
            self.low = 0;
            self.v.vrc_irq.irq = false;
        } else if register == 0xF001 {
            self.counting = true;
        } else if register == 0xF003 {
            self.high = value >> 4;
        } else if register < 0xF000 {
            vrc24_cpu_write(self, address, value, t);
        }
    }
    fn m2(&mut self, _t: i64) {
        if !self.counting {
            return;
        }
        self.low = (self.low + 1) & 0xFFF;
        if self.low == 2048 {
            self.high = self.high.wrapping_sub(1) & 0x0F;
        }
        self.v.vrc_irq.irq = self.high == 0 && self.low < 2048;
    }
}

/// NES 2.0 mapper 298 (UNL-TF1201: Lethal Weapon): a VRC4b clone whose $F003
/// only acknowledges (NES_2.0_Mapper_298.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tf1201 {
    pub v: Vrc24,
}

impl Tf1201 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Tf1201 {
        let mut board = Tf1201 {
            v: Vrc24::state(rewired(image, 25, 1), cpu_cycle_ticks),
        };
        board.update_prg();
        board
    }
}

impl Vrc24Like for Tf1201 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn irq_acknowledge(&mut self) {
        self.v.vrc_irq.irq = false;
    }
}

impl Cartridge for Tf1201 {
    vrc24_cartridge!(cpu_read, cpu_write, m2);
}

/// Mapper 183 (Shui Guan Pipe, a Gimmick! bootleg): VRC4e with an 8 KiB bank
/// at $6000 (INES_Mapper_183.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ShuiGuan183 {
    pub v: Vrc24,
    pub bank6000: usize,
}

impl ShuiGuan183 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> ShuiGuan183 {
        let mut board = ShuiGuan183 {
            v: Vrc24::state(rewired(image, 23, 2), cpu_cycle_ticks),
            bank6000: 0,
        };
        board.update_prg();
        board.v.b.prg_ram_enabled = false;
        board
    }
}

impl Vrc24Like for ShuiGuan183 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
}

impl Cartridge for ShuiGuan183 {
    vrc24_cartridge!(m2);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if (0x6000..0x8000).contains(&address) {
            let prg = &self.v.b.prg;
            return Some(
                prg[(self.bank6000 * 0x2000 + (address as usize & 0x1FFF)) % prg.len()] as u16,
            );
        }
        vrc24_cpu_read(&self.v, address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if (0x6000..0x8000).contains(&address) {
            self.bank6000 = (address & 0x0F) as usize;
        } else {
            vrc24_cpu_write(self, address, value, t);
        }
    }
}

/// Mapper 266 (UNL-CITYFIGHT): City Fighter IV, a VRC4e clone with A13 and
/// A14 exchanged and a 32 KiB PRG bank (NES_2.0_Mapper_266.wiki; the DAC is
/// not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CityFighter266 {
    pub v: Vrc24,
    pub prg32: i64,
}

impl CityFighter266 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> CityFighter266 {
        let mut board = CityFighter266 {
            prg32: 0,
            v: Vrc24::state(vrc4e(image), cpu_cycle_ticks),
        };
        board.update_prg();
        board
    }
}

impl Vrc24Like for CityFighter266 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn update_prg(&mut self) {
        let bank = self.prg32;
        self.map_prg(0, 32, bank);
    }
}

impl Cartridge for CityFighter266 {
    vrc24_cartridge!(cpu_read, m2);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address < 0x8000 {
            vrc24_cpu_write(self, address, value, t);
        } else if address & 0xF80C == 0x900C {
            self.prg32 = ((value >> 2) & 3) as i64;
            self.update_prg();
        } else if address & 0xF000 == 0x9000 {
            if address & 0x0C == 0 {
                vrc24_cpu_write(self, address, value, t);
            }
        } else if address & 0xF000 != 0x8000 {
            let a13 = (address >> 13) & 1;
            let a14 = (address >> 14) & 1;
            vrc24_cpu_write(self, (address & 0x9FFF) | a14 << 13 | a13 << 14, value, t);
        }
    }
}

/// Mapper 222: pirate VRC2b boards: Dragon Ninja (222.1, CTC-31) and Super
/// Bros. 8 (222.2), with a '4020 and two 74161s for the IRQ (forum t=12654).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ninja222 {
    pub v: Vrc24,
    pub whole_chr: bool,
    pub loading: bool,
    pub count: u8,
    pub divider: u16,
}

impl Ninja222 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ninja222 {
        let whole_chr = image.board.submapper == 2;
        let mut board = Ninja222 {
            whole_chr,
            v: Vrc24::state(rewired(image, 23, 3), cpu_cycle_ticks),
            loading: true,
            count: 0,
            divider: 0,
        };
        board.update_prg();
        board.v.counts_m2 = true;
        board
    }
}

impl Vrc24Like for Ninja222 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
}

impl Cartridge for Ninja222 {
    vrc24_cartridge!(cpu_read);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if self.whole_chr && (0xB000..0xF000).contains(&address) {
            if address & 1 == 0 {
                vrc24_cpu_write(self, address, value & 0x0F, t);
                vrc24_cpu_write(self, address | 1, value >> 4, t);
            }
        } else if address & 0xF003 == 0xF000 {
            self.loading = true;
        } else if address & 0xF003 == 0xF002 {
            self.loading = false;
        } else if address & 0xF001 == 0xF001 {
            self.v.vrc_irq.irq = false;
            if self.loading {
                self.count = value;
            }
        } else {
            vrc24_cpu_write(self, address, value, t);
        }
    }
    fn m2(&mut self, _t: i64) {
        if self.loading || self.v.vrc_irq.irq {
            self.divider = 0;
            return;
        }
        self.divider = (self.divider + 1) & 0x3FFF;
        if self.divider & 0x7F == 0x40 {
            let low = ((self.count & 0x0F) + 1) & 0x0F;
            let mut high = self.count >> 4;
            if low == 15 {
                high = (high + 1) & 0x0F;
            }
            self.count = high << 4 | low;
            if high == 15 {
                self.v.vrc_irq.irq = true;
            }
        }
    }
}

/// Mapper 272: a bootleg Akumajou Special: a VRC2b clone with a PAL for IRQs
/// and mirroring (lidnariq's and krzysiobal's analyses, forum t=15302).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct KidDracula272 {
    pub v: Vrc24,
    pub arrangement: Mirroring,
    pub extended: u8,
    pub a13: u16,
    pub count: u32,
    pub running: bool,
}

impl KidDracula272 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> KidDracula272 {
        let mut board = KidDracula272 {
            v: Vrc24::state(rewired(image, 23, 3), cpu_cycle_ticks),
            arrangement: VERTICAL,
            extended: 0,
            a13: 0,
            count: 0,
            running: false,
        };
        board.update_prg();
        board
    }
}

impl Vrc24Like for KidDracula272 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
}

impl Cartridge for KidDracula272 {
    vrc24_cartridge!(cpu_read, m2);
    fn watches_ppu_address(&self) -> bool {
        true
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x8000 {
            let extra = address & 0xC00C;
            if extra == 0x8004 {
                self.extended = value & 3;
            } else if extra == 0x800C {
                self.v.vrc_irq.irq = true;
            } else if extra == 0xC004 {
                self.v.vrc_irq.irq = false;
            } else if extra == 0xC008 {
                self.running = true;
            } else if extra == 0xC00C {
                self.v.vrc_irq.irq = false;
                self.running = false;
                self.count = 0;
            }
            if address & 0xF003 == 0x9000 {
                self.arrangement = if value & 1 != 0 { HORIZONTAL } else { VERTICAL };
            }
        }
        vrc24_cpu_write(self, address, value, t);
        if address >= 0x8000 {
            let mode = if self.extended & 2 != 0 {
                if self.extended & 1 != 0 {
                    SCREEN_B
                } else {
                    SCREEN_A
                }
            } else {
                self.arrangement
            };
            self.v.b.set_mirroring(mode);
        }
    }
    fn ppu_address(&mut self, address: u16, _t: i64) {
        let a13 = address & 0x2000;
        if self.a13 != 0 && a13 == 0 && self.running {
            self.count += 1;
            if self.count == 86 {
                self.count = 0;
                self.v.vrc_irq.irq = true;
            }
        }
        self.a13 = a13;
    }
}

/// `swap_bits`: `value` with bits `a` and `b` exchanged.
pub fn swap_bits(value: i64, a: u32, b: u32) -> i64 {
    if ((value >> a) ^ (value >> b)) & 1 != 0 {
        return value ^ (1 << a | 1 << b);
    }
    value
}

/// Mapper 530 (UNL-AX5705): Super Mario Bros. Pocker Mali, a scrambled VRC4f
/// clone (NES_2.0_Mapper_530.wiki; A3 stands in for A12, C66).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Ax5705 {
    pub v: Vrc24,
}

impl Ax5705 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Ax5705 {
        let mut board = Ax5705 {
            v: Vrc24::state(rewired(image, 23, 1), cpu_cycle_ticks),
        };
        board.update_prg();
        board
    }
}

impl Vrc24Like for Ax5705 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn map_prg(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.v.b.map_prg(window, size_kib, swap_bits(bank, 1, 3));
    }
    fn map_chr(&mut self, window: usize, size_kib: usize, bank: i64) {
        self.v.b.map_chr(window, size_kib, swap_bits(bank, 5, 6));
    }
}

impl Cartridge for Ax5705 {
    vrc24_cartridge!(cpu_read, m2);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        let mut address = address;
        if address >= 0x8000 {
            address = (address & 0xE003) | (address & 8) << 9;
        }
        vrc24_cpu_write(self, address, value, t);
    }
}

/// Mapper 529 (UNL-T-230): bootleg Dragon Ball Z: a VRC4e clone with 16 KiB
/// banking and, with PRG-NVRAM in the header, a 93C56 (NES_2.0_Mapper_529.wiki).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct T230 {
    pub v: Vrc24,
    pub eeprom: Option<Microwire>,
}

impl T230 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> T230 {
        let nvram = image.board.prg_nvram;
        let mut board = T230 {
            v: Vrc24::state(vrc4e(image), cpu_cycle_ticks),
            eeprom: None,
        };
        board.update_prg();
        board.eeprom = if nvram != 0 {
            Some(Microwire::c93c56())
        } else {
            None
        };
        board.v.b.prg_ram = Vec::new();
        board.v.b.prg_ram_enabled = false;
        board
    }
}

impl Vrc24Like for T230 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn update_prg(&mut self) {
        let bank = self.v.prg_registers[1] as i64;
        self.map_prg(0, 16, bank);
        let last = (self.v.b.prg.len() / 0x4000) as i64 - 1;
        self.map_prg(2, 16, last);
    }
}

impl Cartridge for T230 {
    vrc24_cartridge!(m2);
    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        if let Some(eeprom) = &self.eeprom {
            if address & 0xF000 == 0x5000 {
                return Some(eeprom.output as u16 | 0xFE << 8);
            }
        }
        vrc24_cpu_read(&self.v, address, t)
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if let Some(eeprom) = &mut self.eeprom {
            if address & 0x8800 == 0x8800 {
                eeprom.lines((value >> 2) & 1, (value >> 1) & 1, value & 1);
            }
        }
        if address & 0xF000 != 0x8000 {
            vrc24_cpu_write(self, address, value, t);
        }
    }
    fn battery_ram(&self) -> Option<Vec<u8>> {
        self.eeprom.as_ref().map(|e| e.memory.clone())
    }
    fn restore(&mut self, data: &[u8]) {
        if let Some(eeprom) = &mut self.eeprom {
            eeprom.memory = data.to_vec();
        }
    }
}

/// `FS306_WINDOWS`: mapper 544's CHR-RAM window by the value written to
/// CHR-ROM (D7, D6, D3, D1): (first bank, banks); None for ROM only.
pub fn fs306_window(key: u16) -> Option<(u16, u16)> {
    match key {
        0x80 => Some((0x28, 4)),
        0x82 => Some((0x00, 4)),
        0x88 => Some((0x4C, 4)),
        0x8A => Some((0x64, 4)),
        0xC0 => Some((0x46, 2)),
        0xC2 => Some((0x7C, 2)),
        0xC8 => Some((0x04, 2)),
        0xCA => None,
        _ => panic!("FS306_WINDOWS has no key {key:#x}"),
    }
}

/// Mapper 544: Waixing's FS306 (Bawang de Dalu): a VRC4 clone with a third
/// PRG bank, 2 KiB of CHR-RAM among the banks and per-nametable CIRAM pages
/// (NES_2.0_Mapper_544.wiki; the IRQ's extra M2 is not modelled).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Fs306 {
    pub v: Vrc24,
    pub prg_c000: i64,
    pub rom: Vec<u8>,
    pub chr_ram: Vec<u8>,
    pub chr_ram_window: [bool; 8],
    pub chr_banks: [i64; 8],
    pub ram_range: Option<(u16, u16)>,
    pub ciram: [u8; 4],
}

impl Fs306 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Fs306 {
        let rom = image.chr.clone();
        let mut board = Fs306 {
            prg_c000: 0,
            rom,
            chr_ram: vec![0; 0x800],
            chr_ram_window: [false; 8],
            chr_banks: [0; 8],
            ram_range: fs306_window(0x80),
            ciram: [0; 4],
            v: Vrc24::state(vrc4e(image), cpu_cycle_ticks),
        };
        board.update_prg();
        board.v.lines0 = vec![10];
        board.v.lines1 = vec![11];
        board.ciram = if board.v.b.mirroring == VERTICAL {
            [0, 1, 0, 1]
        } else {
            [0, 0, 1, 1]
        };
        let size = board.v.b.prg_ram.len().max(0x2000);
        board.v.b.prg_ram = vec![0; size];
        board.v.b.prg_ram_mask = 0x1FFF;
        board.v.b.prg_ram_enabled = true;
        board.remap();
        board
    }

    fn remap(&mut self) {
        for window in 0..8 {
            let bank = self.chr_banks[window];
            let low = bank & 0xFF;
            let (first, count) = self.ram_range.unwrap_or((0, 0));
            let (first, count) = (first as i64, count as i64);
            let ram = first <= low && low < first + count;
            self.chr_ram_window[window] = ram;
            self.v.b.chr_offsets[window] = if ram {
                ((low - first).rem_euclid(2) * 0x400) as usize
            } else {
                (bank * 0x400).rem_euclid(self.rom.len() as i64) as usize
            };
        }
    }
}

impl Vrc24Like for Fs306 {
    fn vrc24(&self) -> &Vrc24 {
        &self.v
    }
    fn vrc24_mut(&mut self) -> &mut Vrc24 {
        &mut self.v
    }
    fn update_prg(&mut self) {
        let (r0, r1) = (
            self.v.prg_registers[0] as i64,
            self.v.prg_registers[1] as i64,
        );
        self.map_prg(0, 8, r0);
        self.map_prg(1, 8, r1);
        let c000 = self.prg_c000;
        self.map_prg(2, 8, c000);
        let last = (self.v.b.prg.len() / 0x2000) as i64 - 1;
        self.map_prg(3, 8, last);
    }
    fn map_chr(&mut self, window: usize, _size_kib: usize, bank: i64) {
        self.chr_banks[window] = bank;
        self.remap();
    }
}

impl Cartridge for Fs306 {
    vrc24_cartridge!(cpu_read, m2);
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if address & 0xF000 != 0x9000 {
            vrc24_cpu_write(self, address, value, t);
        } else if address & 0x0C00 == 0x0C00 {
            if address & 4 != 0 {
                self.ciram[(address & 3) as usize] = value & 1;
            } else {
                self.prg_c000 = (value & 0x1F) as i64;
                self.update_prg();
            }
        }
    }
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        Some(self.ciram[((address >> 10) & 3) as usize])
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        if address < 0x2000 {
            let window = (address >> 10) as usize;
            let memory = if self.chr_ram_window[window] {
                &self.chr_ram
            } else {
                &self.rom
            };
            return Some(memory[self.v.b.chr_offsets[window] + (address as usize & 0x3FF)]);
        }
        self.v.b.base_ppu_read(address, t)
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if address >= 0x2000 {
            self.v.b.base_ppu_write(address, value, t);
            return;
        }
        let window = (address >> 10) as usize;
        if self.chr_ram_window[window] {
            self.chr_ram[self.v.b.chr_offsets[window] + (address as usize & 0x3FF)] = value;
        } else if self.chr_banks[window] & 0x80 != 0 {
            let bank = self.chr_banks[window];
            self.ram_range = if bank & 0x10 != 0 {
                None
            } else {
                fs306_window((bank & 0xCA) as u16)
            };
            self.remap();
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
        21 | 22 | 23 | 25 => Box::new(Vrc24::new(image, t)),
        24 | 26 => Box::new(Vrc6::new(image, t)),
        73 => Box::new(Vrc3::new(image, t)),
        75 => Box::new(Vrc1::new(image, t)),
        85 => Box::new(Vrc7::new(image, t)),
        183 => Box::new(ShuiGuan183::new(image, t)),
        222 => Box::new(Ninja222::new(image, t)),
        266 => Box::new(CityFighter266::new(image, t)),
        272 => Box::new(KidDracula272::new(image, t)),
        298 => Box::new(Tf1201::new(image, t)),
        308 => Box::new(Th2131::new(image, t)),
        529 => Box::new(T230::new(image, t)),
        530 => Box::new(Ax5705::new(image, t)),
        544 => Box::new(Fs306::new(image, t)),
        _ => return Err(image),
    })
}
