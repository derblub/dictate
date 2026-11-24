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

//! `dictate-ui`: Wayland-Oberfläche von dictate unter niri (PLAN-wayland.md, Block W1).
//!
//! `dictate-ui run` ist der langlebige Prozess (User-Dienst `dictate-ui.service` in der Wayland-Sitzung): er hängt
//! wie früher das Awesome-Modul am Daemon (`subscribe`), zeichnet das Overlay als Layer-Fläche (ohne Fokus,
//! klick-durch) und führt das Halten zum Sprechen aus (Tastenzustand nur über `EVIOCGKEY`, `ui::keys`). Die
//! niri-Kürzel (`niri/dictate-binds.kdl`) rufen `dictate-ui hold|toggle|cancel` auf; diese kurzen Aufrufe schicken
//! eine Zeile an den Steuer-Socket des laufenden Prozesses (`ui::ctl`) und starten GTK nie.
//!
//! Den fertigen Text schreibt der Ausgabe-Thread (`output.rs`, Block W2): Eingabemethode, Einfügen über die
//! Zwischenablage (`clip.rs`) oder eigene virtuelle Tastatur, über eine eigene Wayland-Verbindung (`wl.rs`); das
//! fokussierte Fenster kommt über niri-IPC (`niri.rs`).
//!
//! Textaktionen (Block W3): Mod+I (`dictate-ui actions`) öffnet das Panel `dictate-panel` (`panel.rs`, schwebendes
//! GTK-Fenster; Automat `ui::panel`), die Markierung liest der Ausgabe-Thread (`primary.rs`, nur PRIMARY), die
//! `act_*`-Aufrufe laufen in `act.rs`, Replace über die Warteschlange der Ausgabe. Sperrt die Sitzung (logind auf dem
//! System-Bus, `lock.rs`), schließt das Panel und die Sitzungen im Daemon werden gelöscht (`act_clear`, `diag_clear`).
//!
//! Befehlspalette (Block W5): Mod+Alt+Space (`dictate-ui palette`) öffnet die Quelle „palette“ im selben Fenster
//! (`pal_view.rs`, Automat `ui::cmdpal`); `pal_resolve`/`pal_ask` laufen in `pal.rs`, ausgeführt wird über
//! `ui::pal_exec` mit dem Wirt `pal_host.rs` (niri-IPC, Kurzläufer).
//!
//! Schlank: die Module werden per Pfad eingebunden, ohne die Bibliothek `dictate` (kein
//! Whisper, kein CUDA im Link). Der Daemon bekommt keinen Bezug zum Display.

#[allow(dead_code)]
#[path = "../../actions/mod.rs"]
mod actions;
#[allow(dead_code)]
#[path = "../../config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../../protocol.rs"]
mod protocol;
#[allow(dead_code)]
#[path = "../../sock_client.rs"]
mod sock_client;
#[allow(dead_code)]
#[path = "../../ui/mod.rs"]
mod ui;

mod act;
mod app;
mod clip;
mod diag;
mod diag_view;
mod link;
mod lock;
mod niri;
mod output;
mod overlay;
mod pal;
mod pal_host;
mod pal_view;
mod panel;
mod primary;
mod wl;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use ui::ctl::{self, Sent, UiCmd};

#[derive(Parser)]
#[command(name = "dictate-ui", version, about = "Wayland overlay, hold-to-speak and text output for dictate (niri)")]
struct Cli {
    #[command(subcommand)]
    command: Sub,
}

#[derive(Subcommand)]
enum Sub {
    /// Run the overlay in the Wayland session (started by dictate-ui.service)
    Run {
        /// Read the key state from this file instead of /dev/input ("1"/"0" = E down/up, then "mod" = modifier held; tests only)
        #[arg(long, value_name = "FILE", hide = true)]
        fake_keys: Option<PathBuf>,
        /// Times of the boot hint in ms: first ask, retry, shown for (tests only)
        #[arg(long, value_name = "FIRST,RETRY,SHOW", hide = true, value_parser = ui::boot_hint::Timing::parse)]
        boot_hint_ms: Option<ui::boot_hint::Timing>,
        /// Find the programs of the command palette only in this directory, never in PATH (absolute; tests only)
        #[arg(long, value_name = "DIR", hide = true)]
        fake_programs: Option<PathBuf>,
    },
    /// Start listening; stops when the dictate key is released (bound to Mod+E in niri)
    Hold,
    /// Start or stop hands-free listening (Mod+Shift+E)
    Toggle,
    /// Stop listening and discard the text (Mod+Ctrl+E); works without a running dictate-ui
    Cancel,
    /// Open or close the text actions panel for the selection (Mod+I)
    Actions,
    /// Open or close the diagnosis panel (Mod+Shift+I)
    Diagnose,
    /// Open or close the command palette (Mod+Alt+Space)
    Palette,
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("DICTATE_LOG").unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Ohne Terminal (Journal des Dienstes) keine Farbcodes
    let ansi = std::io::IsTerminal::is_terminal(&std::io::stderr());
    tracing_subscriber::fmt().with_env_filter(filter).with_ansi(ansi).with_writer(std::io::stderr).init();
}

