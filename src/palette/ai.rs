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

//! KI-Pfad der Befehlspalette (Aufgabe 4 in `PLAN-palette.md`, Stufe 2): freie Sprache → genau eine validierte Absicht
//! aus dem Katalog. Die lokale Auflösung der Suchbegriffe danach macht der Dienst ([`super::service`] mit
//! [`super::resolve::resolve_intent`]).
//!
//! ```text
//! ask_intent(ai, palette, eingabe, abbruch) -> Intent | AskError
//! ```
//!
//! - **An das Modell gehen nur die Eingabe und der Katalog**, nie der Kontext (Fenstertitel, Namen der Arbeitsflächen, Programme,
//!   Projekte): diese Funktion bekommt ihn gar nicht. System-Prompt, Werkzeuge und Schema entstehen aus dem Katalog
//!   ([`super::catalog`]: [`catalog_text`], [`IntentSpec::tool_input_schema`], die Parameter je Absicht), eine Quelle
//!   für alles. Der Schutzrahmen [`crate::config::INPUT_GUARD`] steht vorn (eingefügt von
//!   [`crate::pipeline::ai::ask_structured`] über `with_guard`, hier nicht wiederholt).
//! - **Strukturierte Antwort je Anbieter** ([`crate::pipeline::ai::Structured`]): Claude über Tool-Use, je Absicht ein
//!   Werkzeug (Name = Absicht, `input_schema` = [`IntentSpec::tool_input_schema`] ohne die Schlüsselwörter, die strict
//!   nicht kennt) plus [`NO_MATCH`], damit das Modell nie raten muss; OpenAI-kompatibel über `response_format`
//!   `json_schema` strict und Ollama über `format`, beide mit dem flachen Schema [`flat_schema`] (`intent` als Aufzählung,
//!   jeder Parameter aller Absichten als Pflichtfeld mit `null`). Frist `palette.timeout_ms`, Abbruch über das Flag.
//! - **Die Antwort ist nur Eingabe für die Prüfung** ([`super::intent::parse_validated`]): unbekannte Absicht, unbekannte
//!   Felder (auch ein gesetzter Parameter einer anderen Absicht im flachen Schema), fehlende Pflichtfelder, Bereiche =
//!   Fehler mit englischer Meldung ohne Eingabetext und ohne Text des Modells.
//! - **Geheimnisse:** Vor jedem Cloud-Aufruf läuft der Geheimnisfilter ([`crate::actions::secrets::scan`]); ein Treffer
//!   sperrt die Cloud, ein lokaler Server bleibt erlaubt (wie `actions.secrets = "local_only"`; „lokal“ nach
//!   [`crate::config::AiCfg::is_cloud`], dieselbe Regel wie bei den Textaktionen).
//! - **Nie loggen:** Eingabe und Antwort kommen in keine Logzeile, auch nicht mit `DICTATE_DEBUG`.

use super::catalog::{self, catalog_text, IntentSpec, ParamKind};
use super::intent::{parse_validated, Intent, IntentError};
use super::resolve::MAX_INPUT_CHARS;
use crate::config::{AiCfg, PaletteCfg};
use crate::pipeline::ai::{self as ai, AiError, AskOpts, CancelFlag, Choice, Provider, Structured, Tool};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

/// Name des Werkzeugs bzw. Wert von `intent` für „keine Absicht passt“; nie der Name einer Absicht (Test).
pub const NO_MATCH: &str = "no_match";
/// Name des Schemas in `response_format` (OpenAI-kompatibel)
pub const SCHEMA_NAME: &str = "palette_intent";
/// Längengrenze der Antwort: eine Absicht ist ein paar Dutzend Tokens, eine Frage (`ask`) höchstens 1000 Zeichen.
/// Bei nicht abschaltbarem Denken legt `pipeline::ai` die Reserve dazu.
pub const MAX_TOKENS: u64 = 1024;

/// Schlüsselwörter, die strict (Claude und OpenAI) nicht kennt; die Grenzen prüft ohnehin Rust.
const NOT_STRICT: &[&str] = &["minimum", "maximum", "minLength", "maxLength", "oneOf"];

