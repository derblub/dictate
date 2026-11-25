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

//! Daemon-Seite der Befehlspalette in `dictate-ui` (PLAN-wayland.md 6g, Block C, Teil C2): zwei Bahnen wie `diag.rs`,
//! je ein Thread, jede Anfrage auf eigener Verbindung. **Resolve** (`pal_resolve`, Frist [`RESOLVE_TIMEOUT`], Lesart L3:
//! der erste Aufruf liest Programme und Projekte von der Platte) und **Ask** (`pal_ask`, Frist `palette.timeout_ms` plus
//! [`ASK_RESERVE`]) laufen getrennt: eine lokale Antwort darf kommen, während die KI rechnet (`ui::cmdpal::Lane`).
//!
//! **Genau eine Antwort je Auftrag:** jeder Auftrag meldet sich mit [`PalMsg::Reply`], auch ein übersprungener (altes
//! `gen`, vorher aufgelegt: [`CallErr::Cancelled`]). Der Automat schickt die nächste lokale Anfrage erst nach der Antwort
//! auf die laufende; fehlte eine, nähme das Panel bis zum nächsten Öffnen keine Eingabe mehr an. Antworten eines alten
//! `gen` verwirft der Automat.
//!
//! **Auflegen** ([`PalIo::hangup`]) geht nicht durch die Reihe, sondern direkt vom Hauptthread auf einen Klon des Sockets
//! des laufenden Aufrufs (`shutdown(Both)`, Muster `diag.rs`): der Daemon sieht das Auflegen und bricht ab. Ein Auftrag,
//! der noch nicht begonnen hat, wird gemerkt und übersprungen. Das Schließen legt auch ein laufendes `pal_resolve` auf
//! (sonst wartete die erste Anfrage der nächsten Öffnung bis zu 10 s dahinter).
//!
//! Dazu zwei Hilfsthreads: **Öffnen** ([`PalIo::open`]: Fokus und Kontext aus niri, bevor das Panel liegt, `pal_host`)
//! und **Ausführen** ([`PalIo::execute`]: `ui::pal_exec::execute` mit dem Wirt über niri, `pal_chosen` auf eigener
//! Verbindung erst nach Erfolg, Lesart L7/L14).
//!
//! Eingabe, Kontext (Fenstertitel, Namen), Kandidaten, Antworten der KI und Ziel-IDs nie loggen, nie in argv: gesendete
//! Zeilen werden überschrieben, gelesene Werte reisen in [`Wiped`]. Ins Log nur Bahn, Dauer, Art des Fehlers, Absicht und
//! Art des Ergebnisses.

use crate::actions::store::{wipe, SecretText};
use crate::diag::Wiped;
use crate::link::Msg;
use crate::pal_host::NiriHost;
use crate::protocol::{Request, SessionEnv};
use crate::sock_client::{Conn, ReadErr};
use crate::ui::cmdpal::Lane;
use crate::ui::diag_panel::CallErr;
use crate::ui::pal_exec::{self, Candidate, ExecCtx, Outcome, Programs};
use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Frist von `pal_resolve` (L3)
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Reserve über `palette.timeout_ms` für `pal_ask` (der Daemon meldet seine eigene Frist vorher)
pub const ASK_RESERVE: Duration = Duration::from_secs(10);
/// Frist von `pal_chosen` (nur Häufigkeit; ein Fehler ändert nichts an der Ausführung)
const CHOSEN_TIMEOUT: Duration = Duration::from_secs(2);

/// Herkunft einer Übergabe an ein anderes Panel (`Outcome::Handoff`): das Fenster vor dem Öffnen der Palette und die
/// Ziel-ID für das spätere `pal_chosen`. Ohne `Debug` (die Ziel-ID gehört nie ins Log)
pub struct Origin {
    pub window: Option<u64>,
    pub app_id: Option<String>,
    pub target_id: String,
}

/// Nachrichten an den Hauptthread
pub enum PalMsg {
    /// Fokus (niri-ID und `app_id`) und Kontext beim Öffnen sind da (`context: None`: niri nicht erreichbar, ohne
    /// Kontext öffnen)
    Opened { prior_focus: Option<crate::pal_host::Focus>, context: Option<SecretText> },
    /// Antwort auf `pal_resolve` bzw. `pal_ask`
    Reply { lane: Lane, gen: u64, seq: u64, res: Result<Wiped, CallErr> },
    /// niri bzw. das Programm nahm die Ausführung `run` an (vor `pal_chosen`; schließt einen Abgang, Entscheidung 17)
    Accepted { run: u64 },
    /// Ausführung `run` fertig (`intent` ist der Name der Absicht, nie ein Ziel); `origin` nur bei `Outcome::Handoff`
    Executed { run: u64, intent: &'static str, outcome: Outcome, origin: Option<Origin> },
}

fn lane_name(l: Lane) -> &'static str {
    match l {
        Lane::Resolve => "pal_resolve",
        Lane::Ask => "pal_ask",
    }
}

