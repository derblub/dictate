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

//! Socket-Protokoll (JSON pro Zeile): Befehle, Antworten und Events.
//!
//! Jede Anfrage bekommt genau eine Antwortzeile mit `"ok"`. Ausnahme, die gestreamten Antworten der Textaktionen: vor
//! der Antwort auf `act_run` können Zwischenzeilen ohne `"ok"` stehen, `{"delta":"…"}` mit dem nächsten Stück
//! Antworttext (nur Aktionen ohne KI-Modus, gebündelt etwa alle 50 ms; jedes Stück endet an einer Graphem-Grenze).
//! Clients lesen darum bis zur ersten Zeile mit `"ok"` und behandeln `delta`-Zeilen davor als Fortschritt; der
//! vollständige Text steht immer in der Endantwort und ist bei Erfolg genau die Verkettung der Stücke. Kommt statt
//! dessen ein Fehler, sind die Stücke zu verwerfen. Auf Subscriber-Verbindungen stehen dazwischen außerdem Events
//! (`"event"`).
//!
//! Befehlspalette (`pal_*`, `PLAN-palette.md`): `pal_resolve` (Eingabe + Kontext → Kandidaten), `pal_chosen`,
//! `pal_sources_refresh`, `pal_frecency_clear`; je genau eine Antwortzeile. Bei `"enabled": false` in `[palette]`
//! antworten alle mit einem Fehler. Eingabe und Kontext gehören dem User (Fenstertitel) und gehen nie ins Log.
//! `pal_resolve` trägt außerdem die Umgebung der Sitzung des Aufrufers ([`SessionEnv`]: `path`, `desktop`,
//! `data_dirs`, `data_home`), weil der Dienst unter systemd eine kürzere hat.
//!
//! Diagnose (`diag_*`, `PLAN-diagnose.md`, Protokoll unter „Stand je Aufgabe“ › Aufgabe 7): vor der Antwort auf
//! `diag_collect` und `diag_check` stehen Zwischenzeilen `{"progress":{…}}` (je Probe `start` und `end`); Clients lesen
//! wie bei `delta` bis zur ersten Zeile mit `"ok"`. Bei `"enabled": false` in `[diagnose]` antworten alle mit einem Fehler.

use crate::actions::store::SecretText;
use crate::config::OverlayCfg;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// Umgebung der Sitzung des Aufrufers (des Panels; bis W7 das aus Awesome gestartete CLI), soweit sie die Programmliste der Befehlspalette
/// bestimmt. Der Dienst läuft unter systemd mit kürzerer Umgebung (kein `XDG_DATA_DIRS`, kein Desktop, anderer `PATH`);
/// `None` heißt: die Variable fehlt in der Sitzung, es gilt der Wert des Daemon-Prozesses (und dort die Vorgabe der
/// Desktop-Entry-Spec). Auf dem Draht sind es die Felder `path`, `desktop`, `data_dirs` und `data_home` von `pal_resolve`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEnv {
    /// `PATH` (durch `:` getrennt) für `TryExec`
    pub path: Option<String>,
    /// `XDG_CURRENT_DESKTOP` für `OnlyShowIn`/`NotShowIn`
    pub desktop: Option<String>,
    /// `XDG_DATA_DIRS` (durch `:` getrennt): die Verzeichnisse mit `applications/*.desktop`
    pub data_dirs: Option<String>,
    /// `XDG_DATA_HOME`: das Verzeichnis des Users mit `applications/*.desktop`
    pub data_home: Option<String>,
}

impl SessionEnv {
    /// Aus den Umgebungsvariablen dieses Prozesses (nicht UTF-8 gilt als fehlend)
    pub fn from_process() -> SessionEnv {
        let get = |name: &str| std::env::var(name).ok();
        SessionEnv { path: get("PATH"), desktop: get("XDG_CURRENT_DESKTOP"), data_dirs: get("XDG_DATA_DIRS"), data_home: get("XDG_DATA_HOME") }
    }

    /// Name der Umgebungsvariable und Wert, in der Reihenfolge des Drahtformats
    pub fn fields(&self) -> [(&'static str, &'static str, &Option<String>); 4] {
        [
            ("path", "PATH", &self.path),
            ("desktop", "XDG_CURRENT_DESKTOP", &self.desktop),
            ("data_dirs", "XDG_DATA_DIRS", &self.data_dirs),
            ("data_home", "XDG_DATA_HOME", &self.data_home),
        ]
    }
}

/// Einfache Befehle des Steuer-Sockets (ohne Parameter, außer dem optionalen `app` bei `start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Start,
    Stop,
    Cancel,
    Toggle,
    Status,
    Shutdown,
}

impl Cmd {
    pub fn name(self) -> &'static str {
        match self {
            Cmd::Start => "start",
            Cmd::Stop => "stop",
            Cmd::Cancel => "cancel",
            Cmd::Toggle => "toggle",
            Cmd::Status => "status",
            Cmd::Shutdown => "shutdown",
        }
    }

    pub fn from_name(s: &str) -> Option<Cmd> {
        Some(match s {
            "start" => Cmd::Start,
            "stop" => Cmd::Stop,
            "cancel" => Cmd::Cancel,
            "toggle" => Cmd::Toggle,
            "status" => Cmd::Status,
            "shutdown" => Cmd::Shutdown,
            _ => return None,
        })
    }
}

/// Standardzahl der Einträge bei `history` ohne `limit`.
pub const HISTORY_DEFAULT_LIMIT: usize = 50;
pub const HISTORY_MAX_LIMIT: usize = 10_000;

