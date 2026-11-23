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

//! Sprachbefehle (Deutsch und Englisch).
//!
//! Der Rohtext wird in Wörter zerlegt; an jeder Stelle wird die längste passende Befehlsphrase gesucht
//! (Groß/klein und Satzzeichen egal). Whisper schreibt gesprochene Satzzeichen oft schon selbst, manchmal
//! zusätzlich zum Befehlswort („Hallo, Komma wie“, „geht's? Fragezeichen,“), manchmal mit dem falschen Zeichen
//! („Grüße. Komma“). Deshalb gilt:
//! - Satzzeichen, die Whisper an ein Befehlswort hängt, fallen weg („neue Zeile,“, „Fragezeichen,“): Whisper setzt
//!   um eingeschobene Befehle Pausenkommas, die nicht zum Text gehören.
//! - Steht am Ende schon ein Satzzeichen von Whisper, ersetzt ein Satzzeichen-Befehl es (gleiches Zeichen = nichts
//!   doppelt).
//! - Ein Satzzeichen von Whisper *vor* einem Umbruchbefehl bleibt immer („Grüße, neue Zeile“ → `Grüße,` + Umbruch).
//!   Whisper macht aus dem gesprochenen „Komma“ selbst ein „,“ (6 von 6 Läufen von t11 im Sprachtest: „Liebe Grüße,
//!   neue Zeile, Daniel.“), und im Text ist dieses Komma von einem Pausenkomma nicht zu unterscheiden. Ein Umbruch hat
//!   kein Satzzeichen, das er ersetzt; das Zeichen davor ist Inhalt (Punkt, Fragezeichen u. a. blieben schon immer).
//!   Nimmt Whisper doch ein Pausenkomma, steht es am Zeilenende, ein Tastendruck zum Löschen. Das umgekehrte Löschen
//!   eines gesprochenen Kommas wäre ein stiller Verlust.
//! - Ein Satzzeichen, das aus einem Befehl stammt, wird mit [`KEEP`] markiert. So ersetzt ein zweiter Befehl es
//!   nicht („Fragezeichen Ausrufezeichen“ → `?!`), und `punctuation = "strip"` im Profil lässt es stehen.
//! - Nach `.`, `?`, `!` und Zeilenumbrüchen aus Befehlen beginnt das nächste Wort groß.
//!
//! Beobachtet mit large-v3-turbo (22.10.2026, Testsätze unter `/home/data/stt-test/v2c/`): „Testpunkt“ und
//! „Satzausrufezeichen“ (Befehl an das Wort davor geklebt), „Neuzeile“, „löscht das letzte Wort“, Kommas und Punkte
//! rund um Befehlswörter („Question mark, new paragraph, regards.“).

use super::KEEP;

/// Was ein Befehl bewirkt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Satzzeichen, an das vorige Wort angehängt
    Punct(char),
    /// Bindestrich ohne Leerzeichen auf beiden Seiten
    Hyphen,
    /// Zeilenumbrüche
    Break(&'static str),
    DeleteWord,
    ScratchAll,
    /// Nur als einziges Diktat: das vorige Diktat per BackSpace löschen
    Undo,
    /// Das nächste Wort (oder die nächste Befehlsphrase) wörtlich tippen
    Literal,
}

pub struct Command {
    /// Deutsche Phrasen; die erste wird im GUI angezeigt
    pub de: &'static [&'static str],
    pub en: &'static [&'static str],
    /// Sichtbarer Text (Englisch)
    pub effect: &'static str,
    pub act: Act,
}

