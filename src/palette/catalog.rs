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

//! Katalog der Absichten: die *eine* Quelle für Klasse, Gruppe, Beschreibung und Parameter jeder Absicht.
//!
//! Aus dieser Tabelle ([`CATALOG`]) entstehen
//! - das JSON-Schema je Absicht ([`IntentSpec::schema`], [`IntentSpec::tool_input_schema`], [`combined_schema`]) für
//!   Anthropic Tool-Use (`input_schema`), OpenAI `response_format` und Ollama `format`,
//! - der kompakte Katalogtext für den System-Prompt ([`catalog_text`]) und
//! - die Prüfung der Parameter ([`IntentSpec::check`]), die [`super::intent::parse_validated`] und
//!   [`super::intent::Intent::validate`] benutzen.
//!
//! Das Schema ist ein Hinweis an das Modell und eine Teilmenge von JSON-Schema (`type`, `enum`, `minimum`, `maximum`,
//! `minLength`, `maxLength`, `properties`, `required`, `additionalProperties`, `anyOf`, `oneOf`, `description`), damit
//! jeder Anbieter es versteht; ein Test lässt jede Absicht gegen dieses Schema *und* die Prüfung laufen. Einzige
//! Schranke bleibt die Prüfung in Rust: sie ist an manchen Stellen strenger (getrimmt, keine Steuerzeichen,
//! URL-Schema, `step` ≠ 0), was ein Schema in dieser Teilmenge nicht ausdrückt; die Parameterbeschreibungen nennen es nur teilweise.

use super::intent::{json_type, ColumnDisplay, DiagProfile, IntentError, MediaKey, ScreenshotMode, SettingsPage, Widget, WindowToggle, TEXT_ACTION_NAMES};
use serde::Serialize;
use serde_json::{json, Map, Value};
use url::Url;

/// Längster Name einer Arbeitsfläche (wie `resolve::context::MAX_NAME_CHARS`).
pub const MAX_NAME: usize = 80;
/// Größte Nummer einer Arbeitsfläche: niri zählt `idx` je Monitor als `u8` ab 1. Eine Quelle für Schema, Prüfung,
/// Resolver und Kontext.
pub const MAX_WORKSPACE_INDEX: u8 = u8::MAX;
/// Längster Suchbegriff.
pub const MAX_QUERY: usize = 200;
/// Längster Suchbegriff einer Unit oder eines Containers (`diagnose`): so lang wie `diagnose::catalog::Term::max_len`
/// (ein Test hält beide gleich).
pub const MAX_TERM: usize = 128;
/// Längste Frage an das Modell.
pub const MAX_QUESTION: usize = 1000;
/// Längste URL.
pub const MAX_URL: usize = 2000;
/// Kürzeste URL (`http://a`), für das Schema.
const MIN_URL: usize = 8;

/// Was nach Return geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// Sofort ausführen.
    Run,
    /// Zweite Bestätigung nötig, auch bei lokalem Treffer.
    Confirm,
    /// Antwort der KI im Panel.
    Ai,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Run => "run",
            Class::Confirm => "confirm",
            Class::Ai => "ai",
        }
    }
}

/// Gruppe im Katalogtext und später in der Ergebnisliste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    WindowsWorkspaces,
    AppsProjects,
    System,
    TextAi,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::WindowsWorkspaces, Group::AppsProjects, Group::System, Group::TextAi];

    /// Englische Überschrift.
    pub fn label(self) -> &'static str {
        match self {
            Group::WindowsWorkspaces => "Windows and workspaces",
            Group::AppsProjects => "Apps and projects",
            Group::System => "System",
            Group::TextAi => "Text and AI",
        }
    }
}

/// Typ und Grenzen eines Parameters; erzeugt Schema, Katalogtext und Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Nummer einer Arbeitsfläche (Ganzzahl 1 bis [`MAX_WORKSPACE_INDEX`]) oder Name.
    Workspace,
    /// Freitext: getrimmt, nicht leer, höchstens `max` Zeichen, ohne Steuerzeichen.
    Text { max: usize },
    /// Ganzzahl 0 bis 100.
    Percent,
    /// Ganzzahl -100 bis 100, nicht 0.
    Step,
    /// `true` oder `false`.
    Flag,
    /// Genau einer der Werte.
    Choice(&'static [&'static str]),
    /// Absolute http- oder https-Adresse.
    Url,
}

/// Ein Parameter einer Absicht.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
    /// Englische Beschreibung für das Modell (Schema).
    pub doc: &'static str,
}

const fn req(name: &'static str, kind: ParamKind, doc: &'static str) -> Param {
    Param { name, kind, required: true, doc }
}

const fn opt(name: &'static str, kind: ParamKind, doc: &'static str) -> Param {
    Param { name, kind, required: false, doc }
}

/// Eine Absicht im Katalog.
#[derive(Debug, Clone, Copy)]
pub struct IntentSpec {
    /// Wert von `intent` (snake_case).
    pub name: &'static str,
    pub group: Group,
    pub class: Class,
    /// Englische Kurzbeschreibung für den Katalogtext und das Schema.
    pub description: &'static str,
    pub params: &'static [Param],
    /// Genau eines dieser Felder muss gesetzt sein (leer = keine Regel).
    pub exactly_one_of: &'static [&'static str],
}

const WORKSPACE_DOC: &str = "Workspace number on the monitor (1 = first; a tag number means the same), or the workspace name";
const WINDOW_QUERY_DOC: &str = "Words to find the window: app name, app id or title";

