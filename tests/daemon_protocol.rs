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

//! Daemon mit Mock-Session und Mock-Audio: Zustände, Idempotenz, cancel, Subscriber, Fehlerpfade.
mod daemon_common;

use daemon_common::*;
use dictate::config::Config;
use dictate::protocol::{Event, State};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn cfg_no_space() -> Config {
    let mut c = quick_cfg();
    c.typing.trailing_space = false;
    c
}

fn is_final(e: &Event) -> bool {
    matches!(e, Event::Final { .. })
}

#[test]
fn first_event_after_subscribe_is_current_state() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    assert_eq!(s.next_line().unwrap(), r#"{"event":"state","state":"idle"}"#);
    h.cmd("start");
    let mut s2 = h.subscribe();
    assert_eq!(s2.next_line().unwrap(), r#"{"event":"state","state":"listening"}"#);
    h.cmd("cancel");
}

/// Ein Abonnent bekommt nach dem Zustand die Overlay-Einstellungen und die Terminal-Klassen (Vorgaben), dann Zustände,
/// Teiltext mit Umlauten, Pegel und das Ergebnis als JSON; der Strom endet, wenn der Daemon endet. (Bis W7 prüfte das
/// `dictate listen --format awesome`.)
#[test]
fn subscriber_gets_overlay_terminals_and_the_session_as_json() {
    let mut h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    assert_eq!(s.next_line().unwrap(), r#"{"event":"state","state":"idle"}"#);
    assert_eq!(
        s.next_line().unwrap(),
        r#"{"event":"overlay","overlay":{"position":"bottom","max_width":720,"transparency":true,"opacity":0.8,"bottom_margin":48}}"#
    );
    assert_eq!(
        s.next_line().unwrap(),
        r#"{"event":"terminals","classes":["Alacritty","kitty","XTerm","URxvt","st-256color","org.wezfurlong.wezterm","foot","footclient","com.mitchellh.ghostty","org.gnome.Console","org.gnome.Ptyxis","org.gnome.Terminal","org.kde.konsole"]}"#
    );
    h.cmd("start");
    assert_eq!(s.next(), Some(Event::State { state: State::Listening }));
    let (mut seen_partial, mut seen_level) = (false, false);
    while !(seen_partial && seen_level) {
        match s.next().expect("event") {
            Event::Partial { stable, tail } => {
                assert!(stable.starts_with("Wort"), "{stable:?}");
                assert_eq!(tail, "Rest");
                seen_partial = true;
            }
            Event::Level { db, .. } => {
                assert!(db.is_finite());
                seen_level = true;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    h.cmd("stop");
    let final_text = loop {
        if let Event::Final { text, cancelled: false, keys, .. } = s.next().expect("event") {
            assert!(keys.is_empty(), "no keys");
            break text;
        }
    };
    assert!(final_text.starts_with("Grüße samples="), "{final_text:?}");
    assert!(final_text.ends_with(' '), "trailing space");
    assert_eq!(s.next(), Some(Event::State { state: State::Idle }));
    // der Strom endet, wenn der Daemon endet
    h.cmd("shutdown");
    while s.next_line().is_some() {}
    h.join().unwrap();
}

#[test]
fn session_event_sequence_and_trailing_space_off() {
    let h = Harness::start(cfg_no_space());
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    assert!(matches!(s.next().unwrap(), Event::Overlay { .. }), "overlay follows the state event");
    assert!(matches!(s.next().unwrap(), Event::Terminals { .. }), "terminal classes follow the overlay");
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    let ev = s.until(|e| matches!(e, Event::Partial { .. }));
    assert!(matches!(ev[0], Event::State { state: State::Listening }));
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"finalizing"}"#);
    let ev = s.until(is_final);
    match ev.last().unwrap() {
        Event::Final { text, cancelled, audio_ms, raw, keys, .. } => {
            assert!(text.starts_with("Grüße samples=") && !text.ends_with(' '));
            assert_eq!(raw, text, "pipeline is a pass-through for now");
            assert!(keys.is_empty());
            assert!(!cancelled);
            assert!(*audio_ms > 0);
        }
        _ => unreachable!(),
    }
    assert!(matches!(s.next().unwrap(), Event::State { state: State::Idle }));
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0, "microphone closed");
}

#[test]
fn start_and_stop_are_idempotent() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"idle"}"#);
    assert_eq!(h.cmd("cancel"), r#"{"ok":true,"state":"idle"}"#);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 1, "only one recording");
    assert_eq!(h.cmd("status"), r#"{"ok":true,"state":"listening"}"#);
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"finalizing"}"#);
    h.wait_state("idle");
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"idle"}"#);
}

#[test]
fn toggle_starts_and_stops() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    assert_eq!(h.cmd("toggle"), r#"{"ok":true,"state":"listening"}"#);
    assert_eq!(h.cmd("toggle"), r#"{"ok":true,"state":"finalizing"}"#);
    h.wait_state("idle");
}

#[test]
fn cancel_gives_cancelled_final_and_resets() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    h.cmd("start");
    s.until(|e| matches!(e, Event::Partial { .. }));
    h.cmd("cancel");
    let ev = s.until(is_final);
    assert!(matches!(ev.last().unwrap(), Event::Final { text, cancelled: true, .. } if text.is_empty()));
    h.wait_state("idle");
    assert!(h.ctl.resets.load(Ordering::SeqCst) >= 1);
    // danach geht eine neue Sitzung normal
    h.cmd("start");
    h.cmd("stop");
    let ev = s.until(|e| matches!(e, Event::Final { cancelled: false, .. }));
    assert!(ev.iter().any(|e| matches!(e, Event::Final { cancelled: false, .. })));
}

#[test]
fn start_during_finalizing_restarts_after_final() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    h.cmd("start");
    h.cmd("stop");
    // sofort wieder starten, egal ob noch finalizing
    h.cmd("start");
    s.until(is_final);
    h.wait_state("listening");
    h.cmd("cancel");
}

