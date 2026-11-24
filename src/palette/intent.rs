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

//! Absichten (Intents) der Befehlspalette und ihre Validierung.
//!
//! Eine [`Intent`] ist das Ergebnis des Resolvers oder des Modells: eine Absicht aus dem festen Katalog
//! ([`super::catalog`]) mit engen Parametern. Parameter sind Suchbegriffe, keine Ziele; sie werden später lokal gegen
//! echte Daten aufgelöst. Strings sind getrimmt, nicht leer, begrenzt und ohne Steuerzeichen.
//!
//! **Eine Quelle für die Regeln:** Typen und Grenzen stehen in [`super::catalog`] (je Parameter eine
//! [`super::catalog::ParamKind`]). Daraus entstehen das JSON-Schema für das Modell *und* die Prüfung hier
//! ([`parse_validated`], [`Intent::validate`]); beide können deshalb nicht auseinanderlaufen. Die Typen in diesem Modul
//! tragen nur die Daten; ein Test vergleicht ihre Felder mit dem Katalog.

use super::catalog::{self, Class, Group, IntentSpec};
use crate::actions::BUILTIN;
use crate::diagnose::catalog::Term;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Längste angenommene Modellantwort (Bytes); eine Absicht ist ein paar Dutzend Bytes groß.
pub const MAX_JSON_BYTES: usize = 8 * 1024;

/// Deklariert ein Enum, dessen Namen auf dem Draht (serde), im Schema und im Katalogtext aus *einem* Literal je Wert
/// stammen. `ALL` und `NAMES` stehen in derselben Reihenfolge wie die Werte.
macro_rules! name_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $var:ident = $s:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $s)] $var,)+
        }

        impl $name {
            /// Alle Werte in der Reihenfolge der Deklaration.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// Die Namen auf dem Draht in derselben Reihenfolge.
            pub const NAMES: &'static [&'static str] = &[$($s),+];

            /// Der Name auf dem Draht.
            pub const fn name(self) -> &'static str {
                match self {
                    $($name::$var => $s,)+
                }
            }
        }
    };
}

name_enum! {
    /// Zustand des fokussierten Fensters, der umgeschaltet wird (`toggle_window`).
    WindowToggle {
        Floating = "floating",
        Fullscreen = "fullscreen",
        Maximized = "maximized",
    }
}

name_enum! {
    /// Darstellung der Spalte des fokussierten Fensters (`column_display`, niri `SetColumnDisplay`; ersetzt `set_layout`
    /// von Awesome, Vorlage E5).
    ColumnDisplay {
        Normal = "normal",
        Tabbed = "tabbed",
    }
}

name_enum! {
    /// Medientaste (`media`).
    MediaKey {
        PlayPause = "play_pause",
        Next = "next",
        Previous = "previous",
    }
}

name_enum! {
    /// Fest eingebaute Fenster und Anzeigen (`show_widget`): Verlauf der Zwischenablage und Emoji-Wähler, eingestellt
    /// oder automatisch Noctalia bzw. rofi-emoji, angeboten nur, wenn gefunden (`super::widgets`, `PLAN-lizenz.md` O6;
    /// vorher fest nach den Vorlagen E8, E9 in `PLAN-wayland.md` 6g; `claude_usage` gestrichen, E7; die Türkamera E6
    /// entfiel mit O3, E3: sie rief ein eigenes Programm des Autors, ein Desktop-Eintrag leistet dasselbe über
    /// `launch_app`).
    Widget {
        ClipboardHistory = "clipboard_history",
        Emoji = "emoji",
    }
}

name_enum! {
    /// Art der Bildschirmaufnahme (`screenshot`).
    ScreenshotMode {
        Full = "full",
        Window = "window",
        Area = "area",
    }
}

