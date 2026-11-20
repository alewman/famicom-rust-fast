// SPDX-License-Identifier: MIT
//! Serial EEPROMs on Bandai's and others' boards (`eeprom.py`; the protocols
//! are described there): the 24C02 (I²C, 256 bytes), Xicor's X24C01 (128
//! bytes, no device byte) and Microchip's Microwire 93C66 / 93C56.
//!
//! The reference's `X24C01` subclasses `Eeprom` (its page and `_received`);
//! here it is a flag. `Microwire93C56` subclasses `Microwire93C66` with other
//! class constants; here they are fields.

const IDLE: u8 = 0;
const DEVICE: u8 = 1;
const ADDRESS: u8 = 2;
const WRITE: u8 = 3;
const READ: u8 = 4;

/// The 24C02, or with `x24c01` the X24C01.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Eeprom {
    pub x24c01: bool,
    pub page: usize,
    pub memory: Vec<u8>,
    pub state: u8,
    pub scl: u8,
    pub sda: u8,
    /// What the part drives on SDA (1 = released).
    pub output: u8,
    /// Bits of the current byte; 8 = the acknowledge clock.
    pub clock: u8,
    pub shift: u8,
    pub address: usize,
    pub ack: bool,
    pub after_ack: u8,
}

impl Eeprom {
    /// The 24C02 (`Eeprom(size)`; 256 bytes by default).
    pub fn new(size: usize) -> Eeprom {
        Eeprom {
            x24c01: false,
            page: 8,
            memory: vec![0; size],
            state: IDLE,
            scl: 1,
            sda: 1,
            output: 1,
            clock: 0,
            shift: 0,
            address: 0,
            ack: false,
            after_ack: IDLE,
        }
    }

    pub fn x24c01() -> Eeprom {
        Eeprom {
            x24c01: true,
            page: 4,
            ..Eeprom::new(128)
        }
    }

    /// The master's SCL and SDA (SDA released reads as 1).
    pub fn lines(&mut self, scl: u8, sda: u8) {
        if scl != 0 && self.scl != 0 {
            if self.sda != 0 && sda == 0 {
                self.start();
            } else if sda != 0 && self.sda == 0 {
                self.state = IDLE;
                self.output = 1;
            }
        } else if scl != 0 {
            self.rise(sda);
        } else if self.scl != 0 {
            self.fall();
        }
        self.scl = scl;
        self.sda = sda;
    }

    fn start(&mut self) {
        self.state = DEVICE;
        self.clock = 0;
        self.shift = 0;
        self.output = 1;
    }

    fn rise(&mut self, sda: u8) {
        if self.state == IDLE {
            return;
        }
        if self.clock < 8 {
            if self.state != READ {
                self.shift = (self.shift << 1) | sda;
            }
            self.clock += 1;
            if self.clock == 8 && self.state != READ {
                let byte = self.shift;
                self.received(byte);
            }
        } else if self.state == READ {
            if sda != 0 {
                self.state = IDLE;
            } else {
                self.address = (self.address + 1) % self.memory.len();
                self.clock = 0;
            }
        } else {
            self.clock = 9;
        }
    }

    fn fall(&mut self) {
        if self.state == IDLE {
            return;
        }
        if self.state == READ {
            if self.clock < 8 {
                self.output = (self.memory[self.address] >> (7 - self.clock)) & 1;
            } else {
                self.output = 1;
            }
        } else if self.clock == 8 {
            self.output = if self.ack { 0 } else { 1 };
        } else if self.clock == 9 {
            self.output = 1;
            self.clock = 0;
            self.shift = 0;
            self.state = if self.ack { self.after_ack } else { IDLE };
            if self.state == READ {
                self.output = self.memory[self.address] >> 7;
            }
        }
    }

    fn received(&mut self, byte: u8) {
        if self.x24c01 {
            if self.state == DEVICE {
                self.address = (byte >> 1) as usize;
                self.ack = true;
                self.after_ack = if byte & 1 != 0 { READ } else { WRITE };
            } else {
                self.store(byte);
            }
            return;
        }
        match self.state {
            DEVICE => {
                self.ack = byte & 0xFE == 0xA0;
                self.after_ack = if byte & 1 != 0 { READ } else { ADDRESS };
            }
            ADDRESS => {
                self.ack = true;
                self.address = byte as usize % self.memory.len();
                self.after_ack = WRITE;
            }
            _ => self.store(byte),
        }
    }

