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

//! Spike S2 (W0, PLAN-wayland.md Abschnitt 5): eigene virtuelle Tastatur (T2) und Eingabemethode (T3) unter niri.
//!
//! `vk kbd`  – `zwp_virtual_keyboard_v1` mit eigener, erzeugter Belegung (eine Taste je Zeichen wie wtype, dazu
//!             Modifikatortasten). Befehle über stdin:
//!     `type TEXT…`        jedes Zeichen drücken und loslassen (Rest der Zeile ist der Text, `\n` = Return)
//!     `chord MOD+…+KEY`   z. B. `chord alt+shift+q` (Modifikatoren über Tasten und `modifiers`)
//!     `down KEY` / `up KEY`   eine Taste halten bzw. loslassen (KEY = Zeichen oder Name wie `alt`)
//!     `destroy`           Gerät mit `destroy` zerstören (Prozess lebt weiter)
//!     `exit`              Prozess sofort beenden, ohne Loslassen und ohne `destroy` (wie SIGKILL)
//!     `sleep MS`
//! `vk im`   – `zwp_input_method_v2`; meldet `activate`, `deactivate`, `done`, `unavailable`, `content_type`,
//!             `surrounding`. Befehle: `commit TEXT…` (commit_string + commit(serial)), `preedit TEXT…`, `destroy`.
//!
//! Ausgabe: eine Zeile je Beobachtung mit Millisekunden seit Start, `ok` nach jedem Befehl.
//! Isolation: startet nur, wenn XDG_RUNTIME_DIR unter /home/data/stt-test/wl-run/ liegt (Harness).

use std::io::Write;
use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// `VK_KEYSYMS=unicode` schaltet auf U+-Keysyms für alle Nicht-ASCII-Zeichen (erste Messung).
static LEGACY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn say(line: &str) {
    let ms = START.get_or_init(Instant::now).elapsed().as_millis();
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{ms:>7} {line}").and_then(|()| out.flush());
}

fn guard() {
    const BASE: &str = "/home/data/stt-test/wl-run/";
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let wl = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !rt.starts_with(BASE) || (wl.starts_with('/') && !wl.starts_with(BASE)) {
        eprintln!("vk: refusing to run outside the test runtime dir {BASE}");
        std::process::exit(70);
    }
}

#[derive(Default)]
struct ImState {
    serial: u32,
    active: bool,
}

struct State {
    im: Arc<Mutex<ImState>>,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: ZwpInputMethodManagerV2);

impl Dispatch<ZwpInputMethodV2, ()> for State {
    fn event(state: &mut Self, _: &ZwpInputMethodV2, event: zwp_input_method_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use zwp_input_method_v2::Event;
        let mut im = state.im.lock().unwrap();
        match event {
            Event::Activate => {
                im.active = true;
                say("activate");
            }
            Event::Deactivate => {
                im.active = false;
                say("deactivate");
            }
            Event::SurroundingText { text, cursor, anchor } => say(&format!("surrounding {text:?} {cursor} {anchor}")),
            Event::ContentType { hint, purpose } => say(&format!("content_type {hint:?} {purpose:?}")),
            Event::TextChangeCause { cause } => say(&format!("change_cause {cause:?}")),
            Event::Done => {
                im.serial += 1;
                say(&format!("done serial={}", im.serial));
            }
            Event::Unavailable => say("unavailable"),
            _ => say("other event"),
        }
    }
}

/// Modifikator: Keysym-Name und Bit in der Standard-Maske (reale Modifikatoren Shift, Control, Mod1, Mod4).
fn modifier(name: &str) -> Option<(&'static str, u32)> {
    Some(match name {
        "shift" => ("Shift_L", 1),
        "ctrl" => ("Control_L", 4),
        "alt" => ("Alt_L", 8),
        "super" => ("Super_L", 64),
        _ => return None,
    })
}

fn keysym_of(c: char) -> String {
    match c {
        '\n' => "Return".into(),
        '\t' => "Tab".into(),
        ' ' => "space".into(),
        c if c.is_ascii_alphanumeric() => c.to_string(),
        // Latin-1: klassische Keysyms (Wert = Codepunkt) wie xkb_utf32_to_keysym/wtype, nicht U+-Keysyms; Chromium
        // verlor mit `U003A`/`U00DF` Zeichen (gemessen, docs/spikes/wayland-vkbd.md).
        c if LEGACY.load(std::sync::atomic::Ordering::Relaxed)
            && matches!(c as u32, 0x20..=0x7e | 0xa0..=0xff) =>
        {
            format!("0x{:x}", c as u32)
        }
        c => format!("U{:04X}", c as u32),
    }
}

