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

//! Halten zum Sprechen unter niri (PLAN-wayland.md 3.2 A, Spike S1): Zustandsmaschine ohne GTK und ohne Uhr.
//!
//! niri meldet nur das Drücken eines Kürzels (`Mod+E` ruft `dictate-ui hold` auf), nie das Loslassen. Das Ende erkennt
//! `dictate-ui` am Tastenzustand der Tastaturen ([`super::keys`], nur `EVIOCGKEY`): Ende, sobald E auf keinem Gerät
//! mehr gedrückt ist, entprellt mit [`DEBOUNCE`] wie früher in Awesome (`debounce` in `init.lua`); Super ist egal.
//! Abgefragt wird nur, solange gehalten wird ([`Out::Keys`]); sonst sind die Geräte geschlossen.
//!
//! Rückfall „Umschalten“ (3.2 B): Sieht die erste Abfrage nach dem Kürzel E nicht gedrückt (Kürzel kam über VNC/wayvnc
//! oder eine virtuelle Tastatur, die nicht unter `/dev/input` erscheint, oder ein sehr kurzer Druck), oder ist gar
//! kein Gerät lesbar, gilt das Kürzel als Schalter: die Aufnahme läuft weiter, bis `Mod+E` noch einmal gedrückt wird
//! (oder der Daemon nach `max_listen_s` selbst stoppt). Das Overlay zeigt dann einen Hinweis ([`Out::Latched`]).
//!
//! Alle Eingaben tragen die Zeit (`now`), damit die Tests die Entprellung und die Fristen genau prüfen können.

use std::time::{Duration, Instant};

/// Abstand der Abfragen des Tastenzustands (S1 live: p99 24,6 ms, Maximum 35 ms bei 20 ms)
pub const POLL: Duration = Duration::from_millis(20);
/// Loslassen wird erst wirksam, wenn E so lange durchgehend losgelassen ist (wie `debounce` in `init.lua`)
pub const DEBOUNCE: Duration = Duration::from_millis(80);
/// Wird E in dieser Zeit nach dem Kürzel nie gedrückt gesehen, kam das Kürzel nicht über eine lesbare Tastatur:
/// Umschalten statt Halten. Das Kürzel erreicht `dictate-ui` je nach Last 10 bis 40 ms nach dem Druck; der
/// kürzeste Druck im Live-Test S1 dauerte 81 ms
pub const GRACE: Duration = Duration::from_millis(150);
/// Höchstdauer des Haltens, wenn der Daemon selbst keine Grenze hat (`max_listen_s = 0`); danach `stop` und die
/// Geräte werden geschlossen (z. B. bei einer hängenden Taste)
pub const CAP_UNLIMITED: Duration = Duration::from_secs(600);
/// Zuschlag auf `max_listen_s`: normalerweise stoppt der Daemon zuerst, `dictate-ui` nur, wenn sein Ende nicht ankam
pub const CAP_MARGIN: Duration = Duration::from_secs(5);

/// Befehl an den Daemon
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonCmd {
    Start,
    Stop,
    Cancel,
    Toggle,
}

/// Folge einer Eingabe, in der angegebenen Reihenfolge auszuführen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Befehl an den Daemon senden
    Daemon(DaemonCmd),
    /// `true`: Geräte öffnen und alle [`POLL`] abfragen ([`Hold::sample`]), erste Abfrage sofort; `false`: schließen
    Keys(bool),
    /// Halten nicht erkannt, die Aufnahme läuft als Schalter weiter (Hinweis im Overlay)
    Latched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    /// Kürzel gedrückt, Abfrage läuft. `seen`: E war seit dem Kürzel mindestens einmal gedrückt; `up_since`: erste
    /// Abfrage mit losgelassenem E nach `seen`
    Holding {
        since: Instant,
        seen: bool,
        up_since: Option<Instant>,
    },
    /// Schalter (Rückfall): nächstes Kürzel stoppt
    Latched,
}

#[derive(Debug)]
pub struct Hold {
    phase: Phase,
    /// Seit dem letzten Kürzel hat der Daemon eine Aufnahme gemeldet (`loading`/`listening`). Erst danach beendet
    /// ein `idle`/`finalizing` des Daemons das Halten; ein älteres Zustands-Event, das noch unterwegs war, nicht
    recording_seen: bool,
    cap: Duration,
}

impl Default for Hold {
    fn default() -> Self {
        Hold::new()
    }
}

impl Hold {
    pub fn new() -> Hold {
        Hold { phase: Phase::Idle, recording_seen: false, cap: CAP_UNLIMITED }
    }

