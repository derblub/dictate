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

//! Daemon-Seite des Diagnose-Panels in `dictate-ui` (PLAN-wayland.md 6e Punkt 2, Block B Punkt 1): alle `diag_*`-Aufrufe
//! laufen der Reihe nach in einem eigenen Thread (ein `diag_clear` trifft nie eine neuere Diagnose), jeder auf einer
//! eigenen Verbindung mit der Frist seines Aufrufs (`Call::timeout`). Zwischenzeilen (`{"progress":…}`) und Antworten
//! gehen als [`DiagMsg`] an den Hauptthread, der sie dem Automaten `ui::diag_panel` gibt; dort zählen nur Antworten des
//! aktuellen `gen` und eines wartenden Aufrufs.
//!
//! Aufrufe mit veraltetem `gen` werden gar nicht erst begonnen, außer `diag_cancel` und `diag_clear` (sie kommen beim
//! Schließen mit dem alten `gen`). Auflegen ([`Diag::hangup`]) geht nicht durch die Reihe (es stünde hinter dem
//! blockierten Aufruf), sondern direkt vom Hauptthread auf einen Klon des Sockets des laufenden Aufrufs: `shutdown(Both)`,
//! der blockierte Leser kehrt zurück, der Daemon sieht das Auflegen und bricht ab (`HangupWatch` im Daemon, Test
//! `shutdown_on_a_clone_hangs_up_a_blocked_call`). Ein Aufruf, der noch nicht begonnen hat, wird gemerkt und übersprungen.
//!
//! Copy holt den Bericht mit `diag_copy` und legt ihn über den Ausgabe-Thread in CLIPBOARD (wie `act.rs`); an den
//! Hauptthread geht nur die Zahl der Zeichen. Bericht, Fortschritt mit Parametern, Antworten, Klartext der Platzhalter
//! und Symptom gehen nie ins Log und nie in argv: gesendete Zeilen werden überschrieben, gelesene Werte reisen in
//! [`Wiped`] (beim Freigeben überschrieben). Ins Log nur Name des Aufrufs, Dauer und Art des Fehlers.

use crate::actions::store::{wipe, SecretText};
use crate::output::OutTx;
use crate::sock_client::{wipe_value, Conn, ReadErr};
use crate::ui::boot_hint::Answer;
use crate::ui::diag_panel::{Call, CallErr};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Höchstens so lange wartet Copy auf den Ausgabe-Thread
const COPY_WAIT: Duration = Duration::from_secs(10);

/// Ein gelesener Wert des Daemons; wird beim Freigeben überschrieben (kann Bericht, Namen und Klartext tragen)
pub struct Wiped(Value);

impl Wiped {
    /// Nimmt einen gelesenen Wert in Obhut (auch für `pal.rs`)
    pub fn new(v: Value) -> Wiped {
        Wiped(v)
    }

    pub fn value_mut(&mut self) -> &mut Value {
        &mut self.0
    }
}

impl Drop for Wiped {
    fn drop(&mut self) {
        wipe_value(&mut self.0);
    }
}

/// Nachrichten an den Hauptthread
pub enum DiagMsg {
    /// Zwischenzeile eines Sammelns oder Prüfens
    Progress {
        gen: u64,
        seq: u64,
        line: Wiped,
    },
    Reply {
        gen: u64,
        seq: u64,
        res: Result<Wiped, CallErr>,
    },
}

/// Zwischenablage für Copy (im Binary der Ausgabe-Thread, in Tests ein Fake)
pub trait Clip: Send + 'static {
    fn copy(&self, text: SecretText) -> Result<(), String>;
}

impl Clip for OutTx {
    fn copy(&self, text: SecretText) -> Result<(), String> {
        OutTx::copy(self, text, COPY_WAIT)
    }
}

struct Job {
    gen: u64,
    seq: u64,
    call: Call,
}

/// Was gerade läuft, für das Auflegen vom Hauptthread
#[derive(Default)]
struct Line {
    /// Laufender Aufruf ab seinem Beginn und, sobald die Verbindung steht, ein Klon seines Sockets (ab dem Beginn
    /// eingetragen: ein Auflegen, während die Verbindung aufgeht, ginge sonst verloren)
    current: Option<(u64, Option<UnixStream>)>,
    /// Der laufende Aufruf wurde aufgelegt
    current_hung: bool,
    /// Höchste `seq`, die der Thread begonnen hat (Aufrufe kommen aufsteigend)
    started: u64,
    /// Aufgelegt, bevor sie begannen
    hung: HashSet<u64>,
}