/// Warum der KI-Pfad keine Absicht liefert. Meldungen englisch, ohne Eingabe und ohne Text des Modells oder des
/// Servers (auch nicht dessen Fehlermeldung: ein Server könnte darin die Eingabe zitieren).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskError {
    /// Nichts eingegeben
    Empty,
    /// Eingabe länger als [`MAX_INPUT_CHARS`]
    TooLong(usize),
    /// Geheimnisse in der Eingabe und das Ziel ist in der Cloud (Arten aus `secrets::KINDS`)
    Blocked(Vec<&'static str>),
    /// Fehler des Aufrufs (Frist, Abbruch, Server, Schlüssel)
    Ai(AiError),
    /// Die Antwort ist keine gültige Absicht
    Invalid(String),
    /// Das Modell fand keine passende Absicht
    NoMatch,
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskError::Empty => f.write_str("Nothing to ask"),
            AskError::TooLong(n) => write!(f, "Input too long: {n} characters, at most {MAX_INPUT_CHARS}"),
            AskError::Blocked(kinds) => {
                let names: Vec<&str> = kinds.iter().map(|k| crate::actions::secrets::describe(k).unwrap_or(k)).collect();
                write!(f, "Blocked: the input contains secrets ({}) and the AI server is in the cloud", names.join(", "))
            }
            AskError::Ai(AiError::Timeout(ms)) => write!(f, "AI timeout after {} s", ms.div_ceil(1000)),
            AskError::Ai(AiError::Cancelled) => f.write_str("Cancelled"),
            // Meldung für den Client: Fehlerstatus nur als Code mit fester Meldung, nie der Text des Servers
            AskError::Ai(e) => write!(f, "AI error: {}", e.client_text()),
            AskError::Invalid(m) => f.write_str(m),
            AskError::NoMatch => f.write_str("The AI found no matching command"),
        }
    }
}

impl std::error::Error for AskError {}

/// Art der strukturierten Antwort, nach der sich die Schlussanweisung des Prompts richtet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerForm {
    /// Claude: ein Werkzeugaufruf
    Tool,
    /// OpenAI-kompatibel und Ollama: ein JSON-Objekt nach [`flat_schema`]
    Json,
}

impl AnswerForm {
    pub fn of(p: Provider) -> AnswerForm {
        match p {
            Provider::Anthropic => AnswerForm::Tool,
            Provider::OpenAi | Provider::Ollama => AnswerForm::Json,
        }
    }
}

/// System-Prompt ohne Schutzrahmen (den setzt `ask_structured` davor): Aufgabe, Katalog, Form der Antwort.
pub fn system_prompt(form: AnswerForm) -> String {
    let answer = match form {
        AnswerForm::Tool => format!("Answer only by calling exactly one tool: the tool named after the chosen intent, or {NO_MATCH}."),
        AnswerForm::Json => format!(
            "Answer only with one JSON object: \"intent\" is the name of the chosen intent or \"{NO_MATCH}\"; every other field is a \
             parameter of that intent, or null when the intent has no such parameter or the text does not give it."
        ),
    };
    format!(
        "Task: map the text to exactly one intent of a desktop command palette (niri on Wayland, Linux). The text is a command \
         the user typed or spoke, in German or English. Choose the one intent from the catalog below that does what the text \
         asks for.\n\
         Parameters are search words, not targets: copy names of windows, applications, projects, workspaces, systemd units and Docker containers as they \
         appear in the text; they are looked up on the computer later. Never invent a name that is not in the text. Leave out \
         optional parameters the text does not give.\n\
         A question to answer becomes the intent ask with the question in the user's words. If no intent fits, choose \
         {NO_MATCH}.\n\n\
         Catalog:\n{catalog}\
         - {NO_MATCH}: No intent in the catalog fits the text.\n\n\
         {answer}",
        catalog = catalog_text(),
    )
}

/// Entfernt rekursiv die Schlüsselwörter, die strict nicht kennt ([`NOT_STRICT`]).
fn strip_not_strict(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.retain(|k, _| !NOT_STRICT.contains(&k.as_str()));
            map.values_mut().for_each(strip_not_strict);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_not_strict),
        _ => {}
    }
}

