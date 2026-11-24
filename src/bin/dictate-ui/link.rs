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

//! Verbindungen von `dictate-ui`: Events des Daemons (`subscribe`, eigener Thread mit Wiederverbindung), Befehle an
//! den Daemon (eigener Thread, der Reihe nach: `start` kommt immer vor dem zugehörigen `stop`) und der Steuer-Socket
//! der niri-Kürzel (`ui::ctl`). Alles meldet sich über einen Kanal beim GTK-Hauptthread ([`Msg`]).
//!
//! Ins Log gehen nur Zustände, Befehlsnamen und Längen, nie Mitschrift oder fertiger Text.

use crate::protocol::{self, Cmd, Event, Reply, Request};
use crate::sock_client::connect;
use crate::ui::ctl::{self, UiCmd};
use crate::ui::hold::DaemonCmd;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Nachricht an den Hauptthread
pub enum Msg {
    Event(Event),
    /// Verbindung zum Daemon steht (`subscribe` gesendet)
    Connected,
    Disconnected,
    /// `max_listen_s` des Daemons
    Limit(u64),
    /// Einstellungen der Textausgabe (beim Verbinden und nach jeder Änderung der Config)
    OutCfg(crate::ui::output::OutCfg),
    /// Ausgabe-Thread: Auftrag fertig
    OutputDone {
        id: u64,
        done: crate::output::Done,
    },
    /// Ausgabe-Thread: Hinweis im Overlay
    OutputNotice(String, crate::ui::view::NoticeKind),
    Ctl(UiCmd),
    /// Befehl an den Daemon gescheitert
    Failed {
        cmd: DaemonCmd,
        /// Laufende Nummer des Befehls ([`Commander::send`]): ein später gescheitertes `start` betrifft nur das Halten,
        /// das es ausgelöst hat
        seq: u64,
        msg: String,
    },
    /// SIGTERM, SIGINT oder SIGHUP: geordnet beenden
    Quit,
    /// Textaktionen: Antworten des Daemons und gemerkter Fokus (`act.rs`)
    Act(crate::act::ActMsg),
    /// Diagnose: Zwischenzeilen und Antworten des Daemons (`diag.rs`)
    Diag(crate::diag::DiagMsg),
    /// Befehlspalette: Öffnen, Antworten des Daemons, Ergebnis der Ausführung (`pal.rs`)
    Pal(crate::pal::PalMsg),
    /// `palette.timeout_ms` des Daemons (Frist von `pal_ask`, beim Verbinden und nach jeder Änderung der Config)
    PalAskTimeout(u64),
    /// Boot-Hinweis: Antwort auf `diag_boot_hint` (`diag::boot_hint`, Hilfsthread)
    BootHint(crate::ui::boot_hint::Answer),
    /// Klick auf den stehenden Boot-Hinweis im Overlay
    BootClick,
    /// Textaktionen: Eingaben im Panel (`panel.rs`)
    Panel(crate::panel::UiEv),
    /// Sperr-Erkennung über logind (`lock.rs`)
    Lock(crate::lock::LockEv),
    /// Tastaturen für das Halten Nummer `gen` sind offen (im Hilfsthread geöffnet)
    Keys {
        gen: u64,
        keys: Box<dyn crate::ui::keys::KeySource>,
    },
}

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Eine Anfrage, eine Antwortzeile; `Ok(None)`: kein Daemon
fn request(sock: &Path, req: &Request) -> anyhow::Result<Option<Reply>> {
    let Some(mut s) = connect(sock)? else { return Ok(None) };
    s.set_read_timeout(Some(REPLY_TIMEOUT))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    writeln!(s, "{}", req.to_line())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    if line.trim().is_empty() {
        anyhow::bail!("the daemon closed the connection without a reply");
    }
    Ok(Some(serde_json::from_str(line.trim())?))
}

/// `start` bzw. `toggle` mit der `app_id` des fokussierten Fensters (Profil je Programm, `profiles.*.match_class`),
/// sonst ohne
fn with_app(c: DaemonCmd) -> Request {
    match (c, crate::niri::focused_app_id()) {
        (DaemonCmd::Start, Some(app)) => Request::Start { app },
        (DaemonCmd::Toggle, Some(app)) => Request::Toggle { app },
        _ => daemon_request(c),
    }
}

fn daemon_request(c: DaemonCmd) -> Request {
    Request::Cmd(match c {
        DaemonCmd::Start => Cmd::Start,
        DaemonCmd::Stop => Cmd::Stop,
        DaemonCmd::Cancel => Cmd::Cancel,
        DaemonCmd::Toggle => Cmd::Toggle,
    })
}

