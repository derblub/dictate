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

//! Notizen und Ursachen der Diagnose-Antwort als Pango-Markup (Port von `notes_items`, `cause_markup` und `mark_ph` aus
//! dem früheren `diagnose.lua`, Zeilen 982–1040; PLAN-wayland.md 6e „Rest Block B“ Punkt 1). Rein, ohne GTK.
//!
//! Die Spannen der Rückübersetzung ([`PhSpan`], vom Daemon, geprüft mit `valid_spans`) nennen, wo im Text des Modells ein
//! Platzhalter steht und welchen Klartext er vertritt. Je nach Ansicht zeigt das Panel den Platzhalter (wie gesendet) oder
//! den Wert (hinterlegt). **Eingesetzt wird nur in Fließtext:** in Inline-Code, Codeblöcken und Linkzielen bleibt immer
//! der Platzhalter, auch wenn eine Spanne dorthin zeigt (ein Befehl zum Abschreiben soll nie Werte tragen, die das Modell
//! nie gesehen hat). Codeblöcke bekommen hier keinen Copy-Knopf (Antwort 6 der Durchsicht unter Awesome).
//!
//! Ablauf wie in Lua: jede Spanne wird im Text durch eine Marke `U+E000 <i> U+E001` ersetzt, dann zerlegt das Markdown
//! ([`markdown::parse`]); erst beim Markup wird je Stück entschieden. Der Daemon entfernt U+E000–U+F8FF aus Texten des
//! Modells; enthalten die Notizen trotzdem eine Marke, entfallen alle Spannen (nichts wird vorgetäuscht). Anders als in
//! Lua wird erst an den Marken zerlegt und dann jedes Stück einzeln maskiert ([`markdown::esc`]). Eine Marke ohne gültige
//! Spanne erscheint nie in der Ausgabe.
//!
//! Klartext lebt nur im Speicher der Sitzung; Zwischentexte mit Marken werden überschrieben ([`Notes`] beim Freigeben).

use crate::actions::store::wipe;
use crate::ui::diag_panel::PhSpan;
use crate::ui::markdown::{self, Block, Span};

/// Marken im privaten Bereich (Unicode), die eine Spanne im Text vertreten
pub const PH_OPEN: char = '\u{E000}';
pub const PH_CLOSE: char = '\u{E001}';

/// Fester Satz über den Notizen (Durchsicht unter Awesome, I4: eine Injektion im Log kann einen schädlichen Befehl als
/// Rat in die Notizen bringen)
pub const NOTES_WARNING: &str = "Suggestions from the AI, written from the report. Check every command before you run it; dictate runs none of them.";

/// Farben für Platzhalter und Werte (Hex `#rrggbb`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhLook {
    /// Platzhalter und Wert (Vordergrund)
    pub accent: String,
    /// Inline-Code: Schrift und Hintergrund, Linkziel: Farbe
    pub mono: String,
    pub code_bg: String,
    pub tail: String,
}

/// Zähler einer Darstellung (für Tests und das Debug-Log, keine Inhalte)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    /// Klartext eingesetzt (Fließtext, Ansicht „Real values“)
    pub values: u64,
    /// Platzhalter gezeigt (Fließtext, Ansicht „Placeholders“)
    pub placeholders: u64,
    /// Platzhalter, weil die Spanne in Code, Codeblock oder Linkziel lag (unabhängig von der Ansicht)
    pub guarded: u64,
}

/// Notizen, zerlegt in Markdown-Blöcke, deren Texte Marken tragen
pub struct Notes {
    pub blocks: Vec<Block>,
    spans: Vec<PhSpan>,
}

impl Drop for Notes {
    fn drop(&mut self) {
        for b in &mut self.blocks {
            wipe_block(b);
        }
        for sp in &mut self.spans {
            wipe(&mut sp.value);
        }
    }
}

fn wipe_spans(spans: &mut [Span]) {
    for s in spans {
        wipe(&mut s.text);
    }
}

fn wipe_block(b: &mut Block) {
    match b {
        Block::Para(s) | Block::Heading(s) | Block::Quote(s) | Block::Item { spans: s, .. } => wipe_spans(s),
        Block::Code { text, .. } => wipe(text),
        Block::Gap => {}
    }
}

