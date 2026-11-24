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

//! KI-Nachbearbeitung (Schritt 4 der Pipeline): ein Chat-Aufruf an Ollama, einen OpenAI-kompatiblen Server oder
//! die Claude API. Blockierend über den eigenen Client [`crate::http`] (abbrechbar, ohne Hilfsthread), keine Async-Runtime. Texte werden nie geloggt (außer mit `debug_text()`),
//! der API-Schlüssel kommt nur aus `api_key_file` und taucht in keiner Fehlermeldung auf.

pub use crate::config::{ai_preset as preset, with_guard, AiPreset as Preset, AI_PRESETS as PRESETS, INPUT_GUARD};
use crate::config::{parse_endpoint, AiCfg};
use crate::http;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

/// Abbruch-Flag: `true` bricht einen laufenden Aufruf ab (siehe [`call`]).
pub type CancelFlag = Arc<AtomicBool>;

/// Standard-Endpoint der Claude API
pub const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Mehr als das Dreifache der Eingabelänge (mindestens aber Eingabe + 40 Zeichen, damit kurze Eingaben wachsen
/// dürfen) gilt als Ausreißer (Modell plaudert statt umzuschreiben). Ausnahme: der Modus `prompt` ([`Shape`]).
const MAX_GROWTH: usize = 3;
const MIN_GROWTH_CHARS: usize = 40;
/// Mindest-Zeitgrenze des Modus `prompt` beim Diktat: Haiku braucht für einen gegliederten Prompt mehrere Sekunden,
/// die Vorgabe `ai.timeout_ms` (4 s) ist für kurze Korrekturen bemessen. Abbrechen geht wie immer (Mod+Ctrl+e).
const REFINE_MIN_TIMEOUT_MS: u64 = 20_000;

/// Was die Antwort eines Modus gegenüber der Eingabe darf. Die Modi korrigieren, übersetzen oder formatieren ihre
/// Eingabe, darum sind Länge und Zeilen eng begrenzt (Schutz gegen ein Modell, das plaudert oder antwortet). Zwei
/// Modi erzeugen ihrer Aufgabe nach Struktur: `email` (Gruß, Absätze) und `prompt` (gegliederter, höchstens dreifach
/// so langer Prompt; getippt wird bis zur harten Grenze [`crate::config::refine_max_chars`], darüber gilt er als
/// ausgeführt statt umgeschrieben und es kommt der Rohtext). Gilt nach dem Namen, also auch für einen selbst geänderten Prompt dieser Modi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    /// Neue Zeilenumbrüche erlaubt (sonst werden zusätzliche zu Leerzeichen)
    lines: bool,
    /// `prompt`: Längengrenze [`crate::config::refine_max_chars`], eigene `max_tokens`, Mindest-Zeitgrenze
    refine: bool,
}

fn shape(mode: &str) -> Shape {
    let refine = mode.eq_ignore_ascii_case(crate::config::REFINE_MODE);
    Shape { lines: refine || mode.eq_ignore_ascii_case("email"), refine }
}

impl Shape {
    /// Höchstzahl der Zeichen in der Antwort bei `len` Zeichen Eingabe
    fn max_chars(self, len: usize) -> usize {
        if self.refine {
            crate::config::refine_max_chars(len)
        } else {
            (len * MAX_GROWTH).max(len + MIN_GROWTH_CHARS)
        }
    }

    /// `max_tokens` der Anfrage: `None` = Vorgabe ([`anthropic_max_tokens`] bzw. die des Servers). Beim Modus
    /// `prompt` passend zu [`Shape::max_chars`], gerechnet wie bei den Textaktionen (`crate::actions::max_tokens`).
    /// Längenziel in Zeichen, das die Anfrage dem Modell als festen Zusatz nennt ([`crate::config::length_budget_note`]);
    /// `None` = der Modus hat keins. Ein weiterer Modus mit Längenziel trägt hier seine Zahl ein, die übrigen bleiben
    /// unberührt. Der Zusatz gilt nach dem Modusnamen, also auch für einen selbst geänderten Prompt.
    fn budget(self, len: usize) -> Option<usize> {
        self.refine.then(|| crate::config::refine_target_chars(len))
    }

    fn max_tokens(self, len: usize) -> Option<u64> {
        self.refine.then(|| crate::actions::replace_tokens(self.max_chars(len)))
    }

    fn timeout_ms(self, configured: u64) -> u64 {
        if self.refine {
            configured.max(REFINE_MIN_TIMEOUT_MS)
        } else {
            configured
        }
    }
}
/// Obergrenze für Antworten, die wir lesen
const MAX_BODY: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// Zeitüberschreitung nach `timeout_ms`
    Timeout(u64),
    /// Vom User abgebrochen (`cancel`); kein Fehler für die Anzeige
    Cancelled,
    /// Fehlerstatus des Servers. `message` ist Servertext (`error.message`, höchstens 300 Zeichen, kann leer sein, bei
    /// 401/403 immer leer): er darf in Logs und Anzeigen des Einstellungsfensters stehen, aber nicht in Antworten, die
    /// an einen Client gehen, weil ein Server darin die Eingabe zitieren könnte ([`AiError::client_text`]). Die
    /// Lernlogik für Denk- und Werkzeug-Einstellungen liest ihn weiter.
    Http {
        code: u16,
        message: String,
    },
    /// Fehler mitten im gestreamten Antworttext; der Inhalt ist Servertext (wie `message` bei [`AiError::Http`]): in Logs
    /// und Anzeigen des Einstellungsfensters ja, in Antworten an einen Client nie ([`AiError::client_text`]).
    Stream(String),
    Other(String),
}

impl AiError {
    fn other(m: impl Into<String>) -> AiError {
        AiError::Other(m.into())
    }

    /// Text für das `error`-Event des Daemons (der Rohtext wird trotzdem getippt)
    pub fn event_message(&self) -> String {
        match self {
            AiError::Timeout(_) => "AI timeout, raw text typed".to_string(),
            AiError::Cancelled => "AI cancelled".to_string(),
            other => format!("AI error: {other}"),
        }
    }

    /// Text für eine Antwort an einen Client (Befehlspalette `pal_ask`, Textaktionen `act_run`): wie `Display`, aber ein
    /// Fehlerstatus des Servers nur als „HTTP <Code> (<feste Meldung>)“ und ein Fehler mitten im Strom nur als feste
    /// Meldung, nie mit Servertext. Alle übrigen Fehler tragen feste oder
    /// lokale Texte (Konfiguration, Verbindung, Format): nichts davon stammt aus der Antwort des Servers.
    pub fn client_text(&self) -> String {
        match self {
            AiError::Http { code, .. } => format!("HTTP {code} ({})", status_hint(*code)),
            AiError::Stream(_) => STREAM_ERROR.to_string(),
            other => other.to_string(),
        }
    }
}

/// Feste Meldung zu einem HTTP-Fehlerstatus für [`AiError::client_text`]
fn status_hint(code: u16) -> &'static str {
    match code {
        401 | 403 => "check API key",
        400 | 422 => "the server rejected the request",
        404 => "model or address not found",
        408 | 504 => "the server did not answer in time",
        413 => "the request is too large",
        429 => "rate limit or quota reached",
        500..=599 => "server error",
        _ => "unexpected answer from the server",
    }
}

/// „HTTP 400: Meldung“, „HTTP 400“ ohne Meldung, bei 401/403 „HTTP 401 (check API key)“
fn http_text(code: u16, message: &str) -> String {
    if matches!(code, 401 | 403) {
        format!("HTTP {code} (check API key)")
    } else if message.is_empty() {
        format!("HTTP {code}")
    } else {
        format!("HTTP {code}: {message}")
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::Timeout(ms) => write!(f, "timed out after {ms} ms"),
            AiError::Cancelled => f.write_str("cancelled"),
            AiError::Http { code, message } => f.write_str(&http_text(*code, message)),
            AiError::Stream(m) if m.is_empty() => f.write_str(STREAM_ERROR),
            AiError::Stream(m) => write!(f, "{STREAM_ERROR}: {m}"),
            AiError::Other(m) => f.write_str(m),
        }
    }
}

/// Die unterstützten Anbieter. Ein neuer Anbieter braucht nur einen weiteren Zweig in den Methoden unten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    OpenAi,
    Anthropic,
}

impl Provider {
    pub fn parse(name: &str) -> Result<Provider, AiError> {
        match name {
            "ollama" => Ok(Provider::Ollama),
            "openai" => Ok(Provider::OpenAi),
            "anthropic" => Ok(Provider::Anthropic),
            other => Err(AiError::other(format!("unknown provider \"{other}\""))),
        }
    }

    /// Schlüssel nötig? (Ollama nie, OpenAI-kompatible lokale Server oft nicht, Anthropic immer)
    fn key_required(self) -> bool {
        self == Provider::Anthropic
    }

    /// `{base}/{path}`; steht bei Anthropic `/v1` schon in der Basis, wird es nicht verdoppelt.
    fn url(self, base: &str, path: &str) -> String {
        match (self, path.strip_prefix("v1/")) {
            (Provider::Anthropic, Some(rest)) if base.ends_with("/v1") => format!("{base}/{rest}"),
            _ => format!("{base}/{path}"),
        }
    }

    fn chat_path(self) -> &'static str {
        match self {
            Provider::Ollama => "api/chat",
            Provider::OpenAi => "chat/completions",
            Provider::Anthropic => "v1/messages",
        }
    }

    fn models_path(self) -> &'static str {
        match self {
            Provider::Ollama => "api/tags",
            Provider::OpenAi => "models",
            Provider::Anthropic => "v1/models",
        }
    }

    fn headers(self, key: Option<&str>) -> Vec<(&'static str, String)> {
        let mut h = vec![("content-type", "application/json".to_string())];
        match (self, key) {
            (Provider::Anthropic, Some(k)) => {
                h.push(("x-api-key", k.to_string()));
                h.push(("anthropic-version", ANTHROPIC_VERSION.to_string()));
            }
            (Provider::Anthropic, None) => h.push(("anthropic-version", ANTHROPIC_VERSION.to_string())),
            (Provider::OpenAi, Some(k)) => h.push(("authorization", format!("Bearer {k}"))),
            _ => {}
        }
        h
    }

    fn parse_chat(self, v: &Value) -> Option<String> {
        match self {
            Provider::Ollama => v.pointer("/message/content").and_then(Value::as_str).map(str::to_string),
            Provider::OpenAi => v.pointer("/choices/0/message/content").and_then(Value::as_str).map(str::to_string),
            Provider::Anthropic => {
                let blocks = v.get("content")?.as_array()?;
                Some(
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect(),
                )
            }
        }
    }

    /// Claude: Name und Eingabe des ersten `tool_use`-Blocks (andere Anbieter antworten strukturiert im Text)
    fn parse_tool(self, v: &Value) -> Option<(String, Value)> {
        if self != Provider::Anthropic {
            return None;
        }
        let block = v.get("content")?.as_array()?.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))?;
        Some((block.get("name")?.as_str()?.to_string(), block.get("input")?.clone()))
    }

    /// Verbrauchte Tokens laut Antwort: Claude `usage.input_tokens`/`output_tokens`, OpenAI `usage.prompt_tokens`/
    /// `completion_tokens`, Ollama `prompt_eval_count`/`eval_count`. Fehlt ein Wert, ist er `None`.
    fn parse_usage(self, v: &Value) -> Usage {
        let (input, output) = match self {
            Provider::Ollama => ("/prompt_eval_count", "/eval_count"),
            Provider::OpenAi => ("/usage/prompt_tokens", "/usage/completion_tokens"),
            Provider::Anthropic => ("/usage/input_tokens", "/usage/output_tokens"),
        };
        Usage { input: v.pointer(input).and_then(Value::as_u64), output: v.pointer(output).and_then(Value::as_u64) }
    }

    /// Grund für das Ende der Antwort: Claude `stop_reason`, OpenAI `finish_reason`, Ollama `done_reason`
    fn stop_reason(self, v: &Value) -> Option<&str> {
        let p = match self {
            Provider::Ollama => "/done_reason",
            Provider::OpenAi => "/choices/0/finish_reason",
            Provider::Anthropic => "/stop_reason",
        };
        v.pointer(p).and_then(Value::as_str)
    }

    fn parse_models(self, v: &Value) -> Option<Vec<String>> {
        let (list, field) = match self {
            Provider::Ollama => ("models", "name"),
            Provider::OpenAi | Provider::Anthropic => ("data", "id"),
        };
        let arr = v.get(list)?.as_array()?;
        Some(arr.iter().filter_map(|m| m.get(field).and_then(Value::as_str)).map(str::to_string).collect())
    }
}

