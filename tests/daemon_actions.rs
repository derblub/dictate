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

//! Textaktionen im Daemon (`act_grab`, `act_run`, `act_cancel`, `act_result`, `act_clear`) gegen einen HTTP-Mock:
//! Prüfung beim Greifen, Sperre bei Geheimnissen, Ablauf, Zielwechsel, Abbruch, Größen, Terminals, KI-Modi und dass der
//! Kern während eines langsamen Aufrufs antwortet.
mod daemon_common;
mod mock_http;

use daemon_common::*;
use dictate::config::{Config, INPUT_GUARD};
use mock_http::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Antwort im Ollama-Format, die zeigt, was ankam: Länge des Benutzertextes
fn echo(r: &Req) -> Resp {
    let v = r.json();
    let user = v["messages"][1]["content"].as_str().unwrap_or("").to_string();
    Resp::ok(json!({"message": {"content": format!("Answer about {} chars.", user.chars().count())}, "done_reason": "stop"}))
}

fn cfg(url: &str) -> Config {
    let mut c = quick_cfg();
    c.features.actions = true;
    c.ai.endpoint = url.into();
    c.ai.model = "m-ai".into();
    c
}

fn grab_line(text: &str, app: Option<&str>) -> String {
    let mut v = json!({"cmd": "act_grab", "text": text});
    if let Some(a) = app {
        v["app"] = json!(a);
    }
    v.to_string()
}

fn grab(h: &Harness, text: &str, app: Option<&str>) -> Value {
    h.req(&grab_line(text, app))
}

fn run(h: &Harness, id: &str, action: &str) -> Value {
    h.req(&json!({"cmd": "act_run", "id": id, "action": action}).to_string())
}

/// Anfrage auf eigener Verbindung mit langem Lese-Timeout, im Hintergrund
fn run_async(h: &Harness, id: &str, action: &str) -> std::thread::JoinHandle<(Value, Duration)> {
    let (sock, line) = (h.sock.clone(), json!({"cmd": "act_run", "id": id, "action": action}).to_string());
    std::thread::spawn(move || {
        let t = Instant::now();
        let mut s = UnixStream::connect(sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        (serde_json::from_str(out.trim()).unwrap(), t.elapsed())
    })
}

fn id_of(v: &Value) -> String {
    assert_eq!(v["ok"], true, "{v}");
    v["id"].as_str().unwrap().to_string()
}

/// Nicht-Loopback-Adresse dieses Rechners (gilt als Cloud), ohne Pakete zu senden; `None` ohne Netz
fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

#[test]
fn grab_and_run_locally() {
    let m = Mock::start(echo);
    let h = Harness::start(cfg(&m.url));
    let text = "Die Funktion gibt None zurück,\nwenn der Schlüssel fehlt.";
    let g = grab(&h, text, Some("firefox"));
    let id = id_of(&g);
    assert_eq!(g["chars"], text.chars().count());
    assert_eq!(g["lines"], 2);
    assert_eq!(g["target"], json!({"host": "127.0.0.1", "model": "m-ai", "cloud": false}));
    assert_eq!(g["secret"], json!([]));
    assert_eq!(g["blocked"], false);
    assert_eq!(g["replace_ok"], true);
    let actions = g["actions"].as_array().unwrap();
    assert_eq!(actions[0], json!({"key": "e", "name": "explain", "label": "Explain", "output": "answer", "allowed": true, "model": "m-ai"}));
    assert!(actions.iter().any(|a| a["name"] == "mode:clean" && a["output"] == "replace"));
    assert!(m.requests().is_empty(), "grab sends nothing");

    let r = run(&h, &id, "explain");
    let want = format!("Answer about {} chars.", text.chars().count());
    assert_eq!(r, json!({"ok": true, "text": want, "output": "answer", "truncated": false}));
    let reqs = m.requests();
    assert_eq!(reqs.len(), 1);
    let body = reqs[0].json();
    assert_eq!(body["model"], "m-ai");
    assert_eq!(body["messages"][1]["content"], text, "the selection is the whole user message");
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with(INPUT_GUARD), "{system}");
    assert!(system.contains("Explain the text"), "{system}");
    assert_eq!(body["options"]["num_predict"], 4096);
    // Ergebnis für Copy
    let res = h.req(&json!({"cmd": "act_result", "id": id}).to_string());
    assert_eq!(res, json!({"ok": true, "text": want, "output": "answer", "truncated": false, "action": "explain"}));
    // nie in den Verlauf
    assert!(!h.history_path().exists());
    // clear löscht
    assert_eq!(h.req(r#"{"cmd":"act_clear"}"#), json!({"ok": true}));
    assert_eq!(run(&h, &id, "explain")["error"], "Unknown or expired selection, select again");
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["ok"], false);
    assert_eq!(m.requests().len(), 1);
}

#[test]
fn secret_with_cloud_target_is_blocked_and_nothing_is_sent() {
    let Some(ip) = lan_ip() else {
        eprintln!("no non-loopback address, skipping");
        return;
    };
    let m = Mock::start_on(&ip, echo);
    let h = Harness::start(cfg(&m.url));
    // Gegenprobe: das Ziel gilt als Cloud und ist erreichbar
    let g = grab(&h, "Ein ganz normaler Satz.", None);
    assert_eq!(g["target"]["cloud"], true, "{g}");
    assert_eq!(g["target"]["host"], ip.as_str());
    assert_eq!(run(&h, &id_of(&g), "explain")["ok"], true);
    assert_eq!(m.requests().len(), 1);

    let secret_text = format!("DB_PASSWORD=hunter2\nAPI=ok {}", "x");
    let g = grab(&h, &secret_text, Some("firefox"));
    let id = id_of(&g);
    assert_eq!(g["secret"], json!(["secret_assignment"]));
    assert_eq!(g["blocked"], true);
    assert!(g["actions"].as_array().unwrap().iter().all(|a| a["allowed"] == false));
    let r = run(&h, &id, "explain");
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"], "Blocked: the selection contains secrets (Password or secret value)");
    assert_eq!(r["secret"], json!(["secret_assignment"]));
    assert_eq!(run(&h, &id, "mode:clean")["ok"], false);
    assert_eq!(m.requests().len(), 1, "nothing of the secret left the machine");
}

#[test]
fn local_target_allows_secrets_with_local_only_and_block_blocks() {
    let m = Mock::start(echo);
    let h = Harness::start(cfg(&m.url));
    let text = "password: Sommer2024";
    let g = grab(&h, text, None);
    assert_eq!((g["secret"].clone(), g["blocked"].clone()), (json!(["secret_assignment"]), json!(false)));
    assert_eq!(run(&h, &id_of(&g), "explain")["ok"], true);
    assert_eq!(m.requests().len(), 1);

    let mut c = cfg(&m.url);
    c.actions.secrets = "block".into();
    let h = Harness::start(c);
    let g = grab(&h, text, None);
    assert_eq!(g["blocked"], true);
    assert!(run(&h, &id_of(&g), "explain")["error"].as_str().unwrap().starts_with("Blocked: "));
    assert_eq!(m.requests().len(), 1);

    // Umschalten auf `block` nach dem Greifen sperrt den nächsten Aufruf
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, text, None));
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"actions":{"secrets":"block"}}}"#)["ok"], true);
    assert!(run(&h, &id, "explain")["error"].as_str().unwrap().starts_with("Blocked: "));
    assert_eq!(m.requests().len(), 1);
}

