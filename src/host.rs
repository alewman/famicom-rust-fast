//! A player's input on the board's devices: the input half of the
//! reference's window (`frontend.py`: `_power_on`'s device choice,
//! `_players`, `_apply_inputs`, R.O.B. in `run_frame`), without its window.
//!
//! The host (a libretro core, a desktop window) fills a [`HostInput`] each
//! frame: the gamepads' buttons, the mouse in picture pixels, and the host
//! keys held, by the reference's pygame names (`"K_a"`, `"K_RETURN"`, ...),
//! so its key tables are kept here verbatim. [`apply`] drives the pads and
//! the plugged device from it as the reference does. Not here: the window's
//! own keys (reset, pause, the disk, the Data Recorder's tape keys) and the
//! Turbo File's and the tape's files, which are a host's storage.

use crate::board::Board;
use crate::joypad::{StandardController, A, B, DOWN, LEFT, RIGHT, SELECT, START, UP};
use crate::peripherals::*;
use std::any::Any;
use std::collections::HashSet;

/// `--vaus-low`'s default: the knob's value at the left edge.
pub const VAUS_LOW: i64 = 0x62;

/// One game controller as SDL reports it (the reference reads pygame's
/// `CONTROLLER_*`): the face buttons by position, the left stick raw.
#[derive(Clone, Debug, Default)]
pub struct Gamepad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// The lower face button (SDL's A, an Xbox pad's A, libretro's B).
    pub south: bool,
    /// The right one (SDL's B, libretro's A).
    pub east: bool,
    /// The upper one (SDL's Y, libretro's X).
    pub north: bool,
    pub back: bool,
    pub start: bool,
    pub left_shoulder: bool,
    pub right_shoulder: bool,
    /// The left stick, -32768 to 32767.
    pub stick_x: i32,
    pub stick_y: i32,
}

impl Gamepad {
    /// Its standard-pad buttons (`_controller`): the d-pad or the stick past
    /// half way, the lower face button B, the right one A.
    pub fn buttons(&self) -> u8 {
        let x = self.stick_x as f64 / 32768.0;
        let y = self.stick_y as f64 / 32768.0;
        [
            (UP, self.up || y < -0.5),
            (DOWN, self.down || y > 0.5),
            (LEFT, self.left || x < -0.5),
            (RIGHT, self.right || x > 0.5),
            (B, self.south),
            (A, self.east),
            (SELECT, self.back),
            (START, self.start),
        ]
        .iter()
        .filter(|(_, held)| *held)
        .map(|(button, _)| button)
        .sum()
    }

    /// A Super NES pad's buttons beyond the NES's (`_snes_extra`): A, X, L, R
    /// as bits 8-11.
    pub fn snes_extra(&self) -> u16 {
        [
            (8, self.east),
            (9, self.north),
            (10, self.left_shoulder),
            (11, self.right_shoulder),
        ]
        .iter()
        .filter(|(_, held)| *held)
        .map(|(bit, _)| 1u16 << bit)
        .sum()
    }
}

/// What the player holds this frame.
#[derive(Clone, Debug, Default)]
pub struct HostInput {
    /// The game controllers, in player order (the reference's
    /// `self.controllers`).
    pub gamepads: Vec<Gamepad>,
    /// Host keys held, by pygame name.
    pub keys: HashSet<String>,
    /// The mouse in picture pixels (0-255, 0-239), and whether it is over
    /// the picture (the window's focus).
    pub mouse_x: i64,
    pub mouse_y: i64,
    pub mouse_focused: bool,
    /// Its motion since the last frame.
    pub mouse_dx: i64,
    pub mouse_dy: i64,
    pub mouse_left: bool,
    pub mouse_right: bool,
}

impl HostInput {
    fn key(&self, name: &str) -> bool {
        self.keys.contains(name)
    }
}

/// The keyboard as player 1's pad: (button, key names).
pub const KEYS: [(u8, &[&str]); 8] = [
    (UP, &["K_UP"]),
    (DOWN, &["K_DOWN"]),
    (LEFT, &["K_LEFT"]),
    (RIGHT, &["K_RIGHT"]),
    (B, &["K_z", "K_j"]),
    (A, &["K_x", "K_k"]),
    (SELECT, &["K_RSHIFT"]),
    (START, &["K_RETURN", "K_KP_ENTER"]),
];