/// Großzügig: etwa vier mal die geschätzten Eingabe-Tokens (3 Zeichen je Token), 1024 bis 4096.
fn anthropic_max_tokens(text: &str) -> u64 {
    let est = text.chars().count().div_ceil(3) as u64;
    (est * 4).clamp(1024, 4096)
}

// ---------------------------------------------------------------------------------------------------------------
// Denken (Claude)

/// Wie eine Anfrage an Claude das Denken abschaltet oder klein hält. Für die Nachbearbeitung ist Denken unnötig: es
/// kostet Latenz und verbraucht `max_tokens`. Welcher Parameter gilt, hängt vom Modell ab (Claude-API-Doku, Tabelle
/// „Thinking & Effort“, Stand 2026-10): ein falscher ist HTTP 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thinking {
    /// Parameter weglassen: ohne ihn denkt das Modell nicht (Claude 3.x, Opus/Sonnet/Haiku 4.x bis Opus 4.8)
    Omit,
    /// `{"type": "disabled"}`: Denken ist ohne Parameter an, Abschalten erlaubt (Opus 5, Sonnet 5, Haiku 5.5; bei
    /// Opus 5 und Haiku 5.5 nur bis effort `high`, wir senden keins: Standard `high` bzw. `medium`)
    Disabled,
    /// `{"type": "between_tools"}`: die niedrigste Stufe bei Sonnet 5.5, das `disabled` mit HTTP 400 ablehnt; kein
    /// erweitertes Denken (ohne Werkzeuge also gar keins), kein weiteres Feld erlaubt, effort höchstens `high`
    BetweenTools,
    /// Denken lässt sich nicht abschalten (Opus 5.5, Fable, Mythos): Parameter weglassen, `effort: low`, und Platz in
    /// `max_tokens` für das Denken, damit die Antwort nicht abgeschnitten wird
    LowEffort,
}

/// Reserve in `max_tokens` für Denken, das sich nicht abschalten lässt
const THINKING_RESERVE: u64 = 8192;

/// Modell-ID ohne Plattform-Präfix und Versionszusatz: `anthropic.claude-sonnet-4-5-20250929-v1:0`,
/// `claude-opus-4-5@20251101` und `claude-sonnet-4-5-latest` werden zu `claude-sonnet-4-5` bzw. `claude-opus-4-5`.
fn model_base(model: &str) -> String {
    let m = model.trim().to_ascii_lowercase();
    let mut id = match m.find("claude-") {
        Some(i) => m[i..].to_string(),
        None => return m,
    };
    if let Some(i) = id.find(['@', '[', ':']) {
        id.truncate(i);
    }
    while let Some((head, tail)) = id.rsplit_once('-') {
        let version = tail.len() > 1 && tail.starts_with('v') && tail[1..].bytes().all(|b| b.is_ascii_digit());
        let date = tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit());
        if version || date || tail == "latest" {
            id = head.to_string();
        } else {
            break;
        }
    }
    id
}

/// Was ein Modell laut Doku braucht; unbekannte (neuere) Modelle bekommen `disabled` und lernen aus der Absage
/// (siehe [`thinking_after_error`]).
fn thinking_known(model: &str) -> Thinking {
    let id = model_base(model);
    let family = |p: &str| id == p || id.starts_with(&format!("{p}-"));
    match id.as_str() {
        "claude-sonnet-5-5" => Thinking::BetweenTools,
        "claude-opus-5-5" => Thinking::LowEffort,
        _ if family("claude-fable") || family("claude-mythos") => Thinking::LowEffort,
        "claude-opus-5" | "claude-sonnet-5" | "claude-haiku-5-5" => Thinking::Disabled,
        _ if id.starts_with("claude-3") || family("claude-opus-4") || family("claude-sonnet-4") || family("claude-haiku-4") => Thinking::Omit,
        _ => Thinking::Disabled,
    }
}

/// Je Modell gelernte Einstellung (nach einer Absage), damit der zusätzliche Aufruf nur einmal je Daemon-Lauf anfällt.
fn learned() -> &'static std::sync::Mutex<std::collections::HashMap<String, Thinking>> {
    static L: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Thinking>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

/// Einstellung für `model`: gelernt, sonst laut Doku.
pub fn thinking_for(model: &str) -> Thinking {
    let key = model.trim().to_string();
    learned().lock().ok().and_then(|m| m.get(&key).copied()).unwrap_or_else(|| thinking_known(model))
}

fn learn(model: &str, t: Thinking) {
    if let Ok(mut m) = learned().lock() {
        m.insert(model.trim().to_string(), t);
    }
}

/// Nächster Versuch nach HTTP 400 mit `msg`, `None` wenn die Absage nichts mit Denken oder effort zu tun hat oder
/// nichts mehr übrig ist. Die API nennt die Alternative in der Meldung („Use "thinking.type.between_tools" …“ bzw.
/// „Use "thinking.type.adaptive" and "output_config.effort" …“).
fn thinking_after_error(cur: Thinking, msg: &str) -> Option<Thinking> {
    let m = msg.to_ascii_lowercase();
    let (thinking, effort) = (m.contains("thinking"), m.contains("effort"));
    if !thinking && !effort {
        return None;
    }
    match cur {
        Thinking::Disabled if m.contains("between_tools") => Some(Thinking::BetweenTools),
        Thinking::Disabled | Thinking::BetweenTools => Some(Thinking::LowEffort),
        Thinking::LowEffort => Some(Thinking::Omit),
        Thinking::Omit => None,
    }
}

/// Anfragekörper für die Claude API. Kein `temperature`: aktuelle Modelle lehnen abweichende Werte mit HTTP 400 ab.
/// `max_tokens` ist Pflicht: ohne Vorgabe des Aufrufers [`anthropic_max_tokens`], bei nicht abschaltbarem Denken plus
/// [`THINKING_RESERVE`].
fn anthropic_body(req: &ChatReq, thinking: Thinking, caps: ToolCaps) -> Value {
    let mut max_tokens = req.max_tokens.unwrap_or_else(|| anthropic_max_tokens(req.text));
    let mut b = json!({
        "model": req.model,
        "system": req.system,
        "messages": [{"role": "user", "content": req.text}],
    });
    if let Some(st) = req.structured {
        let tools: Vec<Value> = st
            .tools
            .iter()
            .map(|t| {
                let mut tool = json!({"name": t.name, "description": t.description, "input_schema": t.input_schema});
                if caps.strict {
                    tool["strict"] = json!(true);
                }
                tool
            })
            .collect();
        b["tools"] = Value::Array(tools);
        // genau ein Aufruf: `any` erzwingt ihn, `auto` (wo `any` nicht geht) verlangt ihn nur der Prompt
        let choice = if caps.forced_with(thinking) { "any" } else { "auto" };
        b["tool_choice"] = json!({"type": choice, "disable_parallel_tool_use": true});
    }
    match thinking {
        Thinking::Omit => {}
        Thinking::Disabled => b["thinking"] = json!({"type": "disabled"}),
        Thinking::BetweenTools => b["thinking"] = json!({"type": "between_tools"}),
        Thinking::LowEffort => {
            b["output_config"] = json!({"effort": "low"});
            max_tokens += THINKING_RESERVE;
        }
    }
    b["max_tokens"] = json!(max_tokens);
    b
}

// ---------------------------------------------------------------------------------------------------------------
// Werkzeuge (Claude, nur strukturierte Antworten)

/// Was ein Claude-Modell bei Werkzeugen kann (Claude-API-Doku, „Tool use“ und „Structured outputs“, Stand 2026-10).
/// Unbekannte Fälle lernen aus der Absage wie beim Denken (siehe [`call_anthropic`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCaps {
    /// `tool_choice` `any` erlaubt: Opus 5.5, Sonnet 5.5, Fable 5.1 und Mythos 5.1 lehnen erzwungene Aufrufe mit
    /// HTTP 400 ab; dort `auto` mit der Anweisung im Prompt
    pub forced: bool,
    /// `strict: true` je Werkzeug (die Eingabe passt garantiert zum Schema); Claude 3 kennt es nicht
    pub strict: bool,
}

impl ToolCaps {
    /// Wird der Aufruf erzwungen? Nur ohne Denken: erzwungene Werkzeugwahl und Denken schließen sich aus.
    pub fn forced_with(self, thinking: Thinking) -> bool {
        self.forced && matches!(thinking, Thinking::Omit | Thinking::Disabled)
    }
}

fn tool_caps_known(model: &str) -> ToolCaps {
    let id = model_base(model);
    let family = |p: &str| id == p || id.starts_with(&format!("{p}-"));
    let unforced = matches!(id.as_str(), "claude-opus-5-5" | "claude-sonnet-5-5") || family("claude-fable-5-1") || family("claude-mythos-5-1");
    ToolCaps { forced: !unforced, strict: !id.starts_with("claude-3") }
}

fn learned_tool_caps() -> &'static std::sync::Mutex<std::collections::HashMap<String, ToolCaps>> {
    static L: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, ToolCaps>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

/// Werkzeug-Fähigkeiten für `model`: gelernt, sonst laut Doku.
pub fn tool_caps_for(model: &str) -> ToolCaps {
    let key = model.trim().to_string();
    learned_tool_caps().lock().ok().and_then(|m| m.get(&key).copied()).unwrap_or_else(|| tool_caps_known(model))
}

/// Nächste Werkzeug-Einstellung nach HTTP 400 mit `msg`, `None` wenn die Absage nichts damit zu tun hat.
fn tool_caps_after_error(cur: ToolCaps, thinking: Thinking, msg: &str) -> Option<ToolCaps> {
    let m = msg.to_ascii_lowercase();
    if cur.forced_with(thinking) && m.contains("tool_choice") {
        return Some(ToolCaps { forced: false, ..cur });
    }
    if cur.strict && m.contains("strict") {
        return Some(ToolCaps { strict: false, ..cur });
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------
// OpenAI-kompatibel: Name der Längengrenze

/// Wie ein OpenAI-kompatibler Server die Längengrenze der Antwort heißt. `max_tokens` verstehen fast alle (llama.cpp,
/// LM Studio, vLLM, Groq, Mistral, Gemini); OpenAIs Reasoning-Modelle (o1, o3, o4, gpt-5) lehnen es ab und wollen
/// `max_completion_tokens`. Unbekannte Fälle lernen aus der Absage wie beim Denken (siehe [`call_openai`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenParam {
    MaxTokens,
    MaxCompletionTokens,
}

impl TokenParam {
    fn field(self) -> &'static str {
        match self {
            TokenParam::MaxTokens => "max_tokens",
            TokenParam::MaxCompletionTokens => "max_completion_tokens",
        }
    }

    fn other(self) -> TokenParam {
        match self {
            TokenParam::MaxTokens => TokenParam::MaxCompletionTokens,
            TokenParam::MaxCompletionTokens => TokenParam::MaxTokens,
        }
    }
}

fn token_param_known(model: &str) -> TokenParam {
    // `openai/o3-mini` (OpenRouter) wie `o3-mini`
    let m = model.trim().to_ascii_lowercase();
    let id = m.rsplit('/').next().unwrap_or("");
    let reasoning = ["o1", "o3", "o4", "gpt-5"].iter().any(|p| id == *p || id.starts_with(&format!("{p}-")));
    if reasoning {
        TokenParam::MaxCompletionTokens
    } else {
        TokenParam::MaxTokens
    }
}