#[test]
fn feature_off() {
    let m = Mock::start(echo);
    let mut c = cfg(&m.url);
    c.features.actions = false;
    let h = Harness::start(c);
    let g = grab(&h, "Hallo Welt", None);
    assert_eq!(g, json!({"ok": false, "error": "Text actions are off (Settings › Actions)"}));
    // eingeschaltet, gegriffen, wieder aus: der Aufruf scheitert und die Markierung ist weg
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"features":{"actions":true}}}"#)["ok"], true);
    let id = id_of(&grab(&h, "Hallo Welt", None));
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"features":{"actions":false}}}"#)["ok"], true);
    assert_eq!(run(&h, &id, "explain")["error"], "Text actions are off (Settings › Actions)");
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"features":{"actions":true}}}"#)["ok"], true);
    assert_eq!(run(&h, &id, "explain")["error"], "Unknown or expired selection, select again");
    assert!(m.requests().is_empty());
}

#[test]
fn expired_replaced_and_unknown_ids() {
    let m = Mock::start(echo);
    let ctl = Arc::new(Ctl::default());
    ctl.act_ttl_ms.store(300, Ordering::SeqCst);
    let h = Harness::start_with(cfg(&m.url), ctl);
    let unknown = "Unknown or expired selection, select again";
    // abgelaufen
    let id = id_of(&grab(&h, "Hallo Welt", None));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(run(&h, &id, "explain")["error"], unknown);
    // durch neues grab ersetzt
    let a = id_of(&grab(&h, "eins", None));
    let b = id_of(&grab(&h, "zwei", None));
    assert_ne!(a, b);
    assert_eq!(run(&h, &a, "explain")["error"], unknown);
    // ein fehlgeschlagenes grab löscht die vorige Markierung ebenfalls
    assert_eq!(grab(&h, "   ", None), json!({"ok": false, "error": "Nothing selected"}));
    assert_eq!(run(&h, &b, "explain")["error"], unknown);
    // Unsinn
    assert_eq!(run(&h, "nope", "explain")["error"], unknown);
    assert_eq!(run(&h, "999", "explain")["error"], unknown);
    let id = id_of(&grab(&h, "drei", None));
    assert_eq!(run(&h, &id, "summarize")["error"], "Unknown action \"summarize\"");
    assert!(m.requests().is_empty());
    // Benutzung verlängert die Frist
    assert_eq!(run(&h, &id, "explain")["ok"], true);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["ok"], true);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["ok"], true);
}

#[test]
fn target_change_after_grab() {
    let a = Mock::start(echo);
    let b = Mock::start(echo);
    let h = Harness::start(cfg(&a.url));
    let id = id_of(&grab(&h, "Hallo Welt", None));
    let patch = json!({"cmd": "set_config", "config": {"ai": {"endpoint": b.url, "model": "m-ai"}}});
    assert_eq!(h.req(&patch.to_string())["ok"], true, "set_config");
    assert_eq!(run(&h, &id, "explain")["error"], "AI server changed, select again");
    assert_eq!(run(&h, &id, "explain")["error"], "Unknown or expired selection, select again");
    assert!(a.requests().is_empty() && b.requests().is_empty());
    // ein anderes Modell auf demselben Server ist kein Zielwechsel
    let id = id_of(&grab(&h, "Hallo Welt", None));
    let patch = json!({"cmd": "set_config", "config": {"ai": {"endpoint": b.url, "model": "m-other"}}});
    assert_eq!(h.req(&patch.to_string())["ok"], true);
    assert_eq!(run(&h, &id, "explain")["ok"], true);
    assert_eq!(b.requests()[0].json()["model"], "m-other");
}

