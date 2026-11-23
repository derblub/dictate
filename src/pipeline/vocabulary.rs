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

//! Vokabular: Ersetzungen nach der Erkennung und Begriffe als Hinweis im Whisper-Prompt.
//!
//! Ersetzungen gelten nur für ganze Wörter (Unicode: „Öl“ trifft nicht „Öltank“), Groß/klein egal; ein
//! Leerzeichen in der Quelle passt auf beliebigen Leerraum im Text. Längere Quellen gewinnen vor kürzeren.

use crate::config::Config;

/// Höchstlänge des festen Prompt-Teils (`initial_prompt` + Begriffe) in Zeichen.
pub const PROMPT_MAX_CHARS: usize = 200;

fn eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Wortzeichen für die Grenzprüfung (Buchstaben und Ziffern aller Schriften, auch Umlaute).
fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// Passt `pat` (Zeichen) bei `text[i..]`? Liefert die Länge des Treffers in Zeichen.
fn match_len(text: &[char], i: usize, pat: &[char]) -> Option<usize> {
    let mut j = i;
    let mut k = 0;
    while k < pat.len() {
        if pat[k].is_whitespace() {
            // Leerraum in der Quelle: mindestens ein Leerraumzeichen im Text
            if j >= text.len() || !text[j].is_whitespace() {
                return None;
            }
            while j < text.len() && text[j].is_whitespace() {
                j += 1;
            }
            while k < pat.len() && pat[k].is_whitespace() {
                k += 1;
            }
            continue;
        }
        if j >= text.len() || !eq_ci(text[j], pat[k]) {
            return None;
        }
        j += 1;
        k += 1;
    }
    Some(j - i)
}

/// Wendet die Ersetzungen an.
pub fn replace<'a>(text: &str, replacements: impl IntoIterator<Item = (&'a String, &'a String)>) -> String {
    let mut rules: Vec<(Vec<char>, &str)> =
        replacements.into_iter().map(|(k, v)| (k.trim().chars().collect::<Vec<_>>(), v.as_str())).filter(|(k, _)| !k.is_empty()).collect();
    if rules.is_empty() {
        return text.to_string();
    }
    rules.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let at_start = i == 0 || !is_word(chars[i - 1]);
        let hit = at_start
            .then(|| {
                rules.iter().find_map(|(pat, rep)| {
                    let n = match_len(&chars, i, pat)?;
                    let at_end = i + n == chars.len() || !is_word(chars[i + n]);
                    // Quelle, die selbst mit einem Nicht-Wortzeichen endet („C++“), braucht keine Grenze danach
                    (at_end || !is_word(pat[pat.len() - 1])).then_some((n, *rep))
                })
            })
            .flatten();
        match hit {
            Some((n, rep)) => {
                out.push_str(rep);
                i += n;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// Fester Prompt-Teil für Whisper: `initial_prompt` und (bei eingeschaltetem Vokabular) die Begriffe, durch Kommas
/// getrennt, gekürzt auf [`PROMPT_MAX_CHARS`] an einer Begriffsgrenze.
pub fn prompt_base(cfg: &Config) -> String {
    let mut base = tail_chars(cfg.initial_prompt.trim(), PROMPT_MAX_CHARS);
    if !cfg.features.vocabulary {
        return base;
    }
    let mut first = true;
    for t in cfg.vocabulary.terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let sep = if first {
            if base.is_empty() {
                ""
            } else {
                " "
            }
        } else {
            ", "
        };
        if base.chars().count() + sep.chars().count() + t.chars().count() > PROMPT_MAX_CHARS {
            break;
        }
        base.push_str(sep);
        base.push_str(t);
        first = false;
    }
    base
}

/// Höchstens `max` Zeichen vom Ende, beginnend an einer Wortgrenze.
pub fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - max).collect();
    match tail.find(' ') {
        Some(i) => tail[i + 1..].to_string(),
        None => tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rep(text: &str, pairs: &[(&str, &str)]) -> String {
        let m: BTreeMap<String, String> = pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        replace(text, &m)
    }

    #[test]
    fn whole_words_case_insensitive() {
        let r = [("post gress", "PostgreSQL"), ("öl", "Oel")];
        assert_eq!(rep("Ich nutze Post Gress und post  gress.", &r), "Ich nutze PostgreSQL und PostgreSQL.");
        assert_eq!(rep("Öl, Öltank, Heizöl, ÖL", &r), "Oel, Öltank, Heizöl, Oel");
        assert_eq!(rep("postgressql bleibt", &r), "postgressql bleibt");
    }

    #[test]
    fn longer_source_wins() {
        let r = [("awesome", "Awesome"), ("awesome wm", "AwesomeWM")];
        assert_eq!(rep("ich mag awesome wm und awesome", &r), "ich mag AwesomeWM und Awesome");
    }

    #[test]
    fn umlaut_boundaries_and_symbols() {
        let r = [("müller", "Mueller"), ("c++", "C++")];
        assert_eq!(rep("Herr Müller, Herr Müllers, über müller", &r), "Herr Mueller, Herr Müllers, über Mueller");
        assert_eq!(rep("ich schreibe c++ code", &r), "ich schreibe C++ code");
        assert_eq!(rep("", &r), "");
    }

    #[test]
    fn prompt_base_appends_and_truncates_terms() {
        let mut c = Config { initial_prompt: "Diktat.".into(), ..Config::default() };
        c.vocabulary.terms = vec!["PostgreSQL".into(), "AwesomeWM".into(), "chezmoi".into()];
        assert_eq!(prompt_base(&c), "Diktat. PostgreSQL, AwesomeWM, chezmoi");
        c.initial_prompt.clear();
        assert_eq!(prompt_base(&c), "PostgreSQL, AwesomeWM, chezmoi");
        c.vocabulary.terms = (0..100).map(|i| format!("Begriff{i}")).collect();
        let p = prompt_base(&c);
        assert!(p.chars().count() <= PROMPT_MAX_CHARS, "{}", p.len());
        assert!(p.ends_with(|c: char| c.is_ascii_digit()), "{p}");
        c.vocabulary.terms.clear();
        c.initial_prompt = "wort ".repeat(100);
        assert!(prompt_base(&c).chars().count() <= PROMPT_MAX_CHARS);
        assert!(prompt_base(&c).starts_with("wort"));
        c.features.vocabulary = false;
        c.initial_prompt = "Nur das.".into();
        assert_eq!(prompt_base(&c), "Nur das.");
    }
}
