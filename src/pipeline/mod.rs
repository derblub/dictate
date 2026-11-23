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

//! Nachbearbeitung zwischen `Streamer::finish` und dem `final`-Event.
//!
//! Reihenfolge (jeder Schritt nur bei eingeschaltetem Feature bzw. wenn das Profil ihn verlangt):
//! 1. Sprachbefehle ([`commands`])
//! 2. Vokabular-Ersetzungen ([`vocabulary`]; die Begriffe gehen schon vorher in den Whisper-Prompt)
//! 3. Profil ([`profile`]): Satzzeichen, Anfangsbuchstabe
//! 4. KI (Aufgabe E, `ai.rs`): kommt hier als letzter Textschritt dazu, siehe [`process`]
//! 5. Leerzeichen am Ende (Profil vor `typing.trailing_space`)
//!
//! Der Verlauf ([`history`]) wird erst im Kern geschrieben, wenn das `final`-Event wirklich mit Text rausgeht
//! (ein `cancel` während `finalizing` verwirft das Ergebnis noch).

pub mod ai;
pub mod commands;
pub mod history;
pub mod profile;
pub mod vocabulary;

use crate::config::Config;
use std::time::{Duration, Instant};

/// „rückgängig“ gilt nur so lange nach dem letzten Diktat
pub const UNDO_WINDOW: Duration = Duration::from_secs(30);
/// Mehr BackSpace-Tasten schickt ein „rückgängig“ nie
pub const UNDO_MAX_KEYS: usize = 2000;

/// Das zuletzt getippte Diktat, soweit seitdem nichts anderes getippt wurde (für „rückgängig“).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    /// Länge in Graphem-Näherung ([`grapheme_count`])
    pub count: usize,
    /// `app_id` der Sitzung
    pub app: Option<String>,
    pub at: Instant,
}

impl Typed {
    pub fn new(text: &str, app: Option<&str>) -> Typed {
        Typed { count: grapheme_count(text), app: app.map(str::to_string), at: Instant::now() }
    }

    /// Darf ein „rückgängig“ in `app` jetzt darauf wirken? Gleiche App und höchstens [`UNDO_WINDOW`] her.
    fn undoable(&self, app: Option<&str>) -> bool {
        self.app.as_deref() == app && self.at.elapsed() <= UNDO_WINDOW
    }
}

/// Anzahl der BackSpace-Tasten, die ein getippter Text braucht: Zeichen, aber ohne Kombinierzeichen,
/// Variationsselektoren, Hautton-Modifikatoren und Zeichen nach einem Zero-Width-Joiner (Näherung für Grapheme).
pub fn grapheme_count(s: &str) -> usize {
    let mut n = 0;
    let mut joined = false;
    for c in s.chars() {
        let cp = c as u32;
        let extends = matches!(cp, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F | 0x200C);
        if c == '\u{200D}' {
            joined = true;
            continue;
        }
        if extends || joined {
            joined = false;
            continue;
        }
        n += 1;
    }
    n
}

/// Markiert das folgende Zeichen als Ergebnis eines Sprachbefehls (Unicode Private Use Area). Lebt nur innerhalb
/// der Pipeline; Rohtext wird vorher davon bereinigt, das Ergebnis danach.
pub const KEEP: char = '\u{E000}';

/// Ergebnis der Pipeline für ein Diktat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    /// Zu tippender Text, inklusive Leerzeichen am Ende
    pub text: String,
    /// Tastennamen (X-Keysym-Namen), die danach gedrückt werden
    pub keys: Vec<String>,
    /// Name des gewählten Profils
    pub profile: Option<String>,
    /// KI-Modus für diese Sitzung (Profil, sonst `ai.default_mode`; leer = keiner), für Aufgabe E
    pub ai_mode: Option<String>,
    /// Fehlertext für das `error`-Event (KI-Fehler oder -Timeout); der Rohtext wurde dann getippt
    pub error: Option<String>,
}

