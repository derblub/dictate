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

//! Textaktionen auf der Markierung: Katalog der Aktionen mit Taste, Ausgabeart und fertigem System-Prompt.
//!
//! Eingebaute Aktionen (Prompts in `actions.prompts` editierbar), danach bei `actions.include_ai_modes` die KI-Modi
//! aus `ai.modes` auf den Tasten 1..9 (ohne den Modus, den eine eingebaute Aktion unverändert abdeckt, siehe
//! [`Builtin::mode`]). Vor jedem Prompt steht derselbe feste, nicht editierbare Schutzrahmen wie bei
//! den KI-Modi ([`crate::config::INPUT_GUARD`]); Antwort-Aktionen bekommen am Ende die Antwortsprache.

pub mod diff;
pub mod markdown;
pub mod secrets;
pub mod selection;
pub mod store;
pub mod wordlist;

use crate::config::{builtin_mode, refine_max_chars, refine_target_chars, with_guard, with_length_budget, Config, PROMPT_REFINE, REFINE_MODE};
use serde::Serialize;

/// Was mit dem Ergebnis geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// Anzeigen (kopierbar)
    Answer,
    /// Darf die Markierung ersetzen (außer in Terminals)
    Replace,
}

impl Output {
    pub fn as_str(self) -> &'static str {
        match self {
            Output::Answer => "answer",
            Output::Replace => "replace",
        }
    }
}

/// Eingebaute Aktion mit Vorgabe-Prompt.
pub struct Builtin {
    pub key: char,
    pub name: &'static str,
    pub label: &'static str,
    pub output: Output,
    pub prompt: &'static str,
    /// Die Antwort ist ein ausgearbeiteter, deutlich längerer Text ([`refine_max_chars`] statt etwa Eingabelänge)
    pub grows: bool,
    /// Eingebauter KI-Modus mit derselben Aufgabe und demselben Vorgabe-Prompt; solange der Modus unverändert ist,
    /// steht er nicht ein zweites Mal als `mode:<name>` im Menü
    pub mode: Option<&'static str>,
    /// Änderungen gegenüber der Markierung standardmäßig markieren ([`diff::words`]); nur bei [`Output::Replace`]
    /// sinnvoll. Aus bei Übersetzungen (jedes Wort ist neu) und bei Refine prompt (der Text wird neu geschrieben).
    pub marks: bool,
}

pub const BUILTIN: [Builtin; 6] = [
    Builtin {
        key: 'e',
        name: "explain",
        label: "Explain",
        output: Output::Answer,
        grows: false,
        mode: None,
        marks: false,
        prompt: "Explain the text clearly and concisely for a technically literate reader. Start with a one-sentence summary, then add only what is needed to understand it: key terms, context and implications. Use short paragraphs or a short list, no preamble.",
    },
    Builtin {
        key: 't',
        name: "translate",
        label: "Translate DE ↔ EN",
        output: Output::Replace,
        grows: false,
        mode: None,
        marks: false,
        prompt: "Translate the text: if it is German, into English; otherwise into German. Write idiomatic, grammatically flawless text as a native speaker would, and keep the meaning, tone and register; in German, choose \"du\" or \"Sie\" from the tone of the source and use it throughout. Keep names, product names, code, URLs, file names and numbers unchanged, and keep the line breaks. Output only the translation: no quotes, notes or explanations.",
    },
    Builtin {
        key: 'r',
        name: "rephrase",
        label: "Rephrase",
        output: Output::Replace,
        grows: false,
        mode: None,
        marks: true,
        prompt: "Rephrase the text so it reads clearer and more natural. Keep the meaning, language, tone and roughly the length; fix grammar, spelling and punctuation. Keep names, code, URLs and numbers unchanged, and keep the line breaks. Output only the rephrased text: no quotes, notes or explanations.",
    },
    Builtin {
        key: 'p',
        name: "refine_prompt",
        label: "Refine prompt",
        output: Output::Replace,
        grows: true,
        mode: Some(REFINE_MODE),
        marks: false,
        prompt: PROMPT_REFINE,
    },
    Builtin {
        key: 'a',
        name: "analyze_error",
        label: "Analyze error",
        output: Output::Answer,
        grows: false,
        mode: None,
        marks: false,
        prompt: "Analyze the error message or log output. Say briefly what went wrong, the most likely cause and how to fix it. To gather more information, suggest only read-only commands that inspect the system (status, logs, versions, listing files or settings); never suggest commands that change, delete, install or restart anything, describe such fixes in words instead. Be concise and use a short list.",
    },
    Builtin {
        key: 'c',
        name: "explain_code",
        label: "Explain code",
        output: Output::Answer,
        grows: false,
        mode: None,
        marks: false,
        prompt: "Explain what the code does, for a developer. Start with a one-sentence summary, then walk through the important parts in order and point out bugs, pitfalls and side effects. Quote identifiers exactly and do not rewrite the code. Be concise.",
    },
];