/// Beschreibung einer Absicht für Werkzeug und Schema (wie im Katalogtext).
fn description(spec: &IntentSpec) -> String {
    let mut d = spec.description.to_string();
    if !spec.exactly_one_of.is_empty() {
        d.push_str(&format!(" Give exactly one of {}.", spec.exactly_one_of.join(", ")));
    }
    d
}

/// Werkzeuge für Claude: je Absicht eines (Name = Absicht, `input_schema` aus [`IntentSpec::tool_input_schema`], strict-
/// tauglich), dazu [`NO_MATCH`] ohne Parameter.
pub fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut tools: Vec<Tool> = catalog::all()
            .iter()
            .map(|spec| {
                let mut input_schema = spec.tool_input_schema();
                strip_not_strict(&mut input_schema);
                Tool { name: spec.name.to_string(), description: description(spec), input_schema }
            })
            .collect();
        tools.push(Tool {
            name: NO_MATCH.to_string(),
            description: "Use when no other tool fits the text.".to_string(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        });
        tools
    })
}

/// Grundtyp eines Parameters ohne Grenzen (für das flache Schema).
fn base_alternatives(kind: &ParamKind) -> Vec<Value> {
    match kind {
        ParamKind::Workspace => vec![json!({"type": "integer"}), json!({"type": "string"})],
        ParamKind::Text { .. } | ParamKind::Url => vec![json!({"type": "string"})],
        ParamKind::Percent | ParamKind::Step => vec![json!({"type": "integer"})],
        ParamKind::Flag => vec![json!({"type": "boolean"})],
        ParamKind::Choice(values) => vec![json!({"type": "string", "enum": values})],
    }
}

/// Fasst die Typen aller Parameter gleichen Namens zusammen und lässt `null` zu: Aufzählungen werden vereinigt, ein
/// freier String schluckt sie; ein einziger Typ wird `["typ", "null"]`, sonst `anyOf` mit `{"type": "null"}`.
fn nullable(kinds: &[ParamKind]) -> Value {
    let mut types: Vec<&'static str> = Vec::new();
    let mut plain_string = false;
    let mut values: Vec<String> = Vec::new();
    for alt in kinds.iter().flat_map(base_alternatives) {
        let t = match alt["type"].as_str() {
            Some("integer") => "integer",
            Some("boolean") => "boolean",
            _ => "string",
        };
        match alt.get("enum").and_then(Value::as_array) {
            Some(e) => values.extend(e.iter().filter_map(Value::as_str).map(str::to_string).filter(|v| !values.contains(v)).collect::<Vec<_>>()),
            None if t == "string" => plain_string = true,
            None => {}
        }
        if !types.contains(&t) {
            types.push(t);
        }
    }
    if plain_string {
        values.clear();
    }
    let alternatives: Vec<Value> = types
        .iter()
        .map(|t| match (*t, values.is_empty()) {
            ("string", false) => json!({"type": "string", "enum": values}),
            (t, _) => json!({"type": t}),
        })
        .collect();
    match alternatives.as_slice() {
        [one] => match one.get("enum") {
            Some(Value::Array(e)) => {
                let mut e = e.clone();
                e.push(Value::Null);
                json!({"type": ["string", "null"], "enum": e})
            }
            _ => json!({"type": [one["type"].clone(), json!("null")]}),
        },
        _ => {
            let mut any = alternatives;
            any.push(json!({"type": "null"}));
            json!({"anyOf": any})
        }
    }
}

