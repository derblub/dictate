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

//! Daemon-Seite des Panels der Textaktionen in `dictate-ui` (PLAN-wayland.md 6d, Befund 10): alle `act_*`-Aufrufe
//! außer `act_run` laufen der Reihe nach in einem eigenen Thread (ein `act_clear` trifft nie eine neuere Markierung);
//! jedes `act_run` hat einen eigenen Thread mit eigener Verbindung und meldet die gestreamten Stücke. Ergebnisse gehen
//! als [`Msg::Act`] an den Hauptthread; Antworten einer älteren Öffnung (`gen`) bzw. Aktion (`run_gen`) verwirft der
//! Automat (`ui::panel`).
//!
//! Die Markierung liest der Ausgabe-Thread (`primary.rs`, nur PRIMARY); Copy legt das Ergebnis über dessen
//! Data-Control-Gerät in CLIPBOARD (Entscheidung 12: ohne Hinweis). Quellen der Palette (W5 Block D): CLIPBOARD liest
//! der Ausgabe-Thread nur für die Quelle `clipboard` (Absicht `explain_clipboard`, Lesart L8; einzige Stelle hier in
//! [`grab`]), die Frage (`question`) kommt aus dem Panel. Markierung, Zwischenablage, Frage, Antworten und kopierter
//! Text gehen nie ins Log und nie in argv; gesendete und gelesene Zeilen werden überschrieben.

use crate::actions::store::{wipe, SecretText, Source};
use crate::link::Msg;
use crate::output::OutTx;
use crate::protocol::Request;
use crate::sock_client::{wipe_value, Conn, ReadErr, ReplyLine};
use crate::ui::panel::{self, GrabInfo, RunEv, DAEMON_DOWN};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Frist für die Antwort auf `act_grab`, `act_result`, `act_clear`, `act_cancel`
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Höchstens so lange wartet das Greifen auf den Ausgabe-Thread (ein laufender Diktat-Auftrag geht vor)
const READ_WAIT: Duration = Duration::from_secs(10);
/// Höchstens so lange wartet Copy auf den Ausgabe-Thread
const COPY_WAIT: Duration = Duration::from_secs(10);
/// Zuschlag auf `actions.timeout_ms` für `act_run` (wie `ACT_RUN_MARGIN` im CLI)
const RUN_MARGIN: Duration = Duration::from_secs(5);
/// Höchste zulässige `actions.timeout_ms`, falls der Daemon keine nennt
const RUN_MAX: Duration = Duration::from_millis(crate::config::ACTIONS_MAX_TIMEOUT_MS);

/// Nachrichten an den Hauptthread
pub enum ActMsg {
    /// Fokus beim Öffnen gemerkt (Hilfsthread vor dem Zeigen des Panels)
    Opened(panel::Target),
    Grabbed {
        gen: u64,
        res: Result<GrabInfo, String>,
    },
    Run {
        gen: u64,
        run_gen: u64,
        ev: RunEv,
    },
    Copied {
        gen: u64,
        block: Option<usize>,
        res: Result<u64, String>,
    },
}

enum Job {
    Grab { gen: u64, app: Option<String>, source: Source, question: SecretText },
    Clear,
    DiagClear,
    Cancel { id: String },
    Copy { gen: u64, id: String, block: Option<usize> },
}

/// Griff des Hauptthreads
pub struct Act {
    tx: std::sync::mpsc::Sender<Job>,
    /// `gen` der aktuellen Öffnung: ein Greifen einer älteren wird gar nicht erst begonnen
    gen: Arc<AtomicU64>,
    sock: PathBuf,
    back: async_channel::Sender<Msg>,
}

impl Act {
    pub fn spawn(sock: PathBuf, out: OutTx, back: async_channel::Sender<Msg>) -> Act {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let gen = Arc::new(AtomicU64::new(0));
        let (g2, s2, b2) = (gen.clone(), sock.clone(), back.clone());
        std::thread::Builder::new()
            .name("act".into())
            .spawn(move || {
                for job in rx {
                    if !worker(job, &s2, &out, &g2, &b2) {
                        return;
                    }
                }
            })
            .expect("thread");
        Act { tx, gen, sock, back }
    }