fn learned_token_param() -> &'static std::sync::Mutex<std::collections::HashMap<String, TokenParam>> {
    static L: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, TokenParam>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

/// Feldname für `model`: gelernt, sonst nach Modellname.
pub fn token_param_for(model: &str) -> TokenParam {
    let key = model.trim().to_string();
    learned_token_param().lock().ok().and_then(|m| m.get(&key).copied()).unwrap_or_else(|| token_param_known(model))
}

fn openai_body(req: &ChatReq, param: TokenParam) -> Value {
    let mut b = json!({
        "model": req.model,
        "stream": false,
        // kein temperature: neuere Modelle (z. B. Reasoning-Modelle) lehnen abweichende Werte ab
        "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": req.text}],
    });
    if let Some(n) = req.max_tokens {
        b[param.field()] = json!(n);
    }
    if let Some(st) = req.structured {
        b["response_format"] = json!({"type": "json_schema", "json_schema": {"name": st.schema_name, "strict": true, "schema": st.schema}});
    }
    b
}

/// Ollama: Längengrenze heißt `options.num_predict`. `temperature` nur, wenn der Aufrufer sie will (die Modi:
/// lokale Modelle schreiben mit 0.2 verlässlicher um).
fn ollama_body(req: &ChatReq) -> Value {
    let mut b = json!({
        "model": req.model,
        "stream": false,
        "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": req.text}],
    });
    let mut options = serde_json::Map::new();
    if let Some(t) = req.ollama_temperature {
        options.insert("temperature".into(), json!(t));
    }
    if let Some(n) = req.max_tokens {
        options.insert("num_predict".into(), json!(n));
    }
    if !options.is_empty() {
        b["options"] = Value::Object(options);
    }
    if let Some(st) = req.structured {
        b["format"] = st.schema.clone();
    }
    b
}

/// Aufgelöstes Ziel einer Konfiguration: Protokoll und Basis-URL ohne Schrägstrich am Ende (Regeln in
/// [`AiCfg::resolved`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub provider: Provider,
    pub base: String,
}

pub fn resolve(cfg: &AiCfg) -> Result<Target, AiError> {
    cfg.check_endpoint().map_err(|m| AiError::other(format!("ai.endpoint {m}")))?;
    let (protocol, base) = cfg.resolved();
    Ok(Target { provider: Provider::parse(&protocol)?, base })
}

// ---------------------------------------------------------------------------------------------------------------
// Schlüssel

/// Liest den Schlüssel aus der Datei. Ist sie für Gruppe oder andere lesbar, lehnen wir sie ab, wie ssh es tut.
fn read_key(path: &str) -> Result<String, AiError> {
    use std::os::unix::fs::PermissionsExt;
    let path = path.trim();
    let full = match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| path.into()),
        None => path.into(),
    };
    let full: std::path::PathBuf = full;
    let meta = std::fs::metadata(&full).map_err(|e| AiError::other(format!("cannot read API key file {}: {e}", full.display())))?;
    if !meta.is_file() {
        return Err(AiError::other(format!("API key file {} is not a regular file", full.display())));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(AiError::other(format!("API key file {} is accessible by others (mode {mode:04o}), run chmod 600", full.display())));
    }
    let key = std::fs::read_to_string(&full).map_err(|e| AiError::other(format!("cannot read API key file {}: {e}", full.display())))?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AiError::other(format!("API key file {} is empty", full.display())));
    }
    Ok(key)
}

fn key_for(p: Provider, cfg: &AiCfg) -> Result<Option<String>, AiError> {
    if cfg.api_key_file.trim().is_empty() {
        return if p.key_required() { Err(AiError::other("no API key file set")) } else { Ok(None) };
    }
    // Ollama braucht keinen Schlüssel; eine Datei dort wird ignoriert
    if p == Provider::Ollama {
        return Ok(None);
    }
    read_key(&cfg.api_key_file).map(Some)
}

// ---------------------------------------------------------------------------------------------------------------
// HTTP

/// Fehlertext aus dem Antwortkörper eines HTTP-Fehlers (Anthropic/OpenAI: `error.message`, Ollama: `error`).
fn error_message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let m = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| v.get("error").and_then(Value::as_str))
        .or_else(|| v.get("message").and_then(Value::as_str))
        .unwrap_or("");
    m.chars().take(300).collect()
}

/// Prüft die Adresse vor jedem Aufruf: keine Zugangsdaten in der URL, den Schlüssel nie unverschlüsselt an einen
/// fremden Rechner. Meldungen nennen nur den Host, nie die volle URL (so auch die des HTTP-Clients).
fn check_request(url: &str, headers: &[(&str, String)]) -> Result<(), AiError> {
    let ep = parse_endpoint(url).map_err(|m| AiError::other(format!("invalid server address {m}")))?;
    if ep.has_userinfo {
        return Err(AiError::other("server address must not contain a user name or password"));
    }
    let has_key = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-api-key") || k.eq_ignore_ascii_case("authorization"));
    if has_key && !ep.https && !ep.local {
        return Err(AiError::other(format!("refusing to send the API key over plain http:// to {}, use https://", ep.host)));
    }
    Ok(())
}

impl From<http::Error> for AiError {
    fn from(e: http::Error) -> AiError {
        match e {
            http::Error::Cancelled => AiError::Cancelled,
            http::Error::Timeout(ms) => AiError::Timeout(ms),
            http::Error::Other(m) => AiError::Other(m),
        }
    }
}

/// Ein HTTP-Aufruf am Stück über [`crate::http`] auf dem aufrufenden Thread, ohne Hilfsthread: jedes Warten am Socket
/// dauert höchstens 50 ms und prüft dazwischen `cancel` und die Gesamtgrenze `timeout_ms` (ab Aufruf, für Verbindung,
/// Senden und Lesen zusammen, wie vorher bei ureq). Ein gesetztes `cancel` beendet den Aufruf also nach spätestens
/// ~50 ms, und `Drop` schließt die Verbindung sofort: nichts läuft nach. Keine Weiterleitungen.
fn call(
    method: http::Method,
    url: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
    timeout_ms: u64,
    cancel: Option<&CancelFlag>,
) -> Result<Value, AiError> {
    check_request(url, headers)?;
    // Leerlauf = Gesamtgrenze: am Stück zählt nur die Gesamtgrenze (die Antwort kommt erst, wenn sie fertig ist)
    let lim = http::Limits::total(timeout_ms, Duration::from_millis(timeout_ms), cancel.map(|c| &**c));
    let body = body.map(|b| b.to_string().into_bytes());
    let headers = with_accept(headers, "application/json");
    let mut resp = http::request(method, url, &headers, body.as_deref(), &lim)?;
    if !(200..300).contains(&resp.status) {
        return Err(failed_status(&mut resp, &lim));
    }
    let buf = resp.read_to_end(&lim, MAX_BODY)?;
    serde_json::from_slice(&buf).map_err(|_| AiError::other("response is not JSON"))
}

/// `headers` plus `Accept`
fn with_accept<'a>(headers: &[(&'a str, String)], accept: &str) -> Vec<(&'a str, String)> {
    let mut h = headers.to_vec();
    h.push(("accept", accept.to_string()));
    h
}

/// Fehler zu einer Antwort mit Fehlerstatus. Bei 401/403 wird der Körper gar nicht gelesen (sein Text käme ohnehin
/// nicht in die Meldung, und ein hängender Server soll den Aufruf nicht aufhalten); sonst höchstens 16 KiB, wobei
/// Abbruch und Gesamtgrenze als solche zurückkommen.
fn failed_status(resp: &mut http::Response, lim: &http::Limits) -> AiError {
    if matches!(resp.status, 401 | 403) {
        return status_error(resp.status, "");
    }
    match resp.error_body(lim) {
        Ok(text) => status_error(resp.status, &text),
        Err(e) => e.into(),
    }
}

/// Fehler zu einem HTTP-Status mit Körper `body`. Bei 401/403 nur der Status: der Servertext kann Teile des
/// Schlüssels oder interne Hinweise enthalten.
fn status_error(code: u16, body: &str) -> AiError {
    let message = if matches!(code, 401 | 403) { String::new() } else { error_message(body) };
    AiError::Http { code, message }
}

// ---------------------------------------------------------------------------------------------------------------
// Streaming (nur `ask` mit Rückruf für Teilantworten)

/// Vorgabe für die Leerlauf-Grenze eines gestreamten Aufrufs ([`AskOpts::idle_ms`]): so lange darf nach dem
/// Antwortkopf kein einziges Byte kommen. Die Claude API hält ruhige Strecken (Denken) mit `ping`-Events wach, Ollama
/// und OpenAI-kompatible Server senden nach dem Kopf fortlaufend Tokens; eine längere Pause ist eine hängende
/// Verbindung. Vor dem Kopf (Verbindung, Modell laden, erstes Token) gilt nur die Gesamtgrenze, die zusätzlich immer
/// gilt.
pub const STREAM_IDLE_MS: u64 = 30_000;

/// Ergebnis eines Chat-Aufrufs, gestreamt oder nicht: Rohtext (`None` = Format unbekannt) und Endgrund
struct Raw {
    text: Option<String>,
    stop: Option<String>,
    /// Claude: der erste `tool_use`-Block (Name, Eingabe), nur ohne Streaming
    tool: Option<(String, Value)>,
    /// Verbrauchte Tokens, nur ohne Streaming
    usage: Usage,
}

impl Raw {
    /// Antwort ohne Streaming (ein JSON-Objekt)
    fn from_json(p: Provider, v: &Value) -> Raw {
        Raw { text: p.parse_chat(v), stop: p.stop_reason(v).map(str::to_string), tool: p.parse_tool(v), usage: p.parse_usage(v) }
    }
}

/// Gestreamter POST: `body` mit `"stream": true`, über [`crate::http`] auf dem aufrufenden Thread. Jedes Warten am
/// Socket dauert höchstens 50 ms und prüft dazwischen Abbruch, Gesamtgrenze (`timeout_ms`, ab Aufruf) und Leerlauf
/// (`idle_ms`, ab Antwortkopf); beim Verlassen, auch bei Abbruch, wird die Verbindung sofort geschlossen, der Server
/// hört auf zu erzeugen. Die Antwort wird zerlegt ([`StreamParser`]), Text geht über `on_text` hinaus. HTTP-Fehler
/// kommen vor dem Stream wie bei [`call`] (die Rückfälle für Denken und Längengrenze greifen also gleich). Antwortet
/// der Server ohne Streaming (`application/json`), wird das Objekt wie bei [`call`] gelesen, ohne Teilantworten.
#[allow(clippy::too_many_arguments)]
fn call_stream(
    p: Provider,
    url: &str,
    headers: &[(&str, String)],
    body: &Value,
    timeout_ms: u64,
    idle_ms: u64,
    cancel: Option<&CancelFlag>,
    on_text: &dyn Fn(&str),
) -> Result<Raw, AiError> {
    check_request(url, headers)?;
    let lim = http::Limits::total(timeout_ms, Duration::from_millis(idle_ms), cancel.map(|c| &**c));
    let headers = with_accept(headers, "text/event-stream, application/x-ndjson, application/json");
    let mut resp = http::request(http::Method::Post, url, &headers, Some(body.to_string().as_bytes()), &lim)?;
    if !(200..300).contains(&resp.status) {
        return Err(failed_status(&mut resp, &lim));
    }
    let whole = resp.media() == "application/json";
    let mut parser = StreamParser::new(p);
    let (mut buf, mut received) = (Vec::new(), 0u64);
    while let Some(b) = resp.next(&lim)? {
        received += b.len() as u64;
        if received > MAX_BODY {
            return Err(AiError::other("response too large"));
        }
        if whole {
            buf.extend_from_slice(&b);
            continue;
        }
        parser.feed(&b, on_text)?;
        if parser.done {
            return Ok(parser.take());
        }
    }
    if whole {
        return serde_json::from_slice(&buf).map(|v| Raw::from_json(p, &v)).map_err(|_| AiError::other("response is not JSON"));
    }
    parser.end(on_text)
}

