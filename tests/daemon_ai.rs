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

//! KI im Daemon (Mock-Session + HTTP-Mock): `final` mit KI-Text, Timeout mit Rohtext und `error`-Event,
//! Zustand `finalizing` während der Anfrage sowie `ai_test` und `ai_models` über den Socket.
mod daemon_common;
mod mock_http;

use daemon_common::*;
use dictate::config::Config;
use dictate::protocol::{Event, State};
use mock_http::*;
use serde_json::json;

fn cfg(url: &str, timeout_ms: u64) -> Config {
    let mut c = quick_cfg();
    c.features.ai = true;
    c.ai.endpoint = url.into();
    c.ai.timeout_ms = timeout_ms;
    c.ai.default_mode = "clean".into();
    c
}

fn ready(c: Config) -> (Harness, Sub) {
    let h = Harness::start(c);
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    (h, s)
}

fn dictate(h: &Harness, s: &mut Sub, raw: &str) -> Vec<Event> {
    *h.ctl.finish_text.lock().unwrap() = Some(raw.to_string());
    assert_eq!(h.req(r#"{"cmd":"start"}"#)["ok"], true);
    h.cmd("stop");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    s.until_state(State::Idle);
    ev
}

fn final_text(ev: &[Event]) -> String {
    match ev.last().unwrap() {
        Event::Final { text, .. } => text.clone(),
        e => panic!("{e:?}"),
    }
}

fn errors(ev: &[Event]) -> Vec<String> {
    ev.iter().filter_map(|e| if let Event::Error { msg } = e { Some(msg.clone()) } else { None }).collect()
}

#[test]
fn final_carries_ai_text_and_state_stays_finalizing() {
    let m = Mock::start(|_| Resp::ok(json!({"message": {"content": "Hallo, Welt."}})).delayed(500));
    let (h, mut s) = ready(cfg(&m.url, 3000));
    *h.ctl.finish_text.lock().unwrap() = Some("hallo welt".into());
    h.req(r#"{"cmd":"start"}"#);
    h.cmd("stop");
    h.wait_state("finalizing");
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(h.state(), "finalizing", "still waiting for the model");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    assert!(errors(&ev).is_empty(), "{ev:?}");
    match ev.last().unwrap() {
        Event::Final { text, raw, .. } => assert_eq!((text.as_str(), raw.as_str()), ("Hallo, Welt. ", "hallo welt")),
        e => panic!("{e:?}"),
    }
    assert_eq!(m.requests().len(), 1);
}

#[test]
fn timeout_types_raw_text_and_reports_error() {
    let m = Mock::start(|_| Resp::ok(json!({"message": {"content": "zu spät"}})).delayed(3000));
    let (h, mut s) = ready(cfg(&m.url, 300));
    let ev = dictate(&h, &mut s, "hallo welt");
    assert_eq!(errors(&ev), ["AI timeout, raw text typed"]);
    assert_eq!(final_text(&ev), "hallo welt ");
}

#[test]
fn server_error_keeps_raw_text() {
    let m = Mock::start(|_| Resp::status(404, json!({"error": "model 'm' not found"})));
    let (h, mut s) = ready(cfg(&m.url, 2000));
    let ev = dictate(&h, &mut s, "hallo welt");
    let e = errors(&ev);
    assert_eq!(e.len(), 1);
    assert!(e[0].starts_with("AI error: ") && e[0].contains("not found"), "{e:?}");
    assert_eq!(final_text(&ev), "hallo welt ");
}

#[test]
fn ai_off_or_no_mode_sends_nothing() {
    let m = Mock::start(|_| Resp::ok(json!({"message": {"content": "X"}})));
    let mut c = cfg(&m.url, 2000);
    c.features.ai = false;
    let (h, mut s) = ready(c);
    assert_eq!(final_text(&dictate(&h, &mut s, "hallo")), "hallo ");
    let mut c = cfg(&m.url, 2000);
    c.ai.default_mode.clear();
    let (h, mut s) = ready(c);
    assert_eq!(final_text(&dictate(&h, &mut s, "hallo")), "hallo ");
    assert!(m.requests().is_empty());
}

#[test]
fn ai_test_and_ai_models_over_the_socket() {
    let m = Mock::start(|r| match r.path.as_str() {
        "/api/tags" => Resp::ok(json!({"models": [{"name": "qwen2.5:7b-instruct"}]})),
        _ => Resp::ok(json!({"message": {"content": "\"Hallo, Welt.\""}})),
    });
    // KI-Schalter aus: Test und Modellliste funktionieren trotzdem
    let mut c = cfg(&m.url, 2000);
    c.features.ai = false;
    let (h, _s) = ready(c);
    let r = h.req(r#"{"cmd":"ai_test","mode":"clean","text":"hallo welt"}"#);
    assert_eq!(r, json!({"ok": true, "text": "Hallo, Welt."}));
    let r = h.req(r#"{"cmd":"ai_models"}"#);
    assert_eq!(r, json!({"ok": true, "models": [{"name": "qwen2.5:7b-instruct"}]}));
    let r = h.req(r#"{"cmd":"ai_test","mode":"nope","text":"x"}"#);
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("unknown mode"), "{r}");
    assert_eq!(h.req(r#"{"cmd":"ai_test","mode":"clean"}"#)["ok"], false);
}

#[test]
fn slow_ai_request_does_not_block_the_core() {
    let m = Mock::start(|_| Resp::ok(json!({"message": {"content": "ok"}})).delayed(800));
    let (h, _s) = ready(cfg(&m.url, 3000));
    let sock = h.sock.clone();
    let t = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut c = std::os::unix::net::UnixStream::connect(sock).unwrap();
        writeln!(c, r#"{{"cmd":"ai_test","mode":"clean","text":"hi"}}"#).unwrap();
        let mut l = String::new();
        BufReader::new(c).read_line(&mut l).unwrap();
        l
    });
    std::thread::sleep(std::time::Duration::from_millis(150));
    let t0 = std::time::Instant::now();
    assert_eq!(h.state(), "idle");
    assert!(t0.elapsed() < std::time::Duration::from_millis(300));
    assert!(t.join().unwrap().contains("\"ok\":true"));
}

#[test]
fn cancel_during_the_ai_call_returns_without_waiting_for_the_timeout() {
    // Der Mock antwortet erst nach 8 s, das Timeout ist 30 s: ohne Abbruch-Flag bliebe der Daemon so lange in finalizing.
    let n = std::sync::atomic::AtomicUsize::new(0);
    let m = Mock::start(move |_| {
        let first = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        Resp::ok(json!({"message": {"content": if first { "Hallo." } else { "Zweites." }}})).delayed(if first { 8000 } else { 0 })
    });
    let (h, mut s) = ready(cfg(&m.url, 30_000));
    *h.ctl.finish_text.lock().unwrap() = Some("hallo".into());
    h.req(r#"{"cmd":"start"}"#);
    h.cmd("stop");
    h.wait_state("finalizing");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let t = std::time::Instant::now();
    h.cmd("cancel");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    assert!(matches!(ev.last().unwrap(), Event::Final { cancelled: true, .. }), "{ev:?}");
    s.until_state(State::Idle);
    assert!(t.elapsed() < std::time::Duration::from_secs(3), "{:?}", t.elapsed());
    assert!(errors(&ev).is_empty(), "a user cancel is no error: {ev:?}");
    // der Daemon nimmt gleich das nächste Diktat an (Flag wurde zurückgesetzt)
    let ev = dictate(&h, &mut s, "zweites");
    assert_eq!(final_text(&ev).trim(), "Zweites.");
}
