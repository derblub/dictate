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

//! Sperr-Erkennung in `dictate-ui` (PLAN-wayland.md 3.6, Regel 7, Befund 11): systemd-logind auf dem System-Bus über
//! gio (kommt mit GTK, keine eigene Crate). Alles läuft im Hauptthread (Kontext von GTK) und blockiert nie: Verbinden,
//! `GetSession` und das Lesen von `LockedHint` sind asynchron, ihre Rückrufe und die der Signale reichen nur
//! [`LockEv`] über den Kanal weiter. Entschieden wird in `ui::lock::Watch` ([`Logind::event`]), ausgeführt in `app.rs`
//! (Panel schließen, `act_clear`, `diag_clear`).
//!
//! Abonniert wird vor `GetSession`, nur mit Absender `org.freedesktop.login1`; gefragt werden `XDG_SESSION_ID` und
//! `auto` zugleich, beobachtet jede Sitzung, die dabei herauskommt (weichen sie ab, steht eine Warnung im Log).
//! Scheitert etwas, steht es als Warnung im Log: ohne Erkennung bleibt eine Sitzung bis zum Schließen bzw. zum Ablauf
//! im Daemon (300 s) liegen. Geht der Bus verloren oder ist er beim Start nicht da, wird mit wachsendem Abstand neu
//! verbunden (2 s bis 60 s).
//!
//! Tests: der System-Bus ist ein privater Bus mit einem falschen logind (`DBUS_SYSTEM_BUS_ADDRESS` aus
//! `tests/wayland/nested/harness.sh`); der Code kennt keinen Testweg.

use crate::link::Msg;
use crate::ui::lock::{self, Reason, Signal, Watch};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Frist der Aufrufe an logind
const CALL_TIMEOUT_MS: i32 = 5000;
/// Erster und höchster Abstand der Versuche, neu zu verbinden
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// Ereignisse vom Bus an den Hauptthread
pub enum LockEv {
    Connected(Bus),
    /// System-Bus nicht erreichbar
    Failed(String),
    Signal(Signal),
    /// Antwort auf `GetSession(id)`: Pfad der Sitzung (`epoch`: Verbindung, auf der gefragt wurde)
    Session {
        epoch: u64,
        id: String,
        res: Result<String, String>,
    },
    /// `LockedHint` einer eigenen Sitzung beim Start
    Initial {
        epoch: u64,
        path: String,
        res: Result<bool, String>,
    },
    /// Verbindung zum Bus weg
    Closed,
    /// Zeit für einen neuen Versuch, zu verbinden
    Retry,
}

/// Verbindung mit den Abos (enden mit dem Wert)
pub struct Bus {
    conn: gio::DBusConnection,
    _subs: Vec<gio::SignalSubscription>,
}

/// Zustand im Hauptthread
pub struct Logind {
    bus: Option<Bus>,
    watch: Watch,
    /// Kandidaten für `GetSession` (aus `XDG_SESSION_ID` und `auto`) und die Pfade, die sie ergaben
    ids: Vec<String>,
    found: Vec<(String, String)>,
    tx: async_channel::Sender<Msg>,
    /// Abstand des nächsten Versuchs, neu zu verbinden
    retry: Duration,
    /// Ausfall schon als Warnung gemeldet (weitere Fehlschläge nur im Debug-Log)
    warned: bool,
    /// Erkennung bekannt aus (kein Bus, Verbindung weg, Sitzung nicht gefunden): das Panel zeigt dann einen Hinweis
    /// (Entscheidung Daniel, 20.11.)
    off: bool,
    /// Nummer der Verbindung: späte Antworten einer verlorenen werden verworfen
    epoch: u64,
}

fn send(tx: &async_channel::Sender<Msg>, ev: LockEv) {
    let _ = tx.send_blocking(Msg::Lock(ev));
}

impl Logind {
    /// Verbinden (asynchron); die Sitzung kommt aus `XDG_SESSION_ID`, sonst `auto`
    pub fn start(tx: async_channel::Sender<Msg>) -> Logind {
        let env = std::env::var("XDG_SESSION_ID").ok();
        let l = Logind {
            bus: None,
            watch: Watch::new(),
            ids: lock::session_ids(env.as_deref()),
            found: vec![],
            tx,
            retry: RETRY_FIRST,
            warned: false,
            off: false,
            epoch: 0,
        };
        l.connect();
        l
    }

