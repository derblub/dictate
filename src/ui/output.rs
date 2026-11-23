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

//! Textausgabe von `dictate-ui` ohne Wayland (PLAN-wayland.md 3.4, Block W2): Warteschlange der fertigen Texte,
//! Bereinigen, Zerlegen in Zeilen und Tasten, und die Entscheidung, auf welchem Weg geschrieben wird. Ausgeführt wird
//! im Ausgabe-Thread (`src/bin/dictate-ui/output.rs`).
//!
//! **Wege** (Entscheidungen 8 und 11): Aktiviert das Zielfeld die Eingabemethode (T3), wird der Text übergeben,
//! außer die `app_id` steht in `typing.keyboard_only_app_ids`. Sonst wird außerhalb von Terminals über die
//! Zwischenablage eingefügt (`typing.output = "paste"`), in Terminals und **im Zweifel** (Fokus oder `app_id` unbekannt,
//! Regel 4.8; Awesome fügte dann ein, Befund 1) mit der eigenen virtuellen Tastatur (T2) getippt. Zeilenumbrüche gehen
//! als Taste: in Terminals Return, sonst Shift+Return (Chatfelder, in denen Return sendet). Ob die Zwischenablage
//! wirklich benutzt wird, entscheidet der Ausgabe-Thread danach (Passwort darin, Entscheidung 10: tippen).
//!
//! **Warteschlange** (Befunde 2, 3 und 12 aus 6b): ein Auftrag zur Zeit; Mod+Ctrl+E leert sie und bricht den laufenden
//! ab; ein `final`, das nach einem Abbruch einer laufenden Aufnahme noch kommt, wird verworfen. Aus `final.keys` gilt
//! nur `BackSpace` (nichts anderes aus dem Socket wird je eine Taste).

use std::collections::VecDeque;

/// Steuerzeichen entfernen: nur `\n` bleibt (Zeilenumbruch als Taste), Tab wird Leerzeichen (eine Tab-Taste
/// wechselte das Feld, in Terminals vervollständigte sie), `\r`, die übrigen C0-Zeichen, DEL und C1 fallen weg. Eine
/// Eingabemethode reicht Text roh an das Programm; in einem Terminal wäre z. B. ESC der Anfang einer Steuerfolge.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' => Some('\n'),
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Stücke einer Ausgabe, deren Text beim Verwerfen überschrieben wird (diktierter Text, KI-Antworten beim Replace)
pub struct Pieces(pub Vec<Piece>);

impl std::ops::Deref for Pieces {
    type Target = [Piece];
    fn deref(&self) -> &[Piece] {
        &self.0
    }
}

impl Drop for Pieces {
    fn drop(&mut self) {
        for p in &mut self.0 {
            if let Piece::Text(t) = p {
                crate::actions::store::wipe(t);
            }
        }
    }
}

/// Teil einer Ausgabe
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Text ohne Steuerzeichen und ohne Zeilenumbruch
    Text(String),
    /// So viele Zeilenumbrüche hintereinander
    Newlines(usize),
    /// Rücktaste (aus `final.keys`, Sprachbefehl „rückgängig“)
    BackSpace,
}

/// Text (bereinigt) in Zeilen und Umbrüche, danach die erlaubten Tasten
pub fn pieces(text: &str, keys: &[String]) -> Vec<Piece> {
    let text = sanitize(text);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let line_end = rest.find('\n').unwrap_or(rest.len());
        if line_end > 0 {
            out.push(Piece::Text(rest[..line_end].to_string()));
        }
        rest = &rest[line_end..];
        let n = rest.len() - rest.trim_start_matches('\n').len();
        if n > 0 {
            out.push(Piece::Newlines(n));
            rest = &rest[n..];
        }
    }
    out.extend(allowed_keys(keys).map(|_| Piece::BackSpace));
    out
}

/// Nur `BackSpace` ist als Taste aus dem Socket erlaubt (`parse_keys` in init.lua)
pub fn allowed_keys(keys: &[String]) -> impl Iterator<Item = &String> {
    keys.iter().filter(|k| k.as_str() == "BackSpace")
}

