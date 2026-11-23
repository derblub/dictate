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

//! Spiegeltests zwischen Daemon und `dictate-ui` (PLAN-wayland.md 6g, Block B): `dictate-ui` bindet nur `ui` ein und
//! kennt `palette` nicht, darum stehen Drahtformat und Grenzen dort ein zweites Mal (`ui::pal_exec`,
//! `ui::pal_context`). Diese Tests laufen nur in der Bibliothek (sie brauchen beide Seiten) und halten die Spiegel
//! gleich: jede Absicht und jedes Ziel, das der Daemon schreibt, liest der Ausführer mit demselben Namen; Pflicht zur
//! Bestätigung wie im Katalog; gleiche Grenzen; jeder gebaute Kontext besteht `Context::from_json`, auch im
//! schlimmsten Fall, und passt mit Eingabe und Umgebung in eine Zeile von `pal_resolve`.

use super::catalog::{Class, CATALOG};
use super::intent::{self, ColumnDisplay, DiagProfile, MediaKey, ScreenshotMode, SettingsPage, Widget, WindowToggle};
use super::resolve::{self, Candidate, Context, Target};
use crate::ui::pal_context::{self as ui_ctx, NiriWindow, NiriWorkspace, Own};
use crate::ui::pal_exec as ui;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Je Absicht mindestens ein Beispiel, dazu jeder Wert jeder Auswahl
fn examples() -> Vec<String> {
    let mut out: Vec<String> = [
        r#"{"intent":"view_workspace","workspace":3}"#,
        r#"{"intent":"view_workspace","workspace":"web"}"#,
        r#"{"intent":"focus_window","query":"firefox"}"#,
        r#"{"intent":"move_window_to_workspace","workspace":2}"#,
        r#"{"intent":"move_window_to_workspace","workspace":"mail","query":"thunderbird"}"#,
        r#"{"intent":"close_window"}"#,
        r#"{"intent":"close_window","query":"mpv"}"#,
        r#"{"intent":"previous_workspace"}"#,
        r#"{"intent":"launch_app","query":"firefox"}"#,
        r#"{"intent":"open_project","query":"dictate"}"#,
        r#"{"intent":"open_folder","query":"dictate"}"#,
        r#"{"intent":"open_url","url":"https://example.org/a?b=1"}"#,
        r#"{"intent":"web_search","query":"rust serde"}"#,
        r#"{"intent":"lock"}"#,
        r#"{"intent":"volume","set":40}"#,
        r#"{"intent":"volume","step":-5}"#,
        r#"{"intent":"volume","mute":true}"#,
        r#"{"intent":"dictate_settings"}"#,
        r#"{"intent":"logout"}"#,
        r#"{"intent":"reboot"}"#,
        r#"{"intent":"poweroff"}"#,
        r#"{"intent":"suspend"}"#,
        r#"{"intent":"ask","question":"why is the sky blue?"}"#,
        r#"{"intent":"explain_clipboard"}"#,
        r#"{"intent":"diagnose"}"#,
        r#"{"intent":"diagnose","profile":"unit","unit":"nginx"}"#,
        r#"{"intent":"diagnose","profile":"docker","container":"db"}"#,
    ]
    .map(String::from)
    .to_vec();
    out.extend(ColumnDisplay::NAMES.iter().map(|n| format!(r#"{{"intent":"column_display","display":"{n}"}}"#)));
    out.extend(WindowToggle::NAMES.iter().map(|n| format!(r#"{{"intent":"toggle_window","what":"{n}"}}"#)));
    out.extend(MediaKey::NAMES.iter().map(|n| format!(r#"{{"intent":"media","action":"{n}"}}"#)));
    out.extend(Widget::NAMES.iter().map(|n| format!(r#"{{"intent":"show_widget","widget":"{n}"}}"#)));
    out.extend(ScreenshotMode::NAMES.iter().map(|n| format!(r#"{{"intent":"screenshot","mode":"{n}"}}"#)));
    out.extend(SettingsPage::NAMES.iter().map(|n| format!(r#"{{"intent":"dictate_settings","page":"{n}"}}"#)));
    out.extend(DiagProfile::NAMES.iter().map(|n| format!(r#"{{"intent":"diagnose","profile":"{n}"}}"#)));
    out.extend(crate::actions::BUILTIN.iter().map(|b| format!(r#"{{"intent":"text_action","action":"{}"}}"#, b.name)));
    out
}

#[test]
fn every_intent_the_daemon_writes_reads_the_same_in_the_ui() {
    let examples = examples();
    for spec in CATALOG {
        assert!(examples.iter().any(|e| e.contains(&format!(r#""intent":"{}""#, spec.name))), "no example for {}", spec.name);
    }
    for json in &examples {
        let daemon = intent::parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"));
        let wire = serde_json::to_value(&daemon).unwrap();
        let mirror: ui::Intent = serde_json::from_value(wire.clone()).unwrap_or_else(|e| panic!("{wire}: {e}"));
        assert_eq!(mirror.name(), daemon.name(), "{json}");
        assert_eq!(mirror.needs_confirm(), daemon.class() == Class::Confirm, "{json}");
    }
    assert_eq!(ui::SETTINGS_PAGES, SettingsPage::NAMES);
    assert_eq!(ui::DIAG_PROFILES, DiagProfile::NAMES);
}

#[test]
fn every_target_the_daemon_writes_reads_the_same_in_the_ui() {
    let cases = [
        (Target::None, ui::Target::None),
        (Target::Window { id: 3 }, ui::Target::Window { id: 3 }),
        (Target::Workspace { id: 9 }, ui::Target::Workspace { id: 9 }),
        (Target::MoveWindow { id: 3, workspace: 9 }, ui::Target::MoveWindow { id: 3, workspace: 9 }),
        (
            Target::App { id: "f.desktop".into(), argv: vec!["firefox".into(), "-P".into()], working_dir: Some(PathBuf::from("/srv")), terminal: true },
            ui::Target::App { id: "f.desktop".into(), argv: vec!["firefox".into(), "-P".into()], working_dir: Some(PathBuf::from("/srv")), terminal: true },
        ),
        (
            Target::App { id: "f.desktop".into(), argv: vec!["firefox".into()], working_dir: None, terminal: false },
            ui::Target::App { id: "f.desktop".into(), argv: vec!["firefox".into()], working_dir: None, terminal: false },
        ),
        (Target::Project { path: PathBuf::from("/home/u/p") }, ui::Target::Project { path: PathBuf::from("/home/u/p") }),
    ];
    for (daemon, mirror) in cases {
        let wire = serde_json::to_value(&daemon).unwrap();
        assert_eq!(serde_json::from_value::<ui::Target>(wire.clone()).unwrap(), mirror, "{wire}");
        // ein ganzer Kandidat, wie ihn der Daemon schreibt
        for class in [Class::Run, Class::Confirm, Class::Ai] {
            let c = Candidate {
                intent: intent::parse_validated(r#"{"intent":"focus_window","query":"x"}"#).unwrap(),
                target: daemon.clone(),
                display: "Firefox — switch to window".into(),
                detail: Some("Mail".into()),
                score: 1.5,
                class,
                target_id: "firefox".into(),
            };
            let read = ui::Candidate::from_value(&serde_json::to_value(&c).unwrap()).expect("candidate");
            assert_eq!(read.target, mirror);
            assert_eq!(serde_json::to_value(class).unwrap(), json!(format!("{:?}", read.class).to_lowercase()));
            assert_eq!(read.target_id, "firefox");
        }
    }
}

#[test]
fn the_context_limits_are_the_same() {
    assert_eq!(ui_ctx::MAX_WINDOWS, resolve::MAX_WINDOWS);
    assert_eq!(ui_ctx::MAX_WORKSPACES, resolve::MAX_WORKSPACES);
    assert_eq!(ui_ctx::MAX_TITLE_CHARS, resolve::MAX_TITLE_CHARS);
    assert_eq!(ui_ctx::MAX_APP_ID_CHARS, resolve::MAX_APP_ID_CHARS);
    assert_eq!(ui_ctx::MAX_NAME_CHARS, resolve::MAX_NAME_CHARS);
    const { assert!(ui_ctx::CONTEXT_BUDGET <= resolve::MAX_CONTEXT_BYTES) };
}

const OWN: Own = Own { app_id: "dictate-panel", pid: 4242 };

fn window(id: u64, title: &str, ws: u64) -> NiriWindow {
    NiriWindow { id, app_id: Some("a".repeat(200)), title: Some(title.into()), pid: Some(1), workspace_id: Some(ws), focus_ts: Some((id, 0)) }
}

fn workspace(id: u64, idx: u8, output: &str, name: Option<&str>) -> NiriWorkspace {
    NiriWorkspace { id, idx, name: name.map(String::from), output: Some(output.into()), active: idx == 1, focused: id == 1 }
}

/// Der schlimmste Kontext: alle Grenzen voll, Steuerzeichen (6 Bytes maskiert), doppelte und ungültige Einträge
fn worst() -> String {
    let title = "\u{1}".repeat(600);
    let name = "\u{2}".repeat(100);
    let windows: Vec<NiriWindow> = (1..=700).map(|i| window(i, &title, i % 300)).collect();
    let mut workspaces: Vec<NiriWorkspace> =
        (1..=300).map(|i| workspace(i, u8::try_from(i % 255 + 1).unwrap(), &format!("{name}{}", i / 255), Some(&name))).collect();
    workspaces.push(workspace(5, 0, "DP-1", None));
    workspaces.push(workspace(6, 1, "DP-9", None));
    workspaces.push(workspace(7, 1, "DP-9", None));
    ui_ctx::build(windows, workspaces, OWN, Some(700))
}

#[test]
fn every_built_context_passes_the_daemon() {
    let json = worst();
    assert!(json.len() <= ui_ctx::CONTEXT_BUDGET, "{}", json.len());
    let ctx = Context::from_json(&json).unwrap_or_else(|e| panic!("{e}"));
    assert!(!ctx.windows.is_empty());
    assert_eq!(ctx.focused_window().map(|w| w.id), Some(700));
    // ein gewöhnlicher Kontext kommt ganz an
    let json = ui_ctx::build(
        vec![window(1, "Mail — Thunderbird", 1), window(2, "zsh", 2)],
        vec![workspace(1, 1, "DP-1", None), workspace(2, 2, "DP-1", Some("web")), workspace(3, 1, "HDMI-A-1", None)],
        OWN,
        Some(2),
    );
    let ctx = Context::from_json(&json).unwrap();
    assert_eq!(ctx.windows.len(), 2);
    assert_eq!(ctx.workspaces.len(), 3);
    assert_eq!(ctx.focused_window().map(|w| w.id), Some(2));
    assert_eq!(ctx.workspace(2).and_then(|w| w.name()), Some("web"));
}

/// Die ganze Zeile von `pal_resolve`/`pal_ask` mit dem größten Kontext, der längsten Eingabe und der größten Umgebung
/// (je Zeichen so teuer maskiert wie möglich) bleibt unter der Grenze des Daemons
#[test]
fn the_worst_line_fits_the_daemon() {
    use crate::actions::store::SecretText;
    use crate::protocol::{Request, SessionEnv};
    let input = "\u{1}".repeat(resolve::MAX_INPUT_CHARS);
    // Werte der Umgebung dürfen keine Steuerzeichen tragen: `"` ist das teuerste Zeichen (2 Bytes)
    let env_value = "\"".repeat(super::service::MAX_ENV_BYTES);
    let env = SessionEnv { path: Some(env_value.clone()), desktop: Some(env_value.clone()), data_dirs: Some(env_value.clone()), data_home: Some(env_value) };
    for ask in [false, true] {
        let (input, context) = (SecretText::new(input.clone()), Some(SecretText::new(worst())));
        let req = if ask { Request::PalAsk { input, context, env: env.clone() } } else { Request::PalResolve { input, context, env: env.clone() } };
        let line = req.to_line();
        assert!(line.len() <= crate::daemon::MAX_PAL_RESOLVE_LINE, "{} > {}", line.len(), crate::daemon::MAX_PAL_RESOLVE_LINE);
        let back: Value = serde_json::from_str(line.trim()).unwrap();
        assert!(back["context"].is_object());
    }
}

/// Der Daemon bietet Zwischenablage und Emoji nur an, wenn er ihr Programm findet (`super::widgets`); `dictate-ui`
/// startet es mit `Programs::find`. Beide müssen dasselbe finden, sonst wäre eine angebotene Anzeige „Program not found“.
#[test]
fn the_daemon_finds_programs_like_the_ui() {
    use super::sources::desktop::find_program;
    use super::sources::testutil::TempDir;
    use std::os::unix::fs::{symlink, PermissionsExt};
    let t = TempDir::new("mirror-find");
    let exe = |rel: &str| {
        let p = t.write(rel, "#!/bin/sh\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let tool = exe("bin/tool");
    exe("other/tool");
    exe("other/second");
    t.write("bin/plain", "");
    t.mkdir("bin/dir");
    symlink(&tool, t.join("bin/link")).unwrap();
    symlink(t.join("bin/nothing"), t.join("bin/dangling")).unwrap();
    symlink(t.join("bin/dir"), t.join("bin/dirlink")).unwrap();
    // doppelter Ordner wie in echten PATH-Werten
    let dirs = vec![t.join("bin"), t.join("bin"), t.join("other")];
    let ui = ui::Programs::with_dirs(dirs.clone());
    let abs = |rel: &str| t.join(rel).display().to_string();
    let names = [
        "tool".to_string(),
        "second".into(),
        "plain".into(),
        "dir".into(),
        "link".into(),
        "dangling".into(),
        "dirlink".into(),
        "missing".into(),
        String::new(),
        "bin/tool".into(),
        "./tool".into(),
        "a\0b".into(),
        abs("bin/tool"),
        abs("bin/plain"),
        abs("bin/dir"),
        abs("bin/link"),
        abs("bin/missing"),
    ];
    for name in names {
        let daemon = find_program(&name, &dirs);
        let panel = ui.find(&name).map(|e| e.path().to_path_buf());
        assert_eq!(daemon, panel, "{name:?}");
    }
    // der erste Treffer im Suchpfad gewinnt auf beiden Seiten
    assert_eq!(find_program("tool", &dirs), Some(tool));
}
