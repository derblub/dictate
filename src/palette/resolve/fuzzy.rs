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

//! Fuzzy-Suche im Stil von fzf für den lokalen Resolver: Falten von Groß/klein und Diakritika, Güte eines Treffers.
//!
//! **Falten.** Text und Eingabe werden vor dem Vergleich klein geschrieben und von Diakritika befreit, und zwar in
//! *zwei* Formen: die einfache Form macht aus `ä ö ü` die Grundbuchstaben (`a o u`), die deutsche Form schreibt sie
//! aus (`ae oe ue`); `ß` ist in beiden `ss`, übrige Akzente (`é ç ñ …`) fallen in beiden weg. Ein Treffer in *einer*
//! der beiden Formen zählt. So finden „grosse“, „größe“ und „groesse“ alle „Größe“ (und „Groesse“ findet „größe“ aus
//! der deutschen Form), ohne dass jemand auf der Tastatur Umlaute tippen muss.
//!
//! **Güte** (`Q`, 0 bis 100, je höher desto besser), vom Ganzen zum Groben:
//! - **Exakt** (`Q = 100`): die Eingabe ist das ganze Feld.
//! - **Präfix** (`88 … 98`): das Feld beginnt mit der Eingabe; je größer der Anteil der Eingabe am Feld, desto höher
//!   (kürzere Felder zuerst).
//! - **Wortanfänge** (`70 … 77`): jedes Wort der Eingabe ist der Anfang irgendeines Wortes im Feld („code“ in „Visual
//!   Studio Code“, „bild sperr“ in „Bildschirm sperren“).
//! - **Fuzzy** (höchstens `60`): die Zeichen jedes Wortes kommen in Reihenfolge im Feld vor, bewertet wie bei fzf
//!   (zusammenhängende Läufe, Wortanfänge und CamelCase-Übergänge zählen mehr, Lücken kosten). „ff“ findet „Firefox“,
//!   „vsc“ findet „Visual Studio Code“, „dr“ findet „DataRoom“.
//!
//! Die Stufen sind so gewählt, dass Fuzzy nie ein Präfix und Wortanfänge nie ein Präfix überholen; der Abstand
//! dazwischen (mindestens 10) ist größer als jeder Häufigkeitsbonus (`resolve::FRECENCY_MAX`).
//!
//! **Leistung.** Gefaltet wird in wiederverwendete Puffer (keine Zuteilung je Feld). Die teure Fuzzy-Rechnung
//! (O(Eingabe × Feld), Feld auf [`MAX_FIELD_CHARS`] begrenzt) läuft nur, wenn die Zeichen der Eingabe überhaupt als
//! Teilfolge im Feld stehen; bei 20 Zeichen Eingabe fällt fast jedes Feld schon dort heraus.

use std::slice;

/// Längster durchsuchter Teil eines Feldes in Zeichen. Fenstertitel sind bis 512 Zeichen lang; der Anfang trägt die
/// Information, und die Rechnung bleibt begrenzt.
pub const MAX_FIELD_CHARS: usize = 160;
/// Höchstens so viele Wörter in der Eingabe; mehr ist ein Satz für das Modell, keine Suche.
pub const MAX_TOKENS: usize = 8;
/// Höchstens so viele Zeichen (ohne Leerraum) in der Eingabe.
pub const MAX_NEEDLE_CHARS: usize = 64;

pub const Q_EXACT: f32 = 100.0;
/// Untergrenze der Präfix-Stufe; dazu kommt bis [`Q_PREFIX_SPAN`] nach Anteil der Eingabe am Feld.
pub const Q_PREFIX: f32 = 88.0;
pub const Q_PREFIX_SPAN: f32 = 10.0;
pub const Q_WORD: f32 = 70.0;
pub const Q_WORD_SPAN: f32 = 7.0;
/// Höchste Güte eines reinen Fuzzy-Treffers.
pub const Q_FUZZY: f32 = 60.0;
/// Mehrere Wörter, jedes in einem anderen Feld desselben Eintrags gefunden: Abschlag gegenüber einem Treffer in einem
/// Feld.
const SPLIT_FACTOR: f32 = 0.95;

// Bewertung der Fuzzy-Rechnung (Werte und Aufbau wie fzf v2).
const MATCH: i32 = 16;
const GAP_START: i32 = 3;
const GAP_EXT: i32 = 1;
const B_BOUNDARY: u8 = 8;
const B_CAMEL: u8 = 7;
const B_CONSEC: i32 = 4;
const FIRST_MULT: i32 = 2;
/// Kleiner als jeder erreichbare Wert, mit Luft nach unten für Abzüge (kein Überlauf).
const NONE: i32 = i32::MIN / 4;

