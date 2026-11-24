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

//! Markierung (PRIMARY) lesen für die Textaktionen (PLAN-wayland.md 3.5, Regel 4.1): im Ausgabe-Thread über dessen
//! Data-Control-Gerät, ohne Fokus. Grenzen aus `src/actions/selection.rs` ([`MAX_BYTES`],
//! Frist [`TIMEOUT`]), Kodierungs-Logik wie früher `Xclip::read_primary` unter X11: UTF-8 streng (ungültig = Fehler), nur
//! `STRING` gilt als ISO 8859-1. Ohne Markierung oder ohne Texttyp ist das Ergebnis leer (der Daemon antwortet dann
//! „Nothing selected“).
//!
//! **Nur PRIMARY.** Data-Control macht unter Wayland auch CLIPBOARD jederzeit lesbar; die Grenze liegt allein im Code:
//! CLIPBOARD liest nur `clip.rs` (Sichern beim Einfügen, Regel 4.6, und seit W5 die Palette-Absicht
//! `explain_clipboard`, `clip::read_clipboard_text`). Der Test `sources_stay_apart` prüft am Quelltext, dass jede Quelle
//! nur in ihrem Modul gelesen wird und CLIPBOARD als Text nur auf diese Absicht.
//!
//! Trägt die Markierung den Hinweis `x-kde-passwordManagerHint: secret`, wird sie nicht gelesen (wie die
//! Zwischenablage nach Entscheidung 10). Inhalt nie loggen, nur Längen.

use crate::actions::selection::{MAX_BYTES, TIMEOUT};
use crate::actions::store::{wipe_bytes, SecretText};
use crate::clip::read_offer;
use crate::config::ACTIONS_MAX_CHARS;
use crate::wl::{Wl, HINT};
use std::time::Instant;
use tracing::debug;

/// Texttypen in der Reihenfolge der Wahl; `true` = ISO 8859-1 (nur `STRING`, ICCCM). Auch für CLIPBOARD
/// (`clip::read_clipboard_text`, Absicht `explain_clipboard`)
pub const TEXT_TYPES: [(&str, bool); 5] =
    [("text/plain;charset=utf-8", false), ("UTF8_STRING", false), ("text/plain", false), ("TEXT", false), ("STRING", true)];

/// Markierung lesen; leer, wenn nichts markiert ist. Fehlertexte sind die Meldungen fürs Panel (Englisch)
pub fn read_primary(wl: &mut Wl) -> Result<SecretText, String> {
    // Ausstehende Ereignisse zuerst (eine eben gemachte Markierung)
    wl.roundtrip().map_err(|e| format!("Cannot read the selection: {e}"))?;
    if wl.device.is_none() {
        return Err("Cannot read the selection: the compositor offers no data control".into());
    }
    if !wl.st.primary_known {
        return Err("Cannot read the selection: the compositor reports no primary selection".into());
    }
    let seq = wl.st.primary_seq;
    let Some(sel) = wl.st.primary.clone() else {
        debug!("primary: empty");
        return Ok(SecretText::new(String::new()));
    };
    let deadline = Instant::now() + TIMEOUT;
    if sel.types.iter().any(|t| t == HINT) {
        let v = read_offer(wl, &sel.offer, HINT, 64, deadline).map_err(|_| "Cannot read the selection: its password hint is unreadable".to_string())?;
        if v.0.trim_ascii() == b"secret" {
            return Err("The selection comes from a password manager, so it is not read".into());
        }
    }
    let Some((mime, latin1)) = TEXT_TYPES.iter().copied().find(|(m, _)| sel.types.iter().any(|t| t == m)) else {
        debug!("primary: no text type among {} types", sel.types.len());
        return Ok(SecretText::new(String::new()));
    };
    let mut data = read_offer(wl, &sel.offer, mime, MAX_BYTES, deadline).map_err(|e| match e {
        "clipboard too large" => format!("Text too long: more than {ACTIONS_MAX_CHARS} characters"),
        "clipboard owner too slow" => format!("Cannot read the selection: no answer within {} s", TIMEOUT.as_secs()),
        other => format!("Cannot read the selection: {}", other.replace("clipboard", "selection")),
    })?;
    // Wechsel mitten im Lesen (das alte Angebot ist dann zerstört): nie einen halben Inhalt übergeben
    if wl.st.primary_seq != seq {
        return Err("The selection changed while reading it. Try again.".into());
    }
    let text = decode(std::mem::take(&mut data.0), latin1)?;
    debug!("primary: {} bytes as {mime}", text.as_str().len());
    Ok(text)
}

