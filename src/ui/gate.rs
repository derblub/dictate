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

//! Eingabesperre der Panels (Textaktionen `ui::panel`, Diagnose `ui::diag_panel`; Befund 9 in PLAN-wayland.md 6b): rein,
//! ohne GTK und ohne Uhr, jede Eingabe trägt `now`.
//!
//! Totzeit: nach dem Öffnen und nach jedem Zustandswechsel lösen für [`DEADTIME`] weder Tasten noch Klicks aus, und
//! Tasten, die beim Wechsel gedrückt waren, erst nach ihrem Loslassen (kein doppeltes `r`). Ein Drücken binnen
//! [`REPEAT_GAP`] nach dem Loslassen derselben Taste gilt als Wiederholung (X schickte bei Autorepeat Loslassen und
//! Drücken; GTK unter Wayland wiederholt nur mit Drücken; beide Formen sind in `ui::panel` getestet). Dazu die Zeit der
//! letzten Eingabe für den Leerlauf ([`IDLE`]).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Totzeit nach jedem Zustandswechsel (`actions_deadtime` in init.lua)
pub const DEADTIME: Duration = Duration::from_millis(300);
/// Ohne Eingabe schließt ein Panel nach dieser Zeit (wie die Sitzung im Daemon)
pub const IDLE: Duration = Duration::from_secs(300);
/// Drücken so kurz nach dem Loslassen derselben Taste gilt als Wiederholung
pub const REPEAT_GAP: Duration = Duration::from_millis(50);
/// Ein Loslassen so kurz vor einem Zustandswechsel gilt als Teil einer Wiederholung (die Taste bleibt alt)
const RELEASE_STALE: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub struct Gate {
    ready_at: Option<Instant>,
    last_input: Option<Instant>,
    down: HashSet<String>,
    stale: HashSet<String>,
    released: HashMap<String, Instant>,
    /// Zähler für Tests und das Log (keine Inhalte): Eingaben, die die Sperre verworfen hat
    pub ignored: u64,
}

impl Gate {
    pub fn new() -> Gate {
        Gate::default()
    }

    /// Öffnen: Totzeit ab `now`, keine alten Tasten
    pub fn open(&mut self, now: Instant) {
        self.clear();
        self.ready_at = Some(now + DEADTIME);
        self.last_input = Some(now);
    }

    /// Schließen: alles vergessen
    pub fn clear(&mut self) {
        self.ready_at = None;
        self.last_input = None;
        self.down.clear();
        self.stale.clear();
        self.released.clear();
    }

    /// Zustandswechsel: Totzeit neu, gedrückte und gerade losgelassene Tasten gelten als alt
    pub fn changed(&mut self, now: Instant) {
        self.ready_at = Some(now + DEADTIME);
        for (k, r) in &self.released {
            if now.saturating_duration_since(*r) < RELEASE_STALE {
                self.stale.insert(k.clone());
            }
        }
        for k in &self.down {
            self.stale.insert(k.clone());
            self.released.remove(k);
        }
    }

    /// Nach der Totzeit (ohne Öffnung: immer)
    pub fn ready(&self, now: Instant) -> bool {
        self.ready_at.is_none_or(|t| now >= t)
    }

    /// Eingabe des Users (Leerlauf beginnt neu)
    pub fn touch(&mut self, now: Instant) {
        self.last_input = Some(now);
    }

    pub fn last_input(&self) -> Option<Instant> {
        self.last_input
    }

    /// Leerlauf abgelaufen
    pub fn idle(&self, now: Instant) -> bool {
        self.last_input.is_some_and(|t| now >= t + IDLE)
    }

    /// Taste gedrückt (`key` wie `gdk::Key::name()`): eine alte Taste wird wieder frisch, wenn sie lange genug los war
    pub fn press(&mut self, key: &str, now: Instant) {
        if self.stale.contains(key) && self.released.get(key).is_some_and(|r| now.saturating_duration_since(*r) > REPEAT_GAP) {
            self.stale.remove(key);
        }
        self.down.insert(key.to_string());
    }

    pub fn release(&mut self, key: &str, now: Instant) {
        self.down.remove(key);
        self.released.insert(key.to_string(), now);
    }

    /// Darf diese Taste auslösen? Nach der Totzeit und nur bei neuem Druck; sonst gezählt
    pub fn fire(&mut self, key: &str, now: Instant) -> bool {
        if !self.ready(now) || self.stale.contains(key) {
            self.ignored += 1;
            return false;
        }
        true
    }

    /// Darf ein Klick auslösen? Nach der Totzeit; sonst gezählt
    pub fn click(&mut self, now: Instant) -> bool {
        if !self.ready(now) {
            self.ignored += 1;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadtime_stale_and_idle() {
        let mut g = Gate::new();
        let t0 = Instant::now();
        assert!(g.ready(t0), "closed: no deadtime");
        g.open(t0);
        assert!(!g.click(t0 + Duration::from_millis(10)));
        assert!(g.click(t0 + DEADTIME));
        // gehalten über einen Wechsel: alt bis zum Loslassen
        g.press("r", t0);
        g.changed(t0 + DEADTIME);
        let t = t0 + DEADTIME * 3;
        g.press("r", t);
        assert!(!g.fire("r", t));
        g.release("r", t + Duration::from_millis(10));
        g.press("r", t + Duration::from_millis(200));
        assert!(g.fire("r", t + Duration::from_millis(200)));
        assert_eq!(g.ignored, 2);
        assert!(!g.idle(t0 + IDLE - Duration::from_millis(1)));
        assert!(g.idle(t0 + IDLE));
        g.touch(t0 + Duration::from_secs(10));
        assert!(!g.idle(t0 + IDLE));
        g.clear();
        assert!(!g.idle(t0 + IDLE * 2));
    }
}