#[test]
fn multiple_subscribers_get_the_same_events() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut subs: Vec<Sub> = (0..3).map(|_| h.subscribe()).collect();
    h.cmd("start");
    h.cmd("stop");
    let mut finals = Vec::new();
    for s in subs.iter_mut() {
        let ev = s.until(is_final);
        finals.push(ev.last().unwrap().clone());
    }
    assert!(finals.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn dead_subscriber_does_not_disturb() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let dead = h.subscribe();
    let mut alive = h.subscribe();
    drop(dead);
    for _ in 0..2 {
        h.cmd("start");
        h.cmd("stop");
        alive.until(is_final);
        h.wait_state("idle");
    }
}

#[test]
fn slow_subscriber_does_not_block_daemon() {
    let ctl = std::sync::Arc::new(Ctl::default());
    ctl.partial_bytes.store(300_000, Ordering::SeqCst);
    let h = Harness::start_with(quick_cfg(), ctl);
    h.wait_state("idle");
    // liest nie
    let _stuck = h.subscribe();
    let mut ok = h.subscribe();
    h.cmd("start");
    let t = Instant::now();
    // während große Partials fließen, antwortet status weiter prompt
    for _ in 0..20 {
        let t0 = Instant::now();
        h.state();
        assert!(t0.elapsed() < Duration::from_millis(500));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(t.elapsed() < Duration::from_secs(5));
    h.cmd("stop");
    // der gesunde Subscriber (liest sofort) bekommt trotzdem das final
    loop {
        match ok.next() {
            Some(Event::Final { .. }) => break,
            Some(_) => {}
            None => panic!("healthy subscriber lost"),
        }
    }
}

#[test]
fn start_during_loading_buffers_audio() {
    let ctl = std::sync::Arc::new(Ctl::default());
    ctl.load_delay_ms.store(700, Ordering::SeqCst);
    let h = Harness::start_with(quick_cfg(), ctl);
    let mut s = h.subscribe();
    assert_eq!(s.next().unwrap(), Event::State { state: State::Loading });
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"loading"}"#);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.state(), "loading");
    h.cmd("stop");
    let ev = s.until(is_final);
    let Event::Final { text, cancelled: false, .. } = ev.last().unwrap() else { panic!("{ev:?}") };
    // ~300 ms Audio bei 10x Echtzeit-Mock sind deutlich mehr als 4800 Samples
    let n: usize = text.trim().rsplit('=').next().unwrap().parse().unwrap();
    assert!(n >= 4800, "buffered samples {n}");
    // Reihenfolge: Laden vor Zuhören ist hier egal, wichtig: kein listening-Event ohne Modell
    assert!(!ev.iter().any(|e| matches!(e, Event::Partial { .. })) || n > 0);
}