/// Teilt `s` in Stücke von höchstens `max` Bytes, an Zeichengrenzen und möglichst nach einem Leerzeichen
/// (Wayland-Nachrichten haben eine feste Höchstgröße; `commit_string` trägt den Text in einer Nachricht)
pub fn split_bytes(s: &str, max: usize) -> Vec<&str> {
    assert!(max >= 4, "room for one UTF-8 char");
    let mut out = Vec::new();
    let mut rest = s;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        // nach dem letzten Leerzeichen der zweiten Hälfte, sonst an der Zeichengrenze
        if let Some(sp) = rest[..cut].rfind(' ').filter(|&i| i + 1 > cut / 2) {
            cut = sp + 1;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// Weg einer Ausgabe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Eingabemethode (T3)
    InputMethod,
    /// Über die Zwischenablage einfügen (Data-Control, `Ctrl+V` über T2)
    Paste,
    /// Eigene virtuelle Tastatur (T2)
    Keyboard,
}

impl Route {
    pub fn name(self) -> &'static str {
        match self {
            Route::InputMethod => "input-method",
            Route::Paste => "paste",
            Route::Keyboard => "keyboard",
        }
    }
}

/// Einstellungen der Ausgabe (aus `get_config` und dem Event `terminals`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutCfg {
    /// Pause nach jedem Anschlag von T2 (`typing.delay_ms`)
    pub delay_ms: u64,
    /// `typing.output = "paste"`
    pub paste: bool,
    pub keyboard_only: Vec<String>,
    /// `actions.terminal_classes` (`app_id` bzw. X11-Klasse unter XWayland)
    pub terminals: Vec<String>,
}

impl Default for OutCfg {
    fn default() -> Self {
        // Vorgaben wie `config::TypingCfg` und `ActionsCfg`, falls der Daemon (älter) die Schlüssel nicht kennt
        let t = crate::config::TypingCfg::default();
        OutCfg {
            delay_ms: t.delay_ms,
            paste: t.output == "paste",
            keyboard_only: t.keyboard_only_app_ids,
            terminals: crate::config::ActionsCfg::default().terminal_classes,
        }
    }
}

impl OutCfg {
    /// Aus der Antwort von `get_config` (`config` als JSON); fehlende oder falsche Schlüssel behalten die Vorgabe
    pub fn from_config_json(c: &serde_json::Value) -> OutCfg {
        let mut o = OutCfg::default();
        let strings = |v: &serde_json::Value| -> Option<Vec<String>> {
            v.as_array().map(|a| a.iter().filter_map(|s| s.as_str()).map(str::to_string).filter(|s| !s.trim().is_empty()).collect())
        };
        if let Some(d) = c["typing"]["delay_ms"].as_u64() {
            o.delay_ms = d.min(1000);
        }
        if let Some(out) = c["typing"]["output"].as_str() {
            o.paste = out != "type";
        }
        if let Some(k) = strings(&c["typing"]["keyboard_only_app_ids"]) {
            o.keyboard_only = k;
        }
        if let Some(t) = strings(&c["actions"]["terminal_classes"]) {
            o.terminals = t;
        }
        o
    }
}

/// Was über das Zielfeld bekannt ist, wenn die Ausgabe beginnt
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// `app_id` des fokussierten Fensters (niri-IPC); `None`: unbekannt
    pub app_id: Option<String>,
    /// Die Eingabemethode ist für das fokussierte Feld aktiv
    pub im_active: bool,
    /// Die Eingabemethode meldet den Zweck `terminal` (`content_type`)
    pub im_terminal: bool,
}

/// Ist das Ziel ein Terminal? Liste ohne Groß/klein, oder der Zweck des Felds sagt es
pub fn is_terminal(t: &Target, cfg: &OutCfg) -> bool {
    t.im_terminal || t.app_id.as_deref().is_some_and(|a| cfg.terminals.iter().any(|c| c.eq_ignore_ascii_case(a)))
}

