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

//! Profile je App: Auswahl per `app_id`, Satzzeichen entfernen, Anfangsbuchstabe, Leerzeichen am Ende, Sprache.

use super::KEEP;
use crate::config::{Config, ProfileCfg};

/// Profil für diese Sitzung; `None`, wenn das Feature aus ist oder kein Profil passt (auch kein `default`).
pub fn active<'a>(cfg: &'a Config, app: Option<&str>) -> Option<&'a ProfileCfg> {
    if cfg.features.profiles {
        cfg.profile_for(app)
    } else {
        None
    }
}

/// Sprache für diese Sitzung, wenn das Profil eine eigene festlegt.
pub fn language(cfg: &Config, app: Option<&str>) -> Option<String> {
    active(cfg, app).map(|p| p.language.trim()).filter(|l| !l.is_empty()).map(str::to_string)
}

/// Leerzeichen nach dem Diktat: das Profil hat Vorrang vor `typing.trailing_space`.
pub fn trailing_space(cfg: &Config, profile: Option<&ProfileCfg>) -> bool {
    profile.map(|p| p.trailing_space).unwrap_or(cfg.typing.trailing_space)
}

/// Satzzeichen, die `punctuation = "strip"` entfernt.
fn is_strippable(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '…' | '"' | '\'' | '„' | '“' | '”' | '‚' | '‘' | '’' | '«' | '»' | '‹' | '›' | '¿' | '¡' | '–' | '—')
}

/// Entfernt Satzzeichen außer innerhalb von Wörtern („v1.2“, „geht's“, „E-Mail“ bleiben) und außer solchen aus
/// Sprachbefehlen (mit [`KEEP`] markiert). Leerraum wird dabei nicht zusammengezogen, nur Leerzeichen, die durch
/// ein entferntes alleinstehendes Zeichen (Gedankenstrich) doppelt würden.
pub fn strip_punctuation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == KEEP {
            out.push(c);
            continue;
        }
        if is_strippable(c) {
            let marked = i > 0 && chars[i - 1] == KEEP;
            let inner = i > 0 && chars[i - 1].is_alphanumeric() && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            if !(marked || inner) {
                continue;
            }
        }
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches(' ').to_string()
}

/// Anfangsbuchstabe: groß bei `on`, sonst klein – aber nur bei einem normal geschriebenen Wort („Hallo“ → „hallo“,
/// „PostgreSQL“ und „USB“ bleiben).
pub fn set_first_case(text: &str, on: bool) -> String {
    let Some(start) = text.find(|c: char| c.is_alphabetic()) else {
        return text.to_string();
    };
    let word_end = text[start..].find(|c: char| !c.is_alphanumeric()).map(|i| start + i).unwrap_or(text.len());
    let word = &text[start..word_end];
    let mut cs = word.chars();
    let first = cs.next().expect("Wort nicht leer");
    let rest: String = cs.collect();
    let new_first: String = if on {
        first.to_uppercase().collect()
    } else if rest.chars().any(char::is_uppercase) {
        return text.to_string();
    } else {
        first.to_lowercase().collect()
    };
    format!("{}{}{}{}", &text[..start], new_first, rest, &text[word_end..])
}

/// Wendet Satzzeichen- und Anfangsbuchstaben-Regeln des Profils an.
pub fn apply(text: &str, p: &ProfileCfg) -> String {
    let t = if p.strips_punctuation() { strip_punctuation(text) } else { text.to_string() };
    set_first_case(&t, p.capitalize_first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> ProfileCfg {
        ProfileCfg {
            name: "terminal".into(),
            match_class: vec!["Alacritty".into(), "kitty".into()],
            punctuation: "strip".into(),
            capitalize_first: false,
            trailing_space: false,
            ..ProfileCfg::default()
        }
    }

    #[test]
    fn terminal_profile_strips_and_lowercases() {
        let p = terminal();
        assert_eq!(apply("Git commit, minus m „Fix“.", &p), "git commit minus m Fix");
        assert_eq!(apply("Update auf v1.2 und geht's? E-Mail!", &p), "update auf v1.2 und geht's E-Mail");
        assert_eq!(apply("Ja – nein.", &p), "ja nein");
        assert_eq!(apply("PostgreSQL starten.", &p), "PostgreSQL starten");
        assert_eq!(apply("...", &p), "");
        // Satzzeichen aus Sprachbefehlen bleiben
        assert_eq!(apply(&format!("Pfad{KEEP}: /tmp."), &p).replace(KEEP, ""), "pfad: /tmp");
    }

    #[test]
    fn capitalize_first_and_keep() {
        let p = ProfileCfg { capitalize_first: true, ..ProfileCfg::default() };
        assert_eq!(apply("„hallo welt.", &p), "„Hallo welt.");
        assert_eq!(apply("äpfel", &p), "Äpfel");
        assert_eq!(apply("", &p), "");
        assert_eq!(set_first_case("\n\nhallo", true), "\n\nHallo");
    }

    #[test]
    fn selection_respects_feature_and_default() {
        let mut c = Config::default();
        let mut d = ProfileCfg { name: "default".into(), language: "en".into(), trailing_space: false, ..ProfileCfg::default() };
        c.profiles = vec![terminal(), d.clone()];
        assert_eq!(active(&c, Some("ALACRITTY")).unwrap().name, "terminal");
        assert_eq!(active(&c, Some("firefox")).unwrap().name, "default");
        assert_eq!(active(&c, None).unwrap().name, "default");
        assert_eq!(language(&c, Some("firefox")).as_deref(), Some("en"));
        assert_eq!(language(&c, Some("kitty")), None);
        assert!(!trailing_space(&c, active(&c, Some("kitty"))));
        c.features.profiles = false;
        assert!(active(&c, Some("kitty")).is_none());
        assert!(trailing_space(&c, None));
        d.name = "other".into();
        c.features.profiles = true;
        c.profiles = vec![d];
        assert!(active(&c, Some("firefox")).is_none());
    }
}