fn lock(l: &Mutex<Line>) -> MutexGuard<'_, Line> {
    l.lock().unwrap_or_else(|e| e.into_inner())
}

/// Trägt den laufenden Aufruf auf jedem Ausgang aus (der Klon des Sockets bliebe sonst offen, und der Daemon sähe das
/// Auflegen nie)
struct Registered<'a>(&'a Mutex<Line>);

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut l = lock(self.0);
        l.current = None;
        l.current_hung = false;
    }
}

/// Griff des Hauptthreads
pub struct Diag {
    tx: std::sync::mpsc::Sender<Job>,
    /// `gen` der aktuellen Öffnung: ein Aufruf einer älteren wird gar nicht erst begonnen
    gen: Arc<AtomicU64>,
    line: Arc<Mutex<Line>>,
}

impl Diag {
    /// `back` gibt eine Nachricht an den Hauptthread; `false`: niemand hört mehr zu (der Thread endet)
    pub fn spawn(sock: PathBuf, clip: impl Clip, back: impl Fn(DiagMsg) -> bool + Send + 'static) -> Diag {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let gen = Arc::new(AtomicU64::new(0));
        let line = Arc::new(Mutex::new(Line::default()));
        let (g2, l2) = (gen.clone(), line.clone());
        std::thread::Builder::new()
            .name("diag".into())
            .spawn(move || {
                for job in rx {
                    if !worker(job, &sock, &clip, &g2, &l2, &back) {
                        return;
                    }
                }
            })
            .expect("thread");
        Diag { tx, gen, line }
    }

    /// Aktuelle Öffnung (vor und nach jedem Schritt des Automaten)
    pub fn set_gen(&self, gen: u64) {
        self.gen.store(gen, Ordering::SeqCst);
    }

    pub fn call(&self, gen: u64, seq: u64, call: Call) {
        let _ = self.tx.send(Job { gen, seq, call });
    }