/// Zerlegt eine gestreamte Antwort: Server-Sent Events (Claude, OpenAI-kompatibel) bzw. NDJSON (Ollama). Arbeitet
/// auf Bytes und dekodiert erst ganze Zeilen, ein UTF-8-Zeichen oder ein Event darf also beliebig über Stücke verteilt
/// sein. Zeilenenden `\n`, `\r\n` und `\r` (SSE-Spezifikation), auch über Stückgrenzen.
struct StreamParser {
    p: Provider,
    line: Vec<u8>,
    /// Das letzte Byte war `\r`: ein folgendes `\n` gehört noch zu diesem Zeilenende
    after_cr: bool,
    /// `data`-Zeilen des laufenden SSE-Events, je mit `\n`
    data: String,
    has_data: bool,
    text: String,
    stop: Option<String>,
    /// Ende der Antwort gesehen (Claude `message_stop`, OpenAI `[DONE]`, Ollama `done: true`)
    done: bool,
}

impl StreamParser {
    fn new(p: Provider) -> StreamParser {
        StreamParser { p, line: Vec::new(), after_cr: false, data: String::new(), has_data: false, text: String::new(), stop: None, done: false }
    }

    fn take(&mut self) -> Raw {
        Raw { text: Some(std::mem::take(&mut self.text)), stop: self.stop.take(), tool: None, usage: Usage::default() }
    }

    fn feed(&mut self, bytes: &[u8], on_text: &dyn Fn(&str)) -> Result<(), AiError> {
        for &b in bytes {
            if self.done {
                break;
            }
            if std::mem::take(&mut self.after_cr) && b == b'\n' {
                continue;
            }
            match b {
                b'\n' => self.line_end(on_text)?,
                b'\r' => {
                    self.after_cr = true;
                    self.line_end(on_text)?;
                }
                _ => self.line.push(b),
            }
        }
        Ok(())
    }

    /// Verbindung zu Ende. Eine letzte Zeile ohne Zeilenende zählt bei NDJSON; ein SSE-Event ohne abschließende
    /// Leerzeile wird laut Spezifikation verworfen. Ohne Endkennung reicht ein gemeldeter Endgrund, sonst ist die
    /// Antwort unvollständig.
    fn end(&mut self, on_text: &dyn Fn(&str)) -> Result<Raw, AiError> {
        if !self.done && !self.line.is_empty() {
            self.line_end(on_text)?;
        }
        if self.done || self.stop.is_some() {
            Ok(self.take())
        } else {
            Err(AiError::other("the response stream ended before the answer was complete"))
        }
    }

    fn line_end(&mut self, on_text: &dyn Fn(&str)) -> Result<(), AiError> {
        let raw = std::mem::take(&mut self.line);
        let line = String::from_utf8(raw).map_err(|_| AiError::other("response stream is not UTF-8"))?;
        if self.p == Provider::Ollama {
            return if line.trim().is_empty() { Ok(()) } else { self.ndjson(&line, on_text) };
        }
        if line.is_empty() {
            if std::mem::take(&mut self.has_data) {
                let mut data = std::mem::take(&mut self.data);
                data.pop();
                self.event(&data, on_text)?;
            }
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(()); // Kommentar (Keep-alive)
        }
        let (field, value) = line.split_once(':').unwrap_or((line.as_str(), ""));
        if field == "data" {
            self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
            self.data.push('\n');
            self.has_data = true;
        }
        // `event`, `id`, `retry`: der Typ steht bei Claude auch im JSON, die übrigen brauchen wir nicht
        Ok(())
    }

    fn emit(&mut self, t: &str, on_text: &dyn Fn(&str)) {
        if !t.is_empty() {
            self.text.push_str(t);
            on_text(t);
        }
    }

    fn json(data: &str) -> Result<Value, AiError> {
        serde_json::from_str(data).map_err(|_| AiError::other("invalid event in the response stream"))
    }

    /// Ein SSE-Event (`data`)
    fn event(&mut self, data: &str, on_text: &dyn Fn(&str)) -> Result<(), AiError> {
        if self.p == Provider::OpenAi {
            if data.trim() == "[DONE]" {
                self.done = true;
                return Ok(());
            }
            let v = Self::json(data)?;
            if v.get("error").is_some_and(|e| !e.is_null()) {
                return Err(stream_error(&v));
            }
            if let Some(t) = v.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                self.emit(t, on_text);
            }
            if let Some(r) = v.pointer("/choices/0/finish_reason").and_then(Value::as_str) {
                self.stop = Some(r.to_string());
            }
            return Ok(());
        }
        let v = Self::json(data)?;
        match v.get("type").and_then(Value::as_str) {
            // Text am Blockanfang ist leer, wir nehmen ihn trotzdem, falls ein Server ihn füllt
            Some("content_block_start") if v.pointer("/content_block/type").and_then(Value::as_str) == Some("text") => {
                if let Some(t) = v.pointer("/content_block/text").and_then(Value::as_str) {
                    self.emit(t, on_text);
                }
            }
            // nur `text_delta`; `thinking_delta`, `signature_delta`, `input_json_delta` … gehören nicht zur Antwort
            Some("content_block_delta") if v.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") => {
                if let Some(t) = v.pointer("/delta/text").and_then(Value::as_str) {
                    self.emit(t, on_text);
                }
            }
            Some("message_delta") => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop = Some(r.to_string());
                }
            }
            Some("message_stop") => self.done = true,
            Some("error") => return Err(stream_error(&v)),
            // `message_start`, `ping`, `content_block_stop` und künftige Typen
            _ => {}
        }
        Ok(())
    }

    /// Eine NDJSON-Zeile von Ollama
    fn ndjson(&mut self, line: &str, on_text: &dyn Fn(&str)) -> Result<(), AiError> {
        let v = Self::json(line)?;
        if v.get("error").is_some_and(|e| !e.is_null()) {
            return Err(stream_error(&v));
        }
        if let Some(t) = v.pointer("/message/content").and_then(Value::as_str) {
            self.emit(t, on_text);
        }
        if v.get("done").and_then(Value::as_bool) == Some(true) {
            self.stop = v.get("done_reason").and_then(Value::as_str).map(str::to_string);
            self.done = true;
        }
        Ok(())
    }
}

/// Fehler mitten im Stream (Claude `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`,
/// OpenAI `{"error":{"message":…}}`, Ollama `{"error":"…"}`)
fn stream_error(v: &Value) -> AiError {
    AiError::Stream(error_message(&v.to_string()))
}

/// Feste Meldung zu [`AiError::Stream`]
const STREAM_ERROR: &str = "server error during the answer";

// ---------------------------------------------------------------------------------------------------------------
// Öffentliche Schnittstelle

/// Schreibt `text` mit dem System-Prompt des Modus `mode` um ([`AiCfg::mode_prompt`], also mit dem festen
/// Schutzrahmen [`INPUT_GUARD`], beim Modus `prompt` dazu am Ende das Längenlimit dieser Anfrage,
/// [`crate::config::length_budget_note`]) und säubert die Antwort. Leere oder überlange Antworten sind ein Fehler; der Aufrufer
/// behält dann den Rohtext.
pub fn rewrite(cfg: &AiCfg, mode: &str, text: &str) -> Result<String, AiError> {
    rewrite_with(cfg, mode, text, None)
}

/// Wie [`rewrite`]; ein gesetztes `cancel` beendet den Aufruf nach spätestens ~50 ms mit [`AiError::Cancelled`].
/// Modell: `ai.mode_models[mode]`, sonst `ai.model`.
pub fn rewrite_with(cfg: &AiCfg, mode: &str, text: &str, cancel: Option<&CancelFlag>) -> Result<String, AiError> {
    rewrite_impl(cfg, cfg.model_for(mode), mode, text, cancel, crate::debug_text())
}

/// Wie [`rewrite_with`]; Texte kommen nur mit `log_text` (und `DICTATE_DEBUG`) ins Debug-Log. Für die Pipeline des
/// Diktats, die auch die Eingabe der Befehlspalette verarbeitet (dort `log_text = false`).
pub fn rewrite_logged(cfg: &AiCfg, mode: &str, text: &str, cancel: Option<&CancelFlag>, log_text: bool) -> Result<String, AiError> {
    rewrite_impl(cfg, cfg.model_for(mode), mode, text, cancel, log_text && crate::debug_text())
}

/// Wie [`rewrite_with`] mit ausdrücklichem Modell (Textaktionen `mode:<name>`, siehe `ActionsCfg::model_for`). Für
/// die Markierung gedacht: loggt nie Text, auch nicht mit `DICTATE_DEBUG`, nur Zeichenzahlen. Zeitgrenze ist
/// `cfg.timeout_ms` (Textaktionen setzen dort `actions.timeout_ms` ein).
pub fn rewrite_as(cfg: &AiCfg, model: &str, mode: &str, text: &str, cancel: Option<&CancelFlag>) -> Result<String, AiError> {
    rewrite_impl(cfg, model, mode, text, cancel, false)
}

/// Gemeinsamer Teil von [`rewrite_with`] und [`rewrite_as`]; `log_text` erlaubt Texte im Debug-Log (nur Diktat).
fn rewrite_impl(cfg: &AiCfg, model: &str, mode: &str, text: &str, cancel: Option<&CancelFlag>, log_text: bool) -> Result<String, AiError> {
    let system = cfg.mode_prompt(mode).ok_or_else(|| AiError::other(format!("unknown mode \"{mode}\"")))?;
    let shape = shape(mode);
    let len = text.chars().count().max(1);
    let system = match shape.budget(len) {
        Some(chars) => crate::config::with_length_budget(&system, chars),
        None => system,
    };
    let req = ChatReq {
        model,
        system: &system,
        text,
        max_tokens: shape.max_tokens(len),
        ollama_temperature: Some(0.2),
        timeout_ms: shape.timeout_ms(cfg.timeout_ms),
        idle_ms: STREAM_IDLE_MS,
        on_text: None,
        structured: None,
    };
    let reply = chat(cfg, &req, cancel)?;
    // Abgeschnittene Antwort ist ein Fehler: lieber Rohtext tippen als einen halben Satz
    if reply.truncated {
        return Err(AiError::other("response cut off (max_tokens)"));
    }
    let mut out = sanitize_reply(&clean_reply(&reply.text, text), text, mode);
    if out.is_empty() {
        return Err(AiError::other("empty response"));
    }
    if out.chars().count() > shape.max_chars(len) {
        return Err(AiError::other("response far longer than the input"));
    }
    // Ein Zeilenumbruch am Ende (Sprachbefehl „neue Zeile“) bleibt erhalten
    let trailing = text.len() - text.trim_end_matches('\n').len();
    out.push_str(&"\n".repeat(trailing));
    if log_text {
        tracing::debug!("ai {mode}: {text:?} -> {out:?}");
    } else {
        tracing::debug!("ai {mode}: {} -> {} chars", text.chars().count(), out.chars().count());
    }
    Ok(out)
}

/// Optionen für [`ask`]. Anlegen mit [`AskOpts::new`], Teilantworten mit [`AskOpts::on_delta`]; `#[non_exhaustive]`
/// hält Platz für weitere Felder, ohne dass sich Aufrufer ändern müssen.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct AskOpts<'a> {
    /// Zeitgrenze für den ganzen Aufruf (bei Textaktionen `actions.timeout_ms`, nicht `ai.timeout_ms`)
    pub timeout_ms: u64,
    /// Nur gestreamt: so lange darf nach dem Antwortkopf nichts kommen (Vorgabe [`STREAM_IDLE_MS`], höchstens
    /// `timeout_ms` wirksam). Trennt eine hängende Verbindung von einer langen, aber laufenden Antwort.
    pub idle_ms: u64,
    /// Längengrenze der Antwort in Tokens (Claude `max_tokens`, OpenAI `max_tokens`/`max_completion_tokens`, Ollama
    /// `num_predict`); wird sie erreicht, ist die Antwort [`Answer::truncated`]
    pub max_tokens: u64,
    /// Rückruf für gestreamte Teilantworten (siehe [`ask`]); `None` = ohne Streaming
    pub on_delta: Option<&'a dyn Fn(&str)>,
}