/// Ein gefalteter Text: Zeichen und je Zeichen der Bonus für die Lage (Wortanfang, CamelCase).
#[derive(Default)]
struct Folded {
    chars: Vec<char>,
    bonus: Vec<u8>,
}

impl Folded {
    fn clear(&mut self) {
        self.chars.clear();
        self.bonus.clear();
    }

    fn push(&mut self, c: char, bonus: u8) {
        self.chars.push(c);
        self.bonus.push(bonus);
    }

    fn len(&self) -> usize {
        self.chars.len()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Anfang des Textes oder ein Zeichen, das kein Buchstabe und keine Ziffer ist.
    Delim,
    Lower,
    Upper,
    Digit,
}

fn class_of(c: char) -> Class {
    if c.is_uppercase() {
        Class::Upper
    } else if c.is_lowercase() || c.is_alphabetic() {
        Class::Lower
    } else if c.is_numeric() {
        Class::Digit
    } else {
        Class::Delim
    }
}

/// Bonus für ein Zeichen der Klasse `cur` nach einem der Klasse `prev`.
fn bonus_for(prev: Class, cur: Class) -> u8 {
    match (prev, cur) {
        (Class::Delim, Class::Lower | Class::Upper | Class::Digit) => B_BOUNDARY,
        (Class::Lower, Class::Upper) => B_CAMEL,
        (Class::Lower | Class::Upper, Class::Digit) => B_CAMEL,
        _ => 0,
    }
}

/// Einfache Form eines kleingeschriebenen Zeichens, wenn es sich ändert.
fn plain_of(c: char) -> Option<&'static str> {
    Some(match c {
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'þ' => "th",
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ţ' | 'ť' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// Deutsche Form: Umlaute ausgeschrieben, sonst wie die einfache.
fn german_of(c: char, plain: &'static str) -> &'static str {
    match c {
        'ä' => "ae",
        'ö' | 'ø' => "oe",
        'ü' => "ue",
        _ => plain,
    }
}

/// Kombinierende Zeichen (zerlegte Akzente): entfallen beim Falten.
fn is_combining(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

/// Faltet `text` in beide Formen (die Puffer werden zuerst geleert), höchstens `cap` Zeichen je Form. Gibt zurück, ob
/// sich die deutsche von der einfachen Form unterscheidet.
fn fold_text(text: &str, plain: &mut Folded, german: &mut Folded, cap: usize) -> bool {
    plain.clear();
    german.clear();
    let mut prev = Class::Delim;
    let mut differs = false;
    for c in text.chars() {
        if is_combining(c) {
            // zerlegtes Ä/Ö/Ü: a/o/u + Trema; die deutsche Form schreibt ein e dahinter
            if c == '\u{0308}' && matches!(german.chars.last(), Some('a' | 'o' | 'u')) {
                german.push('e', 0);
                differs = true;
            }
            continue;
        }
        if plain.len() >= cap {
            break;
        }
        let class = class_of(c);
        let bonus = bonus_for(prev, class);
        prev = class;
        for lower in c.to_lowercase() {
            match plain_of(lower) {
                Some(p) => {
                    let g = german_of(lower, p);
                    differs |= p != g;
                    for (i, pc) in p.chars().enumerate() {
                        plain.push(pc, if i == 0 { bonus } else { 0 });
                    }
                    for (i, gc) in g.chars().enumerate() {
                        german.push(gc, if i == 0 { bonus } else { 0 });
                    }
                }
                None => {
                    plain.push(lower, bonus);
                    german.push(lower, bonus);
                }
            }
        }
    }
    // ein Zeichen kann zu zwei werden (ß → ss): die Grenze gilt für die Zeichen nach dem Falten
    for folded in [plain, german] {
        folded.chars.truncate(cap);
        folded.bonus.truncate(cap);
    }
    differs
}

/// Gefalteter Text als String, in der einfachen (`german = false`) oder der deutschen Form. Für den Vergleich fester
/// Wörter (z. B. der Befehlspräfixe), nicht für die Suche.
pub fn fold_string(text: &str, german: bool) -> String {
    let (mut plain, mut g) = (Folded::default(), Folded::default());
    fold_text(text, &mut plain, &mut g, usize::MAX);
    let f = if german { g } else { plain };
    f.chars.into_iter().collect()
}

/// Beide gefalteten Formen der Eingabe.
struct NeedleForm {
    /// Wörter mit je einem Leerzeichen dazwischen.
    joined: Vec<char>,
    tokens: Vec<Vec<char>>,
}

/// Die Eingabe, einmal gefaltet; Grundlage aller [`Matcher`].
pub struct Needle {
    forms: [NeedleForm; 2],
    /// Unterscheidet sich die deutsche Form von der einfachen?
    german_differs: bool,
}

impl Needle {
    /// `None` bei leerer Eingabe, mehr als [`MAX_TOKENS`] Wörtern oder mehr als [`MAX_NEEDLE_CHARS`] Zeichen: das ist
    /// keine Suche mehr, sondern ein Satz.
    pub fn new(text: &str) -> Option<Needle> {
        let (mut plain, mut german) = (Folded::default(), Folded::default());
        let mut forms = [NeedleForm { joined: Vec::new(), tokens: Vec::new() }, NeedleForm { joined: Vec::new(), tokens: Vec::new() }];
        let mut german_differs = false;
        let mut raw_chars = 0;
        for (n, token) in text.split_whitespace().enumerate() {
            if n >= MAX_TOKENS {
                return None;
            }
            raw_chars += token.chars().count();
            if raw_chars > MAX_NEEDLE_CHARS {
                return None;
            }
            german_differs |= fold_text(token, &mut plain, &mut german, MAX_NEEDLE_CHARS * 2);
            if plain.len() == 0 {
                continue;
            }
            for (form, folded) in forms.iter_mut().zip([&plain, &german]) {
                if !form.joined.is_empty() {
                    form.joined.push(' ');
                }
                form.joined.extend_from_slice(&folded.chars);
                form.tokens.push(folded.chars.clone());
            }
        }
        if forms[0].tokens.is_empty() {
            return None;
        }
        Some(Needle { forms, german_differs })
    }

    /// Anzahl der Wörter.
    pub fn tokens(&self) -> usize {
        self.forms[0].tokens.len()
    }
}

/// Mindestanteil der Fuzzy-Bewertung an der bestmöglichen, je nach Länge des Wortes: ein Zeichen zählt nur an einem
/// Wortanfang, zwei brauchen noch gute Lage, ab drei reicht ein lockerer Zusammenhang.
fn min_ratio(len: usize) -> f32 {
    match len {
        1 => 0.9,
        2 => 0.5,
        _ => 0.4,
    }
}

/// Fuzzy-Anteil (0 bis 1) von `n` in `h` oder `None`, wenn es nicht als Teilfolge vorkommt oder zu locker liegt.
fn fuzzy_ratio(n: &[char], h: &Folded) -> Option<f32> {
    let (nl, hl) = (n.len(), h.len());
    if nl == 0 || nl > hl {
        return None;
    }
    // Teilfolge? Fast jedes Feld fällt hier heraus.
    let mut k = 0;
    for &c in &h.chars {
        if c == n[k] {
            k += 1;
            if k == nl {
                break;
            }
        }
    }
    if k < nl {
        return None;
    }
    // m[i][j]: beste Bewertung, wenn n[i] genau auf h[j] liegt; g[i][j]: n[i] lag früher, dazwischen eine Lücke.
    let mut m = [[NONE; MAX_FIELD_CHARS]; 2];
    let mut g = [[NONE; MAX_FIELD_CHARS]; 2];
    for (i, &nc) in n.iter().enumerate() {
        let (cur, prev) = (i & 1, (i & 1) ^ 1);
        for j in 0..hl {
            let mut score = NONE;
            if h.chars[j] == nc {
                let b = i32::from(h.bonus[j]);
                if i == 0 {
                    score = MATCH + b * FIRST_MULT;
                } else if j > 0 {
                    score = (m[prev][j - 1] + MATCH + b.max(B_CONSEC)).max(g[prev][j - 1] + MATCH + b);
                }
            }
            m[cur][j] = score;
            g[cur][j] = if j > 0 { (m[cur][j - 1] - GAP_START).max(g[cur][j - 1] - GAP_EXT) } else { NONE };
        }
    }
    let raw = m[(nl - 1) & 1][..hl].iter().copied().max()?;
    if raw < 0 {
        return None;
    }
    // bestmöglich: ein zusammenhängender Lauf ab einem Wortanfang
    let perfect = nl as i32 * MATCH + (nl as i32 - 1) * B_CONSEC + FIRST_MULT * i32::from(B_BOUNDARY);
    let ratio = (raw as f32 / perfect as f32).min(1.0);
    (ratio >= min_ratio(nl)).then_some(ratio)
}

/// Güte der Wörter `tokens` (zu `joined` verbunden) gegen den gefalteten Text `h`.
fn tier_score(joined: &[char], tokens: &[Vec<char>], h: &Folded) -> Option<f32> {
    let hl = h.len();
    if hl == 0 {
        return None;
    }
    let nl = joined.len();
    if nl <= hl && h.chars.starts_with(joined) {
        return Some(if nl == hl { Q_EXACT } else { Q_PREFIX + Q_PREFIX_SPAN * nl as f32 / hl as f32 });
    }
    // Wortanfänge: jedes Wort der Eingabe beginnt irgendein Wort im Feld
    let word_starts = (0..hl).filter(|&i| h.chars[i].is_alphanumeric() && (i == 0 || !h.chars[i - 1].is_alphanumeric()));
    let all_words = tokens.iter().all(|t| word_starts.clone().any(|i| h.chars[i..].starts_with(t)));
    if all_words {
        let alnum = h.chars.iter().filter(|c| c.is_alphanumeric()).count().max(1);
        let typed: usize = tokens.iter().map(Vec::len).sum();
        return Some(Q_WORD + Q_WORD_SPAN * (typed as f32 / alnum as f32).min(1.0));
    }
    let mut sum = 0.0;
    for t in tokens {
        sum += fuzzy_ratio(t, h)?;
    }
    Some(Q_FUZZY * sum / tokens.len() as f32)
}

/// Wertet Felder gegen eine [`Needle`] aus. Hält die Puffer zum Falten, damit nicht je Feld zugeteilt wird; ein
/// `Matcher` gehört zu einem Durchlauf und wird nicht geteilt.
pub struct Matcher<'n> {
    needle: &'n Needle,
    plain: Folded,
    german: Folded,
}

impl<'n> Matcher<'n> {
    pub fn new(needle: &'n Needle) -> Matcher<'n> {
        Matcher { needle, plain: Folded::default(), german: Folded::default() }
    }

    /// Güte des ganzen Textes (alle Wörter) gegen ein Feld; `None` ohne Treffer.
    pub fn score(&mut self, text: &str) -> Option<f32> {
        self.item([(text, 1.0)])
    }

    /// Güte der Eingabe gegen die Felder *eines* Eintrags (Text, Gewicht): die beste Güte eines Feldes mal sein Gewicht.
    /// Bei mehreren Wörtern zählt auch, dass jedes Wort in einem anderen Feld steht (etwa Gruppe und Name eines
    /// Projekts), mit dem Abschlag [`SPLIT_FACTOR`]. `None`, wenn kein Feld (und keine Aufteilung) trifft.
    pub fn item<'a>(&mut self, fields: impl IntoIterator<Item = (&'a str, f32)>) -> Option<f32> {
        let k = self.needle.tokens();
        let mut joint: Option<f32> = None;
        let mut per_token = [0.0f32; MAX_TOKENS];
        for (text, weight) in fields {
            if text.is_empty() {
                continue;
            }
            let differs = fold_text(text, &mut self.plain, &mut self.german, MAX_FIELD_CHARS);
            let german = differs || self.needle.german_differs;
            if let Some(q) = self.eval(None, german) {
                joint = Some(joint.map_or(q * weight, |j| j.max(q * weight)));
            }
            if k > 1 {
                for (t, best) in per_token.iter_mut().enumerate().take(k) {
                    if let Some(q) = self.eval(Some(t), german) {
                        *best = best.max(q * weight);
                    }
                }
            }
        }
        if k > 1 && per_token[..k].iter().all(|&q| q > 0.0) {
            let split = per_token[..k].iter().sum::<f32>() / k as f32 * SPLIT_FACTOR;
            joint = Some(joint.map_or(split, |j| j.max(split)));
        }
        joint
    }

    /// Güte des ganzen Needles (`token = None`) oder eines Wortes gegen den zuletzt gefalteten Text, bester der Formen.
    fn eval(&self, token: Option<usize>, german: bool) -> Option<f32> {
        let one = |form: &NeedleForm, h: &Folded| match token {
            None => tier_score(&form.joined, &form.tokens, h),
            Some(t) => tier_score(&form.tokens[t], slice::from_ref(&form.tokens[t]), h),
        };
        let plain = one(&self.needle.forms[0], &self.plain);
        if !german {
            return plain;
        }
        let german = one(&self.needle.forms[1], &self.german);
        match (plain, german) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(needle: &str, text: &str) -> Option<f32> {
        let n = Needle::new(needle)?;
        Matcher::new(&n).score(text)
    }

    fn qi(needle: &str, text: &str) -> f32 {
        q(needle, text).unwrap_or_else(|| panic!("{needle:?} should match {text:?}"))
    }

    #[test]
    fn folding_maps_diacritics_in_both_forms() {
        assert_eq!(fold_string("Größe", false), "grosse");
        assert_eq!(fold_string("Größe", true), "groesse");
        assert_eq!(fold_string("Änderung ÜBER Öl", false), "anderung uber ol");
        assert_eq!(fold_string("Änderung ÜBER Öl", true), "aenderung ueber oel");
        assert_eq!(fold_string("Café Ñandú Łódź", false), "cafe nandu lodz");
        assert_eq!(fold_string("Straße", false), "strasse");
        assert_eq!(fold_string("Straße", true), "strasse");
        // zerlegte Zeichen (a + Trema) falten wie das zusammengesetzte
        assert_eq!(fold_string("a\u{308}", false), "a");
        assert_eq!(fold_string("a\u{308}", true), "ae");
        assert_eq!(fold_string("e\u{301}", true), "e");
    }

    #[test]
    fn umlaut_spellings_find_each_other() {
        for needle in ["grosse", "größe", "groesse", "GROSSE", "Größe"] {
            assert!(q(needle, "Größe ändern").is_some(), "{needle:?} should find the German text");
        }
        for needle in ["aender", "änder", "ander"] {
            assert!(q(needle, "Änderungen").is_some(), "{needle:?} should find Änderungen");
        }
        // die deutsche Form des Textes: ASCII-Schreibweise im Text, Umlaut in der Eingabe
        assert!(q("größe", "Groesse").is_some());
        assert!(q("grüße", "Gruesse").is_some());
        assert!(q("straße", "Strasse").is_some());
        assert!(q("strasse", "Straße").is_some());
        assert!(q("öl", "Ol").is_some(), "ö darf auch o sein");
        // ein Umlaut ist kein beliebiges Zeichen
        assert!(q("gxoße", "Größe").is_none());
    }

    #[test]
    fn case_is_ignored() {
        assert_eq!(q("FIREFOX", "firefox"), Some(Q_EXACT));
        assert_eq!(q("firefox", "FireFox"), Some(Q_EXACT));
    }

    #[test]
    fn tiers_are_ordered_exact_prefix_word_fuzzy() {
        let exact = qi("fire", "Fire");
        let prefix = qi("fire", "Firefox");
        let word = qi("code", "Visual Studio Code");
        let fuzzy = qi("vsc", "Visual Studio Code");
        assert_eq!(exact, Q_EXACT);
        assert!((Q_PREFIX..Q_EXACT).contains(&prefix), "{prefix}");
        assert!((Q_WORD..Q_WORD + Q_WORD_SPAN + 0.01).contains(&word), "{word}");
        assert!(fuzzy <= Q_FUZZY && fuzzy > 0.0, "{fuzzy}");
        assert!(exact > prefix && prefix > word && word > fuzzy);
    }

    #[test]
    fn shorter_fields_win_among_prefix_matches() {
        assert!(qi("fire", "Fire Alarm") > qi("fire", "Fire Alarm Control Center"));
    }

    #[test]
    fn initials_and_camel_case_match() {
        assert!(q("ff", "Firefox").is_some());
        assert!(q("vsc", "Visual Studio Code").is_some());
        // CamelCase-Übergang zählt wie ein Wortanfang
        assert!(qi("dr", "DataRoom") > qi("dr", "Android"));
        assert!(qi("dr", "DataRoom") > qi("dr", "Dandruff"));
        // Wortanfänge vor Mitte des Wortes
        assert!(qi("sc", "Screen Capture") > qi("sc", "Disc Scanner Tool 2") - 1.0);
        assert!(qi("ff", "Fast Fetch") > qi("ff", "Kaffee"));
    }

    #[test]
    fn contiguous_runs_beat_scattered_matches() {
        assert!(qi("fox", "unfoxed") > qi("fox", "unfaoxed"));
        assert!(qi("fox", "unfaoxed") > qi("fox", "unfaaoaaxed"));
    }

    #[test]
    fn non_subsequences_do_not_match() {
        assert!(q("xyz", "Firefox").is_none());
        assert!(q("firefoxx", "Firefox").is_none());
        assert!(q("asdkjhqwe", "Visual Studio Code").is_none());
    }

    #[test]
    fn loose_scatter_is_rejected() {
        // die Zeichen liegen als Teilfolge im Feld, aber durch lange Lücken getrennt
        let text = format!("a{}b{}c", "x".repeat(60), "x".repeat(60));
        assert!(q("abc", &text).is_none());
        assert!(q("abc", "a1b2c").is_some());
    }

    #[test]
    fn a_single_character_matches_only_at_word_starts() {
        assert!(q("f", "Firefox").is_some());
        assert!(q("x", "Firefox").is_none());
        assert!(q("c", "Visual Studio Code").is_some());
        assert!(q("u", "Visual Studio Code").is_none());
    }

    #[test]
    fn several_words_must_all_match() {
        assert!(q("bild sperr", "Bildschirm sperren").is_some());
        assert!(q("sperr bild", "Bildschirm sperren").is_some(), "Reihenfolge egal");
        assert!(q("lock sc", "Lock Screen").unwrap() > Q_PREFIX);
        assert!(q("lock xyz", "Lock Screen").is_none());
        assert!(q("claude code", "claude-code").is_some(), "Wörter über Trennzeichen");
    }

    #[test]
    fn words_may_come_from_different_fields_of_one_item() {
        let n = Needle::new("customers acme").unwrap();
        let mut m = Matcher::new(&n);
        let split = m.item([("acme-site", 1.0), ("customers", 0.5)]).expect("one word per field");
        assert!(split > 0.0);
        assert!(m.item([("acme-site", 1.0), ("internal", 0.5)]).is_none(), "ein Wort fehlt überall");
        assert!(m.item([("acme customers", 1.0)]).is_some());
    }

    #[test]
    fn field_weights_scale_the_quality() {
        let n = Needle::new("firefox").unwrap();
        let mut m = Matcher::new(&n);
        assert_eq!(m.item([("Firefox", 1.0)]), Some(100.0));
        assert_eq!(m.item([("Firefox", 0.5)]), Some(50.0));
        assert_eq!(m.item([("Chrome", 1.0), ("Firefox", 0.75)]), Some(75.0));
        assert_eq!(m.item([("", 1.0)]), None);
    }

    #[test]
    fn needle_rejects_sentences_and_empty_input() {
        assert!(Needle::new("").is_none());
        assert!(Needle::new("   \t ").is_none());
        assert!(Needle::new("a b c d e f g h").is_some());
        assert!(Needle::new("a b c d e f g h i").is_none(), "neun Wörter sind ein Satz");
        assert!(Needle::new(&"x".repeat(MAX_NEEDLE_CHARS)).is_some());
        assert!(Needle::new(&"x".repeat(MAX_NEEDLE_CHARS + 1)).is_none());
        let n = Needle::new("  Fire   Fox ").unwrap();
        assert_eq!(n.tokens(), 2);
    }

    #[test]
    fn long_fields_are_cut_not_rejected() {
        let long = format!("firefox {}", "x".repeat(2000));
        assert_eq!(q("firefox", &long).map(|v| v > Q_PREFIX), Some(true));
        // hinter der Grenze wird nichts mehr gefunden
        let hidden = format!("{} needle", "x".repeat(MAX_FIELD_CHARS + 10));
        assert!(q("needle", &hidden).is_none());
    }

    #[test]
    fn folding_respects_the_length_limit_even_when_characters_expand() {
        let text = "ß".repeat(MAX_FIELD_CHARS * 2);
        assert!(q("ss", &text).is_some());
        assert!(q("sssss", &format!("{}x", "ß".repeat(MAX_FIELD_CHARS))).is_some());
        let (mut plain, mut german) = (Folded::default(), Folded::default());
        fold_text(&text, &mut plain, &mut german, MAX_FIELD_CHARS);
        assert_eq!((plain.len(), german.len()), (MAX_FIELD_CHARS, MAX_FIELD_CHARS));
        assert_eq!(plain.bonus.len(), MAX_FIELD_CHARS);
    }

    #[test]
    fn dots_and_digits_are_literal() {
        assert_eq!(q("github.com", "github.com"), Some(Q_EXACT));
        assert!(q("3", "Tag 3").is_some());
        assert!(q("py3", "python3").is_some());
    }
}