    fn store(&mut self, byte: u8) {
        self.memory[self.address] = byte;
        let page = self.page;
        self.address = (self.address & !(page - 1)) | ((self.address + 1) & (page - 1));
        self.ack = true;
        self.after_ack = WRITE;
    }
}

/// Microchip's 93C66 with ORG low (512 bytes), or the 93C56 with ORG high
/// (128 16-bit words, big-endian here).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Microwire {
    pub size: usize,
    pub address_bits: u32,
    pub word_bits: u32,
    pub memory: Vec<u8>,
    pub cs: u8,
    pub clk: u8,
    pub output: u8,
    pub bits: u32,
    pub shift: u32,
    pub write_enabled: bool,
    pub reading: bool,
    pub read_bits: u32,
    pub address: usize,
    pub words: usize,
}

impl Microwire {
    fn build(size: usize, address_bits: u32, word_bits: u32) -> Microwire {
        Microwire {
            size,
            address_bits,
            word_bits,
            memory: vec![0xFF; size],
            cs: 0,
            clk: 0,
            output: 1,
            bits: 0,
            shift: 0,
            write_enabled: false,
            reading: false,
            read_bits: 0,
            address: 0,
            words: size * 8 / word_bits as usize,
        }
    }

    pub fn c93c66() -> Microwire {
        Microwire::build(512, 9, 8)
    }

    pub fn c93c56() -> Microwire {
        Microwire::build(256, 8, 16)
    }

    pub fn lines(&mut self, cs: u8, clk: u8, di: u8) {
        if cs == 0 {
            self.bits = 0;
            self.reading = false;
            self.output = 1;
        } else if clk != 0 && self.clk == 0 {
            self.rise(di);
        }
        self.cs = cs;
        self.clk = clk;
    }

    fn word(&self, address: usize) -> u32 {
        if self.word_bits == 8 {
            return self.memory[address] as u32;
        }
        ((self.memory[address * 2] as u32) << 8) | self.memory[address * 2 + 1] as u32
    }

    fn store(&mut self, address: usize, word: u32) {
        if self.word_bits == 8 {
            self.memory[address] = word as u8;
        } else {
            self.memory[address * 2] = (word >> 8) as u8;
            self.memory[address * 2 + 1] = (word & 0xFF) as u8;
        }
    }

    fn rise(&mut self, di: u8) {
        if self.reading {
            if self.read_bits == self.word_bits {
                self.address = (self.address + 1) % self.words;
                self.read_bits = 0;
            }
            let word = self.word(self.address);
            self.output = ((word >> (self.word_bits - 1 - self.read_bits)) & 1) as u8;
            self.read_bits += 1;
            return;
        }
        if self.bits == 0 {
            if di != 0 {
                self.bits = 1;
                self.shift = 0;
            }
            return;
        }
        self.shift = (self.shift << 1) | di as u32;
        self.bits += 1;
        if self.bits == 3 + self.address_bits {
            self.instruction();
        } else if self.bits == 3 + self.address_bits + self.word_bits {
            self.program();
        }
    }

    fn instruction(&mut self) {
        let opcode = self.shift >> self.address_bits;
        let address = self.shift & ((1 << self.address_bits) - 1);
        let special = address >> (self.address_bits - 2);
        let address = address as usize % self.words;
        self.address = address;
        if opcode == 0b10 {
            self.reading = true;
            self.read_bits = 0;
            self.output = 0;
        } else if opcode == 0b11 {
            if self.write_enabled {
                self.store(address, (1 << self.word_bits) - 1);
            }
            self.done();
        } else if opcode == 0b00 {
            if special == 0b11 || special == 0b00 {
                self.write_enabled = special == 0b11;
                self.done();
            } else if special == 0b10 {
                if self.write_enabled {
                    self.memory.fill(0xFF);
                }
                self.done();
            }
        }
    }

    fn program(&mut self) {
        let word_bits = self.word_bits;
        let opcode = (self.shift >> (self.address_bits + word_bits)) & 3;
        let data = self.shift & ((1 << word_bits) - 1);
        if self.write_enabled {
            if opcode == 0b01 {
                let address = (self.shift >> word_bits) & ((1 << self.address_bits) - 1);
                self.store(address as usize % self.words, data);
            } else {
                for address in 0..self.words {
                    self.store(address, data);
                }
            }
        }
        self.done();
    }

    fn done(&mut self) {
        self.bits = 0;
        self.output = 1;
    }
}
