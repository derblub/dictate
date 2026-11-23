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

//! Wortdiff zwischen Markierung und Ersatztext, damit das Panel die Änderungen einer ersetzenden Aktion markieren kann.
//!
//! - **Tokens**: Läufe aus alphanumerischen Zeichen (`char::is_alphanumeric`, also auch Umlaute), Läufe aus Leerraum
//!   ohne `\n`, jedes `\n` einzeln (Zeilen bleiben so als Grenze sichtbar), jedes andere Zeichen einzeln (Satzzeichen
//!   ändern sich oft allein).
//! - **Diff**: Myers in linearem Speicher (Mittelschlange, teile und herrsche), Zeit O((N+M)·D) für N, M Tokens und D
//!   geänderte Tokens; höchstens [`MAX_EDITS`] Änderungen, darüber gibt es keinen Diff.
//! - **Aufbereitung**: gleiche Nachbarn zusammengefasst; ein unverändertes Stück nur aus Leerzeichen/Tabs zwischen zwei
//!   Änderungen gehört zur Änderung (sonst zerfiele „ein neuer Satz“ in drei markierte Wörter mit zwei unmarkierten
//!   Lücken); je Änderungsgruppe erst das Gelöschte, dann das Eingefügte.
//!
//! Std-only und ohne `crate::pipeline`: `dictate-settings` bindet `actions/` per Pfad ein. Der Text ist die Markierung
//! bzw. das KI-Ergebnis: nie loggen.

use std::collections::HashMap;

/// Art eines Diff-Stücks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// In beiden Texten
    Same,
    /// Nur im alten Text
    Del,
    /// Nur im neuen Text
    Ins,
}

impl Op {
    /// Zeichen im Protokoll: `=`, `-`, `+`
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Same => "=",
            Op::Del => "-",
            Op::Ins => "+",
        }
    }
}

/// Höchstens so viele geänderte Tokens (gelöschte plus eingefügte). Begrenzt die Rechenzeit: Myers braucht
/// O((N+M)·D), gewöhnlich eher O(N+M+D²). Bei der größten zulässigen Markierung (`actions.max_chars` 30000, je Seite
/// rund 14000 Tokens) gemessen: ein Zehntel der Wörter geändert etwa 20 ms, dieselben Wörter neu gemischt (läuft bis
/// an die Grenze) etwa 0,2 s (Release, `large_inputs_are_bounded`). Ganz neue Texte erreicht der Diff nicht, sie
/// scheitern vorher an der Zählung ([`surplus`]). Die Grenze lässt Umformulierungen langer Texte mit einem guten
/// Drittel geänderter Tokens noch zu; mehr ist bei dieser Länge ohnehin überwiegend neu.
pub const MAX_EDITS: usize = 10_000;

/// Wortdiff von `old` nach `new`. Die Stücke ergeben aneinandergehängt ohne `Ins` genau `old` und ohne `Del` genau
/// `new`.
///
/// `None`, wenn mehr als [`MAX_EDITS`] Tokens geändert sind oder der Text überwiegend neu ist: mehr als die Hälfte
/// der Nicht-Leerraum-Tokens beider Texte zusammen (gelöschte aus `old` plus eingefügte aus `new`, gemessen an allen
/// aus `old` plus `new`) ist geändert. Markierungen helfen nur, solange der unveränderte Text das Lesen trägt; ab der
/// Hälfte steht im Schnitt neben jedem erhaltenen Wort ein durchgestrichenes oder neues, das Panel wäre mit Diff
/// schwerer zu lesen als der neue Text allein. Die Schwelle zählt Wörter und Satzzeichen, nicht Leerraum, damit
/// geänderte Abstände oder Umbrüche einen sonst gleichen Text nicht „neu“ machen.
pub fn words(old: &str, new: &str) -> Option<Vec<(Op, String)>> {
    let a = tokens(old);
    let b = tokens(new);
    let (ia, ib) = intern(&a, &b);
    let total = solid(&a) + solid(&b);
    // Untergrenze ohne Diff: Wörter, die in einem Text öfter vorkommen als im anderen, sind sicher geändert. Spart
    // bei ganz neuen Texten den Diff (sonst bis an [`MAX_EDITS`])
    if surplus(&a, &ia, &b, &ib) * 2 > total {
        return None;
    }
    let mut runs: Vec<(Op, usize)> = Vec::new();
    let mut budget = MAX_EDITS;
    compare(&ia, &ib, &mut runs, &mut budget).ok()?;

    // Schwelle auf Token-Ebene, vor der Aufbereitung (die Leerraum in Änderungen zieht)
    let (mut changed, mut pa, mut pb) = (0, 0, 0);
    for &(op, n) in &runs {
        match op {
            Op::Same => {
                pa += n;
                pb += n;
            }
            Op::Del => {
                changed += solid(&a[pa..pa + n]);
                pa += n;
            }
            Op::Ins => {
                changed += solid(&b[pb..pb + n]);
                pb += n;
            }
        }
    }
    if changed * 2 > total {
        return None;
    }
    Some(tidy(&runs, old, new, &a, &b))
}