/// Verarbeitet den Rohtext. `app` ist die `app_id` aus `start`, `last_typed` das zuletzt getippte Diktat (für
/// „rückgängig“), `cancel` bricht einen laufenden KI-Aufruf ab, `log_text` erlaubt Texte im Debug-Log der KI (nur
/// `DICTATE_DEBUG` und ein Diktat ins Fenster, nie die Eingabe der Befehlspalette).
pub fn process(cfg: &Config, raw: &str, app: Option<&str>, last_typed: Option<&Typed>, cancel: Option<&ai::CancelFlag>, log_text: bool) -> Output {
    let prof = profile::active(cfg, app);
    let mut out = Output {
        profile: prof.map(|p| p.name.clone()),
        ai_mode: prof
            .map(|p| p.ai_mode.as_str())
            .filter(|m| !m.is_empty())
            .or_else(|| Some(cfg.ai.default_mode.as_str()).filter(|m| !m.is_empty()))
            .map(str::to_string),
        ..Output::default()
    };
    let raw: String = raw.chars().filter(|&c| c != KEEP).collect();

    // 1. Sprachbefehle
    let mut text = if cfg.features.voice_commands {
        match commands::apply(&raw) {
            commands::Outcome::Undo => match last_typed {
                Some(t) if t.undoable(app) => {
                    out.keys = vec!["BackSpace".to_string(); t.count.min(UNDO_MAX_KEYS)];
                    return out;
                }
                // Anderes Fenster oder zu lange her: ein gewöhnliches Wort, nichts wird gelöscht
                Some(_) => raw.trim().to_string(),
                None => {
                    out.error = Some("Nothing to undo".to_string());
                    return out;
                }
            },
            commands::Outcome::Text(t) => t,
        }
    } else {
        raw.trim().to_string()
    };
    // 2. Vokabular
    if cfg.features.vocabulary && !cfg.vocabulary.replacements.is_empty() {
        text = vocabulary::replace(&text, &cfg.vocabulary.replacements);
    }
    // 3. Profil
    if let Some(p) = prof {
        text = profile::apply(&text, p);
    }
    text.retain(|c| c != KEEP);
    // 4. KI: bei Fehler oder Timeout bleibt der Text wie er ist und `error` meldet es
    if let Some(mode) = out.ai_mode.as_deref().filter(|_| cfg.features.ai) {
        if !text.trim().is_empty() {
            match ai::rewrite_logged(&cfg.ai, mode, &text, cancel, log_text) {
                Ok(t) => text = t,
                Err(ai::AiError::Cancelled) => {}
                Err(e) => out.error = Some(e.event_message()),
            }
        }
    }

    // 5. Leerzeichen am Ende; nach einem Umbruch keins
    if profile::trailing_space(cfg, prof) && !text.is_empty() && !text.ends_with('\n') {
        text.push(' ');
    }
    out.text = text;
    out
}