/// Höchstens so viele KI-Modi (Tasten 1..9).
pub const MAX_MODES: usize = 9;

/// Eine Aktion im Menü.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Action {
    pub key: char,
    /// Eingebaut: `explain` usw.; KI-Modus: `mode:<name>`
    pub name: String,
    pub label: String,
    pub output: Output,
    /// Name in `ai.modes`, wenn die Aktion ein KI-Modus ist (läuft über `rewrite_with`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Fertiger System-Prompt inklusive festem Zusatz
    #[serde(skip)]
    pub prompt: String,
    /// Siehe [`Builtin::grows`]; KI-Modi prüfen ihre Länge selbst (`pipeline::ai::rewrite_as`)
    #[serde(skip)]
    pub grows: bool,
    /// Siehe [`Builtin::marks`]; KI-Modi ja, außer den Übersetzungsmodi ([`TRANSLATION_MODES`]) und `prompt`
    #[serde(skip)]
    pub marks: bool,
}

impl Action {
    /// System-Prompt für eine Anfrage mit `chars` Zeichen Markierung: bei einer wachsenden Aktion ([`Builtin::grows`])
    /// mit dem festen Längenzusatz [`crate::config::length_budget_note`] am Ende (dasselbe Limit wie beim Modus
    /// `prompt` im Diktat, [`refine_target_chars`]), sonst [`Action::prompt`]. KI-Modi laufen über `rewrite_as` und
    /// bekommen ihn dort.
    pub fn prompt_for(&self, chars: usize) -> String {
        if self.grows {
            with_length_budget(&self.prompt, refine_target_chars(chars))
        } else {
            self.prompt.clone()
        }
    }
}

/// Eingebaute KI-Modi, die übersetzen: ihr Ergebnis wird nicht markiert (siehe [`Builtin::marks`])
pub const TRANSLATION_MODES: [&str; 2] = ["to_en", "to_de"];

/// Markiert ein KI-Modus seine Änderungen? Ja, außer beim Übersetzen und beim Modus `prompt` (wie Refine prompt:
/// der Text wird neu geschrieben), auch wenn sein Prompt geändert ist.
fn mode_marks(name: &str) -> bool {
    !TRANSLATION_MODES.contains(&name) && name != REFINE_MODE
}

/// Lesbarer Name eines KI-Modus im Menü: eingebaute Modi fest, eigene aus dem Namen (`my_mode` → „My mode“:
/// `_` und `-` als Leerzeichen, mehrfache zusammengefasst, erster Buchstabe groß).
pub fn mode_label(name: &str) -> String {
    let fixed = match name {
        "clean" => Some("Clean up"),
        "email" => Some("Write as email"),
        "to_en" => Some("To English"),
        "to_de" => Some("To German"),
        REFINE_MODE => Some("Refine prompt"),
        _ => None,
    };
    if let Some(l) = fixed {
        return l.into();
    }
    let words: Vec<&str> = name.split(|c: char| c == '_' || c == '-' || c.is_whitespace()).filter(|w| !w.is_empty()).collect();
    let joined = words.join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name.trim().to_string(),
    }
}

