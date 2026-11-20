// SPDX-License-Identifier: MIT
//! The PIC16C54, the protection microcontroller of mapper 355, and the board
//! around it (`pic16c54.py`; from Microchip's PIC16C5X data sheet, DS30453E:
//! Table 10-2 the instructions, Table 5-3 the reset state). The watchdog and
//! SLEEP are not modelled (SLEEP waits for ever). The program is the NES 2.0
//! image's Misc ROM.
//!
//! The reference's arithmetic is on Python ints, which go negative (`value -
//! w`, `value - 1`) before a mask: here `i32`, whose `&` agrees with Python's
//! on negative values.
//!
//! The mapper numbers [`build`] answers, and the reference's class for each:
//!
//! - 355: `Block355`

use super::banked::Banked;
use super::connector::Cartridge;
use super::ines::RomImage;
use crate::banked_field;

/// The board for `mapper` if this module builds it.
pub fn build(
    mapper: u16,
    image: RomImage,
    cpu_cycle_ticks: i64,
) -> Result<Box<dyn Cartridge>, RomImage> {
    match mapper {
        355 => Ok(Box::new(Block355::new(image, cpu_cycle_ticks))),
        _ => Err(image),
    }
}

pub const INDF: usize = 0;
pub const TMR0: usize = 1;
pub const PCL: usize = 2;
pub const STATUS: usize = 3;
pub const FSR: usize = 4;
pub const PORTA: usize = 5;
pub const PORTB: usize = 6;
pub const C: i32 = 1;
pub const DC: i32 = 2;
pub const Z: i32 = 4;

/// The core. `port_a`/`port_b` are the pins' inputs, set by the caller
/// between cycles.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Pic16c54 {
    pub program: Vec<u16>,
    pub memory: [i32; 32],
    pub w: i32,
    pub pc: i32,
    pub stack: [i32; 2],
    pub tris_a: i32,
    pub tris_b: i32,
    pub option: i32,
    pub prescaler: i32,
    /// A write to TMR0 stops it for two cycles.
    pub timer_hold: i32,
    pub port_a: i32,
    pub port_b: i32,
    /// Cycles left of a two-cycle instruction.
    pub waiting: i32,
    pub skip: bool,
}

impl Pic16c54 {
    pub fn new(program: &[u8]) -> Pic16c54 {
        let program = (0..program.len().min(1024))
            .step_by(2)
            .map(|i| (program[i] as u16 | (program[i + 1] as u16) << 8) & 0xFFF)
            .collect();
        let mut memory = [0; 32];
        memory[STATUS] = 0x18;
        memory[FSR] = 0xE0;
        Pic16c54 {
            program,
            memory,
            w: 0,
            pc: 0x1FF, // the reset vector, the last word
            stack: [0, 0],
            tris_a: 0x0F,
            tris_b: 0xFF,
            option: 0x3F,
            prescaler: 0,
            timer_hold: 0,
            port_a: 0,
            port_b: 0,
            waiting: 0,
            skip: false,
        }
    }

    // -- registers ---------------------------------------------------------

    fn address(&self, f: usize) -> usize {
        if f == INDF {
            (self.memory[FSR] & 0x1F) as usize
        } else {
            f
        }
    }

    pub fn read(&self, f: usize) -> i32 {
        let f = self.address(f);
        if f == INDF {
            return 0;
        }
        if f == PORTA {
            return ((self.port_a & self.tris_a) | (self.memory[PORTA] & !self.tris_a)) & 0x0F;
        }
        if f == PORTB {
            return (self.port_b & self.tris_b) | (self.memory[PORTB] & !self.tris_b & 0xFF);
        }
        if f == PCL {
            return self.pc & 0xFF;
        }
        self.memory[f]
    }

    pub fn write(&mut self, f: usize, value: i32) {
        let f = self.address(f);
        if f == PCL {
            self.pc = value; // bit 8 clears: DS30453, "Program Counter"
            self.waiting = 1;
        } else if f == FSR {
            self.memory[FSR] = value | 0xE0;
        } else if f == STATUS {
            self.memory[STATUS] = (value & 0xE7) | (self.memory[STATUS] & 0x18);
        } else if f == TMR0 {
            self.memory[TMR0] = value;
            self.prescaler = 0;
            self.timer_hold = 2;
        } else if f != INDF {
            self.memory[f] = value;
        }
    }