/// A Super NES pad's extra buttons on the keyboard: A, X, L, R.
pub const SNES_KEYS: [(u16, &str); 4] = [(8, "K_c"), (9, "K_v"), (10, "K_a"), (11, "K_s")];

/// The Power Pad's and Family Trainer's twelve buttons, by row.
pub const MAT_KEYS: [&str; 12] = [
    "K_1", "K_2", "K_3", "K_4", "K_q", "K_w", "K_e", "K_r", "K_a", "K_s", "K_d", "K_f",
];

const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";

fn letters_and_digits(letters: &str, digits: bool) -> Vec<(String, String)> {
    let mut table: Vec<(String, String)> = letters
        .chars()
        .map(|c| (format!("K_{}", c.to_ascii_lowercase()), c.to_string()))
        .collect();
    if digits {
        table.extend(DIGITS.chars().map(|d| (format!("K_{d}"), d.to_string())));
    }
    table
}

fn named(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The host keyboard on the Family BASIC Keyboard, by position on a US layout.
pub fn keyboard_keys() -> Vec<(String, String)> {
    let mut table = letters_and_digits(LETTERS, true);
    table.extend((1..=8).map(|n| (format!("K_F{n}"), format!("F{n}"))));
    table.extend(named(&[
        ("K_MINUS", "-"),
        ("K_EQUALS", "^"),
        ("K_BACKQUOTE", "YEN"),
        ("K_END", "STOP"),
        ("K_LEFTBRACKET", "@"),
        ("K_RIGHTBRACKET", "["),
        ("K_RETURN", "RETURN"),
        ("K_SEMICOLON", ";"),
        ("K_QUOTE", ":"),
        ("K_BACKSLASH", "]"),
        ("K_RALT", "KANA"),
        ("K_COMMA", ","),
        ("K_PERIOD", "."),
        ("K_SLASH", "/"),
        ("K_RCTRL", "_"),
        ("K_LSHIFT", "LSHIFT"),
        ("K_RSHIFT", "RSHIFT"),
        ("K_LCTRL", "CTR"),
        ("K_LALT", "GRPH"),
        ("K_SPACE", "SPACE"),
        ("K_HOME", "CLR"),
        ("K_INSERT", "INS"),
        ("K_DELETE", "DEL"),
        ("K_BACKSPACE", "DEL"),
        ("K_PAUSE", "ESC"),
        ("K_UP", "UP"),
        ("K_DOWN", "DOWN"),
        ("K_LEFT", "LEFT"),
        ("K_RIGHT", "RIGHT"),
    ]));
    table
}

/// The host keyboard on the Subor Keyboard (a PC-style layout).
pub fn subor_host_keys() -> Vec<(String, String)> {
    let mut table = letters_and_digits(LETTERS, true);
    table.extend((1..=12).map(|n| (format!("K_F{n}"), format!("F{n}"))));
    table.extend(
        DIGITS
            .chars()
            .map(|d| (format!("K_KP{d}"), format!("KP{d}"))),
    );
    table.extend(named(&[
        ("K_KP_MINUS", "KP-"),
        ("K_KP_PLUS", "KP+"),
        ("K_KP_MULTIPLY", "KP*"),
        ("K_KP_DIVIDE", "KP/"),
        ("K_KP_PERIOD", "KP."),
        ("K_NUMLOCK", "NUMLOCK"),
        ("K_NUMLOCKCLEAR", "NUMLOCK"),
        ("K_MINUS", "-"),
        ("K_EQUALS", "="),
        ("K_LEFTBRACKET", "["),
        ("K_RIGHTBRACKET", "]"),
        ("K_BACKSLASH", "\\"),
        ("K_SEMICOLON", ";"),
        ("K_QUOTE", "'"),
        ("K_COMMA", ","),
        ("K_PERIOD", "."),
        ("K_SLASH", "/"),
        ("K_BACKQUOTE", "`"),
        ("K_RETURN", "ENTER"),
        ("K_SPACE", "SPACE"),
        ("K_BACKSPACE", "BACKSPACE"),
        ("K_TAB", "TAB"),
        ("K_CAPSLOCK", "CAPSLOCK"),
        ("K_LSHIFT", "LSHIFT"),
        ("K_RSHIFT", "LSHIFT"),
        ("K_LCTRL", "LCTRL"),
        ("K_LALT", "LALT"),
        ("K_INSERT", "INSERT"),
        ("K_DELETE", "DELETE"),
        ("K_HOME", "HOME"),
        ("K_END", "END"),
        ("K_PAGEUP", "PAGEUP"),
        ("K_PAGEDOWN", "PAGEDOWN"),
        ("K_PAUSE", "PAUSE"),
        ("K_UP", "UP"),
        ("K_DOWN", "DOWN"),
        ("K_LEFT", "LEFT"),
        ("K_RIGHT", "RIGHT"),
    ]));
    table
}

/// The Jissen Mahjong panel: tiles A-N on those letters, the calls on 1-5.
pub fn mahjong_keys() -> Vec<(String, String)> {
    let mut table = letters_and_digits("ABCDEFGHIJKLMN", false);
    table.extend(named(&[
        ("K_1", "KAN"),
        ("K_2", "PON"),
        ("K_3", "CHII"),
        ("K_4", "RIICHI"),
        ("K_5", "RON"),
        ("K_RETURN", "START"),
        ("K_RSHIFT", "SELECT"),
    ]));
    table
}

fn held(input: &HostInput, table: &[(String, String)]) -> HashSet<String> {
    table
        .iter()
        .filter(|(key, _)| input.key(key))
        .map(|(_, name)| name.clone())
        .collect()
}

/// The window's power-on device choice: `device` (an NES 2.0 expansion
/// number), else the cartridge's (`expansion`, from nes20db or the header);
/// the NES Vaus becomes the Famicom's on a Famicom; and on a Famicom with
/// the expansion port still empty, a four-player adapter's pads 3 and 4.
/// `palette` is the light guns' (the picture's palette; None: the machine's
/// pally palette from the reference's images, as `plug` reads it).
pub fn plug_for_host(
    board: &mut Board,
    device: u8,
    famicom: bool,
    palette: Option<Vec<[u8; 3]>>,
) -> Plugged {
    let device = if device == VAUS_NES && famicom {
        VAUS_FAMICOM
    } else {
        device
    };
    let plugged = plug(board, device, famicom, palette);
    if board.expansion.is_none() && famicom {
        board.expansion = Some(Box::new(ExpansionPads::default()));
    }
    plugged
}

/// The standard pads plugged in, in player order (`_players`).
pub fn players<'a>(board: &'a mut Board, plugged: &Plugged) -> Vec<&'a mut StandardController> {
    let [first, second] = &mut board.pads;
    let mut players: Vec<&'a mut StandardController> = Vec::new();
    if matches!(plugged, Plugged::FourScore) {
        let a = first.as_any_mut().downcast_mut::<TapPort>();
        let b = second.as_any_mut().downcast_mut::<TapPort>();
        if let (Some(a), Some(b)) = (a, b) {
            players.extend([&mut a.first, &mut b.first, &mut a.second, &mut b.second]);
        }
    } else {
        // The standard pads, then the port-test ports' pads: one pass, as
        // the port test replaces both ports, so the two never mix.
        for pad in [first, second] {
            let pad = pad.as_any_mut();
            if pad.is::<StandardController>() {
                players.extend(pad.downcast_mut::<StandardController>());
            } else if let Some(port) = pad.downcast_mut::<PortTestPort>() {
                players.push(&mut port.pad);
            }
        }
    }
    if let Some(expansion) = board.expansion.as_mut() {
        let expansion = expansion.as_any_mut();
        let expansion: &mut dyn Any = if expansion.is::<Famicom3DSystem>() {
            &mut expansion.downcast_mut::<Famicom3DSystem>().unwrap().through
        } else {
            expansion
        };
        if expansion.is::<ExpansionPads>() {
            let pads = expansion.downcast_mut::<ExpansionPads>().unwrap();
            players.extend(pads.pads.iter_mut());
        } else if expansion.is::<BandaiHyperShot>() {
            let gun = expansion.downcast_mut::<BandaiHyperShot>().unwrap();
            players.insert(players.len().min(1), &mut gun.pad); // its pad is player 1's too
        } else if let Some(dial) = expansion.downcast_mut::<CoconutsPachinko>() {
            players.insert(players.len().min(1), &mut dial.pad);
        }
    }
    players
}