struct Job {
    gen: u64,
    seq: u64,
    input: SecretText,
    context: Option<SecretText>,
    timeout: Duration,
}

/// Was gerade läuft, für das Auflegen vom Hauptthread (wie `diag.rs`)
#[derive(Default)]
struct Line {
    /// Laufender Aufruf ab seinem Beginn und, sobald die Verbindung steht, ein Klon seines Sockets
    current: Option<(u64, Option<UnixStream>)>,
    current_hung: bool,
    /// Höchste `seq`, die der Thread begonnen hat (Aufträge kommen aufsteigend)
    started: u64,
    /// Aufgelegt, bevor sie begannen
    hung: HashSet<u64>,
}

fn lock(l: &Mutex<Line>) -> MutexGuard<'_, Line> {
    l.lock().unwrap_or_else(|e| e.into_inner())
}

/// Trägt den laufenden Aufruf auf jedem Ausgang aus (der Klon des Sockets bliebe sonst offen)
struct Registered<'a>(&'a Mutex<Line>);

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut l = lock(self.0);
        l.current = None;
        l.current_hung = false;
    }
}

/// Eine Bahn: ein Thread, Aufträge der Reihe nach, Auflegen am Thread vorbei
struct Track {
    tx: std::sync::mpsc::Sender<Job>,
    gen: Arc<AtomicU64>,
    line: Arc<Mutex<Line>>,
}

impl Track {
    /// `back` gibt eine Antwort an den Hauptthread; `false`: niemand hört mehr zu (der Thread endet)
    fn spawn(lane: Lane, sock: PathBuf, back: impl Fn(PalMsg) -> bool + Send + 'static) -> Track {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let gen = Arc::new(AtomicU64::new(0));
        let line = Arc::new(Mutex::new(Line::default()));
        let (g2, l2) = (gen.clone(), line.clone());
        std::thread::Builder::new()
            .name(if lane == Lane::Resolve { "pal-resolve" } else { "pal-ask" }.into())
            .spawn(move || {
                for job in rx {
                    if !worker(lane, job, &sock, &g2, &l2, &back) {
                        return;
                    }
                }
            })
            .expect("thread");
        Track { tx, gen, line }
    }

    fn set_gen(&self, gen: u64) {
        self.gen.store(gen, Ordering::SeqCst);
    }

    fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    /// Auftrag `seq` auflegen: läuft er, sofort; hat er noch nicht begonnen, wird er übersprungen; sonst nichts
    fn hangup(&self, seq: u64) {
        let mut l = lock(&self.line);
        match &l.current {
            Some((s, sock)) if *s == seq => {
                // ohne Klon: die Verbindung geht gerade auf; `exec` sieht `current_hung` danach und legt selbst auf
                if let Some(Err(e)) = sock.as_ref().map(|s| s.shutdown(std::net::Shutdown::Both)) {
                    debug!("palette: hangup: {e}");
                }
                l.current_hung = true;
            }
            _ if seq > l.started => {
                l.hung.insert(seq);
            }
            _ => {}
        }
    }
}

/// Ein Auftrag; `false`: der Hauptthread ist weg
fn worker(lane: Lane, job: Job, sock: &Path, gen: &AtomicU64, line: &Mutex<Line>, back: &dyn Fn(PalMsg) -> bool) -> bool {
    let (g, seq) = (job.gen, job.seq);
    let name = lane_name(lane);
    let res = if gen.load(Ordering::SeqCst) != g {
        // Panel inzwischen zu (oder neu offen): gar nicht beginnen, aber antworten (Modulkopf)
        let mut l = lock(line);
        l.started = l.started.max(seq);
        l.hung.remove(&seq);
        debug!("palette {name} skipped (panel closed before it started)");
        Err(CallErr::Cancelled)
    } else {
        let t = Instant::now();
        let res = exec(lane, job, sock, line);
        let ms = t.elapsed().as_millis();
        // lokale Anfragen kommen je Tastendruck: nur im Debug-Log
        match (&res, lane) {
            (Ok(_), Lane::Resolve) => debug!("palette {name}: ok after {ms} ms"),
            (Ok(_), Lane::Ask) => info!("palette {name}: ok after {ms} ms"),
            (Err(e), _) => info!("palette {name}: {} after {ms} ms", e.kind()),
        }
        res
    };
    back(PalMsg::Reply { lane, gen: g, seq, res })
}

