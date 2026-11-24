// SPDX-License-Identifier: MIT
//! The Vs. System on synthetic images: the reference's tests/test_vs_system.py
//! and tests/test_vs_dual.py (famicom-python `b3dbf1b`), transcribed. The
//! game-level cases are the lockstep gate's (scripts/cases-vs.tsv).

use famicom_rust::apu::Apu;
use famicom_rust::cartridge::{self, Cartridge};
use famicom_rust::dual::{split, VsDualSystem};
use famicom_rust::joypad::{A, B, LEFT, RIGHT, SELECT, START, UP};
use famicom_rust::machine::{ApuTables, Machine, NTSC, VS_PPUS};
use famicom_rust::peripherals::{Plugged, PortDevice};
use famicom_rust::state::{load_state, save_state};
use famicom_rust::video::{images, rgb_palette, RGB_DAC};
use famicom_rust::vs::{panel, Side, VsStick, VsZapper, VS_ZAPPER};
use famicom_rust::System;

#[derive(Clone, Copy)]
struct Image {
    prg_size: usize,
    chr_size: usize,
    mapper: u16,
    ppu: u8,
    hardware: u8,
    expansion: u8,
}

impl Default for Image {
    fn default() -> Image {
        Image {
            prg_size: 0x8000,
            chr_size: 0x4000,
            mapper: 99,
            ppu: 0,
            hardware: 0,
            expansion: 4,
        }
    }
}

/// An NES 2.0 Vs. System image: `program` at the reset vector, each 8 KiB of
/// PRG ending in its bank number, each 8 KiB of CHR filled with its own.
fn vs_image(program: &[u8], i: Image) -> Vec<u8> {
    let mut prg = vec![0u8; i.prg_size];
    for bank in 0..i.prg_size / 0x2000 {
        prg[bank * 0x2000 + 0x1FF0] = bank as u8;
    }
    let top = i.prg_size.min(0x8000);
    let start = 0x10000 - top;
    prg[top - 0x1000..top - 0x1000 + program.len()].copy_from_slice(program);
    let reset = start + top - 0x1000;
    prg[top - 6..top].copy_from_slice(&[0, 0xF8, reset as u8, (reset >> 8) as u8, 0, 0xF8]);
    let chr: Vec<u8> = (0..i.chr_size / 0x2000)
        .flat_map(|bank| vec![0xC0 + bank as u8; 0x2000])
        .collect();
    let (size_low, size_high) = if i.prg_size % 0x4000 != 0 {
        match i.prg_size {
            0xA000 => ((13 << 2) | 2, 0x0F),
            0x6000 => ((13 << 2) | 1, 0x0F),
            _ => (
                ((usize::BITS - 1 - i.prg_size.leading_zeros()) << 2) as u8,
                0x0F,
            ),
        }
    } else {
        ((i.prg_size / 0x4000) as u8, 0)
    };
    let m = i.mapper;
    let mut out = vec![
        0x4E,
        0x45,
        0x53,
        0x1A,
        size_low,
        (i.chr_size / 0x2000) as u8,
        ((m as u8 & 0x0F) << 4) | 0x08,
        (m as u8 & 0xF0) | 0x09,
        (m >> 8) as u8,
        size_high,
        0x05,
        0,
        0,
        (i.hardware << 4) | i.ppu,
        0,
        i.expansion,
    ];
    out.extend(prg);
    out.extend(chr);
    out
}

const JMP_8000: &[u8] = &[0x4c, 0x00, 0x80];

fn vs_system_with(program: &[u8], i: Image) -> System {
    let image = vs_image(program, i);
    let loaded = cartridge::load(&image, 12, None, false).unwrap();
    let machine = Machine::vs_machine(loaded.image.board.vs_ppu);
    let mut system = System::new(loaded.cartridge, machine);
    system.image = Some(loaded.image);
    system.step(); // the reset sequence
    system
}

fn vs_system(i: Image) -> System {
    vs_system_with(JMP_8000, i)
}

fn read(system: &mut System, address: u16) -> u8 {
    let cpu = &mut system.cpu;
    cpu.bus.read_byte(address, &mut cpu.frame)
}