    /// Aufruf `seq` auflegen: läuft er, sofort (`shutdown` auf dem Klon); hat er noch nicht begonnen, wird er
    /// übersprungen; ist er schon fertig, geschieht nichts
    pub fn hangup(&self, seq: u64) {
        let mut l = lock(&self.line);
        match &l.current {
            Some((s, sock)) if *s == seq => {
                // ohne Klon: die Verbindung geht gerade auf; `exec` sieht `current_hung` danach und legt selbst auf
                if let Some(Err(e)) = sock.as_ref().map(|s| s.shutdown(std::net::Shutdown::Both)) {
                    debug!("diag: hangup: {e}");
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
fn worker(job: Job, sock: &std::path::Path, clip: &dyn Clip, gen: &AtomicU64, line: &Mutex<Line>, back: &dyn Fn(DiagMsg) -> bool) -> bool {
    let Job { gen: g, seq, call } = job;
    let name = call.name();
    // `diag_cancel` und `diag_clear` immer (sie kommen beim Schließen mit dem alten `gen`)
    let always = matches!(call, Call::Cancel { .. } | Call::Clear);
    if !always && gen.load(Ordering::SeqCst) != g {
        let mut l = lock(line);
        l.started = l.started.max(seq);
        l.hung.remove(&seq);
        debug!("diag {name} skipped (panel closed before it started)");
        return true;
    }
    let t = Instant::now();
    let mut progress = 0usize;
    let mut alive = true;
    let current = || gen.load(Ordering::SeqCst) == g;
    let res = exec(seq, &call, sock, clip, line, &current, &mut |v| {
        progress += 1;
        if alive {
            alive = back(DiagMsg::Progress { gen: g, seq, line: v });
        }
    });
    let ms = t.elapsed().as_millis();
    match &res {
        Ok(_) if progress > 0 => info!("diag {name}: ok after {ms} ms, {progress} progress lines"),
        Ok(_) => info!("diag {name}: ok after {ms} ms"),
        Err(e) => info!("diag {name}: {} after {ms} ms", e.kind()),
    }
    alive && back(DiagMsg::Reply { gen: g, seq, res })
}

/// Aufruf ausführen; `current`: gehört er noch zur offenen Diagnose (vor dem Setzen der Zwischenablage geprüft)
fn exec(
    seq: u64,
    call: &Call,
    sock: &std::path::Path,
    clip: &dyn Clip,
    line: &Mutex<Line>,
    current: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(Wiped),
) -> Result<Wiped, CallErr> {
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
        // aufgelegt, während die Verbindung aufging: nichts senden (beim Verwerfen schließt die Verbindung)
        if l.current_hung {
            return Err(CallErr::Cancelled);
        }
        l.current = Some((seq, Some(handle)));
    }
    let hung = || lock(line).current_hung;
    let req = call.request();
    let mut sent = req.to_line();
    drop(req); // überschreibt das Symptom
    let r = conn.send(&sent);
    wipe(&mut sent);
    if let Err(e) = r {
        return Err(if hung() { CallErr::Cancelled } else { CallErr::Failed(format!("{e:#}")) });
    }
    let reply = conn.reply_with(Instant::now() + call.timeout(), &mut |_, v| {
        if v.get("progress").is_some() {
            on_progress(Wiped(std::mem::take(v)));
        }
        Ok(())
    });
    let mut r = match reply {
        Ok(r) => r,
        Err(_) if hung() => return Err(CallErr::Cancelled),
        Err(ReadErr::Timeout(_)) => return Err(CallErr::Timeout),
        // der Daemon ist mitten im Aufruf weg (Neustart, Absturz)
        Err(ReadErr::Closed) => return Err(CallErr::DaemonDown),
        Err(e @ ReadErr::Other(_)) => return Err(CallErr::Failed(e.message())),
    };
    if !r.ok() {
        return Err(CallErr::from_reply(&r.v));
    }
    if matches!(call, Call::Copy { .. }) {
        return copy(&mut r.v, clip, current);
    }
    // den Rest der Zeile überschreibt `ReplyLine` beim Verwerfen
    Ok(Wiped(std::mem::take(&mut r.v)))
}

/// Bericht aus der Antwort von `diag_copy` in CLIPBOARD; zurück nur die Zahl der Zeichen
fn copy(v: &mut Value, clip: &dyn Clip, current: &dyn Fn() -> bool) -> Result<Wiped, CallErr> {
    let text = match v["text"].take() {
        Value::String(s) => s,
        _ => return Err(CallErr::Failed("Unexpected answer from dictate".into())),
    };
    let chars = text.chars().count();
    let text = SecretText::new(text);
    if !current() {
        return Err(CallErr::Failed("The panel was closed".into()));
    }
    clip.copy(text).map_err(|e| CallErr::Failed(format!("Cannot copy: {e}")))?;
    info!("diag copy: {chars} chars");
    Ok(Wiped(json!({"ok": true, "chars": chars})))
}

/// Beim Beenden mit offener Diagnose: Sitzung im Daemon sofort löschen (der Thread lebt danach nicht mehr)
pub fn clear_now(sock: &std::path::Path) {
    let r = Conn::open(sock).map_err(|e| format!("{e:#}")).and_then(|c| {
        let Some(mut c) = c else { return Ok(()) };
        c.send(&Call::Clear.request().to_line()).map_err(|e| format!("{e:#}"))?;
        c.reply(Instant::now() + crate::ui::diag_panel::CALL_TIMEOUT, &mut |_, _| Ok(())).map(|_| ()).map_err(|e| e.message())
    });
    if let Err(e) = r {
        warn!("diag clear on exit: {e}");
    }
}

/// `diag_boot_hint` (Boot-Hinweis, `ui::boot_hint`): eine Frage auf eigener Verbindung, blockierend (im Hilfsthread),
/// nicht durch die Reihe des Panels (die Probe `boots` kann dauern, das Panel soll dabei nicht warten). Frist wie
/// Sammeln: der Daemon liest dafür das Journal des vorigen Boots. Die Antwort trägt nur `show` und `reason`; ins Log nur
/// die Einordnung (im Hauptthread)
pub fn boot_hint(sock: &std::path::Path) -> Answer {
    let mut conn = match Conn::open(sock) {
        Ok(Some(c)) => c,
        Ok(None) => return Answer::DaemonDown,
        Err(e) => {
            debug!("boot hint: cannot connect ({})", e.root_cause());
            return Answer::No;
        }
    };
    if conn.send(&crate::protocol::Request::DiagBootHint.to_line()).is_err() {
        return Answer::DaemonDown;
    }
    match conn.reply(Instant::now() + crate::ui::diag_panel::COLLECT_TIMEOUT, &mut |_, _| Ok(())) {
        Ok(r) if r.ok() => Answer::from_reply(&r.v),
        Ok(_) => Answer::No,
        // ohne Antwort zu: der Daemon startet gerade neu oder ist weg
        Err(ReadErr::Closed) => Answer::DaemonDown,
        Err(e) => {
            info!("boot hint: {}", if matches!(e, ReadErr::Timeout(_)) { "no answer in time" } else { "reading the answer failed" });
            Answer::No
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    /// Fake-Daemon: nimmt Verbindungen an, liest die Zeile und antwortet je Befehl; `diag_units` hängt, bis der Client
    /// auflegt, und meldet das
    struct Fake {
        dir: PathBuf,
        seen: mpsc::Receiver<String>,
    }

    impl Fake {
        fn start() -> Fake {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("dictate-ui-diag-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
        let _ = seen.send(cmd.clone());
        let reply = match cmd.as_str() {
            "diag_units" => {
                // hängt bis zum Auflegen; danach liest der Fake das Ende
                let mut rest = Vec::new();
                let _ = r.read_to_end(&mut rest);
                let _ = seen.send("hung up".into());
                return;
            }
            "diag_collect" => {
                for i in 1..=2 {
                    writeln!(s, "{}", json!({"progress": {"id": "4", "phase": "start", "index": i, "total": 2, "probe": "mem", "params": "secret-name"}}))
                        .unwrap();
                }
                json!({"ok": true, "id": "4", "text": "report"})
            }
            "diag_copy" => json!({"ok": true, "text": "Diagnostic report äöü", "bytes": 24, "preview": 1}),
            "diag_send" => json!({"ok": false, "error": "AI server changed since the preview"}),
            "diag_boot_hint" => json!({"ok": true, "show": true, "reason": "unclean"}),
            _ => json!({"ok": true}),
        };
        let _ = writeln!(s, "{reply}");
    }

    struct FakeClip(Arc<Mutex<Vec<String>>>);

    impl Clip for FakeClip {
        fn copy(&self, text: SecretText) -> Result<(), String> {
            self.0.lock().unwrap().push(text.as_str().to_string());
            Ok(())
        }
    }

    fn spawn(f: &Fake) -> (Diag, mpsc::Receiver<DiagMsg>, Arc<Mutex<Vec<String>>>) {
        let (tx, rx) = mpsc::channel();
        let clipped = Arc::new(Mutex::new(Vec::new()));
        let d = Diag::spawn(f.sock(), FakeClip(clipped.clone()), move |m| tx.send(m).is_ok());
        (d, rx, clipped)
    }

    /// Nächste Antwort (Zwischenzeilen gezählt)
    fn reply(rx: &mpsc::Receiver<DiagMsg>) -> (u64, Result<Wiped, CallErr>, usize) {
        let mut progress = 0;
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("no reply") {
                DiagMsg::Progress { .. } => progress += 1,
                DiagMsg::Reply { seq, res, .. } => return (seq, res, progress),
            }
        }
    }

    #[test]
    fn stale_calls_are_skipped_but_cancel_and_clear_always_run() {
        let f = Fake::start();
        let (d, rx, _) = spawn(&f);
        d.set_gen(2);
        d.call(1, 1, Call::Plan { profile: None, unit: None, container: None });
        d.call(1, 2, Call::Cancel { id: "4".into() });
        d.call(1, 3, Call::Clear);
        d.call(2, 4, Call::Plan { profile: None, unit: None, container: None });
        assert_eq!([f.next(), f.next(), f.next()], ["diag_cancel", "diag_clear", "diag_plan"]);
        let seqs: Vec<u64> = (0..3).map(|_| reply(&rx).0).collect();
        assert_eq!(seqs, [2, 3, 4], "the stale plan sends no reply");
    }

    #[test]
    fn progress_lines_and_the_reply_reach_the_main_thread() {
        let f = Fake::start();
        let (d, rx, _) = spawn(&f);
        d.set_gen(1);
        d.call(1, 1, Call::Collect { profile: "now".into(), unit: None, container: None, skip: vec![] });
        let mut lines = vec![];
        let res = loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                DiagMsg::Progress { mut line, .. } => lines.push(line.value_mut()["progress"]["index"].as_u64()),
                DiagMsg::Reply { res, .. } => break res,
            }
        };
        assert_eq!(lines, [Some(1), Some(2)]);
        let mut v = res.ok().unwrap();
        assert_eq!(v.value_mut()["text"], "report");
        // ok:false wird ein Fehler mit der Meldung des Daemons
        d.call(1, 2, Call::Send { id: "4".into(), preview: 1 });
        let (_, res, _) = reply(&rx);
        assert_eq!(res.err(), Some(CallErr::Failed("AI server changed since the preview".into())));
    }

    #[test]
    fn hangup_ends_a_blocked_call_and_the_daemon_sees_it() {
        let f = Fake::start();
        let (d, rx, _) = spawn(&f);
        d.set_gen(1);
        d.call(1, 1, Call::Units);
        assert_eq!(f.next(), "diag_units");
        let t = Instant::now();
        d.hangup(1);
        let (seq, res, _) = reply(&rx);
        assert_eq!((seq, res.err()), (1, Some(CallErr::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        assert_eq!(f.next(), "hung up");
        // die Reihe läuft weiter
        d.call(1, 2, Call::Clear);
        assert_eq!(f.next(), "diag_clear");
        assert!(reply(&rx).1.is_ok());
    }

    #[test]
    fn a_call_hung_up_before_it_started_is_skipped() {
        let f = Fake::start();
        let (d, rx, _) = spawn(&f);
        d.set_gen(1);
        d.call(1, 1, Call::Units);
        d.call(1, 2, Call::Plan { profile: None, unit: None, container: None });
        assert_eq!(f.next(), "diag_units");
        // erst den wartenden, dann den laufenden auflegen (wie `close`: aufsteigend, aber der wartende kommt hier zuerst)
        d.hangup(2);
        d.hangup(1);
        assert_eq!(reply(&rx).1.err(), Some(CallErr::Cancelled));
        assert_eq!(reply(&rx).1.err(), Some(CallErr::Cancelled));
        assert_eq!(f.next(), "hung up");
        d.call(1, 3, Call::Clear);
        assert_eq!(f.next(), "diag_clear", "the plan never reached the daemon");
        // ein Auflegen nach dem Ende merkt sich nichts
        let _ = reply(&rx);
        d.hangup(3);
        assert!(lock(&d.line).hung.is_empty());
    }

    /// Auflegen gleich nach dem Auftrag trifft den Aufruf in jeder Lage (vor, während oder nach dem Öffnen der
    /// Verbindung); vorher ging es zwischen dem Beginn und dem Eintragen des Klons verloren (der Aufruf lief weiter)
    #[test]
    fn a_hangup_right_after_the_call_is_never_lost() {
        let f = Fake::start();
        let (d, rx, _) = spawn(&f);
        d.set_gen(1);
        for seq in 1..=300 {
            d.call(1, seq, Call::Units);
            if seq % 3 == 0 {
                std::thread::yield_now();
            }
            d.hangup(seq);
            let (got, res, _) = reply(&rx);
            assert_eq!((got, res.err()), (seq, Some(CallErr::Cancelled)), "call {seq}");
        }
    }

    #[test]
    fn copy_sets_the_clipboard_and_reports_only_the_count() {
        let f = Fake::start();
        let (d, rx, clipped) = spawn(&f);
        d.set_gen(1);
        d.call(1, 1, Call::Copy { id: "4".into() });
        let (_, res, _) = reply(&rx);
        let mut v = res.ok().unwrap();
        assert_eq!(*v.value_mut(), json!({"ok": true, "chars": 21}));
        assert_eq!(*clipped.lock().unwrap(), ["Diagnostic report äöü"]);
        // nach dem Schließen (neues `gen`) wird nichts mehr kopiert
        d.call(1, 2, Call::Copy { id: "4".into() });
        d.set_gen(2);
        let _ = rx.recv_timeout(Duration::from_millis(500));
        assert_eq!(clipped.lock().unwrap().len(), 1);
    }

    #[test]
    fn boot_hint_asks_once_and_names_a_missing_daemon() {
        let f = Fake::start();
        assert_eq!(boot_hint(&f.sock()), Answer::Show);
        assert_eq!(f.next(), "diag_boot_hint");
        // kein Socket: Daemon fehlt (der Hinweis fragt dann noch einmal)
        assert_eq!(boot_hint(&f.dir.join("no-such-sock")), Answer::DaemonDown);
        // Verbindung ohne Antwort zu (Daemon startet neu): ebenso
        let drop_sock = f.dir.join("drop");
        let l = UnixListener::bind(&drop_sock).unwrap();
        let t = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            assert!(line.contains("diag_boot_hint"), "{line}");
        });
        assert_eq!(boot_hint(&drop_sock), Answer::DaemonDown);
        t.join().unwrap();
    }
}