/// Belegung wie wtype: jede Taste trägt genau ein Keysym. Modifikatoren immer vorn (feste Keycodes).
struct Keymap {
    syms: Vec<String>,
}

impl Keymap {
    fn new() -> Self {
        Self { syms: ["Shift_L", "Control_L", "Alt_L", "Super_L"].iter().map(|s| s.to_string()).collect() }
    }
    /// evdev-Code (xkb-Keycode − 8) eines Keysyms; `true`, wenn die Belegung neu hochgeladen werden muss.
    fn code(&mut self, sym: &str) -> (u32, bool) {
        if let Some(i) = self.syms.iter().position(|s| s == sym) {
            return (i as u32 + 1, false);
        }
        self.syms.push(sym.to_string());
        (self.syms.len() as u32, true)
    }
    fn text(&self) -> String {
        let mut s = String::from("xkb_keymap {\nxkb_keycodes \"(unnamed)\" {\nminimum = 8;\nmaximum = 255;\n");
        for i in 0..self.syms.len() {
            s += &format!("<K{}> = {};\n", i + 1, i + 9);
        }
        s += "};\nxkb_types \"(unnamed)\" { include \"complete\" };\nxkb_compat \"(unnamed)\" { include \"complete\" };\n";
        s += "xkb_symbols \"(unnamed)\" {\n";
        for (i, sym) in self.syms.iter().enumerate() {
            s += &format!("key <K{}> {{[ {} ]}};\n", i + 1, sym);
        }
        s += "modifier_map Shift { <K1> };\nmodifier_map Control { <K2> };\nmodifier_map Mod1 { <K3> };\nmodifier_map Mod4 { <K4> };\n";
        s += "};\n};\n";
        s
    }
}

struct Kbd {
    vk: ZwpVirtualKeyboardV1,
    map: Keymap,
    mods: u32,
    start: Instant,
    tmp: std::path::PathBuf,
}