/// Eine Anfrage auf dem Socket.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Cmd(Cmd),
    /// `start` mit App-Name (`app_id` des fokussierten Fensters, nicht leer); ohne App gilt `Cmd(Cmd::Start)`
    Start {
        app: String,
    },
    /// `toggle` mit App-Name (wie `start`: Profil des fokussierten Programms, wenn die Aufnahme damit beginnt); ohne App
    /// gilt `Cmd(Cmd::Toggle)`. Ein älterer Daemon ignoriert `app` und schaltet trotzdem um
    Toggle {
        app: String,
    },
    /// `start` oder `toggle` mit `"target":"palette"`: die Aufnahme gehört der Eingabezeile der Befehlspalette
    /// (PLAN-palette.md, Aufgaben 8 und 9). Ihr final wird nicht in den Diktatverlauf geschrieben und gilt nicht als
    /// getippt („rückgängig“ löscht im Fenster nichts, was nie dort ankam). `app` wählt wie bei `start` das Profil.
    PaletteVoice {
        toggle: bool,
        app: Option<String>,
    },
    Subscribe,
    GetConfig,
    /// Vollständige oder teilweise Konfiguration als JSON; Schlüssel der obersten Ebene ersetzen die bisherigen
    SetConfig(Value),
    Reload,
    ListSources,
    ListModels,
    History {
        limit: usize,
    },
    HistoryClear,
    AiModels,
    /// Testlauf der KI mit dem gespeicherten Stand: Modus-Name und Beispieltext
    AiTest {
        mode: String,
        text: String,
    },
    /// Tabelle der Sprachbefehle
    Commands,
    Meter {
        on: bool,
    },
    /// Textaktionen: Markierung übergeben (`app` = `app_id` des Zielfensters, entscheidet `replace_ok`). Der Text
    /// geht nie ins Log (`SecretText` zeigt in `Debug` nur die Länge). Antwort:
    /// `{"ok":true,"id":"7","chars":342,"lines":7,"target":{"host":"api.anthropic.com","model":"…","cloud":true},
    /// "secret":["private_key"],"blocked":true,"replace_ok":false,"actions":[{"key":"e","name":"explain",
    /// "label":"Explain","output":"answer","allowed":false,"model":"…"},…]}`
    ///
    /// `source` (Vorgabe `selection`): `clipboard` (CLIPBOARD, Absicht `explain_clipboard` der Palette) und
    /// `question` (Frage der Palette, `ask`) bieten genau eine Aktion an und gehören der Palette (`palette.enabled`,
    /// `palette.model`, Geheimnis sperrt die Cloud).
    ActGrab {
        text: SecretText,
        app: Option<String>,
        source: crate::actions::store::Source,
    },
    /// Aktion `action` (`explain`, `mode:clean` …) auf der Markierung `id` ausführen. Antwort
    /// `{"ok":true,"text":"…","output":"answer","truncated":false}`, Abbruch `{"ok":false,"error":"Cancelled",
    /// "cancelled":true}`; davor `{"delta":"…"}`-Zeilen (siehe Moduldoku)
    ActRun {
        id: String,
        action: String,
    },
    /// Laufenden Aufruf der Markierung `id` abbrechen
    ActCancel {
        id: String,
    },
    /// Ergebnis des letzten Aufrufs abholen (Copy)
    ActResult {
        id: String,
    },
    /// Markierung und Ergebnis löschen
    ActClear,
    /// Befehlspalette: Eingabe und Kontext (Tags, Fenster, Layouts als JSON-Text im Format von Awesome, bis W5 es für
    /// niri festlegt; `None` = leer) lokal
    /// auflösen. `env` ist die Umgebung der Sitzung des Aufrufers (der Dienst hat oft eine kürzere): `PATH` und
    /// `XDG_CURRENT_DESKTOP` entscheiden über `TryExec` und `OnlyShowIn` (`PATH` auch darüber, ob Zwischenablage und
    /// Emoji angeboten werden, `palette::widgets`), `XDG_DATA_DIRS` und `XDG_DATA_HOME` über die Verzeichnisse mit den
    /// `.desktop`-Dateien. Eingabe und Kontext gehen nie ins Log (`SecretText` zeigt in `Debug` nur die Länge). Antwort:
    /// `{"ok":true,"candidates":[…],"confident":false,"web_search_url":"https://…{query}","terminal":[…],
    /// "project_command":[…],"clipboard_command":[…],"emoji_command":[…]}` (die letzten fünf aus `[palette]`,
    /// `palette::service::ExecPrefs`)
    PalResolve {
        input: SecretText,
        context: Option<SecretText>,
        env: SessionEnv,
    },
    /// Befehlspalette, KI-Pfad: wie `pal_resolve`, aber die Eingabe geht (allein, mit dem Katalog, ohne Kontext) an das
    /// Modell, das genau eine Absicht wählt; die Absicht wird geprüft und ihre Suchbegriffe lokal gegen den Kontext
    /// aufgelöst. Antwort `{"ok":true,"intent":{…},"candidates":[…],"web_search_url":"…","terminal":[…],
    /// "project_command":[…],"clipboard_command":[…],"emoji_command":[…]}` oder
    /// `{"ok":false,"error":"…"}` (Abbruch: zusätzlich `"cancelled":true`, Geheimnis: `"secret":[…]`). Schließt der
    /// Client die Verbindung vorher, wird der Aufruf abgebrochen
    PalAsk {
        input: SecretText,
        context: Option<SecretText>,
        env: SessionEnv,
    },
    /// Befehlspalette: der User hat ein Ziel gewählt (Häufigkeit). `intent` ist der Name der Absicht, `target_id` die
    /// Ziel-ID eines Kandidaten (`target_id` der Antwort auf `pal_resolve`); der Daemon prüft beides gegen das, was er
    /// selbst angeboten hat
    PalChosen {
        intent: String,
        target_id: String,
    },
    /// Befehlspalette: Programme und Projekte beim nächsten Zugriff neu lesen
    PalSourcesRefresh,
    /// Befehlspalette: gemerkte Häufigkeit löschen
    PalFrecencyClear,
    /// Diagnose (`PLAN-diagnose.md`, Protokoll unter „Stand je Aufgabe“ › Aufgabe 7): ohne `profile` die Liste der
    /// Profile, mit `profile` dessen Einträge (`unit` für das Profil `unit`, `container` optional für `docker`)
    DiagPlan {
        profile: Option<String>,
        unit: Option<String>,
        container: Option<String>,
    },
    /// Diagnose: fehlgeschlagene Units zur Auswahl für das Profil `unit`
    DiagUnits,
    /// Diagnose: neue Sitzung, Plan sammeln (`skip` = Einträge des Plans, die nicht laufen); vor der Antwort
    /// `{"progress":{…}}`-Zeilen, die Antwort ist die Vorschau. Schließt der Client vorher, wird abgebrochen
    DiagCollect {
        profile: String,
        unit: Option<String>,
        container: Option<String>,
        skip: Vec<usize>,
    },
    /// Diagnose: Vorschau, mit `deselected` (Kennungen `S1`, … der Abschnitte, die fehlen sollen) als neue Abwahl und mit
    /// `symptom` als Symptom des Users („What is wrong?“, leer = keins). Das Symptom geht nie ins Log (`SecretText`)
    DiagPreview {
        id: String,
        deselected: Option<Vec<String>>,
        symptom: Option<SecretText>,
    },
    /// Diagnose: Prüfbefehle (Proben aus dem Katalog als JSON-Objekte) als weitere Runde sammeln; `network` = der User
    /// hat Netzverkehr bestätigt (`net_reach`). Fortschritt und Abbruch wie `diag_collect`
    DiagCheck {
        id: String,
        checks: Vec<Value>,
        network: bool,
    },
    /// Diagnose: Bericht der Vorschau `preview` an das Modell senden
    DiagSend {
        id: String,
        preview: u64,
    },
    /// Diagnose: Bericht für „Copy report“ (das CLI schreibt ihn in CLIPBOARD)
    DiagCopy {
        id: String,
    },
    /// Diagnose: laufendes Sammeln oder Prüfen der Sitzung abbrechen
    DiagCancel {
        id: String,
    },
    /// Diagnose: Sitzung löschen
    DiagClear,
    /// Diagnose: endete der Boot davor unsauber, und wurde das in diesem Boot schon gemeldet? Antwort
    /// `{"show":bool,"reason":…}`; sammelt und sendet nichts (`PLAN-diagnose.md`, Aufgabe 9)
    DiagBootHint,
}

/// Schlüssel der Zwischenzeilen vor der Antwort auf `act_run` (siehe Moduldoku)
pub const DELTA_KEY: &str = "delta";

/// Frist eines Clients für die Antwort auf `diag_collect`, `diag_check` und `diag_units`: das Sammelbudget des Daemons
/// (15 s, `diagnose::report::COLLECT_BUDGET`) plus das Warten auf eine ersetzte Sammlung und eine Reserve (ein Test im
/// Daemon prüft es). Läuft sie ab, legt der Client auf, und der Daemon bricht die Sammlung ab. Für das Diagnose-Panel
/// unter niri (W4); bis W7 stand sie im CLI `dictate diag`.
pub const DIAG_COLLECT_REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Zwischenzeile `{"delta":"…"}` (ohne Zeilenende) mit dem nächsten Stück Antworttext
pub fn delta_line(text: &str) -> String {
    let mut m = Map::new();
    m.insert(DELTA_KEY.to_string(), Value::String(text.to_string()));
    Value::Object(m).to_string()
}

/// Erwarteter JSON-Typ eines Feldes von [`RawRequest`] für [`check_request_shape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Str,
    Bool,
    /// Ganzzahl ab 0 (`u64`, `usize`)
    Uint,
    /// beliebiger Wert (`config`, `context`: der Befehl prüft selbst)
    Any,
    /// Liste von Ganzzahlen ab 0
    UintList,
    /// Liste von Strings
    StrList,
    /// Liste beliebiger Werte
    List,
}

