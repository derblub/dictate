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

//! Speicher der Textaktionen im Daemon: genau eine Sitzung (die zuletzt gegriffene Markierung), ihr laufender
//! KI-Aufruf und ihr Ergebnis.
//!
//! - **Eine Sitzung**: `act_grab` ersetzt die vorige vollständig (laufender Aufruf wird abgebrochen, Text und Ergebnis
//!   werden gelöscht). Ids zählen hoch, eine alte Id trifft also nie die neue Sitzung.
//! - **Ablauf**: [`TTL`] nach der letzten Benutzung (grab, Start und Ende eines Aufrufs, Abholen des Ergebnisses). Der
//!   Ablauf wartet nicht auf die nächste Anfrage: ein eigener Thread ([`Sweeper`]) löscht die Sitzung zur Frist.
//!   Während eines Aufrufs läuft die Frist nicht ab (er dauert höchstens `actions.timeout_ms`).
//! - **Ein Aufruf je Sitzung**: ein zweiter `act_run` bricht den ersten über dessen Abbruch-Flag ab; nur das Ergebnis
//!   des jüngsten Aufrufs wird gespeichert.
//! - **Löschen heißt überschreiben**: Markierung und Ergebnis liegen in [`SecretText`], das seinen Speicher beim
//!   Freigeben mit Nullen überschreibt (ohne `unsafe` und ohne Crate). Das gilt für die Kopien, die hier bis zu
//!   [`TTL`] leben. Kurzlebige Kopien in Bibliotheken (JSON-Parser, HTTP-Client, TLS) während einer Anfrage erfasst es
//!   nicht; sie werden direkt danach freigegeben.
//!
//! Std-only und ohne `crate::pipeline`: `dictate-settings` bindet `actions/` per Pfad ein.

use super::Output;
use std::fmt;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Lebensdauer einer Sitzung ohne Benutzung
pub const TTL: Duration = Duration::from_secs(300);

// ---------------------------------------------------------------------------------------------------------------
// Überschreibender Text

/// Überschreibt den ganzen Puffer von `s` (auch die ungenutzte Kapazität) mit Nullen und lässt `s` leer zurück.
/// Sicherer Rust-Code: `fill` für die belegten Bytes, `MaybeUninit::new(0)` für die Reserve; `black_box` und der
/// Zaun verhindern, dass der Compiler die Schreibzugriffe vor der Freigabe als tot entfernt.
pub fn wipe(s: &mut String) {
    let mut v = std::mem::take(s).into_bytes();
    wipe_bytes(&mut v);
}

/// Wie [`wipe`] für Bytes: überschreibt Inhalt und Reserve von `v` mit Nullen und lässt `v` leer zurück.
pub fn wipe_bytes(v: &mut Vec<u8>) {
    let mut v = std::mem::take(v);
    v.fill(0);
    for b in v.spare_capacity_mut() {
        *b = MaybeUninit::new(0);
    }
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    std::hint::black_box(&mut v);
}

/// Text, der nie ins Log gerät (`Debug` zeigt nur die Länge) und beim Freigeben überschrieben wird ([`wipe`]).
#[derive(Default)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(s: String) -> SecretText {
        SecretText(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl Clone for SecretText {
    /// Kopie mit genau passender Kapazität (kein Umkopieren beim Wachsen, das unüberschriebene Reste ließe)
    fn clone(&self) -> SecretText {
        let mut s = String::with_capacity(self.0.len());
        s.push_str(&self.0);
        SecretText(s)
    }
}

impl PartialEq for SecretText {
    fn eq(&self, other: &SecretText) -> bool {
        self.0 == other.0
    }
}

impl Eq for SecretText {}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretText({} bytes)", self.0.len())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sitzung

/// Ziel, gegen das eine Markierung geprüft wurde. Ändert es sich vor `act_run`, gilt die Prüfung (Geheimnisse,
/// Cloud-Vorschau) nicht mehr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetKey {
    /// `ollama` | `openai` | `anthropic`
    pub protocol: String,
    /// Basis-URL wie aufgelöst (`AiCfg::resolved`)
    pub base: String,
    pub cloud: bool,
}