fn write(system: &mut System, address: u16, value: u8) {
    let cpu = &mut system.cpu;
    cpu.bus.write_byte(address, value, &mut cpu.frame)
}

/// Two more cycles: a $4016 write's OUT lines move on the next put.
fn settle(system: &mut System) {
    read(system, 0x0000);
    read(system, 0x0000);
}

fn cart(system: &mut System) -> &mut dyn Cartridge {
    system.cpu.bus.ppu.cart.as_mut()
}

// -- the machine rows --------------------------------------------------------

#[test]
fn test_the_header_names_the_console_and_its_ppu() {
    let image = vs_image(
        JMP_8000,
        Image {
            ppu: 5,
            hardware: 4,
            ..Image::default()
        },
    );
    let board = cartridge::load(&image, 12, None, false)
        .unwrap()
        .image
        .board;
    assert_eq!((board.console, board.vs_ppu, board.vs_hardware), (1, 5, 4));
    let machine = |ppu: u8| {
        System::machine_for(&vs_image(
            JMP_8000,
            Image {
                ppu,
                ..Image::default()
            },
        ))
        .unwrap()
    };
    assert_eq!(machine(5), Machine::vs_machines(5).unwrap());
    assert_eq!(machine(0), Machine::by_key("vs").unwrap());
    assert_eq!(machine(7), Machine::by_key("vs").unwrap()); // a reserved number
}

#[test]
fn test_every_vs_ppu_has_a_row_and_a_colour_table() {
    let numbers: Vec<u8> = VS_PPUS.iter().map(|row| row.0).collect();
    assert_eq!(numbers, [0, 2, 3, 4, 5, 8, 9, 10, 11]);
    for (number, chip, timing) in VS_PPUS {
        let machine = Machine::vs_machines(number).unwrap();
        assert!(machine.ppu_variant == chip && machine.timing == timing);
        assert!(machine.cpu_variant == "RP2A03" && machine.vs_side == Some("secondary"));
        assert!(timing.rgb && !timing.odd_frame_skip && !timing.oam_corruption);
        assert!(RGB_DAC.iter().any(|(name, _)| *name == timing.palette));
        let key = if number == 0 {
            "vs".to_string()
        } else {
            format!("vs-{}", chip.to_lowercase())
        };
        assert_eq!(Machine::by_key(&key).unwrap(), machine);
        assert_eq!(timing.swapped_control, chip.starts_with("RC2C05"));
    }
}

#[test]
fn test_the_home_consoles_are_untouched() {
    const { assert!(NTSC.odd_frame_skip && NTSC.oam_corruption && !NTSC.rgb) };
    const { assert!(!NTSC.swapped_control && NTSC.identifier_mask == 0) };
    let nes = Machine::nes_001();
    assert!(nes.vs_side.is_none() && nes.apu_tables == ApuTables::Ntsc);
}

// -- the RGB PPUs ------------------------------------------------------------

#[test]
fn test_the_2c03_palette_is_the_pinned_file() {
    let path = images().join("2C03_wiki.pal");
    let Ok(data) = std::fs::read(&path) else {
        eprintln!("2C03_wiki.pal not fetched: skipped");
        return;
    };
    let file: Vec<[u8; 3]> = data.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    assert_eq!(file, rgb_palette("RP2C03"));
}

#[test]
fn test_the_four_2c04s_permute_one_master_palette() {
    let sets: Vec<std::collections::BTreeSet<u16>> = (1..=4)
        .map(|n| {
            let name = format!("RP2C04-000{n}");
            let table = RGB_DAC.iter().find(|(chip, _)| *chip == name).unwrap().1;
            table.iter().copied().collect()
        })
        .collect();
    assert!(sets.iter().all(|set| *set == sets[0]));
    assert_eq!(sets[0].len(), 60); // the "60-color master palette"
}