/// Befehlstabelle; die erste Phrase je Sprache erscheint im GUI.
pub const COMMANDS: &[Command] = &[
    Command {
        de: &["neue Zeile", "neuer Zeile", "nächste Zeile", "Neuzeile", "Zeilenumbruch"],
        en: &["new line", "newline", "next line", "line break"],
        effect: "Line break",
        act: Act::Break("\n"),
    },
    Command {
        de: &["neuer Absatz", "neuen Absatz", "neue Absatz", "nächster Absatz"],
        en: &["new paragraph", "next paragraph"],
        effect: "Blank line",
        act: Act::Break("\n\n"),
    },
    Command { de: &["Komma"], en: &["comma"], effect: "Types ,", act: Act::Punct(',') },
    Command { de: &["Punkt"], en: &["period", "full stop"], effect: "Types .", act: Act::Punct('.') },
    Command { de: &["Fragezeichen"], en: &["question mark"], effect: "Types ?", act: Act::Punct('?') },
    Command { de: &["Ausrufezeichen", "Ausrufzeichen", "Rufzeichen"], en: &["exclamation mark", "exclamation point"], effect: "Types !", act: Act::Punct('!') },
    Command { de: &["Doppelpunkt"], en: &["colon"], effect: "Types :", act: Act::Punct(':') },
    Command { de: &["Semikolon", "Strichpunkt"], en: &["semicolon"], effect: "Types ;", act: Act::Punct(';') },
    Command { de: &["Bindestrich"], en: &["hyphen", "dash"], effect: "Types - (no spaces)", act: Act::Hyphen },
    Command {
        de: &["lösch das letzte Wort", "lösche das letzte Wort", "löscht das letzte Wort", "letztes Wort löschen"],
        en: &["delete last word", "delete the last word"],
        effect: "Removes the last word of this dictation",
        act: Act::DeleteWord,
    },
    Command {
        de: &["alles löschen", "lösch alles", "lösche alles"],
        en: &["scratch that", "delete everything"],
        effect: "Discards this dictation so far",
        act: Act::ScratchAll,
    },
    Command {
        de: &["rückgängig", "rückgängig machen", "mach das rückgängig"],
        en: &["undo that", "undo"],
        effect: "Deletes the previous dictation (say it alone)",
        act: Act::Undo,
    },
    Command { de: &["wörtlich"], en: &["literally"], effect: "Types the next word as it is, e.g. \"literally comma\"", act: Act::Literal },
];

/// Glieder an das Wort davor geklebter Befehle, die Whisper so schreibt („Satzausrufezeichen“). Nur lange,
/// eindeutige Wörter; „…punkt“ (Standpunkt, Zeitpunkt) und „…komma“ bleiben bewusst außen vor.
const GLUED: &[(&str, char)] = &[("ausrufezeichen", '!'), ("fragezeichen", '?')];

/// Vor diesen Wörtern ist ein einzelnes Satzzeichen-Befehlswort ein Substantiv („der Punkt“, „ein Komma“).
const ARTICLES: &[&str] = &["der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "kein", "keinen", "the", "a", "an"];

/// Ergebnis der Befehlsstufe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Text (mit [`KEEP`]-Markierungen)
    Text(String),
    /// Das Diktat war nur „rückgängig“
    Undo,
}

/// Satzzeichen, die Whisper selbst setzt und ein Befehl ersetzen darf.
fn is_mark(c: char) -> bool {
    matches!(c, ',' | '.' | ';' | ':' | '!' | '?')
}

/// Ein Wort des Rohtexts: Originaltext und normalisierte Form (klein, ohne Satzzeichen am Rand).
struct Tok<'a> {
    orig: &'a str,
    norm: String,
    /// Whisper hat ein Satzzeichen ans Wort gehängt („falsch,“)
    trailing_mark: bool,
}

fn edge_trim(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric())
}

fn tokens(s: &str) -> Vec<Tok<'_>> {
    s.split_whitespace().map(|w| Tok { orig: w, norm: edge_trim(w).to_lowercase(), trailing_mark: w.chars().last().is_some_and(is_mark) }).collect()
}