    /// Aufnahmelimit des Daemons (`max_listen_s`, 0 = keins) bestimmt die Höchstdauer des Haltens
    pub fn set_daemon_limit(&mut self, max_listen_s: u64) {
        self.cap = if max_listen_s == 0 { CAP_UNLIMITED } else { Duration::from_secs(max_listen_s) + CAP_MARGIN };
    }

    pub fn cap(&self) -> Duration {
        self.cap
    }

    /// Wird gerade abgefragt (Geräte offen)?
    pub fn polling(&self) -> bool {
        matches!(self.phase, Phase::Holding { .. })
    }

    /// Läuft eine Aufnahme als Schalter (Rückfall)?
    pub fn latched(&self) -> bool {
        self.phase == Phase::Latched
    }

    /// Kürzel `Mod+E` (`dictate-ui hold`)
    pub fn press(&mut self, now: Instant) -> Vec<Out> {
        match self.phase {
            Phase::Idle => {
                self.recording_seen = false;
                self.phase = Phase::Holding { since: now, seen: false, up_since: None };
                vec![Out::Daemon(DaemonCmd::Start), Out::Keys(true)]
            }
            // E kurz losgelassen und binnen der Entprellung wieder gedrückt: weiter gehalten (wie Autorepeat in Awesome)
            Phase::Holding { since, seen: true, .. } => {
                self.phase = Phase::Holding { since, seen: true, up_since: None };
                vec![]
            }
            // Zweites Kürzel, ohne dass E je gedrückt zu sehen war (VNC): als Schalter stoppen
            Phase::Holding { seen: false, .. } => {
                self.phase = Phase::Idle;
                vec![Out::Keys(false), Out::Daemon(DaemonCmd::Stop)]
            }
            Phase::Latched => {
                self.phase = Phase::Idle;
                vec![Out::Daemon(DaemonCmd::Stop)]
            }
        }
    }

    /// Kürzel `Mod+Shift+E` (freihändig): geht unverändert an den Daemon und beendet ein laufendes Halten
    pub fn toggle(&mut self) -> Vec<Out> {
        let mut out = self.end();
        out.push(Out::Daemon(DaemonCmd::Toggle));
        out
    }

    /// Kürzel `Mod+Ctrl+E`: abbrechen; danach stoppt kein Loslassen mehr
    pub fn cancel(&mut self) -> Vec<Out> {
        let mut out = self.end();
        out.push(Out::Daemon(DaemonCmd::Cancel));
        out
    }

    /// Zustand des Daemons: `recording` = `loading` oder `listening`. Endet die Aufnahme beim Daemon (Aufnahmelimit,
    /// Fehler, `stop` von anderswo), endet das Halten ohne weiteren Befehl
    pub fn daemon_state(&mut self, recording: bool) -> Vec<Out> {
        if recording {
            self.recording_seen = true;
            return vec![];
        }
        if !self.recording_seen {
            return vec![];
        }
        self.recording_seen = false;
        self.end()
    }

    /// Ergebnis einer Abfrage: `Some(true)` = E auf irgendeinem Gerät gedrückt, `Some(false)` = auf keinem, `None` =
    /// kein Gerät lesbar
    pub fn sample(&mut self, now: Instant, e: Option<bool>) -> Vec<Out> {
        let Phase::Holding { since, seen, up_since } = self.phase else { return vec![] };
        if now.saturating_duration_since(since) >= self.cap {
            self.phase = Phase::Idle;
            return vec![Out::Keys(false), Out::Daemon(DaemonCmd::Stop)];
        }
        match (e, seen) {
            // Kein Gerät (mehr) lesbar. Vorher nie gesehen: Schalter; sonst (Tastatur abgezogen): wie Loslassen
            (None, false) => self.latch(),
            (None, true) => {
                self.phase = Phase::Idle;
                vec![Out::Keys(false), Out::Daemon(DaemonCmd::Stop)]
            }
            (Some(true), _) => {
                self.phase = Phase::Holding { since, seen: true, up_since: None };
                vec![]
            }
            (Some(false), false) => {
                if now.saturating_duration_since(since) >= GRACE {
                    self.latch()
                } else {
                    vec![]
                }
            }
            (Some(false), true) => {
                let up = up_since.unwrap_or(now);
                if now.saturating_duration_since(up) >= DEBOUNCE {
                    self.phase = Phase::Idle;
                    vec![Out::Keys(false), Out::Daemon(DaemonCmd::Stop)]
                } else {
                    self.phase = Phase::Holding { since, seen: true, up_since: Some(up) };
                    vec![]
                }
            }
        }
    }

