// Copyright (C) 2026 Daniel Kurdoghlian
// Pushing Pixels, Vienna <https://pushingpixels.at>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
// for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, see <https://www.gnu.org/licenses>.
//
// Additional permission under GNU GPL version 3 section 7
//
// If you modify this Program, or any covered work, by linking or combining
// it with NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as
// libcudart, libcublas and libcublasLt, and the CUDA driver library libcuda
// of the NVIDIA driver) (or a modified version of that library), containing
// parts covered by the terms of the NVIDIA CUDA Toolkit End User License
// Agreement or the NVIDIA Driver License Agreement, the licensors of this
// Program grant you additional permission to convey the resulting work.

//! Kopie von niri-coach `examples/inject_input.rs` (gleicher Autor) für die dictate-Spikes W0, erweitert um
//! `down:KEY`/`up:KEY` (Taste halten, z. B. gehaltenes Mod), die neutrale Taste `f24` und die Isolationsprüfung.
//!
//! Keyboard and pointer injector for nested niri tests (tests/nested, SP-1/SP-2/SP-3).
//!
//! `wtype` cannot drive a nested niri: it uploads a keymap with arbitrary keycodes and sends modifiers only as
//! state, while niri's winit backend interprets raw evdev keycodes with its own xkb keymap and tracks modifiers from
//! key events. This tool creates a `zwp_virtual_keyboard_v1` on the host compositor (headless sway) with the given
//! standard keymap and presses real evdev keycodes, so niri sees exactly what a physical keyboard would send.
//!
//! The pointer is a `zwlr_virtual_pointer_v1`: headless sway has no input devices, so without it the seat has no
//! pointer capability and the nested niri never gets a `wl_pointer` (`swaymsg seat … cursor` alone is not enough).
//!
//! Usage: `inject_input --keymap FILE --extent W,H [STEP...]` where STEP is a chord `alt+h`, `ctrl+shift+left`,
//! `escape`, `text:hello` (lowercase letters, digits, `.` and `-`; no spaces inside one step, use `space`), `move:X,Y`
//! (absolute, host-compositor pixels within the extent), `press`/`release` (left button), `scroll:N` (N notches of the
//! mouse wheel, positive = down, negative = up), or `sleep:MS`. Keys are evdev codes at their US positions: `text:`
//! only types what it says when the nested niri uses the `us` layout (tests/gui sets it; `de` swaps y/z and moves
//! `-`). Keymap:
//! `xkbcli compile-keymap --layout us`. Without steps it reads lines of steps from stdin and answers `ok` or
//! `error: …` per line, keeping one virtual keyboard alive: a client only binds `wl_keyboard` after the seat gains the
//! keyboard capability, so keys sent right after creating the device are lost (observed with nested niri).

mod guard;

use std::io::Write;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);

const BTN_LEFT: u32 = 0x110;

/// Modifier key: evdev code and its bit in the standard xkb modifier mask.
fn modifier(name: &str) -> Option<(u32, u32)> {
    Some(match name {
        "shift" => (42, 1),
        "ctrl" => (29, 4),
        "alt" => (56, 8),
        "super" | "mod4" => (125, 64),
        _ => return None,
    })
}