name_enum! {
    /// Seite des Einstellungsfensters (`dictate_settings`); Namen wie `dictate-settings --page` sie versteht.
    SettingsPage {
        General = "general",
        Audio = "audio",
        Model = "model",
        Recognition = "recognition",
        Overlay = "overlay",
        Voice = "voice",
        Vocabulary = "vocabulary",
        Profiles = "profiles",
        Ai = "ai",
        Actions = "actions",
        Palette = "palette",
        Diagnose = "diagnose",
        History = "history",
        About = "about",
    }
}

name_enum! {
    /// Profil der Diagnose (`diagnose`); Namen wie `diagnose::Profile::name` sie kennt (ein Test hält beide gleich).
    DiagProfile {
        LastCrash = "last_crash",
        Now = "now",
        Unit = "unit",
        Docker = "docker",
        Network = "network",
        Heat = "heat",
    }
}

/// Namen der eingebauten Textaktionen (`actions::BUILTIN`), die `text_action` wählen kann; direkt aus der Tabelle
/// der Aktionen, damit keine zweite Liste entsteht.
pub const TEXT_ACTION_NAMES: [&str; BUILTIN.len()] = {
    let mut names = [""; BUILTIN.len()];
    let mut i = 0;
    while i < BUILTIN.len() {
        names[i] = BUILTIN[i].name;
        i += 1;
    }
    names
};

/// Eine Arbeitsfläche: Nummer (`idx` ab 1 auf dem Monitor des Zielfensters, Vorlage E2 b) oder Name. Eine Zahl als Text
/// (`"3"`) ist ein Name; der Resolver entscheidet später, ob eine Arbeitsfläche so heißt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspaceRef {
    Index(u8),
    Name(String),
}

/// Eine Absicht aus dem Katalog v1 (`PLAN-palette.md`). Auf dem Draht ein JSON-Objekt mit `intent` als Name, z. B.
/// `{"intent":"move_window_to_workspace","workspace":3,"query":"firefox"}`. Unbekannte Felder werden abgelehnt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    /// Arbeitsfläche anzeigen.
    ViewWorkspace { workspace: WorkspaceRef },
    /// Fenster per Suchbegriff finden und fokussieren (niri wechselt dabei Arbeitsfläche und Monitor).
    FocusWindow { query: String },
    /// Fenster auf eine Arbeitsfläche verschieben; ohne `query` das fokussierte.
    MoveWindowToWorkspace {
        workspace: WorkspaceRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// Spalte des fokussierten Fensters normal oder als Reiter zeigen.
    ColumnDisplay { display: ColumnDisplay },
    /// Zustand des fokussierten Fensters umschalten.
    ToggleWindow { what: WindowToggle },
    /// Fenster schließen; ohne `query` das fokussierte. Braucht Bestätigung.
    CloseWindow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// Zurück zur zuletzt gezeigten Arbeitsfläche.
    PreviousWorkspace {},
    /// Programm per Suchbegriff starten.
    LaunchApp { query: String },
    /// Projekt in einer Claude-Code-Sitzung im Projektordner öffnen.
    OpenProject { query: String },
    /// Projektordner im Dateimanager öffnen.
    OpenFolder { query: String },
    /// http(s)-Adresse im Browser öffnen.
    OpenUrl { url: String },
    /// Websuche.
    WebSearch { query: String },
    /// Bildschirm sperren.
    Lock {},
    /// Lautstärke setzen (`set`), ändern (`step`) oder stumm schalten (`mute`); genau eines davon.
    Volume {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<i8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mute: Option<bool>,
    },
    /// Medientaste senden.
    Media { action: MediaKey },
    /// Eingebautes Fenster oder eingebaute Anzeige zeigen.
    ShowWidget { widget: Widget },
    /// Einstellungsfenster öffnen, optional auf einer Seite.
    DictateSettings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<SettingsPage>,
    },
    /// Bildschirmaufnahme starten.
    Screenshot { mode: ScreenshotMode },
    /// Abmelden. Braucht Bestätigung.
    Logout {},
    /// Neustart. Braucht Bestätigung.
    Reboot {},
    /// Ausschalten. Braucht Bestätigung.
    Poweroff {},
    /// Bereitschaft. Braucht Bestätigung.
    Suspend {},
    /// Allgemeine Frage ans Modell; die Antwort erscheint im Panel.
    Ask { question: String },
    /// Eingebaute Textaktion auf der Markierung (Name aus `actions::BUILTIN`).
    TextAction { action: String },
    /// Inhalt der Zwischenablage erklären (liest CLIPBOARD, nur auf diese Absicht, mit Filter und Vorschau).
    ExplainClipboard {},
    /// Diagnose-Panel öffnen (`PLAN-diagnose.md`, Aufgabe 9), optional mit vorbelegtem Profil. `unit` und `container`
    /// sind Suchbegriffe: das Panel und der Sammler lösen sie gegen die echten Listen auf (nie ein Text des Modells im
    /// argv). Die Palette führt nie selbst eine Probe aus.
    Diagnose {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<DiagProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        container: Option<String>,
    },
}