/// Weg für den Text einer Ausgabe
pub fn route(t: &Target, cfg: &OutCfg) -> Route {
    let keyboard_only = t.app_id.as_deref().is_some_and(|a| cfg.keyboard_only.iter().any(|c| c.eq_ignore_ascii_case(a)));
    if t.im_active && !keyboard_only {
        Route::InputMethod
    } else if cfg.paste && t.app_id.is_some() && !is_terminal(t, cfg) {
        Route::Paste
    } else {
        // Terminal, `typing.output = "type"` oder Ziel unbekannt: tippen
        Route::Keyboard
    }
}

/// Ein Auftrag der Warteschlange: ein fertiger Text samt Tasten
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub text: String,
    pub keys: Vec<String>,
    /// Replace der Textaktionen: niri-ID des Fensters, in dem ersetzt wird (nur dort, nie in einem Terminal; die
    /// Ausgabe wartet begrenzt, bis niri den Fokus nach dem Schließen des Panels dorthin zurückgegeben hat)
    pub replace: Option<u64>,
}

/// Text eines Auftrags (Diktat, Antwort beim Replace) beim Verwerfen überschreiben (Durchsicht W3 N4)
impl Drop for Job {
    fn drop(&mut self) {
        crate::actions::store::wipe(&mut self.text);
    }
}

impl Job {
    /// Zahlen fürs Log (nie der Text)
    pub fn describe(&self) -> String {
        format!("job {}: {} chars, {} keys", self.id, self.text.chars().count(), allowed_keys(&self.keys).count())
    }
}

/// Ergebnis von [`Queue::cancel`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled {
    /// Laufender Auftrag, der abgebrochen werden muss
    pub running: Option<u64>,
    /// Verworfene wartende Aufträge
    pub dropped: usize,
}

/// Warteschlange der Ausgabe (ein Auftrag zur Zeit)
#[derive(Debug, Default)]
pub struct Queue {
    waiting: VecDeque<Job>,
    running: Option<u64>,
    /// Ein Abbruch kam, während eine Aufnahme lief: das nächste `final` verwerfen
    discard: bool,
    next_id: u64,
}

impl Queue {
    pub fn new() -> Queue {
        Queue::default()
    }

    /// Eine neue Aufnahme beginnt: ein offenes Verwerfen gilt nicht mehr (kommt nach einem Abbruch kein `final`,
    /// soll das der neuen Aufnahme nicht verschluckt werden)
    pub fn recording_started(&mut self) {
        self.discard = false;
    }

    /// `final` des Daemons; liefert, ob ein Auftrag eingereiht wurde
    pub fn on_final(&mut self, text: &str, keys: &[String], cancelled: bool) -> bool {
        if std::mem::take(&mut self.discard) || cancelled {
            return false;
        }
        let keys: Vec<String> = allowed_keys(keys).cloned().collect();
        if sanitize(text).is_empty() && keys.is_empty() {
            return false;
        }
        self.next_id += 1;
        self.waiting.push_back(Job { id: self.next_id, text: text.to_string(), keys, replace: None });
        true
    }

    /// Replace der Textaktionen einreihen (hinter laufende Diktate); liefert die Nummer des Auftrags
    pub fn push_replace(&mut self, text: String, window: u64) -> u64 {
        self.next_id += 1;
        self.waiting.push_back(Job { id: self.next_id, text, keys: Vec::new(), replace: Some(window) });
        self.next_id
    }

    /// Mod+Ctrl+E: wartende Aufträge verwerfen, den laufenden abbrechen. `in_flight`: eine Aufnahme lief (lädt, hört
    /// zu oder schließt ab); dann wird das nächste `final` verworfen (Befund 3)
    pub fn cancel(&mut self, in_flight: bool) -> Cancelled {
        let dropped = self.waiting.len();
        self.waiting.clear();
        if in_flight {
            self.discard = true;
        }
        Cancelled { running: self.running, dropped }
    }

    /// Nächster Auftrag, wenn keiner läuft
    pub fn take_next(&mut self) -> Option<Job> {
        if self.running.is_some() {
            return None;
        }
        let job = self.waiting.pop_front()?;
        self.running = Some(job.id);
        Some(job)
    }