/// The devices `plugged` names, in the reference's order (`_devices`).
fn devices<'a>(board: &'a mut Board, plugged: &'a mut Plugged) -> Vec<&'a mut dyn Any> {
    let [first, second] = &mut board.pads;
    let expansion = board.expansion.as_mut().map(|e| e.as_any_mut());
    match plugged {
        Plugged::Pad(0) => vec![first.as_any_mut()],
        Plugged::Pad(_) => vec![second.as_any_mut()],
        Plugged::Pads => vec![first.as_any_mut(), second.as_any_mut()],
        Plugged::Expansion => expansion.into_iter().collect(),
        Plugged::ExpansionZapper => expansion
            .and_then(|e| e.downcast_mut::<FamicomZapper>())
            .map(|z| vec![&mut z.zapper as &mut dyn Any])
            .unwrap_or_default(),
        Plugged::PadVaus(port) => {
            let pad = if *port == 0 { first } else { second };
            pad.as_any_mut()
                .downcast_mut::<VausNes>()
                .map(|v| vec![&mut v.vaus as &mut dyn Any])
                .unwrap_or_default()
        }
        Plugged::ExpansionVaus => expansion
            .and_then(|e| e.downcast_mut::<VausFamicom>())
            .map(|v| vec![&mut v.vaus as &mut dyn Any])
            .unwrap_or_default(),
        Plugged::VausPair => match expansion.and_then(|e| e.downcast_mut::<VausFamicom>()) {
            Some(VausFamicom { vaus, second }) => {
                let mut list = vec![vaus as &mut dyn Any];
                if let Some(second) = second.as_mut() {
                    list.push(second);
                }
                list
            }
            None => Vec::new(),
        },
        Plugged::SuborPair(port) => {
            let pad = if *port == 0 { first } else { second };
            expansion
                .into_iter()
                .chain(std::iter::once(pad.as_any_mut()))
                .collect()
        }
        Plugged::Rob(rob) => vec![rob as &mut dyn Any],
        // A Four Score, the Data Recorder: nothing to drive from here.
        _ => Vec::new(),
    }
}