    fn connect(&self) {
        let t = self.tx.clone();
        gio::bus_get(gio::BusType::System, gio::Cancellable::NONE, move |res| match res {
            Ok(conn) => {
                let bus = subscribe(conn, &t);
                send(&t, LockEv::Connected(bus));
            }
            Err(e) => send(&t, LockEv::Failed(e.message().to_string())),
        });
    }

    /// Erkennung aus: Sperren löscht die Textaktionen nicht (Hinweis im Panel)
    pub fn off(&self) -> bool {
        self.off
    }

    /// Gesperrt (`LockedHint` einer eigenen Sitzung oder Einschlafen): das Panel öffnet dann nicht
    pub fn locked(&self) -> bool {
        self.watch.locked()
    }

    /// Erkennung aus (kein Bus, Sitzung nicht gefunden): neuer Versuch nach `retry`, Abstand wächst
    fn lost(&mut self, what: &str) {
        self.off = true;
        if self.warned {
            debug!("lock: {what}; next try in {} s", self.retry.as_secs());
        } else {
            warn!("lock: {what}; text actions are not cleared when the session locks until it works again (next try in {} s)", self.retry.as_secs());
            self.warned = true;
        }
        self.bus = None;
        self.watch = Watch::new();
        self.found.clear();
        self.epoch += 1;
        let tx = self.tx.clone();
        glib::timeout_add_local_once(self.retry, move || send(&tx, LockEv::Retry));
        self.retry = (self.retry * 2).min(RETRY_MAX);
    }

    /// Ein Ereignis; `Some`, wenn eine eigene Sitzung gesperrt wurde (oder das System einschläft)
    pub fn event(&mut self, ev: LockEv) -> Option<Reason> {
        match ev {
            LockEv::Connected(bus) => {
                debug!("lock: connected to the system bus");
                self.bus = Some(bus);
                self.watch.expect(self.ids.len());
                for id in self.ids.clone() {
                    self.get_session(id);
                }
                None
            }
            LockEv::Failed(e) => {
                self.lost(&format!("no system bus ({e})"));
                None
            }
            LockEv::Retry => {
                self.connect();
                None
            }
            LockEv::Signal(s) => self.watch.signal(s),
            LockEv::Session { epoch, .. } | LockEv::Initial { epoch, .. } if epoch != self.epoch => None,
            LockEv::Session { id, res, .. } => {
                let gate = id == "auto";
                let checked = res.and_then(|p| self.watch.resolved(&p, gate).map(|r| (p, r)));
                let reason = match checked {
                    Ok((path, early)) => {
                        info!("lock: watching logind session {path} (from \"{id}\")");
                        self.off = false;
                        if let Some((other_id, other)) = self.found.iter().find(|(_, p)| *p != path) {
                            warn!("lock: logind sessions differ: \"{other_id}\" is {other}, \"{id}\" is {path}; watching both");
                        }
                        self.found.push((id, path.clone()));
                        self.read_hint(&path);
                        early
                    }
                    Err(e) => {
                        info!("lock: GetSession(\"{id}\") failed: {e}");
                        self.watch.failed();
                        None
                    }
                };
                if self.watch.gave_up() {
                    self.off = true;
                    warn!("lock: own logind session not found; text actions are cleared only before sleep, not when the session locks");
                } else if !self.found.is_empty() {
                    // läuft: Abstand der Versuche zurücksetzen
                    self.retry = RETRY_FIRST;
                    self.warned = false;
                }
                reason
            }
            LockEv::Initial { path, res: Ok(locked), .. } => {
                debug!("lock: LockedHint of {path} at start {locked}");
                self.watch.initial(&path, locked)
            }
            LockEv::Initial { path, res: Err(e), .. } => {
                warn!("lock: cannot read LockedHint of {path}: {e}");
                None
            }
            LockEv::Closed => {
                self.lost("the system bus connection closed");
                None
            }
        }
    }