#[test]
fn cancel_ends_a_slow_run_quickly_and_a_second_run_cancels_the_first() {
    let m = Mock::start(|r| echo(r).delayed(3000));
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "Hallo Welt", None));
    let t = run_async(&h, &id, "explain");
    h.wait_until("request at the mock", || !m.requests().is_empty());
    let t0 = Instant::now();
    assert_eq!(h.req(&json!({"cmd": "act_cancel", "id": id}).to_string()), json!({"ok": true}));
    let (r, _) = t.join().unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500), "cancel took {:?}", t0.elapsed());
    assert_eq!(r, json!({"ok": false, "error": "Cancelled", "cancelled": true}));
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["error"], "No result yet");
    // cancel ohne laufenden Aufruf ist harmlos
    assert_eq!(h.req(&json!({"cmd": "act_cancel", "id": id}).to_string()), json!({"ok": true}));
    assert_eq!(h.req(r#"{"cmd":"act_cancel","id":"12345"}"#), json!({"ok": true}));

    // zweiter Aufruf bricht den ersten ab
    let first = run_async(&h, &id, "explain");
    h.wait_until("first request", || m.requests().len() == 2);
    let t0 = Instant::now();
    let second = run_async(&h, &id, "translate");
    let (r1, _) = first.join().unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500), "first run ended after {:?}", t0.elapsed());
    assert_eq!(r1["cancelled"], true, "{r1}");
    // ein neues grab bricht den zweiten ab
    h.wait_until("second request", || m.requests().len() == 3);
    let t0 = Instant::now();
    id_of(&grab(&h, "Neu", None));
    let (r2, _) = second.join().unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500));
    assert_eq!(r2["cancelled"], true, "{r2}");
}

#[test]
fn sizes() {
    let m = Mock::start(echo);
    let mut c = cfg(&m.url);
    c.actions.max_chars = 100;
    let h = Harness::start(c);
    let r = grab(&h, &"a".repeat(101), None);
    assert_eq!(r, json!({"ok": false, "error": "Selection too long: 101 characters, at most 100 (Settings › Actions)", "field": "actions.max_chars"}));
    assert_eq!(grab(&h, &"ä".repeat(100), None)["ok"], true);

    // Zeilen über 64 KB sind für act_grab bis 256 KB erlaubt (30000 Zeichen à 4 Byte), andere Befehle bleiben bei 64 KB
    let mut c = cfg(&m.url);
    c.actions.max_chars = 30_000;
    let h = Harness::start(c);
    let big = "😀".repeat(30_000);
    let line = grab_line(&big, None);
    assert!(line.len() > 64 * 1024 && line.len() < 256 * 1024);
    let g = h.req(&line);
    assert_eq!((g["ok"].clone(), g["chars"].clone()), (json!(true), json!(30_000)));
    assert_eq!(h.req(&grab_line(&"😀".repeat(30_001), None))["error"], "Selection too long: 30001 characters, at most 30000 (Settings › Actions)");
    assert_eq!(h.req(&grab_line(&"a".repeat(300 * 1024), None)), json!({"ok": false, "error": "request too large"}));
    let other = json!({"cmd": "ai_test", "mode": "clean", "text": "a".repeat(70 * 1024)}).to_string();
    assert_eq!(h.req(&other), json!({"ok": false, "error": "request too large"}));
    // Ersetzende Aktionen bekommen eine Längengrenze passend zur Eingabe
    let id = id_of(&h.req(&grab_line(&"Satz. ".repeat(1500), None)));
    assert_eq!(run(&h, &id, "translate")["ok"], true);
    assert_eq!(m.requests().last().unwrap().json()["options"]["num_predict"], 4756);
    assert!(m.requests().iter().all(|r| !r.body.contains('😀')), "nothing sent on grab");
}

/// Refine prompt (wachsende Aktion): wie beim Diktat-Modus `prompt` steht das Limit dieser Markierung (dreifach, in
/// Zeichen) am Ende des System-Prompts; `max_tokens` richtet sich nach der harten Grenze. Andere Aktionen bekommen es nicht.
#[test]
fn refine_prompt_states_the_length_limit_of_the_selection() {
    use dictate::config::{with_guard, with_length_budget, PROMPT_REFINE};
    let m = Mock::start(echo);
    let h = Harness::start(cfg(&m.url));
    let text = "Ich brauche eine Übersicht für unser Team, wie wir mit Urlaubsanträgen umgehen."; // 79 Zeichen
    assert_eq!(text.chars().count(), 79);
    let id = id_of(&grab(&h, text, Some("firefox")));
    assert_eq!(run(&h, &id, "refine_prompt")["ok"], true);
    let b = m.requests().last().unwrap().json();
    assert_eq!(b["messages"][0]["content"], with_length_budget(&with_guard(PROMPT_REFINE), 237).as_str());
    assert_eq!(b["messages"][1]["content"], text);
    // harte Grenze 474 Zeichen -> 237 + 256 Tokens, auf 1024 angehoben
    assert_eq!(b["options"]["num_predict"], 1024);
    // Erklären: Antwortaktion, kein Limit im Prompt
    assert_eq!(run(&h, &id, "explain")["ok"], true);
    let system = m.requests().last().unwrap().json()["messages"][0]["content"].as_str().unwrap().to_string();
    assert!(system.starts_with(INPUT_GUARD) && !system.contains("Length limit"), "{system}");
}