#[test]
fn test_emphasis_drives_its_channel_to_full_brightness() {
    let palette = rgb_palette("RP2C03");
    assert!(palette[0x16] == [255, 0, 0] && palette[0x00] == [109, 109, 109]);
    assert_eq!(palette[0x40], [255, 109, 109]); // $2001 bit 5: red
    assert_eq!(palette[0x80], [109, 255, 109]); // bit 6: green
    assert_eq!(palette[0x100 | 0x0F], [0, 0, 255]); // bit 7: blue, even on black
    assert!((0..64).all(|index| palette[0x1C0 | index] == [255, 255, 255]));
}

#[test]
fn test_the_rc2c05_swaps_control_and_mask_and_returns_its_identifier() {
    let mut system = vs_system(Image {
        ppu: 10,
        ..Image::default()
    }); // RC2C05-03: $2002 AND $1F = $1C
    system.cpu.bus.ppu.resetting = false;
    write(&mut system, 0x2000, 0x1E);
    assert!(system.cpu.bus.ppu.mask == 0x1E && system.cpu.bus.ppu.ctrl == 0);
    write(&mut system, 0x2001, 0x04);
    assert_eq!(system.cpu.bus.ppu.ctrl, 0x04);
    write(&mut system, 0x2005, 0xFF); // the I/O latch holds $FF
    assert_eq!(read(&mut system, 0x2002) & 0x1F, 0x1C);
    assert_eq!(read(&mut system, 0x2000) & 0x1F, 0x1F); // the latch itself is untouched
    let mut eleven = vs_system(Image {
        ppu: 11,
        ..Image::default()
    });
    assert_eq!(read(&mut eleven, 0x2002) & 0x1F, 0x1B);
    let mut nine = vs_system(Image {
        ppu: 9,
        ..Image::default()
    });
    assert_eq!(read(&mut nine, 0x2002) & 0x3F, 0x3D);
}

#[test]
fn test_the_other_rgb_ppus_keep_their_registers() {
    let mut system = vs_system(Image {
        ppu: 5,
        ..Image::default()
    });
    system.cpu.bus.ppu.resetting = false;
    write(&mut system, 0x2000, 0x04);
    write(&mut system, 0x2001, 0x1E);
    assert_eq!(
        (system.cpu.bus.ppu.ctrl, system.cpu.bus.ppu.mask),
        (0x04, 0x1E)
    );
    write(&mut system, 0x2005, 0xFF);
    assert_eq!(read(&mut system, 0x2002) & 0x1F, 0x1F);
}