/// Woher der Text einer Sitzung kommt. Entscheidet, welche Aktionen auf ihr laufen dürfen, welcher Schalter gilt und
/// welche Regel für Geheimnisse (PLAN-palette.md, Aufgabe 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Markierung (PRIMARY) über Mod+i: Katalog der Textaktionen, Schalter `features.actions`
    #[default]
    Selection,
    /// CLIPBOARD, nur auf die ausdrückliche Absicht `explain_clipboard` der Befehlspalette (Vorschau vor dem Senden)
    Clipboard,
    /// Frage aus der Befehlspalette (Absicht `ask`)
    Question,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Selection => "selection",
            Source::Clipboard => "clipboard",
            Source::Question => "question",
        }
    }

    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "selection" => Some(Source::Selection),
            "clipboard" => Some(Source::Clipboard),
            "question" => Some(Source::Question),
            _ => None,
        }
    }

    /// Gehört die Sitzung der Befehlspalette (Schalter `palette.enabled`, Modell `palette.model`)?
    pub fn is_palette(self) -> bool {
        self != Source::Selection
    }
}

/// Eine neue Markierung für [`Store::grab`].
pub struct NewSelection {
    /// `None` = gesperrt (Geheimnis), der Text wird gar nicht gespeichert
    pub text: Option<SecretText>,
    pub chars: usize,
    pub target: TargetKey,
    /// Arten aus `secrets::scan`
    pub secret: Vec<&'static str>,
    pub source: Source,
}

/// Ergebnis eines Aufrufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActResult {
    pub action: String,
    pub output: Output,
    pub text: SecretText,
    pub truncated: bool,
}