#[test]
fn replace_ok_depends_on_the_app() {
    let m = Mock::start(echo);
    let h = Harness::start(cfg(&m.url));
    assert_eq!(grab(&h, "Hallo", Some("Alacritty"))["replace_ok"], false);
    assert_eq!(grab(&h, "Hallo", Some("alacritty"))["replace_ok"], false, "case-insensitive");
    assert_eq!(grab(&h, "Hallo", Some("org.wezfurlong.wezterm"))["replace_ok"], false);
    assert_eq!(grab(&h, "Hallo", Some("firefox"))["replace_ok"], true);
    assert_eq!(grab(&h, "Hallo", None)["replace_ok"], false, "unknown window: never replace");
    assert_eq!(grab(&h, "Hallo", Some(""))["replace_ok"], false);
}

#[test]
fn ai_mode_runs_through_rewrite_with_the_action_timeout_and_model_precedence() {
    // langsamer als ai.timeout_ms, schneller als actions.timeout_ms
    let m = Mock::start(|r| {
        let v = r.json();
        let user = v["messages"][1]["content"].as_str().unwrap_or("").to_string();
        Resp::ok(json!({"message": {"content": format!("\"{}\"", user.trim().replace("hallo", "Hallo"))}})).delayed(600)
    });
    let mut c = cfg(&m.url);
    c.ai.timeout_ms = 200;
    c.actions.timeout_ms = 3000;
    c.actions.model = "m-act".into();
    c.ai.mode_models.insert("clean".into(), "m-clean".into());
    let h = Harness::start(c);
    let g = grab(&h, "hallo welt\n", Some("firefox"));
    let acts = g["actions"].as_array().unwrap();
    let model = |n: &str| acts.iter().find(|a| a["name"] == n).unwrap()["model"].clone();
    assert_eq!((model("mode:clean"), model("explain"), model("mode:email")), (json!("m-clean"), json!("m-act"), json!("m-act")));
    assert_eq!(g["target"]["model"], "m-act");
    let id = id_of(&g);
    let r = run(&h, &id, "mode:clean");
    // rewrite: Anführungszeichen weg, Zeilenumbruch am Ende bleibt; Wortdiff zur Markierung
    let diff = json!([["-", "hallo"], ["+", "Hallo"], ["=", " welt\n"]]);
    assert_eq!(r, json!({"ok": true, "text": "Hallo welt\n", "output": "replace", "truncated": false, "diff": diff}));
    let body = m.requests()[0].json();
    assert_eq!(body["model"], "m-clean");
    let system = body["messages"][0]["content"].as_str().unwrap();
    let cfg = Config::default();
    assert_eq!(system, cfg.ai.mode_prompt("clean").unwrap());
    assert_eq!(body["options"]["temperature"], 0.2);
    assert_eq!(run(&h, &id, "explain")["ok"], true);
    assert_eq!(m.requests()[1].json()["model"], "m-act");
}

#[test]
fn errors_from_the_ai_are_reported() {
    let m = Mock::start(|r| match r.json()["messages"][1]["content"].as_str() {
        Some("slow") => echo(r).delayed(3000),
        _ => Resp::status(404, json!({"error": "model 'm-ai' not found"})),
    });
    let mut c = cfg(&m.url);
    c.actions.timeout_ms = 1000;
    let h = Harness::start(c);
    let id = id_of(&grab(&h, "x y", None));
    let r = run(&h, &id, "explain");
    assert_eq!(r["ok"], false);
    // Code und feste Meldung, nie der Text des Servers (Entscheidung Daniel 21.11.2026, wie `pal_ask`)
    assert_eq!(r["error"], "AI error: HTTP 404 (model or address not found)", "{r}");
    let id = id_of(&grab(&h, "slow", None));
    assert_eq!(run(&h, &id, "explain"), json!({"ok": false, "error": "AI timeout after 1 s"}));
    // nach einem Fehler bleibt die Markierung für einen neuen Versuch
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["error"], "No result yet");
}

#[test]
fn status_answers_during_a_slow_run() {
    let m = Mock::start(|r| echo(r).delayed(1500));
    let h = Harness::start(cfg(&m.url));
    h.wait_state("idle");
    let id = id_of(&grab(&h, "Hallo Welt", None));
    let t = run_async(&h, &id, "explain");
    h.wait_until("request at the mock", || !m.requests().is_empty());
    for _ in 0..3 {
        let t0 = Instant::now();
        assert_eq!(h.state(), "idle");
        assert!(t0.elapsed() < Duration::from_millis(200), "status took {:?}", t0.elapsed());
        // auch andere Textaktions-Befehle warten nicht auf den Aufruf
        let t0 = Instant::now();
        assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["error"], "No result yet");
        assert!(t0.elapsed() < Duration::from_millis(200));
        std::thread::sleep(Duration::from_millis(100));
    }
    let (r, took) = t.join().unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert!(took >= Duration::from_millis(1400));
}

// ---------------------------------------------------------------------------------------------------------------
// Streaming (Aufgabe 7)

/// `act_run` auf eigener Verbindung: alle Zeilen bis zur Antwort (`delta`-Zeilen als Text, Antwort geparst)
fn run_lines(sock: &std::path::Path, id: &str, action: &str) -> (Vec<String>, Value) {
    let mut s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    writeln!(s, "{}", json!({"cmd": "act_run", "id": id, "action": action})).unwrap();
    let mut r = BufReader::new(s);
    let mut deltas = Vec::new();
    loop {
        let mut l = String::new();
        assert!(r.read_line(&mut l).unwrap() > 0, "closed before the reply");
        let v: Value = serde_json::from_str(l.trim()).unwrap();
        if v.get("ok").is_some() {
            return (deltas, v);
        }
        assert_eq!(v.as_object().unwrap().len(), 1, "{v}");
        deltas.push(v["delta"].as_str().unwrap().to_string());
    }
}

