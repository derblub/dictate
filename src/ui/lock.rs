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

//! Sperr-Erkennung von `dictate-ui` über systemd-logind (PLAN-wayland.md 3.6, Regel 7, Befund 11): der reine Teil
//! ohne D-Bus. Den Bus (gio, System-Bus) bedient `src/bin/dictate-ui/lock.rs`; dessen Rückrufe reichen nur
//! [`Signal`]e weiter, entschieden wird hier.
//!
//! Ausgelöst wird bei
//!   * `LockedHint = true` der eigenen Sitzung (`PropertiesChanged`): Noctalia setzt den Hinweis beim Sperren
//!     (Spike S5, gemessen), auch beim Start schon gesetzt ([`Watch::initial`]),
//!   * Signal `Lock` der eigenen Sitzung (`loginctl lock-session`),
//!   * `PrepareForSleep(true)` des Managers (Suspend und Ruhezustand).
//!
//! Die eigene Sitzung kommt von `Manager.GetSession` für `XDG_SESSION_ID` (falls gesetzt) und `auto` (logind: die
//! Sitzung des Aufrufers, außerhalb einer Sitzung, wie beim User-Dienst, die grafische Sitzung des Users); beobachtet
//! werden alle Sitzungen, die dabei herauskommen ([`Watch`]). Nie eine feste Nummer. Die Signale werden vor `GetSession`
//! abonniert; was vor den Antworten eintrifft, merkt sich [`Watch`] und prüft es, sobald ein Pfad bekannt ist (sonst
//! ginge eine Sperre direkt nach dem Start verloren). Signale anderer Sitzungen zählen nicht.

/// Bus-Name von logind; die Abos verlangen ihn als Absender (sonst könnte jeder Client am Bus „sperren“ oder, was
/// schwerer wiegt, mit einem falschen Pfad die eigene Sitzung verdecken)
pub const LOGIND: &str = "org.freedesktop.login1";
pub const MANAGER_PATH: &str = "/org/freedesktop/login1";
pub const MANAGER_IFACE: &str = "org.freedesktop.login1.Manager";
pub const SESSION_IFACE: &str = "org.freedesktop.login1.Session";
pub const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
/// Präfix der Sitzungspfade (`/org/freedesktop/login1/session/_34` für Sitzung 4)
pub const SESSION_NS: &str = "/org/freedesktop/login1/session";

/// Höchstens so viele Signale vor der Antwort auf `GetSession` merken (danach die ältesten verwerfen)
const EARLY_MAX: usize = 32;

/// Auslöser einer Sperre (nur fürs Log)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `LockedHint` wurde `true`
    Hint,
    /// Signal `Lock`
    Lock,
    /// `PrepareForSleep(true)`
    Sleep,
    /// `LockedHint` war beim Start schon `true`
    AtStart,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::Hint => "locked hint",
            Reason::Lock => "lock signal",
            Reason::Sleep => "prepare for sleep",
            Reason::AtStart => "locked at start",
        }
    }
}

/// Ein Signal von logind, schon ausgepackt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// `PropertiesChanged` am Interface `Session` einer Sitzung; `locked`: neuer Wert von `LockedHint`, `None` = nicht
    /// unter den geänderten Werten
    Props { path: String, locked: Option<bool> },
    /// `Lock` einer Sitzung
    Lock { path: String },
    /// `PrepareForSleep` des Managers (`true` = vor dem Schlafen, `false` = danach)
    Sleep(bool),
}

/// Kandidaten für `GetSession` der Reihe nach: `XDG_SESSION_ID` (falls gesetzt), dann `auto`
pub fn session_ids(env: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(id) = env.map(str::trim).filter(|i| !i.is_empty() && *i != "auto") {
        v.push(id.to_string());
    }
    v.push("auto".to_string());
    v
}