    /// Aktuelle Öffnung (nach jedem Schritt des Automaten)
    pub fn set_gen(&self, gen: u64) {
        self.gen.store(gen, Ordering::SeqCst);
    }

    /// `act_grab` für `source`; `question` nur bei [`Source::Question`] (sonst leer)
    pub fn grab(&self, gen: u64, app: Option<String>, source: Source, question: SecretText) {
        let _ = self.tx.send(Job::Grab { gen, app, source, question });
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Job::Clear);
    }

    /// Diagnose-Sitzung im Daemon löschen (Sperre); in derselben Reihe wie die `act_*`-Aufrufe
    pub fn diag_clear(&self) {
        let _ = self.tx.send(Job::DiagClear);
    }

    pub fn cancel(&self, id: String) {
        let _ = self.tx.send(Job::Cancel { id });
    }

    pub fn copy(&self, gen: u64, id: String, block: Option<usize>) {
        let _ = self.tx.send(Job::Copy { gen, id, block });
    }

    /// `act_run` in einem eigenen Thread
    pub fn run(&self, gen: u64, run_gen: u64, id: String, action: String) {
        let (sock, back) = (self.sock.clone(), self.back.clone());
        let spawned = std::thread::Builder::new().name("act-run".into()).spawn(move || {
            let t = Instant::now();
            let mut deltas = 0usize;
            let ev = run(&sock, &id, &action, &mut |d| {
                deltas += 1;
                back.send_blocking(Msg::Act(ActMsg::Run { gen, run_gen, ev: RunEv::Delta(d) })).is_ok()
            });
            info!("act_run {run_gen}: {} after {} ms, {deltas} deltas", ev_name(&ev), t.elapsed().as_millis());
            let _ = back.send_blocking(Msg::Act(ActMsg::Run { gen, run_gen, ev }));
        });
        if let Err(e) = spawned {
            warn!("act_run: cannot start a thread: {e}");
            let _ = self.back.send_blocking(Msg::Act(ActMsg::Run { gen, run_gen, ev: RunEv::Error("Cannot start the action".into()) }));
        }
    }
}

/// Name eines Ergebnisses fürs Log (ohne Text)
fn ev_name(ev: &RunEv) -> &'static str {
    match ev {
        RunEv::Delta(_) => "delta",
        RunEv::Result { .. } => "result",
        RunEv::Cancelled => "cancelled",
        RunEv::Error(_) => "error",
        RunEv::Ended { .. } => "ended",
    }
}

/// Eine Anfrage, eine Antwort; `Ok(None)`: kein Daemon
fn request(sock: &std::path::Path, line: &str) -> Result<Option<ReplyLine>, String> {
    let Some(mut c) = Conn::open(sock).map_err(|e| format!("{e:#}"))? else { return Ok(None) };
    c.send(line).map_err(|e| format!("{e:#}"))?;
    c.reply(Instant::now() + REPLY_TIMEOUT, &mut |_, _| Ok(())).map(Some).map_err(|e| e.message())
}

/// Ein Auftrag; `false`: der Hauptthread ist weg
fn worker(job: Job, sock: &std::path::Path, out: &OutTx, gen: &AtomicU64, back: &async_channel::Sender<Msg>) -> bool {
    match job {
        Job::Grab { gen: g, app, source, question } => {
            if gen.load(Ordering::SeqCst) != g {
                debug!("act_grab skipped (panel closed before it started)");
                return true;
            }
            let res = grab(sock, out, app, source, question);
            if res.is_ok() && gen.load(Ordering::SeqCst) != g {
                // währenddessen geschlossen oder gesperrt (deren `act_clear` lief vielleicht schon): die eben angelegte
                // Sitzung nicht im Daemon liegen lassen
                debug!("act_grab finished after the panel closed: clearing");
                simple(sock, &Request::ActClear);
            }
            back.send_blocking(Msg::Act(ActMsg::Grabbed { gen: g, res })).is_ok()
        }
        Job::Clear => {
            simple(sock, &Request::ActClear);
            true
        }
        Job::DiagClear => {
            simple(sock, &Request::DiagClear);
            true
        }
        Job::Cancel { id } => {
            simple(sock, &Request::ActCancel { id });
            true
        }
        Job::Copy { gen: g, id, block } => {
            // wie beim Greifen: nach Schließen oder Sperre nichts mehr kopieren
            let res = if gen.load(Ordering::SeqCst) == g {
                copy(sock, out, &id, block, &|| gen.load(Ordering::SeqCst) == g)
            } else {
                Err("The panel was closed".into())
            };
            back.send_blocking(Msg::Act(ActMsg::Copied { gen: g, block, res })).is_ok()
        }
    }
}