/// Ollama-NDJSON mit `n` Stücken im Abstand von `every_ms`
fn ndjson_stream(n: usize, every_ms: u64) -> Resp {
    let words: Vec<String> = (0..n).map(|i| format!("wört{i}·{} ", if i % 7 == 0 { "👨\u{200d}👩" } else { "x" })).collect();
    let texts: Vec<&str> = words.iter().map(String::as_str).collect();
    let body = ollama_ndjson(&texts, "stop");
    let per = body.len() / n + 1;
    Resp::stream("application/x-ndjson", split(&body, per, every_ms))
}

/// Freie Aktionen streamen: `delta`-Zeilen vor der Antwort, gedrosselt (≈ 50 ms), Endtext == Verkettung
#[test]
fn ask_actions_stream_throttled_delta_lines_before_the_reply() {
    let m = Mock::start(|_| ndjson_stream(150, 3));
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "some selected text", Some("firefox")));
    let t = Instant::now();
    let (deltas, r) = run_lines(&h.sock, &id, "explain");
    let took = t.elapsed();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["output"], "answer");
    let text = r["text"].as_str().unwrap();
    assert!(text.starts_with("wört0·👨\u{200d}👩 wört1·x") && text.ends_with("wört149·x"), "{text}");
    assert_eq!(deltas.concat(), text);
    // 150 Stücke in ≈ 450 ms: gebündelt zu wenigen Zeilen, aber mehr als einer
    let max_lines = (took.as_millis() / 50) as usize + 2;
    assert!(deltas.len() >= 2 && deltas.len() <= max_lines, "{} lines in {took:?}", deltas.len());
    assert!(deltas.len() < 40, "{}", deltas.len());
    assert_eq!(m.requests()[0].json()["stream"], true);
    // Copy holt denselben Text
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["text"], text);
}

/// KI-Modi streamen nicht: ihr Ersatztext wird erst als Ganzes geprüft
#[test]
fn ai_modes_do_not_stream() {
    let m = Mock::start(|r| {
        assert_eq!(r.json()["stream"], false);
        Resp::ok(json!({"message": {"content": "Hallo Welt."}, "done": true, "done_reason": "stop"}))
    });
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "hallo welt", Some("firefox")));
    let (deltas, r) = run_lines(&h.sock, &id, "mode:clean");
    assert_eq!((deltas.len(), r["text"].as_str()), (0, Some("Hallo Welt.")));
}

/// Über eine Subscriber-Verbindung kommen die Zeilen zwischen den Events an
#[test]
fn deltas_on_a_subscriber_connection() {
    let m = Mock::start(|_| ndjson_stream(20, 10));
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "text", None));
    let mut sub = h.subscribe();
    sub.send(&json!({"cmd": "act_run", "id": id, "action": "explain"}).to_string());
    let mut deltas = String::new();
    let reply = loop {
        let l = sub.next_line().expect("closed");
        let v: Value = serde_json::from_str(&l).unwrap();
        if v.get("ok").is_some() {
            break v;
        }
        if let Some(d) = v.get("delta").and_then(Value::as_str) {
            deltas.push_str(d);
        }
    };
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["text"].as_str().unwrap(), deltas);
}

/// Fehler mitten im Stream: Endantwort ist der Fehler (die Deltas davor verwirft der Client)
#[test]
fn error_in_the_middle_of_a_stream_is_the_reply() {
    let m = Mock::start(|_| {
        // zwei ganze Zeilen ohne `done`, dann die Fehlerzeile
        let body = ollama_ndjson(&["Teil eins, ", "Teil zwei"], "stop");
        let two = body.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(1).unwrap().0 + 1;
        let mut chunks = split(&body[..two], 30, 5);
        chunks.push((60, b"{\"error\":\"model runner crashed\"}\n".to_vec()));
        Resp::stream("application/x-ndjson", chunks)
    });
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "text", None));
    let (_, r) = run_lines(&h.sock, &id, "explain");
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"], "AI error: server error during the answer", "the server's text never reaches the panel");
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["error"], "No result yet");
}