impl Kbd {
    fn time(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }
    fn upload(&mut self) -> Result<(), String> {
        let mut text = self.map.text().into_bytes();
        text.push(0);
        let path = self.tmp.join(format!("vk-{}.xkb", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("keymap file: {e}"))?;
        f.write_all(&text).map_err(|e| format!("keymap file: {e}"))?;
        let _ = std::fs::remove_file(&path);
        self.vk.keymap(1, f.as_fd(), text.len() as u32);
        say(&format!("keymap uploaded ({} keys)", self.map.syms.len()));
        Ok(())
    }
    fn code(&mut self, sym: &str) -> Result<u32, String> {
        let (code, new) = self.map.code(sym);
        if new {
            self.upload()?;
        }
        Ok(code)
    }
    fn key(&mut self, code: u32, down: bool) {
        self.vk.key(self.time(), code, u32::from(down));
    }
    fn sym_for(name: &str) -> String {
        if let Some((sym, _)) = modifier(name) {
            return sym.into();
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => keysym_of(c),
            _ => name.to_string(),
        }
    }
    fn hold(&mut self, name: &str, down: bool) -> Result<(), String> {
        let sym = Self::sym_for(name);
        let code = self.code(&sym)?;
        self.key(code, down);
        if let Some((_, bit)) = modifier(name) {
            self.mods = if down { self.mods | bit } else { self.mods & !bit };
            self.vk.modifiers(self.mods, 0, 0, 0);
        }
        Ok(())
    }
}

fn main() {
    guard();
    START.get_or_init(Instant::now);
    if std::env::var("VK_KEYSYMS").as_deref() == Ok("unicode") {
        LEGACY.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    let mode = std::env::args().nth(1).unwrap_or_default();
    if let Err(e) = run(&mode) {
        say(&format!("error {e}"));
        std::process::exit(1);
    }
}

fn run(mode: &str) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
    let im_state = Arc::new(Mutex::new(ImState::default()));
    let mut state = State { im: im_state.clone() };

    let tmp = std::env::temp_dir();
    let mut kbd = None;
    let mut im = None;
    match mode {
        "kbd" => {
            let mgr: ZwpVirtualKeyboardManagerV1 =
                globals.bind(&qh, 1..=1, ()).map_err(|e| format!("virtual keyboard manager: {e}"))?;
            let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
            let mut k = Kbd { vk, map: Keymap::new(), mods: 0, start: Instant::now(), tmp };
            k.upload()?;
            kbd = Some(k);
        }
        "im" => {
            let mgr: ZwpInputMethodManagerV2 =
                globals.bind(&qh, 1..=1, ()).map_err(|e| format!("input method manager: {e}"))?;
            im = Some(mgr.get_input_method(&seat, &qh, ()));
        }
        _ => return Err("usage: vk kbd|im".into()),
    }
    queue.roundtrip(&mut state).map_err(|e| format!("roundtrip: {e}"))?;
    say("ready");

    // stdin in einem eigenen Thread; Anfragen gehen vom Hauptthread aus, Ereignisse werden zwischen Befehlen verteilt.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    loop {
        // Ereignisse ohne Blockieren abholen.
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut pfd = [rustix_poll(fd)];
            if poll(&mut pfd, 10) > 0 {
                let _ = guard.read();
            }
        }
        queue.dispatch_pending(&mut state).map_err(|e| format!("dispatch: {e}"))?;
        let line = match rx.try_recv() {
            Ok(l) => l,
            Err(std::sync::mpsc::TryRecvError::Empty) => continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        };
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let rest = rest.replace("\\n", "\n");
        let res: Result<(), String> = match (cmd, kbd.as_mut(), im.as_ref()) {
            ("sleep", _, _) => {
                std::thread::sleep(Duration::from_millis(rest.trim().parse().unwrap_or(0)));
                Ok(())
            }
            ("type", Some(k), _) => rest.chars().try_for_each(|c| {
                let sym = keysym_of(c);
                let code = k.code(&sym)?;
                k.key(code, true);
                k.key(code, false);
                conn.flush().map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(2));
                Ok(())
            }),
            // Alle Zeichen des Textes vorab in die Belegung, ein einziges Hochladen vor dem Tippen.
            ("prepare", Some(k), _) => {
                let mut new = false;
                for c in rest.chars() {
                    new |= k.map.code(&keysym_of(c)).1;
                }
                if new { k.upload() } else { Ok(()) }
            }
            ("chord", Some(k), _) => {
                let parts: Vec<&str> = rest.trim().split('+').collect();
                let (last, mods) = parts.split_last().ok_or("empty chord")?;
                mods.iter().try_for_each(|m| k.hold(m, true))?;
                k.hold(last, true)?;
                k.hold(last, false)?;
                mods.iter().rev().try_for_each(|m| k.hold(m, false))
            }
            ("down", Some(k), _) => k.hold(rest.trim(), true),
            ("up", Some(k), _) => k.hold(rest.trim(), false),
            ("destroy", Some(k), _) => {
                k.vk.destroy();
                Ok(())
            }
            ("exit", _, _) => {
                // Kein Loslassen, kein destroy: die Verbindung endet mit dem Prozess.
                std::process::exit(0);
            }
            ("commit", _, Some(m)) => {
                let s = im_state.lock().unwrap();
                m.commit_string(rest.clone());
                m.commit(s.serial);
                say(&format!("committed serial={} active={}", s.serial, s.active));
                Ok(())
            }
            ("preedit", _, Some(m)) => {
                let s = im_state.lock().unwrap();
                m.set_preedit_string(rest.clone(), 0, rest.len() as i32);
                m.commit(s.serial);
                Ok(())
            }
            ("destroy", _, Some(m)) => {
                m.destroy();
                Ok(())
            }
            _ => Err(format!("unknown command {line}")),
        };
        conn.flush().map_err(|e| format!("flush: {e}"))?;
        let _ = queue.roundtrip(&mut state);
        say(&match res {
            Ok(()) => format!("ok {cmd}"),
            Err(e) => format!("error {cmd}: {e}"),
        });
    }
    Ok(())
}

// Minimales poll(2) ohne zusätzliche Crate.
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
fn rustix_poll(fd: std::os::fd::BorrowedFd<'_>) -> PollFd {
    use std::os::fd::AsRawFd;
    PollFd { fd: fd.as_raw_fd(), events: 1, revents: 0 }
}
unsafe extern "C" {
    #[link_name = "poll"]
    fn c_poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
}
fn poll(fds: &mut [PollFd], timeout_ms: i32) -> i32 {
    // SAFETY: fds ist ein gültiges, veränderbares Feld von PollFd mit passender Länge.
    unsafe { c_poll(fds.as_mut_ptr(), fds.len() as u64, timeout_ms) }
}
