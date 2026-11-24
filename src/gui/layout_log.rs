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

//! Lage-Log des Fensters für die GUI-Tests (Lesart L46, W6 Block B): nur mit `DICTATE_DEBUG` schreibt das Fenster
//! nach stderr, wo die benannten Bedienelemente liegen, damit `tests/gui/*.sh` ihre Klickziele daraus lesen statt aus
//! festen Zahlen (eine andere Schrift oder ein längerer Text verschiebt dann keinen Test mehr). Muster wie
//! `panel: layout <id> …` in `dictate-ui`.
//!
//! Zeile: `dictate-settings: layout <id> <x> <y> <w> <h> visible|hidden`, Koordinaten in logischen Pixeln relativ zur
//! linken oberen Ecke des Fensterinhalts (egui-Punkte; `tests/gui/lib.sh` rechnet wie bisher in diese Koordinaten),
//! `visible`, wenn das Element ganz im sichtbaren Ausschnitt seines Bereichs liegt (sonst muss der Test erst scrollen).
//! Verschwindet ein Element, folgt `dictate-settings: layout <id> gone`. Geschrieben wird nur, was sich gegenüber dem
//! vorigen Frame geändert hat; ohne `DICTATE_DEBUG` kostet [`note`] nur eine Abfrage.
//!
//! IDs nur aus festen Namen und Indizes, nie aus Inhalten (Namen der Ping-Ziele, Pfade, Text der Felder sind
//! Nutzerdaten und gehören nie ins Log).

use egui::{Rect, Response, Ui};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Lage eines Elements in einem Frame
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub rect: Rect,
    pub visible: bool,
}

#[derive(Default)]
struct Log {
    /// in diesem Frame gemeldet
    now: BTreeMap<String, Spot>,
    /// zuletzt geschrieben
    last: BTreeMap<String, Spot>,
}

thread_local! {
    static LOG: RefCell<Log> = RefCell::new(Log::default());
}

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DICTATE_DEBUG").is_some())
}

/// Meldet die Lage von `resp` unter `id` für diesen Frame (nur mit `DICTATE_DEBUG`)
pub fn note(ui: &Ui, id: impl FnOnce() -> String, resp: &Response) {
    if !enabled() {
        return;
    }
    let spot = Spot { rect: resp.rect, visible: ui.clip_rect().contains_rect(resp.rect) };
    LOG.with(|l| {
        l.borrow_mut().now.insert(id(), spot);
    });
}

/// Am Ende des Frames: Änderungen schreiben (nur mit `DICTATE_DEBUG`)
pub fn flush() {
    if !enabled() {
        return;
    }
    LOG.with(|l| {
        let mut l = l.borrow_mut();
        let now = std::mem::take(&mut l.now);
        for line in diff(&l.last, &now) {
            eprintln!("dictate-settings: layout {line}");
        }
        l.last = now;
    });
}

/// Zeilen (ohne Vorsatz) für alles, was zwischen `last` und `now` neu ist, sich bewegt hat oder verschwunden ist
pub fn diff(last: &BTreeMap<String, Spot>, now: &BTreeMap<String, Spot>) -> Vec<String> {
    let mut out = Vec::new();
    for (id, s) in now {
        if last.get(id) != Some(s) {
            let r = s.rect;
            let state = if s.visible { "visible" } else { "hidden" };
            out.push(format!("{id} {:.1} {:.1} {:.1} {:.1} {state}", r.left(), r.top(), r.width(), r.height()));
        }
    }
    for id in last.keys().filter(|id| !now.contains_key(*id)) {
        out.push(format!("{id} gone"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn spot(x: f32, y: f32, visible: bool) -> Spot {
        Spot { rect: Rect::from_min_size(pos2(x, y), vec2(80.0, 40.0)), visible }
    }

    fn map(items: &[(&str, Spot)]) -> BTreeMap<String, Spot> {
        items.iter().map(|(k, s)| (k.to_string(), *s)).collect()
    }

    #[test]
    fn only_changes_are_written() {
        let a = map(&[("apply", spot(288.0, 804.0, true)), ("nav:audio", spot(0.0, 146.0, true))]);
        assert_eq!(diff(&BTreeMap::new(), &a), ["apply 288.0 804.0 80.0 40.0 visible", "nav:audio 0.0 146.0 80.0 40.0 visible"]);
        assert!(diff(&a, &a).is_empty(), "an unchanged frame writes nothing");
        let moved = map(&[("apply", spot(288.0, 804.0, true)), ("nav:audio", spot(0.0, 106.5, false))]);
        assert_eq!(diff(&a, &moved), ["nav:audio 0.0 106.5 80.0 40.0 hidden"]);
    }

    #[test]
    fn a_vanished_element_is_reported_once() {
        let a = map(&[("diagnose.ping_targets[2].remove", spot(800.0, 420.0, true))]);
        assert_eq!(diff(&a, &BTreeMap::new()), ["diagnose.ping_targets[2].remove gone"]);
        assert!(diff(&BTreeMap::new(), &BTreeMap::new()).is_empty());
    }
}