/// Erlaubnis für einen Aufruf: Kopie des Textes, Nummer des Aufrufs, Abbruch-Flag.
#[derive(Debug)]
pub struct Ticket {
    pub text: SecretText,
    pub chars: usize,
    pub seq: u64,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// Id unbekannt, ersetzt, gelöscht oder abgelaufen
    Unknown,
    /// Das KI-Ziel hat sich seit dem Greifen geändert; die Sitzung ist gelöscht
    TargetChanged,
    /// Geheimnis und Cloud bzw. `secrets = "block"`; Arten wie bei `scan`
    Blocked(Vec<&'static str>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultError {
    Unknown,
    /// Kein Ergebnis (noch nicht gelaufen, läuft noch, abgebrochen, fehlgeschlagen)
    NotReady,
}

struct Run {
    seq: u64,
    cancel: Arc<AtomicBool>,
}

struct Selection {
    id: u64,
    text: Option<SecretText>,
    chars: usize,
    target: TargetKey,
    secret: Vec<&'static str>,
    source: Source,
    result: Option<ActResult>,
    run: Option<Run>,
    touched: Instant,
}

impl Selection {
    fn stop_run(&mut self) {
        if let Some(r) = self.run.take() {
            r.cancel.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for Selection {
    fn drop(&mut self) {
        // Text und Ergebnis überschreiben sich selbst; ein laufender Aufruf soll nicht weiter ins Leere arbeiten
        self.stop_run();
    }
}

/// Zustand ohne Sperre und mit ausdrücklicher Uhr (für Tests); [`Store`] ist die geteilte Hülle.
pub struct State {
    sel: Option<Selection>,
    next_id: u64,
    next_seq: u64,
    ttl: Duration,
}

impl State {
    pub fn new(ttl: Duration) -> State {
        State { sel: None, next_id: 1, next_seq: 1, ttl }
    }

    pub fn grab(&mut self, new: NewSelection, now: Instant) -> u64 {
        self.sel = None;
        let id = self.next_id;
        self.next_id += 1;
        self.sel = Some(Selection {
            id,
            text: new.text,
            chars: new.chars,
            target: new.target,
            secret: new.secret,
            source: new.source,
            result: None,
            run: None,
            touched: now,
        });
        id
    }

    pub fn clear(&mut self) {
        self.sel = None;
    }

    /// Frist der aktuellen Sitzung; `None` = keine Sitzung oder ein Aufruf läuft.
    pub fn deadline(&self) -> Option<Instant> {
        self.sel.as_ref().filter(|s| s.run.is_none()).map(|s| s.touched + self.ttl)
    }

    /// Löscht die Sitzung, wenn ihre Frist um ist; `true` = gelöscht.
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.deadline().is_some_and(|d| now >= d) {
            self.sel = None;
            return true;
        }
        false
    }

    /// Lebende Sitzung mit dieser Id
    fn live(&mut self, id: u64, now: Instant) -> Option<&mut Selection> {
        self.expire(now);
        self.sel.as_mut().filter(|s| s.id == id)
    }

    /// Herkunft der lebenden Sitzung `id`; `None` = unbekannt, ersetzt, gelöscht oder abgelaufen.
    pub fn source(&mut self, id: u64, now: Instant) -> Option<Source> {
        self.live(id, now).map(|s| s.source)
    }

    /// Startet einen Aufruf: prüft Id, Ziel und Sperre gegen den jetzigen Stand und bricht einen laufenden Aufruf ab.
    /// `block_secrets` = `actions.secrets == "block"`.
    pub fn begin_run(&mut self, id: u64, target: &TargetKey, block_secrets: bool, now: Instant) -> Result<Ticket, RunError> {
        // Nummern dürfen Lücken haben, sie müssen nur eindeutig sein
        let seq = self.next_seq;
        self.next_seq += 1;
        let Some(s) = self.live(id, now) else {
            return Err(RunError::Unknown);
        };
        if s.target != *target {
            self.sel = None;
            return Err(RunError::TargetChanged);
        }
        if !s.secret.is_empty() && (target.cloud || block_secrets) {
            s.text = None;
            s.result = None;
            return Err(RunError::Blocked(s.secret.clone()));
        }
        let Some(text) = s.text.clone() else {
            return Err(RunError::Blocked(s.secret.clone()));
        };
        s.stop_run();
        s.result = None;
        let cancel = Arc::new(AtomicBool::new(false));
        s.run = Some(Run { seq, cancel: cancel.clone() });
        s.touched = now;
        Ok(Ticket { text, chars: s.chars, seq, cancel })
    }

    /// Beendet Aufruf `seq`; ein Ergebnis wird nur gespeichert, wenn er noch der aktuelle der Sitzung `id` ist
    /// (`true`). Sonst verfällt es (und überschreibt sich beim Freigeben).
    pub fn finish_run(&mut self, id: u64, seq: u64, result: Option<ActResult>, now: Instant) -> bool {
        let Some(s) = self.sel.as_mut().filter(|s| s.id == id) else {
            return false;
        };
        if s.run.as_ref().is_none_or(|r| r.seq != seq) {
            return false;
        }
        s.run = None;
        s.touched = now;
        if result.is_some() {
            s.result = result;
        }
        true
    }

    /// Bricht den laufenden Aufruf der Sitzung `id` ab; `true`, wenn einer lief.
    pub fn cancel(&mut self, id: u64) -> bool {
        match self.sel.as_mut().filter(|s| s.id == id) {
            Some(s) if s.run.is_some() => {
                s.stop_run();
                true
            }
            _ => false,
        }
    }

    /// Kopie des Ergebnisses (für `act_result`/Copy).
    pub fn result(&mut self, id: u64, now: Instant) -> Result<ActResult, ResultError> {
        let s = self.live(id, now).ok_or(ResultError::Unknown)?;
        let r = s.result.clone().ok_or(ResultError::NotReady)?;
        s.touched = now;
        Ok(r)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Geteilte Hülle mit Ablauf-Thread

struct Shared {
    state: Mutex<State>,
    /// Weckt den Ablauf-Thread bei jeder Änderung (neue Frist) und beim Beenden
    changed: Condvar,
    stop: AtomicBool,
}

/// Speicher der Textaktionen, geteilt zwischen den Verbindungs-Threads des Daemons.
#[derive(Clone)]
pub struct Store {
    shared: Arc<Shared>,
}

/// Ablauf-Thread; `Drop` beendet ihn.
pub struct Sweeper {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl Store {
    /// Speicher mit Ablauf nach `ttl` und dem Thread, der abgelaufene Sitzungen löscht.
    pub fn start(ttl: Duration) -> std::io::Result<(Store, Sweeper)> {
        let shared = Arc::new(Shared { state: Mutex::new(State::new(ttl)), changed: Condvar::new(), stop: AtomicBool::new(false) });
        let s2 = shared.clone();
        let handle = std::thread::Builder::new().name("dictate-act-ttl".into()).spawn(move || sweep(&s2))?;
        Ok((Store { shared: shared.clone() }, Sweeper { shared, handle: Some(handle) }))
    }

    fn with<T>(&self, f: impl FnOnce(&mut State, Instant) -> T) -> T {
        let mut g = lock(&self.shared.state);
        let r = f(&mut g, Instant::now());
        drop(g);
        self.shared.changed.notify_all();
        r
    }

    pub fn grab(&self, new: NewSelection) -> u64 {
        self.with(|s, now| s.grab(new, now))
    }

    pub fn clear(&self) {
        self.with(|s, _| s.clear())
    }

    pub fn source(&self, id: u64) -> Option<Source> {
        self.with(|s, now| s.source(id, now))
    }

    pub fn begin_run(&self, id: u64, target: &TargetKey, block_secrets: bool) -> Result<Ticket, RunError> {
        self.with(|s, now| s.begin_run(id, target, block_secrets, now))
    }

    pub fn finish_run(&self, id: u64, seq: u64, result: Option<ActResult>) -> bool {
        self.with(|s, now| s.finish_run(id, seq, result, now))
    }

    pub fn cancel(&self, id: u64) -> bool {
        self.with(|s, _| s.cancel(id))
    }

    pub fn result(&self, id: u64) -> Result<ActResult, ResultError> {
        self.with(|s, now| s.result(id, now))
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn sweep(shared: &Shared) {
    let mut g = lock(&shared.state);
    while !shared.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        g.expire(now);
        g = match g.deadline() {
            None => shared.changed.wait(g).unwrap_or_else(|e| e.into_inner()),
            Some(d) => shared.changed.wait_timeout(g, d.saturating_duration_since(now)).unwrap_or_else(|e| e.into_inner()).0,
        };
    }
}

impl Drop for Sweeper {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // unter der Sperre wecken, damit der Thread das Signal nicht zwischen Prüfen und Warten verpasst
        drop(lock(&self.shared.state));
        self.shared.changed.notify_all();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        lock(&self.shared.state).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(base: &str, cloud: bool) -> TargetKey {
        TargetKey { protocol: "ollama".into(), base: base.into(), cloud }
    }

    fn sel(text: &str, secret: Vec<&'static str>, cloud: bool) -> NewSelection {
        NewSelection {
            text: Some(SecretText::new(text.into())),
            chars: text.chars().count(),
            target: key("http://127.0.0.1:1", cloud),
            secret,
            source: Source::Selection,
        }
    }

    fn result(text: &str) -> ActResult {
        ActResult { action: "explain".into(), output: Output::Answer, text: SecretText::new(text.into()), truncated: false }
    }

    #[test]
    fn wipe_zeroes_the_whole_buffer() {
        let mut s = String::with_capacity(64);
        s.push_str("hunter2 Grüße");
        wipe(&mut s);
        assert!(s.is_empty());
        let t = SecretText::new("geheim".into());
        assert_eq!(format!("{t:?}"), "SecretText(6 bytes)");
        assert_eq!(t.clone(), t);
    }

    #[test]
    fn one_session_new_grab_replaces_old() {
        let t0 = Instant::now();
        let mut st = State::new(TTL);
        let a = st.grab(sel("eins", vec![], false), t0);
        let k = key("http://127.0.0.1:1", false);
        let ticket = st.begin_run(a, &k, false, t0).unwrap();
        assert_eq!(ticket.text.as_str(), "eins");
        let b = st.grab(sel("zwei", vec![], false), t0);
        assert_ne!(a, b);
        assert!(ticket.cancel.load(Ordering::SeqCst), "running call of the replaced session is cancelled");
        assert!(!st.finish_run(a, ticket.seq, Some(result("x")), t0));
        assert_eq!(st.begin_run(a, &k, false, t0).unwrap_err(), RunError::Unknown);
        assert_eq!(st.begin_run(b, &k, false, t0).unwrap().text.as_str(), "zwei");
    }

    #[test]
    fn second_run_cancels_first_and_only_latest_result_counts() {
        let t0 = Instant::now();
        let mut st = State::new(TTL);
        let id = st.grab(sel("text", vec![], false), t0);
        let k = key("http://127.0.0.1:1", false);
        let r1 = st.begin_run(id, &k, false, t0).unwrap();
        let r2 = st.begin_run(id, &k, false, t0).unwrap();
        assert!(r1.cancel.load(Ordering::SeqCst));
        assert!(!r2.cancel.load(Ordering::SeqCst));
        assert!(!st.finish_run(id, r1.seq, Some(result("alt")), t0));
        assert_eq!(st.result(id, t0).unwrap_err(), ResultError::NotReady);
        assert!(st.finish_run(id, r2.seq, Some(result("neu")), t0));
        assert_eq!(st.result(id, t0).unwrap().text.as_str(), "neu");
        assert!(!st.cancel(id), "nothing running");
    }

    #[test]
    fn expires_after_ttl_since_last_use_but_not_while_running() {
        let t0 = Instant::now();
        let ttl = Duration::from_secs(10);
        let mut st = State::new(ttl);
        let id = st.grab(sel("text", vec![], false), t0);
        let k = key("http://127.0.0.1:1", false);
        assert_eq!(st.deadline(), Some(t0 + ttl));
        let r = st.begin_run(id, &k, false, t0 + Duration::from_secs(5)).unwrap();
        assert_eq!(st.deadline(), None, "no expiry while a call runs");
        assert!(!st.expire(t0 + Duration::from_secs(100)));
        assert!(st.finish_run(id, r.seq, Some(result("ok")), t0 + Duration::from_secs(20)));
        assert_eq!(st.deadline(), Some(t0 + Duration::from_secs(30)));
        assert!(st.result(id, t0 + Duration::from_secs(29)).is_ok());
        assert!(!st.expire(t0 + Duration::from_secs(38)));
        assert!(st.expire(t0 + Duration::from_secs(39)));
        assert_eq!(st.result(id, t0 + Duration::from_secs(39)).unwrap_err(), ResultError::Unknown);
    }

    #[test]
    fn target_change_and_secrets_block() {
        let t0 = Instant::now();
        let mut st = State::new(TTL);
        let k = key("http://127.0.0.1:1", false);
        let id = st.grab(sel("text", vec![], false), t0);
        assert_eq!(st.begin_run(id, &key("http://127.0.0.1:2", false), false, t0).unwrap_err(), RunError::TargetChanged);
        assert_eq!(st.begin_run(id, &k, false, t0).unwrap_err(), RunError::Unknown, "session is gone after a target change");
        // Geheimnis lokal mit local_only: erlaubt; danach auf block umgestellt: gesperrt und Text gelöscht
        let id = st.grab(sel("pw", vec!["password"], false), t0);
        assert!(st.begin_run(id, &k, false, t0).is_ok());
        assert_eq!(st.begin_run(id, &k, true, t0).unwrap_err(), RunError::Blocked(vec!["password"]));
        assert!(st.sel.as_ref().unwrap().text.is_none());
        assert_eq!(st.begin_run(id, &k, false, t0).unwrap_err(), RunError::Blocked(vec!["password"]));
        // gesperrt gegriffen: kein Text
        let id =
            st.grab(NewSelection { text: None, chars: 2, target: key("https://api.example", true), secret: vec!["password"], source: Source::Selection }, t0);
        assert_eq!(st.begin_run(id, &key("https://api.example", true), false, t0).unwrap_err(), RunError::Blocked(vec!["password"]));
    }

    #[test]
    fn sweeper_deletes_at_the_deadline_without_a_request() {
        let (store, sweeper) = Store::start(Duration::from_millis(100)).unwrap();
        let id = store.grab(sel("text", vec![], false));
        std::thread::sleep(Duration::from_millis(300));
        assert!(lock(&store.shared.state).sel.is_none(), "deleted by the sweeper");
        assert_eq!(store.result(id).unwrap_err(), ResultError::Unknown);
        let t = Instant::now();
        drop(sweeper);
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn source_is_kept_per_session_and_unknown_after_replace() {
        let t0 = Instant::now();
        let mut st = State::new(TTL);
        let mut n = sel("frage", vec![], false);
        n.source = Source::Question;
        let a = st.grab(n, t0);
        assert_eq!(st.source(a, t0), Some(Source::Question));
        let b = st.grab(sel("text", vec![], false), t0);
        assert_eq!((st.source(a, t0), st.source(b, t0)), (None, Some(Source::Selection)));
        assert_eq!(st.source(b, t0 + TTL), None, "expired");
        for s in [Source::Selection, Source::Clipboard, Source::Question] {
            assert_eq!(Source::parse(s.as_str()), Some(s));
        }
        assert_eq!(Source::parse("primary"), None);
        assert!(Source::Clipboard.is_palette() && Source::Question.is_palette() && !Source::Selection.is_palette());
    }
}
