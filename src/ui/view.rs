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

//! Modell des Overlays ohne GTK: Pango-Markup der Mitschrift, Kürzen auf drei Zeilen, Sichtbarkeit; die Pixelwelle
//! samt Maßen liegt in [`super::wave`] (ohne `protocol`, damit das Einstellungsfenster sie für die Vorschau mitnutzt)
//! und wird hier weitergereicht. Übertragen aus dem früheren Awesome-Modul (`init.lua`: `text_markup`, `fit_markup`,
//! `render`); Maße wie dort,
//! Farben aus der Noctalia-Palette ([`super::palette`]; das Pushing-Pixels-Design entfällt, Entscheidung Daniel
//! 20.11.2026). Gezeichnet wird in `src/bin/dictate-ui/overlay.rs`.

use super::palette::Palette;
use crate::protocol::State;

pub use super::wave::{level_from_db, size, wave_size, Tone, Wave, WaveMode, DASH_ALPHA, OFF_ALPHA};

/// Pango-Markup maskieren
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Mitschrift als Markup ab Byte `from` des Gesamttexts (`stable` + Leerzeichen + `tail`); vorn abgeschnitten mit „…“,
/// der Tail in `tail_color` (`on_surface_variant`)
pub fn text_markup(stable: &str, tail: &str, from: usize, tail_color: &str) -> String {
    let sep = if !stable.is_empty() && !tail.is_empty() { " " } else { "" };
    let pre = if from > 0 { "\u{2026}" } else { "" };
    let (st, tl): (&str, String) = if from < stable.len() {
        (&stable[from..], tail.to_string())
    } else {
        let full = format!("{stable}{sep}{tail}");
        ("", full[from.min(full.len())..].to_string())
    };
    let mut m = format!("{pre}{}", esc(st));
    if !tl.is_empty() {
        if !st.is_empty() {
            m.push_str(sep);
        }
        m.push_str(&format!("<span foreground=\"{tail_color}\">{}</span>", esc(&tl)));
    }
    m
}

/// Byte-Positionen der Wortanfänge im Gesamttext (erste ist 0)
fn word_starts(full: &str) -> Vec<usize> {
    let mut out = vec![0];
    let mut prev_space = false;
    for (i, c) in full.char_indices() {
        if prev_space && !c.is_whitespace() {
            out.push(i);
        }
        prev_space = c.is_whitespace();
    }
    out
}

/// Markup, das in höchstens [`size::MAX_LINES`] Zeilen passt: der Anfang fällt wortweise weg (binäre Suche wie
/// `fit_markup`). `fits` misst ein Markup bei der aktuellen Breite
pub fn fit_markup(stable: &str, tail: &str, tail_color: &str, fits: impl Fn(&str) -> bool) -> String {
    let sep = if !stable.is_empty() && !tail.is_empty() { " " } else { "" };
    let full = format!("{stable}{sep}{tail}");
    let starts = word_starts(&full);
    let m0 = text_markup(stable, tail, 0, tail_color);
    if fits(&m0) {
        return m0;
    }
    let (mut lo, mut hi) = (0, starts.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if fits(&text_markup(stable, tail, starts[mid], tail_color)) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    text_markup(stable, tail, starts[lo], tail_color)
}

/// Art eines Hinweises im Overlay
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Error,
    Info,
}

/// Was das Overlay zeigt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Hidden,
    /// Nur die Welle (Indikator ohne Text)
    Indicator(WaveMode),
    /// Welle und Mitschrift
    Transcript {
        stable: String,
        tail: String,
    },
    Notice {
        text: String,
        kind: NoticeKind,
    },
}

/// Inhalt aus Zustand, Mitschrift und Hinweis (wie `render` in `init.lua`): sichtbar während einer Aufnahme oder mit
/// Hinweis; Text nur bei Mitschrift oder Hinweis
pub fn content(state: State, stable: &str, tail: &str, notice: Option<(&str, NoticeKind)>) -> Content {
    if let Some((text, kind)) = notice {
        return Content::Notice { text: text.to_string(), kind };
    }
    match state {
        State::Loading => Content::Indicator(WaveMode::Wait),
        State::Listening | State::Finalizing if !stable.is_empty() || !tail.is_empty() => {
            Content::Transcript { stable: stable.to_string(), tail: tail.to_string() }
        }
        State::Listening => Content::Indicator(WaveMode::Listen),
        State::Finalizing => Content::Indicator(WaveMode::Wait),
        State::Idle | State::Error => Content::Hidden,
    }
}