impl Intent {
    /// Name der Absicht (Wert von `intent` auf dem Draht, Schlüssel im Katalog).
    pub fn name(&self) -> &'static str {
        match self {
            Intent::ViewWorkspace { .. } => "view_workspace",
            Intent::FocusWindow { .. } => "focus_window",
            Intent::MoveWindowToWorkspace { .. } => "move_window_to_workspace",
            Intent::ColumnDisplay { .. } => "column_display",
            Intent::ToggleWindow { .. } => "toggle_window",
            Intent::CloseWindow { .. } => "close_window",
            Intent::PreviousWorkspace {} => "previous_workspace",
            Intent::LaunchApp { .. } => "launch_app",
            Intent::OpenProject { .. } => "open_project",
            Intent::OpenFolder { .. } => "open_folder",
            Intent::OpenUrl { .. } => "open_url",
            Intent::WebSearch { .. } => "web_search",
            Intent::Lock {} => "lock",
            Intent::Volume { .. } => "volume",
            Intent::Media { .. } => "media",
            Intent::ShowWidget { .. } => "show_widget",
            Intent::DictateSettings { .. } => "dictate_settings",
            Intent::Screenshot { .. } => "screenshot",
            Intent::Logout {} => "logout",
            Intent::Reboot {} => "reboot",
            Intent::Poweroff {} => "poweroff",
            Intent::Suspend {} => "suspend",
            Intent::Ask { .. } => "ask",
            Intent::TextAction { .. } => "text_action",
            Intent::ExplainClipboard {} => "explain_clipboard",
            Intent::Diagnose { .. } => "diagnose",
        }
    }

    /// Eintrag im Katalog. Jede Absicht hat einen (Test `every_intent_has_a_catalog_entry`).
    pub fn spec(&self) -> &'static IntentSpec {
        catalog::spec(self.name()).expect("every intent has a catalog entry (checked by a test)")
    }

    /// Klasse: sofort ausführen, bestätigen oder Antwort der KI.
    pub fn class(&self) -> Class {
        self.spec().class
    }

    /// Gruppe im Katalog.
    pub fn group(&self) -> Group {
        self.spec().group
    }

    /// Prüft eine selbst gebaute Absicht (z. B. vom Resolver) gegen dieselben Regeln wie [`parse_validated`], nur
    /// ohne Trimmen: Strings müssen bereits getrimmt sein.
    pub fn validate(&self) -> Result<(), IntentError> {
        let value = serde_json::to_value(self).map_err(|e| IntentError::Malformed(e.to_string()))?;
        match value {
            Value::Object(map) => self.spec().check(&map)?,
            _ => return Err(IntentError::NotObject),
        }
        self.check_rules()
    }

    /// Regeln über mehrere Felder oder mit Wissen außerhalb des Katalogs, die das Schema nicht ausdrückt
    /// (`diagnose`: höchstens einer der Suchbegriffe, passend zum Profil, Zeichen und Länge wie die Suchbegriffe der
    /// Proben). Läuft nach der Prüfung der einzelnen Felder.
    pub(crate) fn check_rules(&self) -> Result<(), IntentError> {
        let Intent::Diagnose { profile, unit, container } = self else { return Ok(()) };
        let combination = |reason: &str| Err(IntentError::Combination { intent: "diagnose", reason: reason.to_string() });
        let term = |t: Term, field: &'static str, value: &Option<String>| match value {
            Some(v) => t.check(v).map_err(|reason| IntentError::InvalidField { intent: "diagnose", field, reason }),
            None => Ok(()),
        };
        if unit.is_some() && container.is_some() {
            return combination("give at most one of unit, container");
        }
        if unit.is_some() && !matches!(profile, None | Some(DiagProfile::Unit)) {
            return combination("unit goes with the profile unit (or no profile)");
        }
        if container.is_some() && !matches!(profile, None | Some(DiagProfile::Docker)) {
            return combination("container goes with the profile docker (or no profile)");
        }
        term(Term::Unit, "unit", unit)?;
        term(Term::Container, "container", container)
    }

    /// Das Profil, das das Panel vorbelegt: das gegebene, sonst `unit` zu einer Unit, `docker` zu einem Container.
    pub fn diagnose_profile(&self) -> Option<DiagProfile> {
        match self {
            Intent::Diagnose { profile, unit, container } => {
                profile.or(unit.as_ref().map(|_| DiagProfile::Unit)).or(container.as_ref().map(|_| DiagProfile::Docker))
            }
            _ => None,
        }
    }
}