/// Aufruf ausführen; Eingabe und Kontext werden mit dem Auftrag überschrieben (`SecretText`)
fn exec(lane: Lane, job: Job, sock: &Path, line: &Mutex<Line>) -> Result<Wiped, CallErr> {
    let Job { seq, input, context, timeout, .. } = job;
    let _registered = {
        let mut l = lock(line);
        l.started = l.started.max(seq);
        if l.hung.remove(&seq) {
            return Err(CallErr::Cancelled);
        }
        l.current = Some((seq, None));
        l.current_hung = false;
        Registered(line)
    };
    let mut conn = match Conn::open(sock) {
        Ok(Some(c)) => c,
        Ok(None) => return Err(CallErr::DaemonDown),
        Err(e) => return Err(CallErr::Failed(format!("{e:#}"))),
    };
    let handle = conn.hangup_handle().map_err(|e| CallErr::Failed(format!("Cannot reach the daemon: {e}")))?;
    {
        let mut l = lock(line);
        // aufgelegt, während die Verbindung aufging: nichts senden
        if l.current_hung {
            return Err(CallErr::Cancelled);
        }
        l.current = Some((seq, Some(handle)));
    }
    let hung = || lock(line).current_hung;
    let env = SessionEnv::from_process();
    let req = match lane {
        Lane::Resolve => Request::PalResolve { input, context, env },
        Lane::Ask => Request::PalAsk { input, context, env },
    };
    let mut sent = req.to_line();
    drop(req); // überschreibt Eingabe und Kontext
    let r = conn.send(&sent);
    wipe(&mut sent);
    if let Err(e) = r {
        return Err(if hung() { CallErr::Cancelled } else { CallErr::Failed(format!("{e:#}")) });
    }
    let mut r = match conn.reply(Instant::now() + timeout, &mut |_, _| Ok(())) {
        Ok(r) => r,
        Err(_) if hung() => return Err(CallErr::Cancelled),
        Err(ReadErr::Timeout(_)) => return Err(CallErr::Timeout),
        Err(ReadErr::Closed) => return Err(CallErr::DaemonDown),
        Err(e @ ReadErr::Other(_)) => return Err(CallErr::Failed(e.message())),
    };
    if !r.ok() {
        return Err(CallErr::from_reply(&r.v));
    }
    Ok(Wiped::new(std::mem::take(&mut r.v)))
}

/// Griff des Hauptthreads
pub struct PalIo {
    resolve: Track,
    ask: Track,
    sock: PathBuf,
    /// `palette.timeout_ms` aus der Config des Daemons (`link::send_config`)
    ask_timeout: Duration,
    programs: Arc<Programs>,
    back: async_channel::Sender<Msg>,
}

impl PalIo {
    pub fn spawn(sock: PathBuf, programs: Programs, back: async_channel::Sender<Msg>) -> PalIo {
        let to_main = |tx: async_channel::Sender<Msg>| move |m| tx.send_blocking(Msg::Pal(m)).is_ok();
        PalIo {
            resolve: Track::spawn(Lane::Resolve, sock.clone(), to_main(back.clone())),
            ask: Track::spawn(Lane::Ask, sock.clone(), to_main(back.clone())),
            sock,
            ask_timeout: Duration::from_millis(crate::config::PALETTE_DEFAULT_TIMEOUT_MS) + ASK_RESERVE,
            programs: Arc::new(programs),
            back,
        }
    }

    /// Aktuelle Öffnung (vor und nach jedem Schritt des Automaten)
    pub fn set_gen(&self, gen: u64) {
        self.resolve.set_gen(gen);
        self.ask.set_gen(gen);
    }

    /// Suchpfad der Programme (derselbe wie beim Ausführen): für den Start des Einstellungsfensters aus einem Panel
    pub fn programs(&self) -> &Programs {
        &self.programs
    }

