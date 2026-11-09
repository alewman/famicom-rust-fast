//! The lockstep gate's Rust side: the same lines as scripts/famicom_trace.py
//! writes for the reference (see there for the format).
//!
//!     famicom-trace ROM --slices N [--machine KEY] [--input FILE] [--device N]
//!                   [--reset FRAME ...] [--detail FRAME] [--auto-disk]
//!                   [--host FILE [--host-device N]] --out FILE
//!
//! `--host FILE`: [`famicom_rust::host`] drives the console as the reference
//! tracer's `--host` drives it through the reference's window (FILE's format
//! is there, `read_host`); each line ends with ` host` and
//! [`famicom_rust::host::describe`].
//!
//! `--auto-disk`: the FDS's disks change as the program asks (`fds_auto`),
//! each change written to stderr with its slice.
//!
//! `--device` and the `FRAME set PATH FIELD VALUE...` input lines are the
//! reference tracer's: [`resolve`] walks PATH from what `plug` returned (a
//! [`Plugged`]) and [`drive`] sets FIELD as the reference's `drive` does.

use famicom_rust::board::Board;
use famicom_rust::fds_auto::AutoDisk;
use famicom_rust::host::{self, Gamepad, HostInput};
use famicom_rust::joypad::StandardController;
use famicom_rust::peripherals::*;
use famicom_rust::{Machine, System};
use std::any::Any;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

fn crc(bytes: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

/// Python's `int(text, 0)`, as far as the input files use it.
fn int(text: &str) -> i64 {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let value = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => i64::from_str_radix(hex, 16),
        None => digits.parse(),
    }
    .unwrap_or_else(|_| panic!("not a number: {text}"));
    if negative {
        -value
    } else {
        value
    }
}

fn bool_value(text: &str) -> bool {
    text == "1" || text == "true"
}

fn index(token: Option<&str>) -> usize {
    token.expect("an index").parse().expect("an index")
}

/// The object PATH names (`-`: itself; dot-separated fields and indices),
/// from what `plug` returned.
fn resolve<'a>(board: &'a mut Board, plugged: &'a mut Plugged, path: &str) -> &'a mut dyn Any {
    let tokens: Vec<&str> = if path == "-" {
        Vec::new()
    } else {
        path.split('.').collect()
    };
    let mut tokens = tokens.into_iter();
    let mut target: &mut dyn Any = match plugged {
        Plugged::None => panic!("no device to drive"),
        Plugged::Rob(rob) => rob,
        Plugged::FourScore => {
            assert_eq!(tokens.next(), Some("pads"));
            let n = index(tokens.next());
            let port = board.pad_as::<TapPort>(n % 2).unwrap();
            if n < 2 {
                &mut port.first
            } else {
                &mut port.second
            }
        }
        Plugged::Pad(port) => board.pads[*port].as_any_mut(),
        Plugged::Pads => board.pads[index(tokens.next())].as_any_mut(),
        Plugged::Expansion => board.expansion.as_mut().unwrap().as_any_mut(),
        Plugged::ExpansionZapper => &mut board.expansion_as::<FamicomZapper>().unwrap().zapper,
        Plugged::PadVaus(port) => &mut board.pad_as::<VausNes>(*port).unwrap().vaus,
        Plugged::ExpansionVaus => &mut board.expansion_as::<VausFamicom>().unwrap().vaus,
        Plugged::VausPair => {
            let pair = board.expansion_as::<VausFamicom>().unwrap();
            match index(tokens.next()) {
                0 => &mut pair.vaus,
                _ => pair.second.as_mut().unwrap(),
            }
        }
        Plugged::Recorder => board
            .expansion_as::<FamilyBasicKeyboard>()
            .unwrap()
            .recorder
            .as_mut()
            .unwrap(),
        Plugged::SuborPair(port) => match index(tokens.next()) {
            0 => board.expansion.as_mut().unwrap().as_any_mut(),
            _ => board.pads[*port].as_any_mut(),
        },
    };
    while let Some(token) = tokens.next() {
        target = match token {
            "pads" if target.is::<HoriAdapter>() => {
                let n = index(tokens.next());
                TapPort::player(&mut target.downcast_mut::<HoriAdapter>().unwrap().ports, n)
            }
            "pads" if target.is::<ExpansionPads>() => {
                &mut target.downcast_mut::<ExpansionPads>().unwrap().pads[index(tokens.next())]
            }
            "through" => &mut target.downcast_mut::<Famicom3DSystem>().unwrap().through,
            "recorder" => target
                .downcast_mut::<FamilyBasicKeyboard>()
                .unwrap()
                .recorder
                .as_mut()
                .unwrap(),
            "zapper" => &mut target.downcast_mut::<BandaiHyperShot>().unwrap().zapper,
            "pad" if target.is::<HoriTrack>() => {
                &mut target.downcast_mut::<HoriTrack>().unwrap().pad
            }
            "pad" if target.is::<CoconutsPachinko>() => {
                &mut target.downcast_mut::<CoconutsPachinko>().unwrap().pad
            }
            "pad" if target.is::<BandaiHyperShot>() => {
                &mut target.downcast_mut::<BandaiHyperShot>().unwrap().pad
            }
            "pad" if target.is::<PortTestPort>() => {
                &mut target.downcast_mut::<PortTestPort>().unwrap().pad
            }
            _ => panic!("no {token} in {path}"),
        };
    }
    target
}