/// Notizen mit Marken zerlegen (`notes_items` 1006). `spans` müssen aufsteigend und ohne Überlappung auf Zeichengrenzen
/// in `notes` liegen (`valid_spans` sorgt dafür); eine Spanne, die das nicht tut, entfällt.
pub fn notes_items(notes: &str, spans: &[PhSpan]) -> Notes {
    let forged = notes.contains(PH_OPEN) || notes.contains(PH_CLOSE);
    let mut kept: Vec<PhSpan> = vec![];
    let mut text = String::with_capacity(notes.len() + 8 * spans.len());
    let mut pos = 0;
    if !forged {
        for sp in spans {
            let ok = sp.start >= pos
                && sp.end > sp.start
                && sp.end <= notes.len()
                && notes.is_char_boundary(sp.start)
                && notes.is_char_boundary(sp.end)
                && notes[sp.start..sp.end] == sp.placeholder;
            if !ok {
                continue;
            }
            text.push_str(&notes[pos..sp.start]);
            kept.push(sp.clone());
            text.push(PH_OPEN);
            text.push_str(&kept.len().to_string());
            text.push(PH_CLOSE);
            pos = sp.end;
        }
    }
    text.push_str(&notes[pos..]);
    let blocks = markdown::parse(&text);
    wipe(&mut text);
    Notes { blocks, spans: kept }
}

/// Ein Stück Text an den Marken: (Text davor, Index der Spanne ab 1 oder `None` für eine kaputte Marke)
enum Piece<'a> {
    Text(&'a str),
    Mark(Option<usize>),
}

fn pieces(s: &str) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(i) = rest.find(PH_OPEN) {
        if i > 0 {
            out.push(Piece::Text(&rest[..i]));
        }
        let after = &rest[i + PH_OPEN.len_utf8()..];
        match after.find(PH_CLOSE) {
            Some(j) => {
                out.push(Piece::Mark(after[..j].parse().ok()));
                rest = &after[j + PH_CLOSE.len_utf8()..];
            }
            None => {
                // offene Marke: der Rest ist Text ohne die Marke
                out.push(Piece::Mark(None));
                rest = after;
            }
        }
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    // eine einzelne schließende Marke im Text fällt weg (esc lässt sie sonst stehen)
    out
}

fn esc_clean(s: &str) -> String {
    markdown::esc(&s.replace(PH_CLOSE, ""))
}

/// Ein Platzhalter bzw. Wert als Markup (`mark_ph` 982)
fn ph_markup(sp: &PhSpan, plain: bool, look: &PhLook, c: &mut Counts) -> String {
    if plain {
        c.values += 1;
        format!("<span foreground=\"{0}\" background=\"{0}\" bgalpha=\"16%\">{1}</span>", look.accent, markdown::esc(&sp.value))
    } else {
        c.placeholders += 1;
        format!("<span foreground=\"{}\">{}</span>", look.accent, markdown::esc(&sp.placeholder))
    }
}

impl Notes {
    /// Text mit Marken als einfacher Text, jede Marke durch ihren Platzhalter ersetzt (Codeblöcke, nie Klartext)
    pub fn guarded_text(&self, s: &str, c: &mut Counts) -> String {
        let mut out = String::new();
        for p in pieces(s) {
            match p {
                Piece::Text(t) => out.push_str(&t.replace(PH_CLOSE, "")),
                Piece::Mark(i) => {
                    if let Some(sp) = i.and_then(|i| self.spans.get(i.checked_sub(1)?)) {
                        c.guarded += 1;
                        out.push_str(&sp.placeholder);
                    }
                }
            }
        }
        out
    }

    /// Markup eines Stücks: in Fließtext je nach Ansicht Platzhalter oder Wert, in Code und Linkziel immer der Platzhalter
    fn span_text(&self, s: &str, prose: bool, plain: bool, look: &PhLook, c: &mut Counts) -> String {
        let mut out = String::new();
        for p in pieces(s) {
            match p {
                Piece::Text(t) => out.push_str(&esc_clean(t)),
                Piece::Mark(i) => {
                    let Some(sp) = i.and_then(|i| self.spans.get(i.checked_sub(1)?)) else { continue };
                    if prose {
                        out.push_str(&ph_markup(sp, plain, look, c));
                    } else {
                        c.guarded += 1;
                        out.push_str(&markdown::esc(&sp.placeholder));
                    }
                }
            }
        }
        out
    }