/// Antwortsprache als Code: `actions.language`, leer = globales `language`.
pub fn answer_language(cfg: &Config) -> &str {
    if cfg.actions.language.is_empty() {
        &cfg.language
    } else {
        &cfg.actions.language
    }
}

/// Englischer Name zum Sprachcode (für den Prompt); unbekannte Codes bleiben als Code stehen.
fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "de" => "German",
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "pl" => "Polish",
        "cs" => "Czech",
        "sk" => "Slovak",
        "hu" => "Hungarian",
        "ro" => "Romanian",
        "hr" => "Croatian",
        "sl" => "Slovenian",
        "sr" => "Serbian",
        "bs" => "Bosnian",
        "tr" => "Turkish",
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "el" => "Greek",
        "sv" => "Swedish",
        "da" => "Danish",
        "no" => "Norwegian",
        "fi" => "Finnish",
        "ar" => "Arabic",
        "fa" => "Persian",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        _ => return None,
    })
}

/// Satz für die Antwortsprache der Antwort-Aktionen.
fn language_line(code: &str) -> String {
    match (code, language_name(code)) {
        ("auto" | "", _) => "Answer in the language of the text, as plain text without Markdown.".into(),
        (_, Some(name)) => format!("Answer in {name}, as plain text without Markdown."),
        (c, None) => format!("Answer in the language with the code \"{c}\", as plain text without Markdown."),
    }
}

/// Prompt einer eingebauten Aktion: Schutzrahmen, Vorgabe oder eigener Text, Zusatz (Antwortsprache bzw. Anrede).
fn system_prompt(base: &str, extra: Option<&str>) -> String {
    match extra {
        Some(e) => with_guard(&format!("{}\n{e}", base.trim())),
        None => with_guard(base),
    }
}

/// Aktionen in Menüreihenfolge: die eingebauten, dann bei `include_ai_modes` die ersten neun KI-Modi (alphabetisch).
pub fn catalog(cfg: &Config) -> Vec<Action> {
    let lang = language_line(answer_language(cfg));
    let mut out: Vec<Action> = BUILTIN
        .iter()
        .map(|b| {
            let base = cfg.actions.prompts.get(b.name).filter(|p| !p.trim().is_empty()).map_or(b.prompt, String::as_str);
            let extra = match b.output {
                Output::Answer => Some(lang.clone()),
                // festgelegte Anrede wie beim Modus `to_de`
                Output::Replace if b.name == "translate" => {
                    crate::config::formality_line(&cfg.ai.formality).map(|l| format!("When you translate into German: {l}"))
                }
                Output::Replace => None,
            };
            Action {
                key: b.key,
                name: b.name.into(),
                label: b.label.into(),
                output: b.output,
                mode: None,
                prompt: system_prompt(base, extra.as_deref()),
                grows: b.grows,
                marks: b.marks,
            }
        })
        .collect();
    if cfg.actions.include_ai_modes {
        let modes = cfg.ai.modes.iter().filter(|(n, p)| !n.trim().is_empty() && !p.trim().is_empty() && !covered_mode(n, p));
        for ((name, prompt), key) in modes.take(MAX_MODES).zip('1'..='9') {
            out.push(Action {
                key,
                name: format!("mode:{name}"),
                // ein geänderter Modus, den sonst eine eingebaute Aktion abdeckt, steht neben ihr: unterscheidbar machen
                label: if BUILTIN.iter().any(|b| b.mode == Some(name.as_str())) { format!("{} (custom)", mode_label(name)) } else { mode_label(name) },
                output: Output::Replace,
                mode: Some(name.clone()),
                // derselbe Prompt wie beim Diktat (beim Modus `to_de` mit festgelegter Anrede), samt Schutzrahmen
                prompt: cfg.ai.mode_prompt(name).unwrap_or_else(|| with_guard(prompt)),
                grows: false,
                marks: mode_marks(name),
            });
        }
    }
    out
}

