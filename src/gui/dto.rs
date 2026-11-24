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

//! Einstellungen als GUI-eigene Datenstrukturen (gegen die SPEC programmiert, serde aus dem JSON von `get_config`).
//! Unbekannte Schlüssel bleiben in `extra` erhalten und gehen unverändert zurück an den Daemon.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Geordnete Zuordnung (JSON-Objekt) als Liste von Paaren, damit die Zeilen im Editor ihre Reihenfolge behalten.
pub mod pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(v: &[(String, String)], s: S) -> Result<S::Ok, S::Error> {
        let mut m = BTreeMap::new();
        for (k, val) in v {
            if !k.trim().is_empty() {
                m.insert(k.clone(), val.clone());
            }
        }
        m.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
        let m = BTreeMap::<String, String>::deserialize(d)?;
        Ok(m.into_iter().collect())
    }
}

/// Wie [`pairs`], aber ohne Einträge mit leerem Wert: ein leeres Modell-Feld heißt „Standardmodell“ und gehört nicht
/// in die Konfiguration (der Daemon lehnt leere Werte in `ai.mode_models` ab).
pub mod nonempty_pairs {
    use serde::{Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(v: &[(String, String)], s: S) -> Result<S::Ok, S::Error> {
        let m: BTreeMap<&str, &str> = v.iter().map(|(k, val)| (k.trim(), val.trim())).filter(|(k, val)| !k.is_empty() && !val.is_empty()).collect();
        m.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
        super::pairs::deserialize(d)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub voice_commands: bool,
    pub vocabulary: bool,
    pub history: bool,
    pub profiles: bool,
    pub ai: bool,
    pub actions: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features { voice_commands: true, vocabulary: true, history: false, profiles: true, ai: false, actions: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    pub position: String,
    pub max_width: u32,
    pub transparency: bool,
    pub opacity: f64,
    pub bottom_margin: u32,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay { position: "bottom".into(), max_width: 720, transparency: true, opacity: 0.8, bottom_margin: 48 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Typing {
    pub delay_ms: u32,
    pub trailing_space: bool,
    /// Nur `dictate-ui` (Wayland): `paste` oder `type` (`config::TypingCfg::output`)
    pub output: String,
    /// Nur `dictate-ui`: `app_id`s ohne Eingabemethode (`config::TypingCfg::keyboard_only_app_ids`)
    pub keyboard_only_app_ids: Vec<String>,
}

impl Default for Typing {
    fn default() -> Self {
        Typing { delay_ms: 12, trailing_space: true, output: "paste".into(), keyboard_only_app_ids: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryCfg {
    pub max_entries: u32,
}

impl Default for HistoryCfg {
    fn default() -> Self {
        HistoryCfg { max_entries: 200 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vocabulary {
    pub terms: Vec<String>,
    #[serde(with = "pairs")]
    pub replacements: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub match_class: Vec<String>,
    pub punctuation: String,
    pub capitalize_first: bool,
    pub trailing_space: bool,
    pub ai_mode: String,
    pub language: String,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: String::new(),
            match_class: Vec::new(),
            punctuation: "keep".into(),
            capitalize_first: true,
            trailing_space: true,
            ai_mode: String::new(),
            language: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    /// Anbieter-Vorauswahl (leer = Custom)
    pub preset: String,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub api_key_file: String,
    pub timeout_ms: u32,
    pub default_mode: String,
    /// `auto` | `informal` | `formal`
    pub formality: String,
    #[serde(with = "pairs")]
    pub modes: Vec<(String, String)>,
    /// Modus → Modell; fehlt ein Modus, gilt `model`
    #[serde(with = "nonempty_pairs")]
    pub mode_models: Vec<(String, String)>,
}

impl Ai {
    /// Modell-Eintrag des Modus `mode` (leer = Standardmodell).
    pub fn mode_model(&self, mode: &str) -> &str {
        self.mode_models.iter().find(|(k, _)| k == mode).map_or("", |(_, v)| v.as_str())
    }

    /// Setzt das Modell des Modus; leer entfernt den Eintrag.
    pub fn set_mode_model(&mut self, mode: &str, model: &str) {
        self.mode_models.retain(|(k, _)| k != mode);
        if !model.trim().is_empty() {
            self.mode_models.push((mode.to_string(), model.to_string()));
        }
    }

    /// Ein Modus wurde umbenannt: sein Modell geht mit.
    pub fn rename_mode_model(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        let model = self.mode_model(old).to_string();
        self.mode_models.retain(|(k, _)| k != old);
        self.set_mode_model(new, &model);
    }

    /// Die Server-Felder als Daemon-Konfiguration, damit Cloud-Regel, Basis-URL und Modellregeln aus einer Quelle
    /// kommen.
    fn server(&self) -> crate::config::AiCfg {
        crate::config::AiCfg {
            preset: self.preset.clone(),
            provider: self.provider.clone(),
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            ..Default::default()
        }
    }

    /// Vorgabemodell des gewählten Servers (leer = keins, dann ist `model` Pflicht).
    pub fn default_model(&self) -> &'static str {
        self.server().default_model()
    }

    /// Modell, das ohne Modus-Modell gilt: `model`, leer = Vorgabe des Servers.
    pub fn effective_model(&self) -> String {
        self.server().effective_model().to_string()
    }

    /// Anzeigename des Servers für Hinweise („Anthropic (Claude)“, „this server“).
    pub fn server_name(&self) -> String {
        self.server().server_name().to_string()
    }

    /// Basis-URL, die bei leerem Endpoint gilt.
    pub fn base(&self) -> String {
        self.server().resolved().1
    }

    /// Hinweis zum Modellnamen am Feld: was der Daemon beim Speichern ablehnen würde oder was vermutlich nicht passt.
    pub fn model_note(&self, model: &str) -> Option<String> {
        self.server().check_model(model).unwrap_or_else(Some)
    }

    /// Modell der Diagnose mit dem Eintrag `model` der Seite: leer = Sonnet 5.5 bei einem Claude-Server, sonst das
    /// Modell von `ai` (dieselbe Regel wie im Daemon, [`crate::config::DiagnoseCfg::model_for`]).
    pub fn diagnose_model(&self, model: &str) -> String {
        let d = crate::config::DiagnoseCfg { model: model.to_string(), ..Default::default() };
        d.model_for(&self.server()).to_string()
    }

    /// Geht der Text an einen Cloud-Anbieter? (gleiche Regel wie im Daemon)
    pub fn is_cloud(&self) -> bool {
        self.server().is_cloud()
    }

    /// Host des aufgelösten Servers, wie ihn das Aktions-Panel zeigt; ohne gültige URL die Basis selbst.
    pub fn host(&self) -> String {
        let (_, base) = self.server().resolved();
        crate::config::parse_endpoint(&base).map(|e| e.host).unwrap_or(base)
    }
}

impl Default for Ai {
    fn default() -> Self {
        Ai {
            preset: String::new(),
            provider: "ollama".into(),
            endpoint: String::new(),
            // leer = Vorgabemodell des Anbieters, wie im Daemon
            model: crate::config::AiCfg::default().model,
            api_key_file: String::new(),
            timeout_ms: 4000,
            default_mode: String::new(),
            formality: crate::config::AiCfg::default().formality,
            // dieselben Vorgaben wie der Daemon (eine Quelle; `BTreeMap` und `pairs` sortieren gleich)
            modes: crate::config::AiCfg::default().modes.into_iter().collect(),
            mode_models: Vec::new(),
        }
    }
}

/// Textaktionen auf der Markierung; Vorgaben aus `config::ActionsCfg` (eine Quelle).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Actions {
    pub model: String,
    pub timeout_ms: u32,
    pub max_chars: u32,
    pub secrets: String,
    pub language: String,
    pub include_ai_modes: bool,
    pub terminal_classes: Vec<String>,
    #[serde(with = "pairs")]
    pub prompts: Vec<(String, String)>,
}

impl Default for Actions {
    fn default() -> Self {
        let d = crate::config::ActionsCfg::default();
        Actions {
            model: d.model,
            timeout_ms: d.timeout_ms as u32,
            max_chars: d.max_chars as u32,
            secrets: d.secrets,
            language: d.language,
            include_ai_modes: d.include_ai_modes,
            terminal_classes: d.terminal_classes,
            prompts: d.prompts.into_iter().collect(),
        }
    }
}

/// Befehlspalette (Baustein 3); Vorgaben aus `config::PaletteCfg` (eine Quelle). Jedes Feld von `[palette]` steht hier:
/// fehlte eines, ginge es beim Speichern aus dem Fenster verloren (der Daemon setzt Fehlendes auf die Vorgabe).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub enabled: bool,
    pub project_roots: Vec<String>,
    pub project_depth: u32,
    pub frecency: bool,
    pub web_search_url: String,
    /// Modell des KI-Pfads; leer = Modell von `ai` (bzw. Vorgabe des Servers)
    pub model: String,
    pub timeout_ms: u32,
    /// argv des Terminals bis vor den Befehl; leer = automatisch
    pub terminal: Vec<String>,
    /// argv im Projektordner; leer = Shell
    pub project_command: Vec<String>,
    /// argv des Verlaufs der Zwischenablage; leer = Noctalia
    pub clipboard_command: Vec<String>,
    /// argv des Emoji-Wählers; leer = rofi mit rofi-emoji
    pub emoji_command: Vec<String>,
}

impl Default for Palette {
    fn default() -> Self {
        let d = crate::config::PaletteCfg::default();
        Palette {
            enabled: d.enabled,
            project_roots: d.project_roots,
            project_depth: d.project_depth as u32,
            frecency: d.frecency,
            web_search_url: d.web_search_url,
            model: d.model,
            timeout_ms: d.timeout_ms as u32,
            terminal: d.terminal,
            project_command: d.project_command,
            clipboard_command: d.clipboard_command,
            emoji_command: d.emoji_command,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: String,
    pub vad_model: String,
    pub language: String,
    pub source: String,
    pub max_gain_db: u32,
    pub threads: u32,
    pub step_ms: u32,
    pub commit_silence_ms: u32,
    pub max_phrase_ms: u32,
    pub idle_unload_s: u32,
    pub final_beam_size: u32,
    pub max_listen_s: u32,
    pub initial_prompt: String,
    pub blocklist: Vec<String>,
    pub support_url: String,
    pub features: Features,
    pub overlay: Overlay,
    pub typing: Typing,
    pub history: HistoryCfg,
    pub vocabulary: Vocabulary,
    pub profiles: Vec<Profile>,
    pub ai: Ai,
    pub actions: Actions,
    pub palette: Palette,
    /// Diagnose (Baustein 4): der Typ der Config selbst (eine Quelle für Vorgaben, Bereiche und Prüfungen), damit jedes
    /// Feld von `[diagnose]` beim Speichern aus dem Fenster erhalten bleibt; die Seite ist `pages/diagnose.rs`
    pub diagnose: crate::config::DiagnoseCfg,
    /// Alter Top-Level-Schlüssel (Alias): wird nur gelesen, nie zurückgeschrieben.
    #[serde(skip_serializing)]
    trailing_space: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            model: "~/.local/share/dictate/models/ggml-large-v3-turbo-q5_0.bin".into(),
            vad_model: "~/.local/share/dictate/models/ggml-silero-v5.1.2.bin".into(),
            language: "de".into(),
            source: String::new(),
            max_gain_db: 30,
            threads: 4,
            step_ms: 250,
            commit_silence_ms: 700,
            max_phrase_ms: 15000,
            idle_unload_s: 600,
            final_beam_size: 1,
            max_listen_s: 120,
            initial_prompt: String::new(),
            blocklist: [
                "Vielen Dank.",
                "Vielen Dank fürs Zuschauen.",
                "Untertitel im Auftrag des ZDF",
                "Untertitelung",
                "Amara.org",
                "Thanks for watching!",
                "Für You in God.",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            support_url: String::new(),
            features: Features::default(),
            overlay: Overlay::default(),
            typing: Typing::default(),
            history: HistoryCfg::default(),
            vocabulary: Vocabulary::default(),
            profiles: Vec::new(),
            ai: Ai::default(),
            actions: Actions::default(),
            palette: Palette::default(),
            diagnose: crate::config::DiagnoseCfg::default(),
            trailing_space: None,
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// Aus dem JSON von `get_config` bzw. aus der TOML (als JSON-Wert). Der alte Schlüssel `trailing_space`
    /// gilt als Alias für `typing.trailing_space`, wenn dort nichts steht.
    pub fn from_value(v: Value) -> Result<Settings, String> {
        let has_typing_ts = v.get("typing").and_then(|t| t.get("trailing_space")).is_some();
        let mut s: Settings = serde_json::from_value(v).map_err(|e| e.to_string())?;
        if let Some(t) = s.trailing_space.take() {
            if !has_typing_ts {
                s.typing.trailing_space = t;
            }
        }
        s.fill_builtin_prompts();
        Ok(s)
    }

    /// Ergänzt jede eingebaute Textaktion ohne Prompt um ihre Vorgabe (`actions::BUILTIN`). Der Daemon liefert alle
    /// wirksamen Prompts; ein älterer kennt neuere Aktionen noch nicht. Ergänzt wird schon beim Laden, damit gespeicherter
    /// und bearbeiteter Stand gleich bleiben: ergänzte die Seite Actions erst beim Zeichnen, stand gleich nach dem Öffnen
    /// „Unsaved changes“ da (Entscheidung Daniel, 21.11.2026).
    fn fill_builtin_prompts(&mut self) {
        let mut added = false;
        for b in &crate::actions::BUILTIN {
            if !self.actions.prompts.iter().any(|(k, _)| k == b.name) {
                self.actions.prompts.push((b.name.to_string(), b.prompt.to_string()));
                added = true;
            }
        }
        // nach Namen wie beim Laden (`pairs`), sonst ergäbe ein erneutes Laden eine andere Reihenfolge
        if added {
            self.actions.prompts.sort_by(|a, b| a.0.cmp(&b.0));
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("Settings serialisierbar")
    }

    /// Wählt das KI-Preset `id` (aus `config::AI_PRESETS`). Ein anderer Anbieter ist ein anderer Server: Endpoint leer
    /// (Basis des Presets), Schlüsseldatei leer (ein Schlüssel gehört zu einem Anbieter) und Modelle (`ai.model`,
    /// `ai.mode_models`, `actions.model`, `diagnose.model`, `palette.model`) nur, wenn der neue Server sie sicher anbietet (`AiCfg::serves_model`), sonst
    /// leer = Vorgabemodell des neuen Anbieters. Zurück zum gespeicherten Preset stellt dessen Felder wieder her.
    /// Custom behält den Server und macht Protokoll und Endpoint frei editierbar.
    pub fn choose_ai_preset(&mut self, id: &str, saved: &Settings) {
        let Some(p) = crate::config::ai_preset(id) else { return };
        if self.ai.preset == id {
            return;
        }
        if id == "custom" {
            if self.ai.endpoint.trim().is_empty() {
                self.ai.endpoint = self.ai.base();
            }
            self.ai.preset = id.to_string();
            return;
        }
        if saved.ai.preset == id {
            let s = &saved.ai;
            (self.ai.preset, self.ai.provider, self.ai.endpoint) = (s.preset.clone(), s.provider.clone(), s.endpoint.clone());
            (self.ai.api_key_file, self.ai.model, self.ai.mode_models) = (s.api_key_file.clone(), s.model.clone(), s.mode_models.clone());
            self.actions.model = saved.actions.model.clone();
            self.diagnose.model = saved.diagnose.model.clone();
            self.palette.model = saved.palette.model.clone();
            return;
        }
        self.ai.preset = p.id.to_string();
        self.ai.provider = p.protocol.to_string();
        self.ai.endpoint.clear();
        self.ai.api_key_file.clear();
        let server = self.ai.server();
        if !server.serves_model(&self.ai.model) {
            self.ai.model.clear();
        }
        self.ai.mode_models.retain(|(_, m)| server.serves_model(m));
        if !server.serves_model(&self.actions.model) {
            self.actions.model.clear();
        }
        // sonst bliebe ein Claude-Modell der Diagnose stehen, und Apply scheiterte an einem Feld einer anderen Seite
        if !server.serves_model(&self.diagnose.model) {
            self.diagnose.model.clear();
        }
        if !server.serves_model(&self.palette.model) {
            self.palette.model.clear();
        }
    }

    /// Namen der KI-Modi in Anzeigereihenfolge.
    pub fn mode_names(&self) -> Vec<String> {
        self.ai.modes.iter().map(|(k, _)| k.clone()).filter(|k| !k.trim().is_empty()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_keeps_unknown_keys() {
        let v = json!({"language":"en","future_key":{"a":1},"ai":{"modes":{"b":"x","a":"y"}}});
        let s = Settings::from_value(v).unwrap();
        assert_eq!(s.language, "en");
        assert_eq!(s.ai.modes[0].0, "a");
        let out = s.to_value();
        assert_eq!(out["future_key"]["a"], 1);
        assert_eq!(out["ai"]["modes"]["b"], "x");
        assert!(out.get("trailing_space").is_none());
    }

    #[test]
    fn missing_builtin_prompts_are_filled_on_load() {
        // wie ein älterer Daemon (und tests/gui/mock_daemon.py): `explain_code` fehlt, `explain` ist geändert
        let v = json!({"actions": {"prompts": {"explain": "Own prompt."}}});
        let s = Settings::from_value(v).unwrap();
        let names: Vec<&str> = s.actions.prompts.iter().map(|(k, _)| k.as_str()).collect();
        for b in &crate::actions::BUILTIN {
            assert!(names.contains(&b.name), "{} is filled", b.name);
        }
        assert_eq!(names.len(), crate::actions::BUILTIN.len(), "each action once");
        let prompt = |n: &str| s.actions.prompts.iter().find(|(k, _)| k == n).map(|(_, p)| p.clone());
        assert_eq!(prompt("explain").as_deref(), Some("Own prompt."), "a changed prompt stays");
        let code = crate::actions::BUILTIN.iter().find(|b| b.name == "explain_code").unwrap();
        assert_eq!(prompt("explain_code").as_deref(), Some(code.prompt));
        // geladen = bearbeitet: ein zweites Laden desselben Stands ergibt dasselbe (kein „Unsaved changes“)
        assert_eq!(Settings::from_value(s.to_value()).unwrap().actions.prompts, s.actions.prompts);
    }

    #[test]
    fn ai_defaults_mirror_the_daemon() {
        // Das Fenster zeigt vor dem ersten Speichern dieselben Vorgaben, die der Daemon anwendet (auch die Prompts).
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(Settings::default().ai, from_daemon.ai);
        let prompt = |name: &str| Settings::default().ai.modes.into_iter().find(|(k, _)| k == name).unwrap().1;
        assert_eq!(prompt("to_de"), crate::config::PROMPT_TO_DE);
        assert_eq!(prompt("to_en"), crate::config::PROMPT_TO_EN);
    }

    #[test]
    fn mode_models_follow_renames_and_skip_empty_values() {
        let mut a = Ai::default();
        a.set_mode_model("to_de", "claude-sonnet-5-5");
        a.set_mode_model("clean", "");
        assert_eq!(a.mode_model("to_de"), "claude-sonnet-5-5");
        a.rename_mode_model("to_de", "german");
        assert_eq!((a.mode_model("to_de"), a.mode_model("german")), ("", "claude-sonnet-5-5"));
        a.mode_models.push(("email".into(), "  ".into()));
        a.formality = "formal".into();
        let s = Settings { ai: a, ..Settings::default() };
        let out = s.to_value();
        assert_eq!(out["ai"]["mode_models"], json!({"german": "claude-sonnet-5-5"}));
        assert_eq!(out["ai"]["formality"], "formal");
        // was das Fenster schickt, nimmt der Daemon an (bis auf den unbekannten Modus)
        let mut cfg = crate::config::Config::from_value(Settings::default().to_value()).unwrap();
        assert!(cfg.ai.mode_models.is_empty());
        cfg.ai.mode_models.insert("to_de".into(), "x".into());
        let back = Settings::from_value(cfg.to_json()).unwrap();
        assert_eq!(back.ai.mode_model("to_de"), "x");
    }

    #[test]
    fn input_gain_default_mirrors_the_daemon() {
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(Settings::default().max_gain_db, from_daemon.max_gain_db);
        assert_eq!(from_daemon.max_gain_db, crate::config::DEFAULT_MAX_GAIN_DB);
        let cfg = crate::config::Config::from_value(Settings { max_gain_db: 0, ..Settings::default() }.to_value()).unwrap();
        assert_eq!(cfg.max_gain_db, 0);
    }

    #[test]
    fn actions_defaults_mirror_the_daemon() {
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(Settings::default().actions, from_daemon.actions);
        assert!(!from_daemon.features.actions);
        let v = json!({"features": {"actions": true}, "actions": {"language": "en"}});
        let out = Settings::from_value(v).unwrap().to_value();
        assert_eq!(out["features"]["actions"], true);
        assert_eq!(out["actions"]["language"], "en");
        assert!(out.get("extra").is_none());
    }

    #[test]
    fn palette_defaults_mirror_the_daemon_and_the_section_survives() {
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(Settings::default().palette, from_daemon.palette);
        assert!(from_daemon.extra.is_empty(), "{:?}", from_daemon.extra);
        // was das Fenster schickt, nimmt der Daemon an, und eine geänderte Einstellung bleibt erhalten
        let cfg = crate::config::Config::from_value(Settings::default().to_value()).unwrap();
        assert_eq!(cfg.palette, crate::config::PaletteCfg::default());
        let v = json!({"palette": {"enabled": false, "project_roots": ["/srv/git"], "project_depth": 3}});
        let out = Settings::from_value(v).unwrap().to_value();
        assert_eq!(
            out["palette"],
            json!({
                "enabled": false, "project_roots": ["/srv/git"], "project_depth": 3, "frecency": true,
                "web_search_url": "https://duckduckgo.com/?q={query}", "model": "", "timeout_ms": 8000,
                "terminal": [], "project_command": [], "clipboard_command": [], "emoji_command": []
            })
        );
        assert!(out.get("extra").is_none());
    }

    #[test]
    fn diagnose_section_mirrors_the_daemon_and_survives() {
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(from_daemon.diagnose, crate::config::DiagnoseCfg::default());
        let v = json!({"diagnose": {"enabled": true, "rounds": 2, "ping_targets": [{"name": "nas", "address": "nas.example.org"}]}});
        let s = Settings::from_value(v).unwrap();
        assert!(s.extra.is_empty());
        let cfg = crate::config::Config::from_value(s.to_value()).unwrap();
        assert_eq!((cfg.diagnose.enabled, cfg.diagnose.rounds, cfg.diagnose.ping_targets.len()), (true, 2, 1));
    }

    #[test]
    fn diagnose_page_fields_survive_a_round_trip_through_the_daemon_config() {
        // jedes Feld, das die Seite „Diagnose“ ändert, kommt unverändert beim Daemon an und wieder zurück
        let s = Settings {
            diagnose: crate::config::DiagnoseCfg {
                enabled: true,
                model: "claude-opus-5-5".into(),
                timeout_ms: 120_000,
                report_kib: 96,
                rounds: 5,
                temp_log_dir: "~/logs/temps".into(),
                ping_targets: vec![
                    crate::config::PingTargetEntry { name: "gateway".into(), address: "192.0.2.1".into() },
                    crate::config::PingTargetEntry { name: "nas".into(), address: "nas.example.org".into() },
                ],
                pseudonymize: false,
                boot_hint: false,
            },
            ..Settings::default()
        };
        let cfg = crate::config::Config::from_value(s.to_value()).unwrap();
        assert_eq!(cfg.diagnose, s.diagnose);
        assert_eq!(Settings::from_value(cfg.to_json()).unwrap().diagnose, s.diagnose);
        // in die Datei kommen nur die Abweichungen von der Vorgabe
        let out = cfg.render_toml("").unwrap();
        for line in [
            "enabled = true",
            "model = \"claude-opus-5-5\"",
            "timeout_ms = 120000",
            "report_kib = 96",
            "rounds = 5",
            "pseudonymize = false",
            "boot_hint = false",
        ] {
            assert!(out.contains(line), "{line}: {out}");
        }
        assert!(out.contains("name = \"nas\""), "{out}");
    }

    #[test]
    fn diagnose_model_follows_the_server_like_the_daemon() {
        // leer: Sonnet 5.5 bei einem Claude-Server, sonst das Modell von `ai` bzw. die Vorgabe des Servers
        let claude = Ai { preset: "anthropic".into(), provider: "anthropic".into(), ..Ai::default() };
        assert_eq!(claude.diagnose_model(""), crate::config::DIAGNOSE_ANTHROPIC_DEFAULT_MODEL);
        assert_eq!(claude.diagnose_model("  "), crate::config::DIAGNOSE_ANTHROPIC_DEFAULT_MODEL);
        assert_eq!(claude.diagnose_model("claude-opus-5-5"), "claude-opus-5-5");
        let local = Ai::default();
        assert_eq!(local.diagnose_model(""), local.effective_model());
        let own = Ai { model: "llama3.1:8b".into(), ..Ai::default() };
        assert_eq!(own.diagnose_model(""), "llama3.1:8b");
        // ohne Vorgabe (OpenAI): leer, das Fenster zeigt dann „not set yet“
        let openai = Ai { preset: "openai".into(), provider: "openai".into(), ..Ai::default() };
        assert_eq!(openai.diagnose_model(""), "");
        // dieselbe Antwort wie der Daemon
        let s = Settings { ai: claude.clone(), ..Settings::default() };
        let cfg = crate::config::Config::from_value(s.to_value()).unwrap();
        assert_eq!(cfg.diagnose.model_for(&cfg.ai), claude.diagnose_model(""));
    }

    #[test]
    fn palette_page_fields_survive_a_round_trip_through_the_daemon_config() {
        // jedes Feld, das die Seite „Palette“ ändert, kommt unverändert beim Daemon an und wieder zurück
        let s = Settings {
            palette: Palette {
                enabled: false,
                project_roots: vec!["~/work".into(), "/srv/git".into()],
                project_depth: 4,
                frecency: false,
                web_search_url: "https://example.org/search?s={query}".into(),
                model: "claude-sonnet-5-5".into(),
                timeout_ms: 15_000,
                terminal: vec!["wezterm".into(), "start".into(), "--".into()],
                project_command: vec!["claude".into()],
                clipboard_command: vec!["copyq".into(), "toggle".into()],
                emoji_command: vec!["bemoji".into()],
            },
            ..Settings::default()
        };
        let cfg = crate::config::Config::from_value(s.to_value()).unwrap();
        assert_eq!(cfg.palette.terminal, ["wezterm", "start", "--"]);
        assert_eq!(cfg.palette.project_command, ["claude"]);
        assert_eq!(cfg.palette.clipboard_command, ["copyq", "toggle"]);
        assert_eq!(cfg.palette.emoji_command, ["bemoji"]);
        assert_eq!(
            (cfg.palette.enabled, cfg.palette.project_depth, cfg.palette.frecency, cfg.palette.model.as_str(), cfg.palette.timeout_ms),
            (false, 4, false, "claude-sonnet-5-5", 15_000)
        );
        assert_eq!(cfg.palette.project_roots, ["~/work", "/srv/git"]);
        assert_eq!(Settings::from_value(cfg.to_json()).unwrap().palette, s.palette);
    }

    #[test]
    fn server_host_and_cloud_follow_the_daemon_rules() {
        let a = Ai::default();
        assert_eq!((a.host(), a.is_cloud()), ("127.0.0.1".to_string(), false));
        let a = Ai { preset: "anthropic".into(), provider: "anthropic".into(), ..Ai::default() };
        assert_eq!((a.host(), a.is_cloud()), ("api.anthropic.com".to_string(), true));
        let a = Ai { preset: "custom".into(), provider: "openai".into(), endpoint: "http://localhost:8080/v1".into(), ..Ai::default() };
        assert_eq!((a.host(), a.is_cloud()), ("localhost".to_string(), false));
    }

    #[test]
    fn anthropic_preset_gets_a_claude_model_and_writes_no_modes() {
        // der Live-Test: Standard-Einstellungen, Preset Anthropic, Apply
        let mut s = Settings::default();
        s.features.ai = true;
        let saved = s.clone();
        s.choose_ai_preset("anthropic", &saved);
        assert_eq!((s.ai.provider.as_str(), s.ai.endpoint.as_str(), s.ai.model.as_str()), ("anthropic", "", ""));
        assert_eq!(s.ai.effective_model(), crate::config::ANTHROPIC_DEFAULT_MODEL);
        assert_eq!(s.ai.base(), "https://api.anthropic.com");
        s.ai.api_key_file = "~/.config/anthropic/api-key".into();
        let cfg = crate::config::Config::from_value(s.to_value()).unwrap();
        cfg.validate_new().unwrap();
        assert_eq!(cfg.ai.effective_model(), "claude-haiku-5-5");
        let out = cfg.render_toml("").unwrap();
        assert!(out.contains("preset = \"anthropic\"") && !out.contains("model") && !out.contains("[ai.modes]") && !out.contains("endpoint"), "{out}");
    }

    #[test]
    fn preset_change_clears_models_and_key_of_the_old_provider() {
        let mut s = Settings::default();
        s.ai.preset = "ollama".into();
        s.ai.model = "llama3.1:8b".into();
        s.ai.set_mode_model("to_de", "qwen2.5:14b");
        s.actions.model = "llama3.1:8b".into();
        let saved = s.clone();
        s.choose_ai_preset("anthropic", &saved);
        assert!(s.ai.model.is_empty() && s.ai.mode_models.is_empty() && s.actions.model.is_empty());
        // Claude-Modelle bleiben bei einem Server, der sie sicher anbietet (Custom mit Anthropic-Protokoll)
        s.ai.model = "claude-sonnet-5-5".into();
        s.ai.set_mode_model("to_de", "claude-opus-5-5");
        s.ai.api_key_file = "~/.config/anthropic/api-key".into();
        s.choose_ai_preset("custom", &saved);
        assert_eq!((s.ai.preset.as_str(), s.ai.provider.as_str(), s.ai.endpoint.as_str()), ("custom", "anthropic", "https://api.anthropic.com"));
        assert_eq!((s.ai.model.as_str(), s.ai.mode_model("to_de")), ("claude-sonnet-5-5", "claude-opus-5-5"));
        s.choose_ai_preset("anthropic", &saved);
        assert_eq!((s.ai.endpoint.as_str(), s.ai.model.as_str()), ("", "claude-sonnet-5-5"));
        // anderer Anbieter: weder Claude-Modelle noch der Anthropic-Schlüssel gehen mit
        s.choose_ai_preset("openai", &saved);
        assert!(s.ai.model.is_empty() && s.ai.mode_models.is_empty() && s.ai.api_key_file.is_empty());
        assert_eq!(s.ai.default_model(), "");
        assert!(s.ai.model_note("claude-haiku-5-5").is_some());
        // zurück zum gespeicherten Preset: dessen Felder kommen wieder
        s.choose_ai_preset("ollama", &saved);
        assert!(s.ai.preset == "ollama" && s.ai.model == "llama3.1:8b" && s.ai.mode_model("to_de") == "qwen2.5:14b" && s.actions.model == "llama3.1:8b");
    }

    #[test]
    fn preset_change_handles_the_diagnose_model_like_the_action_model() {
        let mut s = Settings::default();
        s.ai.preset = "ollama".into();
        s.diagnose.model = "llama3.1:8b".into();
        let saved = s.clone();
        // ein anderer Anbieter bietet das Modell nicht sicher an: leer = Vorgabe der Diagnose (Sonnet 5.5 bei Claude)
        s.choose_ai_preset("anthropic", &saved);
        assert!(s.diagnose.model.is_empty());
        assert_eq!(s.ai.diagnose_model(&s.diagnose.model), crate::config::DIAGNOSE_ANTHROPIC_DEFAULT_MODEL);
        // ein Claude-Modell bleibt bei einem Server, der es sicher anbietet, und geht bei einem anderen Anbieter
        s.diagnose.model = "claude-opus-5-5".into();
        s.choose_ai_preset("custom", &saved);
        assert_eq!(s.diagnose.model, "claude-opus-5-5");
        s.choose_ai_preset("openai", &saved);
        assert!(s.diagnose.model.is_empty());
        // zurück zum gespeicherten Preset: der gespeicherte Eintrag kommt wieder
        s.choose_ai_preset("ollama", &saved);
        assert_eq!(s.diagnose.model, "llama3.1:8b");
        // und die Datei, die daraus entsteht, ist gültig
        let mut t = Settings::default();
        t.diagnose.enabled = true;
        t.diagnose.model = "claude-sonnet-5-5".into();
        let saved = t.clone();
        t.choose_ai_preset("openai", &saved);
        crate::config::Config::from_value(t.to_value()).unwrap().validate_new().unwrap();
    }

    #[test]
    fn preset_change_handles_the_palette_model_like_the_other_models() {
        let mut s = Settings::default();
        s.ai.preset = "ollama".into();
        s.palette.model = "llama3.1:8b".into();
        let saved = s.clone();
        // ein anderer Anbieter bietet das Modell nicht sicher an: leer = Modell von [ai]
        s.choose_ai_preset("anthropic", &saved);
        assert!(s.palette.model.is_empty());
        s.palette.model = "claude-haiku-5-5".into();
        s.choose_ai_preset("custom", &saved);
        assert_eq!(s.palette.model, "claude-haiku-5-5", "custom keeps the server");
        s.choose_ai_preset("openai", &saved);
        assert!(s.palette.model.is_empty());
        // zurück zum gespeicherten Preset: der gespeicherte Eintrag kommt wieder
        s.choose_ai_preset("ollama", &saved);
        assert_eq!(s.palette.model, "llama3.1:8b");
        // Apply scheitert nicht mehr an einem Feld der Seite Palette, die beim Wechsel gar nicht offen war
        let mut t = Settings::default();
        t.palette.model = "claude-sonnet-5-5".into();
        let saved = t.clone();
        t.choose_ai_preset("openai", &saved);
        crate::config::Config::from_value(t.to_value()).unwrap().validate_new().unwrap();
    }

    #[test]
    fn typing_defaults_mirror_the_daemon_and_the_wayland_keys_survive() {
        let from_daemon = Settings::from_value(crate::config::Config::default().to_json()).unwrap();
        assert_eq!(Settings::default().typing, from_daemon.typing);
        // Ein Speichern im Fenster verliert die Schlüssel von dictate-ui nicht (unbekannte Schlüssel in `typing`
        // fielen sonst still weg, `extra` hält nur die oberste Ebene)
        let v = json!({"typing": {"delay_ms": 5, "output": "type", "keyboard_only_app_ids": ["com.example.App"]}});
        let out = Settings::from_value(v).unwrap().to_value();
        assert_eq!(out["typing"]["output"], "type");
        assert_eq!(out["typing"]["keyboard_only_app_ids"], json!(["com.example.App"]));
        let cfg = crate::config::Config::from_value(out).unwrap();
        assert_eq!(cfg.typing.output, "type");
        assert_eq!(cfg.typing.keyboard_only_app_ids, vec!["com.example.App".to_string()]);
    }

    #[test]
    fn legacy_trailing_space_alias() {
        let s = Settings::from_value(json!({"trailing_space": false})).unwrap();
        assert!(!s.typing.trailing_space);
        let s = Settings::from_value(json!({"trailing_space": false, "typing": {"trailing_space": true}})).unwrap();
        assert!(s.typing.trailing_space);
    }
}
