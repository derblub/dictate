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

//! Wer die Diagnose führt ([`Identity`] für das Pseudonymisieren, `PLAN-diagnose.md` Antwort 4): Benutzername und
//! Home-Ordner (`getpwuid_r`), Rechnername (`gethostname`), die übrigen Konten von Menschen (`/etc/passwd`, uid ≥ 1000
//! außer `nobody`) und die Machine-ID (`/etc/machine-id`).
//!
//! Der Daemon ruft [`system_identity`] je Sammlung auf (die Werte können sich ändern, etwa der Rechnername). Das Lesen und
//! Zerlegen trennt [`identity_from`] vom System: Tests geben erfundene Texte hinein, nie die echten Dateien von blub.
//! Nichts davon wird geloggt.

use super::pseudo::Identity;
use std::ffi::CStr;
use std::io::Read;
use std::path::Path;

/// Kleinste uid eines Kontos von Menschen (Vorgabe `UID_MIN` in `/etc/login.defs`).
pub const UID_MIN: u32 = 1000;
/// uid von `nobody` (`overflowuid`), kein Mensch.
pub const NOBODY_UID: u32 = 65534;
/// So viel von `/etc/passwd` wird gelesen (Bytes).
const PASSWD_MAX: u64 = 1024 * 1024;

/// Was [`identity_from`] braucht, aus welcher Quelle auch immer.
#[derive(Debug, Clone, Default)]
pub struct IdentityParts {
    /// Name und Home-Ordner des Benutzers, unter dem der Daemon läuft.
    pub user: String,
    pub home: String,
    pub hostname: String,
    /// Inhalt von `/etc/passwd` (leer = nicht lesbar).
    pub passwd: String,
    /// Inhalt von `/etc/machine-id` (leer = nicht lesbar).
    pub machine_id: String,
}