#[test]
fn idle_unload_and_reload() {
    let h = Harness::start(Config { idle_unload_s: 1, ..Config::default() });
    h.wait_state("idle");
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 1);
    let t = Instant::now();
    while h.ctl.unloads.load(Ordering::SeqCst) == 0 {
        assert!(t.elapsed() < Duration::from_secs(4), "model not unloaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(h.state(), "idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"loading"}"#);
    h.wait_state("listening");
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 2);
    h.cmd("stop");
    s.until(is_final);
}

#[test]
fn no_unload_while_listening() {
    let h = Harness::start(Config { idle_unload_s: 1, ..Config::default() });
    h.wait_state("idle");
    h.cmd("start");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(h.ctl.unloads.load(Ordering::SeqCst), 0);
    h.cmd("cancel");
}

#[test]
fn load_failure_gives_error_state_and_recovers() {
    let ctl = std::sync::Arc::new(Ctl::default());
    ctl.load_fail.store(true, Ordering::SeqCst);
    let h = Harness::start_with(quick_cfg(), ctl.clone());
    h.wait_state("error");
    let mut s = h.subscribe();
    assert_eq!(s.next().unwrap(), Event::State { state: State::Error });
    ctl.load_fail.store(false, Ordering::SeqCst);
    h.cmd("start");
    h.wait_state("listening");
    h.cmd("stop");
    s.until(|e| matches!(e, Event::Final { cancelled: false, .. }));
}

#[test]
fn audio_open_failure_reports_error() {
    let ctl = std::sync::Arc::new(Ctl::default());
    ctl.audio_fail.store(true, Ordering::SeqCst);
    let h = Harness::start_with(quick_cfg(), ctl.clone());
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    h.cmd("start");
    let ev = s.until(|e| matches!(e, Event::Error { .. }));
    assert!(matches!(ev.last().unwrap(), Event::Error { msg } if msg.contains("audio source")));
    s.until(is_final); // aufgeräumt: final(cancelled)
    h.wait_state("error");
    ctl.audio_fail.store(false, Ordering::SeqCst);
    h.cmd("start");
    h.wait_state("listening");
    h.cmd("cancel");
}

#[test]
fn engine_panic_is_contained() {
    let ctl = std::sync::Arc::new(Ctl::default());
    ctl.tick_panic.store(true, Ordering::SeqCst);
    let h = Harness::start_with(quick_cfg(), ctl.clone());
    h.wait_state("idle");
    let mut s = h.subscribe();
    h.cmd("start");
    let ev = s.until(|e| matches!(e, Event::Error { .. }));
    assert!(matches!(ev.last().unwrap(), Event::Error { msg } if msg.contains("panicked")));
    h.wait_state("error");
    h.cmd("stop"); // keine Aufnahme mehr: idempotent
    ctl.tick_panic.store(false, Ordering::SeqCst);
    h.cmd("start");
    h.wait_state("listening");
    h.cmd("stop");
    s.until(|e| matches!(e, Event::Final { cancelled: false, .. }));
}

