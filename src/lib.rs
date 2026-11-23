// SPDX-License-Identifier: MIT
//! A Rust Famicom/NES board transcribed from
//! [famicom-python](https://github.com/alewman/famicom-python) at `fe89099`
//! (the old tag `famicom-rust-pin-1`) and proven equivalent, on the gate's
//! cases, to famicom-python `e3e1474` by a lockstep gate (scripts/lockstep.py).
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

/// The reference's checkout, where the run-time data the board reads lives
/// (pally's palettes, nes20db): `FAMICOM_REFERENCE`, else this repository's
/// `external/famicom-python`.
///
/// The repository is found at run time, never baked into the build (a wasm
/// build would otherwise carry its build machine's path): `CARGO_MANIFEST_DIR`
/// when cargo runs the program, else the nearest directory above the running
/// executable (target/release/..., target/release/deps/...) that holds
/// `external/famicom-python`, else that path relative to the working directory.
pub fn reference_checkout() -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(path) = std::env::var("FAMICOM_REFERENCE") {
        return PathBuf::from(path);
    }
    let inside = PathBuf::from("external/famicom-python");
    if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(root).join(inside);
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1) {
            if dir.join(&inside).is_dir() {
                return dir.join(inside);
            }
        }
    }
    inside
}
