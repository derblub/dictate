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

//! Steuer-Socket von `dictate-ui` für die niri-Kürzel (`dictate-binds.kdl`): `dictate-ui hold|toggle|cancel|actions|diagnose|palette`.
//!
//! niri ruft bei einem Kürzel nur ein Programm auf (`spawn` mit Argumentliste). Das Halten muss aber der langlebige
//! Prozess `dictate-ui` beobachten (Tastenzustand, Overlay, später die Stimme der Palette), darum schickt der kurze
//! Aufruf den Befehl als eine Zeile an `$XDG_RUNTIME_DIR/dictate/ui.sock` (`DICTATE_UI_SOCKET` überschreibt) und
//! wartet auf `ok`. Läuft `dictate-ui` nicht, geht nur `cancel` direkt an den Daemon (eine Aufnahme muss sich immer
//! abbrechen lassen); `hold` und `toggle` melden den Fehler (ohne Oberfläche käme der Text nirgends an).
//!
//! `actions` (Mod+I) öffnet bzw. schließt das Panel der Textaktionen; das zweite Mod+I kommt so als Befehl, nicht als
//! Taste im Panel. `diagnose` (Mod+Shift+I, Kürzel ab Block W4-C) öffnet bzw. schließt das Diagnose-Panel; ein offenes
//! Panel jeder Quelle schließt es nur (ein Panel zur Zeit). `palette` (Mod+Alt+Space, W5) öffnet bzw. schließt die
//! Befehlspalette, ebenso nach der Regel „ein Panel zur Zeit“.
//!
//! Zeilen: Anfrage `hold`, `toggle`, `cancel`, `actions`, `diagnose` oder `palette`; Antwort `ok` oder `error <text>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Frist für Verbindung, Schreiben und Antwort (die Oberfläche antwortet sofort, bevor sie den Befehl ausführt)
pub const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCmd {
    Hold,
    Toggle,
    Cancel,
    /// Panel der Textaktionen öffnen oder schließen
    Actions,
    /// Diagnose-Panel öffnen oder schließen
    Diagnose,
    /// Befehlspalette öffnen oder schließen
    Palette,
}

impl UiCmd {
    pub fn name(self) -> &'static str {
        match self {
            UiCmd::Hold => "hold",
            UiCmd::Toggle => "toggle",
            UiCmd::Cancel => "cancel",
            UiCmd::Actions => "actions",
            UiCmd::Diagnose => "diagnose",
            UiCmd::Palette => "palette",
        }
    }

    pub fn parse(line: &str) -> Option<UiCmd> {
        Some(match line.trim() {
            "hold" => UiCmd::Hold,
            "toggle" => UiCmd::Toggle,
            "cancel" => UiCmd::Cancel,
            "actions" => UiCmd::Actions,
            "diagnose" => UiCmd::Diagnose,
            "palette" => UiCmd::Palette,
            _ => return None,
        })
    }
}

/// `$DICTATE_UI_SOCKET`, sonst `$XDG_RUNTIME_DIR/dictate/ui.sock` (neben dem Socket des Daemons)
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DICTATE_UI_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("dictate/ui.sock")
}

/// Ergebnis von [`send`]
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    Ok,
    /// Die Oberfläche hat abgelehnt (Text der Antwort)
    Refused(String),
    /// Keine Oberfläche erreichbar (Socket fehlt oder lehnt ab)
    NotRunning,
}

/// Befehl an die laufende Oberfläche
pub fn send(sock: &Path, cmd: UiCmd) -> std::io::Result<Sent> {
    let mut s = match UnixStream::connect(sock) {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => return Ok(Sent::NotRunning),
        Err(e) => return Err(e),
    };
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))?;
    writeln!(s, "{}", cmd.name())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    Ok(match line.trim() {
        "ok" => Sent::Ok,
        "" => Sent::Refused("no reply".into()),
        other => Sent::Refused(other.strip_prefix("error ").unwrap_or(other).to_string()),
    })
}

/// Antwortzeile des Servers auf eine Anfragezeile (ohne Zeilenende); `None` bei unbekanntem Befehl
pub fn answer(line: &str) -> (Option<UiCmd>, String) {
    match UiCmd::parse(line) {
        Some(c) => (Some(c), "ok".into()),
        None => (None, "error unknown command (expected hold, toggle, cancel, actions, diagnose or palette)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn names_roundtrip() {
        for c in [UiCmd::Hold, UiCmd::Toggle, UiCmd::Cancel, UiCmd::Actions, UiCmd::Diagnose, UiCmd::Palette] {
            assert_eq!(UiCmd::parse(c.name()), Some(c));
            assert_eq!(UiCmd::parse(&format!(" {}\n", c.name())), Some(c));
        }
        assert_eq!(UiCmd::parse("start"), None);
        assert_eq!(answer("bogus").0, None);
        assert!(answer("bogus").1.starts_with("error "));
    }

    #[test]
    fn send_to_server_and_to_nothing() {
        let dir = std::env::temp_dir().join(format!("dictate-ctl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("ui.sock");
        assert_eq!(send(&sock, UiCmd::Hold).unwrap(), Sent::NotRunning);
        let l = UnixListener::bind(&sock).unwrap();
        let t = std::thread::spawn(move || {
            let mut got = vec![];
            for _ in 0..2 {
                let (s, _) = l.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                let (cmd, reply) = answer(&line);
                got.push(cmd);
                writeln!(&s, "{reply}").unwrap();
            }
            got
        });
        assert_eq!(send(&sock, UiCmd::Cancel).unwrap(), Sent::Ok);
        assert_eq!(send(&sock, UiCmd::Toggle).unwrap(), Sent::Ok);
        assert_eq!(t.join().unwrap(), vec![Some(UiCmd::Cancel), Some(UiCmd::Toggle)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