/// Alle Absichten in der Reihenfolge von `PLAN-palette.md` (Katalog v1).
pub static CATALOG: &[IntentSpec] = &[
    // Fenster und Arbeitsflächen
    IntentSpec {
        name: "view_workspace",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Show a workspace (also called tag) by number or name.",
        params: &[req("workspace", ParamKind::Workspace, WORKSPACE_DOC)],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "focus_window",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Focus a window found by search words; switches to its workspace.",
        params: &[req("query", ParamKind::Text { max: MAX_QUERY }, WINDOW_QUERY_DOC)],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "move_window_to_workspace",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Move a window to a workspace; without query the focused window.",
        params: &[req("workspace", ParamKind::Workspace, WORKSPACE_DOC), opt("query", ParamKind::Text { max: MAX_QUERY }, WINDOW_QUERY_DOC)],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "column_display",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Show the column of the focused window normally or as tabs.",
        params: &[req("display", ParamKind::Choice(ColumnDisplay::NAMES), "normal = windows stacked, tabbed = one window at a time with tabs")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "toggle_window",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Toggle a state of the focused window.",
        params: &[req("what", ParamKind::Choice(WindowToggle::NAMES), "State to toggle")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "close_window",
        group: Group::WindowsWorkspaces,
        class: Class::Confirm,
        description: "Close a window; without query the focused window.",
        params: &[opt("query", ParamKind::Text { max: MAX_QUERY }, WINDOW_QUERY_DOC)],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "previous_workspace",
        group: Group::WindowsWorkspaces,
        class: Class::Run,
        description: "Go back to the previously shown workspace.",
        params: &[],
        exactly_one_of: &[],
    },
    // Programme und Projekte
    IntentSpec {
        name: "launch_app",
        group: Group::AppsProjects,
        class: Class::Run,
        description: "Start an application found by name.",
        params: &[req("query", ParamKind::Text { max: MAX_QUERY }, "Words to find the application")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "open_project",
        group: Group::AppsProjects,
        class: Class::Run,
        description: "Open a project in a new terminal in its folder.",
        params: &[req("query", ParamKind::Text { max: MAX_QUERY }, "Words to find the project")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "open_folder",
        group: Group::AppsProjects,
        class: Class::Run,
        description: "Open a project folder in the file manager.",
        params: &[req("query", ParamKind::Text { max: MAX_QUERY }, "Words to find the project")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "open_url",
        group: Group::AppsProjects,
        class: Class::Run,
        description: "Open a web address in the browser.",
        params: &[req("url", ParamKind::Url, "Absolute http or https URL without credentials")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "web_search",
        group: Group::AppsProjects,
        class: Class::Run,
        description: "Search the web.",
        params: &[req("query", ParamKind::Text { max: MAX_QUERY }, "Search terms")],
        exactly_one_of: &[],
    },
    // System
    IntentSpec { name: "lock", group: Group::System, class: Class::Run, description: "Lock the screen.", params: &[], exactly_one_of: &[] },
    IntentSpec {
        name: "volume",
        group: Group::System,
        class: Class::Run,
        description: "Set the output volume, change it by a step or mute it.",
        params: &[
            opt("set", ParamKind::Percent, "New volume in percent"),
            opt("step", ParamKind::Step, "Change in percent points, negative = quieter, never 0"),
            opt("mute", ParamKind::Flag, "true = mute, false = unmute"),
        ],
        exactly_one_of: &["set", "step", "mute"],
    },
    IntentSpec {
        name: "media",
        group: Group::System,
        class: Class::Run,
        description: "Control media playback.",
        params: &[req("action", ParamKind::Choice(MediaKey::NAMES), "Playback control")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "show_widget",
        group: Group::System,
        class: Class::Run,
        description: "Show a built-in panel: clipboard history or emoji picker.",
        params: &[req("widget", ParamKind::Choice(Widget::NAMES), "Panel to show")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "dictate_settings",
        group: Group::System,
        class: Class::Run,
        description: "Open the dictate settings window, optionally on a page.",
        params: &[opt("page", ParamKind::Choice(SettingsPage::NAMES), "Settings page to open")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "screenshot",
        group: Group::System,
        class: Class::Run,
        description: "Take a screenshot.",
        params: &[req("mode", ParamKind::Choice(ScreenshotMode::NAMES), "What to capture")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "diagnose",
        group: Group::System,
        class: Class::Ai,
        description: "Diagnose this computer with the AI: open the diagnosis panel, optionally on a profile.",
        params: &[
            opt(
                "profile",
                ParamKind::Choice(DiagProfile::NAMES),
                "last_crash = why the computer crashed or froze, now = what is wrong right now, unit = a failed systemd unit, docker = containers, network = connection problems, heat = temperatures",
            ),
            opt("unit", ParamKind::Text { max: MAX_TERM }, "Name of a systemd unit or service as written in the text; only with profile unit"),
            opt("container", ParamKind::Text { max: MAX_TERM }, "Name of a Docker container as written in the text; only with profile docker"),
        ],
        exactly_one_of: &[],
    },
    IntentSpec { name: "logout", group: Group::System, class: Class::Confirm, description: "Log out of the session.", params: &[], exactly_one_of: &[] },
    IntentSpec { name: "reboot", group: Group::System, class: Class::Confirm, description: "Restart the computer.", params: &[], exactly_one_of: &[] },
    IntentSpec { name: "poweroff", group: Group::System, class: Class::Confirm, description: "Shut the computer down.", params: &[], exactly_one_of: &[] },
    IntentSpec {
        name: "suspend",
        group: Group::System,
        class: Class::Confirm,
        description: "Suspend the computer to memory.",
        params: &[],
        exactly_one_of: &[],
    },
    // Text und KI
    IntentSpec {
        name: "ask",
        group: Group::TextAi,
        class: Class::Ai,
        description: "Answer a general question; the answer is shown in the panel.",
        params: &[req("question", ParamKind::Text { max: MAX_QUESTION }, "The question, in the user's words")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "text_action",
        group: Group::TextAi,
        class: Class::Ai,
        description: "Run a text action on the selected text.",
        params: &[req("action", ParamKind::Choice(&TEXT_ACTION_NAMES), "Text action to run")],
        exactly_one_of: &[],
    },
    IntentSpec {
        name: "explain_clipboard",
        group: Group::TextAi,
        class: Class::Ai,
        description: "Explain the text currently in the clipboard.",
        params: &[],
        exactly_one_of: &[],
    },
];

/// Alle Absichten des Katalogs.
pub fn all() -> &'static [IntentSpec] {
    CATALOG
}

/// Eintrag zu einem Namen (`None` bei unbekannter Absicht).
pub fn spec(name: &str) -> Option<&'static IntentSpec> {
    CATALOG.iter().find(|s| s.name == name)
}

// ---------------------------------------------------------------------------------------------------------------
// Prüfung

/// Zeichen, die in einem Parameter nichts verloren haben: Steuerzeichen (auch Zeilenumbruch, Tab, DEL, C1) und die
/// Richtungssteuerzeichen von Unicode, mit denen sich der angezeigte Text umdrehen ließe („Trojan Source“).
fn is_forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn check_text(s: &str, max: usize) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err("must not be empty".into());
    }
    if s != s.trim() {
        return Err("must not start or end with whitespace".into());
    }
    let len = s.chars().count();
    if len > max {
        return Err(format!("must be at most {max} characters, got {len}"));
    }
    if s.chars().any(is_forbidden) {
        return Err("must not contain control characters".into());
    }
    Ok(())
}

fn check_url(s: &str) -> Result<(), String> {
    check_text(s, MAX_URL)?;
    if s.chars().any(char::is_whitespace) {
        return Err("must not contain whitespace".into());
    }
    let lower = s.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("must be an http or https URL".into());
    }
    let url = Url::parse(s).map_err(|e| format!("is not a valid URL ({e})"))?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err("must contain a host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not contain credentials".into());
    }
    Ok(())
}

fn check_int(v: &Value, min: i64, max: i64) -> Result<i64, String> {
    match v.as_i64() {
        Some(n) if (min..=max).contains(&n) => Ok(n),
        Some(n) => Err(format!("must be between {min} and {max}, got {n}")),
        None if v.is_number() => Err("must be an integer".into()),
        None => Err(format!("must be an integer, got {}", json_type(v))),
    }
}

impl ParamKind {
    /// Kurzform des Typs für den Katalogtext.
    pub fn type_label(&self) -> String {
        match self {
            ParamKind::Workspace => "number|name".into(),
            ParamKind::Text { .. } => "text".into(),
            ParamKind::Percent => "0-100".into(),
            ParamKind::Step => "-100..100".into(),
            ParamKind::Flag => "true|false".into(),
            ParamKind::Choice(values) => values.join("|"),
            ParamKind::Url => "http(s) URL".into(),
        }
    }

    /// Ist der Wert nach den Regeln dieses Typs gültig? Die Meldung ist die Fortsetzung von „field "x" of intent y …“.
    pub fn check(&self, v: &Value) -> Result<(), String> {
        match self {
            ParamKind::Workspace => match v {
                Value::String(s) => check_text(s, MAX_NAME),
                Value::Number(_) => check_int(v, 1, i64::from(MAX_WORKSPACE_INDEX))
                    .map(|_| ())
                    .map_err(|e| e.replace("an integer", &format!("a workspace number (integer 1-{MAX_WORKSPACE_INDEX}) or a workspace name"))),
                _ => Err(format!("must be a workspace number (integer 1-{MAX_WORKSPACE_INDEX}) or a workspace name, got {}", json_type(v))),
            },
            ParamKind::Text { max } => match v {
                Value::String(s) => check_text(s, *max),
                _ => Err(format!("must be a string, got {}", json_type(v))),
            },
            ParamKind::Percent => check_int(v, 0, 100).map(|_| ()),
            ParamKind::Step => match check_int(v, -100, 100)? {
                0 => Err("must not be 0 (a step changes the volume)".into()),
                _ => Ok(()),
            },
            ParamKind::Flag => match v {
                Value::Bool(_) => Ok(()),
                _ => Err(format!("must be true or false, got {}", json_type(v))),
            },
            ParamKind::Choice(values) => match v {
                Value::String(s) if values.iter().any(|value| value == s) => Ok(()),
                Value::String(_) => Err(format!("must be one of: {}", values.join(", "))),
                _ => Err(format!("must be a string (one of: {}), got {}", values.join(", "), json_type(v))),
            },
            ParamKind::Url => match v {
                Value::String(s) => check_url(s),
                _ => Err(format!("must be a string, got {}", json_type(v))),
            },
        }
    }

    /// Ob Zeichenketten dieses Typs vor der Prüfung von Leerraum am Rand befreit werden (Freitext, Namen, URL); bei
    /// Aufzählungen nicht, dort ist nur der genaue Wert gültig.
    fn trims(&self) -> bool {
        matches!(self, ParamKind::Workspace | ParamKind::Text { .. } | ParamKind::Url)
    }

    /// JSON-Schema des Wertes (Teilmenge, siehe Modulkopf).
    pub fn schema(&self) -> Value {
        match self {
            ParamKind::Workspace => json!({
                "anyOf": [
                    {"type": "integer", "minimum": 1, "maximum": MAX_WORKSPACE_INDEX},
                    {"type": "string", "minLength": 1, "maxLength": MAX_NAME},
                ]
            }),
            ParamKind::Text { max } => json!({"type": "string", "minLength": 1, "maxLength": max}),
            ParamKind::Percent => json!({"type": "integer", "minimum": 0, "maximum": 100}),
            ParamKind::Step => json!({"type": "integer", "minimum": -100, "maximum": 100}),
            ParamKind::Flag => json!({"type": "boolean"}),
            ParamKind::Choice(values) => json!({"type": "string", "enum": values}),
            ParamKind::Url => json!({"type": "string", "minLength": MIN_URL, "maxLength": MAX_URL}),
        }
    }
}

impl IntentSpec {
    /// Der Parameter mit diesem Namen.
    pub fn param(&self, name: &str) -> Option<&'static Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Befreit Textparameter (Freitext, Name einer Arbeitsfläche, URL) von Leerraum am Rand; sonst bleibt alles, wie es ist.
    pub(crate) fn normalize(&self, map: &mut Map<String, Value>) {
        for p in self.params.iter().filter(|p| p.kind.trims()) {
            if let Some(Value::String(s)) = map.get_mut(p.name) {
                let trimmed = s.trim();
                if trimmed.len() != s.len() {
                    *s = trimmed.to_string();
                }
            }
        }
    }

    /// Prüft die Felder einer Absicht (JSON-Objekt einschließlich `intent`): keine unbekannten Felder, Pflichtfelder
    /// da, jeder Wert nach seinem Typ, Regel „genau eines von“. Der Wert von `intent` ist schon aufgelöst.
    pub(crate) fn check(&self, map: &Map<String, Value>) -> Result<(), IntentError> {
        for key in map.keys() {
            if key != "intent" && self.param(key).is_none() {
                return Err(IntentError::UnknownField { intent: self.name, field: key.clone() });
            }
        }
        for p in self.params.iter().filter(|p| p.required) {
            if !map.contains_key(p.name) {
                return Err(IntentError::MissingField { intent: self.name, field: p.name });
            }
        }
        for p in self.params {
            if let Some(v) = map.get(p.name) {
                p.kind.check(v).map_err(|reason| IntentError::InvalidField { intent: self.name, field: p.name, reason })?;
            }
        }
        if !self.exactly_one_of.is_empty() {
            let given = self.exactly_one_of.iter().filter(|f| map.contains_key(**f)).count();
            if given != 1 {
                return Err(IntentError::Combination { intent: self.name, reason: format!("give exactly one of {}", self.exactly_one_of.join(", ")) });
            }
        }
        Ok(())
    }

    fn object_schema(&self, with_intent: bool) -> Value {
        let mut properties = Map::new();
        let mut required: Vec<Value> = Vec::new();
        if with_intent {
            properties.insert("intent".into(), json!({"type": "string", "enum": [self.name]}));
            required.push(json!("intent"));
        }
        for p in self.params {
            let mut schema = p.kind.schema();
            if let Value::Object(o) = &mut schema {
                o.insert("description".into(), json!(p.doc));
            }
            properties.insert(p.name.into(), schema);
            if p.required {
                required.push(json!(p.name));
            }
        }
        let mut description = self.description.to_string();
        if !self.exactly_one_of.is_empty() {
            description.push_str(&format!(" Give exactly one of {}.", self.exactly_one_of.join(", ")));
        }
        let mut schema = Map::new();
        schema.insert("type".into(), json!("object"));
        schema.insert("description".into(), json!(description));
        schema.insert("properties".into(), Value::Object(properties));
        if !required.is_empty() {
            schema.insert("required".into(), Value::Array(required));
        }
        schema.insert("additionalProperties".into(), json!(false));
        if !self.exactly_one_of.is_empty() {
            let alternatives: Vec<Value> = self.exactly_one_of.iter().map(|f| json!({"required": [f]})).collect();
            schema.insert("oneOf".into(), Value::Array(alternatives));
        }
        Value::Object(schema)
    }

    /// Vollständiges Schema der Absicht als Objekt mit dem Feld `intent` (für `response_format` und `format`).
    pub fn schema(&self) -> Value {
        self.object_schema(true)
    }

    /// Schema nur der Parameter, ohne `intent` (für `input_schema` eines Tools; der Name des Tools ist die Absicht).
    pub fn tool_input_schema(&self) -> Value {
        self.object_schema(false)
    }

    /// Eine Zeile des Katalogtextes: `name(param: typ, optional?: typ): Beschreibung`.
    fn text_line(&self) -> String {
        let params: Vec<String> = self.params.iter().map(|p| format!("{}{}: {}", p.name, if p.required { "" } else { "?" }, p.kind.type_label())).collect();
        let mut line = format!("- {}", self.name);
        if !params.is_empty() {
            line.push_str(&format!("({})", params.join(", ")));
        }
        line.push_str(": ");
        line.push_str(self.description);
        if !self.exactly_one_of.is_empty() {
            line.push_str(&format!(" Give exactly one of {}.", self.exactly_one_of.join(", ")));
        }
        line
    }
}

/// Ein Schema für jede Absicht: `{"anyOf": [<Schema je Absicht>]}`; genau eine Alternative passt über `intent`.
pub fn combined_schema() -> Value {
    json!({"anyOf": CATALOG.iter().map(IntentSpec::schema).collect::<Vec<_>>()})
}

/// Kompakter Katalogtext für den System-Prompt: nach Gruppen, eine Zeile je Absicht mit Parametern.
/// Der Prompt drumherum (Schutzrahmen, Anweisung zur Antwort) kommt erst mit dem KI-Pfad (Aufgabe 4).
pub fn catalog_text() -> String {
    let mut out = String::new();
    for group in Group::ALL {
        out.push_str(group.label());
        out.push_str(":\n");
        for s in CATALOG.iter().filter(|s| s.group == group) {
            out.push_str(&s.text_line());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::intent::{parse_validated, DiagProfile, Intent, WorkspaceRef};
    use std::collections::BTreeSet;

    // ---- Ein kleiner Prüfer für genau die Teilmenge von JSON-Schema, die der Katalog erzeugt. Jedes andere
    // Schlüsselwort lässt den Test scheitern, damit das Schema in der Teilmenge bleibt, die alle Anbieter kennen.

    fn schema_accepts(schema: &Value, v: &Value) -> bool {
        let s = schema.as_object().expect("schema is an object");
        for (key, rule) in s {
            let ok = match key.as_str() {
                "description" => true,
                "type" => match rule.as_str().unwrap() {
                    "object" => v.is_object(),
                    "string" => v.is_string(),
                    "integer" => v.is_i64() || v.is_u64(),
                    "boolean" => v.is_boolean(),
                    other => panic!("type {other} is outside the supported subset"),
                },
                "enum" => rule.as_array().unwrap().contains(v),
                "minimum" => v.as_i64().is_none_or(|n| n >= rule.as_i64().unwrap()),
                "maximum" => v.as_i64().is_none_or(|n| n <= rule.as_i64().unwrap()),
                "minLength" => v.as_str().is_none_or(|t| t.chars().count() as u64 >= rule.as_u64().unwrap()),
                "maxLength" => v.as_str().is_none_or(|t| t.chars().count() as u64 <= rule.as_u64().unwrap()),
                "properties" => {
                    v.as_object().is_none_or(|o| rule.as_object().unwrap().iter().all(|(k, sub)| o.get(k).is_none_or(|val| schema_accepts(sub, val))))
                }
                "required" => v.as_object().is_none_or(|o| rule.as_array().unwrap().iter().all(|k| o.contains_key(k.as_str().unwrap()))),
                "additionalProperties" => {
                    assert_eq!(rule, &json!(false), "only additionalProperties:false is supported");
                    v.as_object().is_none_or(|o| o.keys().all(|k| s["properties"].as_object().unwrap().contains_key(k)))
                }
                "anyOf" => rule.as_array().unwrap().iter().any(|sub| schema_accepts(sub, v)),
                "oneOf" => rule.as_array().unwrap().iter().filter(|sub| schema_accepts(sub, v)).count() == 1,
                other => panic!("keyword {other} is outside the supported subset"),
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// Jede Absicht mit einem gültigen Beispiel (JSON) und derselben Absicht als Rust-Wert.
    fn examples() -> Vec<(&'static str, Intent)> {
        vec![
            (r#"{"intent":"view_workspace","workspace":3}"#, Intent::ViewWorkspace { workspace: WorkspaceRef::Index(3) }),
            (r#"{"intent":"view_workspace","workspace":"web"}"#, Intent::ViewWorkspace { workspace: WorkspaceRef::Name("web".into()) }),
            (r#"{"intent":"focus_window","query":"firefox"}"#, Intent::FocusWindow { query: "firefox".into() }),
            (r#"{"intent":"move_window_to_workspace","workspace":2}"#, Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(2), query: None }),
            (
                r#"{"intent":"move_window_to_workspace","workspace":"mail","query":"thunderbird"}"#,
                Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Name("mail".into()), query: Some("thunderbird".into()) },
            ),
            (r#"{"intent":"column_display","display":"tabbed"}"#, Intent::ColumnDisplay { display: ColumnDisplay::Tabbed }),
            (r#"{"intent":"column_display","display":"normal"}"#, Intent::ColumnDisplay { display: ColumnDisplay::Normal }),
            (r#"{"intent":"toggle_window","what":"fullscreen"}"#, Intent::ToggleWindow { what: WindowToggle::Fullscreen }),
            (r#"{"intent":"close_window"}"#, Intent::CloseWindow { query: None }),
            (r#"{"intent":"close_window","query":"slack"}"#, Intent::CloseWindow { query: Some("slack".into()) }),
            (r#"{"intent":"previous_workspace"}"#, Intent::PreviousWorkspace {}),
            (r#"{"intent":"launch_app","query":"gimp"}"#, Intent::LaunchApp { query: "gimp".into() }),
            (r#"{"intent":"open_project","query":"dataroom"}"#, Intent::OpenProject { query: "dataroom".into() }),
            (r#"{"intent":"open_folder","query":"dictate"}"#, Intent::OpenFolder { query: "dictate".into() }),
            (r#"{"intent":"open_url","url":"https://example.com/a?b=c#d"}"#, Intent::OpenUrl { url: "https://example.com/a?b=c#d".into() }),
            (r#"{"intent":"web_search","query":"rust serde untagged"}"#, Intent::WebSearch { query: "rust serde untagged".into() }),
            (r#"{"intent":"lock"}"#, Intent::Lock {}),
            (r#"{"intent":"volume","set":30}"#, Intent::Volume { set: Some(30), step: None, mute: None }),
            (r#"{"intent":"volume","set":0}"#, Intent::Volume { set: Some(0), step: None, mute: None }),
            (r#"{"intent":"volume","step":-10}"#, Intent::Volume { set: None, step: Some(-10), mute: None }),
            (r#"{"intent":"volume","mute":false}"#, Intent::Volume { set: None, step: None, mute: Some(false) }),
            (r#"{"intent":"media","action":"play_pause"}"#, Intent::Media { action: MediaKey::PlayPause }),
            (r#"{"intent":"show_widget","widget":"clipboard_history"}"#, Intent::ShowWidget { widget: Widget::ClipboardHistory }),
            (r#"{"intent":"dictate_settings"}"#, Intent::DictateSettings { page: None }),
            (r#"{"intent":"dictate_settings","page":"voice"}"#, Intent::DictateSettings { page: Some(SettingsPage::Voice) }),
            (r#"{"intent":"screenshot","mode":"area"}"#, Intent::Screenshot { mode: ScreenshotMode::Area }),
            (r#"{"intent":"logout"}"#, Intent::Logout {}),
            (r#"{"intent":"reboot"}"#, Intent::Reboot {}),
            (r#"{"intent":"poweroff"}"#, Intent::Poweroff {}),
            (r#"{"intent":"suspend"}"#, Intent::Suspend {}),
            (r#"{"intent":"ask","question":"How do I undo the last git commit?"}"#, Intent::Ask { question: "How do I undo the last git commit?".into() }),
            (r#"{"intent":"text_action","action":"translate"}"#, Intent::TextAction { action: "translate".into() }),
            (r#"{"intent":"explain_clipboard"}"#, Intent::ExplainClipboard {}),
            (r#"{"intent":"diagnose"}"#, Intent::Diagnose { profile: None, unit: None, container: None }),
            (r#"{"intent":"diagnose","profile":"last_crash"}"#, Intent::Diagnose { profile: Some(DiagProfile::LastCrash), unit: None, container: None }),
            (r#"{"intent":"diagnose","profile":"heat"}"#, Intent::Diagnose { profile: Some(DiagProfile::Heat), unit: None, container: None }),
            (r#"{"intent":"diagnose","unit":"cups"}"#, Intent::Diagnose { profile: None, unit: Some("cups".into()), container: None }),
            (
                r#"{"intent":"diagnose","profile":"unit","unit":"docker.service"}"#,
                Intent::Diagnose { profile: Some(DiagProfile::Unit), unit: Some("docker.service".into()), container: None },
            ),
            (r#"{"intent":"diagnose","container":"dataroom"}"#, Intent::Diagnose { profile: None, unit: None, container: Some("dataroom".into()) }),
            (
                r#"{"intent":"diagnose","profile":"docker","container":"data_room-1.web"}"#,
                Intent::Diagnose { profile: Some(DiagProfile::Docker), unit: None, container: Some("data_room-1.web".into()) },
            ),
        ]
    }

    /// Jede Absicht einmal mit *allen* Feldern gesetzt, als Struct-Literal ohne `..`: kommt ein Feld im Typ hinzu,
    /// übersetzt der Test nicht mehr. (`volume` hat so alle drei Felder und ist als Absicht ungültig; hier zählt nur,
    /// welche Felder der Typ kennt.)
    fn full_intents() -> Vec<Intent> {
        vec![
            Intent::ViewWorkspace { workspace: WorkspaceRef::Index(1) },
            Intent::FocusWindow { query: "q".into() },
            Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(1), query: Some("q".into()) },
            Intent::ColumnDisplay { display: ColumnDisplay::Tabbed },
            Intent::ToggleWindow { what: WindowToggle::Floating },
            Intent::CloseWindow { query: Some("q".into()) },
            Intent::PreviousWorkspace {},
            Intent::LaunchApp { query: "q".into() },
            Intent::OpenProject { query: "q".into() },
            Intent::OpenFolder { query: "q".into() },
            Intent::OpenUrl { url: "https://example.com".into() },
            Intent::WebSearch { query: "q".into() },
            Intent::Lock {},
            Intent::Volume { set: Some(1), step: Some(1), mute: Some(true) },
            Intent::Media { action: MediaKey::Next },
            Intent::ShowWidget { widget: Widget::Emoji },
            Intent::DictateSettings { page: Some(SettingsPage::Ai) },
            Intent::Screenshot { mode: ScreenshotMode::Full },
            Intent::Logout {},
            Intent::Reboot {},
            Intent::Poweroff {},
            Intent::Suspend {},
            Intent::Ask { question: "q".into() },
            Intent::TextAction { action: "explain".into() },
            Intent::ExplainClipboard {},
            Intent::Diagnose { profile: Some(DiagProfile::Now), unit: Some("u".into()), container: Some("c".into()) },
        ]
    }

    #[test]
    fn every_intent_has_a_catalog_entry() {
        let names: BTreeSet<&str> = CATALOG.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), CATALOG.len(), "duplicate names in the catalog");
        let full = full_intents();
        assert_eq!(full.len(), CATALOG.len());
        for intent in &full {
            assert!(spec(intent.name()).is_some(), "{} is missing in the catalog", intent.name());
        }
        let covered: BTreeSet<&str> = full.iter().map(Intent::name).collect();
        assert_eq!(covered, names);
        // `name()` ist der Name auf dem Draht
        for intent in &full {
            assert_eq!(serde_json::to_value(intent).unwrap()["intent"], json!(intent.name()));
        }
    }

    #[test]
    fn schema_fields_equal_struct_fields() {
        for intent in full_intents() {
            let spec = intent.spec();
            let Value::Object(map) = serde_json::to_value(&intent).unwrap() else { panic!("not an object") };
            let struct_fields: BTreeSet<&str> = map.keys().map(String::as_str).filter(|k| *k != "intent").collect();
            let spec_fields: BTreeSet<&str> = spec.params.iter().map(|p| p.name).collect();
            assert_eq!(struct_fields, spec_fields, "fields of {} differ between the type and the catalog", spec.name);
            // Und das erzeugte Schema nennt genau diese Felder
            let schema = spec.schema();
            let schema_fields: BTreeSet<&str> = schema["properties"].as_object().unwrap().keys().map(String::as_str).filter(|k| *k != "intent").collect();
            assert_eq!(schema_fields, spec_fields, "schema fields of {}", spec.name);
            let tool = spec.tool_input_schema();
            assert!(tool["properties"].as_object().unwrap().keys().all(|k| k != "intent"));
            // Pflichtfelder des Schemas = Pflichtfelder im Katalog
            let required: BTreeSet<&str> =
                schema.get("required").map(|r| r.as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect()).unwrap_or_default();
            let expected: BTreeSet<&str> = spec.params.iter().filter(|p| p.required).map(|p| p.name).chain(["intent"]).collect();
            assert_eq!(required, expected, "required fields of {}", spec.name);
            assert_eq!(schema["additionalProperties"], json!(false));
        }
    }

    #[test]
    fn examples_are_valid_for_schema_and_validation() {
        let mut seen = BTreeSet::new();
        for (json, expected) in examples() {
            let value: Value = serde_json::from_str(json).unwrap();
            let intent = parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(intent, expected, "{json}");
            assert!(intent.validate().is_ok(), "{json}");
            assert!(schema_accepts(&intent.spec().schema(), &value), "schema of {} rejects {json}", intent.name());
            assert!(schema_accepts(&combined_schema(), &value), "combined schema rejects {json}");
            // Das Schema der Tool-Eingabe passt auf dieselben Parameter ohne `intent`
            let mut params = value.as_object().unwrap().clone();
            params.remove("intent");
            assert!(schema_accepts(&intent.spec().tool_input_schema(), &Value::Object(params)), "tool schema of {} rejects {json}", intent.name());
            // Und das Schema einer anderen Absicht nimmt das Beispiel nicht an
            for other in CATALOG.iter().filter(|s| s.name != intent.name()) {
                assert!(!schema_accepts(&other.schema(), &value), "schema of {} accepts {json}", other.name);
            }
            seen.insert(intent.name());
        }
        let all: BTreeSet<&str> = CATALOG.iter().map(|s| s.name).collect();
        assert_eq!(seen, all, "every intent needs an example");
    }

    /// Fehlerfälle: (JSON, Teil der Fehlermeldung, ob auch das Schema den Fall ablehnt). Was das Schema in seiner
    /// Teilmenge nicht ausdrückt (Leerraum, Steuerzeichen, URL-Schema, `step` 0), lehnt nur Rust ab.
    fn rejected() -> Vec<(String, &'static str, bool)> {
        let long = |n: usize| "x".repeat(n);
        let mut cases: Vec<(String, &'static str, bool)> = Vec::new();
        let mut add = |json: String, msg: &'static str, schema: bool| cases.push((json, msg, schema));
        // Absicht und Felder
        add(r#"{"intent":"format_disk"}"#.into(), "unknown intent", true);
        add(r#"{"intent":"view_workspace","workspace":3,"extra":1}"#.into(), "unknown field", true);
        add(r#"{"intent":"lock","now":true}"#.into(), "unknown field", true);
        add(r#"{"intent":"previous_workspace","x":1}"#.into(), "unknown field", true);
        add(r#"{"intent":"view_workspace"}"#.into(), "missing field", true);
        add(r#"{"intent":"column_display"}"#.into(), "missing field", true);
        add(r#"{"intent":"focus_window"}"#.into(), "missing field", true);
        add(r#"{"intent":"text_action"}"#.into(), "missing field", true);
        // Arbeitsfläche: 1-255 oder Name
        add(r#"{"intent":"view_workspace","workspace":0}"#.into(), "between 1 and 255", true);
        add(r#"{"intent":"view_workspace","workspace":256}"#.into(), "between 1 and 255", true);
        add(r#"{"intent":"view_workspace","workspace":-1}"#.into(), "between 1 and 255", true);
        add(r#"{"intent":"view_workspace","workspace":2.5}"#.into(), "integer", true);
        add(r#"{"intent":"view_workspace","workspace":true}"#.into(), "workspace number", true);
        add(r#"{"intent":"view_workspace","workspace":""}"#.into(), "must not be empty", true);
        add(r#"{"intent":"view_workspace","workspace":"   "}"#.into(), "must not be empty", false);
        add(format!(r#"{{"intent":"move_window_to_workspace","workspace":"{}"}}"#, long(MAX_NAME + 1)), "at most 80", true);
        add(r#"{"intent":"move_window_to_workspace","workspace":"we\u0007b"}"#.into(), "control characters", false);
        // Text
        add(r#"{"intent":"focus_window","query":""}"#.into(), "must not be empty", true);
        add(r#"{"intent":"focus_window","query":"  \n "}"#.into(), "must not be empty", false);
        add(r#"{"intent":"focus_window","query":5}"#.into(), "must be a string", true);
        add(r#"{"intent":"focus_window","query":["a"]}"#.into(), "must be a string", true);
        add(format!(r#"{{"intent":"launch_app","query":"{}"}}"#, long(MAX_QUERY + 1)), "at most 200", true);
        add(r#"{"intent":"launch_app","query":"a\nb"}"#.into(), "control characters", false);
        add(r#"{"intent":"launch_app","query":"a\tb"}"#.into(), "control characters", false);
        add(r#"{"intent":"launch_app","query":"a\u007fb"}"#.into(), "control characters", false);
        add(r#"{"intent":"launch_app","query":"a\u0085b"}"#.into(), "control characters", false);
        add(r#"{"intent":"open_project","query":"data\u202eroom"}"#.into(), "control characters", false);
        add(r#"{"intent":"close_window","query":null}"#.into(), "must be a string", true);
        add(r#"{"intent":"ask","question":""}"#.into(), "must not be empty", true);
        add(format!(r#"{{"intent":"ask","question":"{}"}}"#, long(MAX_QUESTION + 1)), "at most 1000", true);
        // Lautstärke
        add(r#"{"intent":"volume","set":101}"#.into(), "between 0 and 100", true);
        add(r#"{"intent":"volume","set":-1}"#.into(), "between 0 and 100", true);
        add(r#"{"intent":"volume","set":"50"}"#.into(), "must be an integer", true);
        add(r#"{"intent":"volume","set":50.5}"#.into(), "must be an integer", true);
        add(r#"{"intent":"volume","step":101}"#.into(), "between -100 and 100", true);
        add(r#"{"intent":"volume","step":-101}"#.into(), "between -100 and 100", true);
        add(r#"{"intent":"volume","step":0}"#.into(), "must not be 0", false);
        add(r#"{"intent":"volume","step":"up"}"#.into(), "must be an integer", true);
        add(r#"{"intent":"volume","mute":"yes"}"#.into(), "true or false", true);
        add(r#"{"intent":"volume","mute":1}"#.into(), "true or false", true);
        add(r#"{"intent":"volume"}"#.into(), "exactly one of set, step, mute", true);
        add(r#"{"intent":"volume","set":10,"step":5}"#.into(), "exactly one of set, step, mute", true);
        add(r#"{"intent":"volume","set":10,"step":5,"mute":true}"#.into(), "exactly one of set, step, mute", true);
        // Aufzählungen
        add(r#"{"intent":"toggle_window","what":"minimized"}"#.into(), "must be one of", true);
        add(r#"{"intent":"toggle_window","what":"sticky"}"#.into(), "must be one of", true);
        add(r#"{"intent":"toggle_window","what":"ontop"}"#.into(), "must be one of", true);
        add(r#"{"intent":"column_display","display":"stacked"}"#.into(), "must be one of", true);
        add(r#"{"intent":"show_widget","widget":"claude_usage"}"#.into(), "must be one of", true);
        add(r#"{"intent":"show_widget","widget":"doorcam"}"#.into(), "must be one of", true);
        add(r#"{"intent":"view_tag","tag":3}"#.into(), "unknown intent", true);
        add(r#"{"intent":"set_layout","layout":"tile"}"#.into(), "unknown intent", true);
        add(r#"{"intent":"toggle_window","what":5}"#.into(), "must be a string", true);
        add(r#"{"intent":"media","action":"stop"}"#.into(), "must be one of", true);
        add(r#"{"intent":"show_widget","widget":"browser"}"#.into(), "must be one of", true);
        add(r#"{"intent":"screenshot","mode":"region"}"#.into(), "must be one of", true);
        add(r#"{"intent":"dictate_settings","page":"nope"}"#.into(), "must be one of", true);
        add(r#"{"intent":"text_action","action":"rm -rf /"}"#.into(), "must be one of", true);
        add(r#"{"intent":"text_action","action":"mode:custom"}"#.into(), "must be one of", true);
        // Diagnose: Profile und Suchbegriffe (die Regeln über mehrere Felder und die Zeichen der Suchbegriffe prüft nur Rust)
        add(r#"{"intent":"diagnose","profile":"everything"}"#.into(), "must be one of", true);
        add(r#"{"intent":"diagnose","profile":5}"#.into(), "must be a string", true);
        add(r#"{"intent":"diagnose","profile":null}"#.into(), "must be a string", true);
        add(r#"{"intent":"diagnose","extra":1}"#.into(), "unknown field", true);
        add(r#"{"intent":"diagnose","unit":""}"#.into(), "must not be empty", true);
        add(r#"{"intent":"diagnose","unit":7}"#.into(), "must be a string", true);
        add(format!(r#"{{"intent":"diagnose","unit":"{}"}}"#, long(MAX_TERM + 1)), "at most 128", true);
        add(r#"{"intent":"diagnose","unit":"-x"}"#.into(), "must start with a letter or digit", false);
        add(r#"{"intent":"diagnose","unit":"--help"}"#.into(), "must start with a letter or digit", false);
        add(r#"{"intent":"diagnose","unit":"my app"}"#.into(), "may only contain", false);
        add(r#"{"intent":"diagnose","unit":"a;rm"}"#.into(), "may only contain", false);
        add(r#"{"intent":"diagnose","container":"/etc/passwd"}"#.into(), "must start with a letter or digit", false);
        add(r#"{"intent":"diagnose","container":"web app"}"#.into(), "may only contain", false);
        add(r#"{"intent":"diagnose","container":"a:b"}"#.into(), "may only contain", false);
        add(r#"{"intent":"diagnose","unit":"cups","container":"web"}"#.into(), "at most one of unit, container", false);
        add(r#"{"intent":"diagnose","profile":"heat","unit":"cups"}"#.into(), "unit goes with the profile unit", false);
        add(r#"{"intent":"diagnose","profile":"docker","unit":"cups"}"#.into(), "unit goes with the profile unit", false);
        add(r#"{"intent":"diagnose","profile":"unit","container":"web"}"#.into(), "container goes with the profile docker", false);
        add(r#"{"intent":"diagnose","profile":"network","container":"web"}"#.into(), "container goes with the profile docker", false);
        // URL
        add(r#"{"intent":"open_url","url":"ftp://example.com/x"}"#.into(), "http or https", false);
        add(r#"{"intent":"open_url","url":"javascript:alert(1)"}"#.into(), "http or https", false);
        add(r#"{"intent":"open_url","url":"file:///etc/passwd"}"#.into(), "http or https", false);
        add(r#"{"intent":"open_url","url":"example.com"}"#.into(), "http or https", false);
        add(r#"{"intent":"open_url","url":"http://"}"#.into(), "valid URL", true);
        add(r#"{"intent":"open_url","url":"https://:8080/x"}"#.into(), "valid URL", false);
        add(r#"{"intent":"open_url","url":"https://user:secret@example.com/"}"#.into(), "credentials", false);
        add(r#"{"intent":"open_url","url":"https://example.com/a b"}"#.into(), "whitespace", false);
        add(r#"{"intent":"open_url","url":""}"#.into(), "must not be empty", true);
        add(r#"{"intent":"open_url","url":42}"#.into(), "must be a string", true);
        add(format!(r#"{{"intent":"open_url","url":"https://example.com/{}"}}"#, long(MAX_URL)), "at most 2000", true);
        cases
    }

    #[test]
    fn invalid_cases_are_rejected_by_validation_and_where_expressible_by_the_schema() {
        let cases = rejected();
        assert!(cases.len() >= 60);
        for (json, msg, schema_rejects) in cases {
            let value: Value = serde_json::from_str(&json).unwrap();
            let err = match parse_validated(&json) {
                Ok(i) => panic!("{json} should be rejected, got {i:?}"),
                Err(e) => e.to_string(),
            };
            assert!(err.contains(msg), "{json}: message {err:?} does not contain {msg:?}");
            // Das Schema der genannten Absicht (falls es sie gibt)
            if let Some(spec) = value.get("intent").and_then(Value::as_str).and_then(spec) {
                assert_eq!(!schema_accepts(&spec.schema(), &value), schema_rejects, "schema verdict for {json}");
            }
            if schema_rejects {
                assert!(!schema_accepts(&combined_schema(), &value), "combined schema accepts {json}");
            }
        }
    }

    #[test]
    fn boundaries_agree_between_schema_and_validation() {
        // Grenzwerte, die Schema und Prüfung gleich behandeln: der letzte gültige und (oben in
        // `rejected`) der erste ungültige Wert. Ein Modell, das dem Schema folgt, scheitert nur an den Stellen, die
        // das Schema nicht ausdrückt (Modulkopf).
        for (json, ok) in [
            (r#"{"intent":"view_workspace","workspace":1}"#, true),
            (r#"{"intent":"view_workspace","workspace":255}"#, true),
            (r#"{"intent":"volume","set":100}"#, true),
            (r#"{"intent":"volume","step":100}"#, true),
            (r#"{"intent":"volume","step":-100}"#, true),
            (r#"{"intent":"volume","step":1}"#, true),
            (r#"{"intent":"volume","step":-1}"#, true),
            (r#"{"intent":"open_url","url":"http://a"}"#, true),
        ] {
            let value: Value = serde_json::from_str(json).unwrap();
            assert_eq!(parse_validated(json).is_ok(), ok, "{json}");
            assert_eq!(schema_accepts(&combined_schema(), &value), ok, "{json}");
        }
        let at_limit = format!(r#"{{"intent":"launch_app","query":"{}"}}"#, "ä".repeat(MAX_QUERY));
        assert!(parse_validated(&at_limit).is_ok(), "length counts characters, not bytes");
        assert!(schema_accepts(&combined_schema(), &serde_json::from_str(&at_limit).unwrap()));
    }

    #[test]
    fn classes_follow_the_plan() {
        let confirm: BTreeSet<&str> = ["logout", "reboot", "poweroff", "suspend", "close_window"].into();
        let ai: BTreeSet<&str> = ["ask", "text_action", "explain_clipboard", "diagnose"].into();
        for s in CATALOG {
            let expected = if confirm.contains(s.name) {
                Class::Confirm
            } else if ai.contains(s.name) {
                Class::Ai
            } else {
                Class::Run
            };
            assert_eq!(s.class, expected, "{}", s.name);
        }
        assert_eq!(Intent::Logout {}.class(), Class::Confirm);
        assert_eq!(Intent::Reboot {}.class(), Class::Confirm);
        assert_eq!(Intent::Poweroff {}.class(), Class::Confirm);
        assert_eq!(Intent::Suspend {}.class(), Class::Confirm);
        assert_eq!(Intent::CloseWindow { query: None }.class(), Class::Confirm);
        assert_eq!(Intent::Ask { question: "?".into() }.class(), Class::Ai);
        assert_eq!(Intent::TextAction { action: "explain".into() }.class(), Class::Ai);
        assert_eq!(Intent::ExplainClipboard {}.class(), Class::Ai);
        assert_eq!(Intent::Diagnose { profile: None, unit: None, container: None }.class(), Class::Ai);
        assert_eq!(Intent::Lock {}.class(), Class::Run);
        assert_eq!(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(1) }.class(), Class::Run);
        assert_eq!(Intent::ColumnDisplay { display: ColumnDisplay::Tabbed }.class(), Class::Run);
        assert_eq!(Class::Confirm.as_str(), "confirm");
        assert_eq!(serde_json::to_value(Class::Ai).unwrap(), json!("ai"));
    }

    #[test]
    fn groups_and_descriptions_are_complete() {
        for s in CATALOG {
            assert!(!s.description.is_empty() && s.description.ends_with('.'), "{}", s.name);
            assert!(s.description.is_ascii(), "{}: visible texts are English", s.name);
            for p in s.params {
                assert!(!p.doc.is_empty(), "{}.{}", s.name, p.name);
            }
            for f in s.exactly_one_of {
                assert!(s.param(f).is_some_and(|p| !p.required), "{}: exactly_one_of names an optional parameter", s.name);
            }
        }
        let groups: BTreeSet<&str> = CATALOG.iter().map(|s| s.group.label()).collect();
        assert_eq!(groups.len(), Group::ALL.len());
        assert_eq!(spec("lock").unwrap().group, Group::System);
        assert_eq!(spec("view_workspace").unwrap().group, Group::WindowsWorkspaces);
        assert_eq!(spec("open_project").unwrap().group, Group::AppsProjects);
        assert_eq!(spec("ask").unwrap().group, Group::TextAi);
        assert_eq!(Intent::Lock {}.group(), Group::System);
        assert!(spec("nope").is_none());
    }

    #[test]
    fn catalog_text_is_compact_and_complete() {
        let text = catalog_text();
        for s in CATALOG {
            let line = text
                .lines()
                .find(|l| l.starts_with(&format!("- {}", s.name)) && l[2 + s.name.len()..].starts_with(['(', ':']))
                .unwrap_or_else(|| panic!("{} missing in the catalog text", s.name));
            assert!(line.contains(s.description), "{line}");
            for p in s.params {
                assert!(line.contains(&format!("{}{}: ", p.name, if p.required { "" } else { "?" })), "{line}");
            }
        }
        for g in Group::ALL {
            assert!(text.contains(&format!("{}:\n", g.label())));
        }
        assert!(text.contains("- view_workspace(workspace: number|name): Show a workspace (also called tag) by number or name.\n"), "{text}");
        assert!(text.contains("- column_display(display: normal|tabbed): "), "{text}");
        for gone in ["view_tag", "set_layout", "sticky", "ontop", "claude_usage", "Claude usage", "layout"] {
            assert!(!text.contains(gone), "{gone} is still in the catalog text");
        }
        assert!(text.contains("- volume(set?: 0-100, step?: -100..100, mute?: true|false): Set the output volume, change it by a step or mute it. Give exactly one of set, step, mute.\n"), "{text}");
        assert!(text.contains("- lock: Lock the screen.\n"), "{text}");
        assert!(text.contains("action: explain|translate|rephrase|refine_prompt|analyze_error|explain_code"), "{text}");
        assert!(text.len() < 4000, "catalog text is {} bytes", text.len());
    }

    #[test]
    fn schema_shapes_for_providers() {
        let s = spec("move_window_to_workspace").unwrap().schema();
        assert_eq!(s["type"], "object");
        assert_eq!(s["properties"]["intent"], json!({"type": "string", "enum": ["move_window_to_workspace"]}));
        assert_eq!(s["properties"]["workspace"]["anyOf"][0], json!({"type": "integer", "minimum": 1, "maximum": 255}));
        assert_eq!(s["properties"]["workspace"]["description"], WORKSPACE_DOC);
        assert_eq!(s["required"], json!(["intent", "workspace"]));
        let t = spec("lock").unwrap().tool_input_schema();
        assert_eq!(t["properties"], json!({}));
        assert!(t.get("required").is_none());
        let v = spec("volume").unwrap().tool_input_schema();
        assert_eq!(v["oneOf"], json!([{"required": ["set"]}, {"required": ["step"]}, {"required": ["mute"]}]));
        assert!(v["description"].as_str().unwrap().contains("exactly one of set, step, mute"));
        let c = combined_schema();
        assert_eq!(c["anyOf"].as_array().unwrap().len(), CATALOG.len());
        // Das Schema lässt sich als JSON ausgeben und wieder lesen
        assert_eq!(serde_json::from_str::<Value>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
    }
}