impl FieldKind {
    fn expected(self) -> &'static str {
        match self {
            FieldKind::Str => "a string",
            FieldKind::Bool => "true or false",
            FieldKind::Uint => "a non-negative integer",
            FieldKind::Any => "a value",
            FieldKind::UintList | FieldKind::StrList | FieldKind::List => "an array",
        }
    }

    fn accepts(self, v: &Value) -> bool {
        match self {
            FieldKind::Str => v.is_string(),
            FieldKind::Bool => v.is_boolean(),
            FieldKind::Uint => v.is_u64(),
            FieldKind::Any => true,
            FieldKind::UintList | FieldKind::StrList | FieldKind::List => v.is_array(),
        }
    }
}

/// Alle Felder von [`RawRequest`] außer `cmd` mit ihrem Typ (Test `request_fields_match_raw_request`). `null` gilt bei
/// jedem als „nicht angegeben“ (wie `Option` in serde).
const REQUEST_FIELDS: &[(&str, FieldKind)] = &[
    ("app", FieldKind::Str),
    ("config", FieldKind::Any),
    ("limit", FieldKind::Uint),
    ("on", FieldKind::Bool),
    ("mode", FieldKind::Str),
    ("text", FieldKind::Str),
    ("id", FieldKind::Str),
    ("action", FieldKind::Str),
    ("input", FieldKind::Str),
    ("context", FieldKind::Any),
    ("path", FieldKind::Str),
    ("desktop", FieldKind::Str),
    ("data_dirs", FieldKind::Str),
    ("data_home", FieldKind::Str),
    ("intent", FieldKind::Str),
    ("target_id", FieldKind::Str),
    ("source", FieldKind::Str),
    ("target", FieldKind::Str),
    ("profile", FieldKind::Str),
    ("unit", FieldKind::Str),
    ("container", FieldKind::Str),
    ("skip", FieldKind::UintList),
    ("deselected", FieldKind::StrList),
    ("symptom", FieldKind::Str),
    ("checks", FieldKind::List),
    ("network", FieldKind::Bool),
    ("preview", FieldKind::Uint),
];