    /// `palette.timeout_ms` (im Bereich der Config geklemmt)
    pub fn set_ask_timeout_ms(&mut self, ms: u64) {
        let (lo, hi) = crate::config::PALETTE_TIMEOUT_RANGE;
        self.ask_timeout = Duration::from_millis(ms.clamp(lo, hi)) + ASK_RESERVE;
    }

    pub fn request(&self, lane: Lane, gen: u64, seq: u64, input: String, context: Option<SecretText>) {
        let (track, timeout) = match lane {
            Lane::Resolve => (&self.resolve, RESOLVE_TIMEOUT),
            Lane::Ask => (&self.ask, self.ask_timeout),
        };
        track.send(Job { gen, seq, input: SecretText::new(input), context, timeout });
    }

    pub fn hangup(&self, lane: Lane, seq: u64) {
        match lane {
            Lane::Resolve => self.resolve.hangup(seq),
            Lane::Ask => self.ask.hangup(seq),
        }
    }

    /// Fokus und Kontext im Hilfsthread holen (niri kann bis 1,5 s brauchen), Ergebnis als [`PalMsg::Opened`]
    pub fn open(&self) {
        let tx = self.back.clone();
        let spawned = std::thread::Builder::new().name("pal-open".into()).spawn(move || {
            let t = Instant::now();
            let (prior_focus, context) = match NiriHost::session() {
                Ok(h) => h.opened(),
                Err(e) => (None, Err(e)),
            };
            // ins Log nur, ob es gelang, und die Größe (nie Titel oder Namen)
            match &context {
                Ok(c) => debug!(
                    "palette: context of {} bytes in {} ms (focus {})",
                    c.len(),
                    t.elapsed().as_millis(),
                    if prior_focus.is_some() { "known" } else { "unknown" }
                ),
                Err(_) => info!("palette: no context from niri, opening without it"),
            }
            let context = context.ok().map(SecretText::new);
            let _ = tx.send_blocking(Msg::Pal(PalMsg::Opened { prior_focus, context }));
        });
        if let Err(e) = spawned {
            warn!("palette: cannot start a thread: {e}");
            let _ = self.back.send_blocking(Msg::Pal(PalMsg::Opened { prior_focus: None, context: None }));
        }
    }

    /// Kandidaten ausführen (nach dem Schließen des Panels, im Abgang davor), in einem Arbeiter: Kurzläufer warten bis
    /// 10 s. `run` kommt mit `Accepted` (sobald niri bzw. das Programm annahm, vor `pal_chosen`) und `Executed` zurück.
    /// `prior_app`: `app_id` des Fensters `ctx.prior_focus` (für eine Übergabe an die Textaktionen)
    pub fn execute(&self, run: u64, cand: Candidate, ctx: ExecCtx, prior_app: Option<String>) {
        let (tx, programs, sock) = (self.back.clone(), self.programs.clone(), self.sock.clone());
        let spawned = std::thread::Builder::new().name("pal-exec".into()).spawn(move || {
            let intent = cand.intent.name();
            let mut accepted = || {
                let _ = tx.send_blocking(Msg::Pal(PalMsg::Accepted { run }));
                chosen(&sock, intent, &cand.target_id)
            };
            let outcome = match NiriHost::session() {
                Ok(mut host) => pal_exec::execute(&cand, &ctx, &programs, &mut host, &mut accepted),
                Err(_) => Outcome::Failed("niri is not reachable".into()),
            };
            let origin =
                matches!(outcome, Outcome::Handoff(_)).then(|| Origin { window: ctx.prior_focus, app_id: prior_app, target_id: cand.target_id.clone() });
            let _ = tx.send_blocking(Msg::Pal(PalMsg::Executed { run, intent, outcome, origin }));
        });
        if let Err(e) = spawned {
            warn!("palette: cannot start a thread: {e}");
            let _ = self.back.send_blocking(Msg::Pal(PalMsg::Executed {
                run,
                intent: "-",
                outcome: Outcome::Failed("Cannot start the command".into()),
                origin: None,
            }));
        }
    }

    /// `pal_chosen` nach einer Übergabe, sobald das Ziel-Panel Erfolg meldet (Lesart L34), in einem Hilfsthread (der
    /// Hauptthread wartet nie auf den Daemon)
    pub fn chosen_later(&self, intent: &'static str, target_id: String) {
        let sock = self.sock.clone();
        if let Err(e) = std::thread::Builder::new().name("pal-chosen".into()).spawn(move || chosen(&sock, intent, &target_id)) {
            warn!("palette: cannot start a thread: {e}");
        }
    }
}