    /// `GetSession(id)` (asynchron)
    fn get_session(&self, id: String) {
        let Some(bus) = &self.bus else { return };
        let (tx, epoch) = (self.tx.clone(), self.epoch);
        let args = (id.as_str(),).to_variant();
        bus.conn.call(
            Some(lock::LOGIND),
            lock::MANAGER_PATH,
            lock::MANAGER_IFACE,
            "GetSession",
            Some(&args),
            Some(glib::VariantTy::new("(o)").expect("type")),
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |res| {
                let res = res
                    .map_err(|e| e.message().to_string())
                    .and_then(|v| v.get::<(glib::variant::ObjectPath,)>().map(|(p,)| p.as_str().to_string()).ok_or_else(|| "unexpected reply".to_string()));
                send(&tx, LockEv::Session { epoch, id, res });
            },
        );
    }

    /// `LockedHint` einmal lesen (war die Sitzung beim Start schon gesperrt?)
    fn read_hint(&self, path: &str) {
        let Some(bus) = &self.bus else { return };
        let (tx, epoch) = (self.tx.clone(), self.epoch);
        let path_owned = path.to_string();
        let args = (lock::SESSION_IFACE, "LockedHint").to_variant();
        bus.conn.call(
            Some(lock::LOGIND),
            path,
            lock::PROPS_IFACE,
            "Get",
            Some(&args),
            Some(glib::VariantTy::new("(v)").expect("type")),
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |res| {
                let res = res
                    .map_err(|e| e.message().to_string())
                    .and_then(|v| v.get::<(glib::Variant,)>().and_then(|(b,)| b.get::<bool>()).ok_or_else(|| "unexpected reply".to_string()));
                send(&tx, LockEv::Initial { epoch, path: path_owned, res });
            },
        );
    }
}

/// Die drei Signale abonnieren (vor `GetSession`, Absender nur logind); die Rückrufe packen nur aus
fn subscribe(conn: gio::DBusConnection, tx: &async_channel::Sender<Msg>) -> Bus {
    // Vorgabe von g_bus_get wäre „Prozess beenden, wenn der Bus geht“ (mit Status 0: Restart=on-failure startete
    // dictate-ui dann nicht neu). Stattdessen melden und ohne Erkennung weiterlaufen
    conn.set_exit_on_close(false);
    let t = tx.clone();
    conn.connect_closed(move |_, _, _| send(&t, LockEv::Closed));
    let flags = gio::DBusSignalFlags::NONE;
    let t = tx.clone();
    let props =
        conn.subscribe_to_signal(Some(lock::LOGIND), Some(lock::PROPS_IFACE), Some("PropertiesChanged"), None, Some(lock::SESSION_IFACE), flags, move |s| {
            send(&t, LockEv::Signal(Signal::Props { path: s.object_path.to_string(), locked: changed_hint(s.parameters) }));
        });
    let t = tx.clone();
    let lock_sig = conn.subscribe_to_signal(Some(lock::LOGIND), Some(lock::SESSION_IFACE), Some("Lock"), None, None, flags, move |s| {
        send(&t, LockEv::Signal(Signal::Lock { path: s.object_path.to_string() }));
    });
    let t = tx.clone();
    let sleep =
        conn.subscribe_to_signal(Some(lock::LOGIND), Some(lock::MANAGER_IFACE), Some("PrepareForSleep"), Some(lock::MANAGER_PATH), None, flags, move |s| {
            if let Some((going,)) = s.parameters.get::<(bool,)>() {
                send(&t, LockEv::Signal(Signal::Sleep(going)));
            }
        });
    Bus { conn, _subs: vec![props, lock_sig, sleep] }
}

/// `LockedHint` aus `PropertiesChanged(s interface, a{sv} changed, as invalidated)` des Interface `Session`
fn changed_hint(params: &glib::Variant) -> Option<bool> {
    if !params.is_type(glib::VariantTy::new("(sa{sv}as)").expect("type")) || params.child_value(0).str() != Some(lock::SESSION_IFACE) {
        return None;
    }
    glib::VariantDict::new(Some(&params.child_value(1))).lookup::<bool>("LockedHint").ok().flatten()
}
