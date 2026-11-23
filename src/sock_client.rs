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

//! Client-Seite des Steuer-Sockets: Verbindung und Antwortzeile (mit Zwischenzeilen vor der Antwort).
//!
//! Nur `dictate-ui` bindet diese Datei per Pfad ein (W4/W5 nutzen sie für Diagnose und Palette mit); bis W7 teilten sie
//! sich die CLI-Clients für AwesomeWM. Darum nur `protocol` und `actions::store` als Abhängigkeiten, nie `engine`,
//! `daemon`, `pipeline` oder `http` (`dictate-ui` linkt weder Whisper noch CUDA).

use crate::actions::store::{wipe, SecretText};
use crate::protocol::DELTA_KEY;
use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Verbindung zum Socket; `None`, wenn kein Daemon lauscht (Socket fehlt oder lehnt ab)
pub fn connect(sock: &Path) -> Result<Option<UnixStream>> {
    match UnixStream::connect(sock) {
        Ok(s) => Ok(Some(s)),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("connecting to {}", sock.display())),
    }
}

/// Verbindung für eine Folge von Anfragen
pub struct Conn {
    w: UnixStream,
    r: BufReader<UnixStream>,
}

pub enum ReadErr {
    Timeout(Duration),
    Closed,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for ReadErr {
    fn from(e: anyhow::Error) -> ReadErr {
        ReadErr::Other(e)
    }
}

impl ReadErr {
    pub fn message(&self) -> String {
        match self {
            ReadErr::Timeout(d) => format!("No answer from the daemon within {} s", d.as_secs_f64().ceil() as u64),
            ReadErr::Closed => "The daemon closed the connection without a reply".into(),
            ReadErr::Other(e) => format!("{e:#}"),
        }
    }
}

/// Antwortzeile (mit `"ok"`); die rohe Zeile und die Texte des gelesenen Werts werden beim Freigeben überschrieben, sie
/// können Text enthalten
pub struct ReplyLine {
    pub line: SecretText,
    pub v: Value,
}

/// Überschreibt alle Texte in einem JSON-Wert ([`wipe`]); Schlüssel sind Namen des Protokolls und bleiben.
pub fn wipe_value(v: &mut Value) {
    match v {
        Value::String(s) => wipe(s),
        Value::Array(a) => a.iter_mut().for_each(wipe_value),
        Value::Object(m) => m.values_mut().for_each(wipe_value),
        _ => {}
    }
}

impl Drop for ReplyLine {
    fn drop(&mut self) {
        // die gelesene Antwort kann Bericht, Klartext der Platzhalter (`diag send`) oder eine Markierung enthalten (Review I5)
        wipe_value(&mut self.v);
    }
}

impl ReplyLine {
    pub fn ok(&self) -> bool {
        self.v["ok"] == true
    }
    pub fn error(&self) -> &str {
        self.v["error"].as_str().unwrap_or("command failed")
    }
}

impl Conn {
    pub fn open(sock: &Path) -> Result<Option<Conn>> {
        let Some(s) = connect(sock)? else { return Ok(None) };
        s.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Some(Conn { r: BufReader::new(s.try_clone()?), w: s }))
    }

    pub fn send(&mut self, line: &str) -> Result<()> {
        self.w.write_all(line.as_bytes()).and_then(|_| self.w.write_all(b"\n")).and_then(|_| self.w.flush()).context("sending to the daemon")
    }

    /// Liest bis zur ersten Zeile mit `"ok"` (der Antwort). Davor stehende `{"delta":"…"}`-Zeilen gehen an
    /// `on_delta` (rohe Zeile, Text), andere Zeilen ohne `"ok"` werden übergangen (siehe `protocol`).
    pub fn reply(&mut self, deadline: Instant, on_delta: &mut dyn FnMut(&str, &str) -> std::io::Result<()>) -> Result<ReplyLine, ReadErr> {
        self.reply_with(deadline, &mut |raw, v| match v.get(DELTA_KEY).and_then(|d| d.as_str()) {
            Some(d) => on_delta(raw, d),
            None => Ok(()),
        })
    }

    /// Klon des Sockets: ein anderer Thread kann damit auflegen (`shutdown(Both)`), während dieser blockierend liest; der
    /// Leser bekommt dann [`ReadErr::Closed`], der Daemon sieht das Auflegen und bricht ab (Test
    /// `shutdown_on_a_clone_hangs_up_a_blocked_call` in `tests/daemon_diagnose.rs`)
    pub fn hangup_handle(&self) -> std::io::Result<UnixStream> {
        self.w.try_clone()
    }

    /// Wie [`Conn::reply`], gibt aber jede Zwischenzeile ohne `"ok"` (roh und gelesen) an `on_line` (Fortschritt der
    /// Diagnose: `{"progress":{…}}`). `on_line` darf den Wert herausnehmen; was bleibt, wird danach überschrieben wie die
    /// rohe Zeile (Zwischenzeilen tragen Parameter mit Namen).
    pub fn reply_with(&mut self, deadline: Instant, on_line: &mut dyn FnMut(&str, &mut Value) -> std::io::Result<()>) -> Result<ReplyLine, ReadErr> {
        let total = deadline.saturating_duration_since(Instant::now());
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ReadErr::Timeout(total));
            }
            self.w.set_read_timeout(Some(left)).context("setting the read timeout")?;
            let mut raw = String::new();
            match self.r.read_line(&mut raw) {
                Ok(0) => return Err(ReadErr::Closed),
                Ok(_) => {}
                Err(e) => {
                    wipe(&mut raw);
                    return Err(match e.kind() {
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => ReadErr::Timeout(total),
                        _ => ReadErr::Other(anyhow::Error::new(e).context("reading the daemon's reply")),
                    });
                }
            }
            while raw.ends_with(['\n', '\r']) {
                raw.pop();
            }
            let line = SecretText::new(raw);
            if line.as_str().trim().is_empty() {
                continue;
            }
            let mut v: Value = serde_json::from_str(line.as_str()).map_err(|e| ReadErr::Other(anyhow::anyhow!("bad reply from daemon: {e}")))?;
            if v.get("ok").is_some() {
                return Ok(ReplyLine { line, v });
            }
            let r = on_line(line.as_str(), &mut v);
            wipe_value(&mut v);
            r.context("writing the output")?;
        }
    }
}