/// evdev keycode of a key name (input-event-codes.h).
fn key(name: &str) -> Option<u32> {
    const LETTERS: &[(char, u32)] = &[
        ('q', 16),
        ('w', 17),
        ('e', 18),
        ('r', 19),
        ('t', 20),
        ('y', 21),
        ('u', 22),
        ('i', 23),
        ('o', 24),
        ('p', 25),
        ('a', 30),
        ('s', 31),
        ('d', 32),
        ('f', 33),
        ('g', 34),
        ('h', 35),
        ('j', 36),
        ('k', 37),
        ('l', 38),
        ('z', 44),
        ('x', 45),
        ('c', 46),
        ('v', 47),
        ('b', 48),
        ('n', 49),
        ('m', 50),
    ];
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if let Some(&(_, code)) = LETTERS.iter().find(|(l, _)| *l == c) {
            return Some(code);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(if d == 0 { 11 } else { 1 + d });
        }
        match c {
            '-' => return Some(12),
            '.' => return Some(52),
            _ => {}
        }
    }
    // A lone modifier tap (`shift`) is a plain key press; only chords update the modifier state.
    if let Some((code, _)) = modifier(name) {
        return Some(code);
    }
    Some(match name {
        "escape" => 1,
        "minus" => 12,
        "equal" => 13,
        "backspace" => 14,
        "tab" => 15,
        "bracketleft" => 26,
        "bracketright" => 27,
        "return" => 28,
        "space" => 57,
        "up" => 103,
        "pageup" => 104,
        "left" => 105,
        "right" => 106,
        "down" => 108,
        "pagedown" => 109,
        "dot" => 52,
        "f24" => 194,
        _ => return None,
    })
}

struct Kbd {
    vk: ZwpVirtualKeyboardV1,
    vp: ZwlrVirtualPointerV1,
    extent: (u32, u32),
    start: Instant,
    mods: u32,
}

impl Kbd {
    fn time(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }
    fn press(&mut self, code: u32, down: bool) {
        self.vk.key(self.time(), code, u32::from(down));
    }
    fn set_mods(&mut self, mask: u32) {
        self.mods = mask;
        self.vk.modifiers(mask, 0, 0, 0);
    }
    fn move_to(&mut self, spec: &str) -> Result<(), String> {
        let (x, y) = spec.split_once(',').ok_or(format!("bad position {spec}"))?;
        let x: u32 = x.parse().map_err(|_| format!("bad x {x}"))?;
        let y: u32 = y.parse().map_err(|_| format!("bad y {y}"))?;
        self.vp.motion_absolute(self.time(), x, y, self.extent.0, self.extent.1);
        self.vp.frame();
        Ok(())
    }
    fn button(&mut self, down: bool) {
        use wayland_client::protocol::wl_pointer::ButtonState;
        let state = if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        self.vp.button(self.time(), BTN_LEFT, state);
        self.vp.frame();
    }
    /// N notches of a mouse wheel (positive = down), each its own frame as from a real wheel: source, discrete step
    /// and the usual 15 units per notch (libinput, `wl_pointer.axis`).
    fn scroll(&mut self, spec: &str) -> Result<(), String> {
        use wayland_client::protocol::wl_pointer::{Axis, AxisSource};
        let n: i32 = spec.parse().map_err(|_| format!("bad scroll {spec}"))?;
        let step = n.signum();
        for _ in 0..n.unsigned_abs() {
            let t = self.time();
            self.vp.axis_source(AxisSource::Wheel);
            self.vp.axis_discrete(t, Axis::VerticalScroll, 15.0 * f64::from(step), step);
            self.vp.frame();
        }
        Ok(())
    }
    fn chord(&mut self, spec: &str) -> Result<(), String> {
        let parts: Vec<&str> = spec.split('+').collect();
        let (last, mods) = parts.split_last().ok_or("empty chord")?;
        let mods: Vec<(u32, u32)> = mods
            .iter()
            .map(|m| modifier(m).ok_or(format!("unknown modifier {m}")))
            .collect::<Result<_, _>>()?;
        let code = key(last).ok_or(format!("unknown key {last}"))?;
        for &(c, bit) in &mods {
            self.press(c, true);
            self.set_mods(self.mods | bit);
        }
        self.press(code, true);
        self.press(code, false);
        for &(c, bit) in mods.iter().rev() {
            self.press(c, false);
            self.set_mods(self.mods & !bit);
        }
        Ok(())
    }
}