fn simple(sock: &std::path::Path, req: &Request) {
    match request(sock, &req.to_line()) {
        Ok(Some(r)) if r.ok() => {}
        Ok(Some(r)) => debug!("act: {}", r.error()),
        Ok(None) => debug!("act: no daemon"),
        Err(e) => debug!("act: {e}"),
    }
}

/// Text der Quelle lesen und übergeben: PRIMARY, CLIPBOARD (nur hier und nur für die Quelle `clipboard`) oder die
/// Frage aus dem Panel. Auch ein misslungenes Lesen ersetzt die vorige Sitzung im Daemon (`act_clear`). Für die
/// Palette geht kein `app` mit (Replace gibt es dort nicht; der Daemon setzt `replace_ok` nur für die Markierung)
fn grab(sock: &std::path::Path, out: &OutTx, app: Option<String>, source: Source, question: SecretText) -> Result<GrabInfo, String> {
    let t = Instant::now();
    let read = match source {
        Source::Selection => out.read_primary(READ_WAIT),
        Source::Clipboard => out.read_clipboard(READ_WAIT),
        Source::Question => Ok(question),
    };
    let text = match read {
        Ok(t) => t,
        Err(e) => {
            simple(sock, &Request::ActClear);
            return Err(e);
        }
    };
    let t_read = t.elapsed();
    let preview = panel::preview(text.as_str());
    let chars = text.as_str().chars().count();
    let app = if source == Source::Selection { app } else { None };
    let req = Request::ActGrab { text, app, source };
    let mut line = req.to_line();
    drop(req); // überschreibt den Text
    let r = request(sock, &line);
    wipe(&mut line);
    let r = match r {
        Ok(Some(r)) => r,
        Ok(None) => return Err(DAEMON_DOWN.into()),
        Err(e) => return Err(e),
    };
    info!("act_grab ({}): {chars} chars, read in {} ms, answered after {} ms", source.as_str(), t_read.as_millis(), t.elapsed().as_millis());
    if !r.ok() {
        return Err(r.error().to_string());
    }
    GrabInfo::from_json(&r.v, preview.as_str().to_string()).ok_or_else(|| "Unexpected answer from dictate".to_string())
}

/// Zeitgrenze der Textaktionen aus dem Daemon (`actions.timeout_ms`)
fn run_limit(c: &mut Conn) -> Result<Duration, ReadErr> {
    c.send(&Request::GetConfig.to_line())?;
    let r = c.reply(Instant::now() + REPLY_TIMEOUT, &mut |_, _| Ok(()))?;
    Ok(r.v["config"]["actions"]["timeout_ms"].as_u64().map(Duration::from_millis).unwrap_or(RUN_MAX))
}

/// `act_run` mit gestreamten Stücken; `on_delta` liefert `false`, wenn niemand mehr zuhört
fn run(sock: &std::path::Path, id: &str, action: &str, on_delta: &mut dyn FnMut(String) -> bool) -> RunEv {
    let mut c = match Conn::open(sock) {
        Ok(Some(c)) => c,
        Ok(None) => return RunEv::Ended { daemon_down: true },
        Err(e) => return RunEv::Error(format!("{e:#}")),
    };
    let limit = match run_limit(&mut c) {
        Ok(l) => l,
        Err(e) => return RunEv::Error(e.message()),
    };
    if let Err(e) = c.send(&Request::ActRun { id: id.to_string(), action: action.to_string() }.to_line()) {
        return RunEv::Error(format!("{e:#}"));
    }
    let mut listening = true;
    let reply = c.reply(Instant::now() + limit + RUN_MARGIN, &mut |_, d| {
        if listening {
            listening = on_delta(d.to_string());
        }
        Ok(())
    });
    let mut r = match reply {
        Ok(r) => r,
        Err(ReadErr::Closed) => return RunEv::Ended { daemon_down: false },
        Err(e) => {
            if matches!(e, ReadErr::Timeout(_)) {
                // der Daemon soll nicht für niemanden weiterrechnen
                simple(sock, &Request::ActCancel { id: id.to_string() });
            }
            return RunEv::Error(e.message());
        }
    };
    // Lesart in `ui::panel` (gegen echte Antworten des Daemons getestet, tests/daemon_actions.rs); den Rest der Zeile
    // überschreibt `ReplyLine` beim Verwerfen
    RunEv::from_reply(&mut r.v)
}