#[test]
fn bad_requests_get_errors() {
    use std::io::{BufRead, BufReader};
    let h = Harness::start(quick_cfg());
    let mut c = std::os::unix::net::UnixStream::connect(&h.sock).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut r = BufReader::new(c.try_clone().unwrap());
    for (req, frag) in [("nonsense\n", "bad request"), ("{\"cmd\":\"dance\"}\n", "unknown command")] {
        c.write_all(req.as_bytes()).unwrap();
        let mut l = String::new();
        r.read_line(&mut l).unwrap();
        assert!(l.contains("\"ok\":false") && l.contains(frag), "{l}");
    }
    // Verbindung bleibt benutzbar
    c.write_all(b"{\"cmd\":\"status\"}\n").unwrap();
    let mut l = String::new();
    r.read_line(&mut l).unwrap();
    assert!(l.contains("\"ok\":true"));
}

#[test]
fn single_instance_stale_socket_and_cleanup() {
    use dictate::daemon::{run_at, Deps};
    let mut h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let meta = std::fs::metadata(&h.sock).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let dir_mode = std::fs::metadata(h.sock.parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);
    // zweite Instanz mit demselben Socket scheitert
    let ctl = std::sync::Arc::new(Ctl::default());
    let deps = Deps {
        backend: std::sync::Arc::new(MockBackend(ctl.clone())),
        audio: std::sync::Arc::new(MockAudio(ctl)),
        history: temp_dir().join("history.jsonl"),
        act_ttl: dictate::actions::store::TTL,
        palette_env: None,
        diagnose: dictate::diagnose::service::DiagDeps::unavailable(),
    };
    let err = run_at(quick_cfg(), &h.sock, deps, false).unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    // der erste läuft weiter
    assert_eq!(h.state(), "idle");
    assert!(h.cmd("shutdown").contains(r#""ok":true"#));
    h.join().unwrap();
    assert!(!h.sock.exists(), "socket removed");

    // veralteter Socket (Datei ohne Listener) wird ersetzt
    let dir = temp_dir();
    let sock = dir.join("control.sock");
    drop(std::os::unix::net::UnixListener::bind(&sock).unwrap());
    assert!(sock.exists());
    let ctl = std::sync::Arc::new(Ctl::default());
    let deps = Deps {
        backend: std::sync::Arc::new(MockBackend(ctl.clone())),
        audio: std::sync::Arc::new(MockAudio(ctl)),
        history: temp_dir().join("history.jsonl"),
        act_ttl: dictate::actions::store::TTL,
        palette_env: None,
        diagnose: dictate::diagnose::service::DiagDeps::unavailable(),
    };
    let s2 = sock.clone();
    let t = std::thread::spawn(move || run_at(quick_cfg(), &s2, deps, false));
    let t0 = Instant::now();
    // bis der neue Daemon den alten Socket ersetzt hat und antwortet
    while std::os::unix::net::UnixStream::connect(&sock).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(raw_cmd(&sock, "status").contains(r#""ok":true"#));
    raw_cmd(&sock, "shutdown");
    t.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hanging_audio_thread_does_not_block_core_and_stale_blocks_are_dropped() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    h.ctl.hang_read.store(true, Ordering::SeqCst);
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    std::thread::sleep(Duration::from_millis(100));
    // stop dauert wegen des Abkoppelns knapp 500 ms, aber nicht ewig
    let t = Instant::now();
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"finalizing"}"#);
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    s.until(is_final);
    h.wait_state("idle");
    // zweite Sitzung mit gesundem Audio; der hängende Thread der ersten liefert jetzt seinen Riesenblock
    h.ctl.hang_read.store(false, Ordering::SeqCst);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    h.ctl.release_hang.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(150));
    h.cmd("stop");
    let ev = s.until(is_final);
    match ev.last().unwrap() {
        Event::Final { text, cancelled, .. } => {
            assert!(!cancelled);
            let n: usize = text.trim().rsplit('=').next().unwrap().parse().unwrap();
            assert!(n < 400_000, "stale block leaked into the next session: {text}");
        }
        _ => unreachable!(),
    }
}

#[test]
fn max_listen_stops_automatically_with_final_text() {
    let h = Harness::start(Config { max_listen_s: 1, ..quick_cfg() });
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    let t = Instant::now();
    h.cmd("start");
    let ev = s.until(is_final);
    assert!(t.elapsed() >= Duration::from_millis(900) && t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    match ev.last().unwrap() {
        Event::Final { text, cancelled, .. } => {
            assert!(!cancelled);
            assert!(text.starts_with("Grüße samples="), "{text}");
        }
        _ => unreachable!(),
    }
    h.wait_state("idle");
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0);
}

#[test]
fn max_listen_zero_is_unlimited() {
    let h = Harness::start(Config { max_listen_s: 0, ..quick_cfg() });
    h.wait_state("idle");
    h.cmd("start");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(h.state(), "listening");
    h.cmd("cancel");
}

#[test]
fn repeated_tick_failures_end_the_recording_with_an_error() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    h.ctl.tick_fail.store(true, Ordering::SeqCst);
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    h.cmd("start");
    let ev = s.until(|e| matches!(e, Event::Error { msg } if msg.contains("in a row")));
    assert!(ev.iter().any(|e| matches!(e, Event::Error { msg } if msg.contains("transcription failed:"))));
    s.until(is_final);
    h.wait_state("error");
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0, "microphone closed");
    // danach geht eine neue Aufnahme wieder
    h.ctl.tick_fail.store(false, Ordering::SeqCst);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    h.cmd("cancel");
}