    /// Halten bzw. Schalter vergessen, ohne Befehl an den Daemon (Verbindung weg, `start` gescheitert); die
    /// Höchstdauer bleibt
    pub fn reset(&mut self) -> Vec<Out> {
        self.recording_seen = false;
        self.end()
    }

    fn latch(&mut self) -> Vec<Out> {
        self.phase = Phase::Latched;
        vec![Out::Keys(false), Out::Latched]
    }

    /// Halten bzw. Schalter beenden, ohne Befehl an den Daemon
    fn end(&mut self) -> Vec<Out> {
        let was = self.phase;
        self.phase = Phase::Idle;
        match was {
            Phase::Holding { .. } => vec![Out::Keys(false)],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DaemonCmd::*;

    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }

    /// Abfragen im Takt von 20 ms von `from` bis `to` (einschließlich) mit festem Ergebnis
    fn poll(h: &mut Hold, t0: Instant, from: u64, to: u64, e: Option<bool>) -> Vec<(u64, Out)> {
        let mut out = vec![];
        let mut t = from;
        while t <= to {
            out.extend(h.sample(ms(t0, t), e).into_iter().map(|o| (t, o)));
            t += 20;
        }
        out
    }

    #[test]
    fn hold_and_release_stops_after_debounce() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        assert_eq!(h.press(t0), vec![Out::Daemon(Start), Out::Keys(true)]);
        assert!(h.polling());
        assert!(poll(&mut h, t0, 0, 1000, Some(true)).is_empty());
        // losgelassen ab 1020 ms: erst nach 80 ms durchgehend losgelassen wirkt es
        let out = poll(&mut h, t0, 1020, 1200, Some(false));
        assert_eq!(out, vec![(1100, Out::Keys(false)), (1100, Out::Daemon(Stop))]);
        assert!(!h.polling());
        // danach ändern Abfragen nichts mehr
        assert!(h.sample(ms(t0, 1300), Some(true)).is_empty());
    }