    /// Markup einer Folge von Stücken (wie [`markdown::spans_markup`], mit Platzhaltern)
    pub fn spans_markup(&self, spans: &[Span], plain: bool, look: &PhLook, c: &mut Counts) -> String {
        let mut out = String::new();
        for s in spans {
            if s.code {
                let t = self.span_text(&s.text, false, plain, look, c);
                out.push_str(&format!("<span font_family=\"{}\" background=\"{}\">\u{2009}{t}\u{2009}</span>", markdown::esc(&look.mono), look.code_bg));
            } else if s.link {
                let t = self.span_text(&s.text, false, plain, look, c);
                out.push_str(&format!("<span foreground=\"{}\">{t}</span>", look.tail));
            } else {
                let mut t = self.span_text(&s.text, true, plain, look, c);
                if s.bold {
                    t = format!("<b>{t}</b>");
                }
                if s.italic {
                    t = format!("<i>{t}</i>");
                }
                out.push_str(&t);
            }
        }
        out
    }
}

/// Ursache eines Befunds (eine Zeile, kein Markdown) mit ihren Spannen (`cause_markup` 993)
pub fn cause_markup(text: &str, spans: &[PhSpan], plain: bool, look: &PhLook, c: &mut Counts) -> String {
    let mut out = String::new();
    let mut pos = 0;
    for sp in spans {
        let ok = sp.start >= pos && sp.end > sp.start && sp.end <= text.len() && text.is_char_boundary(sp.start) && text.is_char_boundary(sp.end);
        if !ok || text[sp.start..sp.end] != sp.placeholder {
            continue;
        }
        out.push_str(&markdown::esc(&text[pos..sp.start]));
        out.push_str(&ph_markup(sp, plain, look, c));
        pos = sp.end;
    }
    out.push_str(&markdown::esc(&text[pos..]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> PhLook {
        PhLook { accent: "#00aaaa".into(), mono: "monospace".into(), code_bg: "#222222".into(), tail: "#888888".into() }
    }

    /// Spannen für jedes Vorkommen von `ph` in `text` mit dem Wert `value`
    fn spans_for(text: &str, ph: &str, value: &str) -> Vec<PhSpan> {
        text.match_indices(ph).map(|(i, _)| PhSpan { start: i, end: i + ph.len(), placeholder: ph.into(), value: value.into() }).collect()
    }

    /// Alles Markup der Notizen (Blöcke hintereinander)
    fn render(n: &Notes, plain: bool, c: &mut Counts) -> String {
        let l = look();
        let mut out = vec![];
        for b in &n.blocks {
            out.push(match b {
                Block::Para(s) | Block::Heading(s) | Block::Quote(s) | Block::Item { spans: s, .. } => n.spans_markup(s, plain, &l, c),
                Block::Code { text, .. } => n.guarded_text(text, c),
                Block::Gap => String::new(),
            });
        }
        out.join("\n")
    }

    const VALUE: &str = "secrethost.example";

    #[test]
    fn values_only_in_prose_never_in_code_blocks_or_links() {
        let notes = "Check host-1 first.\n\nRun `ping host-1` there:\n\n```\nssh host-1\n```\n\nSee [the docs](https://host-1/x) and **host-1**.";
        let spans = spans_for(notes, "host-1", VALUE);
        assert_eq!(spans.len(), 5);
        let n = notes_items(notes, &spans);
        let mut c = Counts::default();
        let m = render(&n, true, &mut c);
        // Fließtext: der erste Satz und das Fettgedruckte
        assert_eq!(m.matches(VALUE).count(), 2, "{m}");
        assert_eq!(c, Counts { values: 2, placeholders: 0, guarded: 3 });
        assert!(m.contains("ping host-1"), "inline code keeps the placeholder: {m}");
        assert!(m.contains("ssh host-1"), "code block keeps the placeholder: {m}");
        assert!(m.contains("(https://host-1/x)"), "link target keeps the placeholder: {m}");
        assert!(!m.contains(PH_OPEN) && !m.contains(PH_CLOSE));

        let mut c = Counts::default();
        let m = render(&n, false, &mut c);
        assert!(!m.contains(VALUE), "placeholders view shows no value: {m}");
        assert_eq!(c, Counts { values: 0, placeholders: 2, guarded: 3 });
    }

    #[test]
    fn forged_marks_drop_every_span() {
        for mark in [PH_OPEN, PH_CLOSE] {
            let notes = format!("host-1 is down {mark}1{PH_CLOSE}");
            let spans = spans_for(&notes, "host-1", VALUE);
            let n = notes_items(&notes, &spans);
            let mut c = Counts::default();
            let m = render(&n, true, &mut c);
            assert!(!m.contains(VALUE), "{m}");
            assert_eq!(c, Counts::default());
            assert!(m.contains("host-1 is down"), "{m}");
        }
    }

    #[test]
    fn a_mark_without_a_valid_index_never_shows() {
        let n = notes_items("plain", &[]);
        let mut c = Counts::default();
        for s in [format!("a{PH_OPEN}7{PH_CLOSE}b"), format!("a{PH_OPEN}x{PH_CLOSE}b"), format!("a{PH_OPEN}0{PH_CLOSE}b"), format!("a{PH_OPEN}b")] {
            let m = n.spans_markup(&[Span { text: s.clone(), ..Span::default() }], true, &look(), &mut c);
            assert!(!m.contains(PH_OPEN) && !m.contains(PH_CLOSE), "{m:?}");
            assert!(m.starts_with('a'), "{m:?}");
            let g = n.guarded_text(&s, &mut c);
            assert!(!g.contains(PH_OPEN) && !g.contains(PH_CLOSE), "{g:?}");
        }
        assert_eq!(c, Counts::default());
    }

    #[test]
    fn spans_that_do_not_fit_are_dropped() {
        let notes = "user-1 and host-1 on host-1";
        let mut spans = spans_for(notes, "host-1", VALUE);
        // Spanne auf falschen Text, rückwärts, außerhalb und mitten im Zeichen
        spans.insert(0, PhSpan { start: 0, end: 6, placeholder: "host-1".into(), value: VALUE.into() });
        spans.push(PhSpan { start: 2, end: 8, placeholder: "host-1".into(), value: VALUE.into() });
        spans.push(PhSpan { start: 40, end: 46, placeholder: "host-1".into(), value: VALUE.into() });
        let n = notes_items(notes, &spans);
        let mut c = Counts::default();
        let m = render(&n, true, &mut c);
        assert_eq!(m.matches(VALUE).count(), 2, "{m}");
        assert!(m.starts_with("user-1 and "), "{m}");

        let multi = "äöü host-1";
        let bad = vec![PhSpan { start: 1, end: 3, placeholder: "x".into(), value: VALUE.into() }];
        let n = notes_items(multi, &bad);
        assert!(!render(&n, true, &mut Counts::default()).contains(VALUE));
    }

    #[test]
    fn every_piece_is_escaped() {
        let notes = "<b>host-1</b> & `a<b>`";
        let spans = spans_for(notes, "host-1", "<i>v&</i>");
        let n = notes_items(notes, &spans);
        let m = render(&n, true, &mut Counts::default());
        assert!(m.contains("&lt;b&gt;"), "{m}");
        assert!(m.contains("&lt;i&gt;v&amp;&lt;/i&gt;"), "{m}");
        assert!(m.contains("&amp;"), "{m}");
        assert!(!m.contains("<b>") && !m.contains("<i>"), "{m}");
    }

    #[test]
    fn cause_marks_its_spans() {
        let cause = "Disk of host-1 is full <now>";
        let spans = spans_for(cause, "host-1", VALUE);
        let mut c = Counts::default();
        let m = cause_markup(cause, &spans, true, &look(), &mut c);
        assert!(m.contains(VALUE) && m.contains("&lt;now&gt;"), "{m}");
        assert_eq!(c.values, 1);
        let m = cause_markup(cause, &spans, false, &look(), &mut c);
        assert!(!m.contains(VALUE) && m.contains("host-1"), "{m}");
        // falsche Spanne: nichts eingesetzt
        let bad = vec![PhSpan { start: 0, end: 4, placeholder: "host-1".into(), value: VALUE.into() }];
        assert!(!cause_markup(cause, &bad, true, &look(), &mut c).contains(VALUE));
    }
}