/// A pointing device's host side: `left`, `right`, `move`.
fn drive_pointer(pointer: &mut Pointer, field: &str, values: &[&str]) {
    match field {
        "left" => pointer.left = bool_value(values[0]),
        "right" => pointer.right = bool_value(values[0]),
        "move" => pointer.move_by(int(values[0]), int(values[1])),
        _ => panic!("cannot set {field}"),
    }
}

fn keys(values: &[&str]) -> std::collections::HashSet<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// `set PATH FIELD VALUE...` on the object `resolve` found, as the reference
/// tracer's `drive` does with setattr or a method call; `time` is `ppu.time`.
fn drive(target: &mut dyn Any, field: &str, values: &[&str], time: i64) {
    let v = |i: usize| values[i];
    let only = |name: &str| assert_eq!(field, name, "cannot set {field}");
    if let Some(d) = target.downcast_mut::<StandardController>() {
        only("buttons");
        d.buttons = int(v(0)) as u8;
    } else if let Some(d) = target.downcast_mut::<Zapper>() {
        match field {
            "aim" => {
                d.aim = if v(0) == "none" {
                    None
                } else {
                    Some((int(v(0)), int(v(1))))
                }
            }
            "trigger" => d.set_trigger(bool_value(v(0)), time),
            _ => panic!("cannot set {field}"),
        }
    } else if let Some(d) = target.downcast_mut::<Vaus>() {
        match field {
            "position" => d.position = int(v(0)),
            "fire" => d.fire = bool_value(v(0)),
            _ => panic!("cannot set {field}"),
        }
    } else if let Some(d) = target.downcast_mut::<PowerPad>() {
        only("buttons");
        d.buttons = int(v(0)) as u32;
    } else if let Some(d) = target.downcast_mut::<FamilyTrainer>() {
        only("buttons");
        d.buttons = int(v(0)) as u32;
    } else if let Some(d) = target.downcast_mut::<KonamiHyperShot>() {
        only("buttons");
        d.buttons = int(v(0)) as u8;
    } else if let Some(d) = target.downcast_mut::<Partytap>() {
        only("buttons");
        d.buttons = int(v(0)) as u8;
    } else if let Some(d) = target.downcast_mut::<SnesController>() {
        only("buttons");
        d.buttons = int(v(0)) as u16;
    } else if let Some(d) = target.downcast_mut::<ExcitingBoxing>() {
        only("sensors");
        d.sensors = int(v(0)) as u8;
    } else if let Some(d) = target.downcast_mut::<FamilyBasicKeyboard>() {
        only("keys");
        d.keys = keys(values);
    } else if let Some(d) = target.downcast_mut::<SuborKeyboard>() {
        only("keys");
        d.keys = keys(values);
    } else if let Some(d) = target.downcast_mut::<JissenMahjong>() {
        only("keys");
        d.keys = keys(values);
    } else if let Some(d) = target.downcast_mut::<DataRecorder>() {
        match field {
            "play" => d.play(time),
            "record" => d.record(time),
            "stop" => d.stop(),
            _ => panic!("cannot set {field}"),
        }
    } else if let Some(d) = target.downcast_mut::<TurboFile>() {
        only("locked");
        d.locked = bool_value(v(0));
    } else if let Some(d) = target.downcast_mut::<SnesMouse>() {
        if field == "sensitivity" {
            d.sensitivity = int(v(0));
        } else {
            drive_pointer(&mut d.pointer, field, values);
        }
    } else if let Some(d) = target.downcast_mut::<SuborMouse>() {
        drive_pointer(&mut d.pointer, field, values);
    } else if let Some(d) = target.downcast_mut::<MacroWinnersMouse>() {
        drive_pointer(&mut d.pointer, field, values);
    } else if let Some(d) = target.downcast_mut::<YuxingMouse>() {
        drive_pointer(&mut d.pointer, field, values);
    } else if let Some(d) = target.downcast_mut::<HoriTrack>() {
        match field {
            "rotation_l" => d.rotation_l = bool_value(v(0)),
            "slow" => d.slow = bool_value(v(0)),
            _ => drive_pointer(&mut d.pointer, field, values),
        }
    } else if let Some(d) = target.downcast_mut::<CoconutsPachinko>() {
        only("dial");
        d.dial = int(v(0));
    } else if let Some(d) = target.downcast_mut::<OekaKidsTablet>() {
        match field {
            "x" => d.x = int(v(0)),
            "y" => d.y = int(v(0)),
            "touching" => d.touching = bool_value(v(0)),
            "click" => d.click = bool_value(v(0)),
            _ => panic!("cannot set {field}"),
        }
    } else {
        panic!("cannot set {field}");
    }
}

