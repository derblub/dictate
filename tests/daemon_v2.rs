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

//! Daemon v2 mit Mock-Session und Mock-Audio: Konfiguration, Reload-Arten, overlay-Event, Quellen, Modelle,
//! Verlauf-Stub, `start --app` und die Pegelmessung `meter`.
mod daemon_common;

use daemon_common::*;
use dictate::config::Config;
use dictate::protocol::{Event, State};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;

fn is_overlay(e: &Event) -> bool {
    matches!(e, Event::Overlay { .. })
}

fn is_final(e: &Event) -> bool {
    matches!(e, Event::Final { .. })
}

#[test]
fn get_config_returns_everything_with_defaults() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let r = h.req(r#"{"cmd":"get_config"}"#);
    assert_eq!(r["ok"], true);
    assert_eq!(r["schema"], 2);
    assert_eq!(r["path"], "", "no file in this harness");
    let c = &r["config"];
    assert_eq!(c["language"], "de");
    assert_eq!(c["idle_unload_s"], 0, "the harness config, not the default");
    assert_eq!(c["typing"]["delay_ms"], 12);
    assert_eq!(c["features"]["voice_commands"], true);
    assert_eq!(c["features"]["history"], false);
    assert_eq!(c["overlay"]["max_width"], 720);
    assert_eq!(c["ai"]["modes"]["clean"], dictate::config::PROMPT_CLEAN);
    assert_eq!(c["ai"]["formality"], "auto");
    assert!(c["profiles"].as_array().unwrap().is_empty());
    // der Wert lässt sich als Config lesen und wieder zurückschicken
    let cfg = Config::from_value(c.clone()).unwrap();
    let r = h.req(&json!({"cmd": "set_config", "config": cfg.to_json()}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["reload"], "live");
}

#[test]
fn set_config_validates_writes_atomically_and_streams_overlay() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    let path = h.cfg_path.clone().unwrap();
    assert!(!path.exists());
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    assert!(is_overlay(&sub.next().unwrap()), "overlay right after the state event");
    let Event::Terminals { classes } = sub.next().unwrap() else { panic!("terminals after the overlay") };
    assert!(classes.iter().any(|c| c == "Alacritty"), "{classes:?}");

    // gültig: live, Datei mit 0600, nur Abweichungen
    let r = h.req(r#"{"cmd":"set_config","config":{"overlay":{"position":"top","max_width":900},"typing":{"delay_ms":20,"trailing_space":false}}}"#);
    assert_eq!(r, json!({"ok": true, "reload": "live", "deferred": false}));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(text.contains("max_width = 900") && text.contains("delay_ms = 20"), "{text}");
    assert!(!text.contains("step_ms"), "{text}");
    // overlay-Event an den Subscriber
    let Event::Overlay { overlay } = sub.next().unwrap() else { panic!() };
    assert_eq!((overlay.position.as_str(), overlay.max_width), ("top", 900));
    // get_config zeigt den neuen Stand samt Pfad
    let r = h.req(r#"{"cmd":"get_config"}"#);
    assert_eq!(r["config"]["overlay"]["position"], "top");
    assert_eq!(r["config"]["typing"]["trailing_space"], false);
    assert_eq!(r["path"], path.display().to_string());

    // dasselbe nochmal: keine Änderung, kein overlay-Event
    h.req(r#"{"cmd":"set_config","config":{"overlay":{"position":"top","max_width":900}}}"#);
    h.cmd("start");
    let ev = sub.until(|e| matches!(e, Event::State { state: State::Listening }));
    assert!(!ev.iter().any(is_overlay), "{ev:?}");
    h.cmd("cancel");
    h.wait_state("idle");

    // ungültig: Fehler mit Feldpfad, Datei bleibt, Zustand bleibt
    let before = std::fs::read_to_string(&path).unwrap();
    let r = h.req(r#"{"cmd":"set_config","config":{"ai":{"timeout_ms":5}}}"#);
    assert_eq!(r["ok"], false);
    assert_eq!(r["field"], "ai.timeout_ms");
    assert!(r["error"].as_str().unwrap().contains("ai.timeout_ms"), "{r}");
    let r2 = h.req(r#"{"cmd":"set_config","config":{"bogus":1}}"#);
    assert_eq!((r2["ok"].clone(), r2["field"].clone()), (json!(false), json!("bogus")));
    let r3 = h.req(r#"{"cmd":"set_config","config":{"overlay":{"opacity":"hoch"}}}"#);
    assert_eq!((r3["ok"].clone(), r3["field"].clone()), (json!(false), json!("overlay.opacity")));
    // ein Modell, das der Server nicht anbietet, lehnt erst das Speichern ab (Laden bleibt nachsichtig)
    let r4 =
        h.req(r#"{"cmd":"set_config","config":{"features":{"ai":true},"ai":{"preset":"anthropic","provider":"anthropic","model":"qwen2.5:7b-instruct"}}}"#);
    assert_eq!((r4["ok"].clone(), r4["field"].clone()), (json!(false), json!("ai.model")), "{r4}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert_eq!(h.req(r#"{"cmd":"get_config"}"#)["config"]["ai"]["timeout_ms"], 4000);
    // kein Rest der temporären Datei
    assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn legacy_trailing_space_in_set_config() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"trailing_space":false}}"#)["ok"], true);
    assert_eq!(h.req(r#"{"cmd":"get_config"}"#)["config"]["typing"]["trailing_space"], false);
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    h.cmd("start");
    h.cmd("stop");
    let ev = s.until(is_final);
    let Event::Final { text, .. } = ev.last().unwrap() else { panic!() };
    assert!(!text.ends_with(' '), "{text:?}");
}

#[test]
fn live_changes_apply_without_reloading_the_model() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 1);
    let r = h.req(r#"{"cmd":"set_config","config":{"step_ms":300,"source":"alsa_input.other","features":{"history":true}}}"#);
    assert_eq!(r["reload"], "live");
    assert_eq!(h.state(), "idle");
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 1, "no model reload");
    assert_eq!(h.ctl.unloads.load(Ordering::SeqCst), 0);
}

#[test]
fn model_changes_reload_in_the_background() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    h.ctl.load_delay_ms.store(150, Ordering::SeqCst);
    for (body, n) in [(r#"{"language":"en"}"#, 2), (r#"{"threads":2}"#, 3), (r#"{"vad_model":"/x/vad.bin"}"#, 4), (r#"{"model":"/x/m.bin"}"#, 5)] {
        let r = h.req(&format!(r#"{{"cmd":"set_config","config":{body}}}"#));
        assert_eq!(r["reload"], "model", "{body}");
        assert_eq!(r["deferred"], false);
        sub.until_state(State::Loading);
        sub.until_state(State::Idle);
        assert_eq!(h.ctl.loads.load(Ordering::SeqCst), n, "{body}");
    }
    assert_eq!(h.ctl.unloads.load(Ordering::SeqCst), 4, "the old model is released before loading");
}

#[test]
fn reload_waits_for_the_end_of_the_session() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    assert!(is_overlay(&sub.next().unwrap()));
    h.cmd("start");
    h.wait_state("listening");
    let r = h.req(r#"{"cmd":"set_config","config":{"threads":2,"overlay":{"max_width":640}}}"#);
    assert_eq!((r["reload"].clone(), r["deferred"].clone()), (json!("model"), json!(true)));
    // gespeichert und per get_config sichtbar, das Overlay geht sofort raus, das Modell wird nicht angefasst
    assert_eq!(h.req(r#"{"cmd":"get_config"}"#)["config"]["threads"], 2);
    let ev = sub.until(is_overlay);
    assert!(matches!(ev.last().unwrap(), Event::Overlay { overlay } if overlay.max_width == 640));
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 1);
    assert_eq!(h.ctl.unloads.load(Ordering::SeqCst), 0);
    assert_eq!(h.state(), "listening");
    // nach dem Ende der Sitzung wird neu geladen
    h.cmd("stop");
    let ev = sub.until_state(State::Loading);
    assert!(ev.iter().any(is_final), "final comes first: {ev:?}");
    sub.until_state(State::Idle);
    assert_eq!(h.ctl.loads.load(Ordering::SeqCst), 2);
    assert_eq!(h.ctl.unloads.load(Ordering::SeqCst), 1);
}

#[test]
fn reload_command_rereads_the_file() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    // ohne Datei: Defaults
    let r = h.req(r#"{"cmd":"reload"}"#);
    assert_eq!(r["ok"], true);
    let path = h.cfg_path.clone().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# mine\n[overlay]\nopacity = 0.3\n").unwrap();
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    sub.next(); // overlay mit dem alten Stand
    sub.next(); // terminals
    let r = h.req(r#"{"cmd":"reload"}"#);
    assert_eq!(r["reload"], "live");
    let Event::Overlay { overlay } = sub.next().unwrap() else { panic!() };
    assert_eq!(overlay.opacity, 0.3);
    assert_eq!(h.req(r#"{"cmd":"get_config"}"#)["config"]["overlay"]["opacity"], 0.3);
    // kaputte Datei: Fehler mit Feld, alter Stand bleibt
    std::fs::write(&path, "[ai]\ntimeout_ms = 1\n").unwrap();
    let r = h.req(r#"{"cmd":"reload"}"#);
    assert_eq!((r["ok"].clone(), r["field"].clone()), (json!(false), json!("ai.timeout_ms")));
    assert_eq!(h.req(r#"{"cmd":"get_config"}"#)["config"]["overlay"]["opacity"], 0.3);
    // ohne Datei im Betrieb gibt es nichts zu laden
    let h2 = Harness::start(quick_cfg());
    h2.wait_state("idle");
    assert_eq!(h2.req(r#"{"cmd":"reload"}"#)["ok"], false);
}

#[test]
fn sources_models_history_and_ai_stubs() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let r = h.req(r#"{"cmd":"list_sources"}"#);
    assert_eq!(r["ok"], true);
    assert_eq!(r["sources"][0], json!({"name": "alsa_input.mock", "description": "Mock Microphone", "default": true}));
    assert_eq!(r["sources"][1]["default"], false);

    let r = h.req(r#"{"cmd":"list_models"}"#);
    assert_eq!(r["ok"], true);
    assert!(r["installed"].is_array());
    let ids: Vec<&str> = r["catalog"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    for want in ["large-v3-turbo-q5_0", "large-v3-turbo", "small", "medium-q5_0", "silero-v5.1.2"] {
        assert!(ids.contains(&want), "{ids:?}");
    }
    let e = &r["catalog"][0];
    assert!(e["url"].as_str().unwrap().starts_with("https://huggingface.co/"));
    assert_eq!(e["sha256"].as_str().unwrap().len(), 64);
    assert!(e["size"].as_u64().unwrap() > 0);

    // Verlauf: Feature aus (Default) → leere Liste; Stub bis zur Pipeline
    assert_eq!(h.req(r#"{"cmd":"history","limit":10}"#), json!({"ok": true, "entries": []}));
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));
    assert_eq!(h.req(r#"{"cmd":"history_clear"}"#), json!({"ok": true}));
    h.req(r#"{"cmd":"set_config","config":{"features":{"history":true}}}"#);
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));

    // Nichts hört auf Port 1: Fehlertext statt Absturz (die Mock-Tests stehen in daemon_ai.rs)
    h.req(r#"{"cmd":"set_config","config":{"ai":{"endpoint":"http://127.0.0.1:1"}}}"#);
    let r = h.req(r#"{"cmd":"ai_models"}"#);
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("cannot reach server"), "{r}");

    // Fehlerhafte Parameter
    assert_eq!(h.req(r#"{"cmd":"meter"}"#)["ok"], false);
    assert_eq!(h.req(r#"{"cmd":"set_config"}"#)["ok"], false);
}

#[test]
fn start_accepts_an_app() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    assert_eq!(h.req(r#"{"cmd":"start","app":"Alacritty"}"#), json!({"ok": true, "state": "listening"}));
    // zweites start (auch mit anderer App) ändert nichts
    assert_eq!(h.req(r#"{"cmd":"start","app":"kitty"}"#)["state"], "listening");
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 1);
    h.cmd("stop");
    // start während finalizing: nahtlos neue Sitzung mit eigener App
    let ev = s.until(is_final);
    assert!(matches!(ev.last().unwrap(), Event::Final { cancelled: false, .. }));
    assert_eq!(h.req(r#"{"cmd":"start","app":"XTerm"}"#)["ok"], true);
    h.cmd("cancel");
    h.wait_state("idle");
}

fn levels_until(s: &mut Sub, n: usize) {
    let got = std::cell::Cell::new(0);
    s.until(|e| {
        if matches!(e, Event::Level { .. }) {
            got.set(got.get() + 1);
        }
        got.get() >= n
    });
}

#[test]
fn meter_opens_the_microphone_only_for_the_level() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0);
    assert_eq!(h.req(r#"{"cmd":"meter","on":true}"#)["ok"], true);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 1);
    levels_until(&mut sub, 3);
    assert_eq!(h.state(), "idle", "metering is not listening");
    // zweites on: dieselbe Messaufnahme
    h.req(r#"{"cmd":"meter","on":true}"#);
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 1);
    h.req(r#"{"cmd":"meter","on":false}"#);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0);
    // off ist idempotent und öffnet nichts
    h.req(r#"{"cmd":"meter","on":false}"#);
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 1);
}

#[test]
fn meter_never_disturbs_dictation() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    h.req(r#"{"cmd":"meter","on":true}"#);
    h.wait_until("meter recording", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1);
    // Diktat starten: sofort Listening, nur noch eine offene Aufnahme (die des Diktats)
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    h.wait_until("one recording", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1 && h.ctl.opens.load(Ordering::SeqCst) == 2);
    // Pegel kommen weiter, jetzt aus der Aufnahme, und das Diktat liefert wie gewohnt Zwischenstände
    let ev = sub.until(|e| matches!(e, Event::Partial { .. }));
    assert!(ev.iter().any(|e| matches!(e, Event::State { state: State::Listening })));
    levels_until(&mut sub, 3);
    assert_eq!(h.cmd("stop"), r#"{"ok":true,"state":"finalizing"}"#);
    let ev = sub.until(is_final);
    let Event::Final { text, cancelled, .. } = ev.last().unwrap() else { panic!() };
    assert!(!cancelled && text.starts_with("Grüße samples="), "{text:?}");
    sub.until_state(State::Idle);
    // die Messung läuft danach weiter
    h.wait_until("meter reopened", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1);
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 3);
    levels_until(&mut sub, 2);
    // Abbruch mitten im Diktat ebenso
    h.cmd("start");
    h.cmd("cancel");
    h.wait_state("idle");
    h.wait_until("meter reopened after cancel", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1 && h.ctl.opens.load(Ordering::SeqCst) == 5);
    h.req(r#"{"cmd":"meter","on":false}"#);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0);
}

#[test]
fn meter_during_listening_opens_no_second_microphone() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    h.cmd("start");
    h.wait_state("listening");
    assert_eq!(h.req(r#"{"cmd":"meter","on":true}"#)["ok"], true);
    assert_eq!(h.ctl.opens.load(Ordering::SeqCst), 1);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 1);
    h.cmd("stop");
    h.wait_until("meter reopened", || h.ctl.opens.load(Ordering::SeqCst) == 2);
    h.req(r#"{"cmd":"meter","on":false}"#);
}

#[test]
fn meter_ends_when_its_subscriber_connection_closes() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut owner = h.subscribe();
    owner.until_state(State::Idle);
    // Anfragen über die Subscriber-Verbindung werden beantwortet; die Antwort steht zwischen den Events
    owner.send(r#"{"cmd":"status"}"#);
    assert_eq!(owner.reply()["state"], "idle");
    owner.send(r#"{"cmd":"meter","on":true}"#);
    assert_eq!(owner.reply()["ok"], true);
    h.wait_until("meter recording", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1);
    levels_until(&mut owner, 2);
    drop(owner);
    h.wait_until("meter closed with the connection", || h.ctl.live_recordings.load(Ordering::SeqCst) == 0);
    // eine Messung über eine kurze Befehlsverbindung gehört keiner Verbindung und bleibt bis off
    h.req(r#"{"cmd":"meter","on":true}"#);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 1);
    h.req(r#"{"cmd":"meter","on":false}"#);
    assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 0);
}

#[test]
fn meter_reports_a_failing_microphone() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    h.ctl.audio_fail.store(true, Ordering::SeqCst);
    assert_eq!(h.req(r#"{"cmd":"meter","on":true}"#)["ok"], true);
    let ev = sub.until(|e| matches!(e, Event::Error { .. }));
    let Event::Error { msg } = ev.last().unwrap() else { panic!() };
    assert!(msg.contains("cannot open audio source"), "{msg}");
    h.wait_until("meter off", || h.ctl.live_recordings.load(Ordering::SeqCst) == 0);
    assert_eq!(h.state(), "idle", "a failing meter is not an error state");
    // Dictation geht danach normal
    h.ctl.audio_fail.store(false, Ordering::SeqCst);
    assert_eq!(h.cmd("start"), r#"{"ok":true,"state":"listening"}"#);
    h.cmd("cancel");
}

/// Geänderte `actions.terminal_classes` gehen sofort als `terminals`-Event an die Subscriber (`dictate-ui` schreibt
/// dort Zeilenumbrüche als Return); unveränderte lösen keins aus.
#[test]
fn set_config_streams_terminal_classes() {
    let h = Harness::start_with_file(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until(|e| matches!(e, Event::Terminals { .. }));
    let r = h.req(r#"{"cmd":"set_config","config":{"actions":{"terminal_classes":["Foot","MyTerm"]}}}"#);
    assert_eq!(r["ok"], true, "{r}");
    let ev = sub.until(|e| matches!(e, Event::Terminals { .. }));
    assert!(matches!(ev.last().unwrap(), Event::Terminals { classes } if classes == &["Foot", "MyTerm"]), "{ev:?}");
    h.req(r#"{"cmd":"set_config","config":{"actions":{"terminal_classes":["Foot","MyTerm"]}}}"#);
    h.req(r#"{"cmd":"set_config","config":{"overlay":{"max_width":640}}}"#);
    let ev = sub.until(is_overlay);
    assert!(!ev.iter().any(|e| matches!(e, Event::Terminals { .. })), "{ev:?}");
}