/// Schließt der Client die Verbindung mitten im Stream, bricht der Daemon den KI-Aufruf ab und schließt auch die
/// Verbindung zum Server
#[test]
fn client_gone_mid_stream_cancels_the_ai_call() {
    let m = Mock::start(|_| ndjson_stream(100, 50));
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "text", None));
    {
        let mut s = UnixStream::connect(&h.sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        writeln!(s, "{}", json!({"cmd": "act_run", "id": id, "action": "explain"})).unwrap();
        let mut l = String::new();
        BufReader::new(s).read_line(&mut l).unwrap();
        assert!(l.starts_with("{\"delta\":"), "{l}");
    }
    let t = Instant::now();
    while m.aborted.load(Ordering::Relaxed) == 0 {
        assert!(t.elapsed() < Duration::from_secs(3), "the AI call kept running");
        std::thread::sleep(Duration::from_millis(20));
    }
    // der Daemon antwortet weiter, die Sitzung ist frei für einen neuen Aufruf
    assert_eq!(h.req(r#"{"cmd":"status"}"#)["ok"], true);
}

/// Ersetzende Aktionen mit Markierung (Umformulieren, KI-Modi außer Übersetzen) bekommen einen Wortdiff zur
/// Markierung; Übersetzen, Antworten und überwiegend neue Texte nicht
#[test]
fn replace_with_marks_carries_a_word_diff() {
    let m = Mock::start(|r: &Req| {
        let user = r.json()["messages"][1]["content"].as_str().unwrap_or("").to_string();
        let out = if user.starts_with("Ganz") { "Something entirely different now".to_string() } else { user.replace("Test", "Versuch") };
        Resp::ok(json!({"message": {"content": out}, "done_reason": "stop"}))
    });
    let h = Harness::start(cfg(&m.url));
    let id = id_of(&grab(&h, "Das ist ein Test für Größe.", Some("firefox")));
    let want = json!([["=", "Das ist ein "], ["-", "Test"], ["+", "Versuch"], ["=", " für Größe."]]);
    for action in ["rephrase", "mode:clean"] {
        let r = run(&h, &id, action);
        assert_eq!(r["text"], "Das ist ein Versuch für Größe.", "{action}: {r}");
        assert_eq!(r["diff"], want, "{action}: {r}");
    }
    for action in ["translate", "mode:to_de", "explain"] {
        let r = run(&h, &id, action);
        assert_eq!(r["ok"], true, "{action}: {r}");
        assert!(r.get("diff").is_none(), "{action}: {r}");
    }
    // überwiegend neu: kein Diff
    let id = id_of(&grab(&h, "Ganz kurz", Some("firefox")));
    let r = run(&h, &id, "rephrase");
    assert_eq!((r["ok"].clone(), r.get("diff")), (json!(true), None), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------
// Befehlspalette: Frage (`ask`) und Zwischenablage (`explain_clipboard`), PLAN-palette.md Aufgabe 9

fn pal_grab(h: &Harness, text: &str, source: &str) -> Value {
    h.req(&json!({"cmd": "act_grab", "text": text, "source": source}).to_string())
}

/// Palette ohne Textaktionen: die Quellen der Palette hängen an `palette.enabled`, nicht an `features.actions`
fn pal_cfg(url: &str) -> Config {
    let mut c = cfg(url);
    c.features.actions = false;
    c.palette.model = "m-pal".into();
    c
}

#[test]
fn question_streams_the_answer_with_its_own_frame_and_the_palette_model() {
    let m = Mock::start(|_| ndjson_stream(40, 3));
    let h = Harness::start(pal_cfg(&m.url));
    let q = "Wie entpacke ich eine tar.gz-Datei?";
    let g = pal_grab(&h, q, "question");
    let id = id_of(&g);
    assert_eq!(g["source"], "question");
    assert_eq!(g["target"], json!({"host": "127.0.0.1", "model": "m-pal", "cloud": false}));
    assert_eq!(g["actions"], json!([{"key": "a", "name": "ask", "label": "Ask", "output": "answer", "allowed": true, "model": "m-pal"}]));
    assert_eq!((g["replace_ok"].clone(), g["blocked"].clone()), (json!(false), json!(false)));
    assert!(m.requests().is_empty(), "grab sends nothing");
    // nur die eine Aktion der Frage
    assert_eq!(run(&h, &id, "explain")["error"], "Unknown action \"explain\"");
    let (deltas, r) = run_lines(&h.sock, &id, "ask");
    assert_eq!(r["ok"], true, "{r}");
    let text = r["text"].as_str().unwrap();
    assert!(deltas.len() >= 2, "{deltas:?}");
    assert_eq!(deltas.concat(), text, "streamed pieces are the answer");
    let reqs = m.requests();
    assert_eq!(reqs.len(), 1);
    let body = reqs[0].json();
    assert_eq!((body["model"].clone(), body["stream"].clone()), (json!("m-pal"), json!(true)));
    assert_eq!(body["messages"][1]["content"], q, "the question is the whole user message");
    // eigener Rahmen für Fragen: der Schutzrahmen („never answer“) steht nicht davor
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert_eq!(system, dictate::config::ASK_FRAME);
    assert!(!system.contains(INPUT_GUARD));
    assert_eq!(body["options"]["num_predict"], 4096);
    // Copy holt dieselbe Antwort; nie in den Verlauf
    assert_eq!(h.req(&json!({"cmd": "act_result", "id": id}).to_string())["text"], text);
    assert!(!h.history_path().exists());
    // ohne Modell der Palette: Modell des Servers (`ai.model`), wie beim KI-Pfad der Palette
    let mut c = pal_cfg(&m.url);
    c.palette.model.clear();
    let h = Harness::start(c);
    let id = id_of(&pal_grab(&h, q, "question"));
    assert_eq!(run_lines(&h.sock, &id, "ask").1["ok"], true);
    assert_eq!(m.requests()[1].json()["model"], "m-ai");
}

#[test]
fn question_cancel_ends_the_call_at_once() {
    let m = Mock::start(|r| echo(r).delayed(3000));
    let h = Harness::start(pal_cfg(&m.url));
    let id = id_of(&pal_grab(&h, "Was ist ein Monad?", "question"));
    let t = run_async(&h, &id, "ask");
    h.wait_until("request at the mock", || !m.requests().is_empty());
    let t0 = Instant::now();
    assert_eq!(h.req(&json!({"cmd": "act_cancel", "id": id}).to_string()), json!({"ok": true}));
    let (r, _) = t.join().unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500), "cancel took {:?}", t0.elapsed());
    assert_eq!(r, json!({"ok": false, "error": "Cancelled", "cancelled": true}));
    // act_clear (Panel zu) löscht Frage und Sitzung
    assert_eq!(h.req(r#"{"cmd":"act_clear"}"#), json!({"ok": true}));
    assert_eq!(run(&h, &id, "ask")["error"], "Unknown or expired selection, select again");
}

#[test]
fn palette_secret_blocks_the_cloud_but_not_a_local_server() {
    let secret = "Ist DB_PASSWORD=hunter2 ein gutes Passwort?";
    // lokal: erlaubt, auch mit `actions.secrets = "block"` (gilt nur für Markierungen)
    let m = Mock::start(echo);
    let mut c = pal_cfg(&m.url);
    c.actions.secrets = "block".into();
    let h = Harness::start(c);
    let g = pal_grab(&h, secret, "question");
    assert_eq!((g["secret"].clone(), g["blocked"].clone()), (json!(["secret_assignment"]), json!(false)));
    assert_eq!(run(&h, &id_of(&g), "ask")["ok"], true);
    assert_eq!(m.requests().len(), 1);
    // Geheimnisfilter gegen den Server, der beim Senden gilt: lokal gegriffen, dann auf einen anderen Server
    // umgestellt, wird nichts gesendet
    let id = id_of(&pal_grab(&h, secret, "question"));
    let other = Mock::start(echo);
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"ai": {"endpoint": other.url}}}).to_string())["ok"], true);
    assert_eq!(run(&h, &id, "ask")["error"], "AI server changed, select again");
    assert!(other.requests().is_empty());
    assert_eq!(m.requests().len(), 1);

    let Some(ip) = lan_ip() else {
        eprintln!("no non-loopback address, skipping the cloud part");
        return;
    };
    let cloud = Mock::start_on(&ip, echo);
    let h = Harness::start(pal_cfg(&cloud.url));
    for (source, action, what) in [("question", "ask", "question"), ("clipboard", "explain", "clipboard")] {
        let g = pal_grab(&h, secret, source);
        assert_eq!(g["target"]["cloud"], true, "{g}");
        assert_eq!((g["secret"].clone(), g["blocked"].clone()), (json!(["secret_assignment"]), json!(true)), "{source}");
        assert_eq!(g["actions"][0]["allowed"], false);
        let r = run(&h, &id_of(&g), action);
        assert_eq!(r["error"], format!("Blocked: the {what} contains secrets (Password or secret value)"));
        assert_eq!(r["secret"], json!(["secret_assignment"]));
    }
    assert!(cloud.requests().is_empty(), "nothing of the secret left the machine");
    // Gegenprobe: ohne Geheimnis geht die Frage in die Cloud
    assert_eq!(run(&h, &id_of(&pal_grab(&h, "Was ist ein Monad?", "question")), "ask")["ok"], true);
    assert_eq!(cloud.requests().len(), 1);
}