/// Phrasen aller Befehle, normalisiert und in Wörter zerlegt, längste zuerst.
fn phrases() -> Vec<(Vec<String>, &'static Command)> {
    let mut v: Vec<(Vec<String>, &'static Command)> = COMMANDS
        .iter()
        .flat_map(|c| c.de.iter().chain(c.en.iter()).map(move |p| (p.to_lowercase().split_whitespace().map(String::from).collect(), c)))
        .collect();
    v.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
    v
}

/// Längste Befehlsphrase ab `i`: (Befehl, Anzahl Wörter).
fn match_at(toks: &[Tok], i: usize, table: &[(Vec<String>, &'static Command)]) -> Option<(&'static Command, usize)> {
    table.iter().find_map(|(words, cmd)| {
        let n = words.len();
        // Innerhalb der Phrase dürfen keine Satzzeichen stehen, außer am letzten Wort („neue Zeile.“ ja,
        // „neue. Zeile“ nein)
        (i + n <= toks.len() && toks[i..i + n].iter().zip(words).all(|(t, w)| t.norm == *w) && toks[i..i + n - 1].iter().all(|t| !t.trailing_mark))
            .then_some((*cmd, n))
    })
}

/// Baut den Ausgabetext auf.
#[derive(Default)]
struct Out {
    s: String,
    /// Nächstes Wort ohne Leerzeichen anhängen (nach Bindestrich)
    glue: bool,
    /// Nächstes Wort groß beginnen
    cap: bool,
}

impl Out {
    fn trim_spaces(&mut self) {
        let n = self.s.trim_end_matches(' ').len();
        self.s.truncate(n);
    }

    fn word(&mut self, w: &str) {
        if !self.s.is_empty() && !self.s.ends_with('\n') && !self.glue {
            self.s.push(' ');
        }
        if self.cap {
            self.s.push_str(&capitalize(w));
        } else {
            self.s.push_str(w);
        }
        self.glue = false;
        self.cap = false;
    }

    /// Letztes Zeichen ist ein Satzzeichen von Whisper (nicht aus einem Befehl)?
    fn ends_with_whisper_mark(&self) -> bool {
        let mut it = self.s.chars().rev();
        matches!((it.next(), it.next()), (Some(c), prev) if is_mark(c) && prev != Some(KEEP))
    }

    fn punct(&mut self, c: char) {
        self.trim_spaces();
        if self.ends_with_whisper_mark() {
            self.s.pop();
        }
        // Am Anfang eines Diktats gibt es kein Wort, an das das Zeichen gehört; es wird trotzdem getippt
        // (Fortsetzung des vorigen Diktats)
        self.s.push(KEEP);
        self.s.push(c);
        self.glue = false;
        self.cap = matches!(c, '.' | '?' | '!');
    }

    fn hyphen(&mut self) {
        self.trim_spaces();
        self.s.push(KEEP);
        self.s.push('-');
        self.glue = true;
        self.cap = false;
    }

    fn line_break(&mut self, br: &str) {
        self.trim_spaces();
        // Satzzeichen vor dem Umbruch bleiben, auch ein Komma von Whisper: es kann das umgewandelte gesprochene
        // „Komma“ sein (siehe Modulkopf); nur Leerzeichen am Zeilenende fallen weg
        self.s.push_str(br);
        self.glue = false;
        self.cap = true;
    }

    fn delete_word(&mut self) {
        let t = self.s.trim_end_matches(' ');
        let cut = if t.ends_with('\n') {
            // Am Ende steht ein Umbruch: der fällt weg
            t.trim_end_matches('\n').len()
        } else {
            t.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0)
        };
        self.s.truncate(cut);
        self.trim_spaces();
        self.glue = false;
        self.cap = false;
    }

    fn scratch(&mut self) {
        *self = Out::default();
    }
}

/// Erster Buchstabe groß.
pub fn capitalize(w: &str) -> String {
    let mut out = String::with_capacity(w.len());
    let mut done = false;
    for c in w.chars() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Wendet die Sprachbefehle auf den Rohtext an.
pub fn apply(raw: &str) -> Outcome {
    let toks = tokens(raw);
    let table = phrases();
    // „rückgängig“ zählt nur als einziges Diktat
    if let Some((cmd, n)) = match_at(&toks, 0, &table) {
        if cmd.act == Act::Undo && n == toks.len() {
            return Outcome::Undo;
        }
    }
    let mut out = Out::default();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let m = match_at(&toks, i, &table).filter(|(cmd, n)| {
            // „der Punkt“, „ein Komma“: Substantiv, kein Befehl
            let single_punct = *n == 1 && matches!(cmd.act, Act::Punct(_));
            let after_article = i > 0 && !toks[i - 1].trailing_mark && ARTICLES.contains(&toks[i - 1].norm.as_str());
            !(single_punct && after_article) && cmd.act != Act::Undo
        });
        match m {
            Some((cmd, n)) => {
                i += n;
                match cmd.act {
                    Act::Punct(c) => out.punct(c),
                    Act::Hyphen => out.hyphen(),
                    Act::Break(b) => out.line_break(b),
                    Act::DeleteWord => out.delete_word(),
                    Act::ScratchAll => out.scratch(),
                    Act::Undo => unreachable!("gefiltert"),
                    Act::Literal => {
                        // Folgt eine Befehlsphrase, wird sie ganz wörtlich übernommen, sonst ein Wort
                        let k = match_at(&toks, i, &table).map(|(_, k)| k).unwrap_or(1);
                        for w in toks.iter().skip(i).take(k) {
                            out.word(w.orig);
                        }
                        i += k;
                    }
                }
            }
            None => {
                i += 1;
                if let Some((prefix, c)) = split_glued(t) {
                    out.word(prefix);
                    out.punct(c);
                } else {
                    out.word(t.orig);
                }
            }
        }
    }
    // Umbrüche am Rand bleiben (ein Diktat darf mit „neue Zeile“ beginnen oder enden)
    Outcome::Text(out.s.trim_matches(' ').to_string())
}

/// „Satzausrufezeichen.“ → („Satz“, '!'); Satzzeichen von Whisper hinter dem Glied fallen weg.
fn split_glued<'a>(t: &Tok<'a>) -> Option<(&'a str, char)> {
    let core = t.orig.trim_end_matches(|c: char| !c.is_alphanumeric());
    GLUED.iter().find_map(|(suffix, c)| {
        let n = suffix.chars().count();
        let total = core.chars().count();
        if total < n + 2 {
            return None;
        }
        let split = core.char_indices().nth(total - n)?.0;
        (core[split..].to_lowercase() == *suffix).then_some((&core[..split], *c))
    })
}

/// Befehlstabelle für `{"cmd":"commands"}` und das GUI.
pub fn table_json() -> serde_json::Value {
    COMMANDS
        .iter()
        .map(|c| {
            let (de, en) =
                if c.act == Act::Literal { (format!("{} …", c.de[0]), format!("{} …", c.en[0])) } else { (c.de[0].to_string(), c.en[0].to_string()) };
            serde_json::json!({"de": de, "en": en, "effect": c.effect})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ergebnis ohne Markierungen
    fn run(s: &str) -> String {
        match apply(s) {
            Outcome::Text(t) => t.replace(KEEP, ""),
            Outcome::Undo => "<undo>".into(),
        }
    }

    #[test]
    fn acceptance_sentence() {
        assert_eq!(run("Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß"), "Hallo, wie geht's?\n\nGruß");
        assert_eq!(run("Hello comma how are you question mark new paragraph regards"), "Hello, how are you?\n\nRegards");
    }

    #[test]
    fn real_whisper_outputs() {
        // Ausgaben von large-v3-turbo-q5_0 auf piper-Testsätzen (siehe Modulkommentar)
        let cases = [
            // Whisper hat das Komma schon gesetzt und zusätzlich „Fragezeichen,“ geschrieben
            ("Hallo, wie geht's? Fragezeichen, neuer Absatz Gruß.", "Hallo, wie geht's?\n\nGruß."),
            // Komma doppelt: Zeichen plus Wort
            ("Liebe Grüße, Komma Daniel.", "Liebe Grüße, Daniel."),
            // falsches Zeichen von Whisper vor dem Befehl
            ("Liebe Grüße. Komma Daniel.", "Liebe Grüße, Daniel."),
            ("Wir treffen uns um drei Doppelpunkt, danach essen wir.", "Wir treffen uns um drei: danach essen wir."),
            ("Das ist ein Test. Punkt. Neue Zeile und noch ein Satz.", "Das ist ein Test.\nUnd noch ein Satz."),
            ("Das ist ein Test, Punkt. Neue Zeile. Noch ein Satz, Ausrufezeichen.", "Das ist ein Test.\nNoch ein Satz!"),
            ("Das ist ein Testpunkt Neue Zeile und noch ein Satzausrufezeichen.", "Das ist ein Testpunkt\nUnd noch ein Satz!"),
            ("Das ist falsch, löscht das letzte Wort.", "Das ist"),
            ("Wörtlich Komma", "Komma"),
            ("Bitte schreib wörtlich Komma und dann weiter.", "Bitte schreib Komma und dann weiter."),
            ("Erste Zeile Neuzeile zweite Zeile Punkt", "Erste Zeile\nZweite Zeile."),
            ("How, how are you? Question mark, new paragraph, regards.", "How, how are you?\n\nRegards."),
            ("This is a test, period new line, another sentence, exclamation mark.", "This is a test.\nAnother sentence!"),
            ("This is wrong. Delete last word.", "This is"),
            ("Literally, comma.", "comma."),
            ("Ich schreibe ein E-Mail-Programm.", "Ich schreibe ein E-Mail-Programm."),
            ("Das E-Mail Bindestrich Programm", "Das E-Mail-Programm"),
        ];
        for (input, want) in cases {
            assert_eq!(run(input), want, "input {input:?}");
        }
    }

    #[test]
    fn undo_only_alone() {
        for s in ["rückgängig", "Rückgängig.", "Undo that.", "mach das rückgängig", "undo"] {
            assert_eq!(apply(s), Outcome::Undo, "{s}");
        }
        // mitten im Diktat ist es ein normales Wort
        assert_eq!(run("Das mache ich rückgängig"), "Das mache ich rückgängig");
    }

    #[test]
    fn scratch_and_delete() {
        assert_eq!(run("Das ist Müll, alles löschen. Neuer Text."), "Neuer Text.");
        assert_eq!(run("Scratch that."), "");
        assert_eq!(run("eins zwei drei delete last word delete last word"), "eins");
        assert_eq!(run("lösch das letzte Wort"), "");
        assert_eq!(run("Zeile neue Zeile löschen lösch das letzte Wort"), "Zeile\n");
    }

    #[test]
    fn no_doubles_and_combinations() {
        assert_eq!(run("Was, Komma"), "Was,");
        assert_eq!(run("Wirklich Fragezeichen Ausrufezeichen"), "Wirklich?!");
        assert_eq!(run("Wirklich? Fragezeichen"), "Wirklich?");
        assert_eq!(run("Liebe Grüße Komma neue Zeile Daniel"), "Liebe Grüße,\nDaniel");
    }

    #[test]
    fn whisper_mark_before_line_break_stays() {
        // Satz t11 des Sprachtests: gesprochen „Liebe Grüße Komma neue Zeile Daniel Punkt“. Whisper macht aus „Komma“
        // selbst ein „,“ und setzt oft ein Pausenkomma hinter den Befehl; das Komma vor dem Umbruch muss bleiben
        let cases = [
            // gesprochenes Wort
            ("Liebe Grüße Komma neue Zeile Daniel Punkt", "Liebe Grüße,\nDaniel."),
            // Whisper hat „Komma“ schon als Zeichen geschrieben, mit und ohne Pausenkomma hinter dem Befehl
            ("Liebe Grüße, neue Zeile, Daniel.", "Liebe Grüße,\nDaniel."),
            ("Liebe Grüße, neue Zeile Daniel", "Liebe Grüße,\nDaniel"),
            ("Liebe Grüße, Neue Zeile. Daniel.", "Liebe Grüße,\nDaniel."),
            // Zeichen und Wort zugleich
            ("Liebe Grüße, Komma neue Zeile Daniel", "Liebe Grüße,\nDaniel"),
            ("Liebe Grüße, Komma, neue Zeile, Daniel.", "Liebe Grüße,\nDaniel."),
            ("Liebe Grüße Komma, neue Zeile, Daniel.", "Liebe Grüße,\nDaniel."),
            // falsches Zeichen von Whisper, das Befehlswort gewinnt
            ("Liebe Grüße. Komma neue Zeile Daniel", "Liebe Grüße,\nDaniel"),
            // ohne jedes Satzzeichen kommt auch keins hinzu
            ("Liebe Grüße neue Zeile Daniel", "Liebe Grüße\nDaniel"),
            ("Liebe Grüße neue Zeile, Daniel", "Liebe Grüße\nDaniel"),
            // Punkt, Fragezeichen, Ausrufezeichen vor dem Umbruch (Wort oder Zeichen)
            ("Das ist ein Test Punkt neue Zeile Noch ein Satz", "Das ist ein Test.\nNoch ein Satz"),
            ("Das ist ein Test. Neue Zeile. Noch ein Satz.", "Das ist ein Test.\nNoch ein Satz."),
            ("Das ist ein Test. Punkt. Neue Zeile. Noch ein Satz.", "Das ist ein Test.\nNoch ein Satz."),
            ("Das ist ein Test, Punkt, neue Zeile, noch ein Satz.", "Das ist ein Test.\nNoch ein Satz."),
            ("Stimmt das? Neue Zeile. Ja!", "Stimmt das?\nJa!"),
            ("Stimmt es Fragezeichen neue Zeile Ja Ausrufezeichen", "Stimmt es?\nJa!"),
            // mehrere Umbrüche, Komma nur dort, wo Whisper eins gesetzt hat
            ("Milch, neue Zeile, Brot neue Zeile Eier, neue Zeile", "Milch,\nBrot\nEier,\n"),
        ];
        for (input, want) in cases {
            assert_eq!(run(input), want, "input {input:?}");
        }
    }

    #[test]
    fn whisper_mark_before_paragraph_stays() {
        // Satz t12 (Rohtexte aus drei Läufen) und Verwandte
        let cases = [
            ("Ist das schon alles? Fragezeichen. Neuer Absatz. Ja, Ausrufezeichen.", "Ist das schon alles?\n\nJa!"),
            ("Ist das schon alles? Neuer Absatz. Ja, Ausrufezeichen.", "Ist das schon alles?\n\nJa!"),
            ("Ist das schon alles Fragezeichen neuer Absatz Ja Ausrufezeichen", "Ist das schon alles?\n\nJa!"),
            ("Ist das schon alles? Neuer Absatz Ja!", "Ist das schon alles?\n\nJa!"),
            ("Sehr geehrte Damen und Herren, neuer Absatz, ich schreibe Ihnen.", "Sehr geehrte Damen und Herren,\n\nIch schreibe Ihnen."),
            ("Sehr geehrte Damen und Herren Komma neuer Absatz ich schreibe Ihnen Punkt", "Sehr geehrte Damen und Herren,\n\nIch schreibe Ihnen."),
            ("Das war es schon. Neuer Absatz.", "Das war es schon.\n\n"),
        ];
        for (input, want) in cases {
            assert_eq!(run(input), want, "input {input:?}");
        }
    }

    #[test]
    fn whisper_mark_before_break_english() {
        let cases = [
            ("Kind regards comma new line John period", "Kind regards,\nJohn."),
            ("Kind regards, new line, John.", "Kind regards,\nJohn."),
            ("Kind regards, comma, new line, John.", "Kind regards,\nJohn."),
            ("Kind regards new line John", "Kind regards\nJohn"),
            ("This is a test, period new line, another sentence, exclamation mark.", "This is a test.\nAnother sentence!"),
            ("Is that all? Question mark, new paragraph, yes, exclamation mark.", "Is that all?\n\nYes!"),
            ("Is that all? New paragraph. Yes!", "Is that all?\n\nYes!"),
            ("Dear John, next paragraph, thank you.", "Dear John,\n\nThank you."),
            ("Dear John, line break, thanks.", "Dear John,\nThanks."),
        ];
        for (input, want) in cases {
            assert_eq!(run(input), want, "input {input:?}");
        }
    }

    #[test]
    fn spoken_comma_survives_markers() {
        // Das Komma aus dem Befehl trägt die Markierung (bleibt im Profil „strip“); das von Whisper nicht
        let Outcome::Text(spoken) = apply("Liebe Grüße Komma neue Zeile Daniel") else { panic!("Text erwartet") };
        assert_eq!(spoken, format!("Liebe Grüße{KEEP},\nDaniel"));
        let Outcome::Text(whisper) = apply("Liebe Grüße, neue Zeile, Daniel.") else { panic!("Text erwartet") };
        assert_eq!(whisper, "Liebe Grüße,\nDaniel.");
    }

    #[test]
    fn nouns_after_articles_stay_words() {
        assert_eq!(run("Der Punkt ist wichtig"), "Der Punkt ist wichtig");
        assert_eq!(run("Hier fehlt ein Komma"), "Hier fehlt ein Komma");
        assert_eq!(run("Ende des Satzes Punkt"), "Ende des Satzes.");
    }

    #[test]
    fn umlauts_and_case() {
        assert_eq!(run("Ich mag Äpfel KOMMA Birnen und Öl PUNKT"), "Ich mag Äpfel, Birnen und Öl.");
        assert_eq!(run("satz punkt ärger"), "satz. Ärger");
    }

    #[test]
    fn literal_takes_whole_phrase() {
        assert_eq!(run("schreib wörtlich neue Zeile bitte"), "schreib neue Zeile bitte");
        assert_eq!(run("wörtlich wörtlich"), "wörtlich");
    }

    #[test]
    fn plain_text_unchanged() {
        for s in ["", "Hallo Welt.", "Version 1.2, Build 7", "Er sagte: „Ja.“"] {
            assert_eq!(run(s), s);
        }
    }

    #[test]
    fn gui_fallback_table_in_sync() {
        // Die Seite zeigt ohne Daemon eine eigene Kopie (das Settings-Binary linkt die Pipeline nicht)
        let src = include_str!("../gui/pages/commands.rs");
        for e in table_json().as_array().unwrap() {
            let f = |k: &str| e[k].as_str().unwrap().replace('"', "\\\"");
            let row = format!("(\"{}\", \"{}\", \"{}\")", f("de"), f("en"), f("effect"));
            assert!(src.contains(&row), "missing in src/gui/pages/commands.rs: {row}");
        }
    }

    #[test]
    fn table_has_both_languages() {
        let t = table_json();
        let a = t.as_array().unwrap();
        assert_eq!(a.len(), COMMANDS.len());
        assert_eq!(a[0], serde_json::json!({"de": "neue Zeile", "en": "new line", "effect": "Line break"}));
        assert!(a.iter().all(|e| !e["de"].as_str().unwrap().is_empty() && !e["en"].as_str().unwrap().is_empty()));
    }
}