/// Bytes als Text: UTF-8 streng, `latin1` als ISO 8859-1; der Puffer wird bei Fehlern überschrieben. Auch für
/// CLIPBOARD (`clip::read_clipboard_text`)
pub fn decode(bytes: Vec<u8>, latin1: bool) -> Result<SecretText, String> {
    if latin1 {
        let mut b = bytes;
        let s: String = b.iter().map(|&c| c as char).collect();
        wipe_bytes(&mut b);
        return Ok(SecretText::new(s));
    }
    match String::from_utf8(bytes) {
        Ok(s) => Ok(SecretText::new(s)),
        Err(e) => {
            let mut b = e.into_bytes();
            wipe_bytes(&mut b);
            Err("The selection is not valid UTF-8 text".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(decode("Grüße".as_bytes().to_vec(), false).unwrap().as_str(), "Grüße");
        assert!(decode(vec![0x47, 0xfc], false).is_err());
        assert_eq!(decode(vec![0x47, 0xfc], true).unwrap().as_str(), "Gü");
    }

    /// Quelltext ohne Kommentare und ohne Testmodul. Geschnitten wird nur am Testmodul (`#[cfg(test)]` vor
    /// `mod tests`), nicht an einem anderen `#[cfg(test)]` (seit W5 etwa in `niri::socket`): sonst bliebe der Rest der
    /// Datei ungeprüft. `//` beginnt nur außerhalb von Strings einen Kommentar (W5 Durchsicht H1: sonst verschwand
    /// `let u = "https://x"; Command::new(p)` hinter der Adresse)
    fn code(src: &str) -> String {
        let body = test_module(src).map_or(src, |i| &src[..i]);
        strip_comments(body)
    }

    /// Entfernt Zeilenkommentare außerhalb von String-Literalen (gewöhnliche mit Escapes, rohe `r#"…"#`, Zeichen `'"'`)
    fn strip_comments(src: &str) -> String {
        let b = src.as_bytes();
        let mut out = String::with_capacity(src.len());
        let mut i = 0;
        let mut start = 0;
        while i < b.len() {
            match b[i] {
                b'/' if b.get(i + 1) == Some(&b'/') => {
                    out.push_str(&src[start..i]);
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                    start = i;
                    continue;
                }
                b'\'' if b.get(i + 2) == Some(&b'\'') || (b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'')) => {
                    // Zeichenliteral wie '"' oder '\"'
                    i += if b[i + 1] == b'\\' { 4 } else { 3 };
                    continue;
                }
                b'r' if matches!(b.get(i + 1), Some(b'"' | b'#')) && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) => {
                    let mut j = i + 1;
                    while b.get(j) == Some(&b'#') {
                        j += 1;
                    }
                    if b.get(j) == Some(&b'"') {
                        let end = format!("\"{}", "#".repeat(j - i - 1));
                        i = src[j + 1..].find(&end).map_or(b.len(), |k| j + 1 + k + end.len());
                        continue;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&src[start.min(src.len())..]);
        out
    }

    #[test]
    fn comments_are_cut_only_outside_strings() {
        assert_eq!(strip_comments("let u = \"https://x\"; Command::new(p) // weg"), "let u = \"https://x\"; Command::new(p) ");
        assert_eq!(strip_comments("a(r#\"{\"u\":\"//x\"}\"#); b() // c\nd"), "a(r#\"{\"u\":\"//x\"}\"#); b() \nd");
        assert_eq!(strip_comments("let q = '\"'; x() // \"y\"\nz"), "let q = '\"'; x() \nz");
        assert_eq!(strip_comments("s(\"a\\\"//b\"); t()"), "s(\"a\\\"//b\"); t()");
        assert_eq!(strip_comments("// nur Kommentar\nfn f() {}"), "\nfn f() {}");
    }

    /// Anfang des Testmoduls: `#[cfg(test)]`, danach höchstens Attribute, dann `mod tests`
    fn test_module(src: &str) -> Option<usize> {
        src.match_indices("#[cfg(test)]").map(|(i, _)| i).find(|&i| {
            let mut rest = src[i + "#[cfg(test)]".len()..].trim_start();
            while rest.starts_with("#[") {
                rest = rest.split_once('\n').map_or("", |(_, r)| r).trim_start();
            }
            rest.starts_with("mod tests")
        })
    }

    #[test]
    fn code_cuts_only_at_the_test_module() {
        let src = "fn a() {}\n#[cfg(test)]\nlet x = 1;\nCommand::new(x)\n#[cfg(test)]\n#[path = \"t.rs\"]\nmod tests;\nhidden";
        let c = code(src);
        assert!(c.contains("Command::new(x)") && !c.contains("hidden"), "{c}");
        assert!(code("a // b\n#[cfg(test)]\nmod tests {\nhidden }").starts_with("a "));
        // jedes Modul mit Testmodul wird genau dort geschnitten
        for (name, src) in MODULES {
            if let Some(i) = test_module(src) {
                assert!(src[i..].lines().take(3).any(|l| l.starts_with("mod tests")), "{name}");
            }
        }
    }

    /// Alle Module des Binaries (`sources_stay_apart` prüft die Liste gegen den Ordner; ein neues muss dazu)
    const MODULES: [(&str, &str); 17] = [
        ("act.rs", include_str!("act.rs")),
        ("app.rs", include_str!("app.rs")),
        ("clip.rs", include_str!("clip.rs")),
        ("diag.rs", include_str!("diag.rs")),
        ("diag_view.rs", include_str!("diag_view.rs")),
        ("link.rs", include_str!("link.rs")),
        ("lock.rs", include_str!("lock.rs")),
        ("main.rs", include_str!("main.rs")),
        ("niri.rs", include_str!("niri.rs")),
        ("output.rs", include_str!("output.rs")),
        ("overlay.rs", include_str!("overlay.rs")),
        ("pal.rs", include_str!("pal.rs")),
        ("pal_host.rs", include_str!("pal_host.rs")),
        ("pal_view.rs", include_str!("pal_view.rs")),
        ("panel.rs", include_str!("panel.rs")),
        ("primary.rs", include_str!("primary.rs")),
        ("wl.rs", include_str!("wl.rs")),
    ];

    /// Keine Shell (Sicherheitsregel 4, CLAUDE.md „Keine Shell“; bis W7 prüfte der Xephyr-Test das für Lua): Prozesse
    /// nur mit festem Programmnamen und argv, nie `sh -c`, nie die Kommandozeilen-Helfer von glib/gio, und die
    /// niri-Kürzel nie mit `spawn-sh`. Seit W5 (Palette, 6g Regel 3) genau zwei Ausnahmen mit nicht festem Programm,
    /// beide nur mit einem über den Suchpfad aufgelösten absoluten Pfad (`ui::pal_exec::Exe`/`Launch`, deren Felder nur
    /// `pal_exec` setzen kann): ein `Command::new(exe.path())` in `pal_host.rs` (Kurzläufer) und ein `Action::Spawn` in
    /// `niri.rs` (langlebige Programme, E10); `SpawnSh` nirgends. Die Teile ohne GTK unter `src/ui/` starten gar keinen
    /// Prozess
    #[test]
    fn no_shell_anywhere() {
        const SHELLS: [&str; 5] = ["sh", "bash", "zsh", "dash", "fish"];
        let mut dynamic = Vec::new();
        let mut spawns = 0;
        for (name, src) in MODULES {
            let c = code(src);
            for (i, _) in c.match_indices("Command::new(") {
                let arg = c[i + "Command::new(".len()..].trim_start();
                if !arg.starts_with('"') {
                    assert!(arg.starts_with("exe.path())"), "{name}: Command::new only with a fixed program name or a resolved Exe");
                    dynamic.push(name);
                    continue;
                }
                let prog = arg[1..].split('"').next().unwrap_or("");
                assert!(!SHELLS.contains(&prog.rsplit('/').next().unwrap_or(prog)), "{name} runs a shell");
            }
            assert!(!c.contains("\"-c\""), "{name} passes -c");
            assert!(!c.contains("spawn_command_line") && !c.contains("Subprocess"), "{name} starts a process through glib/gio");
            // weitere Startwege, die `Command::new(` nicht träfe (W5 Durchsicht H1)
            for bad in ["Command as", "CommandExt", "spawn_async", "AppInfo", "UriLauncher", "show_uri", "posix_spawn", "libc::exec", "\"Spawn\""] {
                assert!(!c.contains(bad), "{name} starts a process through {bad}");
            }
            assert!(!c.contains("SpawnSh"), "{name} spawns through niri's shell");
            let n = c.matches("Action::Spawn").count();
            assert!(n == 0 || name == "niri.rs", "{name} spawns through niri");
            spawns += n;
        }
        assert_eq!(dynamic, ["pal_host.rs"], "exactly one Command::new with a resolved program");
        assert_eq!(spawns, 1, "exactly one niri Spawn");
        // pal_host startet nur über `run_child`, und das nimmt nur ein `Exe`
        let host = code(include_str!("pal_host.rs"));
        assert!(host.contains("pub fn run_child(exe: &Exe, args: &[&str], timeout: Duration)"));
        let niri = code(include_str!("niri.rs"));
        assert!(niri.contains("pub fn spawn_at(path: &Path, launch: Launch)"));
        // die Teile ohne GTK (`src/ui/`, auch in der Bibliothek) starten nichts
        let ui = concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui");
        let mut seen = 0;
        for e in std::fs::read_dir(ui).unwrap() {
            let path = e.unwrap().path();
            if path.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            seen += 1;
            let c = code(&std::fs::read_to_string(&path).unwrap());
            let name = path.display();
            for bad in ["Command::new", "process::Command", "SpawnSh", "Action::Spawn", "spawn_command_line", "Subprocess", "\"-c\""] {
                assert!(!c.contains(bad), "{name} contains {bad}");
            }
        }
        assert!(seen > 20, "{seen} modules under src/ui");
        let kdl = include_str!("../../../niri/dictate-binds.kdl");
        assert!(!kdl.lines().any(|l| l.split("//").next().unwrap_or("").contains("spawn-sh")), "spawn-sh in dictate-binds.kdl");
    }

    /// Nur PRIMARY für die Textaktionen (Regel 4.1): jede Quelle wird nur in ihrem Modul gelesen, empfangen wird nur
    /// in `clip::read_offer`
    #[test]
    fn sources_stay_apart() {
        let files = MODULES;
        // alle Module des Binaries sind hier aufgeführt (ein neues muss dazu)
        let listed = files.len();
        let on_disk = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/dictate-ui"))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "rs"))
            .count();
        assert_eq!(listed, on_disk, "list every module of dictate-ui here");
        for (name, src) in files {
            let c = code(src);
            let uses = |pat: &str| c.contains(pat);
            if uses("st.selection") || uses(".selection.") {
                assert!(["clip.rs", "wl.rs"].contains(&name), "{name} touches CLIPBOARD");
            }
            if uses("st.primary") {
                assert!(["primary.rs", "wl.rs"].contains(&name), "{name} touches PRIMARY");
            }
            if uses(".receive(") {
                assert_eq!(name, "clip.rs", "{name} receives an offer");
            }
            if uses("read_offer(") {
                assert!(["clip.rs", "primary.rs"].contains(&name), "{name} reads an offer");
            }
        }
        // `primary.rs` liest nie die Zwischenablage
        let p = code(include_str!("primary.rs"));
        assert!(!p.contains("selection.clone") && !p.contains("st.selection"));
        // CLIPBOARD als Text nur für `explain_clipboard` (Lesart L8): gelesen in `clip.rs`, aufgerufen genau einmal im
        // Ausgabe-Thread, angefordert genau einmal in `act.rs` und dort nur für die Quelle `clipboard`
        let count = |name: &str, pat: &str| files.iter().filter(|(n, _)| *n == name).map(|(_, src)| code(src).matches(pat).count()).sum::<usize>();
        for (name, src) in files {
            let c = code(src);
            if c.contains("read_clipboard_text(") {
                assert!(["clip.rs", "output.rs"].contains(&name), "{name} reads the clipboard as text");
            }
            if c.contains(".read_clipboard(") {
                assert_eq!(name, "act.rs", "{name} asks for the clipboard");
            }
            if c.contains("Cmd::ReadClipboard") {
                assert_eq!(name, "output.rs", "{name} sends a clipboard read");
            }
        }
        assert_eq!(count("clip.rs", "fn read_clipboard_text("), 1);
        assert_eq!(count("output.rs", "read_clipboard_text("), 1, "one caller in the output thread");
        assert_eq!(count("act.rs", ".read_clipboard("), 1, "one request in act.rs");
        assert!(code(include_str!("act.rs")).contains("Source::Clipboard => out.read_clipboard(READ_WAIT)"), "only for the source clipboard");
    }
}