impl<'a> AskOpts<'a> {
    pub fn new(timeout_ms: u64, max_tokens: u64) -> AskOpts<'a> {
        AskOpts { timeout_ms, idle_ms: STREAM_IDLE_MS, max_tokens, on_delta: None }
    }

    /// Leerlauf-Grenze beim Streamen ändern (siehe [`AskOpts::idle_ms`])
    pub fn idle_ms(mut self, ms: u64) -> AskOpts<'a> {
        self.idle_ms = ms;
        self
    }

    /// Antwort streamen und jedes neue Stück an `f` geben (siehe [`ask`])
    pub fn on_delta(mut self, f: &'a dyn Fn(&str)) -> AskOpts<'a> {
        self.on_delta = Some(f);
        self
    }
}

impl fmt::Debug for AskOpts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AskOpts")
            .field("timeout_ms", &self.timeout_ms)
            .field("idle_ms", &self.idle_ms)
            .field("max_tokens", &self.max_tokens)
            .field("on_delta", &self.on_delta.is_some())
            .finish()
    }
}

/// Antwort von [`ask`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Answer {
    /// Gesäubert ([`clean_answer`]): mehrzeilig, ohne Steuerzeichen außer `\n` und `\t`
    pub text: String,
    /// Die Längengrenze hat die Antwort beendet; `text` ist der Anfang
    pub truncated: bool,
}

/// Freie Anfrage für Textaktionen (Erklären, Fehleranalyse …): System-Prompt `system` (ohne [`INPUT_GUARD`] am
/// Anfang wird er vorangestellt, mit genau einmal belassen; Ausnahme: ein Prompt, der mit dem festen Rahmen für Fragen
/// [`crate::config::ASK_FRAME`] beginnt, bleibt ohne), `text` als ganze Benutzernachricht. Modell: `model`, sonst
/// `ai.model` bzw. das Vorgabemodell des Anbieters ([`AiCfg::effective_model`]). Anders als [`rewrite`]: die Antwort darf beliebig lang und mehrzeilig sein und wird nicht als Umschrift
/// geprüft; eine an `max_tokens` abgeschnittene Antwort ist kein Fehler, sondern `truncated`. Gesetztes `cancel`
/// beendet den Aufruf nach spätestens ~50 ms mit [`AiError::Cancelled`]. Weder Text noch Antwort werden geloggt,
/// auch nicht mit `DICTATE_DEBUG`, nur Zeichenzahlen und Dauer.
///
/// Mit [`AskOpts::on_delta`] wird gestreamt (Claude und OpenAI-kompatibel SSE, Ollama NDJSON): `on_delta` bekommt auf
/// dem aufrufenden Thread die neuen Stücke, schon gesäubert ([`AnswerStream`]: ohne Steuerzeichen, ohne Leerzeilen am
/// Anfang, Leerraum am Ende erst, wenn Text folgt, jedes Stück endet an einer Graphem-Grenze). Bei `Ok` ist die
/// Verkettung aller Stücke genau [`Answer::text`]; bei `Err` (auch Fehler mitten im Stream, Abbruch, Ablehnung am Ende)
/// sind die Stücke zu verwerfen. Antwortet ein Server trotz Anfrage nicht gestreamt, kommt kein Stück.
pub fn ask(cfg: &AiCfg, model: Option<&str>, system: &str, text: &str, opts: &AskOpts, cancel: Option<&CancelFlag>) -> Result<Answer, AiError> {
    if opts.max_tokens == 0 {
        return Err(AiError::other("max_tokens must be at least 1"));
    }
    if opts.timeout_ms == 0 || opts.idle_ms == 0 {
        return Err(AiError::other("timeout must be at least 1 ms"));
    }
    if text.trim().is_empty() {
        return Err(AiError::other("nothing to send"));
    }
    let model = model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or_else(|| cfg.effective_model());
    if model.is_empty() {
        return Err(AiError::other(NO_MODEL));
    }
    // Schutzrahmen als Sicherheitsnetz; einzige Ausnahme ist der feste Rahmen für Fragen der Palette (`ASK_FRAME`,
    // Begründung dort), der nie mit einem anderen Text beginnt
    let system = if system.starts_with(INPUT_GUARD) || system.starts_with(crate::config::ASK_FRAME) { system.to_string() } else { with_guard(system) };
    let cleaner = RefCell::new(AnswerStream::default());
    let deltas = std::cell::Cell::new(0usize);
    let forward = |piece: &str| {
        if let Some(f) = opts.on_delta {
            let out = cleaner.borrow_mut().push(piece);
            if !out.is_empty() {
                deltas.set(deltas.get() + 1);
                f(&out);
            }
        }
    };
    let on_text: Option<&dyn Fn(&str)> = opts.on_delta.map(|_| &forward as &dyn Fn(&str));
    let req = ChatReq {
        model,
        system: &system,
        text,
        max_tokens: Some(opts.max_tokens),
        ollama_temperature: None,
        timeout_ms: opts.timeout_ms,
        idle_ms: opts.idle_ms,
        on_text,
        structured: None,
    };
    let started = Instant::now();
    let reply = chat(cfg, &req, cancel)?;
    let out = clean_answer(&reply.text);
    if out.is_empty() {
        return Err(AiError::other(if reply.truncated { "response cut off before any text (max_tokens)" } else { "empty response" }));
    }
    if let Some(f) = opts.on_delta {
        let rest = cleaner.borrow_mut().finish();
        if !rest.is_empty() {
            deltas.set(deltas.get() + 1);
            f(&rest);
        }
    }
    tracing::debug!(
        "ai ask: {} -> {} chars{} in {} ms{}",
        text.chars().count(),
        out.chars().count(),
        if reply.truncated { " (truncated)" } else { "" },
        started.elapsed().as_millis(),
        if opts.on_delta.is_some() { format!(", {} deltas", deltas.get()) } else { String::new() }
    );
    Ok(Answer { text: out, truncated: reply.truncated })
}

// ---------------------------------------------------------------------------------------------------------------
// Strukturierte Antworten (Befehlspalette: freie Sprache → genau eine Absicht)

/// Ein Werkzeug für Claude: Name, Beschreibung und JSON-Schema der Eingabe (`input_schema`).
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Wie [`ask_structured`] die Antwort festlegt, je Anbieter die passende Form. Der Aufrufer liefert beide Formen aus
/// einer Quelle; die Antwort ist in jedem Fall nur Eingabe für seine eigene Prüfung.
#[derive(Debug, Clone, Copy)]
pub struct Structured<'a> {
    /// Claude (Tool-Use): genau ein Aufruf eines dieser Werkzeuge, erzwungen mit `tool_choice` `any`, wo das Modell
    /// es erlaubt ([`ToolCaps`]), sonst `auto` mit der Anweisung im Prompt; nie parallel. `strict`, wo möglich.
    pub tools: &'a [Tool],
    /// OpenAI-kompatibel: Name des Schemas in `response_format` (`^[a-zA-Z0-9_-]+$`)
    pub schema_name: &'a str,
    /// OpenAI-kompatibel (`response_format` `json_schema`, `strict: true`) und Ollama (`format`): Schema der ganzen
    /// Antwort. Für strict muss es dessen Regeln folgen (jedes Objekt `additionalProperties: false`, alle Felder in
    /// `required`, optionale als `null`-Union, kein `oneOf`, keine Längen- und Zahlengrenzen).
    pub schema: &'a Value,
}

/// Verbrauchte Tokens eines Aufrufs, soweit der Server sie meldet (nur Zahlen, fürs Log).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: Option<u64>,
    pub output: Option<u64>,
}

/// Antwort von [`ask_structured`]: der Werkzeugaufruf (Claude) oder das JSON-Objekt (OpenAI-kompatibel, Ollama).
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Tool { name: String, input: Value },
    Json(Value),
}

/// Strukturierte Anfrage: wie [`ask`] (Schutzrahmen vorn, Modell, Frist, Abbruch, Denk- und Token-Parameter je Modell,
/// kein `temperature`, nie geloggt), aber die Antwort ist genau eine Wahl nach `spec` statt Text. Ohne Streaming.
/// Fehler: Ablehnung, Inhaltsfilter, Antwort an der Längengrenze abgeschnitten, kein Werkzeugaufruf (Claude) bzw. kein
/// JSON-Objekt (sonst). `on_delta` in `opts` wird nicht benutzt.
pub fn ask_structured(
    cfg: &AiCfg,
    model: Option<&str>,
    system: &str,
    text: &str,
    spec: &Structured,
    opts: &AskOpts,
    cancel: Option<&CancelFlag>,
) -> Result<Choice, AiError> {
    ask_structured_usage(cfg, model, system, text, spec, opts, cancel).map(|(choice, _)| choice)
}

/// Wie [`ask_structured`], dazu die verbrauchten Tokens laut Server (Diagnose: fürs Log).
pub fn ask_structured_usage(
    cfg: &AiCfg,
    model: Option<&str>,
    system: &str,
    text: &str,
    spec: &Structured,
    opts: &AskOpts,
    cancel: Option<&CancelFlag>,
) -> Result<(Choice, Usage), AiError> {
    if opts.max_tokens == 0 {
        return Err(AiError::other("max_tokens must be at least 1"));
    }
    if opts.timeout_ms == 0 {
        return Err(AiError::other("timeout must be at least 1 ms"));
    }
    if text.trim().is_empty() {
        return Err(AiError::other("nothing to send"));
    }
    if spec.tools.is_empty() {
        return Err(AiError::other("no tools to choose from"));
    }
    let model = model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or_else(|| cfg.effective_model());
    if model.is_empty() {
        return Err(AiError::other(NO_MODEL));
    }
    let system = if system.starts_with(INPUT_GUARD) { system.to_string() } else { with_guard(system) };
    let req = ChatReq {
        model,
        system: &system,
        text,
        max_tokens: Some(opts.max_tokens),
        ollama_temperature: None,
        timeout_ms: opts.timeout_ms,
        idle_ms: opts.timeout_ms,
        on_text: None,
        structured: Some(spec),
    };
    let started = Instant::now();
    let (p, raw) = chat_raw(cfg, &req, cancel)?;
    if raw_stop(&raw)? {
        return Err(AiError::other("response cut off (max_tokens)"));
    }
    let choice = match p {
        Provider::Anthropic => match raw.tool {
            Some((name, input)) => Choice::Tool { name, input },
            None => return Err(AiError::other("the model answered without choosing a tool")),
        },
        Provider::OpenAi | Provider::Ollama => {
            let text = raw.text.ok_or_else(|| AiError::other("unexpected response format"))?;
            match serde_json::from_str::<Value>(text.trim()) {
                Ok(v) if v.is_object() => Choice::Json(v),
                _ => return Err(AiError::other("the model did not answer with a JSON object")),
            }
        }
    };
    tracing::debug!("ai structured: {} chars in {} ms", text.chars().count(), started.elapsed().as_millis());
    Ok((choice, raw.usage))
}

/// Eine Chat-Anfrage, wie sie [`chat`] an alle Anbieter schickt.
struct ChatReq<'a> {
    model: &'a str,
    system: &'a str,
    text: &'a str,
    /// Längengrenze der Antwort; `None` = Vorgabe des Anbieters (Claude braucht immer eine: [`anthropic_max_tokens`])
    max_tokens: Option<u64>,
    /// Nur Ollama; Cloud-Modelle bekommen nie `temperature`
    ollama_temperature: Option<f64>,
    timeout_ms: u64,
    /// Leerlauf-Grenze beim Streamen
    idle_ms: u64,
    /// Gesetzt = streamen; bekommt die rohen Textstücke in Reihenfolge
    on_text: Option<&'a dyn Fn(&str)>,
    /// Gesetzt = strukturierte Antwort ([`ask_structured`]): Claude Tool-Use, OpenAI `response_format`, Ollama `format`
    structured: Option<&'a Structured<'a>>,
}