/// Prüft die Form einer Anfrage, bevor serde sie liest. Die Meldung nennt nur den Feldnamen aus [`REQUEST_FIELDS`]
/// (bei Listen mit Index) und den erwarteten Typ, **nie einen Wert und nie einen fremden Schlüssel**: serde zitiert den
/// Wert („invalid type: string \"…\"“), und die Zeile kann Eingabe, Markierung oder Fenstertitel tragen
/// (Sicherheitsregel 5 in `PLAN-wayland.md` 6g, Vorlage E12). Unbekannte Felder bleiben erlaubt.
fn check_request_shape(v: &Value) -> Result<(), String> {
    let Value::Object(map) = v else { return Err("bad request: the line must be a JSON object".into()) };
    match map.get("cmd") {
        Some(Value::String(_)) => {}
        None | Some(Value::Null) => return Err("bad request: missing \"cmd\"".into()),
        Some(_) => return Err("bad request: \"cmd\" must be a string".into()),
    }
    for &(name, kind) in REQUEST_FIELDS {
        let Some(val) = map.get(name).filter(|v| !v.is_null()) else { continue };
        if !kind.accepts(val) {
            return Err(format!("bad request: \"{name}\" must be {}", kind.expected()));
        }
        let item: Option<(FieldKind, &str)> = match kind {
            FieldKind::UintList => Some((FieldKind::Uint, "a non-negative integer")),
            FieldKind::StrList => Some((FieldKind::Str, "a string")),
            _ => None,
        };
        if let (Some((item, expected)), Some(list)) = (item, val.as_array()) {
            if let Some(i) = list.iter().position(|x| !item.accepts(x)) {
                return Err(format!("bad request: \"{name}\"[{i}] must be {expected}"));
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct RawRequest {
    cmd: String,
    #[serde(default)]
    app: Option<String>,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    on: Option<bool>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    context: Option<Value>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    desktop: Option<String>,
    #[serde(default)]
    data_dirs: Option<String>,
    #[serde(default)]
    data_home: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    skip: Option<Vec<usize>>,
    #[serde(default)]
    deselected: Option<Vec<String>>,
    #[serde(default)]
    symptom: Option<String>,
    #[serde(default)]
    checks: Option<Vec<Value>>,
    #[serde(default)]
    network: Option<bool>,
    #[serde(default)]
    preview: Option<u64>,
}

/// Leere Texte gelten als nicht angegeben
fn optional(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Feld `name` muss ein nicht leerer String sein
fn required(v: Option<String>, cmd: &str, name: &str) -> Result<String, String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(|| format!("{cmd} needs \"{name}\""))
}

/// `{"cmd":"<cmd>", …}` mit `cmd` zuerst (wie in der Spezifikation); `fields` sind fertige JSON-Werte
fn cmd_line(cmd: &str, fields: &[(&str, Value)]) -> String {
    let mut out = format!("{{\"cmd\":\"{cmd}\"");
    for (k, v) in fields {
        out.push_str(&format!(",\"{k}\":{v}"));
    }
    out.push('}');
    out
}

impl Request {
    pub fn parse(line: &str) -> Result<Request, String> {
        // Syntaxfehler von serde_json nennen nur Art und Stelle, nie Text der Zeile; trotzdem eine eigene Meldung
        let value: Value = serde_json::from_str(line).map_err(|e| format!("bad request: not valid JSON (line {}, column {})", e.line(), e.column()))?;
        check_request_shape(&value)?;
        // nach der Prüfung oben kann serde nicht mehr scheitern; falls doch (ein Feld fehlt in REQUEST_FIELDS), ohne
        // seine Meldung, die den Wert zitierte
        let raw: RawRequest = serde_json::from_value(value).map_err(|_| "bad request: a field has the wrong type".to_string())?;
        Ok(match raw.cmd.as_str() {
            "subscribe" => Request::Subscribe,
            "start" | "toggle" if raw.target.as_deref().is_some_and(|t| t != "window") => match raw.target.as_deref() {
                Some("palette") => Request::PaletteVoice { toggle: raw.cmd == "toggle", app: raw.app.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) },
                _ => return Err(format!("{} \"target\" must be \"window\" or \"palette\"", raw.cmd)),
            },
            "start" => match raw.app.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) {
                Some(app) => Request::Start { app },
                None => Request::Cmd(Cmd::Start),
            },
            "toggle" if raw.app.as_deref().is_some_and(|a| !a.trim().is_empty()) => Request::Toggle { app: raw.app.unwrap_or_default().trim().to_string() },
            "get_config" => Request::GetConfig,
            "set_config" => match raw.config {
                Some(c) if c.is_object() => Request::SetConfig(c),
                _ => return Err("set_config needs a \"config\" object".into()),
            },
            "reload" => Request::Reload,
            "list_sources" => Request::ListSources,
            "list_models" => Request::ListModels,
            "history" => Request::History { limit: raw.limit.map(|l| (l as usize).clamp(1, HISTORY_MAX_LIMIT)).unwrap_or(HISTORY_DEFAULT_LIMIT) },
            "history_clear" => Request::HistoryClear,
            "ai_models" => Request::AiModels,
            "ai_test" => match (raw.mode, raw.text) {
                (Some(mode), Some(text)) => Request::AiTest { mode, text },
                _ => return Err("ai_test needs \"mode\" and \"text\"".into()),
            },
            "commands" => Request::Commands,
            "meter" => match raw.on {
                Some(on) => Request::Meter { on },
                None => return Err("meter needs \"on\": true or false".into()),
            },
            "act_grab" => match raw.text {
                Some(text) => Request::ActGrab {
                    text: SecretText::new(text),
                    app: raw.app.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()),
                    source: match raw.source.as_deref() {
                        None => crate::actions::store::Source::Selection,
                        Some(s) => crate::actions::store::Source::parse(s).ok_or("act_grab \"source\" must be \"selection\", \"clipboard\" or \"question\"")?,
                    },
                },
                None => return Err("act_grab needs \"text\"".into()),
            },
            "act_run" => Request::ActRun { id: required(raw.id, "act_run", "id")?, action: required(raw.action, "act_run", "action")? },
            "act_cancel" => Request::ActCancel { id: required(raw.id, "act_cancel", "id")? },
            "act_result" => Request::ActResult { id: required(raw.id, "act_result", "id")? },
            "act_clear" => Request::ActClear,
            "pal_resolve" | "pal_ask" => {
                let cmd = raw.cmd.as_str();
                let input = SecretText::new(raw.input.ok_or_else(|| format!("{cmd} needs \"input\" (a string, may be empty)"))?);
                let context = match raw.context {
                    None | Some(Value::Null) => None,
                    Some(c) if c.is_object() => Some(SecretText::new(c.to_string())),
                    Some(_) => return Err(format!("{cmd} \"context\" must be an object")),
                };
                let env = SessionEnv { path: raw.path, desktop: raw.desktop, data_dirs: raw.data_dirs, data_home: raw.data_home };
                if cmd == "pal_ask" {
                    Request::PalAsk { input, context, env }
                } else {
                    Request::PalResolve { input, context, env }
                }
            }
            "pal_chosen" => Request::PalChosen {
                intent: required(raw.intent, "pal_chosen", "intent")?,
                // Ziel-IDs (Pfade) werden nie gekürzt: sie müssen genau dem entsprechen, was angeboten wurde
                target_id: raw.target_id.filter(|t| !t.is_empty()).ok_or("pal_chosen needs \"target_id\"")?,
            },
            "pal_sources_refresh" => Request::PalSourcesRefresh,
            "pal_frecency_clear" => Request::PalFrecencyClear,
            "diag_plan" => Request::DiagPlan { profile: optional(raw.profile), unit: optional(raw.unit), container: optional(raw.container) },
            "diag_units" => Request::DiagUnits,
            "diag_collect" => Request::DiagCollect {
                profile: required(raw.profile, "diag_collect", "profile")?,
                unit: optional(raw.unit),
                container: optional(raw.container),
                skip: raw.skip.unwrap_or_default(),
            },
            "diag_preview" => {
                Request::DiagPreview { id: required(raw.id, "diag_preview", "id")?, deselected: raw.deselected, symptom: raw.symptom.map(SecretText::new) }
            }
            "diag_check" => Request::DiagCheck {
                id: required(raw.id, "diag_check", "id")?,
                checks: raw.checks.ok_or("diag_check needs \"checks\" (a list of probes)")?,
                network: raw.network.unwrap_or(false),
            },
            "diag_send" => Request::DiagSend {
                id: required(raw.id, "diag_send", "id")?,
                preview: raw.preview.ok_or("diag_send needs \"preview\" (the number of the preview that was shown)")?,
            },
            "diag_copy" => Request::DiagCopy { id: required(raw.id, "diag_copy", "id")? },
            "diag_cancel" => Request::DiagCancel { id: required(raw.id, "diag_cancel", "id")? },
            "diag_clear" => Request::DiagClear,
            "diag_boot_hint" => Request::DiagBootHint,
            other => return Cmd::from_name(other).map(Request::Cmd).ok_or_else(|| format!("unknown command: {other}")),
        })
    }

    pub fn to_line(&self) -> String {
        let v = match self {
            Request::Cmd(c) => json!({"cmd": c.name()}),
            // `cmd` zuerst, wie in der Spezifikation
            Request::Start { app } => return format!("{{\"cmd\":\"start\",\"app\":{}}}", Value::String(app.clone())),
            Request::Toggle { app } => return format!("{{\"cmd\":\"toggle\",\"app\":{}}}", Value::String(app.clone())),
            Request::PaletteVoice { toggle, app } => {
                let mut fields = vec![];
                if let Some(a) = app {
                    fields.push(("app", json!(a)));
                }
                fields.push(("target", json!("palette")));
                return cmd_line(if *toggle { "toggle" } else { "start" }, &fields);
            }
            Request::Subscribe => json!({"cmd": "subscribe"}),
            Request::GetConfig => json!({"cmd": "get_config"}),
            Request::SetConfig(c) => json!({"cmd": "set_config", "config": c}),
            Request::Reload => json!({"cmd": "reload"}),
            Request::ListSources => json!({"cmd": "list_sources"}),
            Request::ListModels => json!({"cmd": "list_models"}),
            Request::History { limit } => json!({"cmd": "history", "limit": limit}),
            Request::HistoryClear => json!({"cmd": "history_clear"}),
            Request::AiModels => json!({"cmd": "ai_models"}),
            Request::AiTest { mode, text } => json!({"cmd": "ai_test", "mode": mode, "text": text}),
            Request::Commands => json!({"cmd": "commands"}),
            Request::Meter { on } => json!({"cmd": "meter", "on": on}),
            Request::ActGrab { text, app, source } => {
                // Text ohne Umweg über `Value` (eine Kopie weniger)
                let mut t = serde_json::to_string(text.as_str()).expect("String serialisierbar");
                let mut extra = app.as_ref().map(|a| format!(",\"app\":{}", Value::String(a.clone()))).unwrap_or_default();
                if *source != crate::actions::store::Source::Selection {
                    extra.push_str(&format!(",\"source\":\"{}\"", source.as_str()));
                }
                // passend angelegt: kein Umkopieren beim Wachsen, das unüberschriebene Reste ließe
                let mut line = String::with_capacity(28 + t.len() + extra.len() + 1);
                line.push_str("{\"cmd\":\"act_grab\",\"text\":");
                line.push_str(&t);
                line.push_str(&extra);
                line.push('}');
                crate::actions::store::wipe(&mut t);
                return line;
            }
            Request::ActRun { id, action } => return cmd_line("act_run", &[("id", json!(id)), ("action", json!(action))]),
            Request::ActCancel { id } => return cmd_line("act_cancel", &[("id", json!(id))]),
            Request::ActResult { id } => return cmd_line("act_result", &[("id", json!(id))]),
            Request::ActClear => return cmd_line("act_clear", &[]),
            Request::PalResolve { input, context, env } | Request::PalAsk { input, context, env } => {
                let cmd = if matches!(self, Request::PalAsk { .. }) { "pal_ask" } else { "pal_resolve" };
                // Kontext ist schon JSON-Text: unverändert einsetzen (kein Umweg über `Value`); er stammt von
                // `Request::parse` oder vom CLI, das ihn als Objekt geprüft hat
                let mut line = format!("{{\"cmd\":\"{cmd}\",\"input\":{}", serde_json::to_string(input.as_str()).expect("String serialisierbar"));
                if let Some(c) = context {
                    line.push_str(",\"context\":");
                    line.push_str(c.as_str());
                }
                for (key, _, value) in env.fields() {
                    if let Some(v) = value {
                        line.push_str(&format!(",\"{key}\":{}", Value::String(v.clone())));
                    }
                }
                line.push('}');
                return line;
            }
            Request::PalChosen { intent, target_id } => return cmd_line("pal_chosen", &[("intent", json!(intent)), ("target_id", json!(target_id))]),
            Request::PalSourcesRefresh => return cmd_line("pal_sources_refresh", &[]),
            Request::PalFrecencyClear => return cmd_line("pal_frecency_clear", &[]),
            Request::DiagPlan { profile, unit, container } => {
                let fields: Vec<(&str, Value)> = [("profile", profile), ("unit", unit), ("container", container)]
                    .into_iter()
                    .filter_map(|(k, v)| v.as_ref().map(|v| (k, json!(v))))
                    .collect();
                return cmd_line("diag_plan", &fields);
            }
            Request::DiagUnits => return cmd_line("diag_units", &[]),
            Request::DiagCollect { profile, unit, container, skip } => {
                let mut fields = vec![("profile", json!(profile))];
                fields.extend([("unit", unit), ("container", container)].into_iter().filter_map(|(k, v)| v.as_ref().map(|v| (k, json!(v)))));
                if !skip.is_empty() {
                    fields.push(("skip", json!(skip)));
                }
                return cmd_line("diag_collect", &fields);
            }
            Request::DiagPreview { id, deselected, symptom } => {
                let mut fields = vec![("id", json!(id))];
                if let Some(d) = deselected {
                    fields.push(("deselected", json!(d)));
                }
                if let Some(s) = symptom {
                    fields.push(("symptom", json!(s.as_str())));
                }
                return cmd_line("diag_preview", &fields);
            }
            Request::DiagCheck { id, checks, network } => {
                return cmd_line("diag_check", &[("id", json!(id)), ("checks", json!(checks)), ("network", json!(network))]);
            }
            Request::DiagSend { id, preview } => return cmd_line("diag_send", &[("id", json!(id)), ("preview", json!(preview))]),
            Request::DiagCopy { id } => return cmd_line("diag_copy", &[("id", json!(id))]),
            Request::DiagCancel { id } => return cmd_line("diag_cancel", &[("id", json!(id))]),
            Request::DiagClear => return cmd_line("diag_clear", &[]),
            Request::DiagBootHint => return cmd_line("diag_boot_hint", &[]),
        };
        v.to_string()
    }
}

/// Zustände des Daemons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Loading,
    Listening,
    Finalizing,
    Error,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Loading => "loading",
            State::Listening => "listening",
            State::Finalizing => "finalizing",
            State::Error => "error",
        }
    }
}