    /// Port A's pins: 1 where driven high or not driven (the /IRQ line's pull-up).
    pub fn output_a(&self) -> i32 {
        (self.memory[PORTA] | self.tris_a) & 0x0F
    }

    fn flags(&mut self, mask: i32, carry: bool, half: bool, result: i32) {
        let mut status = self.memory[STATUS] & !mask;
        if mask & C != 0 && carry {
            status |= C;
        }
        if mask & DC != 0 && half {
            status |= DC;
        }
        if mask & Z != 0 && result & 0xFF == 0 {
            status |= Z;
        }
        self.memory[STATUS] = status;
    }

    // -- the instruction cycle ---------------------------------------------

    fn tick_timer(&mut self) {
        if self.option & 0x20 != 0 {
            // T0CS: the external pin, never driven here
            return;
        }
        if self.timer_hold != 0 {
            self.timer_hold -= 1;
            return;
        }
        if self.option & 0x08 == 0 {
            // the prescaler on TMR0
            self.prescaler += 1;
            if self.prescaler < 2 << (self.option & 7) {
                return;
            }
            self.prescaler = 0;
        }
        self.memory[TMR0] = (self.memory[TMR0] + 1) & 0xFF;
    }

    /// One instruction cycle (four clocks).
    pub fn cycle(&mut self) {
        self.tick_timer();
        if self.waiting != 0 {
            self.waiting -= 1;
            return;
        }
        let word = self.program[(self.pc & 0x1FF) as usize] as i32;
        self.pc = (self.pc + 1) & 0x1FF;
        if self.skip {
            self.skip = false;
            return;
        }
        self.execute(word);
    }

    fn store(&mut self, d: i32, f: usize, value: i32) {
        if d != 0 {
            self.write(f, value & 0xFF);
        } else {
            self.w = value & 0xFF;
        }
    }

    fn execute(&mut self, word: i32) {
        let top = word >> 8;
        let (f, d, k) = ((word & 0x1F) as usize, word & 0x20, word & 0xFF);
        if word < 0x020 {
            if (0x005..=0x007).contains(&word) {
                // TRIS
                if word == 5 {
                    self.tris_a = self.w & 0x0F;
                } else if word == 6 {
                    self.tris_b = self.w;
                }
            } else if word == 0x002 {
                self.option = self.w & 0x3F;
            } else if word == 0x003 {
                // SLEEP
                self.pc = (self.pc - 1) & 0x1FF;
            }
        } else if word < 0x400 {
            self.byte_operation(word >> 6, f, d); // bits 9-6 name it, D5 the destination
        } else if top < 0x08 {
            // BCF, BSF, BTFSC, BTFSS
            let bit = 1 << ((word >> 5) & 7);
            let kind = (word >> 8) & 3;
            let value = self.read(f);
            if kind == 0 {
                self.write(f, value & !bit & 0xFF);
            } else if kind == 1 {
                self.write(f, value | bit);
            } else {
                self.skip = (value & bit != 0) == (kind == 3);
            }
        } else if top == 0x08 {
            // RETLW
            self.w = k;
            self.pc = self.stack[0];
            self.stack[0] = self.stack[1];
            self.waiting = 1;
        } else if top == 0x09 {
            // CALL
            self.stack[1] = self.stack[0];
            self.stack[0] = self.pc;
            self.pc = k;
            self.waiting = 1;
        } else if top < 0x0C {
            // GOTO
            self.pc = word & 0x1FF;
            self.waiting = 1;
        } else if top == 0x0C {
            // MOVLW
            self.w = k;
        } else {
            let value = match top {
                0x0D => self.w | k,
                0x0E => self.w & k,
                0x0F => self.w ^ k,
                _ => unreachable!("a 12-bit word"),
            };
            self.w = value;
            self.flags(Z, false, false, value);
        }
    }