/// Steht der KI-Modus `name` mit dem Prompt `prompt` schon als eingebaute Aktion im Menü? Ja, wenn eine Aktion ihn
/// abdeckt ([`Builtin::mode`]) und sein Prompt wortgleich die Vorgabe ist; ein geänderter Modus ist eine eigene Aufgabe
/// und erscheint als `mode:<name>`. So bleiben die Tasten der übrigen Modi gleich und nichts steht doppelt im Menü.
pub fn covered_mode(name: &str, prompt: &str) -> bool {
    BUILTIN.iter().any(|b| b.mode == Some(name)) && builtin_mode(name).is_some_and(|p| p.trim() == prompt.trim())
}

/// Längengrenze der Antwort in Tokens für `pipeline::ai::ask` bei `chars` Zeichen Eingabe.
///
/// - Antwort-Aktionen (Erklären, Fehleranalyse): 4096 Tokens, rund 3000 Wörter. Länger als das ist keine Erklärung
///   mehr; was darüber hinausgeht, kommt als `truncated` zurück und das Panel sagt es.
/// - Ersetzende Aktionen (Übersetzen, Umformulieren): die Antwort ist etwa so lang wie die Eingabe. Gerechnet mit
///   2 Zeichen je Token (Deutsch und Code liegen bei 3 bis 4, Akzente und andere Schriften darunter) plus 256 Reserve,
///   mindestens 1024, höchstens 16384 (Ausgabegrenze älterer Modelle wie GPT-4o; `actions.max_chars` 30000 ergibt
///   15256). Eine an der Grenze abgeschnittene Ersetzung wird nie getippt (`truncated`).
/// - Wachsende Aktionen (`grows`, Refine prompt): wie Ersetzen, gerechnet mit der zulässigen Länge
///   [`refine_max_chars`] (harte Grenze: anderthalbfaches Ziel, also 4,5-fache Eingabe) statt der
///   Eingabelänge. Bis dahin zeigt das Panel das Ergebnis, über dem Ziel mit Hinweis (`daemon`, `note`).
pub fn max_tokens(output: Output, grows: bool, chars: usize) -> u64 {
    match output {
        Output::Answer => 4096,
        Output::Replace if grows => replace_tokens(refine_max_chars(chars)),
        Output::Replace => replace_tokens(chars),
    }
}

/// Tokens für eine Antwort von höchstens `chars` Zeichen: 2 Zeichen je Token plus 256 Reserve, 1024 bis 16384 (siehe
/// [`max_tokens`]). Auch für den Modus `prompt` beim Diktat (`pipeline::ai`).
pub fn replace_tokens(chars: usize) -> u64 {
    (chars.div_ceil(2) as u64 + 256).clamp(1024, 16_384)
}

/// Die eine Aktion einer Sitzung der Befehlspalette (PLAN-palette.md, Aufgabe 9); `None` für die Markierung (dort gilt
/// der ganze [`catalog`]).
/// - [`Source::Clipboard`] (`explain_clipboard`): die eingebaute Aktion `explain` mit Schutzrahmen, eigenem Prompt aus
///   `actions.prompts` und Antwortsprache, wie bei einer Markierung. Der Inhalt der Zwischenablage ist fremder Text.
/// - [`Source::Question`] (`ask`): Frage beantworten mit dem festen Rahmen [`crate::config::ASK_FRAME`] statt des
///   Schutzrahmens (Begründung dort).
pub fn source_action(cfg: &Config, source: store::Source) -> Option<Action> {
    match source {
        store::Source::Selection => None,
        store::Source::Clipboard => find(cfg, "explain"),
        store::Source::Question => Some(Action {
            key: 'a',
            name: ASK.into(),
            label: "Ask".into(),
            output: Output::Answer,
            mode: None,
            prompt: crate::config::ASK_FRAME.to_string(),
            grows: false,
            marks: false,
        }),
    }
}

/// Name der Aktion einer Frage aus der Palette ([`source_action`])
pub const ASK: &str = "ask";