#[test]
fn clipboard_is_only_sent_after_confirmation_with_limit_and_switch() {
    let m = Mock::start(echo);
    let mut c = pal_cfg(&m.url);
    c.actions.max_chars = 100;
    let h = Harness::start(c);
    let text = "fn main() {\n    println!(\"hi\");\n}";
    let g = pal_grab(&h, text, "clipboard");
    let id = id_of(&g);
    // Vorschau: Zeichen, Zeilen, Ziel; die eine Aktion ist Explain mit dem Modell der Palette
    assert_eq!((g["source"].clone(), g["chars"].clone(), g["lines"].clone()), (json!("clipboard"), json!(text.chars().count()), json!(3)));
    assert_eq!(g["target"]["host"], "127.0.0.1");
    assert_eq!(g["actions"], json!([{"key": "e", "name": "explain", "label": "Explain", "output": "answer", "allowed": true, "model": "m-pal"}]));
    std::thread::sleep(Duration::from_millis(200));
    assert!(m.requests().is_empty(), "nothing is sent before the confirmation");
    // Bestätigung = act_run
    assert_eq!(run(&h, &id, "explain")["ok"], true);
    let body = m.requests()[0].json();
    assert_eq!(body["messages"][1]["content"], text);
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with(INPUT_GUARD) && system.contains("Explain the text"), "{system}");
    assert_eq!(body["model"], "m-pal");
    assert!(!h.history_path().exists());
    // Grenze `actions.max_chars`, leere Zwischenablage, leere Frage
    assert_eq!(
        pal_grab(&h, &"x".repeat(101), "clipboard"),
        json!({"ok": false, "error": "Clipboard text too long: 101 characters, at most 100 (Settings › Actions)", "field": "actions.max_chars"})
    );
    assert_eq!(pal_grab(&h, " \n ", "clipboard"), json!({"ok": false, "error": "The clipboard is empty"}));
    assert_eq!(pal_grab(&h, "", "question"), json!({"ok": false, "error": "Nothing to ask"}));
    // Palette aus: weder greifen noch senden, die Sitzung ist danach weg
    let id = id_of(&pal_grab(&h, text, "clipboard"));
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"palette":{"enabled":false}}}"#)["ok"], true);
    assert_eq!(run(&h, &id, "explain")["error"], "The command palette is off (Settings › Palette)");
    assert_eq!(pal_grab(&h, text, "clipboard")["error"], "The command palette is off (Settings › Palette)");
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"palette":{"enabled":true}}}"#)["ok"], true);
    assert_eq!(run(&h, &id, "explain")["error"], "Unknown or expired selection, select again");
    assert_eq!(m.requests().len(), 1);
    // die Markierung bleibt bei den Textaktionen (hier aus)
    assert_eq!(grab(&h, "Hallo", None)["error"], "Text actions are off (Settings › Actions)");
}