    /// Auftrag `id` ist fertig (oder abgebrochen); erst jetzt darf der nächste beginnen (ein abgebrochenes Einfügen
    /// legt die Zwischenablage erst zurück, Befund 2)
    pub fn done(&mut self, id: u64) {
        if self.running == Some(id) {
            self.running = None;
        }
    }

    pub fn running(&self) -> Option<u64> {
        self.running
    }

    /// Nichts läuft und nichts wartet
    pub fn idle(&self) -> bool {
        self.running.is_none() && self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn replace_jobs_queue_behind_dictation() {
        let mut q = Queue::new();
        assert!(q.on_final("Diktat", &[], false));
        let id = q.push_replace("Neu".into(), 7);
        let a = q.take_next().unwrap();
        assert_eq!(a.replace, None);
        assert!(q.take_next().is_none());
        q.done(a.id);
        let b = q.take_next().unwrap();
        assert_eq!((b.id, b.replace, b.text.as_str()), (id, Some(7), "Neu"));
        // Abbruch verwirft auch ein wartendes Replace
        q.done(b.id);
        q.push_replace("x".into(), 7);
        assert_eq!(q.cancel(false).dropped, 1);
    }

    #[test]
    fn sanitize_keeps_newlines_only() {
        assert_eq!(sanitize("a\tb\r\nc\u{1b}[31m\u{7f}\u{9b}d\u{0}"), "a b\nc[31md");
        assert_eq!(sanitize("Grüße „ok“ €"), "Grüße „ok“ €");
    }

    #[test]
    fn pieces_split_lines_and_keys() {
        let p = pieces("Hallo\n\nWelt\n", &keys(&["BackSpace", "Return", "ctrl+w", "BackSpace"]));
        assert_eq!(
            p,
            vec![Piece::Text("Hallo".into()), Piece::Newlines(2), Piece::Text("Welt".into()), Piece::Newlines(1), Piece::BackSpace, Piece::BackSpace]
        );
        assert_eq!(pieces("\nx", &[]), vec![Piece::Newlines(1), Piece::Text("x".into())]);
        assert_eq!(pieces("\u{1b}", &[]), vec![]);
        assert_eq!(pieces("a\r\nb", &[]), vec![Piece::Text("a".into()), Piece::Newlines(1), Piece::Text("b".into())]);
    }

    #[test]
    fn split_bytes_at_char_and_word_boundaries() {
        let s = "äöü ".repeat(500);
        let parts = split_bytes(&s, 1000);
        assert_eq!(parts.concat(), s);
        assert!(parts.iter().all(|p| p.len() <= 1000 && p.len() > 500));
        assert!(parts[..parts.len() - 1].iter().all(|p| p.ends_with(' ')));
        let long = "€".repeat(1000);
        let parts = split_bytes(&long, 10);
        assert_eq!(parts.concat(), long);
        assert!(parts.iter().all(|p| p.len() <= 9 && p.len() % 3 == 0));
        assert_eq!(split_bytes("kurz", 10), vec!["kurz"]);
        assert!(split_bytes("", 10).is_empty());
    }

    fn cfg() -> OutCfg {
        OutCfg { delay_ms: 12, paste: true, keyboard_only: keys(&["com.example.Broken"]), terminals: keys(&["Alacritty", "XTerm"]) }
    }

    fn target(app: Option<&str>, im: bool) -> Target {
        Target { app_id: app.map(str::to_string), im_active: im, im_terminal: false }
    }

    #[test]
    fn route_prefers_the_input_method() {
        let c = cfg();
        assert_eq!(route(&target(Some("firefox"), true), &c), Route::InputMethod);
        assert_eq!(route(&target(Some("Alacritty"), true), &c), Route::InputMethod);
        assert_eq!(route(&target(None, true), &c), Route::InputMethod);
        // Ausnahme: nie T3, dann wie ohne Eingabemethode
        assert_eq!(route(&target(Some("com.example.broken"), true), &c), Route::Paste);
    }

    #[test]
    fn route_without_input_method_types_in_terminals_and_in_doubt() {
        let mut c = cfg();
        assert_eq!(route(&target(Some("spotify"), false), &c), Route::Paste);
        assert_eq!(route(&target(Some("xterm"), false), &c), Route::Keyboard, "case-insensitive");
        assert_eq!(route(&target(None, false), &c), Route::Keyboard, "unknown focus: type (Befund 1)");
        let t = Target { app_id: Some("unknown".into()), im_active: false, im_terminal: true };
        assert_eq!(route(&t, &c), Route::Keyboard, "field says terminal");
        c.paste = false;
        assert_eq!(route(&target(Some("spotify"), false), &c), Route::Keyboard);
    }

    #[test]
    fn terminal_decision() {
        let c = cfg();
        assert!(is_terminal(&target(Some("alacritty"), false), &c));
        assert!(!is_terminal(&target(Some("Alacritty2"), false), &c));
        assert!(!is_terminal(&target(None, true), &c));
    }

    #[test]
    fn cfg_from_config_json() {
        let j = serde_json::json!({"typing": {"delay_ms": 3, "output": "type", "keyboard_only_app_ids": ["a", " "]},
            "actions": {"terminal_classes": ["foot"]}});
        let o = OutCfg::from_config_json(&j);
        assert_eq!(o, OutCfg { delay_ms: 3, paste: false, keyboard_only: keys(&["a"]), terminals: keys(&["foot"]) });
        // älterer Daemon ohne die neuen Schlüssel: Vorgaben
        let old = OutCfg::from_config_json(&serde_json::json!({"typing": {"delay_ms": 12}}));
        assert!(old.paste && old.keyboard_only.is_empty() && old.terminals.iter().any(|t| t == "com.mitchellh.ghostty"));
    }

    #[test]
    fn queue_runs_one_job_at_a_time() {
        let mut q = Queue::new();
        assert!(q.on_final("eins", &[], false));
        assert!(q.on_final("", &keys(&["BackSpace"]), false));
        assert!(!q.on_final("", &keys(&["Return"]), false), "no allowed key, no text");
        assert!(!q.on_final("weg", &[], true), "cancelled final");
        let a = q.take_next().unwrap();
        assert_eq!(a.text, "eins");
        assert_eq!(q.take_next(), None, "one at a time");
        q.done(a.id + 100);
        assert_eq!(q.take_next(), None, "unknown id does not free the queue");
        q.done(a.id);
        let b = q.take_next().unwrap();
        assert_eq!(b.keys, keys(&["BackSpace"]));
        q.done(b.id);
        assert!(q.idle());
    }

    #[test]
    fn cancel_clears_and_discards_the_late_final() {
        let mut q = Queue::new();
        q.on_final("a", &[], false);
        q.on_final("b", &[], false);
        let a = q.take_next().unwrap();
        // Abbruch während einer Aufnahme: laufender Auftrag abbrechen, wartender weg, spätes final verwerfen
        assert_eq!(q.cancel(true), Cancelled { running: Some(a.id), dropped: 1 });
        assert!(!q.on_final("spät", &[], false), "late final after cancel is discarded");
        // danach wieder normal
        assert!(q.on_final("neu", &[], false));
        q.done(a.id);
        assert_eq!(q.take_next().unwrap().text, "neu");
    }

    #[test]
    fn cancel_when_idle_does_not_swallow_the_next_final() {
        let mut q = Queue::new();
        assert_eq!(q.cancel(false), Cancelled { running: None, dropped: 0 });
        assert!(q.on_final("echt", &[], false));
        // Abbruch während der Aufnahme, aber das final kam nie: die nächste Aufnahme verliert ihr final nicht
        let mut q = Queue::new();
        q.cancel(true);
        q.recording_started();
        assert!(q.on_final("nächste", &[], false));
        // Reihenfolge Abbruch → final cancelled → neue Aufnahme → final: das echte bleibt
        let mut q = Queue::new();
        q.cancel(true);
        assert!(!q.on_final("", &[], true));
        assert!(q.on_final("bleibt", &[], false));
    }

    #[test]
    fn job_description_has_no_text() {
        let mut q = Queue::new();
        q.on_final("Geheimer Satz", &keys(&["BackSpace", "x"]), false);
        let d = q.take_next().unwrap().describe();
        assert!(d.contains("13 chars, 1 keys") && !d.contains("Geheim"), "{d}");
    }
}
