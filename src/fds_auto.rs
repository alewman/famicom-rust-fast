//! Changing FDS disks without a player (`fds_auto.py`): for scripted runs and
//! a frontend's default.
//!
//! Not part of the console. A player ejects the disk when a game asks for
//! another side and puts that side in; this does the same from what the
//! program does on the bus:
//!
//! - A game asking for another side polls the drive status ($4032) with the
//!   motor stopped, waiting for the disk to come out. After [`EJECT_POLLS`]
//!   such reads with no disk access for [`IDLE_FRAMES`] frames, the disk is
//!   ejected, and after [`SWAP_FRAMES`] frames side 0 goes back in as a
//!   placeholder.
//! - Before reading, the BIOS checks the disk with CheckDiskHeader ($E445):
//!   its pointer at $00-$01 names the 10 bytes of the disk ID it wants
//!   (licensee, game name, type, version, side, disk number, disk type, ...;
//!   $FF matches anything; FDS_BIOS.wiki). At that call the side whose disk
//!   info block matches is put in, if exactly one does.
//!
//! The reference hangs it on `Board.bus_observers`; here the board holds it
//! as [`Board::auto_disk`] and calls it after each CPU-bus cycle, where the
//! reference calls its observers. It reads the request from console RAM,
//! which a cartridge may not do (the connector rule), so it lives beside the
//! board, not in `cartridge/fds.rs`. It is not part of a save state (nor is
//! the reference's).

use crate::board::Board;
use crate::cartridge::fds::Fds;
use crate::cartridge::fds_disk::{blocks, fds_side};

pub const CHECK_DISK_HEADER: u16 = 0xE445;
/// A prompt to change sides waits as long as it takes; between the accesses
/// of one load a game also checks the disk is in (Zelda's "please wait"), for
/// up to a second or two. So be slow to decide.
pub const EJECT_POLLS: u32 = 40;
pub const IDLE_FRAMES: u64 = 180;
pub const SWAP_FRAMES: u64 = 60;

/// Swap disk sides as the program asks for them.
pub struct AutoDisk {
    ids: Vec<[u8; 10]>,
    polls: u32,
    /// Frame of the last disk transfer.
    last_access: u64,
    eject_frame: Option<u64>,
    /// What it did, for the host to show ("FDS: disk 1 side B (asked for)").
    pub messages: Vec<String>,
}

impl AutoDisk {
    /// For the disk in `fds`.
    pub fn new(fds: &Fds) -> AutoDisk {
        AutoDisk {
            ids: fds.sides.iter().map(|raw| disk_id(raw)).collect(),
            polls: 0,
            last_access: 0,
            eject_frame: None,
            messages: Vec::new(),
        }
    }

    /// One CPU-bus cycle, after it happened.
    pub fn observe(&mut self, board: &mut Board, write: bool, address: u16, value: u8) {
        let frame = board.ppu.frame;
        if write {
            if address == 0x4025 && value & 0x01 != 0 && value & 0x02 == 0 {
                self.touched(frame); // the program started the motor: a transfer
            }
            return;
        }
        if address == 0x4032 {
            let Some(fds) = board.ppu.cart.fds() else {
                return;
            };
            if fds.side.is_some() && !fds.scanning() {
                self.polls += 1;
                if self.polls >= EJECT_POLLS && frame - self.last_access >= IDLE_FRAMES {
                    self.insert(board, None, "ejected");
                    self.eject_frame = Some(frame);
                }
            } else if fds.side.is_none() {
                if let Some(ejected) = self.eject_frame {
                    if frame - ejected >= SWAP_FRAMES {
                        self.insert(board, Some(0), "placeholder");
                        self.eject_frame = None;
                    }
                }
            }
        } else if address == CHECK_DISK_HEADER {
            let pointer = board.ram[0] as u16 | (board.ram[1] as u16) << 8;
            let mut wanted = [0u8; 10];
            for (i, byte) in wanted.iter_mut().enumerate() {
                *byte = peek(board, pointer.wrapping_add(i as u16));
            }
            let matches: Vec<usize> = (0..self.ids.len())
                .filter(|&i| {
                    wanted
                        .iter()
                        .zip(&self.ids[i])
                        .all(|(&w, &d)| w == 0xFF || w == d)
                })
                .collect();
            let side = board.ppu.cart.fds().and_then(|fds| fds.side);
            if matches.len() == 1 && Some(matches[0]) != side {
                self.insert(board, Some(matches[0]), "asked for");
            }
            self.touched(frame);
        }
    }

    fn touched(&mut self, frame: u64) {
        self.polls = 0;
        self.last_access = frame;
        self.eject_frame = None;
    }

    fn insert(&mut self, board: &mut Board, side: Option<usize>, why: &str) {
        if let Some(fds) = board.ppu.cart.fds_mut() {
            fds.insert(side);
        }
        self.polls = 0;
        self.messages
            .push(format!("FDS: {} ({why})", Fds::side_name(side)));
    }
}

fn peek(board: &Board, address: u16) -> u8 {
    if address < 0x2000 {
        return board.ram[address as usize & 0x7FF];
    }
    let Some(fds) = board.ppu.cart.fds() else {
        return 0xFF;
    };
    if (0x6000..0xE000).contains(&address) {
        fds.b.prg_ram[(address - 0x6000) as usize]
    } else if address >= 0xE000 {
        fds.bios[(address - 0xE000) as usize]
    } else {
        0xFF // registers: treated as "any"
    }
}

/// A side's disk ID: its disk info block's bytes after *NINTENDO-HVC*.
fn disk_id(raw: &[u8]) -> [u8; 10] {
    let found = blocks(&fds_side(raw));
    let mut id = [0u8; 10];
    if let Some(first) = found.first() {
        // Python's slice: as much of 15..25 as the block holds (the rest
        // compares as zero bytes would not, but a block that short is not a
        // disk info block a BIOS could ask for).
        let part = first.get(15..first.len().min(25)).unwrap_or(&[]);
        id[..part.len()].copy_from_slice(part);
    }
    id
}
