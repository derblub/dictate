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

//! Kontext des Resolvers: was die Oberfläche über Arbeitsflächen und Fenster von niri weiß (Vorlage E2 d in
//! `PLAN-wayland.md` 6g).
//!
//! `dictate-ui` baut ihn beim Öffnen der Palette aus niri-IPC (`Workspaces`, `Windows`, ohne das eigene Panel) und
//! schickt ihn im Feld `context` von `pal_resolve`/`pal_ask` (nie per argv). Er lebt nur für eine Anfrage im Speicher
//! und wird nie geloggt oder an ein Modell geschickt (`PLAN-palette.md`, Entscheidungen).
//!
//! **Form:** `{"workspaces": [{"id", "idx", "name", "output", "active", "focused"}], "windows": [{"id", "app_id",
//! "title", "workspace_id", "focused"}], "focus": id}`; alle Felder außer den IDs und `idx` sind optional, `null` gilt als
//! nicht angegeben. Die Felder entsprechen `niri_ipc::Workspace` (`is_active`, `is_focused`) und `niri_ipc::Window`.
//! `idx` zählt je Monitor (`output`) ab 1; niri führt am Ende jedes Monitors eine leere Arbeitsfläche, die in der Liste
//! mitkommt (Spike in `docs/spikes/wayland-spawn.md`). `focused` einer Arbeitsfläche bestimmt den Monitor, wenn kein
//! Fenster fokussiert ist (Lesart L9 in 6g).
//!
//! **Grenzen** (der Client kürzt vor dem Senden, Rust prüft und lehnt Überschreitungen ab, statt still zu kürzen):
//! höchstens [`MAX_WINDOWS`] Fenster und [`MAX_WORKSPACES`] Arbeitsflächen; Titel höchstens [`MAX_TITLE_CHARS`] Zeichen,
//! `app_id` [`MAX_APP_ID_CHARS`], Name und Monitor einer Arbeitsfläche [`MAX_NAME_CHARS`]; das JSON höchstens
//! [`MAX_CONTEXT_BYTES`]. IDs sind einmalig, `idx` je Monitor ebenfalls. Ein Fenster, dessen `workspace_id` in der Liste
//! fehlt (zwei IPC-Abfragen sind nicht atomar), gilt als „ohne Arbeitsfläche“ statt den Kontext abzulehnen.
//! Steuerzeichen in Titeln sind erlaubt (Terminals setzen sie); angezeigt werden sie als Leerzeichen.
//!
//! **Meldungen ohne Werte** (Sicherheitsregel 5): bei falschem Typ, fehlendem oder unbekanntem Feld nennt
//! [`ContextError::Shape`] nur den Pfad aus bekannten Feldnamen und Indizes und den erwarteten Typ, nie den Wert und nie
//! den fremden Schlüssel (serde zitierte beides). Darum prüft [`Context::from_json`] die Form selbst, bevor serde liest.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fmt;

pub const MAX_WINDOWS: usize = 500;
pub const MAX_WORKSPACES: usize = 256;
pub const MAX_TITLE_CHARS: usize = 512;
pub const MAX_APP_ID_CHARS: usize = 128;
pub const MAX_NAME_CHARS: usize = 80;
/// 500 Titel zu 512 Zeichen (bis 4 Bytes je Zeichen) und etwas Rahmen.
pub const MAX_CONTEXT_BYTES: usize = 2 * 1024 * 1024;

/// Eine Arbeitsfläche von niri.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInfo {
    /// Feste ID (`niri_ipc::Workspace::id`); das Ziel jeder Absicht auf einer Arbeitsfläche, nie `idx` oder Name.
    pub id: u64,
    /// Position auf dem Monitor ab 1 ([`crate::palette::catalog::MAX_WORKSPACE_INDEX`] ist die größte).
    pub idx: u8,
    /// Name aus der niri-Config, falls benannt.
    #[serde(default)]
    pub name: Option<String>,
    /// Name des Monitors (`DP-1`); fehlt, wenn die Arbeitsfläche auf keinem liegt.
    #[serde(default)]
    pub output: Option<String>,
    /// Auf ihrem Monitor gerade sichtbar?
    #[serde(default)]
    pub active: bool,
    /// Die fokussierte Arbeitsfläche (höchstens eine)?
    #[serde(default)]
    pub focused: bool,
}