fn run_step(kbd: &mut Kbd, queue: &mut wayland_client::EventQueue<State>, step: &str) -> Result<(), String> {
    if let Some(ms) = step.strip_prefix("sleep:") {
        std::thread::sleep(Duration::from_millis(
            ms.parse().map_err(|_| format!("bad sleep {ms}"))?,
        ));
        return Ok(());
    }
    if let Some(name) = step.strip_prefix("down:").or_else(|| step.strip_prefix("up:")) {
        let down = step.starts_with("down:");
        let code = key(name).ok_or(format!("unknown key {name}"))?;
        kbd.press(code, down);
        if let Some((_, bit)) = modifier(name) {
            kbd.set_mods(if down { kbd.mods | bit } else { kbd.mods & !bit });
        }
    } else if let Some(pos) = step.strip_prefix("move:") {
        kbd.move_to(pos)?;
    } else if let Some(n) = step.strip_prefix("scroll:") {
        kbd.scroll(n)?;
    } else if step == "press" || step == "release" {
        kbd.button(step == "press");
    } else if let Some(text) = step.strip_prefix("text:") {
        for c in text.chars() {
            kbd.chord(&c.to_string())?;
        }
    } else {
        kbd.chord(step)?;
    }
    queue.roundtrip(&mut State).map_err(|e| format!("roundtrip: {e}"))?;
    std::thread::sleep(Duration::from_millis(20));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("inject_input: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    guard()?;
    let mut args = std::env::args().skip(1);
    let usage = "usage: inject_input --keymap FILE --extent W,H [STEP...]";
    if args.next().as_deref() != Some("--keymap") {
        return Err(usage.into());
    }
    let keymap_path = args.next().ok_or("missing keymap file")?;
    if args.next().as_deref() != Some("--extent") {
        return Err(usage.into());
    }
    let extent = args.next().ok_or(usage)?;
    let extent = extent
        .split_once(',')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or(format!("bad extent {extent}"))?;
    let steps: Vec<String> = args.collect();

    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
    let mgr: ZwpVirtualKeyboardManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("virtual keyboard manager: {e}"))?;
    let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
    let pmgr: ZwlrVirtualPointerManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .map_err(|e| format!("virtual pointer manager: {e}"))?;
    let vp = pmgr.create_virtual_pointer(Some(&seat), &qh, ());

    // The protocol wants a NUL-terminated keymap in a file descriptor.
    let mut text = std::fs::read(&keymap_path).map_err(|e| format!("{keymap_path}: {e}"))?;
    text.push(0);
    let tmp_path = std::env::temp_dir().join(format!("inject-keys-{}.xkb", std::process::id()));
    // Readable too: the compositor mmaps it read-only.
    let mut tmp = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| format!("keymap temp file: {e}"))?;
    tmp.write_all(&text).map_err(|e| format!("keymap temp file: {e}"))?;
    let _ = std::fs::remove_file(&tmp_path);
    let size = u32::try_from(text.len()).map_err(|_| "keymap too large")?;
    vk.keymap(1, tmp.as_fd(), size); // 1 = WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
    queue.roundtrip(&mut State).map_err(|e| format!("roundtrip: {e}"))?;

    let mut kbd = Kbd {
        vk,
        vp,
        extent,
        start: Instant::now(),
        mods: 0,
    };
    // Give clients time to bind wl_keyboard after the seat gained the keyboard capability.
    std::thread::sleep(Duration::from_millis(300));
    if steps.is_empty() {
        let mut out = std::io::stdout();
        for line in std::io::stdin().lines() {
            let line = line.map_err(|e| format!("stdin: {e}"))?;
            let res = line
                .split_whitespace()
                .try_for_each(|step| run_step(&mut kbd, &mut queue, step));
            let reply = match res {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("error: {e}"),
            };
            writeln!(out, "{reply}")
                .and_then(|()| out.flush())
                .map_err(|e| format!("stdout: {e}"))?;
        }
    } else {
        for step in &steps {
            run_step(&mut kbd, &mut queue, step)?;
        }
    }
    kbd.vk.destroy();
    kbd.vp.destroy();
    queue.roundtrip(&mut State).map_err(|e| format!("roundtrip: {e}"))?;
    Ok(())
}

/// Isolation: nur im Laufzeitordner des Harness, nie gegen die echte Sitzung (`guard.rs`).
fn guard() -> Result<(), String> {
    guard::from_env()
}