fn read_all_lines(s: &mut std::os::unix::net::UnixStream) -> Vec<String> {
    use std::io::{BufRead, BufReader};
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    BufReader::new(s).lines().map_while(Result::ok).collect()
}

#[test]
fn oversized_requests_get_a_clear_error_and_the_connection_closes() {
    use std::os::unix::net::UnixStream;
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    // 70 KB für einen gewöhnlichen Befehl: zu groß
    let mut s = UnixStream::connect(&h.sock).unwrap();
    writeln!(s, "{{\"cmd\":\"status\",\"pad\":\"{}\"}}", "x".repeat(70 * 1024)).unwrap();
    let lines = read_all_lines(&mut s);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&lines[0]).unwrap(), serde_json::json!({"ok": false, "error": "request too large"}));
    // und der Daemon lebt weiter
    assert_eq!(h.req(r#"{"cmd":"status"}"#)["ok"], true);

    // set_config darf bis 1 MB: 200 KB Vokabular gehen durch
    let terms: Vec<String> = (0..20_000).map(|i| format!("\"term{i:05}\"")).collect();
    let line = format!("{{\"cmd\":\"set_config\",\"config\":{{\"vocabulary\":{{\"terms\":[{}]}}}}}}", terms.join(","));
    assert!(line.len() > 150 * 1024 && line.len() < 1024 * 1024);
    let r = h.req(&line);
    assert_eq!(r["ok"], true, "{r}");

    // über 1 MB auch für set_config zu groß
    let mut s = UnixStream::connect(&h.sock).unwrap();
    let big = format!("{{\"cmd\":\"set_config\",\"config\":{{\"initial_prompt\":\"{}\"}}}}", "y".repeat(1100 * 1024));
    let _ = writeln!(s, "{big}");
    let lines = read_all_lines(&mut s);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("request too large"), "{lines:?}");
}

#[test]
fn oversized_request_on_a_subscriber_connection_closes_it() {
    use std::os::unix::net::UnixStream;
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = UnixStream::connect(&h.sock).unwrap();
    writeln!(s, "{{\"cmd\":\"subscribe\"}}").unwrap();
    writeln!(s, "{{\"cmd\":\"status\",\"pad\":\"{}\"}}", "x".repeat(70 * 1024)).unwrap();
    let lines = read_all_lines(&mut s);
    assert!(lines.iter().any(|l| l.contains("request too large")), "{lines:?}");
}