/// Flaches Schema für OpenAI `response_format` (strict) und Ollama `format`: `intent` (Aufzählung aller Absichten und
/// [`NO_MATCH`]) und jeder Parameter aller Absichten als Pflichtfeld mit `null`, ohne `oneOf` und ohne Grenzen. Welche
/// Felder zu welcher Absicht gehören, sagt die Beschreibung; geprüft wird in Rust.
pub fn flat_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut params: BTreeMap<&'static str, (Vec<ParamKind>, Vec<String>)> = BTreeMap::new();
        for spec in catalog::all() {
            for p in spec.params {
                let entry = params.entry(p.name).or_default();
                entry.0.push(p.kind);
                entry.1.push(format!("{}: {}", spec.name, p.doc));
            }
        }
        let mut names: Vec<&str> = catalog::all().iter().map(|s| s.name).collect();
        names.push(NO_MATCH);
        let mut properties = Map::new();
        properties.insert("intent".into(), json!({"type": "string", "enum": names, "description": format!("The chosen intent, or {NO_MATCH} when none fits")}));
        for (name, (kinds, docs)) in &params {
            let mut schema = nullable(kinds);
            schema["description"] = json!(format!("Only for {}. Null otherwise.", docs.join("; ")));
            properties.insert((*name).into(), schema);
        }
        let required: Vec<Value> = properties.keys().map(|k| json!(k)).collect();
        json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
    })
}

/// Meldung zu einer abgelehnten Antwort: nie der Name oder Wert, den das Modell geschrieben hat (er kann aus der
/// Eingabe stammen), nur Katalognamen und Regeln.
fn invalid(e: IntentError) -> AskError {
    AskError::Invalid(match e {
        IntentError::UnknownIntent(_) | IntentError::MissingIntent => "The AI chose an intent that is not in the catalog".to_string(),
        IntentError::UnknownField { intent, .. } => format!("The AI gave an unknown field for {intent}"),
        IntentError::MissingField { .. } | IntentError::InvalidField { .. } | IntentError::Combination { .. } | IntentError::TooLong { .. } => {
            format!("The AI gave an invalid intent: {e}")
        }
        IntentError::Json(_) | IntentError::NotObject | IntentError::Malformed(_) => "The AI gave an invalid intent".to_string(),
    })
}

/// Antwort des Modells → Absicht (`None` = [`NO_MATCH`]). Nur Eingabe für [`parse_validated`]: dort fällt alles durch,
/// was der Katalog nicht erlaubt.
pub fn intent_from_choice(choice: Choice) -> Result<Option<Intent>, AskError> {
    let map = match choice {
        Choice::Tool { name, input } => {
            if name == NO_MATCH {
                return Ok(None);
            }
            let Value::Object(input) = input else { return Err(invalid(IntentError::NotObject)) };
            if catalog::spec(&name).is_none() {
                return Err(invalid(IntentError::UnknownIntent(name)));
            }
            let mut map = Map::new();
            map.insert("intent".into(), Value::String(name.clone()));
            for (k, v) in input {
                if k == "intent" {
                    // das Werkzeug ist die Absicht; ein Feld `intent` in der Eingabe ist ein unbekanntes Feld
                    return Err(invalid(IntentError::UnknownField { intent: catalog::spec(&name).map_or("", |s| s.name), field: k }));
                }
                map.insert(k, v);
            }
            map
        }
        Choice::Json(Value::Object(mut map)) => {
            if map.get("intent").and_then(Value::as_str) == Some(NO_MATCH) {
                return Ok(None);
            }
            // im flachen Schema stehen alle Felder, nicht benutzte als null; ein gesetztes fremdes Feld bleibt Fehler
            map.retain(|_, v| !v.is_null());
            map
        }
        Choice::Json(_) => return Err(invalid(IntentError::NotObject)),
    };
    parse_validated(&Value::Object(map).to_string()).map(Some).map_err(invalid)
}