// ---------------------------------------------------------------------------------------------------------------
// Lesart des Panels unter Wayland (`dictate-ui`, PLAN-wayland.md 6d): dieselben Funktionen wie im Client
// (`GrabInfo::from_json`, `RunEv::from_reply`) gegen die Antworten des echten Daemons, nicht nur gegen den Mock des
// Prüfschritts `wayland` (sonst teilten Mock und Client nur eine Lesart)

#[test]
fn panel_reads_the_real_replies() {
    use dictate::ui::markdown::DiffPiece;
    use dictate::ui::panel::{GrabInfo, RunEv};
    let m = Mock::start(|r: &Req| {
        let user = r.json()["messages"][1]["content"].as_str().unwrap_or("").to_string();
        match user.as_str() {
            "slow" => echo(r).delayed(3000),
            "fail" => Resp::status(404, json!({"error": "model 'm-ai' not found"})),
            "cut" => Resp::ok(json!({"message": {"content": "Halb"}, "done_reason": "length"})),
            // Refine prompt: viermal so lang wie die Eingabe (über dem Ziel, unter der harten Grenze)
            u if u.starts_with("Kurz") => Resp::ok(json!({"message": {"content": "x".repeat(u.chars().count() * 4)}, "done_reason": "stop"})),
            u => Resp::ok(json!({"message": {"content": u.replace("Test", "Versuch")}, "done_reason": "stop"})),
        }
    });
    let mut c = cfg(&m.url);
    c.actions.timeout_ms = 1000;
    let h = Harness::start(c);
    let text = "Das ist ein Test für Größe.";

    // act_grab
    let mut g = grab(&h, text, Some("firefox"));
    let info = GrabInfo::from_json(&g, "Vorschau".into()).expect("grab info");
    assert_eq!(info.id, id_of(&g));
    assert_eq!((info.chars, info.lines, info.cloud, info.replace_ok), (text.chars().count() as u64, 1, false, true));
    assert_eq!((info.host.as_str(), info.model.as_str()), ("127.0.0.1", "m-ai"));
    assert!(info.secret.is_empty());
    let explain = info.actions.iter().find(|a| a.name == "explain").expect("explain");
    assert_eq!((explain.key.as_str(), explain.label.as_str(), explain.output.as_str(), explain.allowed), ("e", "Explain", "answer", true));
    assert!(info.actions.iter().any(|a| a.name == "rephrase" && a.replaces()));
    // `cloud` kommt aus `target`
    g["target"]["cloud"] = json!(true);
    assert!(GrabInfo::from_json(&g, String::new()).unwrap().cloud);
    let id = info.id.clone();

    // Antwort
    let mut r = run(&h, &id, "explain");
    assert_eq!(
        RunEv::from_reply(&mut r),
        RunEv::Result { output: "answer".into(), truncated: false, note: None, text: "Das ist ein Versuch für Größe.".into(), diff: vec![] }
    );
    assert_eq!(r["text"], Value::Null, "the text is taken out, not copied");

    // Ersetzen mit Wortdiff
    let mut r = run(&h, &id, "rephrase");
    let piece = |op, t: &str| DiffPiece { op, text: t.into() };
    assert_eq!(
        RunEv::from_reply(&mut r),
        RunEv::Result {
            output: "replace".into(),
            truncated: false,
            note: None,
            text: "Das ist ein Versuch für Größe.".into(),
            diff: vec![piece('=', "Das ist ein "), piece('-', "Test"), piece('+', "Versuch"), piece('=', " für Größe.")],
        }
    );

    // Hinweis einer wachsenden Aktion
    let id2 = id_of(&grab(&h, "Kurz und knapp", Some("firefox")));
    match RunEv::from_reply(&mut run(&h, &id2, "refine_prompt")) {
        RunEv::Result { note: Some(n), output, .. } => assert!(n.contains("Longer than aimed for") && output == "replace", "{n}"),
        e => panic!("no note: {e:?}"),
    }

    // abgeschnitten
    let id3 = id_of(&grab(&h, "cut", None));
    assert!(matches!(RunEv::from_reply(&mut run(&h, &id3, "explain")), RunEv::Result { truncated: true, .. }));

    // Fehler der KI und unbekannte Auswahl
    let id4 = id_of(&grab(&h, "fail", None));
    match RunEv::from_reply(&mut run(&h, &id4, "explain")) {
        RunEv::Error(e) => assert_eq!(e, "AI error: HTTP 404 (model or address not found)"),
        e => panic!("{e:?}"),
    }
    assert_eq!(RunEv::from_reply(&mut run(&h, "999", "explain")), RunEv::Error("Unknown or expired selection, select again".into()));

    // Abbruch
    let id5 = id_of(&grab(&h, "slow", None));
    let t = run_async(&h, &id5, "explain");
    h.wait_until("request at the mock", || m.requests().iter().any(|r| r.json()["messages"][1]["content"] == "slow"));
    assert_eq!(h.req(&json!({"cmd": "act_cancel", "id": id5}).to_string()), json!({"ok": true}));
    let (mut r, _) = t.join().unwrap();
    assert_eq!(RunEv::from_reply(&mut r), RunEv::Cancelled);
}