/// Zerlegt in Tokens (siehe Modulkopf); aneinandergehängt ergeben sie `s`.
fn tokens(s: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Single,
    }
    let class = |c: char| {
        if c.is_alphanumeric() {
            Class::Word
        } else if c.is_whitespace() && c != '\n' {
            Class::Space
        } else {
            Class::Single
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<Class> = None;
    for (i, c) in s.char_indices() {
        let k = class(c);
        if let Some(p) = prev {
            if p != k || k == Class::Single {
                out.push(&s[start..i]);
                start = i;
            }
        }
        prev = Some(k);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Zahl der Tokens, die nicht Leerraum sind (Wörter, Satzzeichen)
fn solid(toks: &[&str]) -> usize {
    toks.iter().filter(|t| !t.starts_with(char::is_whitespace)).count()
}

/// Zahl der Nicht-Leerraum-Tokens, die ein Text öfter enthält als der andere (beide Richtungen zusammen): so viele
/// sind in jedem Diff gelöscht bzw. eingefügt, denn eine gemeinsame Teilfolge enthält jedes Token höchstens so oft wie
/// der Text, der es seltener hat.
fn surplus(a: &[&str], ia: &[u32], b: &[&str], ib: &[u32]) -> usize {
    let mut count: HashMap<u32, isize> = HashMap::new();
    for (t, i) in a.iter().zip(ia) {
        if !t.starts_with(char::is_whitespace) {
            *count.entry(*i).or_default() += 1;
        }
    }
    for (t, i) in b.iter().zip(ib) {
        if !t.starts_with(char::is_whitespace) {
            *count.entry(*i).or_default() -= 1;
        }
    }
    count.values().map(|c| c.unsigned_abs()).sum()
}

/// Tokens als Zahlen (gleiches Token, gleiche Zahl): der Vergleich in Myers ist dann ein Ganzzahlvergleich.
fn intern<'t>(a: &[&'t str], b: &[&'t str]) -> (Vec<u32>, Vec<u32>) {
    let mut ids: HashMap<&'t str, u32> = HashMap::new();
    let mut map = |toks: &[&'t str]| -> Vec<u32> {
        toks.iter()
            .map(|t| {
                let n = ids.len() as u32;
                *ids.entry(*t).or_insert(n)
            })
            .collect()
    };
    let ia = map(a);
    let ib = map(b);
    (ia, ib)
}

/// Hängt `n` Tokens der Art `op` an; gleiche Nachbarn werden ein Lauf.
fn push(runs: &mut Vec<(Op, usize)>, op: Op, n: usize) {
    if n == 0 {
        return;
    }
    match runs.last_mut() {
        Some((o, m)) if *o == op => *m += n,
        _ => runs.push((op, n)),
    }
}

/// Mehr Änderungen als erlaubt
struct TooMany;

/// Kürzeste Folge von Änderungen von `a` nach `b` als Läufe (Myers, linearer Speicher). `budget` ist die Zahl der
/// noch erlaubten Änderungen; die oberste Ebene findet D für das ganze Problem, die Teilprobleme teilen es sich.
fn compare(a: &[u32], b: &[u32], runs: &mut Vec<(Op, usize)>, budget: &mut usize) -> Result<(), TooMany> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[pre..], &b[pre..]);
    let suf = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[..a.len() - suf], &b[..b.len() - suf]);
    push(runs, Op::Same, pre);
    if a.is_empty() || b.is_empty() {
        let d = a.len() + b.len();
        if d > *budget {
            return Err(TooMany);
        }
        *budget -= d;
        push(runs, Op::Del, a.len());
        push(runs, Op::Ins, b.len());
    } else {
        let s = middle_snake(a, b, *budget).ok_or(TooMany)?;
        compare(&a[..s.x], &b[..s.y], runs, budget)?;
        push(runs, Op::Same, s.u - s.x);
        compare(&a[s.u..], &b[s.v..], runs, budget)?;
    }
    push(runs, Op::Same, suf);
    Ok(())
}