/// Liest eine Absicht aus JSON (Antwort des Modells oder der Eingabe des Resolvers) und prüft sie vollständig:
/// bekannte Absicht, keine unbekannten Felder, alle Pflichtfelder, Typen, Bereiche, Längen. Strings werden vorher
/// getrimmt; der Rest wird nie stillschweigend korrigiert.
pub fn parse_validated(json: &str) -> Result<Intent, IntentError> {
    if json.len() > MAX_JSON_BYTES {
        return Err(IntentError::TooLong { bytes: json.len(), max: MAX_JSON_BYTES });
    }
    let value: Value = serde_json::from_str(json).map_err(|e| IntentError::Json(e.to_string()))?;
    let Value::Object(mut map) = value else {
        return Err(IntentError::NotObject);
    };
    let name = match map.get("intent") {
        None => return Err(IntentError::MissingIntent),
        Some(Value::String(name)) => name.clone(),
        Some(_) => return Err(IntentError::MissingIntent),
    };
    let spec = catalog::spec(&name).ok_or(IntentError::UnknownIntent(name))?;
    spec.normalize(&mut map);
    spec.check(&map)?;
    let intent: Intent = serde_json::from_value(Value::Object(map)).map_err(|e| IntentError::Malformed(e.to_string()))?;
    intent.check_rules()?;
    Ok(intent)
}