/// Antwort auf einen Befehl. Befehle mit Nutzdaten (`config`, `sources`, `entries` …) legen sie in `extra`;
/// die Felder stehen in der JSON-Zeile auf oberster Ebene neben `ok`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub state: Option<State>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    #[serde(flatten, skip_serializing_if = "Map::is_empty", default)]
    pub extra: Map<String, Value>,
}

impl Reply {
    pub fn ok(state: State) -> Reply {
        Reply { ok: true, state: Some(state), error: None, extra: Map::new() }
    }
    /// Erfolg ohne Zustand, mit Nutzdaten aus einem JSON-Objekt (andere Werte ergeben keine Zusatzfelder).
    pub fn data(v: Value) -> Reply {
        let extra = match v {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        Reply { ok: true, state: None, error: None, extra }
    }
    pub fn err(msg: impl Into<String>) -> Reply {
        Reply { ok: false, state: None, error: Some(msg.into()), extra: Map::new() }
    }
    /// Fehler, der sich einem Konfigurationsfeld zuordnen lässt (`field`, z. B. `ai.timeout_ms`).
    pub fn field_err(msg: impl Into<String>, field: impl Into<String>) -> Reply {
        let mut r = Reply::err(msg);
        r.extra.insert("field".into(), Value::String(field.into()));
        r
    }
    pub fn with(mut self, key: &str, value: Value) -> Reply {
        self.extra.insert(key.into(), value);
        self
    }
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("Reply serialisierbar")
    }
}

/// Events an Subscriber.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    State {
        state: State,
    },
    Partial {
        stable: String,
        tail: String,
    },
    Level {
        db: f32,
        speech: bool,
    },
    Final {
        /// Fertiger Text (nach der Pipeline, mit Leerzeichen am Ende, wenn so konfiguriert)
        text: String,
        cancelled: bool,
        audio_ms: u64,
        finalize_ms: u64,
        /// Text vor der Pipeline (Whisper-Ausgabe)
        #[serde(default)]
        raw: String,
        /// Tastennamen (X-Keysym-Namen), die nach dem Text gedrückt werden (z. B. `BackSpace`)
        #[serde(default)]
        keys: Vec<String>,
    },
    Error {
        msg: String,
    },
    /// Overlay-Einstellungen für `dictate-ui`; direkt nach dem State-Event beim Subscribe und nach jeder Änderung
    Overlay {
        overlay: OverlayCfg,
    },
    /// Klassen bzw. `app_id`s der Terminals (`actions.terminal_classes`) für `dictate-ui`; nach dem Overlay-Event beim
    /// Subscribe und nach jeder Änderung. Dort fügt `dictate-ui` nicht über die Zwischenablage ein
    /// und schreibt Zeilenumbrüche als Return (sonst als Shift+Return, für Chat-Felder, in denen Enter sendet);
    /// Textaktionen ersetzen dort nie.
    Terminals {
        classes: Vec<String>,
    },
}

impl Event {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Event serialisierbar")
    }
}

