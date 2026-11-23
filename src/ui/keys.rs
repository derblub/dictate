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

//! Tastenzustand der Tastaturen über `EVIOCGKEY` (PLAN-wayland.md 3.2 A, Entscheidung 3; Spike S1).
//!
//! **Regel: nur `ioctl`, nie `read`.** Die Gruppe `input` erlaubt, jeden Tastendruck aus `/dev/input/event*` zu
//! lesen. Dieses Modul fragt nur das Bitfeld der gerade gedrückten Tasten ab (`EVIOCGKEY`) und, beim Öffnen, ob ein
//! Gerät die Taste E überhaupt hat (`EVIOCGBIT(EV_KEY)`); der Ereignisstrom bleibt ungelesen. Darum gibt es hier
//! keinen `File` (der `Read` hätte), nur `OwnedFd` aus `libc::open` mit `O_RDONLY`, und genau eine Stelle mit
//! `libc::ioctl`, die nur die Anfragen aus [`ALLOWED_IOCTLS`] zulässt. Die Tests prüfen das am Quelltext und am
//! Verhalten (ein FIFO `event0` behält seine Bytes). Wer hier etwas ändert, hält beide Tests grün.
//!
//! Geöffnet wird nur, solange gehalten wird (`hold::Out::Keys`), und bei jedem Halten neu (Hotplug, Tastatur am
//! Dock). Abgefragt werden alle Geräte mit E, auch weitere Schnittstellen derselben Tastatur und das uinput-Gerät von
//! Espanso (S1): gedrückt = auf irgendeinem Gerät gedrückt.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Quelle des Tastenzustands (echte Geräte oder, nur im Test-Harness, [`super::fake_keys::FileKeys`])
/// `Send`: `dictate-ui` öffnet die Geräte in einem eigenen Thread (das Öffnen kostet je Gerät mehrere ms)
pub trait KeySource: Send {
    /// `Some(true)`: E auf mindestens einem Gerät gedrückt; `Some(false)`: auf keinem; `None`: kein Gerät lesbar
    fn e_down(&mut self) -> Option<bool>;
    /// Wie [`KeySource::e_down`] für die Modifikatoren [`MODIFIERS`] (Ctrl, Shift, Alt, AltGr, Super): vor jeder
    /// Textausgabe wird gewartet, bis keiner mehr gehalten ist (PLAN-wayland.md Abschnitt 4, Regel 2)
    fn mods_down(&mut self) -> Option<bool>;
}

/// evdev-Code der Taste E
pub const KEY_E: usize = 18;
/// evdev-Codes der Modifikatoren: Ctrl links/rechts, Shift links/rechts, Alt links/rechts (AltGr), Super links/rechts
pub const MODIFIERS: [usize; 8] = [29, 97, 42, 54, 56, 100, 125, 126];
const KEY_MAX: usize = 0x2ff;
/// Größe des Bitfelds aller Tasten (`KEY_MAX / 8 + 1`)
const KEY_BYTES: usize = KEY_MAX / 8 + 1;
const EV_KEY: u32 = 0x01;

/// `_IOC(_IOC_READ, 'E', nr, size)` aus `linux/input.h`
const fn ioc_r(nr: u32, size: usize) -> u32 {
    (2 << 30) | ((size as u32) << 16) | ((b'E' as u32) << 8) | nr
}

/// `EVIOCGBIT(EV_KEY, KEY_BYTES)`: welche Tasten das Gerät melden kann
pub const EVIOCGBIT_KEY: u32 = ioc_r(0x20 + EV_KEY, KEY_BYTES);
/// `EVIOCGKEY(KEY_BYTES)`: welche Tasten gerade gedrückt sind
pub const EVIOCGKEY: u32 = ioc_r(0x18, KEY_BYTES);
/// Die einzigen Anfragen an ein Eingabegerät
pub const ALLOWED_IOCTLS: [u32; 2] = [EVIOCGBIT_KEY, EVIOCGKEY];

/// Geöffnete Tastaturen (alle Geräte unter dem Ordner, die E melden können)
pub struct Evdev {
    devs: Vec<OwnedFd>,
}

impl Evdev {
    /// Alle Tastaturen unter `/dev/input`, die laut sysfs E haben (`ui::sysfs`); ist sysfs nicht lesbar, wird jedes
    /// Gerät geöffnet und geprüft
    pub fn open() -> Evdev {
        let only = super::sysfs::devices_with_key(Path::new("/sys/class/input"), KEY_E);
        Evdev::open_in(Path::new("/dev/input"), only.as_deref())
    }

    /// Alle `event*` unter `dir` (mit `only` nur diese Namen), die sich nur lesend öffnen lassen und E melden; die
    /// übrigen werden sofort geschlossen
    pub fn open_in(dir: &Path, only: Option<&[String]>) -> Evdev {
        let mut paths: Vec<_> = std::fs::read_dir(dir).map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
        paths.retain(|p| p.file_name().is_some_and(|n| n.as_bytes().starts_with(b"event")));
        if let Some(names) = only {
            paths.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| names.iter().any(|m| m == n)));
        }
        paths.sort();
        let devs = paths.iter().filter_map(|p| open_device(p)).filter(|fd| query(fd, EVIOCGBIT_KEY).is_some_and(|b| bit(&b, KEY_E))).collect();
        Evdev { devs }
    }

    /// Zahl der geöffneten Geräte
    pub fn len(&self) -> usize {
        self.devs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devs.is_empty()
    }
}