/// Warum eine Absicht abgelehnt wurde; die Meldung nennt Ursache und Feld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentError {
    /// Mehr als [`MAX_JSON_BYTES`].
    TooLong { bytes: usize, max: usize },
    /// Kein gültiges JSON.
    Json(String),
    /// Das JSON ist kein Objekt.
    NotObject,
    /// `intent` fehlt oder ist kein String.
    MissingIntent,
    /// Der Name steht nicht im Katalog.
    UnknownIntent(String),
    /// Ein Feld, das die Absicht nicht kennt.
    UnknownField { intent: &'static str, field: String },
    /// Ein Pflichtfeld fehlt.
    MissingField { intent: &'static str, field: &'static str },
    /// Falscher Typ, Bereich, Länge oder Inhalt eines Feldes.
    InvalidField { intent: &'static str, field: &'static str, reason: String },
    /// Regel über mehrere Felder verletzt (z. B. bei `volume` genau eines von `set`, `step`, `mute`).
    Combination { intent: &'static str, reason: String },
    /// Die Prüfung ließ etwas durch, was die Typen nicht lesen können; ein Fehler im Katalog, nicht in der Eingabe
    /// (der Test `examples_are_valid_for_schema_and_validation` verhindert das).
    Malformed(String),
}

/// Höchstens so viele Zeichen eines fremden Namens in Meldungen, damit eine Modellantwort kein Log füllt.
const SHOWN_CHARS: usize = 40;

fn shown(s: &str) -> String {
    let cut: String = s.chars().take(SHOWN_CHARS).collect();
    if s.chars().count() > SHOWN_CHARS {
        format!("{cut:?}...")
    } else {
        format!("{cut:?}")
    }
}

impl fmt::Display for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntentError::TooLong { bytes, max } => write!(f, "intent JSON is too long ({bytes} bytes, at most {max})"),
            IntentError::Json(e) => write!(f, "intent is not valid JSON: {e}"),
            IntentError::NotObject => write!(f, "intent must be a JSON object"),
            IntentError::MissingIntent => write!(f, "missing \"intent\" (a string naming the intent)"),
            IntentError::UnknownIntent(name) => write!(f, "unknown intent {}", shown(name)),
            IntentError::UnknownField { intent, field } => write!(f, "unknown field {} for intent {intent}", shown(field)),
            IntentError::MissingField { intent, field } => write!(f, "missing field \"{field}\" for intent {intent}"),
            IntentError::InvalidField { intent, field, reason } => write!(f, "field \"{field}\" of intent {intent} {reason}"),
            IntentError::Combination { intent, reason } => write!(f, "intent {intent}: {reason}"),
            IntentError::Malformed(e) => write!(f, "intent does not match its type: {e}"),
        }
    }
}

impl std::error::Error for IntentError {}