/// Kurzer Aufruf aus einem niri-Kürzel: eine Zeile an die laufende Oberfläche. Exit 0 ok, 1 abgelehnt/Fehler, 2 keine
/// Oberfläche (bei `cancel`: kein Daemon)
fn client(cmd: UiCmd) -> ExitCode {
    match ctl::send(&ctl::socket_path(), cmd) {
        Ok(Sent::Ok) => ExitCode::SUCCESS,
        Ok(Sent::Refused(e)) => {
            eprintln!("dictate-ui: {e}");
            ExitCode::from(1)
        }
        Ok(Sent::NotRunning) if cmd == UiCmd::Cancel => link::cancel_direct(),
        Ok(Sent::NotRunning) => {
            eprintln!("dictate-ui: not running (no socket at {})", ctl::socket_path().display());
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("dictate-ui: {e}");
            ExitCode::from(1)
        }
    }
}

/// Suchpfad aus genau einem Ordner mit Fake-Programmen (`--fake-programs`, L22): absolut, UTF-8, vorhanden. Sonst ein
/// Fehler, denn `Programs::with_dirs` verwürfe den Ordner still, und ein leerer Suchpfad sähe wie ein Test aus
fn fake_programs_dir(dir: &Path) -> Result<ui::pal_exec::Programs, String> {
    if !dir.is_absolute() || dir.to_str().is_none() {
        return Err(format!("--fake-programs needs an absolute UTF-8 path, not {}", dir.display()));
    }
    if !dir.is_dir() {
        return Err(format!("--fake-programs: {} is not a directory", dir.display()));
    }
    Ok(ui::pal_exec::Programs::with_dirs([dir.to_path_buf()]))
}

fn main() -> ExitCode {
    // Argumente vor allem anderen: die kurzen Aufrufe berühren GTK und das Display nie
    let cli = Cli::parse();
    match cli.command {
        Sub::Hold => client(UiCmd::Hold),
        Sub::Toggle => client(UiCmd::Toggle),
        Sub::Cancel => client(UiCmd::Cancel),
        Sub::Actions => client(UiCmd::Actions),
        Sub::Diagnose => client(UiCmd::Diagnose),
        Sub::Palette => client(UiCmd::Palette),
        Sub::Run { fake_keys, boot_hint_ms, fake_programs } => {
            init_logging();
            // Suchpfad der Programme der Palette: der der Sitzung, in Tests nur der Ordner mit Fake-Programmen (L22).
            // Ein unbrauchbarer Ordner beendet: nie still auf den echten Suchpfad zurückfallen
            let programs = match fake_programs.as_deref().map(fake_programs_dir) {
                None => ui::pal_exec::Programs::session(),
                Some(Ok(p)) => {
                    tracing::info!("palette programs only from {} (test mode)", fake_programs.as_deref().unwrap_or(Path::new("")).display());
                    p
                }
                Some(Err(e)) => {
                    tracing::error!("{e}");
                    eprintln!("dictate-ui: {e}");
                    return ExitCode::from(1);
                }
            };
            match app::run(fake_keys, boot_hint_ms.unwrap_or_default(), programs) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    eprintln!("dictate-ui: {e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake_programs_dir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fake_programs_must_be_an_absolute_directory() {
        let dir = std::env::temp_dir().join(format!("dictate-ui-fakes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("systemctl");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = fake_programs_dir(&dir).expect("absolute directory");
        assert_eq!(p.find("systemctl").map(|e| e.path().to_path_buf()), Some(fake.clone()), "the fake is found");
        assert!(p.find("sh").is_none(), "nothing from PATH");
        assert!(fake_programs_dir(std::path::Path::new("fakes")).is_err(), "relative");
        assert!(fake_programs_dir(&dir.join("missing")).is_err(), "missing");
        assert!(fake_programs_dir(&fake).is_err(), "a file");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