/// Die übrigen Konten von Menschen aus dem Text von `/etc/passwd`: uid ≥ [`UID_MIN`] außer [`NOBODY_UID`], ohne `user`
/// selbst, ohne leere und doppelte Namen, in der Reihenfolge der Datei. Kaputte Zeilen werden übergangen.
pub fn human_accounts(passwd: &str, user: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in passwd.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let (Some(name), Some(_pw), Some(uid)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let Ok(uid) = uid.trim().parse::<u32>() else { continue };
        let name = name.trim();
        // ein Name mit ungültigem UTF-8 (siehe [`passwd_text`]) ist kein brauchbarer Name, die übrigen Zeilen zählen weiter
        if name.contains('\u{fffd}') {
            continue;
        }
        if uid < UID_MIN || uid == NOBODY_UID || name.is_empty() || name == user || name.starts_with(['+', '-']) || out.iter().any(|n| n == name) {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

/// Text von `/etc/passwd` aus den gelesenen Bytes, **zeilenweise** (Review L5): eine Zeile mit ungültigem UTF-8 (etwa ein
/// GECOS-Feld in Latin-1) verwirft nicht die ganze Datei; ungültige Bytes werden `U+FFFD` (für die Konten zählen nur
/// Name und uid, ein Name mit `U+FFFD` wird übergangen). `cut` = die Datei war länger als gelesen: die letzte,
/// möglicherweise abgeschnittene Zeile fällt weg (eine halbe uid wäre eine falsche).
pub fn passwd_text(bytes: &[u8], cut: bool) -> String {
    let mut lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    if cut && !bytes.ends_with(b"\n") {
        lines.pop();
    }
    let mut out = String::with_capacity(bytes.len());
    for line in lines {
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    out
}

/// Die Machine-ID aus dem Text von `/etc/machine-id`: 32 Hex-Zeichen (klein geschrieben), sonst leer (unbekannt; etwa
/// `uninitialized` beim ersten Boot).
pub fn machine_id(text: &str) -> String {
    let id = text.trim().to_ascii_lowercase();
    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        id
    } else {
        String::new()
    }
}

/// Die Identität aus ihren Teilen. `keep` sind Ordner, die die Config nennt (etwa `diagnose.temp_log_dir`): liegen sie unter
/// dem Home, bleiben sie im Bericht lesbar ([`Identity::keep_dirs`], relativ zum Home).
pub fn identity_from(parts: IdentityParts, keep: &[&Path]) -> Identity {
    let home = Path::new(&parts.home);
    let keep_dirs = keep
        .iter()
        .filter(|_| home.is_absolute() && home != Path::new("/"))
        .filter_map(|d| d.strip_prefix(home).ok())
        .filter_map(|rel| rel.to_str())
        .map(|rel| rel.trim_end_matches('/').to_string())
        .filter(|rel| !rel.is_empty())
        .collect();
    Identity {
        accounts: human_accounts(&parts.passwd, &parts.user),
        machine_id: machine_id(&parts.machine_id),
        user: parts.user,
        home: parts.home,
        hostname: parts.hostname,
        keep_dirs,
    }
}

/// Die Identität des Daemon-Prozesses aus dem System (siehe Modulkopf). Was sich nicht lesen lässt, bleibt leer: das
/// Pseudonymisieren ersetzt dann weniger, sammelt aber weiter.
pub fn system_identity(keep: &[&Path]) -> Identity {
    let (user, home) = current_user().unwrap_or_default();
    let parts = IdentityParts {
        user,
        home,
        hostname: hostname().unwrap_or_default(),
        passwd: read_bytes(Path::new("/etc/passwd"), PASSWD_MAX).map(|(b, cut)| passwd_text(&b, cut)).unwrap_or_default(),
        machine_id: read_bytes(Path::new("/etc/machine-id"), 256).map(|(b, _)| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
    };
    identity_from(parts, keep)
}

/// Höchstens `max` Bytes einer Datei und ob sie länger war; `None`, wenn sie sich nicht lesen lässt.
fn read_bytes(path: &Path, max: u64) -> Option<(Vec<u8>, bool)> {
    let f = std::fs::File::open(path).ok()?;
    let mut b = Vec::new();
    f.take(max + 1).read_to_end(&mut b).ok()?;
    let cut = b.len() as u64 > max;
    b.truncate(max as usize);
    Some((b, cut))
}

/// Name und Home-Ordner der eigenen uid aus der Benutzerdatenbank (`getpwuid_r`).
fn current_user() -> Option<(String, String)> {
    let mut buf = vec![0u8; 16 * 1024];
    // SAFETY: `passwd` ist ein POD-Typ, nullinitialisiert gültig; `getpwuid_r` schreibt nur in `pwd` und `buf` (mit seiner
    // Länge) und setzt `result` auf `&pwd` oder null
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    loop {
        // SAFETY: siehe oben; `buf` lebt bis nach dem Lesen der Zeiger
        let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result) };
        if rc == libc::ERANGE && buf.len() < 1024 * 1024 {
            buf.resize(buf.len() * 4, 0);
            continue;
        }
        if rc != 0 || result.is_null() {
            return None;
        }
        break;
    }
    // SAFETY: bei Erfolg zeigen `pw_name` und `pw_dir` auf nullterminierte Texte in `buf`
    let text = |p: *const libc::c_char| if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    Some((text(pwd.pw_name), text(pwd.pw_dir)))
}

/// Der Rechnername (`gethostname`).
fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: schreibt höchstens `buf.len()` Bytes
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Erfundene Benutzerdatenbank
    const PASSWD: &str = "root:x:0:0::/root:/bin/bash\n\
        bin:x:1:1::/:/usr/bin/nologin\n\
        alice:x:1000:1000:Alice:/home/alice:/bin/zsh\n\
        # comment\n\
        bob:x:1001:1001::/home/bob:/bin/bash\n\
        nobody:x:65534:65534:Nobody:/:/usr/bin/nologin\n\
        broken line\n\
        carol:x:notanumber:1::/home/carol:/bin/sh\n\
        dave:x:60001:60001::/home/dave:/bin/sh\n\
        bob:x:1002:1002::/home/bob2:/bin/sh\n\
        +nis:x:1003:1003::/:/bin/sh\n";

    #[test]
    fn accounts_are_humans_other_than_the_user() {
        assert_eq!(human_accounts(PASSWD, "alice"), ["bob", "dave"]);
        assert_eq!(human_accounts(PASSWD, "bob"), ["alice", "dave"]);
        assert!(human_accounts("", "alice").is_empty());
    }

    #[test]
    fn invalid_utf8_in_one_line_keeps_the_other_accounts() {
        // GECOS in Latin-1 (`Müller` als 0xFC) und ein Name mit ungültigem Byte: nur diese eine Zeile zählt nicht
        let mut bytes = b"alice:x:1000:1000:Alice:/home/alice:/bin/zsh\n".to_vec();
        bytes.extend_from_slice(b"bob:x:1001:1001:M\xfcller:/home/bob:/bin/bash\n");
        bytes.extend_from_slice(b"\xe4rger:x:1002:1002::/home/x:/bin/sh\n");
        bytes.extend_from_slice(b"dave:x:1003:1003::/home/dave:/bin/sh\n");
        let text = passwd_text(&bytes, false);
        assert_eq!(human_accounts(&text, "alice"), ["bob", "dave"]);
        // abgeschnitten mitten in der letzten Zeile (auch mitten in einem Zeichen): nur sie fällt weg
        let cut = &bytes[..bytes.len() - 20];
        assert_eq!(human_accounts(&passwd_text(cut, true), "alice"), ["bob"]);
        assert_eq!(human_accounts(&passwd_text(b"eve:x:1004:1004:\xc3", true), "alice"), Vec::<String>::new());
        assert_eq!(human_accounts(&passwd_text(b"eve:x:1004:1004:\xc3\n", true), "alice"), ["eve"]);
    }

    #[test]
    fn machine_id_only_in_its_form() {
        assert_eq!(machine_id("0123456789ABCDEF0123456789abcdef\n"), "0123456789abcdef0123456789abcdef");
        for bad in ["", "uninitialized\n", "0123456789abcdef", "0123456789abcdef0123456789abcdeg"] {
            assert_eq!(machine_id(bad), "", "{bad:?}");
        }
    }

    #[test]
    fn identity_from_parts_keeps_configured_dirs_under_home() {
        let parts = IdentityParts {
            user: "alice".into(),
            home: "/home/alice".into(),
            hostname: "box".into(),
            passwd: PASSWD.into(),
            machine_id: "0123456789abcdef0123456789abcdef".into(),
        };
        let id = identity_from(parts, &[Path::new("/home/alice/temperature-log"), Path::new("/srv/temps"), Path::new("/home/alice")]);
        assert_eq!((id.user.as_str(), id.home.as_str(), id.hostname.as_str()), ("alice", "/home/alice", "box"));
        assert_eq!(id.accounts, ["bob", "dave"]);
        assert_eq!(id.machine_id, "0123456789abcdef0123456789abcdef");
        assert_eq!(id.keep_dirs, ["temperature-log"]);
        // ein Home `/` (Systemdienst) schützt keine Ordner
        let root = identity_from(IdentityParts { home: "/".into(), ..Default::default() }, &[Path::new("/srv/temps")]);
        assert!(root.keep_dirs.is_empty());
    }

    #[test]
    fn system_lookups_do_not_fail() {
        // Nur die Aufrufe (getpwuid_r, gethostname) prüfen: ihre Werte gehören dem Rechner und werden nicht verglichen
        let (user, home) = current_user().expect("eigene uid in der Benutzerdatenbank");
        assert!(!user.is_empty() && home.starts_with('/'));
        assert!(hostname().is_some_and(|h| !h.is_empty()));
    }
}