/// Master ticks between frame starts with rendering on, over six frames.
fn frame_lengths(machine: &str) -> std::collections::BTreeSet<i64> {
    let program = [0xa9, 0x08, 0x8d, 0x01, 0x20, 0x4c, 0x00, 0xf0]; // LDA #$08; STA $2001; JMP $F000
    let loaded = cartridge::load(&vs_image(&program, Image::default()), 12, None, false).unwrap();
    let mut system = System::new(loaded.cartridge, Machine::by_key(machine).unwrap());
    system.run_frames(3);
    let mut starts = Vec::new();
    for _ in 0..7 {
        let frame = system.cpu.bus.ppu.frame;
        while system.cpu.bus.ppu.frame == frame {
            system.step();
        }
        let board = &system.cpu.bus;
        let ppu = &board.ppu;
        starts.push(board.t - (ppu.line as i64 * 341 + ppu.dot as i64) * 4 - (board.t - ppu.time));
    }
    starts.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn test_an_rgb_ppu_never_skips_the_odd_frames_dot() {
    let whole = 341 * 262 * 4;
    assert_eq!(frame_lengths("vs"), [whole].into());
    assert_eq!(frame_lengths("nes"), [whole - 4, whole].into());
}

#[test]
fn test_rendering_switched_off_mid_line_corrupts_no_oam_on_an_rgb_ppu() {
    for (name, corrupts) in [("vs", false), ("nes", true)] {
        let loaded =
            cartridge::load(&vs_image(JMP_8000, Image::default()), 12, None, false).unwrap();
        let mut system = System::new(loaded.cartridge, Machine::by_key(name).unwrap());
        let ppu = &mut system.cpu.bus.ppu;
        ppu.resetting = false;
        ppu.write_register(1, 0x18, None, 0);
        let time = ppu.time + (341 * 10 + 100) * 4;
        ppu.run_to(time);
        ppu.write_register(1, 0x00, None, 0);
        assert_eq!(ppu.oam_corrupt().is_some(), corrupts, "{name}");
    }
}

// -- the letterless RP2A03 ---------------------------------------------------

fn apu(tables: ApuTables) -> Apu {
    Apu::new(false, 44100, tables, true, false)
}

#[test]
fn test_the_letterless_2a03_has_no_tonal_noise_and_a_shorter_lowest_period() {
    let (mut old, mut new) = (apu(ApuTables::Rp2a03), apu(ApuTables::Ntsc));
    for apu in [&mut old, &mut new] {
        apu.write(0x400E, 0x8F);
    }
    assert_eq!((old.noise.mode_bit, new.noise.mode_bit), (1, 6));
    assert!(old.noise.period == 2046 / 2 - 1 && new.noise.period == 4068 / 2 - 1);
    old.write(0x400E, 0x0E);
    new.write(0x400E, 0x0E);
    assert_eq!(old.noise.period, new.noise.period);
}

#[test]
fn test_reset_leaves_the_letterless_frame_counter_alone() {
    let (mut old, mut new) = (apu(ApuTables::Rp2a03), apu(ApuTables::Ntsc));
    for apu in [&mut old, &mut new] {
        apu.power();
        for _ in 0..1000 {
            apu.clock();
        }
        apu.reset();
    }
    assert!(new.pending_frame_write.is_some()); // $4017 rewritten: the counter restarts
    assert!(old.pending_frame_write.is_none());
}

// -- the mainboard's ports ---------------------------------------------------

#[test]
fn test_4016_and_4017_carry_the_switches_and_drive_every_line() {
    let mut system = vs_system(Image::default());
    system.cpu.bus.data_bus = 0xFF;
    // D7: a UniSystem game's CPU is the secondary side's (C75).
    assert!(read(&mut system, 0x4016) == 0x80 && read(&mut system, 0x4017) == 0x00);
    system.cpu.bus.vs.as_mut().unwrap().side = Side::Primary;
    assert_eq!(read(&mut system, 0x4016), 0x00);
    system.cpu.bus.vs.as_mut().unwrap().dip = 0b0000_0001; // switch 1: $4016 D3
    assert_eq!(read(&mut system, 0x4016), 0x08);
    system.cpu.bus.vs.as_mut().unwrap().dip = 0b0000_0010; // switch 2: $4016 D4
    assert_eq!(read(&mut system, 0x4016), 0x10);
    system.cpu.bus.vs.as_mut().unwrap().dip = 0b1111_1100; // switches 3-8: $4017 D2-D7
    assert!(read(&mut system, 0x4016) == 0x00 && read(&mut system, 0x4017) == 0xFC);
    system.cpu.bus.vs.as_mut().unwrap().dip = 0;
    system.cpu.bus.vs.as_mut().unwrap().service = true;
    assert_eq!(read(&mut system, 0x4016), 0x04);
    assert!(Machine::famicom().vs_side.is_none());
}

#[test]
fn test_a_coin_is_a_pulse_of_about_fifty_milliseconds() {
    let mut system = vs_system(Image::default());
    system.insert_coin(0).unwrap();
    assert_eq!(read(&mut system, 0x4016) & 0x60, 0x20);
    system.run_cycles((0.040f64 * 1_789_773.0).round() as i64);
    assert_eq!(read(&mut system, 0x4016) & 0x60, 0x20);
    system.run_cycles((0.011f64 * 1_789_773.0).round() as i64);
    assert_eq!(read(&mut system, 0x4016) & 0x60, 0x00);
    system.insert_coin(1).unwrap(); // the second acceptor is its own bit
    assert_eq!(read(&mut system, 0x4016) & 0x60, 0x40);
    let image = vs_image(JMP_8000, Image::default());
    let mut famicom = System::from_rom(&image, Machine::famicom(), None).unwrap();
    assert!(famicom.insert_coin(0).is_err());
}

#[test]
fn test_the_sticks_report_as_controllers_do() {
    let mut system = vs_system(Image::default());
    system.cpu.bus.pad_as::<VsStick>(0).unwrap().buttons = A | SELECT;
    system.cpu.bus.pad_as::<VsStick>(1).unwrap().buttons = B;
    write(&mut system, 0x4016, 1);
    settle(&mut system);
    write(&mut system, 0x4016, 0);
    settle(&mut system);
    let mut bits = |address: u16| -> Vec<u8> {
        (0..8)
            .map(|_| {
                let bit = read(&mut system, address) & 1;
                read(&mut system, 0x0000); // /OE rises: the register shifts
                bit
            })
            .collect()
    };
    let right = bits(0x4016);
    let left = bits(0x4017);
    assert_eq!(right, [1, 0, 1, 0, 0, 0, 0, 0]);
    assert_eq!(left, [0, 1, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn test_the_panel_puts_player_one_where_the_game_reads() {
    let mut pads: [Box<dyn PortDevice>; 2] =
        [Box::new(VsStick::default()), Box::new(VsStick::default())];
    let buttons = |pads: &mut [Box<dyn PortDevice>; 2], n: usize| {
        pads[n]
            .as_any_mut()
            .downcast_mut::<VsStick>()
            .unwrap()
            .buttons
    };
    panel(&mut pads, &[A | START | LEFT, B | SELECT], false);
    assert!(buttons(&mut pads, 0) == A | START | LEFT && buttons(&mut pads, 1) == B | SELECT);
    panel(&mut pads, &[A | START | LEFT, B | SELECT], true);
    // The sticks change sides; buttons 1 and 3 stay on $4016, 2 and 4 on $4017.
    assert!(buttons(&mut pads, 1) == A | LEFT | SELECT && buttons(&mut pads, 0) == B | START);
}

#[test]
fn test_the_protection_jumper_holds_the_fourth_bit() {
    let image = vs_image(
        JMP_8000,
        Image {
            hardware: 4,
            ..Image::default()
        },
    );
    let mut system = System::from_rom(&image, Machine::vs_machine(0), None).unwrap();
    for port in 0..2 {
        assert_eq!(
            system.cpu.bus.pad_as::<VsStick>(port).unwrap().jumper,
            START
        );
    }
    let plain = vs_image(JMP_8000, Image::default());
    let mut other = System::from_rom(&plain, Machine::vs_machine(0), None).unwrap();
    assert_eq!(other.cpu.bus.pad_as::<VsStick>(0).unwrap().jumper, 0);
    let mut stick = VsStick::new(START);
    stick.set_strobe(true);
    stick.set_strobe(false);
    let bits: Vec<u8> = (0..8)
        .map(|_| {
            let bit = stick.read();
            stick.clock();
            bit
        })
        .collect();
    assert_eq!(bits, [0, 0, 0, 1, 0, 0, 0, 0]);
}

#[test]
fn test_the_vs_zapper_reports_through_the_shift_register() {
    let image = vs_image(
        JMP_8000,
        Image {
            expansion: VS_ZAPPER,
            ..Image::default()
        },
    );
    let mut system = System::from_rom(&image, Machine::vs_machine(0), None).unwrap();
    assert!(matches!(system.plug(None), Plugged::Pad(0)));
    let report = |system: &mut System| -> u8 {
        let board = &mut system.cpu.bus;
        let ppu = &board.ppu;
        let gun = board.pads[0].as_mut();
        gun.set_strobe(true, ppu);
        gun.set_strobe(false, ppu);
        let mut value = 0;
        for bit in 0..8 {
            value |= gun.read(ppu) << bit;
            gun.clock(ppu);
        }
        value
    };
    assert_eq!(report(&mut system), UP); // 0, 0, 0, 0, 1, 0, light, trigger
    system
        .cpu
        .bus
        .pad_as::<VsZapper>(0)
        .unwrap()
        .zapper
        .set_trigger(true, 0);
    assert_eq!(report(&mut system), UP | RIGHT);
    system
        .cpu
        .bus
        .pad_as::<VsZapper>(0)
        .unwrap()
        .zapper
        .set_trigger(false, 0);
    system.cpu.bus.pad_as::<VsZapper>(0).unwrap().zapper.aim = Some((128, 120));
    let ppu = &mut system.cpu.bus.ppu;
    ppu.picture[120 * 256 + 128] = 0x20; // white, just drawn
    ppu.line = 121;
    ppu.dot = 0;
    assert_eq!(report(&mut system), UP | LEFT); // light sense is 1 when detecting
}

#[test]
fn test_the_coin_counter_counts_falling_edges_anywhere_in_4020_5fff() {
    let mut system = vs_system(Image::default());
    let counted = |system: &System| {
        let vs = system.cpu.bus.vs.as_ref().unwrap();
        (vs.coin_counter, vs.coins_counted)
    };
    write(&mut system, 0x4020, 1);
    assert_eq!(counted(&system), (1, 0));
    write(&mut system, 0x5FFF, 0);
    assert_eq!(counted(&system), (0, 1));
    write(&mut system, 0x4020, 1);
    system.cpu.bus.data_bus = 0x40; // "reading from the register effectively writes ... open bus"
    assert_eq!(read(&mut system, 0x4020), 0x40);
    assert_eq!(counted(&system).1, 2);
    write(&mut system, 0x4020, 1);
    system.reset();
    assert_eq!(counted(&system).0, 0);
}

#[test]
fn test_the_ram_at_6000_is_two_kilobytes_mirrored() {
    let mut system = vs_system(Image::default());
    write(&mut system, 0x6123, 0x5A);
    for address in [0x6123, 0x6923, 0x7123, 0x7923] {
        assert_eq!(read(&mut system, address), 0x5A);
    }
    write(&mut system, 0x7FFF, 0xA5);
    assert_eq!(read(&mut system, 0x67FF), 0xA5);
}

#[test]
fn test_the_ram_is_the_mainboards_whatever_the_daughterboard() {
    // Mapper 206 has no RAM of its own and MMC1 can switch its own off.
    for mapper in [206, 1, 2, 67, 75] {
        let mut system = vs_system(Image {
            mapper,
            prg_size: 0x10000,
            chr_size: 0x8000,
            ..Image::default()
        });
        write(&mut system, 0x6000, 0x77);
        assert_eq!(read(&mut system, 0x6800), 0x77, "{mapper}");
    }
}

#[test]
fn test_every_vs_mapper_has_four_screens() {
    for mapper in [99, 1, 2, 67, 75, 206] {
        let mut image = vs_image(
            JMP_8000,
            Image {
                mapper,
                prg_size: 0x10000,
                chr_size: 0x8000,
                ..Image::default()
            },
        );
        image[6] |= 0x08; // four-screen: every Vs. image's header says so
        let mut cartridge = cartridge::load(&image, 12, None, false).unwrap().cartridge;
        for table in 0..4u16 {
            assert!(cartridge.ciram_a10(0x2000 + table * 0x400).is_none());
            cartridge.ppu_write(0x2000 + table * 0x400, table as u8 + 1, 0);
        }
        let got: Vec<Option<u8>> = (0..4u16)
            .map(|t| cartridge.ppu_read(0x2000 + t * 0x400, 0))
            .collect();
        assert_eq!(got, [Some(1), Some(2), Some(3), Some(4)], "{mapper}");
    }
}

#[test]
fn test_a_save_state_keeps_the_mainboard() {
    // LDA $4016; STA $6000; INC $6001; JMP $F000
    let program = [
        0xad, 0x16, 0x40, 0x8d, 0x00, 0x60, 0xee, 0x01, 0x60, 0x4c, 0x00, 0xf0,
    ];
    let image = vs_image(
        &program,
        Image {
            ppu: 5,
            ..Image::default()
        },
    );
    let mut first = System::from_rom(&image, Machine::vs_machine(5), None).unwrap();
    first.run_cycles(5000);
    first.insert_coin(0).unwrap();
    first.cpu.bus.vs.as_mut().unwrap().dip = 0x41;
    first.run_cycles(100);
    let mut second = System::from_rom(&image, Machine::vs_machine(5), None).unwrap();
    load_state(&mut second, &save_state(&first)).unwrap();
    for system in [&mut first, &mut second] {
        system.run_cycles(200_000); // past the coin's 50 ms
    }
    let (one, two) = (
        first.cpu.bus.vs.as_ref().unwrap(),
        second.cpu.bus.vs.as_ref().unwrap(),
    );
    assert!(first.cpu.bus.t == second.cpu.bus.t && one.ram.data == two.ram.data);
    assert_eq!(
        (one.dip, &one.coins, one.coin_counter),
        (two.dip, &two.coins, two.coin_counter)
    );
    assert_ne!(one.ram.data[1], 0);
}

// -- mapper 99 ---------------------------------------------------------------

#[test]
fn test_out2_selects_the_chr_socket() {
    let mut system = vs_system(Image::default());
    assert_eq!(cart(&mut system).ppu_read(0x0000, 0), Some(0xC0));
    write(&mut system, 0x4016, 0x04);
    settle(&mut system);
    assert_eq!(cart(&mut system).ppu_read(0x0000, 0), Some(0xC1));
    assert_eq!(cart(&mut system).ppu_read(0x1FFF, 0), Some(0xC1));
    write(&mut system, 0x4016, 0x00);
    settle(&mut system);
    assert_eq!(cart(&mut system).ppu_read(0x0000, 0), Some(0xC0));
}

#[test]
fn test_an_empty_chr_socket_drives_nothing() {
    let mut system = vs_system(Image {
        chr_size: 0x2000,
        ..Image::default()
    });
    write(&mut system, 0x4016, 0x04);
    settle(&mut system);
    assert_eq!(cart(&mut system).ppu_read(0x0000, 0), None);
}

#[test]
fn test_a_24k_image_lies_at_a000_and_8000_is_open_bus() {
    let image = vs_image(
        JMP_8000,
        Image {
            prg_size: 0x6000,
            ..Image::default()
        },
    );
    let mut cartridge = cartridge::load(&image, 12, None, false).unwrap().cartridge;
    assert!(cartridge.cpu_read(0x8000, 0).is_none() && cartridge.cpu_read(0x9FFF, 0).is_none());
    let got: Vec<Option<u16>> = [0xBFF0, 0xDFF0, 0xFFF0]
        .iter()
        .map(|&a| cartridge.cpu_read(a, 0))
        .collect();
    assert_eq!(got, [Some(0), Some(1), Some(2)]);
}

#[test]
fn test_gumshoes_fifth_bank_follows_out2() {
    let mut system = vs_system(Image {
        prg_size: 0xA000,
        ..Image::default()
    });
    let banks = |system: &mut System| -> Vec<Option<u16>> {
        [0x9FF0, 0xBFF0, 0xDFF0, 0xFFF0]
            .iter()
            .map(|&a| cart(system).cpu_read(a, 0))
            .collect()
    };
    assert_eq!(banks(&mut system), [Some(0), Some(1), Some(2), Some(3)]);
    write(&mut system, 0x4016, 0x04);
    settle(&mut system);
    assert_eq!(banks(&mut system), [Some(4), Some(1), Some(2), Some(3)]);
}

// -- the daughterboards' protection chips -----------------------------------

#[test]
fn test_the_namco_108_boards_get_their_chips_by_hardware_type() {
    let board_for = |hardware: u8| {
        let image = vs_image(
            JMP_8000,
            Image {
                mapper: 206,
                prg_size: 0x10000,
                chr_size: 0x8000,
                hardware,
                ..Image::default()
            },
        );
        cartridge::load(&image, 12, None, false).unwrap().cartridge
    };
    let mut plain = board_for(0);
    assert!(plain.cpu_read(0x5E01, 0).is_none()); // the plain Namco108
    let mut rbi = board_for(1); // a VsProtected108
    assert_eq!(rbi.cpu_read(0x5E00, 0), Some(0));
    let got: Vec<Option<u16>> = (0..4).map(|_| rbi.cpu_read(0x5E01, 0)).collect();
    assert_eq!(got, [Some(0xFF), Some(0xFD), Some(0xF5), Some(0xF4)]);
    assert_eq!(rbi.cpu_read(0x5600, 0), Some(0));
    assert_eq!(rbi.cpu_read(0x5601, 0), Some(0xFF)); // A11 not decoded
    assert!(rbi.cpu_read(0x5E02, 0).is_none());
    let mut tko = board_for(2);
    assert_eq!(tko.cpu_read(0x5E00, 0), Some(0));
    let got: Vec<Option<u16>> = (0..3).map(|_| tko.cpu_read(0x5E01, 0)).collect();
    assert_eq!(got, [Some(0xFF), Some(0xBF), Some(0xB7)]);
    assert!(tko.cpu_read(0x5601, 0).is_none());
    for _ in 0..29 {
        tko.cpu_read(0x5E01, 0);
    }
    assert_eq!(tko.cpu_read(0x5E01, 0), Some(0xFF)); // the 32 bytes come round
    let mut xevious = board_for(3);
    let answers = |cart: &mut Box<dyn Cartridge>| -> Vec<Option<u16>> {
        [0x5678, 0x578F, 0x5567]
            .iter()
            .map(|&a| cart.cpu_read(a, 0))
            .collect()
    };
    assert_eq!(answers(&mut xevious), [Some(0x00), Some(0xD1), Some(0x3E)]);
    assert_eq!(xevious.cpu_read(0x54FF, 0), Some(0x05));
    assert_eq!(answers(&mut xevious), [Some(0x01), Some(0x89), Some(0x37)]);
    assert!(xevious.cpu_read(0xFFF0, 0).is_some());
}

#[test]
fn test_a_protected_board_still_sees_the_coin_counter() {
    let mut system = vs_system(Image {
        mapper: 206,
        prg_size: 0x10000,
        chr_size: 0x8000,
        hardware: 2,
        ..Image::default()
    });
    write(&mut system, 0x4020, 1);
    system.cpu.bus.data_bus = 0x5E; // as LDA $5E00 leaves it: the address's high byte
    assert_eq!(read(&mut system, 0x5E00), 0); // the chip answers; the read clocked the latch
    assert_eq!(system.cpu.bus.vs.as_ref().unwrap().coins_counted, 1);
}

// -- the DualSystem (tests/test_vs_dual.py) ----------------------------------

fn dual_image(program: &[u8]) -> Vec<u8> {
    vs_image(
        program,
        Image {
            prg_size: 0x10000,
            chr_size: 0x8000,
            hardware: 5,
            ..Image::default()
        },
    )
}

#[test]
fn test_a_dual_image_is_split_between_the_sides() {
    let data = dual_image(JMP_8000);
    let loaded = cartridge::load(&data, 12, None, false).unwrap();
    let (first, second) = split(&loaded.image);
    assert_eq!((first.prg.len(), second.prg.len()), (0x8000, 0x8000));
    assert_eq!((first.chr.len(), second.chr.len()), (0x4000, 0x4000));
    assert!(first.prg == loaded.image.prg[..0x8000] && second.prg == loaded.image.prg[0x8000..]);
    assert!(first.chr[0] == 0xC0 && second.chr[0] == 0xC2);
}

#[test]
fn test_each_side_answers_its_own_d7() {
    let mut dual = VsDualSystem::from_rom(&dual_image(JMP_8000)).unwrap();
    assert_eq!(read(&mut dual.primary, 0x4016) & 0x80, 0x00);
    assert_eq!(read(&mut dual.secondary, 0x4016) & 0x80, 0x80);
}

#[test]
fn test_the_two_sides_run_on_one_clock() {
    let mut dual = VsDualSystem::from_rom(&dual_image(JMP_8000)).unwrap();
    dual.run_frames(3);
    let (one, two) = (dual.primary.cpu.bus.t, dual.secondary.cpu.bus.t);
    assert!((one - two).abs() <= 7 * 12 + 4 * 12, "{one} {two}");
    assert_eq!(
        dual.primary.cpu.bus.ppu.frame,
        dual.secondary.cpu.bus.ppu.frame
    );
}