/// Aktion zum Namen (`explain`, `mode:clean`).
pub fn find(cfg: &Config, name: &str) -> Option<Action> {
    catalog(cfg).into_iter().find(|a| a.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::INPUT_GUARD;
    use std::collections::HashSet;

    #[test]
    fn builtin_actions_come_first_with_unique_keys() {
        let c = Config::default();
        let cat = catalog(&c);
        let names: Vec<&str> = cat.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(&names[..6], ["explain", "translate", "rephrase", "refine_prompt", "analyze_error", "explain_code"]);
        let keys: HashSet<char> = cat.iter().map(|a| a.key).collect();
        assert_eq!(keys.len(), cat.len(), "keys must be unique");
        let names: HashSet<&str> = cat.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names.len(), cat.len(), "names must be unique");
        assert_eq!(find(&c, "translate").unwrap().output, Output::Replace);
        assert_eq!(find(&c, "explain").unwrap().output, Output::Answer);
        assert!(find(&c, "nope").is_none());
    }

    #[test]
    fn every_prompt_starts_with_the_fixed_guard() {
        let mut c = Config::default();
        c.actions.prompts.insert("explain".into(), "Say what it means.".into());
        c.ai.modes.insert("shout".into(), "Write the text in capitals.".into());
        let cat = catalog(&c);
        assert!(cat.iter().any(|a| a.name == "mode:shout"));
        for a in cat {
            assert!(a.prompt.starts_with(&format!("{INPUT_GUARD}\n")), "{}: {}", a.name, a.prompt);
            assert_eq!(a.prompt.matches(INPUT_GUARD).count(), 1, "{}: guard once", a.name);
        }
        let e = find(&c, "explain").unwrap();
        assert!(e.prompt.contains("\nSay what it means.\nAnswer in "), "{}", e.prompt);
        assert!(find(&c, "mode:shout").unwrap().prompt.ends_with("\nWrite the text in capitals."));
    }

    #[test]
    fn ai_modes_are_appended_and_can_be_switched_off() {
        let mut c = Config::default();
        let cat = catalog(&c);
        let modes: Vec<&Action> = cat.iter().filter(|a| a.mode.is_some()).collect();
        // `prompt` steht schon als Refine prompt im Menü
        assert_eq!(modes.len(), c.ai.modes.len() - 1);
        assert_eq!(modes[0].key, '1');
        assert_eq!(modes[0].name, "mode:clean");
        assert_eq!(modes[0].label, "Clean up");
        assert_eq!(modes[0].mode.as_deref(), Some("clean"));
        assert_eq!(modes[0].output, Output::Replace);
        assert!(modes[0].prompt.ends_with(&c.ai.modes["clean"]));
        c.actions.include_ai_modes = false;
        assert_eq!(catalog(&c).len(), BUILTIN.len());
    }

    #[test]
    fn only_growing_actions_get_the_length_budget_in_the_prompt() {
        let c = Config::default();
        let r = find(&c, "refine_prompt").unwrap();
        // dasselbe Limit wie beim Modus `prompt`: dreifache Markierung, hinter Schutzrahmen und Prompt
        assert_eq!(r.prompt_for(34), with_length_budget(&r.prompt, 102));
        assert!(r.prompt_for(34).starts_with(INPUT_GUARD) && r.prompt_for(34).ends_with("Do not mention the limit."));
        assert!(r.prompt_for(34).contains("at most 81 characters") && r.prompt_for(500).contains("at most 1200 characters"));
        assert_eq!(r.prompt, find(&c, "refine_prompt").unwrap().prompt, "the catalog prompt stays without a number");
        for a in catalog(&c).iter().filter(|a| !a.grows) {
            assert_eq!(a.prompt_for(34), a.prompt, "{}", a.name);
        }
    }

    #[test]
    fn refine_prompt_shares_the_prompt_of_the_mode_and_hides_it_while_unchanged() {
        let mut c = Config::default();
        let r = find(&c, "refine_prompt").unwrap();
        assert_eq!((r.key, r.label.as_str(), r.output, r.grows), ('p', "Refine prompt", Output::Replace, true));
        assert_eq!(r.prompt, with_guard(PROMPT_REFINE));
        assert_eq!(c.ai.modes[REFINE_MODE], PROMPT_REFINE);
        // die übrigen Modi behalten ihre Tasten
        let keys: Vec<(char, String)> = catalog(&c).into_iter().filter(|a| a.mode.is_some()).map(|a| (a.key, a.name)).collect();
        assert_eq!(keys, [('1', "mode:clean".into()), ('2', "mode:email".into()), ('3', "mode:to_de".into()), ('4', "mode:to_en".into())]);
        // ein geänderter Modus ist eine eigene Aufgabe
        c.ai.modes.insert(REFINE_MODE.into(), "Make the prompt shorter.".into());
        let m = find(&c, "mode:prompt").unwrap();
        assert!(m.prompt.ends_with("Make the prompt shorter."));
        assert_eq!((m.label.as_str(), m.marks), ("Refine prompt (custom)", false));
        assert!(!covered_mode("clean", crate::config::PROMPT_CLEAN) && covered_mode(REFINE_MODE, &format!(" {PROMPT_REFINE}\n")));
        // eigener Text für die Aktion
        c.actions.prompts.insert("refine_prompt".into(), "Polish the prompt.".into());
        assert!(find(&c, "refine_prompt").unwrap().prompt.ends_with("\nPolish the prompt."));
    }

    #[test]
    fn readable_mode_labels() {
        let labels: Vec<String> = crate::config::BUILTIN_MODES.iter().map(|(n, _)| mode_label(n)).collect();
        assert_eq!(labels, ["Clean up", "Write as email", "To English", "To German", "Refine prompt"]);
        assert_eq!(mode_label("my_mode"), "My mode");
        assert_eq!(mode_label("über-kurz__fassen"), "Über kurz fassen");
        assert_eq!(mode_label("Shout"), "Shout");
        assert_eq!(mode_label("_"), "_");
        let mut c = Config::default();
        c.ai.modes.insert("formal_reply".into(), "Make it formal.".into());
        assert_eq!(find(&c, "mode:formal_reply").unwrap().label, "Formal reply");
        assert_eq!(find(&c, "mode:to_de").unwrap().label, "To German");
    }

    #[test]
    fn marks_only_for_rewrites_that_keep_the_text() {
        let mut c = Config::default();
        c.ai.modes.insert("shout".into(), "Write the text in capitals.".into());
        let marks: Vec<(String, bool)> = catalog(&c).into_iter().map(|a| (a.name, a.marks)).collect();
        let want = [
            ("explain", false),
            ("translate", false),
            ("rephrase", true),
            ("refine_prompt", false),
            ("analyze_error", false),
            ("explain_code", false),
            ("mode:clean", true),
            ("mode:email", true),
            ("mode:shout", true),
            ("mode:to_de", false),
            ("mode:to_en", false),
        ];
        assert_eq!(marks, want.map(|(n, m)| (n.to_string(), m)));
        assert!(BUILTIN.iter().all(|b| !b.marks || b.output == Output::Replace));
    }

    #[test]
    fn at_most_nine_modes() {
        let mut c = Config::default();
        for i in 0..12 {
            c.ai.modes.insert(format!("m{i:02}"), "Do it.".into());
        }
        let cat = catalog(&c);
        let keys: Vec<char> = cat.iter().filter(|a| a.mode.is_some()).map(|a| a.key).collect();
        assert_eq!(keys, ('1'..='9').collect::<Vec<_>>());
        assert_eq!(cat.len(), BUILTIN.len() + MAX_MODES);
        assert!(!cat.iter().any(|a| a.name == "mode:prompt"));
    }

    #[test]
    fn answer_language_follows_settings() {
        let mut c = Config::default();
        assert!(find(&c, "explain").unwrap().prompt.contains("Answer in German"));
        assert!(find(&c, "analyze_error").unwrap().prompt.contains("Answer in German"));
        // Ersetzende Aktionen bleiben in der Sprache des Textes bzw. übersetzen DE↔EN
        assert!(!find(&c, "translate").unwrap().prompt.contains("Answer in"));
        assert!(!find(&c, "rephrase").unwrap().prompt.contains("Answer in"));
        assert!(!find(&c, "refine_prompt").unwrap().prompt.contains("Answer in"));
        c.actions.language = "en".into();
        assert!(find(&c, "explain_code").unwrap().prompt.contains("Answer in English"));
        c.actions.language.clear();
        c.language = "auto".into();
        assert!(find(&c, "explain").unwrap().prompt.contains("language of the text"));
        c.actions.language = "eo".into();
        assert!(find(&c, "explain").unwrap().prompt.contains("code \"eo\""));
    }

    #[test]
    fn formality_reaches_translate_and_to_de() {
        let mut c = Config::default();
        assert!(!find(&c, "translate").unwrap().prompt.contains("When you translate into German"));
        c.ai.formality = "formal".into();
        let line = crate::config::formality_line("formal").unwrap();
        assert!(find(&c, "translate").unwrap().prompt.contains(line));
        assert!(find(&c, "mode:to_de").unwrap().prompt.contains(line));
        assert!(!find(&c, "mode:to_en").unwrap().prompt.contains(line));
        assert!(!find(&c, "rephrase").unwrap().prompt.contains(line));
    }

    #[test]
    fn missing_prompt_falls_back_to_builtin() {
        let mut c = Config::default();
        c.actions.prompts.clear();
        let t = find(&c, "translate").unwrap();
        assert!(t.prompt.starts_with(&format!("{INPUT_GUARD}\n{}", BUILTIN[1].prompt)));
    }

    #[test]
    fn max_tokens_by_output() {
        assert_eq!(max_tokens(Output::Answer, false, 10), 4096);
        assert_eq!(max_tokens(Output::Answer, false, 30_000), 4096);
        assert_eq!(max_tokens(Output::Replace, false, 10), 1024);
        assert_eq!(max_tokens(Output::Replace, false, 8000), 4256);
        assert_eq!(max_tokens(Output::Replace, false, 30_000), 15_256);
        assert_eq!(max_tokens(Output::Replace, false, 1_000_000), 16_384);
        // Refine prompt: Platz bis zur harten Grenze (anderthalbfaches Ziel: 4,5-fach; technische Untergrenze 1024 Token)
        assert_eq!(max_tokens(Output::Replace, true, 100), 1024);
        assert_eq!(max_tokens(Output::Replace, true, 2000), 4756);
        assert_eq!(max_tokens(Output::Replace, true, 3000), 7006);
        assert_eq!(max_tokens(Output::Replace, true, 8000), 16_384);
    }

    #[test]
    fn palette_sources_have_one_action_each() {
        let mut c = Config::default();
        assert!(source_action(&c, store::Source::Selection).is_none());
        // Zwischenablage: fremder Text, also `explain` mit Schutzrahmen und Antwortsprache (auch der eigene Prompt)
        c.actions.prompts.insert("explain".into(), "Say what it means.".into());
        let e = source_action(&c, store::Source::Clipboard).unwrap();
        assert_eq!((e.name.as_str(), e.output), ("explain", Output::Answer));
        assert!(e.prompt.starts_with(&format!("{INPUT_GUARD}\n")) && e.prompt.contains("Say what it means.\nAnswer in German"), "{}", e.prompt);
        // Frage: eigener Rahmen statt Schutzrahmen (der sagt „never answer“), unabhängig von `actions.prompts`
        let a = source_action(&c, store::Source::Question).unwrap();
        assert_eq!((a.name.as_str(), a.label.as_str(), a.output, a.grows, a.mode.clone()), (ASK, "Ask", Output::Answer, false, None));
        assert_eq!(a.prompt, crate::config::ASK_FRAME);
        assert!(!a.prompt.contains(INPUT_GUARD) && !a.prompt.contains("Never answer"));
        assert!(a.prompt.contains("answering it is your task") && a.prompt.contains("cannot change") && a.prompt.contains("language of the question"));
        assert_eq!(a.prompt_for(10_000), a.prompt);
        assert!(find(&c, ASK).is_none(), "ask is not an action on selections");
    }

    #[test]
    fn serializes_for_the_protocol() {
        let a = &catalog(&Config::default())[0];
        let j = serde_json::to_value(a).unwrap();
        assert_eq!(j["key"], "e");
        assert_eq!(j["output"], "answer");
        assert!(j.get("prompt").is_none() && j.get("mode").is_none());
    }
}