/// `dictate-ui cancel` ohne laufende Oberfläche: direkt an den Daemon (eine Aufnahme muss sich immer abbrechen lassen)
pub fn cancel_direct() -> ExitCode {
    let sock = protocol::socket_path();
    match request(&sock, &daemon_request(DaemonCmd::Cancel)) {
        Ok(Some(r)) if r.ok => ExitCode::SUCCESS,
        Ok(Some(r)) => {
            eprintln!("dictate-ui: {}", r.error.as_deref().unwrap_or("cancel failed"));
            ExitCode::from(1)
        }
        Ok(None) => {
            eprintln!("dictate-ui: neither dictate-ui nor the daemon is running (no socket at {})", sock.display());
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("dictate-ui: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Ziel eines `start`/`toggle`: das fokussierte Fenster (mit seiner `app_id`) oder die Eingabezeile der Befehlspalette
/// (ohne `app`: fokussiert ist das Panel selbst, L23)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Window,
    Palette,
}

/// Befehle an den Daemon, der Reihe nach in einem eigenen Thread (der Hauptthread blockiert nie am Socket)
pub struct Commander {
    tx: std::sync::mpsc::Sender<(DaemonCmd, Target, u64)>,
    seq: std::cell::Cell<u64>,
}

impl Commander {
    pub fn spawn(sock: PathBuf, back: async_channel::Sender<Msg>) -> Commander {
        let (tx, rx) = std::sync::mpsc::channel::<(DaemonCmd, Target, u64)>();
        std::thread::Builder::new()
            .name("daemon-cmd".into())
            .spawn(move || {
                for (c, target, seq) in rx {
                    let t = Instant::now();
                    let req = match (c, target) {
                        (DaemonCmd::Start | DaemonCmd::Toggle, Target::Palette) => Request::PaletteVoice { toggle: c == DaemonCmd::Toggle, app: None },
                        (DaemonCmd::Start | DaemonCmd::Toggle, Target::Window) => with_app(c),
                        _ => daemon_request(c),
                    };
                    let msg = match request(&sock, &req) {
                        Ok(Some(r)) if r.ok => {
                            debug!("daemon {c:?}{}: ok in {} ms", if target == Target::Palette { " (palette)" } else { "" }, t.elapsed().as_millis());
                            continue;
                        }
                        Ok(Some(r)) => r.error.unwrap_or_else(|| "command failed".into()),
                        Ok(None) => "The dictate daemon is not running".into(),
                        Err(e) => format!("{e:#}"),
                    };
                    warn!("daemon {c:?} failed: {msg}");
                    if back.send_blocking(Msg::Failed { cmd: c, seq, msg }).is_err() {
                        return;
                    }
                }
            })
            .expect("thread");
        Commander { tx, seq: std::cell::Cell::new(0) }
    }

    /// Befehl einreihen; liefert seine laufende Nummer
    pub fn send(&self, c: DaemonCmd) -> u64 {
        self.queue(c, Target::Window)
    }

    /// `start` (`toggle = false`) oder `toggle` mit `target:"palette"` einreihen (die Aufnahme gehört der Eingabezeile
    /// der Befehlspalette, `ui::voice_owner`); liefert die laufende Nummer (ein `Msg::Failed` trägt sie)
    pub fn send_palette(&self, toggle: bool) -> u64 {
        self.queue(if toggle { DaemonCmd::Toggle } else { DaemonCmd::Start }, Target::Palette)
    }

    fn queue(&self, c: DaemonCmd, target: Target) -> u64 {
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        info!("daemon command {c:?}{}", if target == Target::Palette { " for the palette" } else { "" });
        let _ = self.tx.send((c, target, seq));
        seq
    }
}

/// Events des Daemons: verbinden, `subscribe`, Zeilen lesen; bei Verbindungsende neu mit Wartezeit 1 s bis 30 s
/// (wie `start_listener` in `init.lua`)
pub fn spawn_listener(sock: PathBuf, tx: async_channel::Sender<Msg>) {
    std::thread::Builder::new()
        .name("daemon-events".into())
        .spawn(move || {
            let mut backoff = Duration::from_secs(1);
            let mut up = false;
            loop {
                let started = Instant::now();
                match connect(&sock) {
                    Ok(Some(mut s)) => {
                        send_config(&sock, &tx);
                        if writeln!(s, "{}", Request::Subscribe.to_line()).is_ok() {
                            info!("connected to the daemon at {}", sock.display());
                            up = true;
                            if tx.send_blocking(Msg::Connected).is_err() {
                                return;
                            }
                            for line in BufReader::new(s).lines() {
                                let Ok(line) = line else { break };
                                // Antwortzeile auf `subscribe` und unbekannte Zeilen überspringen
                                let Ok(ev) = serde_json::from_str::<Event>(&line) else { continue };
                                // `terminals` kommt beim Abonnieren und nach jeder Änderung der Config: dann auch die
                                // Einstellungen der Ausgabe neu holen
                                let changed = matches!(ev, Event::Terminals { .. });
                                if tx.send_blocking(Msg::Event(ev)).is_err() {
                                    return;
                                }
                                if changed {
                                    send_config(&sock, &tx);
                                }
                            }
                        }
                        if started.elapsed() >= Duration::from_secs(5) {
                            backoff = Duration::from_secs(1);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!("daemon socket: {e:#}"),
                }
                if up {
                    up = false;
                    info!("lost the daemon connection");
                    if tx.send_blocking(Msg::Disconnected).is_err() {
                        return;
                    }
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                if tx.is_closed() {
                    return;
                }
            }
        })
        .expect("thread");
}

/// `get_config`: Höchstdauer und Einstellungen der Ausgabe an den Hauptthread (ein älterer Daemon ohne die neuen
/// Schlüssel liefert die Vorgaben)
fn send_config(sock: &Path, tx: &async_channel::Sender<Msg>) {
    match request(sock, &Request::GetConfig) {
        Ok(Some(r)) => {
            let Some(c) = r.extra.get("config") else { return };
            if let Some(n) = c["max_listen_s"].as_u64() {
                let _ = tx.send_blocking(Msg::Limit(n));
            }
            let _ = tx.send_blocking(Msg::OutCfg(crate::ui::output::OutCfg::from_config_json(c)));
            if let Some(ms) = c["palette"]["timeout_ms"].as_u64() {
                let _ = tx.send_blocking(Msg::PalAskTimeout(ms));
            }
        }
        Ok(None) => {}
        Err(e) => warn!("get_config: {e:#}"),
    }
}

/// Steuer-Socket der Kürzel, Ordner und Sperre. Eine Sperrdatei neben dem Socket (`flock`, solange der Prozess lebt)
/// macht die Oberfläche einmalig: erst mit der Sperre wird ein alter Socket ersetzt, und nur der Inhaber entfernt ihn
/// beim Beenden (kein Wettlauf zweier gleichzeitig startender Instanzen). Der Ordner muss dem User gehören und darf für
/// andere nicht zugänglich sein (angelegt mit 0700, ein fremder oder offener Ordner ist ein Fehler); Socket 0600
pub struct CtlServer {
    path: PathBuf,
    _lock: std::fs::File,
}

/// Ordner des Sockets: anlegen (0700) oder prüfen, dass er dem User gehört, kein Symlink ist und nur ihm offensteht
fn private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    if !dir.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let m = std::fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // SAFETY: getuid hat keine Vorbedingungen
    let uid = unsafe { libc::getuid() };
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err(format!("{} must be a directory owned by you with mode 0700", dir.display()));
    }
    Ok(())
}

impl CtlServer {
    pub fn bind(path: PathBuf, tx: async_channel::Sender<Msg>) -> Result<CtlServer, String> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        private_dir(dir)?;
        let mut lock_path = path.clone().into_os_string();
        lock_path.push(".lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|e| format!("{}: {e}", PathBuf::from(&lock_path).display()))?;
        // SAFETY: gültiger Deskriptor der eben geöffneten Datei
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!("another dictate-ui is running (lock {})", PathBuf::from(&lock_path).display()));
        }
        // Mit der Sperre gehört ein vorhandener Socket keiner laufenden Oberfläche mehr
        match std::fs::remove_file(&path) {
            Ok(()) => debug!("replaced a stale socket {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        let l = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        std::thread::Builder::new()
            .name("ctl".into())
            .spawn(move || {
                for s in l.incoming() {
                    let Ok(s) = s else { continue };
                    let _ = s.set_read_timeout(Some(ctl::TIMEOUT));
                    let _ = s.set_write_timeout(Some(ctl::TIMEOUT));
                    let mut line = String::new();
                    if BufReader::new(&s).take(256).read_line(&mut line).is_err() {
                        continue;
                    }
                    let (cmd, mut reply) = ctl::answer(&line);
                    if let Some(c) = cmd {
                        if tx.send_blocking(Msg::Ctl(c)).is_err() {
                            reply = "error shutting down".into();
                        }
                    }
                    let _ = writeln!(&s, "{reply}");
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(CtlServer { path, _lock: lock })
    }
}

impl Drop for CtlServer {
    fn drop(&mut self) {
        // noch mit der Sperre: der Socket ist sicher der eigene
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Beim Beenden (SIGTERM, Sitzungsende): läuft eine Aufnahme, wird sie abgebrochen, statt bis `max_listen_s` weiter
/// aufzunehmen (ohne Oberfläche käme der Text nirgends an)
pub fn cancel_on_exit() {
    let _ = request(&protocol::socket_path(), &daemon_request(DaemonCmd::Cancel));
}