    fn byte_operation(&mut self, op: i32, f: usize, d: i32) {
        let w = self.w;
        let kind = op;
        if kind == 0 {
            // MOVWF (D5 set; clear is the control group, decoded by the caller)
            self.write(f, w);
            return;
        }
        let value = self.read(f);
        let carry_in = self.memory[STATUS] & C;
        match kind {
            1 => {
                // CLRW, CLRF
                self.store(d, f, 0);
                self.flags(Z, false, false, 0);
            }
            2 => {
                // SUBWF
                let result = value - w;
                self.flags(C | DC | Z, result >= 0, (value & 0xF) >= (w & 0xF), result);
                self.store(d, f, result);
            }
            3 => {
                // DECF
                self.store(d, f, value - 1);
                self.flags(Z, false, false, value - 1);
            }
            4 => {
                // IORWF
                self.store(d, f, value | w);
                self.flags(Z, false, false, value | w);
            }
            5 => {
                // ANDWF
                self.store(d, f, value & w);
                self.flags(Z, false, false, value & w);
            }
            6 => {
                // XORWF
                self.store(d, f, value ^ w);
                self.flags(Z, false, false, value ^ w);
            }
            7 => {
                // ADDWF
                let result = value + w;
                self.flags(
                    C | DC | Z,
                    result > 0xFF,
                    (value & 0xF) + (w & 0xF) > 0xF,
                    result,
                );
                self.store(d, f, result);
            }
            8 => {
                // MOVF
                self.store(d, f, value);
                self.flags(Z, false, false, value);
            }
            9 => {
                // COMF
                self.store(d, f, value ^ 0xFF);
                self.flags(Z, false, false, value ^ 0xFF);
            }
            10 => {
                // INCF
                self.store(d, f, value + 1);
                self.flags(Z, false, false, value + 1);
            }
            11 => {
                // DECFSZ
                self.store(d, f, value - 1);
                self.skip = (value - 1) & 0xFF == 0;
            }
            12 => {
                // RRF
                self.flags(C, value & 1 != 0, false, 0);
                self.store(d, f, (value >> 1) | (carry_in << 7));
            }
            13 => {
                // RLF
                self.flags(C, value & 0x80 != 0, false, 0);
                self.store(d, f, (value << 1) | carry_in);
            }
            14 => {
                // SWAPF
                self.store(d, f, ((value & 0x0F) << 4) | (value >> 4));
            }
            _ => {
                // INCFSZ
                self.store(d, f, value + 1);
                self.skip = (value + 1) & 0xFF == 0;
            }
        }
    }
}

/// Mapper 355 (3D-BLOCK): NROM-256 with CHR-RAM and a PIC16C54 watching the
/// CPU's address bus at a quarter of M2 (NES_2.0_Mapper_355.wiki); RA0 drives
/// /IRQ.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Block355 {
    pub b: Banked,
    pub pic: Pic16c54,
    pub address: u16,
    pub divider: u8,
    pub irq: bool,
}

impl Block355 {
    pub fn new(image: RomImage, cpu_cycle_ticks: i64) -> Block355 {
        let pic = Pic16c54::new(&image.misc);
        let mut b = Banked::new(image, cpu_cycle_ticks);
        b.map_prg(0, 32, 0);
        b.map_chr(0, 8, 0);
        Block355 {
            b,
            pic,
            address: 0,
            divider: 0,
            irq: false,
        }
    }

    fn inputs(&mut self) {
        let a = self.address as i32;
        let pic = &mut self.pic;
        pic.port_a = (((a >> 6) & 1) << 1) | (((a >> 5) & 1) << 2) | (((a >> 4) & 1) << 3) | 1;
        pic.port_b = ((a >> 12) & 1)
            | (((a >> 7) & 1) << 1)
            | (((a >> 10) & 1) << 2)
            | (((a >> 11) & 1) << 3)
            | (((a >> 9) & 1) << 4)
            | (((a >> 8) & 1) << 5)
            | (((a >> 13) & 1) << 6)
            | (((a >> 14) & 1) << 7);
    }
}

impl Cartridge for Block355 {
    banked_field!(b);
    fn counts_m2(&self) -> bool {
        true
    }
    fn snoops_cpu_reads(&self) -> bool {
        true
    }
    fn irq(&self) -> bool {
        self.irq
    }

    fn cpu_read(&mut self, address: u16, t: i64) -> Option<u16> {
        self.address = address;
        self.b.base_cpu_read(address, t)
    }

    fn cpu_write(&mut self, address: u16, _value: u8, _t: i64) {
        self.address = address;
    }

    fn m2(&mut self, _t: i64) {
        self.divider = (self.divider + 1) & 3;
        if self.divider == 0 {
            self.inputs();
            self.pic.cycle();
            self.irq = self.pic.output_a() & 1 == 0;
        }
    }
}