impl Content {
    pub fn wave_mode(&self, listening: bool) -> Option<WaveMode> {
        match self {
            Content::Hidden => None,
            Content::Indicator(m) => Some(*m),
            Content::Transcript { .. } => Some(if listening { WaveMode::Listen } else { WaveMode::Wait }),
            Content::Notice { kind: NoticeKind::Error, .. } => Some(WaveMode::Error),
            Content::Notice { kind: NoticeKind::Info, .. } => Some(WaveMode::Wait),
        }
    }

    /// Markup des Textes (Hinweise ungekürzt), `None` = nur die Welle
    pub fn markup(&self, pal: &Palette, fits: impl Fn(&str) -> bool) -> Option<String> {
        match self {
            Content::Hidden | Content::Indicator(_) => None,
            Content::Transcript { stable, tail } => Some(fit_markup(stable, tail, &pal.on_surface_variant.hex(), fits)),
            Content::Notice { text, kind } => {
                let c = if *kind == NoticeKind::Info { pal.on_surface.hex() } else { pal.error.hex() };
                Some(format!("<span foreground=\"{c}\">{}</span>", esc(text)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::palette::FALLBACK;
    const T: &str = "#9aa5ce";

    #[test]
    fn markup_escapes_and_greys_the_tail() {
        assert_eq!(text_markup("a <b>", "c & d", 0, T), "a &lt;b&gt; <span foreground=\"#9aa5ce\">c &amp; d</span>");
        assert_eq!(text_markup("", "tail", 0, T), "<span foreground=\"#9aa5ce\">tail</span>");
        assert_eq!(text_markup("stable", "", 0, T), "stable");
        // vorn gekürzt: im stabilen Teil bzw. im Tail
        assert_eq!(text_markup("one two", "three", 4, T), "\u{2026}two <span foreground=\"#9aa5ce\">three</span>");
        assert_eq!(text_markup("one two", "three four", 14, T), "\u{2026}<span foreground=\"#9aa5ce\">four</span>");
    }

    #[test]
    fn fit_drops_whole_words_from_the_front() {
        // „passt“, wenn höchstens 12 sichtbare Zeichen übrig sind (Markup ohne Tags gezählt)
        let visible = |m: &str| {
            let mut n = 0;
            let mut tag = false;
            for c in m.chars() {
                match c {
                    '<' => tag = true,
                    '>' => tag = false,
                    _ if !tag => n += 1,
                    _ => {}
                }
            }
            n
        };
        let fits = |m: &str| visible(m) <= 12;
        assert_eq!(fit_markup("hi", "you", T, fits), "hi <span foreground=\"#9aa5ce\">you</span>");
        let m = fit_markup("alpha beta gamma", "delta", T, fits);
        assert_eq!(m, "\u{2026}gamma <span foreground=\"#9aa5ce\">delta</span>");
        // Umlaute: Byte-Positionen an Zeichengrenzen
        let m = fit_markup("äöü ßß", "€€€ „x“ yy", T, |m| visible(m) <= 7);
        assert_eq!(m, "\u{2026}<span foreground=\"#9aa5ce\">„x“ yy</span>");
    }

    #[test]
    fn content_follows_state_and_notice() {
        assert_eq!(content(State::Idle, "", "", None), Content::Hidden);
        assert_eq!(content(State::Error, "x", "", None), Content::Hidden);
        assert_eq!(content(State::Loading, "", "", None), Content::Indicator(WaveMode::Wait));
        assert_eq!(content(State::Listening, "", "", None), Content::Indicator(WaveMode::Listen));
        assert_eq!(content(State::Finalizing, "", "", None), Content::Indicator(WaveMode::Wait));
        assert_eq!(content(State::Listening, "a", "b", None), Content::Transcript { stable: "a".into(), tail: "b".into() });
        let n = content(State::Idle, "", "", Some(("Dictation error: x", NoticeKind::Error)));
        assert_eq!(n.wave_mode(false), Some(WaveMode::Error));
        assert_eq!(n.markup(&FALLBACK, |_| true).unwrap(), "<span foreground=\"#f7768e\">Dictation error: x</span>");
    }
}