    #[test]
    fn short_bounce_is_ignored() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        assert!(h.sample(ms(t0, 0), Some(true)).is_empty());
        // 60 ms los, dann wieder gedrückt: kein Ende
        assert!(poll(&mut h, t0, 20, 60, Some(false)).is_empty());
        assert!(poll(&mut h, t0, 80, 400, Some(true)).is_empty());
        assert!(h.polling());
        // erneut los: die Entprellung beginnt neu
        assert!(poll(&mut h, t0, 420, 480, Some(false)).is_empty());
        assert_eq!(h.sample(ms(t0, 500), Some(false)), vec![Out::Keys(false), Out::Daemon(Stop)]);
    }

    #[test]
    fn repress_within_debounce_keeps_holding() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        h.sample(ms(t0, 0), Some(true));
        h.sample(ms(t0, 20), Some(false));
        // niri löst das Kürzel erneut aus (E neu gedrückt, Super gehalten): kein zweiter Start, weiter gehalten
        assert!(h.press(ms(t0, 40)).is_empty());
        assert!(h.sample(ms(t0, 60), Some(true)).is_empty());
        assert!(h.sample(ms(t0, 120), Some(false)).is_empty());
        assert!(h.sample(ms(t0, 180), Some(false)).is_empty());
        assert_eq!(h.sample(ms(t0, 200), Some(false)), vec![Out::Keys(false), Out::Daemon(Stop)]);
    }

    #[test]
    fn never_seen_down_switches_to_toggle() {
        // VNC: niri löst das Kürzel aus, unter /dev/input ist E nie gedrückt
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        assert!(poll(&mut h, t0, 0, 140, Some(false)).is_empty(), "within the grace time nothing happens");
        assert_eq!(h.sample(ms(t0, 160), Some(false)), vec![Out::Keys(false), Out::Latched]);
        assert!(h.latched() && !h.polling());
        // nichts stoppt außer dem nächsten Kürzel
        assert!(h.sample(ms(t0, 5000), Some(false)).is_empty());
        assert_eq!(h.press(ms(t0, 9000)), vec![Out::Daemon(Stop)]);
        assert!(!h.latched());
        // und das übernächste startet wieder
        assert_eq!(h.press(ms(t0, 9500)), vec![Out::Daemon(Start), Out::Keys(true)]);
    }

    #[test]
    fn no_readable_device_switches_to_toggle_at_once() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        assert_eq!(h.sample(t0, None), vec![Out::Keys(false), Out::Latched]);
        assert_eq!(h.press(ms(t0, 3000)), vec![Out::Daemon(Stop)]);
    }

    #[test]
    fn second_press_before_grace_without_keys_stops() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        h.sample(t0, Some(false));
        assert_eq!(h.press(ms(t0, 100)), vec![Out::Keys(false), Out::Daemon(Stop)]);
        assert!(!h.polling());
    }

    #[test]
    fn keyboard_unplugged_while_holding_stops() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        h.sample(t0, Some(true));
        assert_eq!(h.sample(ms(t0, 20), None), vec![Out::Keys(false), Out::Daemon(Stop)]);
    }

    #[test]
    fn cancel_ends_polling_and_release_sends_nothing() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        h.sample(t0, Some(true));
        assert_eq!(h.cancel(), vec![Out::Keys(false), Out::Daemon(Cancel)]);
        assert!(!h.polling());
        assert!(poll(&mut h, t0, 20, 500, Some(false)).is_empty());
        // Abbrechen ohne Halten geht nur an den Daemon
        assert_eq!(h.cancel(), vec![Out::Daemon(Cancel)]);
        // auch der Schalter wird beendet
        h.press(t0);
        h.sample(t0, None);
        assert_eq!(h.cancel(), vec![Out::Daemon(Cancel)]);
        assert!(!h.latched());
    }

    #[test]
    fn toggle_goes_to_the_daemon_and_ends_holding() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        assert_eq!(h.toggle(), vec![Out::Daemon(Toggle)]);
        h.press(t0);
        assert_eq!(h.toggle(), vec![Out::Keys(false), Out::Daemon(Toggle)]);
        assert!(!h.polling());
    }

    #[test]
    fn daemon_end_ends_holding_only_after_recording_was_seen() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.press(t0);
        // ein altes `idle`/`finalizing` (vorige Sitzung) vor dem Start dieser Aufnahme zählt nicht
        assert!(h.daemon_state(false).is_empty());
        assert!(h.polling());
        assert!(h.daemon_state(true).is_empty());
        // Aufnahmelimit des Daemons: Halten endet, ohne dass dictate-ui noch etwas sendet
        assert_eq!(h.daemon_state(false), vec![Out::Keys(false)]);
        assert!(!h.polling());
        // Schalter endet ebenso
        h.press(t0);
        h.sample(t0, None);
        h.daemon_state(true);
        assert!(h.daemon_state(false).is_empty());
        assert!(!h.latched());
    }

    #[test]
    fn reset_keeps_the_cap() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.set_daemon_limit(2);
        h.press(t0);
        assert_eq!(h.reset(), vec![Out::Keys(false)]);
        assert!(!h.polling());
        assert_eq!(h.cap(), Duration::from_secs(7));
        assert!(h.reset().is_empty());
    }

    #[test]
    fn cap_stops_a_stuck_key() {
        let t0 = Instant::now();
        let mut h = Hold::new();
        h.set_daemon_limit(2);
        assert_eq!(h.cap(), Duration::from_secs(7));
        h.press(t0);
        assert!(poll(&mut h, t0, 0, 6980, Some(true)).is_empty());
        assert_eq!(h.sample(ms(t0, 7000), Some(true)), vec![Out::Keys(false), Out::Daemon(Stop)]);
        h.set_daemon_limit(0);
        assert_eq!(h.cap(), CAP_UNLIMITED);
    }

    #[test]
    fn every_end_of_holding_closes_the_devices() {
        // Regel aus PLAN-wayland.md 3.2 A: Geräte nur während des Haltens offen. Jeder Weg aus `Holding` muss
        // `Keys(false)` liefern
        type Step = fn(&mut Hold, Instant) -> Vec<Out>;
        let t0 = Instant::now();
        let ends: [(&str, Step); 7] = [
            ("release", |h, t| [h.sample(t, Some(true)), h.sample(ms(t, 20), Some(false)), h.sample(ms(t, 100), Some(false))].concat()),
            ("latch", |h, t| h.sample(ms(t, 200), Some(false))),
            ("no device", |h, t| h.sample(t, None)),
            ("cancel", |h, _| h.cancel()),
            ("toggle", |h, _| h.toggle()),
            ("daemon end", |h, _| [h.daemon_state(true), h.daemon_state(false)].concat()),
            ("cap", |h, t| h.sample(t + CAP_UNLIMITED, Some(true))),
        ];
        for (name, end) in ends {
            let mut h = Hold::new();
            h.press(t0);
            let out = end(&mut h, t0);
            assert!(!h.polling(), "{name}: still polling");
            assert_eq!(out.iter().filter(|o| **o == Out::Keys(false)).count(), 1, "{name}: {out:?}");
        }
    }
}
