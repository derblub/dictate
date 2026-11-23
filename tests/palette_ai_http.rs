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

//! KI-Pfad der Befehlspalette (`palette::ai::ask_intent`) gegen den HTTP-Mock, für alle drei Anbieterarten: Claude
//! (Tool-Use, ein Werkzeug je Absicht plus `no_match`), OpenAI-kompatibel (`response_format` json_schema strict) und
//! Ollama (`format`). Gültige Absicht, ungültige (Bereich, fehlendes und unbekanntes Feld), erfundene Absicht,
//! `no_match`, Frist und Abbruch; Form der Anfrage (Schema, Schutzrahmen vorn, nur die Eingabe als Nachricht, kein
//! `temperature`, Werkzeugwahl und Denken je Modell, Rückfall nach Absage).
mod daemon_common;
mod mock_http;

use dictate::config::{AiCfg, PaletteCfg, INPUT_GUARD};
use dictate::palette::ai::{ask_intent, flat_schema, tools, AskError, NO_MATCH, SCHEMA_NAME};
use dictate::palette::catalog;
use dictate::palette::intent::{Intent, WorkspaceRef};
use dictate::pipeline::ai::{AiError, CancelFlag};
use mock_http::*;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Anthropic,
    OpenAi,
    Ollama,
}

const KINDS: [Kind; 3] = [Kind::Anthropic, Kind::OpenAi, Kind::Ollama];

