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

//! Pipeline im Daemon (Mock-Session): Sprachbefehle und `keys`/„rückgängig“, Profilwahl per `app` samt Sprache und
//! Leerzeichen, Verlauf an/aus, `history_clear` und der Befehl `commands`.
mod daemon_common;

use daemon_common::*;
use dictate::config::{Config, ProfileCfg};
use dictate::protocol::{Event, State};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

/// Ein Diktat: `finish` liefert `raw`; Ergebnis (text, keys, raw) aus dem `final`-Event.
fn dictate(h: &Harness, s: &mut Sub, app: Option<&str>, raw: &str) -> (String, Vec<String>, String) {
    *h.ctl.finish_text.lock().unwrap() = Some(raw.to_string());
    let r = match app {
        Some(a) => h.req(&json!({"cmd": "start", "app": a}).to_string()),
        None => h.req(r#"{"cmd":"start"}"#),
    };
    assert_eq!(r["ok"], true, "{r}");
    h.cmd("stop");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    s.until_state(State::Idle);
    match ev.last().unwrap().clone() {
        Event::Final { text, keys, raw, cancelled: false, .. } => (text, keys, raw),
        e => panic!("unexpected {e:?}"),
    }
}

fn ready(cfg: Config) -> (Harness, Sub) {
    let h = Harness::start(cfg);
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);
    (h, s)
}

fn terminal() -> ProfileCfg {
    ProfileCfg {
        name: "terminal".into(),
        match_class: vec!["Alacritty".into(), "kitty".into()],
        punctuation: "strip".into(),
        capitalize_first: false,
        trailing_space: false,
        language: "en".into(),
        ..ProfileCfg::default()
    }
}

#[test]
fn commands_and_undo_keys() {
    let (h, mut s) = ready(quick_cfg());
    let (text, keys, raw) = dictate(&h, &mut s, None, "Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß");
    assert_eq!(text, "Hallo, wie geht's?\n\nGruß ");
    assert!(keys.is_empty());
    assert_eq!(raw, "Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß");

    // „rückgängig“ löscht das vorige Diktat samt Leerzeichen
    let (text, keys, _) = dictate(&h, &mut s, None, "Rückgängig.");
    assert_eq!(text, "");
    assert_eq!(keys, vec!["BackSpace".to_string(); "Hallo, wie geht's?\n\nGruß ".chars().count()]);
    // ein zweites Mal gibt es nichts mehr zu löschen
    let (_, keys, _) = dictate(&h, &mut s, None, "undo that");
    assert!(keys.is_empty());

    // abgebrochene Sitzungen ändern die gemerkte Länge nicht
    dictate(&h, &mut s, None, "Kurz.");
    *h.ctl.finish_text.lock().unwrap() = Some("Viel längerer Text, der verworfen wird.".into());
    h.cmd("start");
    h.cmd("cancel");
    s.until(|e| matches!(e, Event::Final { cancelled: true, .. }));
    s.until_state(State::Idle);
    let (_, keys, _) = dictate(&h, &mut s, None, "rückgängig");
    assert_eq!(keys.len(), "Kurz. ".len());
}

#[test]
fn voice_commands_can_be_switched_off() {
    let mut cfg = quick_cfg();
    cfg.features.voice_commands = false;
    let (h, mut s) = ready(cfg);
    let (text, keys, _) = dictate(&h, &mut s, None, "Hallo Komma Welt");
    assert_eq!((text.as_str(), keys.len()), ("Hallo Komma Welt ", 0));
    let (text, keys, _) = dictate(&h, &mut s, None, "rückgängig");
    assert_eq!((text.as_str(), keys.len()), ("rückgängig ", 0));
}

