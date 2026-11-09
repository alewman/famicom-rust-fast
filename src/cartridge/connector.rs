//! The cartridge connector, as signals: the only thing a mapper knows of the
//! console (`cartridge/connector.py`; the protocol is described there).
//!
//! Differences forced by Rust:
//!
//! - The reference's class attributes (`snoops_cpu_reads`, `counts_m2`, ...)
//!   are methods; `irq` is a method the board samples; "not driven" is
//!   `None`. A CPU read that drives only some data lines returns `driven |
//!   undriven_mask << 8`, as in the reference.
//! - The reference's `audio_out` (the APU, whose `expansion` the chip sets)
//!   becomes [`Cartridge::expansion`], the level the chip last set, which the
//!   board copies to the APU each cycle when the console routes cartridge
//!   audio; [`Cartridge::connect_audio`] tells the chip it is routed.
//! - **Inheritance.** Most of the reference's mappers subclass `Banked`. Here
//!   a mapper holds a [`Banked`] and returns it from [`Cartridge::banked`] and
//!   [`Cartridge::banked_mut`]; every connector method's default then does
//!   what `Banked` does (its `base_*` methods), so a mapper overrides only
//!   what its Python class overrides and calls `self.b.base_*` where the
//!   Python calls `super()`. The hooks a Python base class calls on `self`
//!   that a subclass may override (`battery_ram` and `_restore` from
//!   `restore_battery`) are trait methods too, so the override is seen.
//!   Helpers the Python overrides further down a chain (`map_prg`,
//!   `set_mirroring`) are each mapper's own business, transcribed per class.

use super::banked::Banked;

/// A full-volume APU square, alone, in the mixer's units.
pub const APU_SQUARE: f64 = 95.88 / (8128.0 / 15.0 + 100.0);

/// Mixer units per step of a chip whose full-volume square is `full_square`
/// steps and sounds `db` dB against a full-volume APU square.
pub fn expansion_gain(db: f64, full_square: i32) -> f64 {
    APU_SQUARE * 10f64.powf(db / 20.0) / full_square as f64
}

#[allow(unused_variables)]
pub trait Cartridge: crate::state::Snapshot {
    /// The `Banked` this mapper is built on, if any.
    fn banked(&self) -> Option<&Banked> {
        None
    }
    fn banked_mut(&mut self) -> Option<&mut Banked> {
        None
    }
    /// The Disk System, for a host that changes its disks (the reference's
    /// `isinstance(cartridge, FDS)`).
    fn fds(&self) -> Option<&super::fds::Fds> {
        None
    }
    fn fds_mut(&mut self) -> Option<&mut super::fds::Fds> {
        None
    }

    // -- what the cartridge asks to see ------------------------------------

    /// Also receive reads of $0000-$401F.
    fn snoops_cpu_reads(&self) -> bool {
        false
    }
    /// Receive `m2(t)` every CPU cycle.
    fn counts_m2(&self) -> bool {
        false
    }
    /// Receive `read_data()` after reads of $4020-$FFFF.
    fn latches_reads(&self) -> bool {
        false
    }
    /// Receive `ppu_address(address, t)` on every ALE.
    fn watches_ppu_address(&self) -> bool {
        false
    }
    /// Has a sound chip: the board may route its sound.
    fn expansion_audio(&self) -> bool {
        false
    }

    // -- the CPU side ------------------------------------------------------

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.banked_mut().and_then(|b| b.base_cpu_read(address, t))
    }
    fn cpu_write(&mut self, address: u16, value: u8, t: i64) {
        if let Some(b) = self.banked_mut() {
            b.base_cpu_write(address, value, t);
        }
    }
    fn read_data(&mut self, address: u16, value: u8, t: i64) {}
    fn m2_gap(&mut self, t: i64) {}
    fn m2(&mut self, t: i64) {}

    // -- the PPU side ------------------------------------------------------

    fn ppu_address(&mut self, address: u16, t: i64) {}
    fn ciram_a10(&mut self, address: u16) -> Option<u8> {
        self.banked().and_then(|b| b.base_ciram_a10(address))
    }
    fn ppu_read(&mut self, address: u16, t: i64) -> Option<u8> {
        self.banked().and_then(|b| b.base_ppu_read(address, t))
    }
    fn ppu_write(&mut self, address: u16, value: u8, t: i64) {
        if let Some(b) = self.banked_mut() {
            b.base_ppu_write(address, value, t);
        }
    }

    // -- outputs the board samples -----------------------------------------

    /// /IRQ, wired-OR on the board: true asserts it.
    fn irq(&self) -> bool {
        false
    }
    /// The sound chip's level, in the APU mixer's units (0 when silent).
    fn expansion(&self) -> f64 {
        0.0
    }
    /// The board connected the chip's output (the reference's `audio_out = apu`).
    fn connect_audio(&mut self) {}

    /// The battery-backed memory to persist, if any.
    fn battery_ram(&self) -> Option<Vec<u8>> {
        self.banked().and_then(|b| b.base_battery_ram())
    }
    /// The inverse of `battery_ram`, for data of its length (`Banked._restore`).
    fn restore(&mut self, data: &[u8]) {
        if let Some(b) = self.banked_mut() {
            b.prg_ram.copy_from_slice(data);
        }
    }
    /// Put back what `battery_ram` returned on an earlier run; false (and
    /// nothing changed) if this cartridge keeps no such memory.
    fn restore_battery(&mut self, data: &[u8]) -> bool {
        match self.battery_ram() {
            Some(saved) if saved.len() == data.len() && self.banked().is_some() => {
                self.restore(data);
                true
            }
            _ => false,
        }
    }
}

/// A cartridge with nothing in it (the connector's defaults).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Empty;

impl Cartridge for Empty {}

/// Forward the two `banked` accessors to a field (`impl Cartridge` bodies).
#[macro_export]
macro_rules! banked_field {
    ($($field:tt)+) => {
        fn banked(&self) -> Option<&$crate::cartridge::Banked> {
            Some(&self.$($field)+)
        }
        fn banked_mut(&mut self) -> Option<&mut $crate::cartridge::Banked> {
            Some(&mut self.$($field)+)
        }
    };
}