/// Ist `p` ein Sitzungspfad von logind (unter [`SESSION_NS`], ein Element, nur Zeichen eines Objektpfads)?
pub fn session_path_ok(p: &str) -> bool {
    let Some(rest) = p.strip_prefix(SESSION_NS).and_then(|r| r.strip_prefix('/')) else { return false };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Zustand der Erkennung: eigene Sitzungen (sobald bekannt), Signale davor und ob gerade gesperrt ist.
///
/// Gefragt werden alle Kandidaten aus [`session_ids`] (`XDG_SESSION_ID` und `auto`), beobachtet wird jede Sitzung, die
/// dabei herauskommt: steht im User-Manager eine veraltete `XDG_SESSION_ID` (etwa eine alte tty-Sitzung), sperrt
/// Noctalia trotzdem die Sitzung hinter `auto`. Eine Sperre zu viel löscht nur, eine zu wenig bräche Regel 7
/// (Durchsicht W3 M2).
#[derive(Debug, Default)]
pub struct Watch {
    sessions: Vec<String>,
    /// Sitzungen, deren `LockedHint` das Öffnen sperrt: die hinter `auto` (die grafische Sitzung laut logind); ohne sie
    /// alle. Eine veraltete Sitzung mit hängendem Hinweis sperrte das Panel sonst für immer
    gates: Vec<String>,
    /// Ausstehende Antworten auf `GetSession`
    pending: usize,
    early: Vec<Signal>,
    /// Kein Pfad mehr zu erwarten (alle `GetSession` gescheitert): nichts mehr merken
    given_up: bool,
    /// `LockedHint` je eigener Sitzung
    hints: Vec<(String, bool)>,
    /// Zwischen `PrepareForSleep(true)` und `(false)`
    sleeping: bool,
}

impl Watch {
    pub fn new() -> Watch {
        Watch::default()
    }

    /// So viele `GetSession` sind unterwegs (vor dem ersten Aufruf)
    pub fn expect(&mut self, n: usize) {
        self.pending = n;
    }

    /// Pfade der eigenen Sitzungen, sobald bekannt
    pub fn sessions(&self) -> &[String] {
        &self.sessions
    }

    /// Gesperrt: `LockedHint` einer eigenen Sitzung ist gesetzt, oder das System schläft gerade ein. Ein `Lock`-Signal
    /// allein setzt den Zustand nicht (ohne Sperrprogramm bliebe er sonst für immer stehen); der Sperrbildschirm setzt
    /// danach `LockedHint`.
    pub fn locked(&self) -> bool {
        let gates = if self.gates.is_empty() { &self.sessions } else { &self.gates };
        self.sleeping || self.hints.iter().any(|(p, h)| *h && gates.contains(p))
    }

    fn set_hint(&mut self, path: &str, locked: bool) {
        match self.hints.iter_mut().find(|(p, _)| p == path) {
            Some(h) => h.1 = locked,
            None => self.hints.push((path.to_string(), locked)),
        }
    }

    /// Signal einer bekannten eigenen Sitzung anwenden
    fn own(&mut self, s: &Signal) -> Option<Reason> {
        match s {
            Signal::Props { path, locked: Some(l) } if self.sessions.contains(path) => {
                self.set_hint(path, *l);
                l.then_some(Reason::Hint)
            }
            Signal::Lock { path } if self.sessions.contains(path) => Some(Reason::Lock),
            _ => None,
        }
    }

    /// Ein Signal; `Some`, wenn es eine Sperre einer eigenen Sitzung ist. Solange `GetSession` aussteht, werden Signale
    /// einer Sitzung gemerkt ([`Watch::resolved`] prüft sie dann); der Schlaf gilt für alle sofort.
    pub fn signal(&mut self, s: Signal) -> Option<Reason> {
        if let Signal::Sleep(go) = s {
            self.sleeping = go;
            return go.then_some(Reason::Sleep);
        }
        let r = self.own(&s);
        let session_signal = matches!(s, Signal::Lock { .. } | Signal::Props { locked: Some(_), .. });
        if r.is_none() && self.pending > 0 && !self.given_up && session_signal {
            if self.early.len() == EARLY_MAX {
                self.early.remove(0);
            }
            self.early.push(s);
        }
        r
    }

    /// Eine Antwort auf `GetSession` ist vorbei (gelungen oder nicht); ist es die letzte, die gemerkten Signale
    /// verwerfen
    fn answered(&mut self) {
        self.pending = self.pending.saturating_sub(1);
        if self.pending == 0 {
            self.early.clear();
            if self.sessions.is_empty() {
                self.given_up = true;
            }
        }
    }

    /// Antwort auf `GetSession`: Pfad prüfen und beobachten; `Ok(Some)`, wenn eines der gemerkten Signale diese
    /// Sitzung sperrte (mehrere zählen als eine Sperre). `gate`: die Sitzung kam von `auto` (ihr Zustand sperrt das
    /// Öffnen, [`Watch::locked`])
    pub fn resolved(&mut self, path: &str, gate: bool) -> Result<Option<Reason>, String> {
        if !session_path_ok(path) {
            self.answered();
            return Err("logind returned an unexpected session path".into());
        }
        if !self.sessions.iter().any(|p| p == path) {
            self.sessions.push(path.to_string());
        }
        if gate && !self.gates.iter().any(|p| p == path) {
            self.gates.push(path.to_string());
        }
        let (mine, rest): (Vec<Signal>, Vec<Signal>) = std::mem::take(&mut self.early).into_iter().partition(|s| match s {
            Signal::Props { path: p, .. } | Signal::Lock { path: p } => p == path,
            Signal::Sleep(_) => false,
        });
        self.early = rest;
        let mut reason = None;
        for s in &mine {
            let r = self.own(s);
            reason = reason.or(r);
        }
        self.answered();
        Ok(reason)
    }

    /// Ein `GetSession` gescheitert
    pub fn failed(&mut self) {
        self.answered();
    }

    /// Kein Pfad zu erwarten (kein Bus): gemerkte Signale verwerfen
    pub fn give_up(&mut self) {
        self.given_up = true;
        self.pending = 0;
        self.early.clear();
    }

    /// Aufgegeben, weil keine Sitzung gefunden wurde (und alle Antworten da sind)
    pub fn gave_up(&self) -> bool {
        self.given_up
    }

    /// Wert von `LockedHint` einer eigenen Sitzung beim Start (nach [`Watch::resolved`] gelesen)
    pub fn initial(&mut self, path: &str, locked: bool) -> Option<Reason> {
        if !self.sessions.iter().any(|p| p == path) {
            return None;
        }
        self.set_hint(path, locked);
        locked.then_some(Reason::AtStart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "/org/freedesktop/login1/session/_34";
    const OTHER: &str = "/org/freedesktop/login1/session/_32";

    fn props(p: &str, locked: Option<bool>) -> Signal {
        Signal::Props { path: p.into(), locked }
    }
    fn lock(p: &str) -> Signal {
        Signal::Lock { path: p.into() }
    }
    fn watching(paths: &[&str]) -> Watch {
        let mut w = Watch::new();
        w.expect(paths.len());
        for p in paths {
            w.resolved(p, true).unwrap();
        }
        w
    }

    #[test]
    fn candidates_env_first_then_auto_never_a_fixed_number() {
        assert_eq!(session_ids(Some("4")), ["4", "auto"]);
        assert_eq!(session_ids(Some(" 12 ")), ["12", "auto"]);
        assert_eq!(session_ids(Some("")), ["auto"]);
        assert_eq!(session_ids(Some("auto")), ["auto"]);
        assert_eq!(session_ids(None), ["auto"]);
    }

    #[test]
    fn session_paths() {
        assert!(session_path_ok(OWN));
        assert!(session_path_ok("/org/freedesktop/login1/session/c1"));
        assert!(!session_path_ok("/org/freedesktop/login1/session"));
        assert!(!session_path_ok("/org/freedesktop/login1/session/"));
        assert!(!session_path_ok("/org/freedesktop/login1/session/_34/x"));
        assert!(!session_path_ok("/org/freedesktop/login1/sessionX/_34"));
        assert!(!session_path_ok("/org/freedesktop/login1/user/_1000"));
        assert!(!session_path_ok("/"));
    }

    #[test]
    fn own_session_only() {
        let mut w = watching(&[OWN]);
        assert_eq!(w.sessions(), [OWN]);
        assert_eq!(w.signal(props(OWN, Some(true))), Some(Reason::Hint));
        assert_eq!(w.signal(lock(OWN)), Some(Reason::Lock));
        assert_eq!(w.signal(props(OTHER, Some(true))), None);
        assert_eq!(w.signal(lock(OTHER)), None);
        // Entsperren und andere Eigenschaften sperren nicht
        assert_eq!(w.signal(props(OWN, Some(false))), None);
        assert_eq!(w.signal(props(OWN, None)), None);
    }

    #[test]
    fn both_candidates_are_watched() {
        // XDG_SESSION_ID zeigt auf eine alte Sitzung, `auto` auf die echte: beide zählen
        let mut w = watching(&[OTHER, OWN]);
        assert_eq!(w.signal(props(OWN, Some(true))), Some(Reason::Hint));
        assert_eq!(w.signal(lock(OTHER)), Some(Reason::Lock));
        assert_eq!(w.signal(lock("/org/freedesktop/login1/session/_35")), None);
        // gleicher Pfad zweimal: einmal beobachtet
        let w = watching(&[OWN, OWN]);
        assert_eq!(w.sessions(), [OWN]);
    }

    #[test]
    fn only_the_auto_session_gates_opening_but_all_clear() {
        let mut w = Watch::new();
        w.expect(2);
        w.resolved(OTHER, false).unwrap(); // aus XDG_SESSION_ID, veraltet
        w.resolved(OWN, true).unwrap(); // aus "auto"
                                        // hängender Hinweis der veralteten Sitzung: löscht, sperrt aber das Öffnen nicht
        assert_eq!(w.initial(OTHER, true), Some(Reason::AtStart));
        assert!(!w.locked());
        w.signal(props(OWN, Some(true)));
        assert!(w.locked());
        // ohne Antwort für "auto" sperren alle
        let mut w = Watch::new();
        w.expect(1);
        w.resolved(OTHER, false).unwrap();
        w.initial(OTHER, true);
        assert!(w.locked());
    }

    #[test]
    fn locked_state_follows_the_hint_and_sleep_not_the_lock_signal() {
        let mut w = watching(&[OWN]);
        assert!(!w.locked());
        w.signal(lock(OWN));
        assert!(!w.locked(), "a lock signal alone (no locker) must not stick");
        w.signal(props(OWN, Some(true)));
        assert!(w.locked());
        w.signal(props(OTHER, Some(false)));
        assert!(w.locked(), "another session does not unlock");
        w.signal(props(OWN, Some(false)));
        assert!(!w.locked());
        w.signal(Signal::Sleep(true));
        assert!(w.locked());
        w.signal(Signal::Sleep(false));
        assert!(!w.locked());
    }

    #[test]
    fn sleep_counts_only_before_sleeping_and_needs_no_session() {
        let mut w = Watch::new();
        assert_eq!(w.signal(Signal::Sleep(true)), Some(Reason::Sleep));
        assert_eq!(w.signal(Signal::Sleep(false)), None);
        w.give_up();
        assert_eq!(w.signal(Signal::Sleep(true)), Some(Reason::Sleep));
    }

    #[test]
    fn signals_before_get_session_are_checked_afterwards() {
        let mut w = Watch::new();
        w.expect(2);
        assert_eq!(w.signal(lock(OTHER)), None);
        assert_eq!(w.signal(lock(OWN)), None);
        assert_eq!(w.signal(props(OWN, Some(true))), None);
        // beide eigenen ergeben eine Sperre; die erste gemerkte nennt den Grund
        assert_eq!(w.resolved(OWN, true), Ok(Some(Reason::Lock)));
        assert!(w.locked(), "the early hint counts for the state too");
        // die zweite Antwort prüft die übrigen gemerkten Signale (hier die fremde Sitzung, nun eigene)
        assert_eq!(w.resolved(OTHER, true), Ok(Some(Reason::Lock)));
        // danach ist der Speicher leer
        assert!(w.early.is_empty());
        assert_eq!(w.signal(lock("/org/freedesktop/login1/session/_35")), None);
        assert!(w.early.is_empty(), "nothing is kept once all answers are in");

        let mut w = Watch::new();
        w.expect(1);
        w.signal(lock(OTHER));
        w.signal(props(OWN, Some(false)));
        assert_eq!(w.resolved(OWN, true), Ok(None));
    }

    #[test]
    fn early_buffer_is_bounded_and_dropped_when_all_answers_failed() {
        let mut w = Watch::new();
        w.expect(1);
        w.signal(lock(OWN));
        for _ in 0..EARLY_MAX {
            w.signal(lock(OTHER));
        }
        // die eigene (älteste) ist verdrängt
        assert_eq!(w.early.len(), EARLY_MAX);
        assert_eq!(w.resolved(OWN, true), Ok(None));

        let mut w = Watch::new();
        w.expect(2);
        w.signal(lock(OWN));
        w.failed();
        assert!(!w.gave_up());
        w.failed();
        assert!(w.gave_up() && w.early.is_empty());
        assert_eq!(w.signal(lock(OWN)), None);
        assert!(w.early.is_empty());
    }

    #[test]
    fn bad_path_from_get_session_is_refused() {
        let mut w = Watch::new();
        w.expect(1);
        w.signal(lock(OWN));
        assert!(w.resolved("/org/freedesktop/login1/user/self", true).is_err());
        assert!(w.sessions().is_empty());
        assert!(w.gave_up());
    }

    #[test]
    fn locked_at_start() {
        let mut w = Watch::new();
        assert_eq!(w.initial(OWN, true), None, "without a session nothing counts");
        let mut w = watching(&[OWN]);
        assert_eq!(w.initial(OWN, true), Some(Reason::AtStart));
        assert!(w.locked());
        assert_eq!(w.initial(OWN, false), None);
        assert!(!w.locked());
        assert_eq!(w.initial(OTHER, true), None);
    }
}