/// `--host FILE`'s lines by slice (the reference tracer's `read_host`).
fn read_host(path: &str) -> HashMap<u64, Vec<Vec<String>>> {
    let mut script: HashMap<u64, Vec<Vec<String>>> = HashMap::new();
    for text in std::fs::read_to_string(path).unwrap().lines() {
        let parts: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        if !parts.is_empty() && !text.starts_with('#') {
            script
                .entry(parts[0].parse().unwrap())
                .or_default()
                .push(parts[1..].to_vec());
        }
    }
    script
}

/// One slice's host lines into `input` (the reference tracer's `host_step`).
fn host_step(input: &mut HostInput, lines: &[Vec<String>]) {
    input.mouse_dx = 0;
    input.mouse_dy = 0;
    let on = |v: &str| v == "1";
    for line in lines {
        let v: Vec<&str> = line.iter().map(|s| s.as_str()).collect();
        match v[0] {
            "gamepads" => {
                let n: usize = v[1].parse().unwrap();
                if input.gamepads.len() < n {
                    input.gamepads.resize(n, Gamepad::default());
                }
            }
            "key" => {
                if on(v[2]) {
                    input.keys.insert(v[1].to_owned());
                } else {
                    input.keys.remove(v[1]);
                }
            }
            "mouse" => {
                input.mouse_x = v[1].parse().unwrap();
                input.mouse_y = v[2].parse().unwrap();
            }
            "move" => {
                input.mouse_dx = v[1].parse().unwrap();
                input.mouse_dy = v[2].parse().unwrap();
            }
            "click" => match v[1] {
                "left" => input.mouse_left = on(v[2]),
                "right" => input.mouse_right = on(v[2]),
                other => panic!("host: click {other}?"),
            },
            "focus" => input.mouse_focused = on(v[1]),
            "pad" => {
                let pad = &mut input.gamepads[v[1].parse::<usize>().unwrap()];
                let held = on(v[3]);
                match v[2] {
                    "up" => pad.up = held,
                    "down" => pad.down = held,
                    "left" => pad.left = held,
                    "right" => pad.right = held,
                    "south" => pad.south = held,
                    "east" => pad.east = held,
                    "north" => pad.north = held,
                    "back" => pad.back = held,
                    "start" => pad.start = held,
                    "lshoulder" => pad.left_shoulder = held,
                    "rshoulder" => pad.right_shoulder = held,
                    other => panic!("host: pad field {other}?"),
                }
            }
            "stick" => {
                let pad = &mut input.gamepads[v[1].parse::<usize>().unwrap()];
                pad.stick_x = v[2].parse().unwrap();
                pad.stick_y = v[3].parse().unwrap();
            }
            other => panic!("host: {other}?"),
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut rom = None;
    let mut slices = 0u64;
    let mut machine = "nes".to_string();
    let mut input = None;
    let mut resets = Vec::new();
    let mut detail = None;
    let mut device = None;
    let mut out = None;
    let mut auto_disk = false;
    let mut host_file = None;
    let mut host_device = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--slices" => slices = args.next().unwrap().parse().unwrap(),
            "--machine" => machine = args.next().unwrap(),
            "--input" => input = args.next(),
            "--reset" => resets.push(args.next().unwrap().parse::<u64>().unwrap()),
            "--detail" => detail = Some(args.next().unwrap().parse::<u64>().unwrap()),
            "--device" => device = args.next(),
            "--out" => out = args.next(),
            "--auto-disk" => auto_disk = true,
            "--host" => host_file = args.next(),
            "--host-device" => host_device = args.next(),
            _ => rom = Some(arg),
        }
    }
    let rom = PathBuf::from(rom.expect("ROM"));
    let mut inputs: HashMap<u64, Vec<(usize, u8)>> = HashMap::new();
    let mut sets: HashMap<u64, Vec<Vec<String>>> = HashMap::new();
    if let Some(path) = input {
        for text in std::fs::read_to_string(path).unwrap().lines() {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if text.starts_with('#') {
                continue;
            }
            if parts.len() == 3 {
                inputs.entry(parts[0].parse().unwrap()).or_default().push((
                    parts[1].parse().unwrap(),
                    u8::from_str_radix(parts[2], 16).unwrap(),
                ));
            } else if parts.len() >= 4 && parts[1] == "set" {
                sets.entry(parts[0].parse().unwrap())
                    .or_default()
                    .push(parts[2..].iter().map(|p| p.to_string()).collect());
            }
        }
    }
    let machine = Machine::by_key(&machine).expect("machine");
    let mut system = match System::from_path(&rom, machine) {
        Ok(system) => system,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let host_script = host_file.map(|path| read_host(&path));
    let mut host_input = HostInput {
        mouse_focused: true,
        ..HostInput::default()
    };
    let mut plugged = match device.as_deref() {
        _ if host_script.is_some() => {
            let device = match host_device.as_deref() {
                Some(number) => int(number) as u8,
                None => system.image.as_ref().map_or(0, |i| i.board.expansion),
            };
            let famicom = system.cpu.bus.machine.famicom_expansion_port;
            host::plug_for_host(system.board(), device, famicom)
        }
        None => Plugged::None,
        Some("auto") => system.plug(None),
        Some("hori-adapter") => {
            system.board().expansion = Some(Box::new(HoriAdapter::new()));
            Plugged::Expansion
        }
        Some(number) => system.plug(Some(int(number) as u8)),
    };
    system.board().bus_log = Some(Vec::with_capacity(40_000));
    if auto_disk {
        let board = system.board();
        let auto = board
            .ppu
            .cart
            .fds()
            .map(AutoDisk::new)
            .expect("--auto-disk: not a disk system");
        board.auto_disk = Some(Box::new(auto));
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(out.expect("--out")).unwrap());
    let mut bytes = Vec::new();
    for frame in 0..slices {
        if let Some(list) = inputs.get(&frame) {
            for &(pad, buttons) in list {
                let device = system.board().pads[pad].as_any_mut();
                if let Some(pad) = device.downcast_mut::<StandardController>() {
                    pad.buttons = buttons;
                }
            }
        }
        for line in sets.get(&frame).into_iter().flatten() {
            let values: Vec<&str> = line[2..].iter().map(|v| v.as_str()).collect();
            let time = system.cpu.bus.ppu.time;
            let target = resolve(&mut system.cpu.bus, &mut plugged, &line[0]);
            drive(target, &line[1], &values, time);
        }
        let mut summary = None;
        if let Some(script) = &host_script {
            host_step(&mut host_input, script.get(&frame).map_or(&[][..], |v| v));
            host::apply(system.board(), &mut plugged, &host_input, host::VAUS_LOW);
            summary = Some(host::describe(system.board(), &mut plugged));
        }
        if resets.contains(&frame) {
            system.reset();
        }
        system.board().bus_log.as_mut().unwrap().clear();
        system.run_frames(1);
        if let Some(auto) = system.board().auto_disk.as_mut() {
            for message in auto.messages.drain(..) {
                eprintln!("slice {frame}: {message}");
            }
        }
        let samples = system.take_samples();
        let board = &system.cpu.bus;
        let log = board.bus_log.as_ref().unwrap();
        bytes.clear();
        for packed in log {
            bytes.extend_from_slice(&packed.to_le_bytes());
        }
        let bus = crc(&bytes);
        bytes.clear();
        for value in &board.ppu.picture {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let pic = crc(&bytes);
        bytes.clear();
        for sample in &samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let snd = crc(&bytes);
        let cpu = &system.cpu;
        let ppu = &board.ppu;
        write!(
            out,
            "{frame} cyc {} ins {} pc {:04X} a {:02X} x {:02X} y {:02X} s {:02X} p {:02X} \
             bus {bus:08X} {} pic {pic:08X} v {:04X} t {:04X} st {:02X} snd {snd:08X} {}",
            board.cycles(),
            system.instructions,
            cpu.pc,
            cpu.a,
            cpu.x,
            cpu.y,
            cpu.s,
            cpu.p,
            log.len(),
            ppu.v,
            ppu.t,
            ppu.status,
            samples.len()
        )
        .unwrap();
        if let Plugged::Rob(rob) = &mut plugged {
            rob.see(&board.ppu.picture);
            write!(
                out,
                " rob {} {} {} {} {}",
                rob.height,
                rob.turn,
                rob.arms_closed as u8,
                rob.led as u8,
                rob.commands.len()
            )
            .unwrap();
        }
        if let Some(summary) = &summary {
            write!(out, " host {summary}").unwrap();
        }
        writeln!(out).unwrap();
        if detail == Some(frame) {
            for &packed in log {
                let t = packed >> 25;
                let rest = packed & 0x1FF_FFFF;
                let kind = if rest >> 24 != 0 { "W" } else { "R" };
                writeln!(
                    out,
                    "  {t} {kind} {:04X} {:02X}",
                    (rest >> 8) & 0xFFFF,
                    rest & 0xFF
                )
                .unwrap();
            }
        }
    }
}