/// Is a typing device plugged (the keyboard is not player 1's pad then)?
fn typing(devices: &mut [&mut dyn Any]) -> bool {
    devices.iter_mut().any(|d| {
        d.is::<FamilyBasicKeyboard>() || d.is::<SuborKeyboard>() || d.is::<JissenMahjong>()
    })
}

/// Drive the pads and the plugged device from `input` (`_apply_inputs`).
/// `vaus_low` is the knob's value at the picture's left edge ([`VAUS_LOW`]).
pub fn apply(board: &mut Board, plugged: &mut Plugged, input: &HostInput, vaus_low: i64) {
    let time = board.ppu.time;
    let typing = typing(&mut devices(board, plugged));
    let mut pads = [0u8; 4];
    for (pad, gamepad) in pads.iter_mut().zip(&input.gamepads) {
        *pad = gamepad.buttons();
    }
    if !typing {
        for (button, names) in KEYS {
            if names.iter().any(|name| input.key(name)) {
                pads[0] |= button;
            }
        }
    }
    for (pad, &buttons) in players(board, plugged).into_iter().zip(pads.iter()) {
        pad.buttons = buttons;
    }
    board.microphone = !typing && input.key("K_m");
    let x = input.mouse_x;
    let y = input.mouse_y;
    let aim = (input.mouse_focused && !input.mouse_right).then_some((x, y));
    let (mut zapped, mut vaused, mut snes) = (false, false, 0usize);
    for device in devices(board, plugged) {
        if let Some(zapper) = device.downcast_mut::<Zapper>() {
            if !zapped {
                // One mouse: the first gun.
                zapped = true;
                zapper.aim = aim;
                zapper.set_trigger(input.mouse_left, time);
            }
        } else if let Some(vaus) = device.downcast_mut::<Vaus>() {
            if !vaused {
                vaused = true;
                let knob = vaus_low + x.clamp(0, 255) * 0xA0 / 256;
                vaus.position = knob & 0xFF;
                vaus.fire = input.mouse_left;
            } else {
                // A second Vaus: gamepad 2's left and right turn it, A fires.
                let step = 3 * (i64::from(pads[1] & RIGHT != 0) - i64::from(pads[1] & LEFT != 0));
                vaus.position = (vaus.position + step) & 0xFF;
                vaus.fire = pads[1] & A != 0;
            }
        } else if let Some(mat) = device.downcast_mut::<PowerPad>() {
            mat.buttons = mat_bits(input, 12);
        } else if let Some(mat) = device.downcast_mut::<FamilyTrainer>() {
            mat.buttons = mat_bits(input, 12);
        } else if let Some(keyboard) = device.downcast_mut::<FamilyBasicKeyboard>() {
            keyboard.keys = held(input, &keyboard_keys());
        } else if let Some(keyboard) = device.downcast_mut::<SuborKeyboard>() {
            keyboard.keys = held(input, &subor_host_keys());
        } else if let Some(gun) = device.downcast_mut::<KonamiHyperShot>() {
            gun.buttons = (0..2)
                .map(|p| {
                    (u8::from(pads[p] & A != 0) << (2 * p))
                        | (u8::from(pads[p] & B != 0) << (2 * p + 1))
                })
                .sum();
        } else if let Some(pointer) = mouse_pointer(device) {
            pointer.move_by(input.mouse_dx, input.mouse_dy);
            pointer.left = input.mouse_left;
            pointer.right = input.mouse_right;
        } else if let Some(track) = device.downcast_mut::<HoriTrack>() {
            track.pointer.move_by(input.mouse_dx, input.mouse_dy);
            track.pad.buttons = pads[0];
        } else if let Some(gun) = device.downcast_mut::<BandaiHyperShot>() {
            gun.zapper.aim = aim;
            gun.zapper.set_trigger(input.mouse_left, time);
        } else if let Some(dial) = device.downcast_mut::<CoconutsPachinko>() {
            dial.dial = 3 + x.clamp(0, 255) * 0x70 / 256;
        } else if let Some(bag) = device.downcast_mut::<ExcitingBoxing>() {
            bag.sensors = mat_bits(input, 8) as u8;
        } else if let Some(tap) = device.downcast_mut::<Partytap>() {
            tap.buttons = (0..6)
                .filter(|n| input.key(&format!("K_{}", n + 1)))
                .map(|n| 1u8 << n)
                .sum();
        } else if let Some(panel) = device.downcast_mut::<JissenMahjong>() {
            panel.keys = held(input, &mahjong_keys());
        } else if let Some(tablet) = device.downcast_mut::<OekaKidsTablet>() {
            tablet.x = x.clamp(0, 255) * 240 / 256;
            tablet.y = y.clamp(0, 239) * 256 / 240;
            tablet.touching = input.mouse_left && input.mouse_focused;
            tablet.click = input.mouse_right;
        } else if let Some(pad) = device.downcast_mut::<SnesController>() {
            let mut buttons = pads.get(snes).copied().unwrap_or(0) as u16;
            if let Some(gamepad) = input.gamepads.get(snes) {
                buttons |= gamepad.snes_extra();
            }
            if snes == 0 && !typing {
                for (bit, name) in SNES_KEYS {
                    if input.key(name) {
                        buttons |= 1 << bit;
                    }
                }
            }
            pad.buttons = buttons;
            snes += 1;
        }
    }
}