/// Mittlere Schlange eines kürzesten Pfads: von (x, y) bis (u, v) nur Diagonalen
struct Snake {
    x: usize,
    y: usize,
    u: usize,
    v: usize,
}

/// Myers (1986), Abschnitt 4b: läuft gleichzeitig vom Anfang und vom Ende, bis sich die Pfade auf einer Diagonale
/// treffen. `a`, `b` nicht leer. `None`, wenn D größer als `max_d` ist.
fn middle_snake(a: &[u32], b: &[u32], max_d: usize) -> Option<Snake> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let delta = n - m;
    let odd = delta & 1 != 0;
    // Runden je Richtung: D = 2d-1 bzw. 2d, also reichen d ≤ (D+1)/2
    let rounds = (((n + m + 1) / 2) as usize).min(max_d.div_ceil(2).saturating_add(1)) as isize;
    let off = rounds + 1;
    let size = (2 * off + 1) as usize;
    // vf[k]: weitestes x auf Diagonale k = x - y von vorn; vb[k]: weitestes x von hinten (gezählt vom Ende) auf
    // Diagonale k der umgedrehten Folgen
    let mut vf = vec![0isize; size];
    let mut vb = vec![0isize; size];
    let idx = |k: isize| (k + off) as usize;
    for d in 0..=rounds {
        // vorwärts
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && vf[idx(k - 1)] < vf[idx(k + 1)]) { vf[idx(k + 1)] } else { vf[idx(k - 1)] + 1 };
            let mut y = x - k;
            let (x0, y0) = (x, y);
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            vf[idx(k)] = x;
            let kb = delta - k;
            if odd && kb.abs() < d && vf[idx(k)] + vb[idx(kb)] >= n {
                if 2 * d as usize - 1 > max_d {
                    return None;
                }
                return Some(Snake { x: x0 as usize, y: y0 as usize, u: x as usize, v: y as usize });
            }
            k += 2;
        }
        // rückwärts
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && vb[idx(k - 1)] < vb[idx(k + 1)]) { vb[idx(k + 1)] } else { vb[idx(k - 1)] + 1 };
            let mut y = x - k;
            let (x0, y0) = (x, y);
            while x < n && y < m && a[(n - 1 - x) as usize] == b[(m - 1 - y) as usize] {
                x += 1;
                y += 1;
            }
            vb[idx(k)] = x;
            let kf = delta - k;
            if !odd && kf.abs() <= d && vb[idx(k)] + vf[idx(kf)] >= n {
                if 2 * d as usize > max_d {
                    return None;
                }
                return Some(Snake { x: (n - x) as usize, y: (m - y) as usize, u: (n - x0) as usize, v: (m - y0) as usize });
            }
            k += 2;
        }
    }
    None
}