/// Befehlstabelle für `{"cmd":"commands"}`.
pub fn commands_table() -> serde_json::Value {
    commands::table_json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProfileCfg;

    fn p(c: &Config, raw: &str, app: Option<&str>, last: Option<&Typed>) -> Output {
        process(c, raw, app, last, None, false)
    }

    fn typed(n: usize, app: Option<&str>) -> Typed {
        Typed { count: n, app: app.map(str::to_string), at: Instant::now() }
    }

    fn terminal() -> ProfileCfg {
        ProfileCfg {
            name: "terminal".into(),
            match_class: vec!["Alacritty".into(), "kitty".into(), "XTerm".into()],
            punctuation: "strip".into(),
            capitalize_first: false,
            trailing_space: false,
            ..ProfileCfg::default()
        }
    }

    #[test]
    fn acceptance_sentence_with_defaults() {
        let c = Config::default();
        let o = p(&c, "Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß", None, None);
        assert_eq!(o.text, "Hallo, wie geht's?\n\nGruß ");
        assert!(o.keys.is_empty());
        let c = Config { typing: crate::config::TypingCfg { trailing_space: false, ..Default::default() }, ..Config::default() };
        assert_eq!(p(&c, "Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß", None, None).text, "Hallo, wie geht's?\n\nGruß");
    }

    #[test]
    fn terminal_profile() {
        let mut c = Config { profiles: vec![terminal()], ..Config::default() };
        let o = p(&c, "Git status, dann Version v1.2 prüfen.", Some("Alacritty"), None);
        assert_eq!(o.text, "git status dann Version v1.2 prüfen");
        assert_eq!(o.profile.as_deref(), Some("terminal"));
        // gesprochene Satzzeichen bleiben auch im Terminal
        assert_eq!(p(&c, "Echo Hallo Doppelpunkt Welt.", Some("kitty"), None).text, "echo Hallo: Welt");
        // anderes Fenster: kein Profil, Text bleibt
        assert_eq!(p(&c, "Git status.", Some("firefox"), None).text, "Git status. ");
        c.features.profiles = false;
        assert_eq!(p(&c, "Git status.", Some("kitty"), None).text, "Git status. ");
    }

    #[test]
    fn undo_uses_last_length() {
        let c = Config::default();
        let o = p(&c, "Rückgängig.", None, Some(&typed(3, None)));
        assert_eq!((o.text.as_str(), o.keys.len()), ("", 3));
        assert!(o.keys.iter().all(|k| k == "BackSpace"));
        // ohne vorheriges Diktat: keine Tasten, kurzer Fehler
        let o = p(&c, "undo that", None, None);
        assert!(o.keys.is_empty() && o.text.is_empty());
        assert_eq!(o.error.as_deref(), Some("Nothing to undo"));
        // ohne Sprachbefehle ist es ein Wort
        let c = Config { features: crate::config::FeaturesCfg { voice_commands: false, ..Default::default() }, ..Config::default() };
        assert_eq!(p(&c, "Rückgängig.", None, Some(&typed(3, None))).text, "Rückgängig. ");
    }

    #[test]
    fn undo_only_in_same_app_and_within_30_s() {
        let c = Config::default();
        // andere App: kein Löschen, das Wort wird getippt
        let o = p(&c, "Rückgängig.", Some("firefox"), Some(&typed(5, Some("kitty"))));
        assert!(o.keys.is_empty(), "{o:?}");
        assert_eq!(o.text, "Rückgängig. ");
        let o = p(&c, "Rückgängig.", None, Some(&typed(5, Some("kitty"))));
        assert!(o.keys.is_empty());
        // gleiche App, frisch
        assert_eq!(p(&c, "Rückgängig.", Some("kitty"), Some(&typed(5, Some("kitty")))).keys.len(), 5);
        // zu alt
        let mut old = typed(5, Some("kitty"));
        old.at = Instant::now() - Duration::from_secs(31);
        let o = p(&c, "Rückgängig.", Some("kitty"), Some(&old));
        assert!(o.keys.is_empty());
        assert_eq!(o.text, "Rückgängig. ");
        let mut edge = typed(5, Some("kitty"));
        edge.at = Instant::now() - Duration::from_secs(29);
        assert_eq!(p(&c, "Rückgängig.", Some("kitty"), Some(&edge)).keys.len(), 5);
    }

    #[test]
    fn undo_is_capped_at_2000_keys() {
        let c = Config::default();
        assert_eq!(p(&c, "Rückgängig.", None, Some(&typed(50_000, None))).keys.len(), UNDO_MAX_KEYS);
        assert_eq!(p(&c, "Rückgängig.", None, Some(&typed(2000, None))).keys.len(), 2000);
    }

    #[test]
    fn grapheme_approximation() {
        assert_eq!(grapheme_count("Hallo "), 6);
        // e + Kombinier-Akut zählt einmal, vorkomponiert ebenso
        assert_eq!(grapheme_count("e\u{301}"), 1);
        assert_eq!(grapheme_count("é"), 1);
        // Familien-Emoji mit ZWJ, Hautton, Flagge mit Variationsselektor
        assert_eq!(grapheme_count("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"), 1);
        assert_eq!(grapheme_count("\u{1F44D}\u{1F3FD}"), 1);
        assert_eq!(grapheme_count("\u{2764}\u{FE0F}"), 1);
        assert_eq!(grapheme_count(""), 0);
        assert_eq!(Typed::new("ab\n", None).count, 3);
    }

    #[test]
    fn vocabulary_after_commands() {
        let mut c = Config::default();
        c.vocabulary.replacements.insert("post gress".into(), "PostgreSQL".into());
        assert_eq!(p(&c, "Ich nutze Post Gress Punkt", None, None).text, "Ich nutze PostgreSQL. ");
        c.features.vocabulary = false;
        assert_eq!(p(&c, "Ich nutze Post Gress Punkt", None, None).text, "Ich nutze Post Gress. ");
    }

    #[test]
    fn no_space_after_line_break_and_ai_mode() {
        let mut c = Config::default();
        assert_eq!(p(&c, "Hallo neue Zeile", None, None).text, "Hallo\n");
        assert_eq!(p(&c, "", None, None).text, "");
        assert_eq!(p(&c, &format!("a{KEEP}b"), None, None).text, "ab ");
        c.ai.default_mode = "clean".into();
        assert_eq!(p(&c, "x", None, None).ai_mode.as_deref(), Some("clean"));
        c.profiles = vec![ProfileCfg { ai_mode: "email".into(), ..terminal() }];
        assert_eq!(p(&c, "x", Some("kitty"), None).ai_mode.as_deref(), Some("email"));
    }
}