struct ChatReply {
    /// Rohtext der Antwort
    text: String,
    /// An der Längengrenze beendet
    truncated: bool,
}

/// Der gemeinsame Chat-Pfad für [`rewrite_as`] und [`ask`]: Ziel und Schlüssel auflösen, Anfrage je Anbieter (mit
/// Denk-Einstellung bzw. Name der Längengrenze und Rückfall bei Absage), Antwort lesen, gestreamt oder am Stück. Eine
/// Ablehnung durch das Modell oder einen Inhaltsfilter ist ein Fehler.
/// Fehler ohne Modell: ohne Vorgabe des Anbieters (OpenAI-kompatible Server) muss der User eins eintragen.
const NO_MODEL: &str = "no AI model set (enter one on the AI page)";

fn chat(cfg: &AiCfg, req: &ChatReq, cancel: Option<&CancelFlag>) -> Result<ChatReply, AiError> {
    let (_, raw) = chat_raw(cfg, req, cancel)?;
    let truncated = raw_stop(&raw)?;
    let text = raw.text.ok_or_else(|| AiError::other("unexpected response format"))?;
    Ok(ChatReply { text, truncated })
}

/// Ablehnung durch Modell oder Inhaltsfilter als Fehler; sonst `true`, wenn die Längengrenze die Antwort beendet hat.
fn raw_stop(raw: &Raw) -> Result<bool, AiError> {
    let stop = raw.stop.as_deref();
    match stop {
        Some("refusal") => return Err(AiError::other("the model declined the request")),
        Some("content_filter") => return Err(AiError::other("the response was blocked by a content filter")),
        _ => {}
    }
    Ok(matches!(stop, Some("max_tokens" | "length" | "model_context_window_exceeded")))
}

/// Ziel und Schlüssel auflösen, Anfrage je Anbieter senden (mit Rückfällen), Rohantwort und Anbieter zurück.
fn chat_raw(cfg: &AiCfg, req: &ChatReq, cancel: Option<&CancelFlag>) -> Result<(Provider, Raw), AiError> {
    // Dieselben Regeln wie beim Speichern (`Config::validate_new`); eine ältere Datei lädt trotzdem, dann steht der
    // Grund hier statt einer unklaren Antwort des Servers
    if req.model.trim().is_empty() {
        return Err(AiError::other(NO_MODEL));
    }
    cfg.check_model(req.model).map_err(|m| AiError::other(format!("model {m}")))?;
    let Target { provider: p, base } = resolve(cfg)?;
    let key = key_for(p, cfg)?;
    let url = p.url(&base, p.chat_path());
    let headers = p.headers(key.as_deref());
    let send = |body: Value| -> Result<Raw, AiError> {
        match req.on_text {
            None => call(http::Method::Post, &url, &headers, Some(&body), req.timeout_ms, cancel).map(|v| Raw::from_json(p, &v)),
            Some(on_text) => {
                let mut body = body;
                body["stream"] = json!(true);
                call_stream(p, &url, &headers, &body, req.timeout_ms, req.idle_ms, cancel, on_text)
            }
        }
    };
    let raw = match p {
        Provider::Anthropic => call_anthropic(req, &send)?,
        Provider::OpenAi => call_openai(req, &send)?,
        Provider::Ollama => send(ollama_body(req))?,
    };
    Ok((p, raw))
}