/// Läufe in Textstücke: Leerzeichen/Tabs zwischen zwei Änderungen gehören zur Änderung, je Gruppe erst `Del`, dann
/// `Ins`, gleiche Nachbarn zusammen.
fn tidy(runs: &[(Op, usize)], old: &str, new: &str, a: &[&str], b: &[&str]) -> Vec<(Op, String)> {
    // Byte-Grenzen der Tokens: Stücke sind Teilstrings von old/new
    let ends = |toks: &[&str]| {
        let mut e = Vec::with_capacity(toks.len() + 1);
        e.push(0);
        let mut p = 0;
        for t in toks {
            p += t.len();
            e.push(p);
        }
        e
    };
    let (ea, eb) = (ends(a), ends(b));
    // (Art, Teilstring) je Lauf
    let mut pieces: Vec<(Op, &str)> = Vec::with_capacity(runs.len());
    let (mut pa, mut pb) = (0, 0);
    for &(op, n) in runs {
        match op {
            Op::Same => {
                pieces.push((op, &old[ea[pa]..ea[pa + n]]));
                pa += n;
                pb += n;
            }
            Op::Del => {
                pieces.push((op, &old[ea[pa]..ea[pa + n]]));
                pa += n;
            }
            Op::Ins => {
                pieces.push((op, &new[eb[pb]..eb[pb + n]]));
                pb += n;
            }
        }
    }
    let blank = |s: &str| !s.is_empty() && s.chars().all(|c| c == ' ' || c == '\t');
    let mut out: Vec<(Op, String)> = Vec::new();
    let (mut del, mut ins, mut open) = (String::new(), String::new(), false);
    let flush = |out: &mut Vec<(Op, String)>, del: &mut String, ins: &mut String| {
        for (op, s) in [(Op::Del, del), (Op::Ins, ins)] {
            if !s.is_empty() {
                out.push((op, std::mem::take(s)));
            }
        }
    };
    for (i, &(op, s)) in pieces.iter().enumerate() {
        match op {
            Op::Del => {
                del.push_str(s);
                open = true;
            }
            Op::Ins => {
                ins.push_str(s);
                open = true;
            }
            Op::Same if open && blank(s) && pieces.get(i + 1).is_some_and(|(o, _)| *o != Op::Same) => {
                del.push_str(s);
                ins.push_str(s);
            }
            Op::Same => {
                flush(&mut out, &mut del, &mut ins);
                open = false;
                match out.last_mut() {
                    Some((Op::Same, t)) => t.push_str(s),
                    _ => out.push((Op::Same, s.to_string())),
                }
            }
        }
    }
    flush(&mut out, &mut del, &mut ins);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(d: &[(Op, String)], skip: Op) -> String {
        d.iter().filter(|(o, _)| *o != skip).map(|(_, s)| s.as_str()).collect()
    }

    fn check(old: &str, new: &str) -> Option<Vec<(Op, String)>> {
        let d = words(old, new);
        if let Some(d) = &d {
            assert_eq!(side(d, Op::Ins), old, "old side of {d:?}");
            assert_eq!(side(d, Op::Del), new, "new side of {d:?}");
            assert!(d.iter().all(|(_, s)| !s.is_empty()), "empty piece in {d:?}");
            // keine gleichen Nachbarn, keine Same-Nachbarn, in einer Gruppe erst Del, dann Ins
            for w in d.windows(2) {
                assert_ne!(w[0].0, w[1].0, "{d:?}");
                assert!(!(w[0].0 == Op::Ins && w[1].0 == Op::Del), "{d:?}");
            }
        }
        d
    }

    fn pieces(d: &[(Op, String)]) -> Vec<(&str, &str)> {
        d.iter().map(|(o, s)| (o.as_str(), s.as_str())).collect()
    }

    #[test]
    fn tokens_split_words_spaces_newlines_and_marks() {
        assert_eq!(tokens("Grüße,  Welt!\n\nÄrger\t x2"), ["Grüße", ",", "  ", "Welt", "!", "\n", "\n", "Ärger", "\t ", "x2"]);
        assert_eq!(tokens(" \r\n"), [" \r", "\n"]);
        assert_eq!(tokens("a--b"), ["a", "-", "-", "b"]);
        assert!(tokens("").is_empty());
    }

    #[test]
    fn one_word_changed() {
        let d = check("Das ist ein Test mit Umlauten.", "Das ist ein Versuch mit Umlauten.").unwrap();
        assert_eq!(pieces(&d), [("=", "Das ist ein "), ("-", "Test"), ("+", "Versuch"), ("=", " mit Umlauten.")]);
    }

    #[test]
    fn spaces_between_changes_join_the_change() {
        let d = check("Ich gehe heute nach Hause und schlafe.", "Ich laufe morgen nach Hause und schlafe.").unwrap();
        assert_eq!(pieces(&d), [("=", "Ich "), ("-", "gehe heute"), ("+", "laufe morgen"), ("=", " nach Hause und schlafe.")]);
    }

    #[test]
    fn newline_stays_a_boundary() {
        let d = check("eins zwei\ndrei vier\nfünf sechs", "eins zwo\nDrei vier\nfünf sechs").unwrap();
        assert_eq!(pieces(&d), [("=", "eins "), ("-", "zwei"), ("+", "zwo"), ("=", "\n"), ("-", "drei"), ("+", "Drei"), ("=", " vier\nfünf sechs")]);
    }

    #[test]
    fn punctuation_and_insertions() {
        let d = check("Hallo Welt, wie geht es dir", "Hallo, Welt, wie geht es dir?").unwrap();
        assert_eq!(pieces(&d), [("=", "Hallo"), ("+", ","), ("=", " Welt, wie geht es dir"), ("+", "?")]);
        let d = check("a b c d e f", "a b c X d e f").unwrap();
        assert_eq!(pieces(&d), [("=", "a b c "), ("+", "X "), ("=", "d e f")]);
    }

    #[test]
    fn empty_and_equal() {
        assert_eq!(check("", ""), Some(vec![]));
        assert_eq!(check("gleich", "gleich").unwrap(), [(Op::Same, "gleich".to_string())]);
        // ganz neu bzw. ganz weg: kein Diff
        assert_eq!(check("", "neu"), None);
        assert_eq!(check("alt", ""), None);
        // nur Leerraum geändert: kein Wort neu
        let d = check("a  b", "a b\n").unwrap();
        assert_eq!(side(&d, Op::Del), "a b\n");
        assert_eq!(check(" ", "\n\n"), Some(vec![(Op::Del, " ".into()), (Op::Ins, "\n\n".into())]));
    }

    #[test]
    fn threshold_is_half_of_the_words() {
        // 4 von 8 Wörtern geändert (je 2 alt und neu): genau die Hälfte, noch ein Diff
        assert!(check("a b c d", "a b x y").is_some());
        // 6 von 8: überwiegend neu
        assert_eq!(check("a b c d", "a x y z"), None);
        assert_eq!(check("Guten Morgen", "Something entirely different here"), None);
    }

    #[test]
    fn minimal_against_dynamic_programming() {
        // Länge der LCS per O(N·M) als Referenz: die Zahl geänderter Tokens muss minimal sein
        fn lcs(a: &[u32], b: &[u32]) -> usize {
            let mut prev = vec![0; b.len() + 1];
            for x in a {
                let mut cur = vec![0; b.len() + 1];
                for (j, y) in b.iter().enumerate() {
                    cur[j + 1] = if x == y { prev[j] + 1 } else { cur[j].max(prev[j + 1]) };
                }
                prev = cur;
            }
            prev[b.len()]
        }
        let mut rng = Rng(7);
        for _ in 0..3000 {
            let a: Vec<u32> = (0..rng.below(14)).map(|_| rng.below(4) as u32).collect();
            let b: Vec<u32> = (0..rng.below(14)).map(|_| rng.below(4) as u32).collect();
            let mut runs = Vec::new();
            let mut budget = usize::MAX;
            assert!(compare(&a, &b, &mut runs, &mut budget).is_ok());
            let edits: usize = runs.iter().filter(|(o, _)| *o != Op::Same).map(|(_, n)| n).sum();
            assert_eq!(edits, a.len() + b.len() - 2 * lcs(&a, &b), "{a:?} {b:?} {runs:?}");
            let (ta, tb): (Vec<String>, Vec<String>) = (a.iter().map(u32::to_string).collect(), b.iter().map(u32::to_string).collect());
            let (ta, tb): (Vec<&str>, Vec<&str>) = (ta.iter().map(String::as_str).collect(), tb.iter().map(String::as_str).collect());
            assert!(surplus(&ta, &a, &tb, &b) <= edits);
            // Läufe ergeben beide Folgen
            let (mut pa, mut pb) = (0, 0);
            for &(o, n) in &runs {
                match o {
                    Op::Same => {
                        assert_eq!(a[pa..pa + n], b[pb..pb + n]);
                        pa += n;
                        pb += n;
                    }
                    Op::Del => pa += n,
                    Op::Ins => pb += n,
                }
            }
            assert_eq!((pa, pb), (a.len(), b.len()));
        }
    }

    /// Kleiner deterministischer Zufall (xorshift), ohne Crate
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    const PARTS: [&str; 14] = ["ä", "Öl", "groß", " ", "  ", "\t", "\n", "\r\n", ",", ".", "x", "über", "-", "🙂"];

    fn random_text(rng: &mut Rng, max: usize) -> String {
        (0..rng.below(max)).map(|_| PARTS[rng.below(PARTS.len())]).collect()
    }

    /// Zufällige Änderungen eines Textes: Teile ersetzen, löschen, einfügen
    fn mutate(rng: &mut Rng, s: &str) -> String {
        let toks = tokens(s);
        let mut out = String::new();
        for t in toks {
            match rng.below(10) {
                0 => {}
                1 => out.push_str(PARTS[rng.below(PARTS.len())]),
                2 => {
                    out.push_str(t);
                    out.push_str(PARTS[rng.below(PARTS.len())]);
                }
                _ => out.push_str(t),
            }
        }
        out
    }

    #[test]
    fn random_inputs_keep_the_invariant() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut some = 0;
        for _ in 0..4000 {
            let old = random_text(&mut rng, 40);
            let new = if rng.below(4) == 0 { random_text(&mut rng, 40) } else { mutate(&mut rng, &old) };
            some += usize::from(check(&old, &new).is_some());
        }
        assert!(some > 1000, "{some}");
    }

    #[test]
    fn large_inputs_are_bounded() {
        let mut rng = Rng(42);
        let vocab = ["Haus", "Baum", "über", "und", "der", "die", "das", "Zeile", "groß", "klein"];
        let text = |rng: &mut Rng, vocab: &[&str]| -> String {
            let mut s = String::new();
            while s.chars().count() < 30_000 {
                s.push_str(vocab[rng.below(vocab.len())]);
                s.push_str([" ", " ", " ", ", ", ".\n"][rng.below(5)]);
            }
            s
        };
        let old = text(&mut rng, &vocab);
        // ein Zehntel der Wörter geändert: Diff in kurzer Zeit
        let new: String = tokens(&old).iter().map(|t| if rng.below(10) == 0 && !t.starts_with(' ') { "neu" } else { t }).collect();
        let t = std::time::Instant::now();
        let d = check(&old, &new).expect("diff");
        assert!(d.len() > 100);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
        // zwei unabhängige Texte: über der Grenze bzw. überwiegend neu, ebenfalls begrenzt
        let other = text(&mut rng, &["Wald", "Feld", "Wiese", "Bach", "Berg", "Tal", "See", "Fluss"]);
        let t = std::time::Instant::now();
        assert_eq!(words(&old, &other), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
        // gleiche Wörter in anderer Reihenfolge: die Zählung lässt den Diff zu, er endet an MAX_EDITS
        let shuffled = text(&mut rng, &vocab);
        let t = std::time::Instant::now();
        assert_eq!(words(&old, &shuffled), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn edit_budget_is_a_hard_limit() {
        let (a, b) = ([1, 2, 3, 4], [5, 2, 3, 6]);
        let mut runs = Vec::new();
        assert!(compare(&a, &b, &mut runs, &mut 3).is_err());
        let (mut runs, mut budget) = (Vec::new(), 4);
        assert!(compare(&a, &b, &mut runs, &mut budget).is_ok());
        assert_eq!((runs, budget), (vec![(Op::Del, 1), (Op::Ins, 1), (Op::Same, 2), (Op::Del, 1), (Op::Ins, 1)], 0));
        // Zählung als Untergrenze: ganz neue Texte ohne Diff
        let (a, b) = (tokens("a b c"), tokens("a b d"));
        let (ia, ib) = intern(&a, &b);
        assert_eq!(surplus(&a, &ia, &b, &ib), 2);
    }
}