/// Freie Eingabe → genau eine validierte Absicht. Server, Anbieter und Schlüssel aus `ai`, Modell und Frist aus
/// `palette` ([`PaletteCfg::model_for`], `timeout_ms`). Kein Kontext: er bleibt auf dem Rechner.
pub fn ask_intent(ai_cfg: &AiCfg, palette: &PaletteCfg, input: &str, cancel: &CancelFlag) -> Result<Intent, AskError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AskError::Empty);
    }
    let chars = input.chars().count();
    if chars > MAX_INPUT_CHARS {
        return Err(AskError::TooLong(chars));
    }
    let target = ai::resolve(ai_cfg).map_err(AskError::Ai)?;
    if ai_cfg.is_cloud() {
        let found = crate::actions::secrets::scan(input);
        if !found.is_empty() {
            return Err(AskError::Blocked(found));
        }
    }
    let prompt = system_prompt(AnswerForm::of(target.provider));
    let spec = Structured { tools: tools(), schema_name: SCHEMA_NAME, schema: flat_schema() };
    let opts = AskOpts::new(palette.timeout_ms, MAX_TOKENS);
    let choice = ai::ask_structured(ai_cfg, Some(palette.model_for(ai_cfg)), &prompt, input, &spec, &opts, Some(cancel)).map_err(AskError::Ai)?;
    intent_from_choice(choice)?.ok_or(AskError::NoMatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::intent::WorkspaceRef;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Alle Schlüssel eines Schemas, rekursiv
    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    out.push(k.clone());
                    // Namen von Eigenschaften sind keine Schlüsselwörter
                    if k != "properties" {
                        keys(x, out);
                    } else if let Value::Object(props) = x {
                        props.values().for_each(|p| keys(p, out));
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
            _ => {}
        }
    }

    #[test]
    fn no_match_is_not_an_intent() {
        assert!(catalog::spec(NO_MATCH).is_none());
    }

    #[test]
    fn one_tool_per_intent_plus_no_match_strict_ready() {
        let t = tools();
        assert_eq!(t.len(), catalog::all().len() + 1);
        for (tool, spec) in t.iter().zip(catalog::all()) {
            assert_eq!(tool.name, spec.name);
            let mut want = spec.tool_input_schema();
            strip_not_strict(&mut want);
            assert_eq!(tool.input_schema, want, "{}", spec.name);
        }
        assert_eq!(t.last().unwrap().name, NO_MATCH);
        for tool in t {
            let mut k = Vec::new();
            keys(&tool.input_schema, &mut k);
            for bad in NOT_STRICT {
                assert!(!k.iter().any(|x| x == bad), "{} hat {bad}", tool.name);
            }
            assert_eq!(tool.input_schema["additionalProperties"], false, "{}", tool.name);
            assert_eq!(tool.input_schema["type"], "object");
        }
    }

    #[test]
    fn flat_schema_follows_the_strict_rules() {
        let s = flat_schema();
        let props = s["properties"].as_object().unwrap();
        let required: Vec<&str> = s["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required.len(), props.len(), "alle Felder Pflicht");
        assert!(props.keys().all(|k| required.contains(&k.as_str())));
        assert_eq!(s["additionalProperties"], false);
        let mut k = Vec::new();
        keys(s, &mut k);
        for bad in NOT_STRICT {
            assert!(!k.iter().any(|x| x == bad), "flaches Schema hat {bad}");
        }
        // jeder Parameter jeder Absicht steht drin und lässt null zu
        for spec in catalog::all() {
            for p in spec.params {
                let f = &props[p.name];
                let null_ok = f["type"].as_array().is_some_and(|t| t.contains(&json!("null")))
                    || f["anyOf"].as_array().is_some_and(|a| a.contains(&json!({"type": "null"})));
                assert!(null_ok, "{}: {f}", p.name);
                if let Some(e) = f.get("enum") {
                    assert!(e.as_array().unwrap().contains(&Value::Null), "{}: enum ohne null", p.name);
                }
            }
        }
        // `action` gehört zu media und text_action: beide Aufzählungen
        let action = props["action"]["enum"].as_array().unwrap();
        assert!(action.contains(&json!("play_pause")) && action.contains(&json!("explain")), "{action:?}");
        // Arbeitsfläche: Zahl oder Name
        assert_eq!(props["workspace"]["anyOf"].as_array().unwrap().len(), 3);
        assert!(props.get("tag").is_none() && props.get("layout").is_none());
        let intents = props["intent"]["enum"].as_array().unwrap();
        assert_eq!(intents.len(), catalog::all().len() + 1);
        assert!(intents.contains(&json!(NO_MATCH)));
    }

    #[test]
    fn prompt_is_built_from_the_catalog() {
        for form in [AnswerForm::Tool, AnswerForm::Json] {
            let p = system_prompt(form);
            assert!(p.contains(&catalog_text()), "{form:?}");
            assert!(p.contains(NO_MATCH));
            // der Schutzrahmen kommt erst in `ask_structured` davor, genau einmal
            assert!(!p.contains(crate::config::INPUT_GUARD));
            assert!(!p.contains(['–', '—']), "keine Gedankenstriche");
        }
        assert!(system_prompt(AnswerForm::Tool).contains("calling exactly one tool"));
        assert!(system_prompt(AnswerForm::Json).contains("JSON object"));
    }

    fn tool(name: &str, input: Value) -> Choice {
        Choice::Tool { name: name.into(), input }
    }

    #[test]
    fn choices_become_validated_intents() {
        assert_eq!(
            intent_from_choice(tool("view_workspace", json!({"workspace": 3}))).unwrap(),
            Some(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(3) })
        );
        assert_eq!(intent_from_choice(tool("lock", json!({}))).unwrap(), Some(Intent::Lock {}));
        assert_eq!(intent_from_choice(tool(NO_MATCH, json!({}))).unwrap(), None);
        // flach: null-Felder fallen weg, Strings werden getrimmt
        let flat = json!({"intent": "focus_window", "query": " firefox ", "workspace": null, "url": null, "set": null});
        assert_eq!(intent_from_choice(Choice::Json(flat)).unwrap(), Some(Intent::FocusWindow { query: "firefox".into() }));
        assert_eq!(intent_from_choice(Choice::Json(json!({"intent": NO_MATCH, "query": null}))).unwrap(), None);
    }

    #[test]
    fn invalid_choices_are_errors_without_model_text() {
        let err = |c: Choice| match intent_from_choice(c) {
            Err(AskError::Invalid(m)) => m,
            other => panic!("Fehler erwartet: {other:?}"),
        };
        // erfundene Absicht (Name kann aus der Eingabe stammen: nie in der Meldung)
        let m = err(tool("rm_rf_secret", json!({})));
        assert!(m.contains("not in the catalog") && !m.contains("rm_rf_secret"), "{m}");
        let m = err(Choice::Json(json!({"intent": "format_disk_secret"})));
        assert!(m.contains("not in the catalog") && !m.contains("secret"), "{m}");
        // Bereich, fehlendes Feld, unbekanntes Feld, fremdes Feld im flachen Schema, `intent` in der Werkzeugeingabe
        assert!(err(tool("view_workspace", json!({"workspace": 256}))).contains("between 1 and 255"));
        assert!(err(tool("volume", json!({"set": 150}))).contains("between 0 and 100"));
        assert!(err(tool("focus_window", json!({}))).contains("missing field \"query\""));
        let m = err(tool("focus_window", json!({"query": "x", "secretfield": 1})));
        assert!(m.contains("unknown field for focus_window") && !m.contains("secretfield"), "{m}");
        assert!(err(Choice::Json(json!({"intent": "lock", "query": "x"}))).contains("unknown field for lock"));
        assert!(err(tool("lock", json!({"intent": "reboot"}))).contains("unknown field for lock"));
        assert!(err(tool("volume", json!({"set": 10, "mute": true}))).contains("exactly one of"));
        assert!(err(Choice::Json(json!({"query": "x"}))).contains("not in the catalog"));
        assert!(err(tool("lock", json!("x"))).contains("invalid intent"));
    }

    #[test]
    fn the_diagnosis_comes_from_the_model_only_with_valid_profile_and_search_terms() {
        use crate::palette::intent::DiagProfile;
        let d = |profile: Option<DiagProfile>, unit: Option<&str>, container: Option<&str>| {
            Some(Intent::Diagnose { profile, unit: unit.map(Into::into), container: container.map(Into::into) })
        };
        assert_eq!(intent_from_choice(tool("diagnose", json!({}))).unwrap(), d(None, None, None));
        assert_eq!(intent_from_choice(tool("diagnose", json!({"profile": "last_crash"}))).unwrap(), d(Some(DiagProfile::LastCrash), None, None));
        assert_eq!(intent_from_choice(tool("diagnose", json!({"unit": " cups "}))).unwrap(), d(None, Some("cups"), None));
        assert_eq!(
            intent_from_choice(tool("diagnose", json!({"profile": "docker", "container": "dataroom"}))).unwrap(),
            d(Some(DiagProfile::Docker), None, Some("dataroom"))
        );
        // flach: null-Felder fallen weg
        let flat = json!({"intent": "diagnose", "profile": null, "unit": "docker.service", "container": null, "query": null, "workspace": null});
        assert_eq!(intent_from_choice(Choice::Json(flat)).unwrap(), d(None, Some("docker.service"), None));
        let err = |c: Choice| match intent_from_choice(c) {
            Err(AskError::Invalid(m)) => m,
            other => panic!("Fehler erwartet: {other:?}"),
        };
        // nichts, was kein Name sein kann, und nichts aus dem Text des Modells in der Meldung
        let m = err(tool("diagnose", json!({"unit": "evil; rm -rf /"})));
        assert!(m.contains("may only contain") && !m.contains("evil") && !m.contains("rm -rf"), "{m}");
        let m = err(tool("diagnose", json!({"unit": "--secretflag"})));
        assert!(m.contains("must start with a letter or digit") && !m.contains("secretflag"), "{m}");
        assert!(err(tool("diagnose", json!({"container": "../../etc"}))).contains("must start with a letter or digit"));
        assert!(err(tool("diagnose", json!({"profile": "heat", "unit": "cups"}))).contains("unit goes with the profile unit"));
        assert!(err(tool("diagnose", json!({"unit": "cups", "container": "web"}))).contains("at most one of unit, container"));
        assert!(err(tool("diagnose", json!({"profile": "everything"}))).contains("must be one of"));
        assert!(err(tool("diagnose", json!({"command": "journalctl -f"}))).contains("unknown field for diagnose"));
    }

    #[test]
    fn the_flat_schema_and_the_tools_know_the_diagnosis() {
        let props = flat_schema()["properties"].as_object().unwrap();
        let profiles = props["profile"]["enum"].as_array().unwrap();
        assert_eq!(profiles.len(), crate::palette::intent::DiagProfile::NAMES.len() + 1, "{profiles:?}");
        assert!(profiles.contains(&Value::Null) && profiles.contains(&json!("last_crash")));
        assert!(props.contains_key("unit") && props.contains_key("container"));
        assert!(flat_schema()["properties"]["intent"]["enum"].as_array().unwrap().contains(&json!("diagnose")));
        let tool = tools().iter().find(|t| t.name == "diagnose").expect("a tool for the diagnosis");
        assert!(tool.description.contains("Diagnose this computer"), "{}", tool.description);
        assert!(system_prompt(AnswerForm::Tool).contains("- diagnose(profile?: last_crash|now|unit|docker|network|heat, unit?: text, container?: text)"));
    }

    fn cloud_cfg() -> AiCfg {
        AiCfg { preset: "anthropic".into(), provider: "anthropic".into(), endpoint: String::new(), api_key_file: "/nonexistent/key".into(), ..AiCfg::default() }
    }

    #[test]
    fn secrets_block_the_cloud_before_any_request() {
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let c = cloud_cfg();
        assert!(c.is_cloud());
        let input = "open -----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n-----END OPENSSH PRIVATE KEY-----";
        assert!(!crate::actions::secrets::scan(input).is_empty(), "der Testtext muss ein Geheimnis sein");
        let e = ask_intent(&c, &PaletteCfg::default(), input, &cancel).unwrap_err();
        assert!(matches!(e, AskError::Blocked(_)), "{e:?}");
        let m = e.to_string();
        assert!(m.starts_with("Blocked") && !m.contains("BEGIN"), "{m}");
        // ohne Geheimnis geht es weiter bis zum Schlüssel (der fehlt hier: Fehler des Aufrufs, nicht gesperrt)
        assert!(matches!(ask_intent(&c, &PaletteCfg::default(), "open firefox", &cancel), Err(AskError::Ai(_))));
    }

    #[test]
    fn empty_and_overlong_input() {
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        assert_eq!(ask_intent(&cloud_cfg(), &PaletteCfg::default(), "  \n", &cancel), Err(AskError::Empty));
        let long = "a".repeat(MAX_INPUT_CHARS + 1);
        assert_eq!(ask_intent(&cloud_cfg(), &PaletteCfg::default(), &long, &cancel), Err(AskError::TooLong(MAX_INPUT_CHARS + 1)));
    }
}