/// `pal_chosen` auf eigener Verbindung (Häufigkeit im Daemon); ins Log nur die Art eines Fehlers, nie das Ziel
fn chosen(sock: &Path, intent: &'static str, target_id: &str) {
    let req = Request::PalChosen { intent: intent.to_string(), target_id: target_id.to_string() };
    let mut line = req.to_line();
    drop(req);
    let r = match Conn::open(sock) {
        Ok(Some(mut c)) => match c.send(&line) {
            Ok(()) => match c.reply(Instant::now() + CHOSEN_TIMEOUT, &mut |_, _| Ok(())) {
                Ok(r) if r.ok() => Ok(()),
                Ok(_) => Err("refused"),
                Err(ReadErr::Timeout(_)) => Err("no answer in time"),
                Err(_) => Err("reading the answer failed"),
            },
            Err(_) => Err("sending failed"),
        },
        Ok(None) => Err("daemon not running"),
        Err(_) => Err("cannot connect"),
    };
    wipe(&mut line);
    match r {
        Ok(()) => debug!("palette: pal_chosen {intent} recorded"),
        Err(why) => info!("palette: pal_chosen {intent}: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    /// Fake-Daemon: liest die Zeile, meldet Befehl und Eingabe; Eingabe `hang` wartet, bis der Client auflegt, `slow`
    /// antwortet nie (Frist), `off` wie die ausgeschaltete Palette
    struct Fake {
        dir: PathBuf,
        seen: mpsc::Receiver<String>,
    }

    impl Fake {
        fn start() -> Fake {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("dictate-ui-pal-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            std::fs::create_dir_all(&dir).unwrap();
            let listener = UnixListener::bind(dir.join("sock")).unwrap();
            let (tx, seen) = mpsc::channel();
            std::thread::spawn(move || {
                for s in listener.incoming() {
                    let Ok(s) = s else { return };
                    let tx = tx.clone();
                    std::thread::spawn(move || serve(s, &tx));
                }
            });
            Fake { dir, seen }
        }

        fn sock(&self) -> PathBuf {
            self.dir.join("sock")
        }

        fn next(&self) -> String {
            self.seen.recv_timeout(Duration::from_secs(5)).expect("fake daemon saw nothing")
        }

        fn quiet(&self) -> bool {
            self.seen.recv_timeout(Duration::from_millis(200)).is_err()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn serve(mut s: UnixStream, seen: &mpsc::Sender<String>) {
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut l = String::new();
        if r.read_line(&mut l).unwrap_or(0) == 0 {
            return;
        }
        let v: Value = serde_json::from_str(l.trim()).unwrap();
        let cmd = v["cmd"].as_str().unwrap_or("").to_string();
        let input = v["input"].as_str().unwrap_or("").to_string();
        let ctx = if v["context"].is_object() { "ctx" } else { "no-ctx" };
        let _ = seen.send(format!("{cmd} {input} {ctx}"));
        let reply = match input.as_str() {
            "hang" | "slow" => {
                // wartet bis zum Auflegen (bzw. bis die Frist des Clients abläuft) und meldet es
                let mut rest = Vec::new();
                let _ = r.read_to_end(&mut rest);
                let _ = seen.send("hung up".into());
                return;
            }
            "off" => json!({"ok": false, "error": "The command palette is off (Settings › Palette)"}),
            _ if cmd == "pal_chosen" => json!({"ok": true, "recorded": true}),
            _ => json!({"ok": true, "candidates": [], "confident": false, "web_search_url": "https://example.org/?q={query}"}),
        };
        let _ = writeln!(s, "{reply}");
    }

    fn track(f: &Fake, lane: Lane) -> (Track, mpsc::Receiver<PalMsg>) {
        let (tx, rx) = mpsc::channel();
        (Track::spawn(lane, f.sock(), move |m| tx.send(m).is_ok()), rx)
    }

    fn job(gen: u64, seq: u64, input: &str, timeout: Duration) -> Job {
        Job { gen, seq, input: SecretText::new(input.into()), context: Some(SecretText::new("{\"windows\":[]}".into())), timeout }
    }

    fn reply(rx: &mpsc::Receiver<PalMsg>) -> (Lane, u64, u64, Result<Value, CallErr>) {
        match rx.recv_timeout(Duration::from_secs(5)).expect("no reply") {
            PalMsg::Reply { lane, gen, seq, res } => (lane, gen, seq, res.map(|mut w| w.value_mut().take())),
            _ => panic!("not a reply"),
        }
    }

    #[test]
    fn a_reply_carries_its_lane_gen_and_seq() {
        let f = Fake::start();
        for (lane, cmd) in [(Lane::Resolve, "pal_resolve"), (Lane::Ask, "pal_ask")] {
            let (t, rx) = track(&f, lane);
            t.set_gen(3);
            t.send(job(3, 7, "fire", RESOLVE_TIMEOUT));
            assert_eq!(f.next(), format!("{cmd} fire ctx"));
            let (l, gen, seq, res) = reply(&rx);
            assert_eq!((l, gen, seq), (lane, 3, 7));
            assert!(matches!(res, Ok(v) if v["web_search_url"] == "https://example.org/?q={query}"));
        }
    }

    #[test]
    fn errors_of_the_daemon_arrive_as_failed() {
        let f = Fake::start();
        let (t, rx) = track(&f, Lane::Resolve);
        t.send(job(0, 1, "off", RESOLVE_TIMEOUT));
        f.next();
        assert!(matches!(reply(&rx).3, Err(CallErr::Failed(m)) if m == "The command palette is off (Settings › Palette)"));
    }

    #[test]
    fn hanging_up_a_blocked_call_answers_cancelled() {
        let f = Fake::start();
        let (t, rx) = track(&f, Lane::Ask);
        t.send(job(0, 1, "hang", Duration::from_secs(30)));
        assert_eq!(f.next(), "pal_ask hang ctx");
        t.hangup(1);
        assert_eq!(f.next(), "hung up");
        assert!(matches!(reply(&rx).3, Err(CallErr::Cancelled)));
        // die Bahn läuft weiter
        t.send(job(0, 2, "next", RESOLVE_TIMEOUT));
        assert_eq!(f.next(), "pal_ask next ctx");
        assert_eq!(reply(&rx).2, 2);
    }

    #[test]
    fn hanging_up_before_the_start_skips_the_call_but_answers() {
        let f = Fake::start();
        let (t, rx) = track(&f, Lane::Ask);
        t.send(job(0, 1, "hang", Duration::from_secs(30)));
        f.next();
        // 2 wartet hinter 1; aufgelegt, bevor sie beginnt
        t.send(job(0, 2, "never", RESOLVE_TIMEOUT));
        t.hangup(2);
        t.hangup(1);
        assert_eq!(f.next(), "hung up");
        let r1 = reply(&rx);
        let r2 = reply(&rx);
        assert!(matches!(r1, (_, _, 1, Err(CallErr::Cancelled))));
        assert!(matches!(r2, (_, _, 2, Err(CallErr::Cancelled))));
        assert!(f.quiet(), "a call hung up before its start reached the daemon");
    }

    #[test]
    fn a_job_of_an_old_opening_is_not_started_but_answered() {
        let f = Fake::start();
        let (t, rx) = track(&f, Lane::Resolve);
        t.set_gen(5);
        t.send(job(4, 1, "old", RESOLVE_TIMEOUT));
        assert!(matches!(reply(&rx), (Lane::Resolve, 4, 1, Err(CallErr::Cancelled))));
        assert!(f.quiet());
    }

    #[test]
    fn deadline_and_missing_daemon() {
        let f = Fake::start();
        let (t, rx) = track(&f, Lane::Resolve);
        t.send(job(0, 1, "slow", Duration::from_millis(200)));
        f.next();
        assert!(matches!(reply(&rx).3, Err(CallErr::Timeout)));
        assert_eq!(f.next(), "hung up", "the client closes the connection after its deadline");
        let gone = Fake::start();
        let sock = gone.sock();
        drop(gone);
        let (tx, rx) = mpsc::channel();
        let t = Track::spawn(Lane::Resolve, sock, move |m| tx.send(m).is_ok());
        t.send(job(0, 1, "x", RESOLVE_TIMEOUT));
        assert!(matches!(reply(&rx).3, Err(CallErr::DaemonDown)));
    }

    #[test]
    fn chosen_sends_intent_and_target() {
        let f = Fake::start();
        chosen(&f.sock(), "launch_app", "firefox.desktop");
        assert_eq!(f.next(), "pal_chosen  no-ctx");
        // ohne Daemon: kein Panic, nur ein Eintrag im Log
        chosen(&std::env::temp_dir().join("dictate-ui-pal-none/sock"), "lock", "lock");
    }
}