impl WorkspaceInfo {
    /// Name, falls nicht leer.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref().filter(|n| !n.trim().is_empty())
    }

    /// Monitor; leer, wenn keiner.
    pub fn output(&self) -> &str {
        self.output.as_deref().unwrap_or("")
    }
}

/// Ein Fenster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowInfo {
    /// Feste ID (`niri_ipc::Window::id`); das Ziel jeder Fensterabsicht, nie ein Name.
    pub id: u64,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Arbeitsfläche, auf der das Fenster liegt.
    #[serde(default)]
    pub workspace_id: Option<u64>,
    #[serde(default)]
    pub focused: bool,
}

impl WindowInfo {
    pub fn app_id(&self) -> &str {
        self.app_id.as_deref().unwrap_or("")
    }

    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }
}

/// Alles, was der Resolver vom Desktop wissen muss.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceInfo>,
    /// Reihenfolge wie vom Client gesendet (am besten nach zuletzt fokussiert): bei gleicher Güte steht der frühere vorn.
    #[serde(default)]
    pub windows: Vec<WindowInfo>,
    /// ID des fokussierten Fensters; fehlt sie, gilt das Fenster mit `focused: true`.
    #[serde(default)]
    pub focus: Option<u64>,
}

/// Warum ein Kontext abgelehnt wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// JSON größer als [`MAX_CONTEXT_BYTES`].
    TooLarge {
        bytes: usize,
    },
    /// Kein gültiges JSON (nur Zeile und Spalte).
    Syntax {
        line: usize,
        column: usize,
    },
    /// Falsche Form: `path` aus bekannten Feldnamen und Indizes (`windows[3].title`), `problem` ohne Wert.
    Shape {
        path: String,
        problem: &'static str,
    },
    TooMany {
        what: &'static str,
        count: usize,
        max: usize,
    },
    TooLong {
        what: &'static str,
        chars: usize,
        max: usize,
    },
    /// `idx` 0 (niri zählt ab 1).
    BadWorkspaceIndex,
    DuplicateWorkspace(u64),
    /// Zwei Arbeitsflächen mit demselben `idx` auf demselben Monitor.
    DuplicateWorkspaceIndex(u8),
    DuplicateWindow(u64),
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextError::TooLarge { bytes } => write!(f, "context is too large ({bytes} bytes, at most {MAX_CONTEXT_BYTES})"),
            ContextError::Syntax { line, column } => write!(f, "context is not valid JSON (line {line}, column {column})"),
            ContextError::Shape { path, problem } => write!(f, "context is not valid: {path} {problem}"),
            ContextError::TooMany { what, count, max } => write!(f, "context has too many {what} ({count}, at most {max})"),
            ContextError::TooLong { what, chars, max } => write!(f, "{what} is too long ({chars} characters, at most {max})"),
            ContextError::BadWorkspaceIndex => write!(f, "workspace idx must be at least 1"),
            ContextError::DuplicateWorkspace(id) => write!(f, "workspace id {id} appears twice"),
            ContextError::DuplicateWorkspaceIndex(n) => write!(f, "workspace idx {n} appears twice on one output"),
            ContextError::DuplicateWindow(id) => write!(f, "window id {id} appears twice"),
        }
    }
}

impl std::error::Error for ContextError {}

// ---------------------------------------------------------------------------------------------------------------
// Prüfung der Form vor serde

/// Erwarteter Typ eines Feldes.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// Ganzzahl 0 bis `u64::MAX`.
    Id,
    /// Ganzzahl 0 bis 255.
    Small,
    Str,
    Bool,
    /// Liste von Objekten mit diesen Feldern.
    Objects(&'static [Field]),
}