/// Aufruf an Claude mit der Denk-Einstellung des Modells. Lehnt die API sie ab (HTTP 400 zu `thinking` oder
/// `effort`, etwa bei einem Modell, das jünger ist als diese Tabelle), folgt höchstens zwei Mal die nächste
/// Einstellung; die erfolgreiche wird für das Modell gemerkt. Gestreamt gilt dasselbe: die Absage kommt als Status vor
/// dem Stream. Strukturiert (Werkzeuge) ebenso für die Werkzeugwahl: Absage zu `tool_choice` → `auto`, zu `strict` →
/// ohne strict ([`ToolCaps`]).
fn call_anthropic(req: &ChatReq, send: &dyn Fn(Value) -> Result<Raw, AiError>) -> Result<Raw, AiError> {
    let model = req.model;
    let (first, first_caps) = (thinking_for(model), tool_caps_for(model));
    let (mut t, mut caps) = (first, first_caps);
    // Denken höchstens zwei Rückfälle, Werkzeuge (nur strukturiert) höchstens zwei
    const ATTEMPTS: usize = 5;
    for attempt in 0..ATTEMPTS {
        match send(anthropic_body(req, t, caps)) {
            Ok(v) => {
                if t != first {
                    tracing::info!("ai: model {model} needs thinking setting {t:?}");
                    learn(model, t);
                }
                if caps != first_caps {
                    tracing::info!("ai: model {model} needs tool setting {caps:?}");
                    if let Ok(mut m) = learned_tool_caps().lock() {
                        m.insert(model.trim().to_string(), caps);
                    }
                }
                return Ok(v);
            }
            Err(e @ AiError::Http { code: 400, .. }) if attempt + 1 < ATTEMPTS => {
                // nur hier wird der Servertext gelesen; er geht nirgends weiter
                let m = e.to_string();
                if let Some(next) = req.structured.and_then(|_| tool_caps_after_error(caps, t, &m)) {
                    caps = next;
                } else {
                    match thinking_after_error(t, &m) {
                        Some(next) => t = next,
                        None => return Err(e),
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the last attempt returns")
}

/// Aufruf an einen OpenAI-kompatiblen Server. Mit Längengrenze: Nennt die Absage (HTTP 400/422) den anderen
/// Feldnamen (`max_completion_tokens`), folgt ein zweiter Versuch damit; der erfolgreiche wird für das Modell gemerkt.
fn call_openai(req: &ChatReq, send: &dyn Fn(Value) -> Result<Raw, AiError>) -> Result<Raw, AiError> {
    let first = token_param_for(req.model);
    let mut param = first;
    for attempt in 0..2 {
        match send(openai_body(req, param)) {
            Ok(v) => {
                if param != first {
                    tracing::info!("ai: model {} needs {}", req.model, param.field());
                    if let Ok(mut m) = learned_token_param().lock() {
                        m.insert(req.model.trim().to_string(), param);
                    }
                }
                return Ok(v);
            }
            Err(AiError::Http { code: 400 | 422, message }) if attempt == 0 && req.max_tokens.is_some() && message.contains("max_completion_tokens") => {
                param = param.other();
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the last attempt returns")
}

/// Modelle des Anbieters. Anthropic: bei Fehlschlag eine kurze statische Liste (zweiter Wert nennt dann den Fehler).
pub fn list_models(cfg: &AiCfg) -> Result<(Vec<String>, Option<String>), AiError> {
    let Target { provider: p, base } = resolve(cfg)?;
    let key = key_for(p, cfg)?;
    let ms = cfg.timeout_ms.max(3000);
    let url = p.url(&base, p.models_path());
    let res = call(http::Method::Get, &url, &p.headers(key.as_deref()), None, ms, None)
        .and_then(|v| p.parse_models(&v).ok_or_else(|| AiError::other("unexpected response format")));
    match res {
        Ok(list) => Ok((list, None)),
        Err(e) if p == Provider::Anthropic => {
            Ok((["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"].iter().map(|s| s.to_string()).collect(), Some(e.to_string())))
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Antwort säubern

const PREAMBLES: &[&str] =
    &["here is", "here's", "here are", "sure", "certainly", "of course", "okay", "ok,", "hier ist", "hier sind", "natürlich", "gerne", "klar"];

/// Entfernt Steuerzeichen außer `\n` und `\t` (ein Return oder eine Escape-Sequenz im Terminal wäre ein Befehl) und
/// Bidi-Steuerzeichen (U+202A–U+202E, U+2066–U+2069), die beim Einfügen eine andere Zeichenfolge zeigen, als
/// ankommt. `\r\n` wird zu `\n`.
pub fn strip_controls(s: &str) -> String {
    s.chars().filter(|&c| keep_char(c)).collect()
}

/// Zeichen, die [`strip_controls`] behält. Ein Filter je Zeichen ohne Zusammenhang (`\r` fällt immer weg, aus `\r\n`
/// wird so `\n`), darum gleich, ob der Text am Stück oder in Stücken kommt.
fn keep_char(c: char) -> bool {
    (c == '\n' || c == '\t' || !c.is_control()) && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Säubert die Antwort von [`ask`]: [`strip_controls`], Leerzeilen am Anfang und Leerraum am Ende weg; Zeilenumbrüche,
/// Tabs und die Einrückung der ersten Zeile bleiben.
pub fn clean_answer(reply: &str) -> String {
    let s = strip_controls(reply);
    let mut start = 0;
    for line in s.split_inclusive('\n') {
        if !line.trim().is_empty() {
            break;
        }
        start += line.len();
    }
    s[start..].trim_end().to_string()
}

/// [`clean_answer`] für eine Antwort, die in Stücken kommt: [`AnswerStream::push`] gibt zurück, was schon sicher
/// feststeht, [`AnswerStream::finish`] den Rest. Die Verkettung aller Rückgaben ist genau `clean_answer` des ganzen
/// Textes: Steuerzeichen fallen je Zeichen weg, Leerzeilen am Anfang werden zurückgehalten, bis Text kommt, Leerraum
/// am Ende, bis wieder Text folgt (am Schluss fällt er weg). Außerdem bleibt die letzte Graphem-Gruppe zurück, weil
/// das nächste Stück sie noch verlängern kann (kombinierende Zeichen, Emoji-Folgen mit ZWJ): jedes Stück endet an
/// einer Grenze, die Anzeige zeigt nie ein halbes Zeichen.
#[derive(Debug, Default)]
pub struct AnswerStream {
    /// Erste Zeile mit Text gesehen
    started: bool,
    held: String,
}

impl AnswerStream {
    pub fn push(&mut self, piece: &str) -> String {
        self.held.extend(piece.chars().filter(|&c| keep_char(c)));
        if !self.started {
            let Some(i) = self.held.find(|c: char| !c.is_whitespace()) else {
                return String::new();
            };
            let start = self.held[..i].rfind('\n').map_or(0, |j| j + 1);
            self.held.drain(..start);
            self.started = true;
        }
        let body = self.held.trim_end().len();
        let cut = self.held[..body].grapheme_indices(true).next_back().map_or(0, |(i, _)| i);
        self.held.drain(..cut).collect()
    }

    pub fn finish(&mut self) -> String {
        if !self.started {
            self.held.clear();
            return String::new();
        }
        let body = self.held.trim_end().len();
        self.held.truncate(body);
        std::mem::take(&mut self.held)
    }
}

/// Macht die Antwort tippbar: [`strip_controls`]. Hat die Antwort mehr Zeilenumbrüche als die Eingabe und darf der
/// Modus keine neuen Zeilen erzeugen (alle außer `email` und `prompt`, siehe [`Shape`]), werden die Umbrüche durch
/// Leerzeichen ersetzt.
pub fn sanitize_reply(reply: &str, input: &str, mode: &str) -> String {
    let s = strip_controls(reply);
    let s = s.trim().to_string();
    let newlines = |t: &str| t.trim().matches('\n').count();
    if newlines(&s) > newlines(input) && !shape(mode).lines {
        let mut out = String::with_capacity(s.len());
        for (i, line) in s.split('\n').map(str::trim).filter(|l| !l.is_empty()).enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(line);
        }
        return out;
    }
    s
}

/// Trim, Präambel („Here is the corrected text:“) und umschließende Anführungszeichen entfernen – beides nur
/// konservativ: Die Präambel nur als eigene erste Zeile mit Doppelpunkt, Anführungszeichen nur als einziges
/// Paar, und nie, wenn schon die Eingabe so beginnt.
pub fn clean_reply(reply: &str, input: &str) -> String {
    let mut s = reply.trim().to_string();
    let input = input.trim();
    if let Some((first, rest)) = s.split_once('\n') {
        let f = first.trim();
        let lower = f.to_lowercase();
        let starts = PREAMBLES.iter().any(|p| lower.starts_with(p));
        let input_lower = input.to_lowercase();
        if f.ends_with(':') && f.chars().count() <= 80 && starts && !PREAMBLES.iter().any(|p| input_lower.starts_with(p)) && !rest.trim().is_empty() {
            s = rest.trim().to_string();
        }
    }
    const PAIRS: &[(char, char)] = &[('"', '"'), ('\'', '\''), ('“', '”'), ('„', '“'), ('„', '”'), ('«', '»'), ('‘', '’')];
    let mut chars = s.chars();
    if let (Some(a), Some(b)) = (chars.next(), chars.next_back()) {
        if PAIRS.contains(&(a, b)) && s.chars().count() >= 2 {
            let inner: String = s.chars().skip(1).take(s.chars().count() - 2).collect();
            let input_quoted = input.starts_with(a) || input.ends_with(b);
            if !input_quoted && !inner.contains(a) && !inner.contains(b) {
                s = inner.trim().to_string();
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_quotes_and_preamble_conservatively() {
        assert_eq!(clean_reply("  \"Hello, world.\"\n", "hello world"), "Hello, world.");
        assert_eq!(clean_reply("„Hallo Welt.“", "hallo welt"), "Hallo Welt.");
        assert_eq!(clean_reply("Here is the corrected text:\nHello, world.", "hello world"), "Hello, world.");
        assert_eq!(clean_reply("Sure! Here's the text:\n\"Hello.\"", "hello"), "Hello.");
        // Anführungszeichen im Text bleiben
        assert_eq!(clean_reply("\"A\" and \"B\"", "a and b"), "\"A\" and \"B\"");
        // Eingabe war selbst zitiert
        assert_eq!(clean_reply("\"Hi\"", "\"hi\""), "\"Hi\"");
        // Doppelpunkt mitten im Satz, keine Präambel
        assert_eq!(clean_reply("Note: this stays", "note this stays"), "Note: this stays");
        assert_eq!(clean_reply("Here is why:\nbecause", "here is why because"), "Here is why:\nbecause");
        assert_eq!(clean_reply("", "x"), "");
    }

    fn cfg(preset: &str, provider: &str, endpoint: &str) -> AiCfg {
        AiCfg { preset: preset.into(), provider: provider.into(), endpoint: endpoint.into(), ..AiCfg::default() }
    }

    fn url_of(c: &AiCfg, chat: bool) -> String {
        let t = resolve(c).unwrap();
        t.provider.url(&t.base, if chat { t.provider.chat_path() } else { t.provider.models_path() })
    }

    #[test]
    fn preset_resolution() {
        let c = cfg("google", "ollama", "");
        assert_eq!(url_of(&c, true), "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions");
        assert_eq!(url_of(&c, false), "https://generativelanguage.googleapis.com/v1beta/openai/models");
        assert_eq!(url_of(&cfg("anthropic", "ollama", ""), true), "https://api.anthropic.com/v1/messages");
        assert_eq!(url_of(&cfg("anthropic", "ollama", ""), false), "https://api.anthropic.com/v1/models");
        assert_eq!(url_of(&cfg("lmstudio", "ollama", ""), true), "http://127.0.0.1:1234/v1/chat/completions");
        assert_eq!(url_of(&cfg("ollama", "openai", ""), true), "http://127.0.0.1:11434/api/chat");
        // ausdrücklicher Endpoint überschreibt die Basis
        assert_eq!(url_of(&cfg("groq", "ollama", "http://h:9/x/"), true), "http://h:9/x/chat/completions");
        // alter Stand: provider ohne Preset
        assert_eq!(url_of(&cfg("", "ollama", "http://h:1/"), true), "http://h:1/api/chat");
        assert_eq!(url_of(&cfg("", "openai", "http://h:1"), true), "http://h:1/v1/chat/completions");
        assert_eq!(url_of(&cfg("", "openai", "http://h:1/v1"), false), "http://h:1/v1/models");
        assert_eq!(url_of(&cfg("custom", "openai", "http://h:1/api/v2"), true), "http://h:1/api/v2/chat/completions");
        assert_eq!(url_of(&cfg("", "anthropic", ""), true), "https://api.anthropic.com/v1/messages");
        assert_eq!(url_of(&cfg("", "openai", ""), true), "https://api.openai.com/v1/chat/completions");
        assert!(resolve(&cfg("", "nope", "")).is_err());
        assert!(cfg("openai", "ollama", "").is_cloud());
        assert!(!cfg("ollama", "ollama", "").is_cloud());
        assert!(!cfg("lmstudio", "ollama", "").is_cloud());
        assert!(!cfg("", "ollama", "http://127.0.0.1:11434").is_cloud());
        assert!(cfg("custom", "openai", "https://x.example/v1").is_cloud());
        assert!(!cfg("custom", "openai", "http://localhost:8080/v1").is_cloud());
    }

    #[test]
    fn sanitize_drops_controls_and_flattens_extra_newlines() {
        assert_eq!(sanitize_reply("a\u{1b}[0mb\u{7}\u{85}c\td", "x", "clean"), "a[0mbc\td");
        assert_eq!(sanitize_reply("one\r\ntwo", "x", "clean"), "one two");
        assert_eq!(sanitize_reply("one\r\ntwo", "x", "email"), "one\ntwo");
        assert_eq!(sanitize_reply("Goal:\nA\n\nOpen points:\n- b", "a", "prompt"), "Goal:\nA\n\nOpen points:\n- b");
        assert_eq!(sanitize_reply("one\ntwo", "a\nb", "clean"), "one\ntwo");
        assert_eq!(sanitize_reply("one\n\n  two \n three", "x", "clean"), "one two three");
        assert_eq!(sanitize_reply("fine.", "x", "clean"), "fine.");
    }

    #[test]
    fn only_the_prompt_mode_may_grow_a_lot_and_take_longer() {
        for mode in ["clean", "email", "to_de", "mine"] {
            let s = shape(mode);
            assert_eq!((s.max_chars(10), s.max_chars(1000)), (50, 3000), "{mode}");
            assert_eq!((s.max_tokens(1000), s.timeout_ms(4000)), (None, 4000), "{mode}");
        }
        assert!(shape("email").lines && !shape("clean").lines && !shape("to_en").lines);
        let p = shape("prompt");
        assert!(p.lines && p.refine && shape("Prompt").refine);
        // harte Grenze: anderthalbfaches Ziel (Ziel dreifach, ohne Untergrenze in Zeichen); max_tokens mindestens 1024
        assert_eq!((p.max_chars(60), p.max_chars(1000)), (270, 4500));
        assert_eq!(p.max_tokens(60), Some(1024));
        assert_eq!(p.max_tokens(1000), Some(2506));
        assert_eq!((p.timeout_ms(4000), p.timeout_ms(45_000)), (20_000, 45_000));
    }

    #[test]
    fn only_the_prompt_mode_has_a_length_budget_and_it_is_three_times_the_input() {
        for mode in ["clean", "email", "to_de", "to_en", "mine"] {
            assert_eq!(shape(mode).budget(34), None, "{mode}");
        }
        let p = shape("prompt");
        assert_eq!((p.budget(34), p.budget(1), p.budget(325)), (Some(102), Some(3), Some(975)));
        // die harte Grenze liegt über dem Limit, und max_tokens reicht für eine Antwort bis zur harten Grenze
        // (2 Zeichen je Token plus Reserve), damit nie mitten im Satz abgeschnitten wird
        for len in [1, 34, 100, 325, 1000, 5000] {
            let (budget, max) = (p.budget(len).unwrap(), p.max_chars(len));
            assert!(max > budget, "{len}");
            assert!(p.max_tokens(len).unwrap() >= (max / 2 + 256) as u64, "{len}");
        }
    }

    #[test]
    fn endpoint_problems_are_reported_before_any_request() {
        let mut c = cfg("openai", "ollama", "");
        assert!(resolve(&c).is_ok());
        c.endpoint = "http://127.0.0.1:11434".into();
        assert!(resolve(&c).unwrap_err().to_string().contains("Ollama address"));
        c = cfg("", "openai", "https://u:p@x.example/v1");
        assert!(resolve(&c).is_err());
    }

    #[test]
    fn model_ids_are_normalized() {
        assert_eq!(model_base("claude-sonnet-4-5-20250929"), "claude-sonnet-4-5");
        assert_eq!(model_base("anthropic.claude-sonnet-4-5-20250929-v1:0"), "claude-sonnet-4-5");
        assert_eq!(model_base("us.anthropic.claude-opus-5-5"), "claude-opus-5-5");
        assert_eq!(model_base("claude-opus-4-5@20251101"), "claude-opus-4-5");
        assert_eq!(model_base("claude-opus-4-6[1m]"), "claude-opus-4-6");
        assert_eq!(model_base("claude-3-5-haiku-latest"), "claude-3-5-haiku");
        assert_eq!(model_base(" Claude-Haiku-5-5 "), "claude-haiku-5-5");
        assert_eq!(model_base("qwen2.5:7b"), "qwen2.5:7b");
    }

    #[test]
    fn thinking_table() {
        use Thinking::*;
        let t = thinking_known;
        assert_eq!(t("claude-sonnet-5-5"), BetweenTools);
        assert_eq!(t("claude-opus-5-5"), LowEffort);
        assert_eq!(t("claude-fable-5-1"), LowEffort);
        assert_eq!(t("claude-mythos-5-1"), LowEffort);
        assert_eq!(t("claude-opus-5"), Disabled);
        assert_eq!(t("claude-sonnet-5"), Disabled);
        assert_eq!(t("claude-haiku-5-5"), Disabled);
        for m in
            ["claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6", "claude-haiku-4-5", "claude-opus-4", "claude-3-7-sonnet-20250219"]
        {
            assert_eq!(t(m), Omit, "{m}");
        }
        // unbekannt (neuer): `disabled`, dann aus der Absage lernen
        assert_eq!(t("claude-haiku-6"), Disabled);
        assert_eq!(t("claude-opus-50"), Disabled);
    }

    #[test]
    fn next_setting_after_a_rejection() {
        use Thinking::*;
        let sonnet =
            "HTTP 400: \"thinking.type.disabled\" is not supported for this model. Use \"thinking.type.between_tools\" for the lowest thinking setting";
        let opus = "HTTP 400: \"thinking.type.disabled\" is not supported for this model. Use \"thinking.type.adaptive\" and \"output_config.effort\"";
        assert_eq!(thinking_after_error(Disabled, sonnet), Some(BetweenTools));
        assert_eq!(thinking_after_error(Disabled, opus), Some(LowEffort));
        assert_eq!(thinking_after_error(BetweenTools, "HTTP 400: \"thinking.type.between_tools\" is not supported for this model."), Some(LowEffort));
        assert_eq!(thinking_after_error(LowEffort, "HTTP 400: output_config.effort: unknown field"), Some(Omit));
        assert_eq!(thinking_after_error(Omit, "HTTP 400: thinking"), None);
        assert_eq!(thinking_after_error(Disabled, "HTTP 400: max_tokens too large"), None);
    }

    fn req<'a>(text: &'a str, max_tokens: Option<u64>) -> ChatReq<'a> {
        ChatReq {
            model: "m",
            system: "s",
            text,
            max_tokens,
            ollama_temperature: Some(0.2),
            timeout_ms: 1000,
            idle_ms: STREAM_IDLE_MS,
            on_text: None,
            structured: None,
        }
    }

    const CAPS: ToolCaps = ToolCaps { forced: true, strict: true };

    #[test]
    fn anthropic_body_never_sends_temperature() {
        for t in [Thinking::Omit, Thinking::Disabled, Thinking::BetweenTools, Thinking::LowEffort] {
            let b = anthropic_body(&req("x", None), t, CAPS);
            assert!(b.get("temperature").is_none(), "{t:?}");
            assert_eq!(b["messages"], json!([{"role": "user", "content": "x"}]));
        }
    }

    #[test]
    fn max_tokens_per_provider() {
        // Claude: Vorgabe aus der Textlänge, sonst die des Aufrufers; Reserve bei nicht abschaltbarem Denken
        assert_eq!(anthropic_body(&req("x", None), Thinking::Disabled, CAPS)["max_tokens"], 1024);
        assert_eq!(anthropic_body(&req("x", Some(300)), Thinking::Disabled, CAPS)["max_tokens"], 300);
        assert_eq!(anthropic_body(&req("x", Some(300)), Thinking::LowEffort, CAPS)["max_tokens"], 300 + THINKING_RESERVE);
        // OpenAI: ohne Grenze kein Feld, sonst unter dem gewählten Namen; nie temperature
        let o = openai_body(&req("x", None), TokenParam::MaxTokens);
        assert!(o.get("max_tokens").is_none() && o.get("max_completion_tokens").is_none() && o.get("temperature").is_none());
        assert_eq!(openai_body(&req("x", Some(9)), TokenParam::MaxTokens)["max_tokens"], 9);
        let o = openai_body(&req("x", Some(9)), TokenParam::MaxCompletionTokens);
        assert_eq!((o["max_completion_tokens"].as_u64(), o.get("max_tokens")), (Some(9), None));
        // Ollama: num_predict, temperature nur auf Wunsch
        let mut r = req("x", Some(7));
        assert_eq!(ollama_body(&r)["options"], json!({"temperature": 0.2, "num_predict": 7}));
        r.ollama_temperature = None;
        assert_eq!(ollama_body(&r)["options"], json!({"num_predict": 7}));
        r.max_tokens = None;
        assert!(ollama_body(&r).get("options").is_none());
    }

    #[test]
    fn token_param_by_model() {
        use TokenParam::*;
        for m in ["o1", "o3-mini", "o4-mini-2025-04-16", "gpt-5", "gpt-5-mini", "openai/o3-mini", "GPT-5"] {
            assert_eq!(token_param_known(m), MaxCompletionTokens, "{m}");
        }
        for m in ["gpt-4o", "gpt-4.1-mini", "llama-3.3-70b", "gemini-2.5-flash", "o1x", "mistral-small", "qwen2.5:7b"] {
            assert_eq!(token_param_known(m), MaxTokens, "{m}");
        }
    }

    #[test]
    fn answers_keep_lines_and_lose_controls() {
        assert_eq!(clean_answer("\n  \n  indented\r\n\tnext\u{1b}[0m\u{7}\n\n"), "  indented\n\tnext[0m");
        assert_eq!(clean_answer("a\u{202e}b\u{2066}c\u{0}d"), "abcd");
        assert_eq!(clean_answer(" \n\t\n"), "");
        assert_eq!(clean_answer("Ünïcödé 😀\n– ok"), "Ünïcödé 😀\n– ok");
    }

    /// Verkettung der Stücke von `AnswerStream` == `clean_answer` am Stück, bei jeder Zerlegung
    #[test]
    fn answer_stream_matches_clean_answer_for_every_split() {
        let samples = [
            "\n\nSummary line.\r\n\r\n- first\u{1b}[31m\n- second\u{202e}\n\tindented code\u{0}\n  ",
            "  \n \t\n  indented start\r\nnext  \n\n",
            "Familie: 👨\u{200d}👩\u{200d}👧 und e\u{301}e\u{301} 🇦🇹🇩🇪 Ünïcödé\r\n\t– ok \u{3000}\n",
            " \n\t\n",
            "",
            "x",
            "a\r\n\r\nb\r",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for s in samples {
            let chars: Vec<char> = s.chars().collect();
            for round in 0..200 {
                let mut st = AnswerStream::default();
                let (mut out, mut pieces) = (String::new(), Vec::new());
                let mut i = 0;
                while i < chars.len() {
                    let n = if round == 0 { 1 } else { 1 + rnd(4) };
                    let piece: String = chars[i..(i + n).min(chars.len())].iter().collect();
                    i += n;
                    let o = st.push(&piece);
                    if !o.is_empty() {
                        pieces.push(o.clone());
                    }
                    out.push_str(&o);
                }
                out.push_str(&st.finish());
                assert_eq!(out, clean_answer(s), "{s:?} round {round}: {pieces:?}");
                // jedes Stück endet an einer Graphem-Grenze des Ergebnisses
                let mut at = 0;
                let bounds: Vec<usize> = out.grapheme_indices(true).map(|(i, _)| i).chain([out.len()]).collect();
                for p in &pieces {
                    at += p.len();
                    assert!(bounds.contains(&at), "{s:?}: {pieces:?}");
                }
            }
        }
    }

    fn parse(p: Provider, chunks: &[&[u8]]) -> (Result<Raw, AiError>, Vec<String>) {
        let got = RefCell::new(Vec::new());
        let on = |t: &str| got.borrow_mut().push(t.to_string());
        let mut sp = StreamParser::new(p);
        let mut r = None;
        for c in chunks {
            if let Err(e) = sp.feed(c, &on) {
                r = Some(Err(e));
                break;
            }
            if sp.done {
                r = Some(Ok(sp.take()));
                break;
            }
        }
        let r = r.unwrap_or_else(|| sp.end(&on));
        (r, got.into_inner())
    }

    /// Byteweise zerlegt (mitten in UTF-8 und `\r\n`) liefert dasselbe wie am Stück
    #[test]
    fn stream_parser_handles_any_byte_split() {
        let sse =
            "event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{}}\r\n\r\n: keep-alive\n\nevent: ping\ndata: {\"type\": \"ping\"}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"secret\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r\r\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Grüße 👋\"}}\r\n\r\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\n\
data: \"delta\":{\"type\":\"text_delta\",\"text\":\" – ok\"}}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
data: {\"type\":\"message_stop\"}\n\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"after stop\"}}\n\n";
        let bytes = sse.as_bytes();
        let whole = parse(Provider::Anthropic, &[bytes]);
        let split: Vec<&[u8]> = bytes.chunks(1).collect();
        let byte = parse(Provider::Anthropic, &split);
        for (r, got) in [whole, byte] {
            let r = r.unwrap();
            assert_eq!((r.text.as_deref(), r.stop.as_deref()), (Some("Grüße 👋 – ok"), Some("end_turn")));
            assert_eq!(got, ["Grüße 👋", " – ok"]);
        }
    }

    #[test]
    fn stream_parser_errors_truncation_and_early_end() {
        // Fehler-Event mitten im Stream
        let s = b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hal\"}}\n\nevent: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let (r, got) = parse(Provider::Anthropic, &[s]);
        assert_eq!(r.err().unwrap().to_string(), "server error during the answer: Overloaded");
        assert_eq!(got, ["Hal"]);
        // abgeschnitten: Endgrund max_tokens, auch ohne message_stop vor dem Verbindungsende
        let s = b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"A\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n";
        assert_eq!(parse(Provider::Anthropic, &[s]).0.unwrap().stop.as_deref(), Some("max_tokens"));
        // ohne Endkennung und Endgrund: unvollständig; ein Event ohne Leerzeile danach zählt nicht
        let s = b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"A\"}}\n\ndata: {\"type\":\"message_stop\"}";
        assert!(parse(Provider::Anthropic, &[s]).0.err().unwrap().to_string().contains("ended before"));
        // OpenAI: delta.content, finish_reason, [DONE]; Fehlerobjekt
        let s = b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
        let (r, got) = parse(Provider::OpenAi, &[s]);
        let r = r.unwrap();
        assert_eq!((r.text.as_deref(), r.stop.as_deref(), got), (Some("Hi"), Some("length"), vec!["Hi".to_string()]));
        let s = b"data: {\"error\":{\"message\":\"rate limited\"}}\n\n";
        assert_eq!(parse(Provider::OpenAi, &[s]).0.err().unwrap().to_string(), "server error during the answer: rate limited");
        // Ollama: NDJSON, letzte Zeile ohne Zeilenende, Fehlerzeile
        let s = b"{\"message\":{\"content\":\"a\"},\"done\":false}\n{\"message\":{\"content\":\"b\"},\"done\":true,\"done_reason\":\"stop\"}";
        let (r, got) = parse(Provider::Ollama, &[s]);
        assert_eq!((r.unwrap().text.as_deref(), got.len()), (Some("ab"), 2));
        assert_eq!(
            parse(Provider::Ollama, &[b"{\"error\":\"model not found\"}\n"]).0.err().unwrap().to_string(),
            "server error during the answer: model not found"
        );
        assert!(parse(Provider::Ollama, &[b"{\"message\":{\"content\":\"a\"},\"done\":false}\n"]).0.is_err());
        // kaputtes UTF-8 in einer Zeile
        assert!(parse(Provider::Ollama, &[b"{\"message\":{\"content\":\"\xff\"}}\n"]).0.is_err());
    }

    #[test]
    fn client_text_never_carries_server_text() {
        let quoted = AiError::Http { code: 400, message: "bad input: quoted-marker".into() };
        // Anzeige im Einstellungsfenster und Log: mit der Meldung des Servers wie bisher
        assert_eq!(quoted.to_string(), "HTTP 400: bad input: quoted-marker");
        assert_eq!(quoted.event_message(), "AI error: HTTP 400: bad input: quoted-marker");
        // Antwort an einen Client: Code und feste Meldung
        assert_eq!(quoted.client_text(), "HTTP 400 (the server rejected the request)");
        for (code, want) in [
            (401, "HTTP 401 (check API key)"),
            (403, "HTTP 403 (check API key)"),
            (404, "HTTP 404 (model or address not found)"),
            (422, "HTTP 422 (the server rejected the request)"),
            (429, "HTTP 429 (rate limit or quota reached)"),
            (500, "HTTP 500 (server error)"),
            (529, "HTTP 529 (server error)"),
            (302, "HTTP 302 (unexpected answer from the server)"),
        ] {
            let e = AiError::Http { code, message: "quoted-marker".into() };
            assert_eq!(e.client_text(), want);
            assert!(!e.client_text().contains("quoted-marker"));
        }
        // 401/403 zeigen den Text auch sonst nicht; ohne Meldung nur der Code
        assert_eq!(status_error(401, r#"{"error":{"message":"key sk-ant-xyz"}}"#).to_string(), "HTTP 401 (check API key)");
        assert_eq!(status_error(500, "").to_string(), "HTTP 500");
        // Fehler mitten im Strom: im Log und im Einstellungsfenster mit Text, an Clients nur die feste Meldung
        let streamed = stream_error(&serde_json::json!({"error": "quoted-marker"}));
        assert_eq!(streamed.to_string(), "server error during the answer: quoted-marker");
        assert_eq!(streamed.client_text(), "server error during the answer");
        let claude = stream_error(&serde_json::json!({"type": "error", "error": {"type": "overloaded_error", "message": "quoted-marker"}}));
        assert!(!claude.client_text().contains("quoted-marker"));
        assert_eq!(stream_error(&serde_json::json!({"error": ""})).to_string(), "server error during the answer");
        // alles andere trägt feste oder lokale Texte und bleibt, wie es ist
        assert_eq!(AiError::Other("response is not JSON".into()).client_text(), "response is not JSON");
        assert_eq!(AiError::Timeout(1500).client_text(), "timed out after 1500 ms");
        assert_eq!(AiError::Cancelled.client_text(), "cancelled");
    }

    #[test]
    fn tokens() {
        assert_eq!(anthropic_max_tokens("hi"), 1024);
        assert_eq!(anthropic_max_tokens(&"x".repeat(30000)), 4096);
        assert_eq!(anthropic_max_tokens(&"x".repeat(1500)), 2000);
    }
}
