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

//! Halluzinationsfilter und Textglättung.
//!
//! - Blocklist: eine Phrase wird verworfen, wenn sie nach Normalisierung (Kleinschreibung, ohne Satzzeichen)
//!   nur aus Blocklist-Einträgen besteht („Vielen Dank.“ in Pausen).
//! - Segmente mit `no_speech_prob > 0.6` werden verworfen.
//! - Wiederholungsschleifen (dasselbe n-Gramm mindestens 4× direkt hintereinander) werden auf ein Vorkommen gekürzt.
//! - `join` fügt Phrasen mit sauberem Leerzeichen und Satzanschluss zusammen.

use crate::engine::Segment;

/// Segmente oberhalb dieser Wahrscheinlichkeit für „keine Sprache“ werden verworfen.
pub const NO_SPEECH_MAX: f32 = 0.6;
/// Ab so vielen direkten Wiederholungen gilt ein n-Gramm als Schleife.
pub const LOOP_MIN_REPEATS: usize = 4;
/// Längstes n-Gramm (in Wörtern), das auf Schleifen geprüft wird.
pub const LOOP_MAX_N: usize = 12;

/// Kleinschreibung, Satzzeichen raus (Bindestriche trennen, Apostrophe verschwinden), Leerzeichen
/// zusammenfassen. Buchstaben und Ziffern aller Schriften bleiben.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == '\'' || c == '’' {
            // „don't“ und „dont“ gleich behandeln
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalisierte Form eines einzelnen Worts (für den Wortvergleich in LocalAgreement).
pub fn norm_word(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn words_of(s: &str) -> Vec<String> {
    normalize(s).split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Besteht `text` nach Normalisierung nur aus Blocklist-Einträgen (auch mehrfach, z. B. „Vielen Dank. Vielen Dank.“)?
/// Leerer Text gilt nicht als blockiert.
pub fn is_blocked(text: &str, blocklist: &[String]) -> bool {
    let words = words_of(text);
    if words.is_empty() {
        return false;
    }
    let entries: Vec<Vec<String>> = blocklist.iter().map(|b| words_of(b)).filter(|e| !e.is_empty()).collect();
    // reach[i]: die ersten i Wörter lassen sich restlos in Blocklist-Einträge zerlegen
    let mut reach = vec![false; words.len() + 1];
    reach[0] = true;
    for i in 0..words.len() {
        if !reach[i] {
            continue;
        }
        for e in &entries {
            if i + e.len() <= words.len() && words[i..i + e.len()] == e[..] {
                reach[i + e.len()] = true;
            }
        }
    }
    reach[words.len()]
}

/// Kürzt Wiederholungsschleifen: ein n-Gramm (1..=LOOP_MAX_N Wörter), das mindestens `LOOP_MIN_REPEATS`-mal
/// direkt hintereinander steht, bleibt nur einmal stehen. Verglichen wird normalisiert, die erste Schreibweise bleibt.
pub fn collapse_repeats(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let norm: Vec<String> = words.iter().map(|w| norm_word(w)).collect();
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    'outer: while i < words.len() {
        // längere Muster zuerst, damit „a b a b a b a b“ als Zweiwort-Schleife erkannt wird
        for n in (1..=LOOP_MAX_N).rev() {
            if i + n * LOOP_MIN_REPEATS > words.len() {
                continue;
            }
            let pat = &norm[i..i + n];
            if pat.iter().all(|w| w.is_empty()) {
                continue;
            }
            let mut reps = 1;
            while i + (reps + 1) * n <= words.len() && norm[i + reps * n..i + (reps + 1) * n] == *pat {
                reps += 1;
            }
            if reps >= LOOP_MIN_REPEATS {
                out.extend_from_slice(&words[i..i + n]);
                i += reps * n;
                continue 'outer;
            }
        }
        out.push(words[i]);
        i += 1;
    }
    out.join(" ")
}

/// Segmente einer Phrase zu Text: Segmente mit hoher `no_speech_prob` fallen weg, Schleifen werden gekürzt,
/// eine Phrase, die nur aus Blocklist-Einträgen besteht, wird leer.
pub fn segments_to_text(segs: &[Segment], blocklist: &[String]) -> String {
    let mut text = String::new();
    for s in segs.iter().filter(|s| s.no_speech_prob <= NO_SPEECH_MAX) {
        text = join(&text, &s.text);
    }
    clean_phrase(&text, blocklist)
}

/// Schleifen kürzen, Blocklist anwenden, Leerzeichen normalisieren.
pub fn clean_phrase(text: &str, blocklist: &[String]) -> String {
    let t = collapse_repeats(text);
    if is_blocked(&t, blocklist) {
        String::new()
    } else {
        t
    }
}

fn strip_ellipsis_end(s: &str) -> Option<&str> {
    s.strip_suffix("...").or_else(|| s.strip_suffix('…'))
}

/// Hängt `next` an `acc` an: genau ein Leerzeichen, keins vor Satzzeichen, Auslassungspunkte an der Nahtstelle
/// entfernen, wenn der Satz kleingeschrieben weitergeht, ein an der Naht doppelt erkanntes Wort nur einmal.
pub fn join(acc: &str, next: &str) -> String {
    let mut a = acc.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut n = next.split_whitespace().collect::<Vec<_>>().join(" ");
    // führende Auslassungspunkte der Fortsetzung („...und dann“)
    for p in ["...", "…"] {
        if let Some(rest) = n.strip_prefix(p) {
            n = rest.trim_start().to_string();
        }
    }
    if a.is_empty() {
        return n;
    }
    if n.is_empty() {
        return a;
    }
    if n.chars().next().is_some_and(char::is_lowercase) {
        if let Some(rest) = strip_ellipsis_end(&a) {
            a = rest.trim_end().to_string();
        }
    }
    // an der Naht doppelt erkanntes Wort („wurden seine | seine Verdienste“), nicht über ein Satzende hinweg
    let last = a.rsplit(' ').next().unwrap_or("");
    let first = n.split(' ').next().unwrap_or("");
    let (nl, nf) = (norm_word(last), norm_word(first));
    if nl.chars().count() >= 3 && nl == nf && !last.ends_with(['.', '!', '?', '…']) {
        n = n[first.len()..].trim_start().to_string();
        if n.is_empty() {
            return a;
        }
    }
    if n.starts_with([',', '.', ';', ':', '!', '?', ')']) {
        format!("{a}{n}")
    } else {
        format!("{a} {n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bl() -> Vec<String> {
        crate::config::Config::default().blocklist
    }

    fn seg(text: &str, nsp: f32) -> Segment {
        Segment { text: text.into(), t0_ms: 0, t1_ms: 0, no_speech_prob: nsp }
    }

    #[test]
    fn normalize_basics() {
        assert_eq!(normalize("  Vielen Dank!  "), "vielen dank");
        assert_eq!(normalize("Fluorchlor-Kohlenwasserstoffe, FCKW."), "fluorchlor kohlenwasserstoffe fckw");
        assert_eq!(normalize("Don't"), "dont");
    }

    #[test]
    fn blocklist_matches_whole_phrase_only() {
        assert!(is_blocked("Vielen Dank.", &bl()));
        assert!(is_blocked(" vielen dank ", &bl()));
        assert!(is_blocked("Vielen Dank. Vielen Dank fürs Zuschauen!", &bl()));
        assert!(!is_blocked("Untertitel im Auftrag des ZDF, 2021", &bl()));
        assert!(!is_blocked("Vielen Dank für die Blumen.", &bl()));
        assert!(!is_blocked("", &bl()));
    }

    #[test]
    fn no_speech_segments_dropped() {
        let segs = [seg(" Hallo Welt.", 0.1), seg(" Vielen Dank.", 0.9)];
        assert_eq!(segments_to_text(&segs, &bl()), "Hallo Welt.");
        assert_eq!(segments_to_text(&[seg(" Vielen Dank.", 0.0)], &bl()), "");
    }

    #[test]
    fn loops_are_collapsed() {
        assert_eq!(collapse_repeats("ja ja ja ja ja gut"), "ja gut");
        assert_eq!(collapse_repeats("ja ja ja gut"), "ja ja ja gut");
        assert_eq!(collapse_repeats("Das ist gut. Das ist gut. Das ist gut. Das ist gut. Ende"), "Das ist gut. Ende");
        assert_eq!(collapse_repeats("a b a b a b a b a b c"), "a b c");
        assert_eq!(collapse_repeats("Der Umwelt, der Umwelt, der Umwelt, der Umwelt, der Umwelt, der Umwelt, der Umwelt,"), "Der Umwelt,");
        assert_eq!(collapse_repeats("dass die Welt der Welt der Welt der Welt der Welt der"), "dass die Welt der");
    }

    #[test]
    fn join_smooths_seams() {
        assert_eq!(join("", " Hallo "), "Hallo");
        assert_eq!(join("Hallo", ""), "Hallo");
        assert_eq!(join("Hallo  Welt.", "  Wie geht's?"), "Hallo Welt. Wie geht's?");
        assert_eq!(join("Er sagte...", "dass es geht."), "Er sagte dass es geht.");
        assert_eq!(join("Er sagte", "...dass es geht."), "Er sagte dass es geht.");
        assert_eq!(join("Er sagte...", "Das geht."), "Er sagte... Das geht.");
        assert_eq!(join("wurden seine", "seine Verdienste"), "wurden seine Verdienste");
        assert_eq!(join("Ende.", "Ende gut"), "Ende. Ende gut");
        assert_eq!(join("Hallo", ", Welt"), "Hallo, Welt");
    }
}
