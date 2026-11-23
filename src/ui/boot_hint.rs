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

//! Hinweis nach einem unsauberen Ende des vorigen Boots im Overlay von `dictate-ui` (PLAN-wayland.md 6e Punkt 6,
//! Befund 12; Daemon-Seite `diagnose::boot_hint`). Rein, ohne GTK: wann gefragt wird und was eine Antwort bedeutet.
//!
//! Ablauf wie unter Awesome (`init.lua` 1495–1511): erste Frage 10 s nach dem Start; fehlt der Daemon oder sammelt er
//! gerade (`busy`), bis zu [`ATTEMPTS`]-mal im Abstand von 20 s; „ja“ zeigt 20 s lang [`TEXT`]. Jede andere Antwort
//! (aus, sauber, schon geprüft, Fehler, Frist) beendet das Fragen: der Daemon merkt sich den Boot selbst.
//!
//! Anders als unter Awesome (Entscheidung Daniel, 21.11.): Klick und Kürzel öffnen die Diagnose; solange der Hinweis
//! steht, hat das Overlay eine Eingaberegion in seiner Fläche, danach wieder keine. Ein „ja“, das gerade nicht gezeigt
//! werden kann (Aufnahme, anderer Hinweis, Sitzung gesperrt), wartet, bis das Overlay frei ist: der Daemon hat den Boot
//! dann schon gemerkt, ein verworfenes „ja“ käme nie wieder. Die 20 s zählen ab dem ersten Zeigen.

use std::time::Duration;

/// Text im Overlay (nur das Kürzel, kein Inhalt)
pub const TEXT: &str = "Last boot ended uncleanly · Mod+Shift+I to diagnose";
/// So oft wird höchstens gefragt (`boot_hint_attempts`)
pub const ATTEMPTS: u32 = 4;

/// Zeiten des Ablaufs; in Tests über `dictate-ui run --boot-hint-ms` verkürzt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Erste Frage nach dem Start (`boot_hint_delay`)
    pub first: Duration,
    /// Abstand weiterer Fragen (`boot_hint_retry`)
    pub retry: Duration,
    /// So lange steht der Hinweis (`boot_hint_timeout`)
    pub show: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing { first: Duration::from_secs(10), retry: Duration::from_secs(20), show: Duration::from_secs(20) }
    }
}

impl Timing {
    /// `ERSTE,ABSTAND,ANZEIGE` in Millisekunden (nur für Tests), jede Zahl 1..=600000
    pub fn parse(s: &str) -> Result<Timing, String> {
        let ms: Vec<u64> = s.split(',').map(|p| p.trim().parse::<u64>().map_err(|_| format!("not a number: {p:?}"))).collect::<Result<_, _>>()?;
        let [first, retry, show] = ms[..] else { return Err("want three numbers: FIRST,RETRY,SHOW (ms)".into()) };
        if [first, retry, show].iter().any(|&m| m == 0 || m > 600_000) {
            return Err("each time must be 1..=600000 ms".into());
        }
        Ok(Timing { first: Duration::from_millis(first), retry: Duration::from_millis(retry), show: Duration::from_millis(show) })
    }
}

/// Was die Antwort auf `diag_boot_hint` bedeutet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// `show: true`
    Show,
    /// `show: false, reason: "busy"`: gerade läuft eine Sammlung, nichts entschieden
    Busy,
    /// Kein Daemon (Socket fehlt, Verbindung abgelehnt oder ohne Antwort zu)
    DaemonDown,
    /// Alles andere: aus, sauber, schon geprüft, unbekannt, Fehler, Frist
    No,
}

impl Answer {
    /// Aus der Antwortzeile (`ok: true`)
    pub fn from_reply(v: &serde_json::Value) -> Answer {
        if v["show"] == true {
            Answer::Show
        } else if v["reason"] == "busy" {
            Answer::Busy
        } else {
            Answer::No
        }
    }

    /// Fürs Log (nie mehr als das)
    pub fn name(self) -> &'static str {
        match self {
            Answer::Show => "show",
            Answer::Busy => "busy",
            Answer::DaemonDown => "no daemon",
            Answer::No => "not needed",
        }
    }
}