/// Ein Feld eines Objekts: Name, Typ, Pflicht.
#[derive(Debug, Clone, Copy)]
struct Field(&'static str, Kind, bool);

const WORKSPACE_FIELDS: &[Field] = &[
    Field("id", Kind::Id, true),
    Field("idx", Kind::Small, true),
    Field("name", Kind::Str, false),
    Field("output", Kind::Str, false),
    Field("active", Kind::Bool, false),
    Field("focused", Kind::Bool, false),
];
const WINDOW_FIELDS: &[Field] = &[
    Field("id", Kind::Id, true),
    Field("app_id", Kind::Str, false),
    Field("title", Kind::Str, false),
    Field("workspace_id", Kind::Id, false),
    Field("focused", Kind::Bool, false),
];
const CONTEXT_FIELDS: &[Field] =
    &[Field("workspaces", Kind::Objects(WORKSPACE_FIELDS), false), Field("windows", Kind::Objects(WINDOW_FIELDS), false), Field("focus", Kind::Id, false)];

fn shape(path: impl Into<String>, problem: &'static str) -> ContextError {
    ContextError::Shape { path: path.into(), problem }
}

/// Prüft ein Objekt gegen `fields`; `path` ist der Pfad des Objekts (leer = oberste Ebene).
fn check_object(map: &Map<String, Value>, fields: &[Field], path: &str) -> Result<(), ContextError> {
    let at = |name: &str| if path.is_empty() { name.to_string() } else { format!("{path}.{name}") };
    if map.keys().any(|k| !fields.iter().any(|f| f.0 == k)) {
        // der fremde Schlüssel ist Eingabe und bleibt ungenannt
        return Err(shape(if path.is_empty() { "context".to_string() } else { path.to_string() }, "has an unknown field"));
    }
    for &Field(name, kind, required) in fields {
        let v = match map.get(name) {
            None | Some(Value::Null) if required => return Err(shape(at(name), "is missing")),
            None | Some(Value::Null) => continue,
            Some(v) => v,
        };
        let problem = match kind {
            Kind::Id => (!v.is_u64()).then_some("must be a non-negative integer"),
            Kind::Small => v.as_u64().is_none_or(|n| n > u64::from(u8::MAX)).then_some("must be an integer from 0 to 255"),
            Kind::Str => (!v.is_string()).then_some("must be a string"),
            Kind::Bool => (!v.is_boolean()).then_some("must be true or false"),
            Kind::Objects(inner) => {
                let Some(list) = v.as_array() else { return Err(shape(at(name), "must be an array of objects")) };
                for (i, item) in list.iter().enumerate() {
                    let item_path = format!("{}[{i}]", at(name));
                    let Some(obj) = item.as_object() else { return Err(shape(item_path, "must be an object")) };
                    check_object(obj, inner, &item_path)?;
                }
                None
            }
        };
        if let Some(problem) = problem {
            return Err(shape(at(name), problem));
        }
    }
    Ok(())
}

fn check_len(what: &'static str, text: &str, max: usize) -> Result<(), ContextError> {
    let chars = text.chars().count();
    if chars > max {
        return Err(ContextError::TooLong { what, chars, max });
    }
    Ok(())
}

impl Context {
    /// Liest und prüft einen Kontext aus JSON. Unbekannte Felder sind ein Fehler; Meldungen ohne Werte (Modulkopf).
    pub fn from_json(json: &str) -> Result<Context, ContextError> {
        if json.len() > MAX_CONTEXT_BYTES {
            return Err(ContextError::TooLarge { bytes: json.len() });
        }
        let value: Value = serde_json::from_str(json).map_err(|e| ContextError::Syntax { line: e.line(), column: e.column() })?;
        let Some(map) = value.as_object() else { return Err(shape("context", "must be an object")) };
        check_object(map, CONTEXT_FIELDS, "")?;
        // nach der Prüfung oben kann serde nicht scheitern; falls doch, ohne seine Meldung (sie zitierte den Wert)
        let ctx: Context = serde_json::from_value(value).map_err(|_| shape("context", "does not match its type"))?;
        ctx.validate()?;
        Ok(ctx)
    }

    /// Prüft die Grenzen (siehe Modulkopf).
    pub fn validate(&self) -> Result<(), ContextError> {
        if self.workspaces.len() > MAX_WORKSPACES {
            return Err(ContextError::TooMany { what: "workspaces", count: self.workspaces.len(), max: MAX_WORKSPACES });
        }
        if self.windows.len() > MAX_WINDOWS {
            return Err(ContextError::TooMany { what: "windows", count: self.windows.len(), max: MAX_WINDOWS });
        }
        let mut ids = HashSet::new();
        let mut places = HashSet::new();
        for w in &self.workspaces {
            if w.idx == 0 {
                return Err(ContextError::BadWorkspaceIndex);
            }
            if !ids.insert(w.id) {
                return Err(ContextError::DuplicateWorkspace(w.id));
            }
            if !places.insert((w.output(), w.idx)) {
                return Err(ContextError::DuplicateWorkspaceIndex(w.idx));
            }
            check_len("workspace name", w.name.as_deref().unwrap_or(""), MAX_NAME_CHARS)?;
            check_len("workspace output", w.output(), MAX_NAME_CHARS)?;
        }
        let mut ids = HashSet::new();
        for c in &self.windows {
            if !ids.insert(c.id) {
                return Err(ContextError::DuplicateWindow(c.id));
            }
            check_len("window title", c.title(), MAX_TITLE_CHARS)?;
            check_len("window app_id", c.app_id(), MAX_APP_ID_CHARS)?;
        }
        Ok(())
    }

    /// Das fokussierte Fenster: das mit der ID aus `focus`, sonst das erste mit `focused: true`.
    pub fn focused_window(&self) -> Option<&WindowInfo> {
        self.focus.and_then(|id| self.windows.iter().find(|c| c.id == id)).or_else(|| self.windows.iter().find(|c| c.focused))
    }

    /// Arbeitsfläche zu einer ID.
    pub fn workspace(&self, id: u64) -> Option<&WorkspaceInfo> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// Arbeitsfläche eines Fensters (`None`, wenn ohne oder unbekannt).
    pub fn workspace_of(&self, window: &WindowInfo) -> Option<&WorkspaceInfo> {
        window.workspace_id.and_then(|id| self.workspace(id))
    }

    /// Der Monitor, auf den sich eine Zahl bezieht, wenn kein bestimmtes Fenster gemeint ist: der des fokussierten
    /// Fensters, sonst der der fokussierten Arbeitsfläche, sonst der einzige Monitor. `None` = nicht bestimmbar (dann
    /// trifft keine Zahl; ein Name trifft weiter).
    pub fn focused_output(&self) -> Option<&str> {
        match self.focused_window().and_then(|c| self.workspace_of(c)) {
            Some(ws) => Some(ws.output()),
            None => self.output_without_window(),
        }
    }

    /// Der Monitor, auf den sich eine Zahl für dieses Fenster bezieht: der seiner Arbeitsfläche, sonst wie ohne
    /// Fenster (die fokussierte Arbeitsfläche oder der einzige Monitor).
    pub fn output_for(&self, window: &WindowInfo) -> Option<&str> {
        match self.workspace_of(window) {
            Some(ws) => Some(ws.output()),
            None => self.output_without_window(),
        }
    }

    /// Monitor der fokussierten Arbeitsfläche, sonst der einzige Monitor.
    fn output_without_window(&self) -> Option<&str> {
        if let Some(ws) = self.workspaces.iter().find(|w| w.focused) {
            return Some(ws.output());
        }
        let mut outputs = self.workspaces.iter().map(WorkspaceInfo::output);
        let first = outputs.next()?;
        outputs.all(|o| o == first).then_some(first)
    }

    /// Index (in [`Self::workspaces`]) der Arbeitsfläche mit `idx` auf `output`.
    pub fn workspace_at(&self, output: &str, idx: u8) -> Option<usize> {
        self.workspaces.iter().position(|w| w.output() == output && w.idx == idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
        "workspaces": [
            {"id": 11, "idx": 1, "name": "web", "output": "DP-1", "active": true, "focused": true},
            {"id": 12, "idx": 2, "output": "DP-1"},
            {"id": 21, "idx": 1, "output": "HDMI-A-1", "active": true}
        ],
        "windows": [
            {"id": 7, "app_id": "firefox", "title": "Rust", "workspace_id": 11, "focused": true},
            {"id": 8, "app_id": "Alacritty"}
        ],
        "focus": 7
    }"#;

    fn shape_err(json: &str) -> String {
        match Context::from_json(json) {
            Err(e @ (ContextError::Shape { .. } | ContextError::Syntax { .. })) => e.to_string(),
            other => panic!("{json}: expected a shape error, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_full_context() {
        let ctx = Context::from_json(FULL).unwrap();
        assert_eq!(ctx.workspaces.len(), 3);
        assert!(ctx.workspaces[0].active && !ctx.workspaces[1].active);
        assert_eq!(ctx.workspaces[0].name(), Some("web"));
        assert_eq!(ctx.workspaces[1].name(), None);
        assert_eq!(ctx.windows[0].id, 7);
        assert_eq!(ctx.windows[1].workspace_id, None);
        assert_eq!(ctx.windows[1].title(), "");
        assert_eq!(ctx.focused_window().unwrap().app_id(), "firefox");
        assert_eq!(ctx.focused_output(), Some("DP-1"));
        assert_eq!(ctx.workspace_at("DP-1", 2), Some(1));
        assert_eq!(ctx.workspace_at("HDMI-A-1", 2), None);
    }

    #[test]
    fn empty_object_is_an_empty_context() {
        assert_eq!(Context::from_json("{}").unwrap(), Context::default());
        assert_eq!(Context::default().focused_output(), None);
    }

    #[test]
    fn ids_use_the_full_u64_range() {
        let ctx = Context::from_json(r#"{"windows":[{"id": 18446744073709551615, "workspace_id": 4294967296}]}"#).unwrap();
        assert_eq!(ctx.windows[0].id, u64::MAX);
        assert!(Context::from_json(r#"{"windows":[{"id": -1}]}"#).is_err());
    }

    #[test]
    fn null_means_not_given() {
        let ctx = Context::from_json(
            r#"{"workspaces":[{"id":1,"idx":1,"name":null,"output":null}],"windows":[{"id":2,"app_id":null,"workspace_id":null}],"focus":null}"#,
        )
        .unwrap();
        assert_eq!(ctx.workspaces[0].output(), "");
        assert_eq!(ctx.windows[0].app_id(), "");
        assert!(Context::from_json(r#"{"windows":[{"id":null}]}"#).is_err(), "IDs sind Pflicht");
    }

    #[test]
    fn unknown_fields_are_rejected_everywhere_without_naming_them() {
        for (json, path) in [
            (r#"{"bogus_key_m1": 1}"#, "context"),
            (r#"{"workspaces":[{"id":1,"idx":1,"bogus_key_m1":1}]}"#, "workspaces[0]"),
            (r#"{"windows":[{"id":1},{"id":2,"bogus_key_m1":1}]}"#, "windows[1]"),
            // die Felder von Awesome gibt es nicht mehr
            (r#"{"tags":[]}"#, "context"),
            (r#"{"windows":[{"id":1,"class":"x"}]}"#, "windows[0]"),
        ] {
            let e = shape_err(json);
            assert_eq!(e, format!("context is not valid: {path} has an unknown field"), "{json}");
            assert!(!e.contains("bogus"));
        }
    }

    /// Sicherheitsregel 5: die Meldung nennt Pfad und erwarteten Typ, nie den Wert (serde zitierte ihn).
    #[test]
    fn wrong_types_name_path_and_type_but_never_the_value() {
        const M: &str = "valuemarker-c5";
        for (json, want) in [
            (format!(r#"{{"windows":[{{"id":1,"title":["{M}"]}}]}}"#), "windows[0].title must be a string"),
            (format!(r#"{{"windows":[{{"id":1}},{{"id":"{M}"}}]}}"#), "windows[1].id must be a non-negative integer"),
            (format!(r#"{{"windows":[{{"id":1,"app_id":{{"{M}":1}}}}]}}"#), "windows[0].app_id must be a string"),
            (format!(r#"{{"windows":[{{"id":1,"focused":"{M}"}}]}}"#), "windows[0].focused must be true or false"),
            (format!(r#"{{"windows":[{{"id":1,"workspace_id":"{M}"}}]}}"#), "windows[0].workspace_id must be a non-negative integer"),
            (format!(r#"{{"workspaces":[{{"id":1,"idx":"{M}"}}]}}"#), "workspaces[0].idx must be an integer from 0 to 255"),
            (r#"{"workspaces":[{"id":1,"idx":300}]}"#.to_string(), "workspaces[0].idx must be an integer from 0 to 255"),
            (format!(r#"{{"workspaces":[{{"id":1,"idx":1,"name":7,"output":"{M}"}}]}}"#), "workspaces[0].name must be a string"),
            (format!(r#"{{"workspaces":[{{"idx":1,"name":"{M}"}}]}}"#), "workspaces[0].id is missing"),
            (format!(r#"{{"workspaces":"{M}"}}"#), "workspaces must be an array of objects"),
            (format!(r#"{{"windows":["{M}"]}}"#), "windows[0] must be an object"),
            (format!(r#"{{"windows":[[1,"{M}"]]}}"#), "windows[0] must be an object"),
            (format!(r#"{{"focus":"{M}"}}"#), "focus must be a non-negative integer"),
            (format!(r#"["{M}"]"#), "context must be an object"),
            (format!(r#"{{"windows":[{{"id":1,"title":5}}],"x":"{M}, expected a string"}}"#), "context has an unknown field"),
        ] {
            let e = shape_err(&json);
            assert_eq!(e, format!("context is not valid: {want}"), "{json}");
            assert!(!e.contains(M), "{e}");
        }
        for json in [format!(r#"{{"windows":[{{"id":1,"title":"{M}"}}"#), M.to_string()] {
            let e = shape_err(&json);
            assert!(e.starts_with("context is not valid JSON (line 1, column "), "{e}");
            assert!(!e.contains(M), "{e}");
        }
    }

    #[test]
    fn workspace_ids_and_places_are_unique() {
        assert_eq!(Context::from_json(r#"{"workspaces":[{"id":1,"idx":0}]}"#), Err(ContextError::BadWorkspaceIndex));
        assert_eq!(Context::from_json(r#"{"workspaces":[{"id":1,"idx":1},{"id":1,"idx":2}]}"#), Err(ContextError::DuplicateWorkspace(1)));
        assert_eq!(
            Context::from_json(r#"{"workspaces":[{"id":1,"idx":2,"output":"A"},{"id":2,"idx":2,"output":"A"}]}"#),
            Err(ContextError::DuplicateWorkspaceIndex(2))
        );
        assert!(Context::from_json(r#"{"workspaces":[{"id":1,"idx":2,"output":"A"},{"id":2,"idx":2,"output":"B"}]}"#).is_ok(), "idx zählt je Monitor");
        assert!(Context::from_json(r#"{"workspaces":[{"id":1,"idx":255}]}"#).is_ok());
    }

    #[test]
    fn window_ids_must_be_unique_and_unknown_workspaces_are_tolerated() {
        assert_eq!(Context::from_json(r#"{"windows":[{"id":7},{"id":7}]}"#), Err(ContextError::DuplicateWindow(7)));
        let ctx = Context::from_json(r#"{"workspaces":[{"id":1,"idx":1}],"windows":[{"id":7,"workspace_id":99}]}"#).unwrap();
        assert!(ctx.workspace_of(&ctx.windows[0]).is_none());
    }

    #[test]
    fn limits_are_checked() {
        let many: Vec<String> = (0..=MAX_WINDOWS).map(|i| format!(r#"{{"id":{i}}}"#)).collect();
        let json = format!(r#"{{"windows":[{}]}}"#, many.join(","));
        assert_eq!(Context::from_json(&json), Err(ContextError::TooMany { what: "windows", count: MAX_WINDOWS + 1, max: MAX_WINDOWS }));
        let ok: Vec<String> = (0..MAX_WINDOWS).map(|i| format!(r#"{{"id":{i}}}"#)).collect();
        assert!(Context::from_json(&format!(r#"{{"windows":[{}]}}"#, ok.join(","))).is_ok());
        let ws: Vec<String> = (0..=MAX_WORKSPACES).map(|i| format!(r#"{{"id":{i},"idx":{},"output":"o{}"}}"#, i % 200 + 1, i / 200)).collect();
        assert!(matches!(Context::from_json(&format!(r#"{{"workspaces":[{}]}}"#, ws.join(","))), Err(ContextError::TooMany { what: "workspaces", .. })));

        let title = |n: usize| format!(r#"{{"windows":[{{"id":1,"title":"{}"}}]}}"#, "ä".repeat(n));
        assert!(Context::from_json(&title(MAX_TITLE_CHARS)).is_ok(), "Zeichen, nicht Bytes");
        assert_eq!(
            Context::from_json(&title(MAX_TITLE_CHARS + 1)),
            Err(ContextError::TooLong { what: "window title", chars: MAX_TITLE_CHARS + 1, max: MAX_TITLE_CHARS })
        );
        let app_id = format!(r#"{{"windows":[{{"id":1,"app_id":"{}"}}]}}"#, "c".repeat(MAX_APP_ID_CHARS + 1));
        assert!(matches!(Context::from_json(&app_id), Err(ContextError::TooLong { what: "window app_id", .. })));
        let name = format!(r#"{{"workspaces":[{{"id":1,"idx":1,"name":"{}"}}]}}"#, "n".repeat(MAX_NAME_CHARS + 1));
        assert!(matches!(Context::from_json(&name), Err(ContextError::TooLong { what: "workspace name", .. })));
        let output = format!(r#"{{"workspaces":[{{"id":1,"idx":1,"output":"{}"}}]}}"#, "o".repeat(MAX_NAME_CHARS + 1));
        assert!(matches!(Context::from_json(&output), Err(ContextError::TooLong { what: "workspace output", .. })));
        let big = format!(r#"{{"windows":[{{"id":1,"title":"{}"}}]}}"#, "x".repeat(MAX_CONTEXT_BYTES));
        assert!(matches!(Context::from_json(&big), Err(ContextError::TooLarge { .. })));
    }

    #[test]
    fn focus_falls_back_to_the_focused_flag() {
        let ctx = Context::from_json(r#"{"windows":[{"id":1},{"id":2,"focused":true}]}"#).unwrap();
        assert_eq!(ctx.focused_window().unwrap().id, 2);
        let ctx = Context::from_json(r#"{"windows":[{"id":1},{"id":2,"focused":true}],"focus":1}"#).unwrap();
        assert_eq!(ctx.focused_window().unwrap().id, 1, "die ID gewinnt");
        let ctx = Context::from_json(r#"{"windows":[{"id":1}],"focus":99}"#).unwrap();
        assert!(ctx.focused_window().is_none(), "unbekannte ID: kein Fokus erfunden");
    }

    #[test]
    fn the_output_of_a_number_follows_window_then_focused_workspace_then_the_only_output() {
        let two =
            r#"{"workspaces":[{"id":1,"idx":1,"output":"A"},{"id":2,"idx":1,"output":"B","focused":true}],"windows":[{"id":5,"workspace_id":1},{"id":6}]"#;
        let ctx = Context::from_json(&format!("{two},\"focus\":5}}")).unwrap();
        assert_eq!(ctx.focused_output(), Some("A"), "Monitor des fokussierten Fensters");
        assert_eq!(ctx.output_for(&ctx.windows[1]), Some("B"), "Fenster ohne Arbeitsfläche: die fokussierte");
        let ctx = Context::from_json(&format!("{two}}}")).unwrap();
        assert_eq!(ctx.focused_output(), Some("B"), "ohne Fenster: die fokussierte Arbeitsfläche");
        assert_eq!(ctx.output_for(&ctx.windows[0]), Some("A"));
        let ctx = Context::from_json(r#"{"workspaces":[{"id":1,"idx":1,"output":"A"},{"id":2,"idx":1,"output":"B"}]}"#).unwrap();
        assert_eq!(ctx.focused_output(), None, "zwei Monitore, nichts fokussiert: nicht bestimmbar");
        let ctx = Context::from_json(r#"{"workspaces":[{"id":1,"idx":1,"output":"A"},{"id":2,"idx":2,"output":"A"}]}"#).unwrap();
        assert_eq!(ctx.focused_output(), Some("A"), "nur ein Monitor");
    }
}
