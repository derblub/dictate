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

//! Verbindung zum Daemon. Eine dauerhafte Subscriber-Verbindung (Thread, Events per Channel) und je Befehl eine
//! eigene kurze Verbindung. Kein Polling: der Subscriber liest blockierend und versucht nur bei getrennter
//! Verbindung alle paar Sekunden neu zu verbinden.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Wofür eine Antwort gedacht war.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    GetConfig,
    Apply,
    Sources,
    Models,
    AiModels,
    History,
    HistoryClear,
    AiTest,
    Commands,
    /// Seite Palette: `pal_frecency_clear` und `pal_sources_refresh`
    PalClear,
    PalRefresh,
}

pub enum Msg {
    Connected(bool),
    Event(Value),
    Reply { tag: Tag, res: Result<Value, String> },
    Download(super::downloads::Progress),
}

#[derive(Clone)]
pub struct Net {
    pub sock: PathBuf,
    tx: Sender<Msg>,
    ctx: egui::Context,
    /// Schreibseite der Subscriber-Verbindung (für `meter`)
    sub_writer: Arc<Mutex<Option<UnixStream>>>,
}

impl Net {
    pub fn new(sock: PathBuf, tx: Sender<Msg>, ctx: egui::Context) -> Net {
        Net { sock, tx, ctx, sub_writer: Arc::new(Mutex::new(None)) }
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    fn post(tx: &Sender<Msg>, ctx: &egui::Context, m: Msg) {
        if tx.send(m).is_ok() {
            ctx.request_repaint();
        }
    }

    /// Startet den Subscriber-Thread.
    pub fn spawn_subscriber(&self) {
        let me = self.clone();
        std::thread::Builder::new().name("subscriber".into()).spawn(move || me.subscriber_loop()).expect("thread");
    }

    fn subscriber_loop(&self) {
        // Verbindungsstatus nur bei Änderung melden (kein Repaint im Takt der Wiederverbindung)
        let mut last: Option<bool> = None;
        let mut report = |on: bool, me: &Net| {
            if last != Some(on) {
                last = Some(on);
                Self::post(&me.tx, &me.ctx, Msg::Connected(on));
            }
        };
        loop {
            match UnixStream::connect(&self.sock) {
                Ok(mut s) => {
                    if writeln!(s, "{}", json!({"cmd":"subscribe"})).is_err() {
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    if let Ok(w) = s.try_clone() {
                        *self.sub_writer.lock().unwrap() = Some(w);
                    }
                    report(true, self);
                    for line in BufReader::new(s).lines() {
                        let Ok(line) = line else { break };
                        if let Ok(v) = serde_json::from_str::<Value>(&line) {
                            // Antworten auf `meter` (ohne "event") sind uninteressant
                            if v.get("event").is_some() {
                                Self::post(&self.tx, &self.ctx, Msg::Event(v));
                            }
                        }
                    }
                    *self.sub_writer.lock().unwrap() = None;
                    report(false, self);
                }
                Err(_) => report(false, self),
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    /// Pegel-Messung ein-/ausschalten. Läuft über die Subscriber-Verbindung, weil der Daemon das Mikrofon
    /// beim Trennen dieser Verbindung wieder schließt.
    pub fn meter(&self, on: bool) {
        if let Some(w) = self.sub_writer.lock().unwrap().as_mut() {
            let _ = writeln!(w, "{}", json!({"cmd":"meter","on":on}));
        }
    }

    /// Befehl auf eigener Verbindung in einem Thread; die Antwort kommt als `Msg::Reply`.
    pub fn request(&self, tag: Tag, cmd: Value) {
        let (tx, ctx, sock) = (self.tx.clone(), self.ctx.clone(), self.sock.clone());
        let timeout = if tag == Tag::AiTest { 60 } else { 10 };
        std::thread::spawn(move || {
            let res = roundtrip(&sock, &cmd, timeout);
            Self::post(&tx, &ctx, Msg::Reply { tag, res });
        });
    }
}

fn roundtrip(sock: &PathBuf, cmd: &Value, timeout_s: u64) -> Result<Value, String> {
    let mut s = UnixStream::connect(sock).map_err(|e| format!("daemon not reachable: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(timeout_s))).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    writeln!(s, "{cmd}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| format!("no reply: {e}"))?;
    if line.trim().is_empty() {
        return Err("daemon closed the connection".into());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("bad reply: {e}"))
}

/// Eine Antwort mit `ok:false` wird zum Fehler (Text und optional Feldpfad).
pub fn reply_error(v: &Value) -> Option<(String, Option<String>)> {
    if v.get("ok").and_then(Value::as_bool) == Some(false) {
        let msg = v.get("error").and_then(Value::as_str).unwrap_or("command failed").to_string();
        let field = v.get("field").and_then(Value::as_str).map(str::to_string);
        return Some((msg, field));
    }
    None
}
