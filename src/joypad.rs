// SPDX-License-Identifier: MIT
//! The standard controller: a 4021 shift register behind the port (`joypad.py`).

pub const A: u8 = 0x01;
pub const B: u8 = 0x02;
pub const SELECT: u8 = 0x04;
pub const START: u8 = 0x08;
pub const UP: u8 = 0x10;
pub const DOWN: u8 = 0x20;
pub const LEFT: u8 = 0x40;
pub const RIGHT: u8 = 0x80;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StandardController {
    /// Pressed buttons, the bits above.
    pub buttons: u8,
    pub shift: u8,
    pub strobe: bool,
}

impl StandardController {
    pub fn set_strobe(&mut self, high: bool) {
        if high || self.strobe {
            self.shift = self.buttons;
        }
        self.strobe = high;
    }

    /// D0 as the CPU sees it: 1 = pressed.
    pub fn read(&self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }
        self.shift & 1
    }

    /// The end of an /OE pulse.
    pub fn clock(&mut self) {
        if self.strobe {
            self.shift = self.buttons;
        } else {
            self.shift = (self.shift >> 1) | 0x80;
        }
    }
}