fn mat_bits(input: &HostInput, count: usize) -> u32 {
    MAT_KEYS[..count]
        .iter()
        .enumerate()
        .filter(|(_, name)| input.key(name))
        .map(|(n, _)| 1u32 << n)
        .sum()
}

/// The pointer of a mouse (Super NES, Subor, Macro Winners, Yuxing).
fn mouse_pointer(device: &mut dyn Any) -> Option<&mut Pointer> {
    if device.is::<SnesMouse>() {
        return device.downcast_mut::<SnesMouse>().map(|m| &mut m.pointer);
    }
    if device.is::<SuborMouse>() {
        return device.downcast_mut::<SuborMouse>().map(|m| &mut m.pointer);
    }
    if device.is::<MacroWinnersMouse>() {
        return device
            .downcast_mut::<MacroWinnersMouse>()
            .map(|m| &mut m.pointer);
    }
    device.downcast_mut::<YuxingMouse>().map(|m| &mut m.pointer)
}

/// After a frame: R.O.B. looks at the picture; its move, if it made one.
pub fn after_frame(board: &mut Board, plugged: &mut Plugged) -> Option<&'static str> {
    match plugged {
        Plugged::Rob(rob) => rob.see(&board.ppu.picture),
        _ => None,
    }
}

/// What the host set, for the lockstep gate (`famicom-trace --host`, the
/// reference tracer's `host_summary`): the players' buttons, the
/// microphone, and each device's host-driven fields.
pub fn describe(board: &mut Board, plugged: &mut Plugged) -> String {
    let pads: Vec<String> = players(board, plugged)
        .iter()
        .map(|pad| format!("{:02X}", pad.buttons))
        .collect();
    let mut out = format!("pads {} mic {}", pads.join(","), u8::from(board.microphone));
    let keys = |keys: &HashSet<String>| {
        let mut keys: Vec<&String> = keys.iter().collect();
        keys.sort();
        if keys.is_empty() {
            "-".to_string()
        } else {
            keys.iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join("+")
        }
    };
    let gun = |zapper: &Zapper| {
        let aim = zapper
            .aim
            .map_or("none".to_string(), |(x, y)| format!("{x},{y}"));
        format!(" zapper {aim} {}", u8::from(zapper.trigger()))
    };
    for device in devices(board, plugged) {
        let text = if let Some(z) = device.downcast_mut::<Zapper>() {
            gun(z)
        } else if let Some(v) = device.downcast_mut::<Vaus>() {
            format!(" vaus {} {}", v.position, u8::from(v.fire))
        } else if let Some(m) = device.downcast_mut::<PowerPad>() {
            format!(" mat {:03X}", m.buttons)
        } else if let Some(m) = device.downcast_mut::<FamilyTrainer>() {
            format!(" mat {:03X}", m.buttons)
        } else if let Some(k) = device.downcast_mut::<FamilyBasicKeyboard>() {
            format!(" keys {}", keys(&k.keys))
        } else if let Some(k) = device.downcast_mut::<SuborKeyboard>() {
            format!(" keys {}", keys(&k.keys))
        } else if let Some(g) = device.downcast_mut::<KonamiHyperShot>() {
            format!(" buttons {:02X}", g.buttons)
        } else if let Some(p) = mouse_pointer(device) {
            let (dx, dy) = p.pending();
            format!(
                " mouse {dx} {dy} {} {}",
                u8::from(p.left),
                u8::from(p.right)
            )
        } else if let Some(t) = device.downcast_mut::<HoriTrack>() {
            let (dx, dy) = t.pointer.pending();
            format!(" track {dx} {dy} {:02X}", t.pad.buttons)
        } else if let Some(g) = device.downcast_mut::<BandaiHyperShot>() {
            gun(&g.zapper)
        } else if let Some(d) = device.downcast_mut::<CoconutsPachinko>() {
            format!(" dial {}", d.dial)
        } else if let Some(b) = device.downcast_mut::<ExcitingBoxing>() {
            format!(" sensors {:02X}", b.sensors)
        } else if let Some(t) = device.downcast_mut::<Partytap>() {
            format!(" buttons {:02X}", t.buttons)
        } else if let Some(p) = device.downcast_mut::<JissenMahjong>() {
            format!(" keys {}", keys(&p.keys))
        } else if let Some(t) = device.downcast_mut::<OekaKidsTablet>() {
            format!(
                " tablet {} {} {} {}",
                t.x,
                t.y,
                u8::from(t.touching),
                u8::from(t.click)
            )
        } else if let Some(p) = device.downcast_mut::<SnesController>() {
            format!(" snes {:03X}", p.buttons)
        } else {
            String::new()
        };
        out.push_str(&text);
    }
    out
}