/// `act_result` holen und in CLIPBOARD legen; mit `block` nur dieser Codeblock (Regeln `actions::markdown`). `current`:
/// gehört der Auftrag noch zur offenen Sitzung (vor dem Setzen der Zwischenablage erneut geprüft)
fn copy(sock: &std::path::Path, out: &OutTx, id: &str, block: Option<usize>, current: &dyn Fn() -> bool) -> Result<u64, String> {
    let mut r = match request(sock, &Request::ActResult { id: id.to_string() }.to_line()) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(DAEMON_DOWN.into()),
        Err(e) => return Err(e),
    };
    if !r.ok() {
        return Err(r.error().to_string());
    }
    let mut text = match r.v["text"].take() {
        serde_json::Value::String(s) => s,
        _ => String::new(),
    };
    wipe_value(&mut r.v);
    drop(r);
    if let Some(n) = block {
        let mut blocks = crate::actions::markdown::code_blocks(&text);
        let picked = n.checked_sub(1).filter(|i| *i < blocks.len()).map(|i| std::mem::take(&mut blocks[i]));
        for b in &mut blocks {
            wipe(b);
        }
        wipe(&mut text);
        match picked {
            Some(t) => text = t,
            None => return Err(format!("No code block {n}")),
        }
    }
    let chars = text.chars().count() as u64;
    let text = SecretText::new(text);
    if !current() {
        return Err("The panel was closed".into());
    }
    out.copy(text, COPY_WAIT).map_err(|e| format!("Cannot copy: {e}"))?;
    info!("copied {chars} chars{}", block.map(|b| format!(" (code block {b})")).unwrap_or_default());
    Ok(chars)
}

/// Fokus beim Öffnen im Hilfsthread merken (niri kann bis 1,5 s brauchen; der Hauptthread wartet nie)
pub fn capture_focus(back: async_channel::Sender<Msg>) {
    let spawned = std::thread::Builder::new().name("act-focus".into()).spawn(move || {
        let t = Instant::now();
        let f = crate::niri::focused();
        debug!("act: focus captured in {} ms ({})", t.elapsed().as_millis(), if f.is_some() { "known" } else { "unknown" });
        let target = panel::Target { window: f.as_ref().map(|f| f.id), app_id: f.and_then(|f| f.app_id) };
        let _ = back.send_blocking(Msg::Act(ActMsg::Opened(target)));
    });
    if let Err(e) = spawned {
        warn!("act: cannot start a thread: {e}");
    }
}

/// Beim Beenden mit offenem Panel: Sitzung im Daemon sofort löschen (der Thread lebt danach nicht mehr)
pub fn clear_now(sock: &std::path::Path) {
    simple(sock, &Request::ActClear);
}

impl Act {
    /// Sperre: Markierung, Antwort und Diagnose im Daemon sofort löschen, auf eigenen Verbindungen in einem Hilfsthread,
    /// nicht hinter einem Greifen oder Copy in der Reihe (die bis zu 20 s warten können; Durchsicht W3 M1). Ein
    /// Greifen, das danach noch fertig wird, räumt selbst auf (`worker`, gen-Vergleich)
    pub fn clear_all_now(&self) {
        let sock = self.sock.clone();
        let spawned = std::thread::Builder::new().name("act-lock-clear".into()).spawn(move || {
            simple(&sock, &Request::ActClear);
            simple(&sock, &Request::DiagClear);
        });
        if let Err(e) = spawned {
            warn!("act: cannot start a thread: {e}; clearing in the queue");
            self.clear();
            self.diag_clear();
        }
    }
}