/// Nächster Schritt nach einer Antwort
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// In dieser Zeit noch einmal fragen
    Ask(Duration),
    /// Hinweis zeigen, sobald das Overlay frei ist
    Show,
    /// Nicht mehr fragen
    Done,
}

/// Zählt die Fragen
#[derive(Debug)]
pub struct Asker {
    timing: Timing,
    asked: u32,
}

impl Asker {
    pub fn new(timing: Timing) -> Asker {
        Asker { timing, asked: 0 }
    }

    pub fn timing(&self) -> Timing {
        self.timing
    }

    /// Nummer der nächsten Frage (ab 1), fürs Log
    pub fn attempt(&self) -> u32 {
        self.asked + 1
    }

    /// Antwort auf die laufende Frage
    pub fn answer(&mut self, a: Answer) -> Next {
        self.asked += 1;
        match a {
            Answer::Show => Next::Show,
            Answer::Busy | Answer::DaemonDown if self.asked < ATTEMPTS => Next::Ask(self.timing.retry),
            Answer::Busy | Answer::DaemonDown | Answer::No => Next::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_timing_is_the_one_from_awesome() {
        let t = Timing::default();
        assert_eq!((t.first, t.retry, t.show), (Duration::from_secs(10), Duration::from_secs(20), Duration::from_secs(20)));
        assert_eq!(Asker::new(t).timing(), t);
    }

    #[test]
    fn replies_are_classified() {
        assert_eq!(Answer::from_reply(&json!({"ok": true, "show": true, "reason": "unclean"})), Answer::Show);
        assert_eq!(Answer::from_reply(&json!({"ok": true, "show": false, "reason": "busy"})), Answer::Busy);
        for r in ["off", "clean", "already_checked", "no_previous", "unknown"] {
            assert_eq!(Answer::from_reply(&json!({"ok": true, "show": false, "reason": r})), Answer::No, "{r}");
        }
        // kein Bool: nie zeigen
        assert_eq!(Answer::from_reply(&json!({"ok": true, "show": "yes"})), Answer::No);
        assert_eq!(Answer::from_reply(&json!({"ok": true})), Answer::No);
    }

    #[test]
    fn busy_and_a_missing_daemon_ask_again_up_to_four_times() {
        for a in [Answer::Busy, Answer::DaemonDown] {
            let mut k = Asker::new(Timing::default());
            assert_eq!(k.attempt(), 1);
            for n in 1..ATTEMPTS {
                assert_eq!(k.answer(a), Next::Ask(Duration::from_secs(20)), "{a:?} attempt {n}");
                assert_eq!(k.attempt(), n + 1);
            }
            assert_eq!(k.answer(a), Next::Done, "{a:?}: the fourth answer ends it");
        }
    }

    #[test]
    fn show_ends_the_asking_also_on_the_last_attempt() {
        let mut k = Asker::new(Timing::default());
        assert_eq!(k.answer(Answer::Show), Next::Show);
        let mut k = Asker::new(Timing::default());
        for _ in 1..ATTEMPTS {
            assert_eq!(k.answer(Answer::DaemonDown), Next::Ask(Duration::from_secs(20)));
        }
        assert_eq!(k.answer(Answer::Show), Next::Show);
    }

    #[test]
    fn any_other_answer_ends_the_asking_at_once() {
        let mut k = Asker::new(Timing::default());
        assert_eq!(k.answer(Answer::No), Next::Done);
        let mut k = Asker::new(Timing::default());
        assert_eq!(k.answer(Answer::Busy), Next::Ask(Duration::from_secs(20)));
        assert_eq!(k.answer(Answer::No), Next::Done);
    }

    #[test]
    fn test_timing_parses_only_three_sane_numbers() {
        let t = Timing::parse("1000, 1500,6000").unwrap();
        assert_eq!((t.first, t.retry, t.show), (Duration::from_millis(1000), Duration::from_millis(1500), Duration::from_millis(6000)));
        for bad in ["", "1,2", "1,2,3,4", "a,2,3", "0,2,3", "1,2,600001", "-1,2,3"] {
            assert!(Timing::parse(bad).is_err(), "{bad:?}");
        }
    }
}
