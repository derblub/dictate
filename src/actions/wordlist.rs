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

//! EFF Large Wordlist (7776 Wörter) für den Geheimnisfilter.
//!
//! Die Passphrasen-Generatoren von Bitwarden und Vaultwarden ziehen ihre Wörter aus genau dieser Liste. Ob ein Wort darin
//! steht, trennt `correct-horse-battery-staple` (Passphrase) von `kebab-case-name` nur dort, wo die Wörter selten sind;
//! das reicht nicht für Gewissheit, aber für eine Regel mit wenigen Fehlalarmen (Messung: Moduldoku von
//! [`secrets`](super::secrets)).
//!
//! Die Liste liegt unverändert unter `assets/wordlists/eff_large_wordlist.txt` (Quelle, Prüfsumme und Lizenz CC BY 4.0
//! in `assets/wordlists/README.md`) und wird mit `include_str!` ins Binary eingebunden. Beim ersten Zugriff werden die
//! Würfelzahlen (`11111<TAB>abacus`) verworfen und die Wörter sortiert; danach ist [`contains`] eine Binärsuche ohne
//! Zuweisung. Vier Einträge enthalten einen Bindestrich (`drop-down`, `felt-tip`, `t-shirt`, `yo-yo`).

use std::sync::OnceLock;

const RAW: &str = include_str!("../../assets/wordlists/eff_large_wordlist.txt");

/// Länge des längsten Eintrags (`abdominal`, `drop-down`); `contains` lehnt alles Längere ohne Suche ab.
const MAX_WORD_LEN: usize = 9;

/// Alle Wörter ohne Würfelzahlen, sortiert (für Tests, die Passphrasen erzeugen).
pub fn words() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut v: Vec<&'static str> = RAW.lines().filter_map(|l| l.split_once('\t')).map(|(_, w)| w.trim()).filter(|w| !w.is_empty()).collect();
        v.sort_unstable();
        v
    })
}

/// Steht das Wort in der Liste? Groß- und Kleinschreibung (ASCII) gilt als gleich, denn die Generatoren schreiben den
/// ersten Buchstaben groß. Nicht-ASCII-Wörter sind nie in der Liste.
pub fn contains(word: &str) -> bool {
    let b = word.as_bytes();
    if !(3..=MAX_WORD_LEN).contains(&b.len()) {
        return false;
    }
    let mut buf = [0u8; MAX_WORD_LEN];
    for (d, s) in buf.iter_mut().zip(b) {
        *d = s.to_ascii_lowercase();
    }
    // Kleinschreibung ändert nur ASCII-Bytes: die Bytes bleiben gültiges UTF-8, sonst gäbe es keinen Treffer
    std::str::from_utf8(&buf[..b.len()]).is_ok_and(|w| words().binary_search(&w).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Prüfsumme der Datei von eff.org (Stand 2026-10-23). Ändert sich die Datei, muss das bewusst geschehen: Prüfsumme in
    /// `assets/wordlists/README.md` und hier gemeinsam anpassen, die Fehlalarm-Messung der Moduldoku von `secrets`
    /// wiederholen.
    const SHA256: &str = "addd35536511597a02fa0a9ff1e5284677b8883b83e986e43f15a3db996b903e";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn file_is_the_unmodified_eff_large_list() {
        assert_eq!(hex(&Sha256::digest(RAW.as_bytes())), SHA256, "eff_large_wordlist.txt weicht vom Original ab");
    }

    #[test]
    fn format_and_content_are_as_documented() {
        let lines: Vec<&str> = RAW.lines().collect();
        assert_eq!(lines.len(), 7776, "5 Würfel mit 6 Seiten");
        assert!(RAW.ends_with('\n') && !RAW.contains('\r'));
        let mut longest = 0;
        for (i, l) in lines.iter().enumerate() {
            let (dice, word) = l.split_once('\t').unwrap_or_else(|| panic!("Zeile {} ohne Tabulator", i + 1));
            // Würfelzahl = Position der Zeile zur Basis 6, Ziffern 1 bis 6
            let expect: String = (0..5).rev().map(|p| char::from(b'1' + (i / 6usize.pow(p) % 6) as u8)).collect();
            assert_eq!(dice, expect, "Zeile {}", i + 1);
            assert!(word.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') && !word.starts_with('-') && !word.ends_with('-'), "{word:?}");
            assert!((3..=MAX_WORD_LEN).contains(&word.len()), "{word:?}");
            longest = longest.max(word.len());
        }
        assert_eq!(longest, MAX_WORD_LEN);
        let w = words();
        assert_eq!(w.len(), 7776);
        assert!(w.windows(2).all(|p| p[0] < p[1]), "sortiert und ohne Doppelte");
        assert_eq!(w.iter().filter(|x| x.contains('-')).copied().collect::<Vec<_>>(), ["drop-down", "felt-tip", "t-shirt", "yo-yo"]);
    }

    #[test]
    fn lookup_ignores_ascii_case_and_rejects_everything_else() {
        for w in ["abacus", "Abacus", "ABACUS", "zoom", "Zoom", "yo-yo", "Drop-Down", "correct", "battery", "staple", "abdominal"] {
            assert!(contains(w), "{w}");
        }
        for w in [
            "",
            "a",
            "yo",
            "xyzzy",
            "horse",
            "orbit",
            "correctbattery",
            "abdominals",
            "abacus7",
            "abacus ",
            " abacus",
            "äbacus",
            "ÄBACUS",
            "zoo-",
            "-zoo",
            "abac\u{fffd}",
            "𝄞𝄞𝄞",
        ] {
            assert!(!contains(w), "{w:?}");
        }
        // jede Zeile findet sich selbst, in jeder Schreibweise
        for l in RAW.lines() {
            let w = l.split_once('\t').unwrap().1;
            assert!(contains(w) && contains(&w.to_uppercase()), "{w}");
        }
    }
}