#[test]
fn profile_by_app_with_language_and_trailing_space() {
    let mut cfg = quick_cfg();
    cfg.profiles = vec![terminal(), ProfileCfg { name: "default".into(), capitalize_first: true, ..ProfileCfg::default() }];
    cfg.typing.trailing_space = true;
    let (h, mut s) = ready(cfg);

    let (text, _, _) = dictate(&h, &mut s, Some("Alacritty"), "Git commit, minus m Version v1.2.");
    assert_eq!(text, "git commit minus m Version v1.2");
    assert_eq!(*h.ctl.language.lock().unwrap(), Some(Some("en".to_string())));

    // anderes Fenster: Profil „default“, Sprache aus der Konfiguration, Leerzeichen aus dem Profil
    let (text, _, _) = dictate(&h, &mut s, Some("firefox"), "hallo Welt.");
    assert_eq!(text, "Hallo Welt. ");
    assert_eq!(*h.ctl.language.lock().unwrap(), Some(None));

    // Profil-Schalter aus: kein Profil
    h.req(r#"{"cmd":"set_config","config":{"features":{"profiles":false}}}"#);
    let (text, _, _) = dictate(&h, &mut s, Some("kitty"), "Git status.");
    assert_eq!(text, "Git status. ");
    assert_eq!(*h.ctl.language.lock().unwrap(), Some(None));
}

#[test]
fn history_on_off_and_clear() {
    let (h, mut s) = ready(quick_cfg());
    // aus (Default): nichts geschrieben
    dictate(&h, &mut s, Some("kitty"), "Geheim.");
    assert!(!h.history_path().exists());
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));

    let r = h.req(r#"{"cmd":"set_config","config":{"features":{"history":true},"history":{"max_entries":2}}}"#);
    assert_eq!(r["ok"], true, "{r}");
    dictate(&h, &mut s, Some("kitty"), "Eins Punkt");
    dictate(&h, &mut s, None, "Zwei.");
    dictate(&h, &mut s, None, "Drei Komma vier.");
    // leere Ergebnisse und „rückgängig“ landen nicht im Verlauf
    dictate(&h, &mut s, None, "alles löschen");
    dictate(&h, &mut s, None, "rückgängig");
    let mode = std::fs::metadata(h.history_path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let r = h.req(r#"{"cmd":"history","limit":10}"#);
    let e = r["entries"].as_array().unwrap();
    assert_eq!(e.len(), 2, "{r}");
    assert_eq!(e[0]["text"], "Drei, vier.");
    assert_eq!(e[0]["raw"], "Drei Komma vier.");
    assert_eq!(e[0]["app"], "");
    assert_eq!(e[1]["text"], "Zwei.");
    assert!(e[0]["ts"].as_str().unwrap().ends_with('Z'));
    assert_eq!(h.req(r#"{"cmd":"history","limit":1}"#)["entries"].as_array().unwrap().len(), 1);

    assert_eq!(h.req(r#"{"cmd":"history_clear"}"#), json!({"ok": true}));
    assert!(!h.history_path().exists());
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));

    // wieder aus: Datei bleibt unangetastet, Liste leer
    dictate(&h, &mut s, Some("kitty"), "Neu.");
    assert_eq!(h.req(r#"{"cmd":"history"}"#)["entries"][0]["app"], "kitty");
    h.req(r#"{"cmd":"set_config","config":{"features":{"history":false}}}"#);
    dictate(&h, &mut s, None, "Nicht gespeichert.");
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));
    let content = std::fs::read_to_string(h.history_path()).unwrap();
    assert_eq!(content.lines().count(), 1);
    assert!(!content.contains("Nicht gespeichert"));
}

#[test]
fn commands_table_command() {
    let (h, _s) = ready(quick_cfg());
    let r = h.req(r#"{"cmd":"commands"}"#);
    assert_eq!(r["ok"], true);
    let c = r["commands"].as_array().unwrap();
    assert_eq!(c[0], json!({"de": "neue Zeile", "en": "new line", "effect": "Line break"}));
    assert!(c.iter().any(|e| e["en"] == "undo that"));
}

#[test]
fn undo_only_works_in_the_same_app_and_without_history_it_reports() {
    let (h, mut s) = ready(quick_cfg());
    // ohne vorheriges Diktat: nichts getippt, kurzer Fehler
    *h.ctl.finish_text.lock().unwrap() = Some("undo that".into());
    h.req(r#"{"cmd":"start"}"#);
    h.cmd("stop");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    s.until_state(State::Idle);
    assert!(ev.iter().any(|e| matches!(e, Event::Error { msg } if msg == "Nothing to undo")), "{ev:?}");
    match ev.last().unwrap() {
        Event::Final { text, keys, .. } => assert!(text.is_empty() && keys.is_empty()),
        e => panic!("{e:?}"),
    }
    // Diktat in kitty, „rückgängig“ in firefox: wird als Wort getippt, nichts gelöscht
    dictate(&h, &mut s, Some("kitty"), "Hallo.");
    let (text, keys, _) = dictate(&h, &mut s, Some("firefox"), "rückgängig");
    assert_eq!((text.as_str(), keys.len()), ("rückgängig ", 0));
    // das Wort ist jetzt das letzte Diktat (firefox): dort wirkt „rückgängig“ wieder, auf genau dieses Wort
    let (text, keys, _) = dictate(&h, &mut s, Some("firefox"), "rückgängig");
    assert_eq!((text.as_str(), keys.len()), ("", "rückgängig ".chars().count()));
}

#[test]
fn undo_is_limited_to_2000_keys() {
    let (h, mut s) = ready(quick_cfg());
    dictate(&h, &mut s, None, &"a".repeat(2500));
    let (_, keys, _) = dictate(&h, &mut s, None, "rückgängig");
    assert_eq!(keys.len(), 2000);
}

/// Diktat in die Eingabezeile der Befehlspalette (`target: palette`, PLAN-palette.md Aufgaben 8/9): das final geht
/// wie immer an die Abonnenten, wird aber nicht in den Verlauf geschrieben und gilt nicht als getippt (ein folgendes
/// „rückgängig“ im Fenster löscht nichts). Ein Diktat ins Fenster danach landet wieder im Verlauf.
#[test]
fn palette_dictation_is_not_kept_in_the_history() {
    let (h, mut s) = ready(quick_cfg());
    assert_eq!(h.req(r#"{"cmd":"set_config","config":{"features":{"history":true}}}"#)["ok"], true);
    let palette = |s: &mut Sub, line: &str, raw: &str| {
        *h.ctl.finish_text.lock().unwrap() = Some(raw.to_string());
        assert_eq!(h.req(line)["ok"], true);
        h.cmd("stop");
        let ev = s.until(|e| matches!(e, Event::Final { .. }));
        s.until_state(State::Idle);
        match ev.last().unwrap().clone() {
            Event::Final { text, cancelled: false, .. } => text,
            e => panic!("unexpected {e:?}"),
        }
    };
    assert_eq!(palette(&mut s, r#"{"cmd":"start","app":"kitty","target":"palette"}"#, "Firefox öffnen."), "Firefox öffnen. ");
    assert_eq!(palette(&mut s, r#"{"cmd":"toggle","target":"palette"}"#, "Lauter."), "Lauter. ");
    assert!(!h.history_path().exists(), "palette input is never stored");
    assert_eq!(h.req(r#"{"cmd":"history"}"#), json!({"ok": true, "entries": []}));
    // nichts getippt: „rückgängig“ hat nichts zu löschen
    let (text, keys, _) = dictate(&h, &mut s, Some("kitty"), "rückgängig");
    assert_eq!((text.as_str(), keys.len()), ("", 0));
    // Diktat ins Fenster: wie bisher im Verlauf
    dictate(&h, &mut s, Some("kitty"), "Ins Fenster.");
    let e = h.req(r#"{"cmd":"history"}"#)["entries"].as_array().unwrap().clone();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0]["text"], "Ins Fenster.");
}