/// Steuer-Socket: `$DICTATE_SOCKET`, sonst `$XDG_RUNTIME_DIR/dictate/control.sock`.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DICTATE_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("dictate/control.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Meldungen bei falschen Typen nennen nur Feld und erwarteten Typ, nie den Wert, einen fremden Schlüssel oder eine
    /// fremde Variante (Sicherheitsregel 5, Vorlage E12). Jeder Fall trägt einen Marker, der in der Meldung fehlen muss.
    #[test]
    fn parse_errors_never_quote_values() {
        const M: &str = "marker-e12-secret";
        for (line, want) in [
            (format!(r#"{{"cmd":"pal_resolve","input":["{M}"]}}"#), "bad request: \"input\" must be a string"),
            (format!(r#"{{"cmd":"pal_resolve","input":{{"{M}":1}}}}"#), "bad request: \"input\" must be a string"),
            (format!(r#"{{"cmd":"history","limit":"{M}"}}"#), "bad request: \"limit\" must be a non-negative integer"),
            (r#"{"cmd":"history","limit":-17}"#.to_string(), "bad request: \"limit\" must be a non-negative integer"),
            (r#"{"cmd":"history","limit":1.5}"#.to_string(), "bad request: \"limit\" must be a non-negative integer"),
            (format!(r#"{{"cmd":"set_enabled","on":"{M}"}}"#), "bad request: \"on\" must be true or false"),
            (format!(r#"{{"cmd":"diag_collect","skip":[1,"{M}"]}}"#), "bad request: \"skip\"[1] must be a non-negative integer"),
            (format!(r#"{{"cmd":"diag_collect","deselected":["a",{{"{M}":2}}]}}"#), "bad request: \"deselected\"[1] must be a string"),
            (format!(r#"{{"cmd":"diag_collect","checks":"{M}"}}"#), "bad request: \"checks\" must be an array"),
            (format!(r#"{{"cmd":{{"{M}":1}}}}"#), "bad request: \"cmd\" must be a string"),
            (format!(r#"{{"{M}":1}}"#), "bad request: missing \"cmd\""),
            (format!(r#"["{M}"]"#), "bad request: the line must be a JSON object"),
            // ein Wert, der wie eine Meldung von serde aussieht
            (format!(r#"{{"cmd":"pal_resolve","input":5,"text":"{M}, expected a string"}}"#), "bad request: \"input\" must be a string"),
            (format!(r#"{{"cmd":"pal_chosen","intent":7,"target_id":"{M}"}}"#), "bad request: \"intent\" must be a string"),
        ] {
            let err = Request::parse(&line).expect_err(&line);
            assert_eq!(err, want, "{line}");
            assert!(!err.contains(M), "{err}");
        }
        // Syntaxfehler: nur Art und Stelle
        for line in [format!(r#"{{"cmd":"pal_resolve","input":"{M}""#), format!(r#"{{"cmd":"x","input":"{M}" "#), M.to_string()] {
            let err = Request::parse(&line).expect_err(&line);
            assert!(err.starts_with("bad request: not valid JSON (line 1, column "), "{err}");
            assert!(!err.contains(M), "{err}");
        }
        // null ist „nicht angegeben“, unbekannte Felder bleiben erlaubt
        assert_eq!(Request::parse(r#"{"cmd":"subscribe","app":null,"limit":null,"skip":null}"#), Ok(Request::Subscribe));
        assert_eq!(Request::parse(&format!(r#"{{"cmd":"subscribe","{M}":[1]}}"#)), Ok(Request::Subscribe));
    }

    /// [`REQUEST_FIELDS`] nennt genau die Felder von `RawRequest` mit dem passenden Typ: der Test liest die Struktur aus
    /// dem Quelltext. Kommt ein Feld hinzu, schlägt er an (sonst gäbe serde bei falschem Typ nur die allgemeine Meldung).
    #[test]
    fn request_fields_match_raw_request() {
        let src = include_str!("protocol.rs");
        let body = src.split("struct RawRequest {").nth(1).expect("struct RawRequest");
        let body = &body[..body.find("\n}\n").expect("end of RawRequest")];
        let mut fields: Vec<(String, String)> = Vec::new();
        for line in body.lines().map(str::trim).filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let (name, ty) = line.trim_end_matches(',').split_once(": ").expect("field line");
            fields.push((name.to_string(), ty.to_string()));
        }
        assert_eq!(fields.first().map(|f| f.0.as_str()), Some("cmd"));
        let rest = &fields[1..];
        assert_eq!(rest.len(), REQUEST_FIELDS.len(), "{rest:?}");
        for ((name, ty), (want, kind)) in rest.iter().zip(REQUEST_FIELDS) {
            assert_eq!(name, want);
            let expected = match kind {
                FieldKind::Str => "Option<String>",
                FieldKind::Bool => "Option<bool>",
                FieldKind::Uint => "Option<u64>",
                FieldKind::Any => "Option<Value>",
                FieldKind::UintList => "Option<Vec<usize>>",
                FieldKind::StrList => "Option<Vec<String>>",
                FieldKind::List => "Option<Vec<Value>>",
            };
            assert_eq!(ty, expected, "{name}");
        }
    }

    #[test]
    fn parse_requests() {
        assert_eq!(Request::parse(r#"{"cmd":"start"}"#), Ok(Request::Cmd(Cmd::Start)));
        assert_eq!(Request::parse(r#"{"cmd":"start","app":""}"#), Ok(Request::Cmd(Cmd::Start)));
        assert_eq!(Request::parse(r#"{"cmd":"start","app":"Alacritty"}"#), Ok(Request::Start { app: "Alacritty".into() }));
        assert_eq!(Request::parse(r#"{"cmd":"toggle","app":" foot "}"#), Ok(Request::Toggle { app: "foot".into() }));
        assert_eq!(Request::parse(r#"{"cmd":"toggle","app":""}"#), Ok(Request::Cmd(Cmd::Toggle)));
        let t = Request::Toggle { app: "org.gnome.Console".into() };
        assert_eq!(t.to_line(), r#"{"cmd":"toggle","app":"org.gnome.Console"}"#);
        assert_eq!(Request::parse(&t.to_line()), Ok(t));
        assert_eq!(Request::parse(r#"{"cmd":"subscribe"}"#), Ok(Request::Subscribe));
        // Aufnahme für die Befehlspalette (kein Verlauf); `window` ist die Vorgabe
        assert_eq!(Request::parse(r#"{"cmd":"start","app":"kitty","target":"window"}"#), Ok(Request::Start { app: "kitty".into() }));
        assert_eq!(Request::parse(r#"{"cmd":"toggle","target":"window"}"#), Ok(Request::Cmd(Cmd::Toggle)));
        for (line, want) in [
            (r#"{"cmd":"start","app":"kitty","target":"palette"}"#, Request::PaletteVoice { toggle: false, app: Some("kitty".into()) }),
            (r#"{"cmd":"start","target":"palette"}"#, Request::PaletteVoice { toggle: false, app: None }),
            (r#"{"cmd":"toggle","app":" ","target":"palette"}"#, Request::PaletteVoice { toggle: true, app: None }),
        ] {
            assert_eq!(Request::parse(line), Ok(want.clone()), "{line}");
            assert_eq!(Request::parse(&want.to_line()), Ok(want), "roundtrip {line}");
        }
        assert_eq!(Request::parse(r#"{"cmd":"start","target":"overlay"}"#), Err("start \"target\" must be \"window\" or \"palette\"".into()));
        assert!(Request::parse(r#"{"cmd":"nope"}"#).is_err());
        assert!(Request::parse("garbage").is_err());
        for c in [Cmd::Start, Cmd::Stop, Cmd::Cancel, Cmd::Toggle, Cmd::Status, Cmd::Shutdown] {
            assert_eq!(Request::parse(&Request::Cmd(c).to_line()), Ok(Request::Cmd(c)));
        }
    }

    #[test]
    fn parse_new_requests() {
        use Request::*;
        let cases = [
            (r#"{"cmd":"get_config"}"#, GetConfig),
            (r#"{"cmd":"reload"}"#, Reload),
            (r#"{"cmd":"list_sources"}"#, ListSources),
            (r#"{"cmd":"list_models"}"#, ListModels),
            (r#"{"cmd":"history"}"#, History { limit: HISTORY_DEFAULT_LIMIT }),
            (r#"{"cmd":"history","limit":5}"#, History { limit: 5 }),
            (r#"{"cmd":"history","limit":0}"#, History { limit: 1 }),
            (r#"{"cmd":"history","limit":99999999}"#, History { limit: HISTORY_MAX_LIMIT }),
            (r#"{"cmd":"history_clear"}"#, HistoryClear),
            (r#"{"cmd":"ai_models"}"#, AiModels),
            (r#"{"cmd":"ai_test","mode":"clean","text":"x"}"#, AiTest { mode: "clean".into(), text: "x".into() }),
            (r#"{"cmd":"commands"}"#, Commands),
            (r#"{"cmd":"meter","on":true}"#, Meter { on: true }),
            (r#"{"cmd":"meter","on":false}"#, Meter { on: false }),
            (r#"{"cmd":"set_config","config":{"threads":2}}"#, SetConfig(json!({"threads": 2}))),
        ];
        for (line, want) in cases.clone() {
            assert_eq!(Request::parse(line), Ok(want.clone()), "{line}");
        }
        assert!(Request::parse(r#"{"cmd":"meter"}"#).is_err());
        assert!(Request::parse(r#"{"cmd":"set_config"}"#).is_err());
        assert!(Request::parse(r#"{"cmd":"set_config","config":[1]}"#).is_err());
        // Roundtrip über to_line
        for (_, r) in cases {
            if let History { .. } = r {
                continue;
            }
            assert_eq!(Request::parse(&r.to_line()), Ok(r));
        }
        let h = History { limit: 7 };
        assert_eq!(Request::parse(&h.to_line()), Ok(h));
        let s = Start { app: "kitty".into() };
        assert_eq!(Request::parse(&s.to_line()), Ok(s));
    }

    #[test]
    fn parse_act_requests() {
        use crate::actions::store::Source;
        use Request::*;
        let grab = |t: &str, app: Option<&str>| ActGrab { text: SecretText::new(t.into()), app: app.map(str::to_string), source: Source::Selection };
        let cases = [
            (r#"{"cmd":"act_grab","text":"x","source":"selection"}"#, grab("x", None)),
            (
                r#"{"cmd":"act_grab","text":"Was ist ein Monad?","source":"question"}"#,
                ActGrab { text: SecretText::new("Was ist ein Monad?".into()), app: None, source: Source::Question },
            ),
            (
                r#"{"cmd":"act_grab","text":"y","app":"kitty","source":"clipboard"}"#,
                ActGrab { text: SecretText::new("y".into()), app: Some("kitty".into()), source: Source::Clipboard },
            ),
            (r#"{"cmd":"act_grab","text":"Grüße\n\tx","app":"Alacritty"}"#, grab("Grüße\n\tx", Some("Alacritty"))),
            (r#"{"cmd":"act_grab","text":"x","app":"  "}"#, grab("x", None)),
            (r#"{"cmd":"act_grab","text":""}"#, grab("", None)),
            (r#"{"cmd":"act_run","id":"7","action":"explain"}"#, ActRun { id: "7".into(), action: "explain".into() }),
            (r#"{"cmd":"act_run","id":"7","action":"mode:clean"}"#, ActRun { id: "7".into(), action: "mode:clean".into() }),
            (r#"{"cmd":"act_cancel","id":"7"}"#, ActCancel { id: "7".into() }),
            (r#"{"cmd":"act_result","id":"7"}"#, ActResult { id: "7".into() }),
            (r#"{"cmd":"act_clear"}"#, ActClear),
        ];
        for (line, want) in cases.clone() {
            assert_eq!(Request::parse(line), Ok(want.clone()), "{line}");
            assert_eq!(Request::parse(&want.to_line()), Ok(want), "roundtrip {line}");
        }
        for (bad, msg) in [
            (r#"{"cmd":"act_grab"}"#, "act_grab needs \"text\""),
            (r#"{"cmd":"act_grab","text":"x","source":"primary"}"#, "act_grab \"source\" must be \"selection\", \"clipboard\" or \"question\""),
            (r#"{"cmd":"act_run","id":"7"}"#, "act_run needs \"action\""),
            (r#"{"cmd":"act_run","action":"explain"}"#, "act_run needs \"id\""),
            (r#"{"cmd":"act_run","id":"","action":"explain"}"#, "act_run needs \"id\""),
            (r#"{"cmd":"act_cancel"}"#, "act_cancel needs \"id\""),
            (r#"{"cmd":"act_result"}"#, "act_result needs \"id\""),
        ] {
            assert_eq!(Request::parse(bad), Err(msg.to_string()), "{bad}");
        }
        // Zahl statt String ist kein gültiges `id`
        assert!(Request::parse(r#"{"cmd":"act_run","id":7,"action":"explain"}"#).is_err());
        // `cmd` steht vorn, Steuerzeichen sind maskiert
        assert_eq!(grab("a\"b\n", Some("kitty")).to_line(), r#"{"cmd":"act_grab","text":"a\"b\n","app":"kitty"}"#);
        assert_eq!(ActRun { id: "7".into(), action: "explain".into() }.to_line(), r#"{"cmd":"act_run","id":"7","action":"explain"}"#);
        assert_eq!(ActClear.to_line(), r#"{"cmd":"act_clear"}"#);
        // Debug zeigt den Text nicht
        let dbg = format!("{:?}", grab("hunter2", None));
        assert!(!dbg.contains("hunter2") && dbg.contains("7 bytes"), "{dbg}");
    }

    #[test]
    fn parse_diag_requests() {
        use Request::*;
        let cases = [
            (r#"{"cmd":"diag_plan"}"#, DiagPlan { profile: None, unit: None, container: None }),
            (
                r#"{"cmd":"diag_plan","profile":"unit","unit":"docker"}"#,
                DiagPlan { profile: Some("unit".into()), unit: Some("docker".into()), container: None },
            ),
            (r#"{"cmd":"diag_units"}"#, DiagUnits),
            (r#"{"cmd":"diag_collect","profile":"now"}"#, DiagCollect { profile: "now".into(), unit: None, container: None, skip: vec![] }),
            (
                r#"{"cmd":"diag_collect","profile":"docker","container":"dataroom","skip":[1,3]}"#,
                DiagCollect { profile: "docker".into(), unit: None, container: Some("dataroom".into()), skip: vec![1, 3] },
            ),
            (r#"{"cmd":"diag_preview","id":"3"}"#, DiagPreview { id: "3".into(), deselected: None, symptom: None }),
            (r#"{"cmd":"diag_preview","id":"3","deselected":["S2"]}"#, DiagPreview { id: "3".into(), deselected: Some(vec!["S2".into()]), symptom: None }),
            (
                r#"{"cmd":"diag_preview","id":"3","symptom":"Wifi drops"}"#,
                DiagPreview { id: "3".into(), deselected: None, symptom: Some(SecretText::new("Wifi drops".into())) },
            ),
            (
                r#"{"cmd":"diag_check","id":"3","checks":[{"probe":"mem"}],"network":true}"#,
                DiagCheck { id: "3".into(), checks: vec![json!({"probe": "mem"})], network: true },
            ),
            (r#"{"cmd":"diag_send","id":"3","preview":2}"#, DiagSend { id: "3".into(), preview: 2 }),
            (r#"{"cmd":"diag_copy","id":"3"}"#, DiagCopy { id: "3".into() }),
            (r#"{"cmd":"diag_cancel","id":"3"}"#, DiagCancel { id: "3".into() }),
            (r#"{"cmd":"diag_clear"}"#, DiagClear),
            (r#"{"cmd":"diag_boot_hint"}"#, DiagBootHint),
        ];
        for (line, want) in cases {
            let got = Request::parse(line).unwrap();
            assert_eq!(got, want, "{line}");
            assert_eq!(Request::parse(&got.to_line()).unwrap(), want, "round trip {line}");
        }
        assert_eq!(
            Request::parse(r#"{"cmd":"diag_collect","profile":"now","skip":[1]}"#).unwrap().to_line(),
            r#"{"cmd":"diag_collect","profile":"now","skip":[1]}"#
        );
        for (line, err) in [
            (r#"{"cmd":"diag_collect"}"#, "diag_collect needs \"profile\""),
            (r#"{"cmd":"diag_check","id":"3"}"#, "diag_check needs \"checks\" (a list of probes)"),
            (r#"{"cmd":"diag_send","id":"3"}"#, "diag_send needs \"preview\" (the number of the preview that was shown)"),
            (r#"{"cmd":"diag_copy"}"#, "diag_copy needs \"id\""),
        ] {
            assert_eq!(Request::parse(line).unwrap_err(), err);
        }
        assert!(Request::parse(r#"{"cmd":"diag_collect","profile":"now","skip":[-1]}"#).is_err());
    }

    #[test]
    fn parse_palette_requests() {
        use Request::*;
        let env = |path: Option<&str>, desktop: Option<&str>| SessionEnv {
            path: path.map(str::to_string),
            desktop: desktop.map(str::to_string),
            ..SessionEnv::default()
        };
        let resolve = |input: &str, context: Option<&str>, env: SessionEnv| PalResolve {
            input: SecretText::new(input.into()),
            context: context.map(|c| SecretText::new(c.into())),
            env,
        };
        let full_env = SessionEnv {
            path: Some("/usr/bin:/bin".into()),
            desktop: Some("awesome".into()),
            data_dirs: Some("/usr/local/share:/var/lib/flatpak/exports/share:/usr/share".into()),
            data_home: Some("/home/u/.local/share".into()),
        };
        let cases = [
            (r#"{"cmd":"pal_resolve","input":"fire"}"#, resolve("fire", None, SessionEnv::default())),
            // leere Eingabe ist gültig (häufige Ziele und Fenster), `null` heißt kein Kontext
            (r#"{"cmd":"pal_resolve","input":"","context":null}"#, resolve("", None, SessionEnv::default())),
            (
                r#"{"cmd":"pal_resolve","input":"Grüße \"x\"\n","context":{"workspaces":[{"id":1,"idx":1,"name":"web"}]},"path":"/usr/bin:/bin","desktop":"awesome"}"#,
                resolve("Grüße \"x\"\n", Some(r#"{"workspaces":[{"id":1,"idx":1,"name":"web"}]}"#), env(Some("/usr/bin:/bin"), Some("awesome"))),
            ),
            // die ganze Umgebung der Sitzung; ein leerer Wert ist ein Wert (die Variable ist gesetzt, aber leer)
            (
                r#"{"cmd":"pal_resolve","input":"x","path":"/usr/bin:/bin","desktop":"awesome","data_dirs":"/usr/local/share:/var/lib/flatpak/exports/share:/usr/share","data_home":"/home/u/.local/share"}"#,
                resolve("x", None, full_env.clone()),
            ),
            (
                r#"{"cmd":"pal_resolve","input":"x","data_dirs":"","data_home":""}"#,
                resolve("x", None, SessionEnv { data_dirs: Some(String::new()), data_home: Some(String::new()), ..SessionEnv::default() }),
            ),
            (
                r#"{"cmd":"pal_chosen","intent":"launch_app","target_id":"firefox.desktop"}"#,
                PalChosen { intent: "launch_app".into(), target_id: "firefox.desktop".into() },
            ),
            // Ziel-IDs bleiben unverändert (Pfade), nur der Name der Absicht wird gekürzt
            (
                r#"{"cmd":"pal_chosen","intent":" open_project ","target_id":" /home/git/a b "}"#,
                PalChosen { intent: "open_project".into(), target_id: " /home/git/a b ".into() },
            ),
            (r#"{"cmd":"pal_sources_refresh"}"#, PalSourcesRefresh),
            (r#"{"cmd":"pal_frecency_clear"}"#, PalFrecencyClear),
            // KI-Pfad: dieselben Felder wie pal_resolve
            (
                r#"{"cmd":"pal_ask","input":"open firefox","context":{"workspaces":[]},"path":"/bin"}"#,
                PalAsk {
                    input: SecretText::new("open firefox".into()),
                    context: Some(SecretText::new(r#"{"workspaces":[]}"#.into())),
                    env: env(Some("/bin"), None),
                },
            ),
            (r#"{"cmd":"pal_ask","input":"x"}"#, PalAsk { input: SecretText::new("x".into()), context: None, env: SessionEnv::default() }),
        ];
        for (line, want) in cases {
            assert_eq!(Request::parse(line), Ok(want.clone()), "{line}");
            assert_eq!(Request::parse(&want.to_line()), Ok(want), "roundtrip {line}");
        }
        for (bad, msg) in [
            (r#"{"cmd":"pal_resolve"}"#, "pal_resolve needs \"input\" (a string, may be empty)"),
            (r#"{"cmd":"pal_resolve","input":"x","context":[1]}"#, "pal_resolve \"context\" must be an object"),
            (r#"{"cmd":"pal_resolve","input":"x","context":"{}"}"#, "pal_resolve \"context\" must be an object"),
            (r#"{"cmd":"pal_ask"}"#, "pal_ask needs \"input\" (a string, may be empty)"),
            (r#"{"cmd":"pal_ask","input":"x","context":[1]}"#, "pal_ask \"context\" must be an object"),
            (r#"{"cmd":"pal_chosen","target_id":"x"}"#, "pal_chosen needs \"intent\""),
            (r#"{"cmd":"pal_chosen","intent":"","target_id":"x"}"#, "pal_chosen needs \"intent\""),
            (r#"{"cmd":"pal_chosen","intent":"launch_app"}"#, "pal_chosen needs \"target_id\""),
            (r#"{"cmd":"pal_chosen","intent":"launch_app","target_id":""}"#, "pal_chosen needs \"target_id\""),
        ] {
            assert_eq!(Request::parse(bad), Err(msg.to_string()), "{bad}");
        }
        // Eingabe kein String: kein gültiges Feld, die Meldung nennt Feld und Typ
        assert_eq!(Request::parse(r#"{"cmd":"pal_resolve","input":5}"#), Err("bad request: \"input\" must be a string".to_string()));
        // Form der Zeile: `cmd` zuerst, Steuerzeichen maskiert, Kontext unverändert
        assert_eq!(
            resolve("a\"b\n", Some(r#"{"workspaces":[]}"#), env(Some("/bin"), None)).to_line(),
            r#"{"cmd":"pal_resolve","input":"a\"b\n","context":{"workspaces":[]},"path":"/bin"}"#
        );
        // die Umgebung der Sitzung in fester Reihenfolge, fehlende Werte entfallen
        assert_eq!(
            resolve("x", None, full_env).to_line(),
            r#"{"cmd":"pal_resolve","input":"x","path":"/usr/bin:/bin","desktop":"awesome","data_dirs":"/usr/local/share:/var/lib/flatpak/exports/share:/usr/share","data_home":"/home/u/.local/share"}"#
        );
        assert_eq!(
            resolve("x", None, SessionEnv { data_home: Some("/h".into()), ..SessionEnv::default() }).to_line(),
            r#"{"cmd":"pal_resolve","input":"x","data_home":"/h"}"#
        );
        assert_eq!(PalSourcesRefresh.to_line(), r#"{"cmd":"pal_sources_refresh"}"#);
        let ask = PalAsk { input: SecretText::new("hunter2".into()), context: None, env: env(Some("/bin"), None) };
        assert_eq!(ask.to_line(), r#"{"cmd":"pal_ask","input":"hunter2","path":"/bin"}"#);
        assert!(!format!("{ask:?}").contains("hunter2"));
        // Eingabe und Kontext (Fenstertitel) stehen nie im Debug-Text
        let dbg = format!("{:?}", resolve("hunter2", Some(r#"{"windows":[{"id":1,"title":"Bank"}]}"#), SessionEnv::default()));
        assert!(!dbg.contains("hunter2") && !dbg.contains("Bank") && dbg.contains("7 bytes"), "{dbg}");
    }

    #[test]
    fn reply_and_event_json() {
        assert_eq!(Reply::ok(State::Idle).to_line(), r#"{"ok":true,"state":"idle"}"#);
        assert_eq!(Reply::err("x").to_line(), r#"{"ok":false,"error":"x"}"#);
        assert_eq!(Reply::field_err("bad", "ai.timeout_ms").to_line(), r#"{"ok":false,"error":"bad","field":"ai.timeout_ms"}"#);
        assert_eq!(Reply::data(json!({"sources": []})).to_line(), r#"{"ok":true,"sources":[]}"#);
        let back: Reply = serde_json::from_str(r#"{"ok":true,"reload":"live"}"#).unwrap();
        assert_eq!(back.extra["reload"], "live");
        let e = Event::Final { text: "hi".into(), cancelled: false, audio_ms: 5, finalize_ms: 2, raw: "hi".into(), keys: vec![] };
        assert_eq!(e.to_json(), r#"{"event":"final","text":"hi","cancelled":false,"audio_ms":5,"finalize_ms":2,"raw":"hi","keys":[]}"#);
        assert_eq!(Event::State { state: State::Listening }.to_json(), r#"{"event":"state","state":"listening"}"#);
        let l = Event::Level { db: -35.2, speech: true };
        assert_eq!(l.to_json(), r#"{"event":"level","db":-35.2,"speech":true}"#);
        let back: Event = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(back, e);
        // ältere Zeilen ohne raw/keys bleiben lesbar
        let old: Event = serde_json::from_str(r#"{"event":"final","text":"a","cancelled":false,"audio_ms":1,"finalize_ms":1}"#).unwrap();
        assert!(matches!(old, Event::Final { ref raw, ref keys, .. } if raw.is_empty() && keys.is_empty()));
        let o = Event::Overlay { overlay: OverlayCfg::default() };
        assert_eq!(o.to_json(), r#"{"event":"overlay","overlay":{"position":"bottom","max_width":720,"transparency":true,"opacity":0.8,"bottom_margin":48}}"#);
        assert_eq!(serde_json::from_str::<Event>(&o.to_json()).unwrap(), o);
        let t = Event::Terminals { classes: vec!["Alacritty".into(), "my\tterm".into()] };
        assert_eq!(t.to_json(), r#"{"event":"terminals","classes":["Alacritty","my\tterm"]}"#);
    }
}