/// Hilfsfunktion für den Katalog: Name des JSON-Typs für Meldungen.
pub(crate) fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(json: &str) -> Intent {
        match parse_validated(json) {
            Ok(i) => i,
            Err(e) => panic!("{json} should be valid: {e}"),
        }
    }

    fn err(json: &str) -> String {
        match parse_validated(json) {
            Ok(i) => panic!("{json} should be rejected, got {i:?}"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn parses_typed_values() {
        assert_eq!(ok(r#"{"intent":"view_workspace","workspace":3}"#), Intent::ViewWorkspace { workspace: WorkspaceRef::Index(3) });
        assert_eq!(ok(r#"{"intent":"view_workspace","workspace":"web"}"#), Intent::ViewWorkspace { workspace: WorkspaceRef::Name("web".into()) });
        assert_eq!(ok(r#"{"intent":"lock"}"#), Intent::Lock {});
        assert_eq!(ok(r#"{"intent":"volume","step":-10}"#), Intent::Volume { set: None, step: Some(-10), mute: None });
        assert_eq!(ok(r#"{"intent":"toggle_window","what":"maximized"}"#), Intent::ToggleWindow { what: WindowToggle::Maximized });
        assert_eq!(ok(r#"{"intent":"column_display","display":"tabbed"}"#), Intent::ColumnDisplay { display: ColumnDisplay::Tabbed });
        assert_eq!(ok(r#"{"intent":"dictate_settings"}"#), Intent::DictateSettings { page: None });
    }

    #[test]
    fn strings_are_trimmed_on_parse_but_not_in_validate() {
        let i = ok("{\"intent\":\"focus_window\",\"query\":\"  fire fox\\n\"}");
        assert_eq!(i, Intent::FocusWindow { query: "fire fox".into() });
        assert!(i.validate().is_ok());
        let untrimmed = Intent::FocusWindow { query: " firefox".into() };
        assert!(untrimmed.validate().unwrap_err().to_string().contains("whitespace"));
        // Auch der Name einer Arbeitsfläche und eine URL werden getrimmt
        assert_eq!(ok(r#"{"intent":"view_workspace","workspace":" web "}"#), Intent::ViewWorkspace { workspace: WorkspaceRef::Name("web".into()) });
        assert_eq!(ok(r#"{"intent":"open_url","url":" https://example.com/a "}"#), Intent::OpenUrl { url: "https://example.com/a".into() });
        // Ein Aufzählungswert wird nicht geraten
        assert!(err(r#"{"intent":"toggle_window","what":" floating"}"#).contains("must be one of"));
    }

    #[test]
    fn validate_rejects_hand_built_intents_outside_the_rules() {
        assert!(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(255) }.validate().is_ok());
        assert!(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(0) }.validate().unwrap_err().to_string().contains("between 1 and 255"));
        assert!(Intent::Volume { set: Some(200), step: None, mute: None }.validate().unwrap_err().to_string().contains("between 0 and 100"));
        assert!(Intent::Volume { set: None, step: Some(0), mute: None }.validate().is_err());
        assert!(Intent::Volume { set: Some(10), step: Some(5), mute: None }.validate().unwrap_err().to_string().contains("exactly one"));
        assert!(Intent::Volume { set: None, step: None, mute: None }.validate().is_err());
        assert!(Intent::Ask { question: "x".repeat(1001) }.validate().is_err());
        assert!(Intent::Ask { question: "x".repeat(1000) }.validate().is_ok());
        assert!(Intent::TextAction { action: "nope".into() }.validate().is_err());
        assert!(Intent::TextAction { action: "explain".into() }.validate().is_ok());
        assert!(Intent::OpenUrl { url: "file:///etc/passwd".into() }.validate().is_err());
    }

    #[test]
    fn errors_name_the_cause() {
        assert!(err("not json").contains("not valid JSON"));
        assert!(err("[]").contains("JSON object"));
        assert!(err(r#"{"workspace":3}"#).contains("missing \"intent\""));
        assert!(err(r#"{"intent":3}"#).contains("missing \"intent\""));
        assert!(err(r#"{"intent":"format_disk"}"#).contains("unknown intent \"format_disk\""));
        assert!(err(r#"{"intent":"view_workspace","workspace":3,"force":true}"#).contains("unknown field \"force\" for intent view_workspace"));
        assert!(err(r#"{"intent":"lock","now":true}"#).contains("unknown field \"now\""));
        assert!(err(r#"{"intent":"view_workspace"}"#).contains("missing field \"workspace\""));
        assert!(err(r#"{"intent":"view_workspace","workspace":256}"#).contains("field \"workspace\" of intent view_workspace must be between 1 and 255"));
        // die Absichten von Awesome gibt es nicht mehr
        for old in ["view_tag", "move_window_to_tag", "previous_tag", "set_layout"] {
            assert!(err(&format!(r#"{{"intent":"{old}"}}"#)).contains("unknown intent"), "{old}");
        }
        assert!(err(r#"{"intent":"toggle_window","what":"sticky"}"#).contains("must be one of"));
        assert!(err(r#"{"intent":"toggle_window","what":"ontop"}"#).contains("must be one of"));
        assert!(err(r#"{"intent":"show_widget","widget":"claude_usage"}"#).contains("must be one of"));
        assert!(err(r#"{"intent":"volume"}"#).contains("exactly one of set, step, mute"));
    }

    #[test]
    fn rejects_oversized_input_and_shortens_foreign_names() {
        let big = format!(r#"{{"intent":"ask","question":"{}"}}"#, "x".repeat(MAX_JSON_BYTES));
        assert!(err(&big).contains("too long"));
        let long_name = format!(r#"{{"intent":"{}"}}"#, "a".repeat(500));
        let msg = err(&long_name);
        assert!(msg.len() < 120, "message echoes too much: {msg}");
        assert!(msg.contains("..."));
    }

    #[test]
    fn null_is_not_a_missing_field() {
        assert!(err(r#"{"intent":"close_window","query":null}"#).contains("must be a string"));
        assert!(err(r#"{"intent":"volume","step":5,"mute":null}"#).contains("must be true or false"));
    }

    #[test]
    fn round_trips_through_json() {
        let all = [
            Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(2), query: Some("kitty".into()) },
            Intent::CloseWindow { query: None },
            Intent::Volume { set: Some(0), step: None, mute: None },
            Intent::Screenshot { mode: ScreenshotMode::Area },
            Intent::PreviousWorkspace {},
            Intent::ColumnDisplay { display: ColumnDisplay::Normal },
        ];
        for i in all {
            let json = serde_json::to_string(&i).unwrap();
            assert_eq!(parse_validated(&json).unwrap(), i, "{json}");
        }
        assert_eq!(serde_json::to_value(Intent::CloseWindow { query: None }).unwrap(), json!({"intent": "close_window"}));
    }

    #[test]
    fn name_enums_agree_with_serde() {
        fn check<T: Serialize + Copy>(all: &[T], names: &[&str], name: impl Fn(T) -> &'static str) {
            assert_eq!(all.len(), names.len());
            for (i, v) in all.iter().enumerate() {
                assert_eq!(serde_json::to_value(v).unwrap(), json!(names[i]));
                assert_eq!(name(*v), names[i]);
            }
        }
        check(WindowToggle::ALL, WindowToggle::NAMES, WindowToggle::name);
        check(ColumnDisplay::ALL, ColumnDisplay::NAMES, ColumnDisplay::name);
        check(MediaKey::ALL, MediaKey::NAMES, MediaKey::name);
        check(Widget::ALL, Widget::NAMES, Widget::name);
        check(ScreenshotMode::ALL, ScreenshotMode::NAMES, ScreenshotMode::name);
        check(SettingsPage::ALL, SettingsPage::NAMES, SettingsPage::name);
        check(DiagProfile::ALL, DiagProfile::NAMES, DiagProfile::name);
    }

    /// Die Profile der Palette sind genau die der Diagnose, in derselben Reihenfolge; ebenso die Grenze der Suchbegriffe.
    #[test]
    fn diagnose_profiles_and_terms_agree_with_the_diagnosis() {
        use crate::diagnose::catalog::Term;
        use crate::diagnose::profile::Profile;
        let diagnosis: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(diagnosis, DiagProfile::NAMES);
        assert_eq!(catalog::MAX_TERM, Term::Unit.max_len());
        assert_eq!(catalog::MAX_TERM, Term::Container.max_len());
        // Ein Name, den die Probe annimmt, nimmt auch die Absicht an (und umgekehrt), für Zeichen und Anfang
        for name in ["cups", "docker.service", "user@1000.service", "a_b-c.d", "app\\x2dfoo.service", "1password", "web-1", "x".repeat(128).as_str()] {
            let unit = Intent::Diagnose { profile: None, unit: Some(name.into()), container: None };
            assert_eq!(unit.validate().is_ok(), Term::Unit.check(name).is_ok(), "{name}");
            let container = Intent::Diagnose { profile: None, unit: None, container: Some(name.into()) };
            assert_eq!(container.validate().is_ok(), Term::Container.check(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn diagnose_rules_across_fields() {
        let d = |profile: Option<DiagProfile>, unit: Option<&str>, container: Option<&str>| Intent::Diagnose {
            profile,
            unit: unit.map(Into::into),
            container: container.map(Into::into),
        };
        for ok in [
            d(None, None, None),
            d(Some(DiagProfile::Heat), None, None),
            d(None, Some("cups"), None),
            d(Some(DiagProfile::Unit), Some("cups"), None),
            d(None, None, Some("web")),
            d(Some(DiagProfile::Docker), None, Some("web")),
        ] {
            assert!(ok.validate().is_ok(), "{ok:?}: {:?}", ok.validate());
        }
        for (bad, msg) in [
            (d(None, Some("cups"), Some("web")), "at most one of unit, container"),
            (d(Some(DiagProfile::Unit), Some("cups"), Some("web")), "at most one of unit, container"),
            (d(Some(DiagProfile::Now), Some("cups"), None), "unit goes with the profile unit"),
            (d(Some(DiagProfile::Docker), Some("cups"), None), "unit goes with the profile unit"),
            (d(Some(DiagProfile::Unit), None, Some("web")), "container goes with the profile docker"),
            (d(Some(DiagProfile::Heat), None, Some("web")), "container goes with the profile docker"),
            (d(None, Some("-x"), None), "field \"unit\" of intent diagnose must start with a letter or digit"),
            (d(None, Some("a b"), None), "may only contain"),
            (d(None, None, Some("--all")), "field \"container\" of intent diagnose must start with a letter or digit"),
            (d(None, None, Some("a/b")), "may only contain"),
            (d(None, Some(" cups"), None), "whitespace"),
        ] {
            let e = bad.validate().expect_err(&format!("{bad:?}")).to_string();
            assert!(e.contains(msg), "{bad:?}: {e}");
        }
        // parse_validated trimmt den Begriff und wendet dieselben Regeln an
        assert_eq!(ok(r#"{"intent":"diagnose","unit":"  cups "}"#), d(None, Some("cups"), None));
        assert!(err(r#"{"intent":"diagnose","unit":"cups","container":"web"}"#).contains("at most one of"));
        assert!(err(r#"{"intent":"diagnose","unit":"a b"}"#).contains("may only contain"));
    }

    #[test]
    fn diagnose_profile_is_the_given_one_or_follows_the_term() {
        let d = |profile: Option<DiagProfile>, unit: Option<&str>, container: Option<&str>| Intent::Diagnose {
            profile,
            unit: unit.map(Into::into),
            container: container.map(Into::into),
        };
        assert_eq!(d(None, None, None).diagnose_profile(), None);
        assert_eq!(d(Some(DiagProfile::Network), None, None).diagnose_profile(), Some(DiagProfile::Network));
        assert_eq!(d(None, Some("cups"), None).diagnose_profile(), Some(DiagProfile::Unit));
        assert_eq!(d(None, None, Some("web")).diagnose_profile(), Some(DiagProfile::Docker));
        assert_eq!(Intent::Lock {}.diagnose_profile(), None);
        // auf dem Draht fehlen leere Felder
        assert_eq!(serde_json::to_value(d(None, None, None)).unwrap(), json!({"intent": "diagnose"}));
        assert_eq!(
            serde_json::to_value(d(Some(DiagProfile::Docker), None, Some("web"))).unwrap(),
            json!({"intent": "diagnose", "profile": "docker", "container": "web"})
        );
    }

    #[test]
    fn text_actions_come_from_the_builtin_table() {
        assert_eq!(TEXT_ACTION_NAMES.len(), BUILTIN.len());
        for (i, b) in BUILTIN.iter().enumerate() {
            assert_eq!(TEXT_ACTION_NAMES[i], b.name);
            assert!(ok(&format!(r#"{{"intent":"text_action","action":"{}"}}"#, b.name)).validate().is_ok());
        }
    }

    /// Jede Seite des Einstellungsfensters ist über die Palette erreichbar und jeder Name dort gültig. `gui::Page` gibt es
    /// nur im Binary `dictate-settings` (das die Whisper-Bibliothek bewusst nicht mitlinkt), nicht in der lib; der Test
    /// liest deshalb die Seitenbezeichnungen aus dem Quelltext und bildet den Namen wie `Page::from_arg` (erstes Wort,
    /// klein). Kommt eine Seite hinzu, schlägt er an und `SettingsPage` muss mitwachsen.
    #[test]
    fn settings_pages_match_the_settings_window() {
        let src = include_str!("../gui/mod.rs");
        let body = src.split("pub fn label(self)").nth(1).expect("Page::label in src/gui/mod.rs");
        let body = &body[..body.find("\n    }\n").expect("end of Page::label")];
        let mut args: Vec<String> = Vec::new();
        for line in body.lines() {
            let Some((_, label)) = line.trim().strip_prefix("Page::").and_then(|rest| rest.split_once("=> \"")) else { continue };
            let label = label.split('"').next().unwrap_or_default();
            args.push(label.to_lowercase().split(' ').next().unwrap_or_default().to_string());
        }
        assert!(args.len() >= 12, "could not read the pages from src/gui/mod.rs: {args:?}");
        assert_eq!(args, SettingsPage::NAMES, "SettingsPage and gui::Page differ");
    }
}
