//! A Rust Famicom/NES board transcribed from
//! [famicom-python](https://github.com/alewman/famicom-python) at the tag
//! `famicom-rust-pin-1` (`fe89099`) and proven equivalent to it by a
//! lockstep gate (scripts/lockstep.py).
//!
//! Each module is the transcription of one `famicom_python/*.py` module, with
//! the same names and the hardware comments' sources carried over (the
//! reference's comments hold the full reasoning). Where Rust forced a
//! difference, the module header says so. The CPU is 6502-rust (crate
//! `sixfiveohtwo`), its RP2A03.

// Each mapper module's `build` hands the image back (`Err(image)`) when the
// number is not its own, so the next module can try it: a large Err by design.
#![allow(clippy::result_large_err)]

pub mod apu;
pub mod board;
pub mod cartridge;
pub mod fds_auto;
pub mod host;
pub mod joypad;
pub mod machine;
pub mod peripherals;
pub mod ppu;
pub mod state;
pub mod system;
pub mod testrom;
pub mod video;

pub use board::Board;
pub use machine::Machine;
pub use system::System;

pub(crate) fn peripherals_nes_only(device: u8) -> bool {
    peripherals::nes_only(device)
}