impl Evdev {
    /// `Some(true)`: eine der Tasten `codes` ist auf mindestens einem Gerät gedrückt; `None`: kein Gerät lesbar
    fn any_down(&self, codes: &[usize]) -> Option<bool> {
        let mut answered = false;
        for fd in &self.devs {
            if let Some(keys) = query(fd, EVIOCGKEY) {
                if codes.iter().any(|&c| bit(&keys, c)) {
                    return Some(true);
                }
                answered = true;
            }
        }
        // Alle Abfragen gescheitert (Geräte abgezogen) oder keine Geräte: nichts lesbar
        answered.then_some(false)
    }
}

impl KeySource for Evdev {
    fn e_down(&mut self) -> Option<bool> {
        self.any_down(&[KEY_E])
    }

    fn mods_down(&mut self) -> Option<bool> {
        self.any_down(&MODIFIERS)
    }
}

/// Nur lesend, nicht blockierend, ohne Erben in Kindprozesse und ohne steuerndes Terminal
fn open_device(path: &Path) -> Option<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` ist ein gültiger, nullterminierter Pfad; das Ergebnis ist ein neuer Deskriptor oder -1
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY) };
    // SAFETY: `fd` ist frisch geöffnet und gehört niemandem sonst
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Einzige Stelle mit `ioctl`: Bitfeld der Größe [`KEY_BYTES`] für eine Anfrage aus [`ALLOWED_IOCTLS`]
fn query(fd: &OwnedFd, req: u32) -> Option<[u8; KEY_BYTES]> {
    if !ALLOWED_IOCTLS.contains(&req) {
        return None;
    }
    let mut buf = [0u8; KEY_BYTES];
    // SAFETY: `buf` ist so groß wie die in `req` kodierte Länge (KEY_BYTES); der Kernel schreibt höchstens so viel
    let r = unsafe { libc::ioctl(fd.as_raw_fd(), req as _, buf.as_mut_ptr()) };
    (r >= 0).then_some(buf)
}

fn bit(buf: &[u8], n: usize) -> bool {
    buf.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Quelltext ohne Kommentare und ohne dieses Testmodul
    fn code() -> String {
        let src = include_str!("keys.rs");
        let body = &src[..src.find("#[cfg(test)]").expect("test module")];
        body.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn only_allowed_ioctls_and_no_reads() {
        let code = code();
        for bad in [
            "read(",
            "Read",
            "read_to",
            "read_exact",
            "read_line",
            "read_buf",
            "read_vectored",
            "pread",
            "readv",
            "File",
            "OpenOptions",
            "mmap",
            "O_RDWR",
            "O_WRONLY",
            "EVIOCGRAB",
            "EVIOCSKEYCODE",
            "write",
            "BufRead",
            "recv",
            "splice",
            "sendfile",
            "copy",
        ] {
            assert!(!code.contains(bad), "forbidden in ui/keys.rs: {bad}");
        }
        assert_eq!(code.matches("libc::ioctl").count(), 1, "exactly one ioctl call site");
        assert_eq!(code.matches("libc::open(").count(), 1, "exactly one open call site");
        assert!(code.contains("libc::O_RDONLY | libc::O_NONBLOCK"));
        // Werte wie in linux/input.h (für KEY_MAX 0x2ff, 96 Bytes)
        assert_eq!(EVIOCGKEY, 0x8060_4518);
        assert_eq!(EVIOCGBIT_KEY, 0x8060_4521);
    }

    #[test]
    fn non_devices_are_closed_unread() {
        let dir = std::env::temp_dir().join(format!("dictate-keys-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("event0");
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: gültiger Pfad
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // Leseende offen halten (nicht blockierend), damit der Schreiber nicht wartet; dann Bytes hinein
        let mut reader = std::fs::OpenOptions::new().read(true).custom_flags_nonblock().open(&fifo).unwrap();
        let mut writer = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
        writer.write_all(b"keystrokes").unwrap();
        std::fs::write(dir.join("event1"), b"plain file").unwrap();
        std::fs::create_dir(dir.join("event2")).unwrap();
        std::fs::write(dir.join("mouse0"), b"ignored").unwrap();

        let mut ev = Evdev::open_in(&dir, None);
        assert!(ev.is_empty(), "no input device in a temp dir");
        assert_eq!(ev.e_down(), None);
        drop(ev);
        // mit Namensliste nur diese (hier keins, das ein Gerät ist)
        assert!(Evdev::open_in(&dir, Some(&["event0".to_string()])).is_empty());
        drop(writer);
        // Alle Bytes noch da: das Modul hat nicht gelesen
        let mut got = String::new();
        reader.read_to_string(&mut got).ok();
        assert_eq!(got, "keystrokes");
        assert_eq!(std::fs::read(dir.join("event1")).unwrap(), b"plain file");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    trait NonBlock {
        fn custom_flags_nonblock(&mut self) -> &mut Self;
    }
    impl NonBlock for std::fs::OpenOptions {
        fn custom_flags_nonblock(&mut self) -> &mut Self {
            use std::os::unix::fs::OpenOptionsExt;
            self.custom_flags(libc::O_NONBLOCK)
        }
    }

    #[test]
    fn bits() {
        let mut b = [0u8; KEY_BYTES];
        assert!(!bit(&b, KEY_E));
        b[KEY_E / 8] |= 1 << (KEY_E % 8);
        assert!(bit(&b, KEY_E));
        assert!(!bit(&b, KEY_E + 1));
        assert!(!bit(&b, 10_000));
    }
}