fn key_file() -> String {
    let p = daemon_common::temp_dir().join("key");
    std::fs::write(&p, "sk-secret-123\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    p.display().to_string()
}

fn ai_cfg(kind: Kind, url: &str, model: &str) -> AiCfg {
    let base = AiCfg { endpoint: url.into(), model: model.into(), timeout_ms: 2000, ..AiCfg::default() };
    match kind {
        Kind::Anthropic => AiCfg { provider: "anthropic".into(), api_key_file: key_file(), ..base },
        Kind::OpenAi => AiCfg { provider: "openai".into(), ..base },
        Kind::Ollama => AiCfg { provider: "ollama".into(), ..base },
    }
}

fn pal(timeout_ms: u64) -> PaletteCfg {
    PaletteCfg { timeout_ms, ..PaletteCfg::default() }
}

/// Alle Felder des flachen Schemas, nicht gegebene als null (so antwortet ein Modell unter strict)
fn flat(v: Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in flat_schema()["properties"].as_object().unwrap().keys() {
        out.insert(k.clone(), v.get(k).cloned().unwrap_or(Value::Null));
    }
    for (k, x) in v.as_object().unwrap() {
        out.insert(k.clone(), x.clone());
    }
    Value::Object(out)
}

/// Antwort des Modells: Werkzeug `name` mit `input` (Claude) bzw. das flache Objekt `{"intent": name, …}`
fn reply(kind: Kind, name: &str, input: Value) -> Resp {
    match kind {
        Kind::Anthropic => Resp::ok(json!({
            "id": "msg_1", "type": "message", "stop_reason": "tool_use",
            "content": [{"type": "tool_use", "id": "toolu_1", "name": name, "input": input}],
        })),
        Kind::OpenAi | Kind::Ollama => {
            let mut v = input;
            v["intent"] = json!(name);
            let text = flat(v).to_string();
            if kind == Kind::OpenAi {
                Resp::ok(json!({"choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]}))
            } else {
                Resp::ok(json!({"message": {"role": "assistant", "content": text}, "done": true, "done_reason": "stop"}))
            }
        }
    }
}

fn no_cancel() -> CancelFlag {
    Arc::new(AtomicBool::new(false))
}

fn ask(kind: Kind, m: &Mock, model: &str, input: &str) -> Result<Intent, AskError> {
    ask_intent(&ai_cfg(kind, &m.url, model), &pal(2000), input, &no_cancel())
}

/// System-Prompt und Nachrichten der Anfrage je Anbieter
fn system_and_user(kind: Kind, b: &Value) -> (String, Value) {
    match kind {
        Kind::Anthropic => (b["system"].as_str().unwrap().to_string(), b["messages"].clone()),
        _ => (b["messages"][0]["content"].as_str().unwrap().to_string(), json!([b["messages"][1]])),
    }
}

#[test]
fn valid_intent_and_request_shape_for_each_provider() {
    for kind in KINDS {
        let m = Mock::start(move |_| reply(kind, "view_workspace", json!({"workspace": 3})));
        let input = "zeig mir tag drei bitte";
        assert_eq!(ask(kind, &m, "claude-haiku-5-5", input).unwrap(), Intent::ViewWorkspace { workspace: WorkspaceRef::Index(3) }, "{kind:?}");
        let reqs = m.requests();
        assert_eq!(reqs.len(), 1, "{kind:?}");
        let b = reqs[0].json();
        assert!(b.get("temperature").is_none(), "{kind:?}: nie temperature");
        assert_eq!(b["model"], "claude-haiku-5-5");
        // Schutzrahmen vorn, genau einmal; der Katalog im Prompt; die Eingabe als einzige Nachricht des Users
        let (system, user) = system_and_user(kind, &b);
        assert!(system.starts_with(INPUT_GUARD), "{kind:?}");
        assert_eq!(system.matches(INPUT_GUARD).count(), 1, "{kind:?}");
        assert!(system.contains(&catalog::catalog_text()), "{kind:?}");
        assert_eq!(user, json!([{"role": "user", "content": input}]), "{kind:?}");
        match kind {
            Kind::Anthropic => {
                assert_eq!(reqs[0].path, "/v1/messages");
                let tools_sent = b["tools"].as_array().unwrap();
                assert_eq!(tools_sent.len(), catalog::all().len() + 1);
                for (sent, want) in tools_sent.iter().zip(tools()) {
                    assert_eq!(sent["name"], json!(want.name));
                    assert_eq!(sent["input_schema"], want.input_schema, "{}", want.name);
                    assert_eq!(sent["strict"], true, "{}", want.name);
                }
                assert_eq!(tools_sent.last().unwrap()["name"], NO_MATCH);
                // Haiku 5.5: Denken aus, also erzwungene Wahl genau eines Werkzeugs
                assert_eq!(b["tool_choice"], json!({"type": "any", "disable_parallel_tool_use": true}));
                assert_eq!(b["thinking"], json!({"type": "disabled"}));
                assert_eq!(b["max_tokens"], 1024);
                assert!(b.get("response_format").is_none() && b.get("format").is_none());
            }
            Kind::OpenAi => {
                assert_eq!(reqs[0].path, "/v1/chat/completions");
                assert_eq!(b["response_format"], json!({"type": "json_schema", "json_schema": {"name": SCHEMA_NAME, "strict": true, "schema": flat_schema()}}));
                assert_eq!(b["max_tokens"], 1024);
                assert!(b.get("tools").is_none() && b.get("format").is_none());
            }
            Kind::Ollama => {
                assert_eq!(reqs[0].path, "/api/chat");
                assert_eq!(&b["format"], flat_schema());
                assert_eq!(b["options"], json!({"num_predict": 1024}));
                assert!(b.get("tools").is_none() && b.get("response_format").is_none());
            }
        }
    }
}

#[test]
fn search_words_come_back_trimmed_and_validated() {
    for kind in KINDS {
        let m = Mock::start(move |_| reply(kind, "move_window_to_workspace", json!({"workspace": "chat", "query": " firefox "})));
        let got = ask(kind, &m, "claude-haiku-5-5", "schieb firefox auf chat").unwrap();
        assert_eq!(got, Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Name("chat".into()), query: Some("firefox".into()) }, "{kind:?}");
    }
}

#[test]
fn invalid_answers_are_errors_without_input_or_model_text() {
    let cases: [(&str, Value, &str); 5] = [
        ("view_workspace", json!({"workspace": 256}), "between 1 and 255"),
        ("volume", json!({"set": 140}), "between 0 and 100"),
        ("focus_window", json!({}), "missing field \"query\""),
        ("lock", json!({"query": "hunter2"}), "unknown field for lock"),
        ("volume", json!({"set": 10, "step": 5}), "exactly one of"),
    ];
    for kind in KINDS {
        for (name, input, want) in &cases {
            let (name, input) = (name.to_string(), input.clone());
            let m = Mock::start(move |_| reply(kind, &name, input.clone()));
            let e = ask(kind, &m, "claude-haiku-5-5", "hunter2 secret words").unwrap_err();
            let msg = e.to_string();
            assert!(matches!(e, AskError::Invalid(_)), "{kind:?}: {e:?}");
            assert!(msg.contains(want), "{kind:?}: {msg}");
            assert!(!msg.contains("hunter2"), "{kind:?}: {msg}");
        }
    }
    // Claude: unbekanntes Feld im Werkzeug (der Name des Felds kann aus der Eingabe stammen)
    let m = Mock::start(|_| reply(Kind::Anthropic, "focus_window", json!({"query": "x", "hunter2": 1})));
    let msg = ask(Kind::Anthropic, &m, "claude-haiku-5-5", "x").unwrap_err().to_string();
    assert!(msg.contains("unknown field for focus_window") && !msg.contains("hunter2"), "{msg}");
}

#[test]
fn invented_intents_and_no_match() {
    for kind in KINDS {
        let m = Mock::start(move |_| reply(kind, "wipe_disk_hunter2", json!({})));
        let e = ask(kind, &m, "claude-haiku-5-5", "x").unwrap_err();
        let msg = e.to_string();
        assert!(matches!(e, AskError::Invalid(_)) && msg.contains("not in the catalog") && !msg.contains("hunter2"), "{kind:?}: {msg}");

        let m = Mock::start(move |_| reply(kind, NO_MATCH, json!({})));
        assert_eq!(ask(kind, &m, "claude-haiku-5-5", "wie ist das wetter").unwrap_err(), AskError::NoMatch, "{kind:?}");
    }
    // Claude antwortet mit Text statt Werkzeug: Fehler, nie geraten
    let m = Mock::start(|_| Resp::ok(json!({"content": [{"type": "text", "text": "lock"}], "stop_reason": "end_turn"})));
    let e = ask(Kind::Anthropic, &m, "claude-haiku-5-5", "x").unwrap_err();
    assert!(matches!(&e, AskError::Ai(AiError::Other(m)) if m.contains("without choosing a tool")), "{e:?}");
    // OpenAI-kompatibel ohne JSON
    let m = Mock::start(|_| Resp::ok(json!({"choices": [{"message": {"content": "lock the screen"}, "finish_reason": "stop"}]})));
    let e = ask(Kind::OpenAi, &m, "m1", "x").unwrap_err();
    assert!(matches!(&e, AskError::Ai(AiError::Other(m)) if m.contains("JSON object")), "{e:?}");
    // abgeschnitten = Fehler
    let m = Mock::start(|_| Resp::ok(json!({"message": {"content": "{\"intent\":"}, "done_reason": "length"})));
    assert!(matches!(ask(Kind::Ollama, &m, "m1", "x").unwrap_err(), AskError::Ai(AiError::Other(m)) if m.contains("cut off")));
}

#[test]
fn server_text_never_reaches_the_client_message() {
    let input = "hunter2 secret words";
    for kind in KINDS {
        // Fehlerstatus, dessen Meldung die Eingabe zitiert (Claude und OpenAI `error.message`, Ollama `error`)
        for code in [400u16, 404, 429, 500] {
            let text = format!("cannot handle: {input}");
            let body = if kind == Kind::Ollama { json!({"error": text}) } else { json!({"error": {"type": "invalid_request_error", "message": text}}) };
            let m = Mock::start(move |_| Resp::status(code, body.clone()));
            let e = ask(kind, &m, "claude-haiku-5-5", input).unwrap_err();
            // im Fehler bleibt der Text (Lernlogik, Einstellungsfenster), in der Meldung an den Client nur Code und feste Meldung
            assert!(matches!(&e, AskError::Ai(AiError::Http { code: c, message }) if *c == code && message.contains("hunter2")), "{kind:?}: {e:?}");
            let msg = e.to_string();
            assert!(msg.starts_with(&format!("AI error: HTTP {code} (")) && msg.ends_with(')') && !msg.contains("hunter2"), "{kind:?} {code}: {msg}");
        }
        let m = Mock::start(|_| Resp::status(401, json!({"error": {"message": "invalid key hunter2"}})));
        assert_eq!(ask(kind, &m, "claude-haiku-5-5", input).unwrap_err().to_string(), "AI error: HTTP 401 (check API key)", "{kind:?}");
    }
    // Werte aus dem Antwortkopf stehen ebenso nicht in der Meldung
    for (header, want) in [("Content-Encoding", "compressed response"), ("Transfer-Encoding", "transfer coding")] {
        let m = Mock::start(move |_| {
            let mut r = Resp::ok(json!({}));
            r.content_type = format!("application/json\r\n{header}: hunter2");
            r
        });
        let msg = ask(Kind::Ollama, &m, "m1", input).unwrap_err().to_string();
        assert!(msg.starts_with("AI error: ") && msg.contains(want) && !msg.contains("hunter2"), "{header}: {msg}");
    }
}

#[test]
fn timeout_is_enforced() {
    for kind in KINDS {
        let m = Mock::start(move |_| reply(kind, "lock", json!({})).delayed(3000));
        let t = Instant::now();
        let e = ask_intent(&ai_cfg(kind, &m.url, "claude-haiku-5-5"), &pal(1000), "lock", &no_cancel()).unwrap_err();
        assert_eq!(e, AskError::Ai(AiError::Timeout(1000)), "{kind:?}");
        assert_eq!(e.to_string(), "AI timeout after 1 s");
        assert!(t.elapsed() < Duration::from_millis(2000), "{kind:?}: {:?}", t.elapsed());
    }
}

#[test]
fn cancel_ends_the_call_at_once() {
    for kind in KINDS {
        let m = Mock::start(move |_| reply(kind, "lock", json!({})).delayed(5000));
        let flag = no_cancel();
        let f2 = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            f2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let e = ask_intent(&ai_cfg(kind, &m.url, "claude-haiku-5-5"), &pal(10_000), "lock", &flag).unwrap_err();
        assert_eq!(e, AskError::Ai(AiError::Cancelled), "{kind:?}");
        assert_eq!(e.to_string(), "Cancelled");
        assert!(t.elapsed() < Duration::from_millis(1500), "{kind:?}: {:?}", t.elapsed());
    }
}

#[test]
fn tool_choice_and_thinking_follow_the_claude_model() {
    let m = Mock::start(|_| reply(Kind::Anthropic, "lock", json!({})));
    let sent = |model: &str| {
        ask(Kind::Anthropic, &m, model, "lock").unwrap();
        m.requests().last().unwrap().json()
    };
    // Opus 5.5: Denken nicht abschaltbar, erzwungene Wahl verboten: auto, nie parallel, effort low
    let b = sent("claude-opus-5-5");
    assert_eq!(b["tool_choice"], json!({"type": "auto", "disable_parallel_tool_use": true}));
    assert_eq!(b["output_config"], json!({"effort": "low"}));
    assert!(b.get("thinking").is_none());
    // Sonnet 5.5: between_tools, erzwungene Wahl verboten
    let b = sent("claude-sonnet-5-5");
    assert_eq!(b["tool_choice"]["type"], "auto");
    assert_eq!(b["thinking"], json!({"type": "between_tools"}));
    // ältere Modelle ohne Denken: erzwungen
    let b = sent("claude-haiku-4-5");
    assert_eq!(b["tool_choice"]["type"], "any");
    assert!(b.get("thinking").is_none());
    // Claude 3 kennt strict nicht
    let b = sent("claude-3-5-haiku-latest");
    assert!(b["tools"].as_array().unwrap().iter().all(|t| t.get("strict").is_none()));
}

#[test]
fn unknown_model_learns_tool_choice_and_strict_from_the_error() {
    let m = Mock::start(|r| {
        let b = r.json();
        if b["tool_choice"]["type"] == "any" {
            return Resp::status(
                400,
                json!({"type": "error", "error": {"type": "invalid_request_error", "message": "tool_choice: type \"tool\" and \"any\" are not supported for this model."}}),
            );
        }
        if b["tools"][0].get("strict").is_some() {
            return Resp::status(
                400,
                json!({"type": "error", "error": {"type": "invalid_request_error", "message": "tools.0.strict: Extra inputs are not permitted"}}),
            );
        }
        reply(Kind::Anthropic, "lock", json!({}))
    });
    let model = "claude-future-palette-9";
    assert_eq!(ask(Kind::Anthropic, &m, model, "lock").unwrap(), Intent::Lock {});
    let kinds: Vec<(Value, bool)> =
        m.requests().iter().map(|r| r.json()).map(|b| (b["tool_choice"]["type"].clone(), b["tools"][0].get("strict").is_some())).collect();
    assert_eq!(kinds, [(json!("any"), true), (json!("auto"), true), (json!("auto"), false)]);
    // gemerkt: der nächste Aufruf geht sofort richtig
    ask(Kind::Anthropic, &m, model, "lock").unwrap();
    let last = m.requests().last().unwrap().json();
    assert_eq!(m.requests().len(), 4);
    assert_eq!(last["tool_choice"]["type"], "auto");
    assert!(last["tools"][0].get("strict").is_none());
}

#[test]
fn a_local_server_may_see_secrets() {
    // wie `actions.secrets = "local_only"`: der Filter sperrt nur die Cloud (hier 127.0.0.1)
    let m = Mock::start(|_| reply(Kind::Ollama, "web_search", json!({"query": "rust"})));
    let input = "search rust ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    assert!(!dictate::actions::secrets::scan(input).is_empty(), "der Testtext muss ein Geheimnis sein");
    assert_eq!(ask(Kind::Ollama, &m, "m1", input).unwrap(), Intent::WebSearch { query: "rust".into() });
}
