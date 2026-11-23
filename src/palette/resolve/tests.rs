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

//! Tabellentests des Resolvers mit festen Fixtures (Programme, Projekte, Kontext) und die Laufzeitprüfung.

use super::*;
use crate::palette::intent::{ColumnDisplay, MediaKey, ScreenshotMode, Widget, WindowToggle};
use crate::palette::sources::testutil::TempDir;
use crate::palette::sources::{Frecency, FrecencySettings};
use std::path::PathBuf;
use std::time::Instant;

const NOW: i64 = 1_800_000_000;

fn app(id: &str, name: &str, generic: &str, keywords: &[&str], class: Option<&str>) -> App {
    let keywords: Vec<String> = keywords.iter().map(|k| k.to_string()).collect();
    App {
        id: id.into(),
        path: PathBuf::from(format!("/usr/share/applications/{id}")),
        name: name.into(),
        name_default: name.into(),
        generic_name: generic.into(),
        generic_name_default: generic.into(),
        keywords: keywords.clone(),
        keywords_default: keywords,
        exec_argv: vec![id.trim_end_matches(".desktop").to_string()],
        icon: None,
        terminal: false,
        working_dir: None,
        startup_wm_class: class.map(Into::into),
    }
}

/// Desktop-ID, wenn das Ziel ein Programm ist.
fn app_id(c: &Candidate) -> Option<&str> {
    match &c.target {
        Target::App { id, .. } => Some(id),
        _ => None,
    }
}

fn apps() -> Vec<App> {
    vec![
        app("firefox.desktop", "Firefox", "Web Browser", &["internet", "browser"], Some("firefox")),
        app("thunar.desktop", "Thunar File Manager", "File Manager", &["files", "folder"], Some("Thunar")),
        app("Alacritty.desktop", "Alacritty", "Terminal", &["shell", "prompt"], Some("Alacritty")),
        app("code.desktop", "Visual Studio Code", "Text Editor", &["vscode", "editor"], Some("Code")),
        app("spotify.desktop", "Spotify", "Music Player", &["music"], Some("Spotify")),
        app("gimp.desktop", "GIMP", "Image Editor", &["image", "photo"], Some("Gimp")),
        app("groessenrechner.desktop", "Größenrechner", "Calculator", &[], None),
        app("libreoffice-writer.desktop", "LibreOffice Writer", "Word Processor", &["office", "document"], Some("libreoffice-writer")),
    ]
}

fn project(path: &str, group: Option<&str>) -> Project {
    let path = PathBuf::from(path);
    let name = path.file_name().unwrap().to_str().unwrap().to_string();
    Project { path, name, group: group.map(Into::into) }
}

fn projects() -> Vec<Project> {
    vec![
        project("/home/git/blub/dictate", Some("blub")),
        project("/home/git/blub/dataroom", Some("blub")),
        project("/home/git/customers/acme-site", Some("customers")),
        project("/home/git/MailSorter", None),
    ]
}

/// Ein Monitor mit vier belegten Arbeitsflächen (drei benannt) und der leeren am Ende, wie niri sie meldet.
const CONTEXT: &str = r#"{
    "workspaces": [
        {"id": 11, "idx": 1, "name": "web", "output": "DP-1"},
        {"id": 12, "idx": 2, "output": "DP-1", "active": true, "focused": true},
        {"id": 13, "idx": 3, "name": "mail", "output": "DP-1"},
        {"id": 14, "idx": 4, "name": "music", "output": "DP-1"},
        {"id": 15, "idx": 5, "output": "DP-1"}
    ],
    "windows": [
        {"id": 100, "app_id": "firefox", "title": "Rust Ownership - Mozilla Firefox", "workspace_id": 11},
        {"id": 200, "app_id": "Alacritty", "title": "dataroom: claude", "workspace_id": 12, "focused": true},
        {"id": 300, "app_id": "spotify", "title": "Spotify Premium", "workspace_id": 14}
    ],
    "focus": 200
}"#;

/// Zwei Monitore: links DP-1 (Firefox fokussiert), rechts HDMI-A-1 (Spotify); je zwei belegte und eine leere.
const TWO_OUTPUTS: &str = r#"{
    "workspaces": [
        {"id": 11, "idx": 1, "output": "DP-1", "active": true, "focused": true},
        {"id": 12, "idx": 2, "name": "mail", "output": "DP-1"},
        {"id": 13, "idx": 3, "output": "DP-1"},
        {"id": 21, "idx": 1, "output": "HDMI-A-1", "active": true},
        {"id": 22, "idx": 2, "name": "chat", "output": "HDMI-A-1"},
        {"id": 23, "idx": 3, "output": "HDMI-A-1"}
    ],
    "windows": [
        {"id": 100, "app_id": "firefox", "title": "Docs", "workspace_id": 11, "focused": true},
        {"id": 300, "app_id": "spotify", "title": "Spotify Premium", "workspace_id": 21}
    ],
    "focus": 100
}"#;

fn ctx() -> Context {
    Context::from_json(CONTEXT).unwrap()
}

fn run(input: &str) -> Resolution {
    resolve(input, &ctx(), &apps(), &projects(), None, NOW)
}

fn run_with(input: &str, ctx: &Context, frecency: Option<&Frecency>) -> Resolution {
    resolve(input, ctx, &apps(), &projects(), frecency, NOW)
}

fn names(r: &Resolution) -> Vec<String> {
    r.candidates.iter().map(|c| format!("{} | {}", c.intent.name(), c.display)).collect()
}

fn top(r: &Resolution) -> &Candidate {
    r.candidates.first().unwrap_or_else(|| panic!("no candidates"))
}

fn find<'a>(r: &'a Resolution, intent: &str) -> Option<&'a Candidate> {
    r.candidates.iter().find(|c| c.intent.name() == intent)
}

// ---------------------------------------------------------------------------------------------------------------
// Abnahme

#[test]
fn ff_and_fire_find_firefox_with_the_running_window_first() {
    for input in ["ff", "fire", "firefox", "FireFox"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::FocusWindow { query: "Firefox".into() }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::Window { id: 100 }, "{input}");
        assert_eq!(c.display, "Firefox · Switch to window");
        assert_eq!(c.detail.as_deref(), Some("Rust Ownership - Mozilla Firefox · Workspace web"));
        assert_eq!(c.class, Class::Run);
        let launch = &r.candidates[1];
        assert_eq!(launch.intent, Intent::LaunchApp { query: "Firefox".into() }, "{input}: {:?}", names(&r));
        assert_eq!(app_id(launch), Some("firefox.desktop"));
        assert_eq!(launch.display, "Firefox · Launch");
        assert_eq!(launch.detail.as_deref(), Some("Web Browser"));
        assert!(c.score > launch.score, "{input}");
    }
}

#[test]
fn fire_and_firefox_are_confident_but_the_fuzzy_ff_is_not() {
    assert!(run("fire").confident);
    assert!(run("firefox").confident, "exakt");
    assert!(!run("ff").confident, "Fuzzy ist nie sicher");
    assert!(!run("f").confident, "ein Buchstabe ist nie sicher");
}

#[test]
fn a_digit_goes_to_that_workspace() {
    let r = run("3");
    let c = top(&r);
    assert_eq!(c.intent, Intent::ViewWorkspace { workspace: WorkspaceRef::Name("mail".into()) });
    assert_eq!(c.target, Target::Workspace { id: 13 });
    assert_eq!(c.display, "Workspace 3");
    assert_eq!(c.detail.as_deref(), Some("mail"));
    assert_eq!(c.target_id, "mail", "benannt: der Name");
    assert!(r.confident);
    let r = run("2");
    let c = top(&r);
    assert_eq!(
        (c.intent.clone(), c.target.clone(), c.target_id.as_str(), c.detail.clone()),
        (Intent::ViewWorkspace { workspace: WorkspaceRef::Index(2) }, Target::Workspace { id: 12 }, "2", None)
    );
    for n in 1..=5u64 {
        assert_eq!(top(&run(&n.to_string())).target, Target::Workspace { id: 10 + n }, "auch die leere am Ende");
    }
    // eine Arbeitsfläche, die es nicht gibt (1 bis 5), und Zahlen außerhalb von 1 bis 255
    for input in ["7", "0", "256", "42"] {
        let r = run(input);
        assert!(r.candidates.iter().all(|c| !matches!(c.target, Target::Workspace { .. })), "{input}: {:?}", names(&r));
        assert!(!r.confident, "{input}");
    }
}

#[test]
fn workspace_prefix_with_number_or_name() {
    for input in [
        "tag 3",
        "Tag 3",
        "tag mail",
        "workspace 3",
        "arbeitsfläche 3",
        "arbeitsflaeche mail",
        "show tag 3",
        "show workspace mail",
        "zeige tag mail",
        "zeige arbeitsfläche 3",
        "switch to 3",
        "gehe zu mail",
    ] {
        let r = run(input);
        assert_eq!(top(&r).target, Target::Workspace { id: 13 }, "{input}: {:?}", names(&r));
        assert!(matches!(top(&r).intent, Intent::ViewWorkspace { .. }));
    }
    assert!(run("tag 3").confident);
    assert!(run("workspace 3").confident);
    assert!(run("tag mail").confident);
    assert_eq!(top(&run("mail")).target, Target::Workspace { id: 13 }, "der Name allein genügt");
}

#[test]
fn without_a_workspace_list_numbers_find_nothing() {
    // ohne Liste gibt es keine ID, also kein Ziel (unter Awesome galt jede Zahl 1 bis 9)
    let empty = Context::default();
    for input in ["7", "tag 3", "workspace 1", "move to 2"] {
        let r = run_with(input, &empty, None);
        assert!(r.candidates.iter().all(|c| !matches!(c.target, Target::Workspace { .. } | Target::MoveWindow { .. })), "{input}: {:?}", names(&r));
    }
}

#[test]
fn with_two_outputs_a_number_means_the_monitor_of_the_target_window() {
    let two = Context::from_json(TWO_OUTPUTS).unwrap();
    // Anzeigen: der Monitor des fokussierten Fensters (DP-1)
    let r = run_with("2", &two, None);
    assert_eq!(top(&r).target, Target::Workspace { id: 12 }, "{:?}", names(&r));
    assert_eq!(top(&r).detail.as_deref(), Some("mail · DP-1"), "zwei Monitore: der Monitor steht dabei");
    assert!(r.confident, "die Zahl auf dem anderen Monitor ist kein Konkurrent");
    assert!(r.candidates.iter().all(|c| c.target != Target::Workspace { id: 22 }), "{:?}", names(&r));
    // ein Name trifft auf jedem Monitor
    assert_eq!(top(&run_with("workspace chat", &two, None)).target, Target::Workspace { id: 22 });
    // Verschieben mit Begriff: die Zahl gilt auf dem Monitor des gefundenen Fensters
    let c = resolve_intent(
        &Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(2), query: Some("spotify".into()) },
        "",
        &two,
        &apps(),
        &projects(),
        None,
        NOW,
    )
    .unwrap();
    assert_eq!(c[0].target, Target::MoveWindow { id: 300, workspace: 22 });
    // das fokussierte Fenster auf 3: die leere am Ende von DP-1
    assert_eq!(top(&run_with("move to 3", &two, None)).target, Target::MoveWindow { id: 100, workspace: 13 });
    // ohne fokussiertes Fenster und ohne fokussierte Arbeitsfläche ist der Monitor offen: keine Zahl, aber Namen
    let open = Context::from_json(&TWO_OUTPUTS.replace(r#""focused": true"#, r#""focused": false"#).replace(r#""focus": 100"#, r#""focus": null"#)).unwrap();
    assert_eq!(open.focused_output(), None);
    assert!(run_with("2", &open, None).candidates.iter().all(|c| !matches!(c.target, Target::Workspace { .. })));
    assert_eq!(top(&run_with("workspace mail", &open, None)).target, Target::Workspace { id: 12 });
    assert_eq!(
        resolve_intent(&Intent::ViewWorkspace { workspace: WorkspaceRef::Index(1) }, "", &open, &apps(), &projects(), None, NOW),
        Err(NoTarget::Workspace)
    );
}

#[test]
fn project_by_name() {
    let r = run("dataroom");
    let c = top(&r);
    assert_eq!(c.intent, Intent::OpenProject { query: "dataroom".into() });
    assert_eq!(c.target, Target::Project { path: PathBuf::from("/home/git/blub/dataroom") });
    assert_eq!(c.display, "Open project dataroom in a terminal");
    assert_eq!(c.detail.as_deref(), Some("/home/git/blub/dataroom"));
    assert_eq!(c.target_id, "/home/git/blub/dataroom");
    assert!(r.confident);
    // auch das Fenster, dessen Titel das Projekt nennt, steht in der Liste, aber dahinter
    assert!(r.candidates.iter().any(|c| c.target == Target::Window { id: 200 }), "{:?}", names(&r));
}

#[test]
fn project_prefixes_search_only_projects() {
    for input in ["projekt dataroom", "project dataroom", "claude dataroom", "open dataroom", "öffne dataroom", "oeffne dataroom", "open project dataroom"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::OpenProject { query: "dataroom".into() }, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}");
    }
    let r = run("claude d");
    let found: Vec<&str> = r.candidates.iter().filter(|c| c.intent.name() == "open_project").map(|c| c.display.as_str()).collect();
    assert_eq!(found.len(), 2, "dictate und dataroom beginnen mit d: {:?}", names(&r));
    assert!(matches!(r.candidates[0].intent, Intent::OpenProject { .. }) && matches!(r.candidates[1].intent, Intent::OpenProject { .. }), "{:?}", names(&r));
    assert!(!r.confident);
}

#[test]
fn folder_prefixes_open_the_project_folder() {
    for input in ["ordner dataroom", "folder dataroom", "open folder dataroom", "öffne ordner dataroom", "files dataroom"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::OpenFolder { query: "dataroom".into() }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::Project { path: PathBuf::from("/home/git/blub/dataroom") });
        assert_eq!(c.display, "Open folder dataroom");
    }
}

#[test]
fn app_prefixes_launch_a_new_instance() {
    for input in ["starte firefox", "launch firefox", "start firefox", "run firefox", "app firefox", "open firefox", "open app firefox"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::LaunchApp { query: "Firefox".into() }, "{input}: {:?}", names(&r));
        assert_eq!(app_id(c), Some("firefox.desktop"), "{input}");
        assert!(r.confident, "{input}");
    }
}

#[test]
fn lock_keywords_are_confident() {
    for input in ["sperr", "sperren", "lock", "Lock Screen", "bildschirm sperren", "sperre"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::Lock {}, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}: {:?}", names(&r));
    }
    assert_eq!(top(&run("lock")).display, "Lock screen");
    assert_eq!(top(&run("lock")).target, Target::None);
}

#[test]
fn volume_keywords_and_set() {
    let step = |input: &str| match &top(&run(input)).intent {
        Intent::Volume { step: Some(s), set: None, mute: None } => *s,
        other => panic!("{input}: {other:?}"),
    };
    for input in ["lauter", "louder", "volume up", "lautstärke erhöhen", "lautstaerke erhoehen"] {
        assert_eq!(step(input), 5, "{input}");
    }
    for input in ["leiser", "quieter", "volume down"] {
        assert_eq!(step(input), -5, "{input}");
    }
    assert!(run("lauter").confident);
    for (input, want) in
        [("volume 40", 40), ("lautstärke 40", 40), ("Volume 40%", 40), ("vol 0", 0), ("set volume to 100", 100), ("lautstärke auf 75", 75), ("volume to 5", 5)]
    {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::Volume { set: Some(want), step: None, mute: None }, "{input}: {:?}", names(&r));
        assert_eq!(top(&r).display, format!("Set volume to {want}%"));
        assert!(r.confident, "{input}");
    }
    for input in ["volume 101", "volume 4x", "volume -5", "volume", "volume 40 50"] {
        let r = run(input);
        assert!(r.candidates.iter().all(|c| !matches!(c.intent, Intent::Volume { set: Some(_), .. })), "{input}: {:?}", names(&r));
    }
    assert_eq!(top(&run("stumm")).intent, Intent::Volume { set: None, step: None, mute: Some(true) });
    assert!(run("stumm").confident, "exakt schlägt „stumm aufheben“");
    assert_eq!(top(&run("mute")).intent, Intent::Volume { set: None, step: None, mute: Some(true) });
    assert_eq!(top(&run("unmute")).intent, Intent::Volume { set: None, step: None, mute: Some(false) });
}

#[test]
fn close_is_a_confirm_class_on_the_focused_window() {
    for input in ["schließen", "schliessen", "close", "Fenster schließen", "schließe"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::CloseWindow { query: None }, "{input}: {:?}", names(&r));
        assert_eq!(c.class, Class::Confirm, "{input}");
        assert_eq!(c.target, Target::Window { id: 200 }, "das fokussierte Fenster");
        assert_eq!(c.detail.as_deref(), Some("Alacritty"));
    }
    assert!(run("schließen").confident);
    // mit Namen: das passende Fenster, ebenfalls zu bestätigen
    for input in ["close firefox", "schließe firefox", "close window firefox"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::CloseWindow { query: Some("Firefox".into()) }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::Window { id: 100 });
        assert_eq!(c.class, Class::Confirm);
        assert_eq!(c.display, "Firefox · Close window");
    }
}

#[test]
fn power_actions_are_confirm_class() {
    for (input, name) in [
        ("abmelden", "logout"),
        ("logout", "logout"),
        ("neustart", "reboot"),
        ("reboot", "reboot"),
        ("ausschalten", "poweroff"),
        ("shutdown", "poweroff"),
        ("herunterfahren", "poweroff"),
        ("bereitschaft", "suspend"),
        ("suspend", "suspend"),
    ] {
        let r = run(input);
        assert_eq!(top(&r).intent.name(), name, "{input}: {:?}", names(&r));
        assert_eq!(top(&r).class, Class::Confirm, "{input}");
        assert!(r.confident, "{input}");
    }
}

#[test]
fn question_mark_asks_and_nothing_else() {
    for input in ["? was ist x", "?was ist x", "  ?   was ist x  "] {
        let r = run(input);
        assert_eq!(r.candidates.len(), 1, "{input}: {:?}", names(&r));
        assert_eq!(top(&r).intent, Intent::Ask { question: "was ist x".into() });
        assert_eq!(top(&r).class, Class::Ai);
        assert_eq!(top(&r).display, "Ask AI: was ist x");
        assert!(r.confident);
    }
    let r = run("? firefox");
    assert_eq!(r.candidates.len(), 1, "keine Fenster oder Programme neben einer ausdrücklichen Frage");
    assert!(run("?").candidates.is_empty());
    assert!(!run("?  ").confident);
    // zu lang für eine Frage: abgeschnitten, nie ein ungültiger Kandidat
    let long = format!("? {}", "a".repeat(5000));
    let r = run(&long);
    assert!(top(&r).intent.validate().is_ok());
}

#[test]
fn g_prefix_searches_the_web() {
    for input in ["g rust ownership", "G rust ownership", "search rust ownership", "suche rust ownership"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::WebSearch { query: "rust ownership".into() }, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}");
    }
    assert_eq!(top(&run("g rust ownership")).display, "Search the web for \"rust ownership\"");
    assert_eq!(top(&run("g Größe & Maß")).intent, Intent::WebSearch { query: "Größe & Maß".into() }, "Schreibweise bleibt erhalten");
    assert!(find(&run("g"), "web_search").is_none(), "ohne Begriff keine Suche");
}

#[test]
fn urls_open_in_the_browser() {
    for (input, url) in [
        ("github.com", "https://github.com"),
        ("https://github.com/rust-lang/rust", "https://github.com/rust-lang/rust"),
        ("http://localhost:3000/x", "http://localhost:3000/x"),
        ("www.example.com", "https://www.example.com"),
        ("docs.rs/serde", "https://docs.rs/serde"),
        ("example.org:8080/a?b=1#c", "https://example.org:8080/a?b=1#c"),
        ("Rust-Lang.ORG", "https://Rust-Lang.ORG"),
        ("shop.example.co", "https://shop.example.co"),
    ] {
        let r = run(input);
        if input == "docs.rs/serde" {
            assert!(find(&r, "open_url").is_none(), "„.rs“ ist auch eine Dateiendung: {:?}", names(&r));
            continue;
        }
        assert_eq!(top(&r).intent, Intent::OpenUrl { url: url.into() }, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}");
    }
    let r = run("github.com");
    assert_eq!(top(&r).display, "Open github.com in browser");
    assert_eq!(top(&r).detail.as_deref(), Some("https://github.com"));
    // keine Adressen
    for input in ["main.rs", "readme.md", "Cargo.toml", "foo.bar", "a b.com", "http://", "https://user:pw@example.com", "1.5", "mail me.com", "x.y.z"] {
        assert!(find(&run(input), "open_url").is_none(), "{input}");
    }
    assert_eq!(top(&run("open github.com")).intent, Intent::OpenUrl { url: "https://github.com".into() });
}

#[test]
fn umlaut_folding_works_end_to_end() {
    for input in ["grossenr", "größenr", "groessenr", "GRÖSSENR", "grosse"] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::LaunchApp { query: "Größenrechner".into() }, "{input}: {:?}", names(&r));
        assert_eq!(app_id(c), Some("groessenrechner.desktop"));
    }
    assert!(run("größenrechner").confident);
    // Umlaut im Text, einfache Eingabe auch bei einem Projekt
    let ps = vec![project("/p/Übungsprojekt", None)];
    for input in ["ubung", "uebung", "übung"] {
        let r = resolve(input, &Context::default(), &[], &ps, None, NOW);
        assert_eq!(top(&r).intent.name(), "open_project", "{input}");
    }
}

#[test]
fn camel_case_and_initials_rank_high() {
    let r = run("ms");
    assert_eq!(top(&r).intent, Intent::OpenProject { query: "MailSorter".into() }, "{:?}", names(&r));
    let r = run("vsc");
    assert_eq!(top(&r).intent, Intent::LaunchApp { query: "Visual Studio Code".into() }, "{:?}", names(&r));
    let r = run("lw");
    assert_eq!(top(&r).intent, Intent::LaunchApp { query: "LibreOffice Writer".into() }, "{:?}", names(&r));
    assert!(!r.confident);
}

#[test]
fn nonsense_has_no_confident_hit() {
    for input in ["asdkjhqwe", "qwertzuiop", "xyzzy plugh", "wie spät ist es in tokio gerade jetzt und morgen", "+++", "..."] {
        let r = run(input);
        assert!(!r.confident, "{input}: {:?}", names(&r));
        assert!(r.candidates.is_empty(), "{input}: {:?}", names(&r));
    }
}

#[test]
fn clipboard_history_and_explain_clipboard_are_separate() {
    for input in ["zwischenablage", "clipboard", "clipboard history", "zwischenablage verlauf"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::ShowWidget { widget: Widget::ClipboardHistory }, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}");
    }
    for input in ["erkläre die zwischenablage", "erklaere die zwischenablage", "explain clipboard", "explain the clipboard", "Zwischenablage erklären"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::ExplainClipboard {}, "{input}: {:?}", names(&r));
        assert_eq!(top(&r).class, Class::Ai, "{input}");
        assert!(r.confident, "{input}");
        assert!(find(&r, "show_widget").is_none(), "{input}: {:?}", names(&r));
    }
}

#[test]
fn widgets_settings_and_screenshot() {
    let widget = |input: &str| match &top(&run(input)).intent {
        Intent::ShowWidget { widget } => *widget,
        other => panic!("{input}: {other:?}"),
    };
    // die Türkamera ist kein eingebautes Widget mehr (O3, E3)
    assert!(run("türkamera").candidates.iter().all(|c| !matches!(c.intent, Intent::ShowWidget { .. })));
    assert_eq!(widget("zwischenablage"), Widget::ClipboardHistory);
    assert!(run("claude usage").candidates.iter().all(|c| !matches!(c.intent, Intent::ShowWidget { .. })), "claude_usage gibt es nicht mehr");
    assert_eq!(widget("emoji"), Widget::Emoji);
    assert_eq!(top(&run("einstellungen")).intent, Intent::DictateSettings { page: None });
    assert_eq!(top(&run("settings")).intent, Intent::DictateSettings { page: None });
    assert!(run("settings").confident, "exakt schlägt „settings audio“");
    let r = run("settings audio");
    assert_eq!(top(&r).intent.name(), "dictate_settings");
    assert_eq!(top(&r).display, "Dictate settings › Audio");
    assert_eq!(top(&r).target_id, "audio");
    // Screenshot: Bereich zuerst, dann ganzer Bildschirm, dann Fenster
    let r = run("screenshot");
    let modes: Vec<ScreenshotMode> = r
        .candidates
        .iter()
        .filter_map(|c| match &c.intent {
            Intent::Screenshot { mode } => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(modes, [ScreenshotMode::Area, ScreenshotMode::Full, ScreenshotMode::Window]);
    assert_eq!(top(&run("screenshot window")).intent, Intent::Screenshot { mode: ScreenshotMode::Window });
}

#[test]
fn media_keys_and_previous_workspace() {
    let media = |input: &str| match &top(&run(input)).intent {
        Intent::Media { action } => *action,
        other => panic!("{input}: {other:?}"),
    };
    assert_eq!(media("pause"), MediaKey::PlayPause);
    assert_eq!(media("play"), MediaKey::PlayPause);
    assert_eq!(media("nächster titel"), MediaKey::Next);
    assert_eq!(media("naechster titel"), MediaKey::Next);
    assert_eq!(media("next track"), MediaKey::Next);
    assert_eq!(media("vorheriger titel"), MediaKey::Previous);
    assert_eq!(media("previous track"), MediaKey::Previous);
    for input in ["vorheriger tag", "back", "zurück", "previous tag", "go back", "previous workspace", "vorherige arbeitsfläche"] {
        let r = run(input);
        assert_eq!(top(&r).intent, Intent::PreviousWorkspace {}, "{input}: {:?}", names(&r));
        assert!(r.confident, "{input}");
    }
    assert!(!run("previous").confident, "vorheriger Titel oder Arbeitsfläche?");
    assert!(!run("vorheriger").confident);
}

#[test]
fn window_toggles_act_on_the_focused_window() {
    for (input, what) in [
        ("floating", WindowToggle::Floating),
        ("vollbild", WindowToggle::Fullscreen),
        ("fullscreen", WindowToggle::Fullscreen),
        ("maximieren", WindowToggle::Maximized),
        ("maximize", WindowToggle::Maximized),
    ] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::ToggleWindow { what }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::Window { id: 200 }, "{input}");
        assert_eq!(c.class, Class::Run);
    }
    assert!(run("floating").confident);
    // sticky und ontop gibt es unter niri nicht
    for input in ["sticky", "always on top"] {
        assert!(run(input).candidates.iter().all(|c| c.intent.name() != "toggle_window"), "{input}: {:?}", names(&run(input)));
    }
}

#[test]
fn column_display_acts_on_the_focused_window() {
    for (input, display) in [
        ("tabbed", ColumnDisplay::Tabbed),
        ("tabs", ColumnDisplay::Tabbed),
        ("spalte tabs", ColumnDisplay::Tabbed),
        ("normal column", ColumnDisplay::Normal),
        ("spalte normal", ColumnDisplay::Normal),
    ] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::ColumnDisplay { display }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::Window { id: 200 }, "{input}");
        assert_eq!(c.detail.as_deref(), Some("Alacritty"));
    }
    assert!(run("tabbed").confident);
    for input in ["layout tile", "set layout max"] {
        assert!(run(input).candidates.iter().all(|c| !c.intent.name().contains("layout")), "{input}");
    }
}

#[test]
fn without_a_focused_window_there_is_nothing_to_close_or_toggle() {
    let ctx = Context::from_json(r#"{"workspaces":[{"id":1,"idx":3,"focused":true}],"windows":[{"id":1,"app_id":"firefox","workspace_id":1}]}"#).unwrap();
    for input in ["schließen", "close", "floating", "vollbild", "tabbed", "move to 3", "verschiebe nach 3"] {
        let r = run_with(input, &ctx, None);
        assert!(
            r.candidates.iter().all(|c| !matches!(c.intent.name(), "close_window" | "toggle_window" | "column_display" | "move_window_to_workspace")
                || matches!(&c.intent, Intent::CloseWindow { query: Some(_) })),
            "{input}: {:?}",
            names(&r)
        );
    }
}

#[test]
fn move_the_focused_window_to_a_workspace() {
    for input in [
        "verschiebe nach 3",
        "verschiebe nach tag 3",
        "verschiebe nach arbeitsfläche 3",
        "verschiebe auf 3",
        "move to 3",
        "move to tag 3",
        "move to workspace 3",
        "move window to tag 3",
        "move window to workspace 3",
        "send to 3",
        "move 3",
        "move to mail",
    ] {
        let r = run(input);
        let c = top(&r);
        assert_eq!(c.intent, Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Name("mail".into()), query: None }, "{input}: {:?}", names(&r));
        assert_eq!(c.target, Target::MoveWindow { id: 200, workspace: 13 }, "{input}: das fokussierte Fenster");
        assert_eq!(c.display, "Move window to workspace mail");
        assert_eq!(c.class, Class::Run);
        assert_eq!(c.target_id, "mail");
    }
    assert!(run("move to 3").confident);
    assert!(run("move to mail").confident, "Name exakt");
    let r = run("move to 5");
    let c = top(&r);
    assert_eq!(
        (c.target.clone(), c.display.as_str(), c.target_id.as_str()),
        (Target::MoveWindow { id: 200, workspace: 15 }, "Move window to workspace 5", "5"),
        "die leere am Ende"
    );
    let r = run("move to 8");
    assert!(find(&r, "move_window_to_workspace").is_none(), "8 gibt es nicht: {:?}", names(&r));
}

#[test]
fn switch_prefix_finds_windows_and_tags() {
    for input in ["wechsle zu spotify", "switch to spotify", "go to spotify", "focus spotify", "zeige spotify"] {
        let r = run(input);
        assert_eq!(top(&r).target, Target::Window { id: 300 }, "{input}: {:?}", names(&r));
        assert!(matches!(top(&r).intent, Intent::FocusWindow { .. }));
    }
    assert_eq!(top(&run("go to 4")).target, Target::Workspace { id: 14 });
}

#[test]
fn windows_match_app_id_and_title() {
    // Titel: „Rust Ownership“ steht im Titel von Firefox
    let r = run("ownership");
    assert_eq!(top(&r).target, Target::Window { id: 100 }, "{:?}", names(&r));
    assert!(!r.confident, "Titeltreffer sind schwächer");
    // app_id ohne Programm
    let ctx = Context::from_json(r#"{"windows":[{"id":9,"app_id":"org.example.Navigator","title":"Start"}]}"#).unwrap();
    assert_eq!(top(&run_with("navigator", &ctx, None)).target, Target::Window { id: 9 });
}

#[test]
fn windows_without_a_matching_app_use_the_app_id() {
    let ctx = Context::from_json(r#"{"windows":[{"id":9,"app_id":"Slack","title":"General - Team"}]}"#).unwrap();
    let r = run_with("slack", &ctx, None);
    assert_eq!(top(&r).intent, Intent::FocusWindow { query: "Slack".into() });
    assert_eq!(top(&r).display, "Slack · Switch to window");
    assert_eq!(top(&r).target, Target::Window { id: 9 });
    assert_eq!(top(&r).target_id, "slack", "app_id klein, nie der Titel");
    assert!(r.confident);
}

#[test]
fn two_windows_of_one_app_are_not_confident() {
    let ctx = Context::from_json(r#"{"windows":[{"id":1,"app_id":"firefox","title":"A"},{"id":2,"app_id":"firefox","title":"B"}],"focus":1}"#).unwrap();
    let r = run_with("firefox", &ctx, None);
    let windows: Vec<u64> = r
        .candidates
        .iter()
        .filter_map(|c| match c.target {
            Target::Window { id } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(windows, [2, 1], "das fokussierte Fenster steht hinter dem anderen");
    assert!(!r.confident);
    assert_eq!(r.candidates[2].intent.name(), "launch_app", "der Neustart steht hinter den Fenstern: {:?}", names(&r));
}

// ---------------------------------------------------------------------------------------------------------------
// Häufigkeit

fn frecency(t: &TempDir, records: &[(&str, &str, u32, i64)]) -> Frecency {
    let f = Frecency::open(t.join("frecency.json"), FrecencySettings::default());
    for (intent, target, times, at) in records {
        for _ in 0..*times {
            f.record(intent, target, *at).unwrap();
        }
    }
    f
}

#[test]
fn empty_input_lists_frecent_targets_then_running_windows() {
    let t = TempDir::new("resolve-empty");
    let f = frecency(
        &t,
        &[
            ("launch_app", "gimp.desktop", 3, NOW - 60),
            ("open_project", "/home/git/blub/dictate", 8, NOW - 120),
            ("launch_app", "firefox.desktop", 30, NOW - 30),
            ("lock", "lock", 1, NOW - 90_000),
            ("view_workspace", "2", 2, NOW - 400_000),
        ],
    );
    let r = run_with("", &ctx(), Some(&f));
    let got: Vec<String> = r.candidates.iter().map(|c| format!("{}:{}", c.intent.name(), c.target_id)).collect();
    assert!(!r.confident);
    // häufig gewählt: Projekt (8 × 4), dann GIMP (3 × 4); Firefox läuft, sein Fenster steht in der Liste und der Neustart nicht
    assert_eq!(got[0], "open_project:/home/git/blub/dictate", "{got:?}");
    assert_eq!(got[1], "launch_app:gimp.desktop", "{got:?}");
    assert!(!got.contains(&"launch_app:firefox.desktop".to_string()), "{got:?}");
    // lock (1 × 0,5) und Arbeitsfläche 2 (2 × 0,5) sind gewählt worden; 2 ist häufiger
    let pos = |s: &str| got.iter().position(|g| g == s).unwrap_or_else(|| panic!("{s} missing in {got:?}"));
    assert!(pos("view_workspace:2") < pos("lock:lock"), "{got:?}");
    // laufende Fenster stehen hinter allen häufig gewählten Zielen; das fokussierte (Alacritty) zuletzt
    assert!(pos("lock:lock") < pos("focus_window:firefox"), "{got:?}");
    let windows: Vec<&str> = got.iter().filter(|g| g.starts_with("focus_window:")).map(String::as_str).collect();
    assert_eq!(windows.last(), Some(&"focus_window:alacritty"), "{got:?}");
    assert_eq!(windows.len(), 3);
    assert!(r.candidates.iter().all(|c| c.intent.validate().is_ok()));
}

#[test]
fn empty_input_without_history_lists_the_running_windows() {
    let r = run("");
    assert_eq!(r.candidates.len(), 3, "{:?}", names(&r));
    assert!(r.candidates.iter().all(|c| c.intent.name() == "focus_window"));
    assert_eq!(r.candidates.last().unwrap().target, Target::Window { id: 200 }, "das fokussierte Fenster zuletzt");
    assert!(!r.confident);
    for blank in ["", "   ", "\t\n "] {
        assert_eq!(run(blank), r);
    }
    assert!(resolve("", &Context::default(), &apps(), &projects(), None, NOW).candidates.is_empty());
}

#[test]
fn empty_input_ignores_history_when_it_is_off() {
    let t = TempDir::new("resolve-off");
    let on = frecency(&t, &[("launch_app", "gimp.desktop", 5, NOW)]);
    let off = Frecency::open(t.join("frecency.json"), FrecencySettings { enabled: false, ..FrecencySettings::default() });
    assert!(run_with("", &ctx(), Some(&on)).candidates.iter().any(|c| c.intent.name() == "launch_app"));
    assert!(run_with("", &ctx(), Some(&off)).candidates.iter().all(|c| c.intent.name() == "focus_window"));
}

#[test]
fn frecency_sorts_equals_but_never_beats_a_clearly_better_text_match() {
    let t = TempDir::new("resolve-bonus");
    let list = vec![
        app("terminal-b.desktop", "Terminal B", "", &[], None),
        app("terminal-a.desktop", "Terminal A", "", &[], None),
        app("my-terminal.desktop", "My Terminal Tool", "", &[], None),
    ];
    // ohne Häufigkeit: Präfix-Treffer vor dem Wortanfang
    let launches = |r: &Resolution| -> Vec<Candidate> { r.candidates.iter().filter(|c| c.intent.name() == "launch_app").cloned().collect() };
    let none = launches(&resolve("term", &Context::default(), &list, &[], None, NOW));
    assert_eq!(none.last().unwrap().target_id, "my-terminal.desktop");
    // gleiche Güte: die häufig gewählte Anwendung nach vorn
    let f = frecency(&t, &[("launch_app", "terminal-b.desktop", 2, NOW - 10)]);
    let r = launches(&resolve("term", &Context::default(), &list, &[], Some(&f), NOW));
    assert_eq!(r[0].target_id, "terminal-b.desktop");
    // auch eine sehr häufig gewählte Anwendung mit schwächerem Treffer (Wortanfang statt Präfix) bleibt dahinter
    let f = frecency(&t, &[("launch_app", "my-terminal.desktop", 100, NOW - 10)]);
    let r = launches(&resolve("term", &Context::default(), &list, &[], Some(&f), NOW));
    let ids: Vec<&str> = r.iter().map(|c| c.target_id.as_str()).collect();
    assert_eq!(ids.last(), Some(&"my-terminal.desktop"), "{ids:?}");
    assert!(r.last().unwrap().score > none.last().unwrap().score, "der Bonus wirkt, nur nicht über die Stufe hinaus");
}

#[test]
fn frecency_never_makes_a_hit_confident() {
    let t = TempDir::new("resolve-conf");
    let f = frecency(&t, &[("launch_app", "gimp.desktop", 100, NOW - 1)]);
    let r = run_with("gi", &Context::default(), Some(&f));
    assert_eq!(top(&r).target_id, "gimp.desktop");
    assert_eq!(r.confident, run("gi").confident);
    assert!(!run_with("ff", &ctx(), Some(&f)).confident);
}

#[test]
fn frecency_bonus_is_bounded_and_monotonic() {
    assert_eq!(frecency_bonus(0.0), 0.0);
    assert_eq!(frecency_bonus(-3.0), 0.0);
    let mut prev = 0.0;
    for f in [0.5, 1.0, 2.0, 4.0, 12.0, 50.0, 100.0, 400.0] {
        let b = frecency_bonus(f);
        assert!(b >= prev && b <= FRECENCY_MAX + 1e-4, "{f}: {b}");
        prev = b;
    }
    assert!((frecency_bonus(400.0) - FRECENCY_MAX).abs() < 1e-4);
}

// ---------------------------------------------------------------------------------------------------------------
// Querschnitt

const MANY_INPUTS: &[&str] = &[
    "",
    "f",
    "ff",
    "fire",
    "3",
    "tag 3",
    "workspace mail",
    "arbeitsfläche 2",
    "dataroom",
    "lock",
    "sperr",
    "lauter",
    "volume 40",
    "schließen",
    "? was ist x",
    "g rust",
    "github.com",
    "close firefox",
    "verschiebe nach 3",
    "move to 5",
    "tabbed",
    "settings audio",
    "screenshot",
    "claude usage",
    "zwischenablage",
    "explain clipboard",
    "starte firefox",
    "projekt dictate",
    "ordner dictate",
    "open https://example.com",
    "a",
    "e",
    "s",
    "t",
    "m",
    "o",
    "1",
    "9",
    "ä",
    "größe",
    "ß",
    "😀",
    "tag",
    "workspace",
    "move",
    "close",
    "open",
    "start",
    "switch to",
    "mail",
    "web",
    "music",
];

#[test]
fn every_candidate_is_valid_sorted_and_bounded() {
    let t = TempDir::new("resolve-valid");
    let f = frecency(&t, &[("launch_app", "gimp.desktop", 5, NOW - 5), ("view_workspace", "2", 3, NOW - 5), ("move_window_to_workspace", "mail", 2, NOW - 5)]);
    let two = Context::from_json(TWO_OUTPUTS).unwrap();
    for (frec, ctx) in [(None, ctx()), (Some(&f), ctx()), (Some(&f), two)] {
        for input in MANY_INPUTS {
            let r = run_with(input, &ctx, frec);
            assert!(r.candidates.len() <= MAX_CANDIDATES, "{input}");
            let mut prev = f32::INFINITY;
            for c in &r.candidates {
                assert_eq!(c.intent.validate(), Ok(()), "{input}: {:?}", c.intent);
                assert_eq!(c.class, c.intent.class(), "{input}");
                assert!(c.score.is_finite() && c.score <= prev, "{input}: sortiert");
                prev = c.score;
                assert!(!c.display.is_empty() && c.display.chars().all(|ch| !ch.is_control()), "{input}: {:?}", c.display);
                assert!(!c.target_id.is_empty() && c.target_id.chars().all(|ch| !ch.is_control()), "{input}");
                if let Some(d) = &c.detail {
                    assert!(!d.is_empty() && d.chars().all(|ch| !ch.is_control()), "{input}: {d:?}");
                }
                // Fenster- und Arbeitsflächenabsichten tragen ihr Ziel, und zwar eins aus dem Kontext
                let window = |id: &u64| ctx.windows.iter().any(|w| w.id == *id);
                let workspace = |id: &u64| ctx.workspaces.iter().any(|w| w.id == *id);
                match (&c.intent, &c.target) {
                    (
                        Intent::FocusWindow { .. } | Intent::ToggleWindow { .. } | Intent::CloseWindow { .. } | Intent::ColumnDisplay { .. },
                        Target::Window { id },
                    ) => {
                        assert!(window(id), "{input}")
                    }
                    (Intent::ViewWorkspace { .. }, Target::Workspace { id }) => assert!(workspace(id), "{input}"),
                    (Intent::MoveWindowToWorkspace { .. }, Target::MoveWindow { id, workspace: ws }) => assert!(window(id) && workspace(ws), "{input}"),
                    (Intent::LaunchApp { .. }, Target::App { .. }) | (Intent::OpenProject { .. } | Intent::OpenFolder { .. }, Target::Project { .. }) => {}
                    (
                        Intent::FocusWindow { .. }
                        | Intent::ToggleWindow { .. }
                        | Intent::CloseWindow { .. }
                        | Intent::ColumnDisplay { .. }
                        | Intent::ViewWorkspace { .. }
                        | Intent::MoveWindowToWorkspace { .. }
                        | Intent::LaunchApp { .. }
                        | Intent::OpenProject { .. }
                        | Intent::OpenFolder { .. },
                        other,
                    ) => panic!("{input}: {} with target {other:?}", c.intent.name()),
                    _ => assert_eq!(c.target, Target::None, "{input}"),
                }
            }
            // jedes Ziel höchstens einmal
            let mut keys: Vec<String> = r.candidates.iter().map(|c| format!("{:?}{:?}", c.intent, c.target)).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), r.candidates.len(), "{input}");
        }
    }
}

#[test]
fn control_characters_in_titles_never_reach_the_candidates() {
    let ctx =
        Context::from_json(r#"{"windows":[{"id":5,"app_id":"xterm","title":"build\n\u001b[31mfailed\u0007\u202e evil","workspace_id":2}],"focus":5}"#).unwrap();
    let r = run_with("xterm", &ctx, None);
    let c = top(&r);
    assert_eq!(c.intent, Intent::FocusWindow { query: "xterm".into() });
    let detail = c.detail.as_deref().unwrap();
    assert!(detail.chars().all(|ch| !ch.is_control() && ch != '\u{202e}'), "{detail:?}");
    assert!(detail.contains("build") && detail.contains("failed"), "{detail:?}");
    // auch als Suchtext und als Eingabe
    let r = run_with("build\nfailed", &ctx, None);
    assert!(r.candidates.iter().all(|c| c.intent.validate().is_ok()));
    let r = run_with("g a\u{0}b", &ctx, None);
    assert_eq!(top(&r).intent, Intent::WebSearch { query: "a b".into() });
}

#[test]
fn a_window_without_any_name_is_skipped() {
    let ctx = Context::from_json(r#"{"windows":[{"id":5}]}"#).unwrap();
    assert!(run_with("", &ctx, None).candidates.is_empty());
}

#[test]
fn long_titles_and_inputs_are_bounded() {
    let title = "x".repeat(MAX_TITLE_CHARS);
    let ctx = Context::from_json(&format!(r#"{{"windows":[{{"id":5,"app_id":"c","title":"{title}"}}]}}"#)).unwrap();
    let r = run_with("c", &ctx, None);
    assert!(top(&r).detail.as_ref().unwrap().chars().count() <= 100);
    let huge = "word ".repeat(10_000);
    let r = run(&huge);
    assert!(!r.confident && r.candidates.is_empty());
    let r = run(&format!("g {}", "a".repeat(10_000)));
    assert!(r.candidates.iter().all(|c| c.intent.validate().is_ok()), "{:?}", names(&r));
}

#[test]
fn apps_with_a_localized_name_match_both_names() {
    let mut a = app("calc.desktop", "Rechner", "Taschenrechner", &["rechnen"], None);
    a.name_default = "Calculator".into();
    a.generic_name_default = "Calculator Tool".into();
    a.keywords_default = vec!["calculate".into()];
    let list = [a];
    for input in ["rechner", "calculator", "taschenr", "calculate", "calc"] {
        let r = resolve(input, &Context::default(), &list, &[], None, NOW);
        assert_eq!(top(&r).target_id, "calc.desktop", "{input}");
        assert_eq!(top(&r).display, "Rechner · Launch", "angezeigt wird der lokalisierte Name");
    }
    // Desktop-ID als Feld
    let list = [app("org.gnome.Nautilus.desktop", "Dateien", "Dateiverwaltung", &[], None)];
    assert_eq!(top(&resolve("nautilus", &Context::default(), &list, &[], None, NOW)).target_id, "org.gnome.Nautilus.desktop");
}

#[test]
fn app_target_carries_the_parsed_argv_for_lua() {
    // Das Panel bekommt nie einen Shell-String, sondern die zerlegte argv samt Arbeitsordner und Terminal-Flag
    let mut editor = app("editor.desktop", "Text Editor", "", &[], None);
    editor.exec_argv = vec!["/usr/bin/editor".into(), "--new-window".into(), "a b".into()];
    editor.working_dir = Some(PathBuf::from("/srv/notes"));
    editor.terminal = true;
    let plain = app("plain.desktop", "Plain Tool", "", &[], None);
    let list = [editor, plain];
    let r = resolve("text editor", &Context::default(), &list, &[], None, NOW);
    assert_eq!(
        top(&r).target,
        Target::App {
            id: "editor.desktop".into(),
            argv: vec!["/usr/bin/editor".into(), "--new-window".into(), "a b".into()],
            working_dir: Some(PathBuf::from("/srv/notes")),
            terminal: true,
        }
    );
    // auf dem Draht: argv als Liste, working_dir nur wenn vorhanden, terminal immer
    let json = serde_json::to_value(top(&r)).unwrap();
    assert_eq!(
        json["target"],
        serde_json::json!({"kind": "app", "id": "editor.desktop", "argv": ["/usr/bin/editor", "--new-window", "a b"], "working_dir": "/srv/notes", "terminal": true})
    );
    let r = resolve("plain tool", &Context::default(), &list, &[], None, NOW);
    assert_eq!(
        serde_json::to_value(top(&r)).unwrap()["target"],
        serde_json::json!({"kind": "app", "id": "plain.desktop", "argv": ["plain"], "terminal": false})
    );
}

#[test]
fn windows_link_to_apps_by_app_id_desktop_id_or_executable() {
    let mut chrome = app("google-chrome.desktop", "Google Chrome", "Web Browser", &[], None);
    chrome.exec_argv = vec!["/usr/bin/google-chrome-stable".into()];
    let list = vec![app("org.mozilla.firefox.desktop", "Firefox", "", &[], None), chrome, app("custom.desktop", "Custom Thing", "", &[], Some("my-class"))];
    let ctx = Context::from_json(
        r#"{"windows":[
            {"id":1,"app_id":"firefox"},
            {"id":2,"app_id":"google-chrome"},
            {"id":3,"app_id":"my-class"},
            {"id":4,"app_id":"Unrelated"}
        ]}"#,
    )
    .unwrap();
    let displays: Vec<String> = resolve("", &ctx, &list, &[], None, NOW).candidates.iter().map(|c| c.display.clone()).collect();
    assert_eq!(displays, ["Firefox · Switch to window", "Google Chrome · Switch to window", "Custom Thing · Switch to window", "Unrelated · Switch to window"]);
    // das zugehörige Fenster steht vor dem Neustart, auch wenn der Neustart besser passt
    let r = resolve("google chrome", &ctx, &list, &[], None, NOW);
    assert_eq!(r.candidates[0].display, "Google Chrome · Switch to window");
    assert_eq!(r.candidates[1].display, "Google Chrome · Launch");
}

#[test]
fn running_window_stays_ahead_of_its_own_launch_even_with_frecency() {
    let t = TempDir::new("resolve-running");
    let f = frecency(&t, &[("launch_app", "firefox.desktop", 100, NOW - 1), ("focus_window", "firefox", 1, NOW - 99_999)]);
    let r = run_with("firefox", &ctx(), Some(&f));
    assert_eq!(r.candidates[0].intent.name(), "focus_window");
    assert_eq!(r.candidates[1].intent.name(), "launch_app");
    // und bei „launch“ ohne laufendes Fenster ist der Neustart natürlich vorn
    let ctx = Context::default();
    assert_eq!(top(&run_with("firefox", &ctx, Some(&f))).intent.name(), "launch_app");
}

#[test]
fn prefix_commands_are_matched_by_whole_words() {
    // „volumes 40“ ist kein „volume 40“, „gx foo“ keine Websuche, „claudex data“ kein Projektpräfix
    for input in ["volumes 40", "gx foo", "claudex data", "starter firefox", "tagx 3", "workspacex 3"] {
        let r = run(input);
        assert!(r.candidates.iter().all(|c| !matches!(c.intent, Intent::Volume { set: Some(_), .. } | Intent::WebSearch { .. })), "{input}: {:?}", names(&r));
        assert!(!r.confident, "{input}: {:?}", names(&r));
    }
}

#[test]
fn resolution_serializes_for_the_protocol() {
    let r = run("fire");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["confident"], true);
    let first = &json["candidates"][0];
    assert_eq!(first["intent"]["intent"], "focus_window");
    assert_eq!(first["target"], serde_json::json!({"kind": "window", "id": 100}));
    assert_eq!(first["class"], "run");
    assert_eq!(first["display"], "Firefox · Switch to window");
    assert_eq!(first["target_id"], "firefox");
    let launch = &json["candidates"][1];
    assert_eq!(launch["target"], serde_json::json!({"kind": "app", "id": "firefox.desktop", "argv": ["firefox"], "terminal": false}));
    let ws = serde_json::to_value(run("3")).unwrap();
    assert_eq!(ws["candidates"][0]["target"], serde_json::json!({"kind": "workspace", "id": 13}));
    let mv = serde_json::to_value(run("move to 2")).unwrap();
    assert_eq!(mv["candidates"][0]["target"], serde_json::json!({"kind": "move_window", "id": 200, "workspace": 12}));
    let none = serde_json::to_value(run("lock")).unwrap();
    assert_eq!(none["candidates"][0]["target"], serde_json::json!({"kind": "none"}));
    let project = serde_json::to_value(run("dataroom")).unwrap();
    assert_eq!(project["candidates"][0]["target"], serde_json::json!({"kind": "project", "path": "/home/git/blub/dataroom"}));
}

#[test]
fn input_is_not_changed_by_surrounding_whitespace_or_case() {
    for (a, b) in [("lock", "  LOCK  "), ("fire", "FIRE\t"), ("dataroom", " DataRoom "), ("volume 40", "  VOLUME   40  ")] {
        assert_eq!(names(&run(a)), names(&run(b)), "{a} / {b}");
        assert_eq!(run(a).confident, run(b).confident);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Leistung

/// 500 Programme, 200 Projekte, 100 Fenster.
fn big() -> (Vec<App>, Vec<Project>, Context) {
    let words = ["Studio", "Editor", "Player", "Manager", "Viewer", "Browser", "Terminal", "Calculator", "Monitor", "Recorder", "Designer", "Builder"];
    let apps: Vec<App> = (0..500)
        .map(|i| {
            let name = format!("{} {} {}", words[i % 12], words[(i * 7 + 3) % 12], i);
            let kw =
                [format!("{}{}", words[(i + 5) % 12].to_lowercase(), i % 9), "utility".into(), "tool".into(), format!("app{i}"), "gtk".into(), "office".into()];
            let mut a = app(
                &format!("org.example.app{i}.desktop"),
                &name,
                words[(i + 2) % 12],
                &kw.iter().map(String::as_str).collect::<Vec<_>>(),
                Some(&format!("app{i}")),
            );
            a.name_default = format!("{name} Default");
            a
        })
        .collect();
    let projects: Vec<Project> = (0..200)
        .map(|i| project(&format!("/home/git/group{}/project-{}-{}", i % 7, words[i % 12].to_lowercase(), i), Some(&format!("group{}", i % 7))))
        .collect();
    let windows: Vec<String> = (0..100)
        .map(|i| {
            format!(
                r#"{{"id":{},"app_id":"app{}","title":"Document {i} - {} - some longer window title with words","workspace_id":{}}}"#,
                1000 + i,
                i * 5,
                words[i % 12],
                i % 9 + 1
            )
        })
        .collect();
    let workspaces: Vec<String> = (1..=10).map(|n| format!(r#"{{"id":{n},"idx":{n},"name":"workspace name {n}","output":"DP-1"}}"#)).collect();
    let ctx = Context::from_json(&format!(r#"{{"workspaces":[{}],"windows":[{}],"focus":1003}}"#, workspaces.join(","), windows.join(","))).unwrap();
    (apps, projects, ctx)
}

/// Mittlere Laufzeit je Anruf in Millisekunden über alle Eingaben (nach einem Probelauf).
fn measure(inputs: &[&str], rounds: usize) -> f64 {
    let (apps, projects, ctx) = big();
    let t = TempDir::new("resolve-perf");
    let f = Frecency::open(t.join("frecency.json"), FrecencySettings::default());
    for i in (0..500).step_by(7) {
        f.record("launch_app", &format!("org.example.app{i}.desktop"), NOW - i as i64).unwrap();
    }
    for input in inputs {
        let _ = resolve(input, &ctx, &apps, &projects, Some(&f), NOW);
    }
    let start = Instant::now();
    for _ in 0..rounds {
        for input in inputs {
            let r = resolve(input, &ctx, &apps, &projects, Some(&f), NOW);
            std::hint::black_box(&r);
        }
    }
    start.elapsed().as_secs_f64() * 1000.0 / (rounds * inputs.len()) as f64
}

/// Eingaben von 20 Zeichen (und kürzere), wie sie beim Tippen entstehen.
const PERF_INPUTS: &[&str] = &[
    "studio editor 17 pla",
    "visual studio cod ed",
    "project-terminal-17",
    "document 5 - viewer",
    "calculator monitor",
    "e",
    "ed",
    "stu",
    "workspace name 3",
    "sperren",
];

#[test]
fn resolver_is_fast_enough_for_typing() {
    // Die Unit-Suite läuft im Release-Profil; die Schwelle hier ist zehnmal großzügiger als das Ziel (10 ms), damit
    // eine ausgelastete Maschine den Test nicht kippt. Die strenge Messung steht in `resolver_meets_the_10ms_target`.
    let avg = measure(PERF_INPUTS, 2);
    let limit = if cfg!(debug_assertions) { 2000.0 } else { 100.0 };
    assert!(avg < limit, "durchschnittlich {avg:.2} ms je Eingabe");
}

#[test]
#[ignore = "Messung: cargo test --release --lib palette::resolve::tests::resolver_meets_the_10ms_target -- --ignored --nocapture"]
fn resolver_meets_the_10ms_target() {
    let avg = measure(PERF_INPUTS, 40);
    let worst = PERF_INPUTS.iter().map(|i| (measure(&[i], 40), *i)).fold((0.0, ""), |a, b| if b.0 > a.0 { b } else { a });
    eprintln!("Resolver: durchschnittlich {avg:.3} ms, langsamste Eingabe {:?} mit {:.3} ms (500 Programme, 200 Projekte, 100 Fenster)", worst.1, worst.0);
    if cfg!(debug_assertions) {
        eprintln!("Debug-Profil: der Zielwert gilt nur für das Release-Profil, keine Prüfung");
        return;
    }
    assert!(worst.0 < 10.0, "{:?} braucht {:.3} ms", worst.1, worst.0);
}

// ---------------------------------------------------------------------------------------------------------------
// Absichten vom Modell (KI-Pfad): lokale Auflösung, nie ein erfundenes Ziel

fn ask(intent: Intent) -> Result<Vec<Candidate>, NoTarget> {
    ask_for(intent, "")
}

/// Wie [`ask`] mit der Eingabe `input` des Users (für Adressen vom Modell)
fn ask_for(intent: Intent, input: &str) -> Result<Vec<Candidate>, NoTarget> {
    resolve_intent(&intent, input, &ctx(), &apps(), &projects(), None, NOW)
}

#[test]
fn model_intents_resolve_against_real_targets() {
    let c = ask(Intent::FocusWindow { query: "firefox".into() }).unwrap();
    assert_eq!(c[0].target, Target::Window { id: 100 });
    assert_eq!(c[0].intent, Intent::FocusWindow { query: "Firefox".into() });
    assert!(c.iter().all(|x| x.intent.name() == "focus_window"), "nur Fenster, kein Programmstart");

    let c = ask(Intent::LaunchApp { query: "spotify".into() }).unwrap();
    assert_eq!(app_id(&c[0]), Some("spotify.desktop"));
    let c = ask(Intent::OpenProject { query: "dataroom".into() }).unwrap();
    assert_eq!(c[0].target, Target::Project { path: "/home/git/blub/dataroom".into() });
    assert_eq!(c[0].target_id, "/home/git/blub/dataroom");
    let c = ask(Intent::OpenFolder { query: "acme".into() }).unwrap();
    assert_eq!(c[0].intent.name(), "open_folder");

    // Arbeitsfläche als Nummer, als Name und als Name aus Ziffern; die Spalte des fokussierten Fensters
    assert_eq!(ask(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(3) }).unwrap()[0].target, Target::Workspace { id: 13 });
    assert_eq!(ask(Intent::ViewWorkspace { workspace: WorkspaceRef::Name("mail".into()) }).unwrap()[0].target, Target::Workspace { id: 13 });
    assert_eq!(ask(Intent::ViewWorkspace { workspace: WorkspaceRef::Name("2".into()) }).unwrap()[0].target, Target::Workspace { id: 12 });
    let c = ask(Intent::ColumnDisplay { display: ColumnDisplay::Tabbed }).unwrap();
    assert_eq!((c[0].target.clone(), c[0].display.as_str()), (Target::Window { id: 200 }, "Column · Tabbed"));

    // Fenster nach Begriff auf eine Arbeitsfläche, ohne Begriff das fokussierte
    let c = ask(Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Name("music".into()), query: Some("firefox".into()) }).unwrap();
    assert_eq!(
        (c[0].target.clone(), c[0].intent.clone()),
        (Target::MoveWindow { id: 100, workspace: 14 }, Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Name("music".into()), query: None })
    );
    let c = ask(Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(2), query: None }).unwrap();
    assert_eq!(c[0].target, Target::MoveWindow { id: 200, workspace: 12 });
    let c = ask(Intent::ToggleWindow { what: WindowToggle::Floating }).unwrap();
    assert_eq!((c[0].target.clone(), c[0].display.as_str()), (Target::Window { id: 200 }, "Toggle floating"));
    let c = ask(Intent::CloseWindow { query: Some("spotify".into()) }).unwrap();
    assert_eq!((c[0].target.clone(), c[0].class), (Target::Window { id: 300 }, Class::Confirm));
}

#[test]
fn model_intents_without_a_target_give_one_candidate() {
    let one = |i: Intent| {
        let c = ask(i.clone()).unwrap();
        assert_eq!(c.len(), 1, "{i:?}");
        assert_eq!(c[0].target, Target::None, "{i:?}");
        c.into_iter().next().unwrap()
    };
    assert_eq!(one(Intent::Lock {}).display, "Lock screen");
    assert_eq!(one(Intent::Media { action: MediaKey::Next }).target_id, "next");
    assert_eq!(one(Intent::Screenshot { mode: ScreenshotMode::Area }).intent.name(), "screenshot");
    assert_eq!(one(Intent::ShowWidget { widget: Widget::Emoji }).display, "Emoji picker");
    assert_eq!(one(Intent::Reboot {}).class, Class::Confirm);
    assert_eq!(one(Intent::Volume { set: Some(30), step: None, mute: None }).display, "Set volume to 30%");
    let up = one(Intent::Volume { set: None, step: Some(15), mute: None });
    assert_eq!((up.display.as_str(), up.target_id.as_str()), ("Volume up by 15 points", "up"));
    assert_eq!(one(Intent::Volume { set: None, step: Some(-7), mute: None }).display, "Volume down by 7 points");
    assert_eq!(one(Intent::WebSearch { query: "rust borrow".into() }).intent, Intent::WebSearch { query: "rust borrow".into() });
    let url = Intent::OpenUrl { url: "https://example.org/a".into() };
    let c = ask_for(url.clone(), "open example.org/a").unwrap();
    assert_eq!((c.len(), c[0].target_id.as_str(), c[0].target.clone(), c[0].intent.clone()), (1, "open_url", Target::None, url));
    assert_eq!(one(Intent::Ask { question: "Why is the sky blue?".into() }).class, Class::Ai);
    let t = one(Intent::TextAction { action: "explain".into() });
    assert_eq!((t.target_id.as_str(), t.class), ("explain", Class::Ai));
    assert!(t.display.ends_with("· Text action"), "{}", t.display);
    assert_eq!(one(Intent::ExplainClipboard {}).target_id, "explain_clipboard");
    assert_eq!(one(Intent::DictateSettings { page: Some(SettingsPage::Ai) }).display, "Dictate settings › Ai");
    assert_eq!(one(Intent::PreviousWorkspace {}).intent, Intent::PreviousWorkspace {});
    assert_eq!(one(Intent::PreviousWorkspace {}).target_id, "previous_workspace");
}

#[test]
fn model_intents_never_invent_a_target() {
    assert_eq!(ask(Intent::FocusWindow { query: "thunderbird".into() }), Err(NoTarget::Window));
    assert_eq!(ask(Intent::LaunchApp { query: "blender".into() }), Err(NoTarget::App));
    assert_eq!(ask(Intent::OpenProject { query: "kubernetes".into() }), Err(NoTarget::Project));
    assert_eq!(ask(Intent::ViewWorkspace { workspace: WorkspaceRef::Index(7) }), Err(NoTarget::Workspace), "nur fünf im Kontext");
    assert_eq!(ask(Intent::ViewWorkspace { workspace: WorkspaceRef::Name("chat".into()) }), Err(NoTarget::Workspace));
    assert_eq!(ask(Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(9), query: None }), Err(NoTarget::Workspace));
    assert_eq!(ask(Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(9), query: Some("firefox".into()) }), Err(NoTarget::Workspace));
    assert_eq!(ask(Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(2), query: Some("thunderbird".into()) }), Err(NoTarget::Window));
    // ohne fokussiertes Fenster
    let none = Context::from_json(r#"{"workspaces":[{"id":1,"idx":1,"name":"a"}]}"#).unwrap();
    for intent in [
        Intent::ToggleWindow { what: WindowToggle::Fullscreen },
        Intent::ColumnDisplay { display: ColumnDisplay::Normal },
        Intent::MoveWindowToWorkspace { workspace: WorkspaceRef::Index(1), query: None },
    ] {
        assert_eq!(resolve_intent(&intent, "", &none, &apps(), &projects(), None, NOW), Err(NoTarget::FocusedWindow), "{intent:?}");
    }
    assert_eq!(NoTarget::Workspace.to_string(), "No match for a workspace");
    // Meldungen ohne Suchbegriff
    assert_eq!(NoTarget::Window.to_string(), "No match for a window");
    assert_eq!(NoTarget::App.to_string(), "No match for an application");
    assert_eq!(NoTarget::Url.to_string(), "No match for a web address");
}

#[test]
fn a_model_url_is_taken_only_with_its_host_in_the_input() {
    let url = |u: &str| Intent::OpenUrl { url: u.into() };
    let accepted = |u: &str, input: &str| {
        let c = ask_for(url(u), input).unwrap_or_else(|e| panic!("{u:?} bei {input:?}: {e}"));
        assert_eq!((c.len(), c[0].intent.clone(), c[0].target_id.as_str()), (1, url(u), "open_url"), "{u:?} bei {input:?}");
    };
    let refused = |u: &str, input: &str| assert_eq!(ask_for(url(u), input), Err(NoTarget::Url), "{u:?} bei {input:?}");

    // der Host steht in der Eingabe: als Adresse, im Satz, mit Pfad, Port, Schema; Schreibung und `www.` egal
    accepted("https://example.org/a?b=c", "öffne example.org bitte");
    accepted("https://example.org/", "open https://example.org/x");
    accepted("https://example.org/", "Open EXAMPLE.org.");
    accepted("https://www.example.org/", "go to example.org");
    accepted("https://example.org/", "go to www.Example.org/docs");
    accepted("http://example.org:8080/", "example.org:8080");
    accepted("https://sub.example.org/", "mail: me@sub.example.org");
    accepted("http://192.168.0.1/admin", "http://192.168.0.1:8080/x");
    // internationale Namen: der Host der Adresse ist Punycode, die Eingabe nennt ihn mit Umlauten
    accepted("https://münchen.de/", "geh auf münchen.de");
    accepted("https://xn--mnchen-3ya.de/", "geh auf MÜNCHEN.de");

    // erfunden: kein Host in der Eingabe
    refused("https://example.org/", "open the example website");
    refused("https://www.heise.de/", "öffne heise");
    refused("https://example.org/", "");
    // nur ein Teil eines Worts oder eines anderen Hosts zählt nicht
    refused("https://evil.com/", "go to notevil.com");
    refused("https://evil.com/", "go to evil.com.example.org");
    refused("https://example.org/", "go to mail.example.org");
    refused("https://sub.example.org/", "go to example.org");
    refused("https://example.org/", "go to example.org.evil.net");
    refused("https://example.org/", "go to example-org");
    // Adresse ohne Host oder ungültig: nie angenommen (die Validierung der Absicht lässt sie ohnehin nicht durch)
    refused("https://[::1]/", "https://[::1]/");
    refused("not a url", "not a url");
}

#[test]
fn mentions_host_matches_whole_words_only() {
    assert!(mentions_host("see example.org, then", "example.org"));
    assert!(mentions_host("(example.org)", "www.example.org"));
    assert!(mentions_host("a www.example.org b", "example.org"));
    assert!(!mentions_host("see example.org", ""));
    assert!(!mentions_host("see example.org", "www."));
    assert!(!mentions_host("xexample.org", "example.org"));
    assert!(!mentions_host("example.orgx", "example.org"));
}

// ---------------------------------------------------------------------------------------------------------------
// Diagnose (PLAN-diagnose.md, Aufgabe 9)

fn diagnose(profile: Option<DiagProfile>) -> Intent {
    Intent::Diagnose { profile, unit: None, container: None }
}

fn with_term(profile: DiagProfile, unit: Option<&str>, container: Option<&str>) -> Intent {
    Intent::Diagnose { profile: Some(profile), unit: unit.map(Into::into), container: container.map(Into::into) }
}

#[test]
fn diagnose_keywords_in_german_and_english_open_the_right_profile() {
    let cases: &[(&str, Option<DiagProfile>)] = &[
        ("diagnose", None),
        ("Diagnose", None),
        ("warum", None),
        ("why", None),
        ("troubleshoot", None),
        ("fehlersuche", None),
        ("diagnose this computer", None),
        ("diagnose rechner", None),
        ("crash", Some(DiagProfile::LastCrash)),
        ("absturz", Some(DiagProfile::LastCrash)),
        ("diagnose absturz", Some(DiagProfile::LastCrash)),
        ("letzter absturz", Some(DiagProfile::LastCrash)),
        ("last crash", Some(DiagProfile::LastCrash)),
        ("why did it crash", Some(DiagProfile::LastCrash)),
        ("warum abgestürzt", Some(DiagProfile::LastCrash)),
        ("warum abgestuerzt", Some(DiagProfile::LastCrash)),
        ("warum ist der rechner abgestürzt", Some(DiagProfile::LastCrash)),
        ("eingefroren", Some(DiagProfile::LastCrash)),
        ("freeze", Some(DiagProfile::LastCrash)),
        ("diagnose now", Some(DiagProfile::Now)),
        ("diagnose jetzt", Some(DiagProfile::Now)),
        ("health check", Some(DiagProfile::Now)),
        ("failed", Some(DiagProfile::Unit)),
        ("failed units", Some(DiagProfile::Unit)),
        ("fehlgeschlagene dienste", Some(DiagProfile::Unit)),
        ("diagnose docker", Some(DiagProfile::Docker)),
        ("diagnose container", Some(DiagProfile::Docker)),
        ("docker kaputt", Some(DiagProfile::Docker)),
        ("warum ist docker kaputt", Some(DiagProfile::Docker)),
        ("diagnose network", Some(DiagProfile::Network)),
        ("diagnose netzwerk", Some(DiagProfile::Network)),
        ("kein internet", Some(DiagProfile::Network)),
        ("no internet", Some(DiagProfile::Network)),
        ("diagnose heat", Some(DiagProfile::Heat)),
        ("überhitzt", Some(DiagProfile::Heat)),
        ("ueberhitzt", Some(DiagProfile::Heat)),
        ("too hot", Some(DiagProfile::Heat)),
        ("zu heiß", Some(DiagProfile::Heat)),
    ];
    for (input, profile) in cases {
        let r = run(input);
        assert_eq!(top(&r).intent, diagnose(*profile), "{input}: {:?}", names(&r));
        assert_eq!(top(&r).class, Class::Ai, "{input}");
        assert_eq!(top(&r).target, Target::None, "{input}");
        assert!(r.confident, "{input}: {:?}", names(&r));
    }
}

#[test]
fn diagnose_display_and_target_ids_come_from_the_catalog_not_from_the_input() {
    let r = run("diagnose absturz");
    assert_eq!(top(&r).display, "Diagnose · Last crash");
    assert_eq!(top(&r).target_id, "last_crash");
    let r = run("warum");
    assert_eq!(top(&r).display, "Diagnose this computer");
    assert_eq!(top(&r).target_id, "diagnose");
    // jedes Schlüsselwort der Diagnose hat eine eigene, feste Ziel-ID
    let ids: HashSet<&str> = keywords::KEYWORDS.iter().filter(|k| matches!((k.intent)(), Intent::Diagnose { .. })).map(|k| k.id).collect();
    assert_eq!(ids, ["diagnose", "last_crash", "now", "unit", "docker", "network", "heat"].into());
}

#[test]
fn diagnose_with_a_search_term_for_a_unit_or_a_container() {
    let r = run("diagnose unit cups");
    assert_eq!(top(&r).intent, with_term(DiagProfile::Unit, Some("cups"), None), "{:?}", names(&r));
    assert_eq!(top(&r).display, "Diagnose unit cups");
    assert_eq!(top(&r).target_id, "unit", "the term is never the target ID");
    assert!(r.confident, "{:?}", names(&r));
    for input in ["diagnose service docker.service", "Diagnose Dienst docker.service", "diagnose failed docker.service", "failed unit docker.service"] {
        let r = run(input);
        assert_eq!(top(&r).intent, with_term(DiagProfile::Unit, Some("docker.service"), None), "{input}: {:?}", names(&r));
    }
    for input in ["diagnose container dataroom", "diagnose docker dataroom", "Diagnose Container dataroom"] {
        let r = run(input);
        assert_eq!(top(&r).intent, with_term(DiagProfile::Docker, None, Some("dataroom")), "{input}: {:?}", names(&r));
        assert_eq!(top(&r).display, "Diagnose container dataroom");
        assert_eq!(top(&r).target_id, "container");
        assert_eq!(top(&r).class, Class::Ai);
    }
}

#[test]
fn diagnose_terms_that_cannot_be_names_give_no_such_candidate() {
    // Leerzeichen, Sonderzeichen und ein führendes `-` gelten nie als Name einer Unit oder eines Containers
    for input in
        ["diagnose unit my app", "diagnose unit -x", "diagnose unit --help", "diagnose container ../etc", "diagnose container a;b", "diagnose service $(id)"]
    {
        let r = run(input);
        assert!(
            r.candidates.iter().all(|c| !matches!(&c.intent, Intent::Diagnose { unit, container, .. } if unit.is_some() || container.is_some())),
            "{input}: {:?}",
            names(&r)
        );
    }
}

#[test]
fn diagnose_is_not_offered_while_the_diagnosis_is_off() {
    let off = Features { diagnose: false, ..Features::default() };
    let run_off = |input: &str| resolve_with(off, input, &ctx(), &apps(), &projects(), None, NOW);
    for input in [
        "diagnose",
        "warum",
        "crash",
        "absturz",
        "failed",
        "diagnose docker",
        "diagnose unit cups",
        "diagnose container dataroom",
        "kein internet",
        "überhitzt",
    ] {
        let r = run_off(input);
        assert!(r.candidates.iter().all(|c| c.intent.name() != "diagnose"), "{input}: {:?}", names(&r));
        let on = run(input);
        assert!(on.candidates.iter().any(|c| c.intent.name() == "diagnose"), "{input}: {:?}", names(&on));
    }
    // nichts anderes ändert sich
    for input in ["lock", "ff", "tabbed", "3"] {
        assert_eq!(names(&run_off(input)), names(&run(input)), "{input}");
    }
    // und leere Eingabe mit gemerkter Häufigkeit: auch dort nicht
    let t = TempDir::new("diagnose-off-frecency");
    let f = Frecency::open(t.join("frecency.json"), FrecencySettings::default());
    f.record("diagnose", "last_crash", NOW).unwrap();
    f.record("lock", "lock", NOW).unwrap();
    let with_f = |features: Features| resolve_with(features, "", &ctx(), &apps(), &projects(), Some(&f), NOW);
    assert!(with_f(Features::default()).candidates.iter().any(|c| c.intent.name() == "diagnose"));
    assert!(with_f(off).candidates.iter().all(|c| c.intent.name() != "diagnose"));
    assert!(with_f(off).candidates.iter().any(|c| c.intent.name() == "lock"));
}

#[test]
fn widgets_are_offered_only_when_their_program_was_found() {
    let clip = |w: &Candidate| matches!(w.intent, Intent::ShowWidget { widget: Widget::ClipboardHistory });
    let emoji = |w: &Candidate| matches!(w.intent, Intent::ShowWidget { widget: Widget::Emoji });
    let no_clip = Features { clipboard_history: false, ..Features::default() };
    let no_emoji = Features { emoji: false, ..Features::default() };
    let run_with = |f: Features, input: &str| resolve_with(f, input, &ctx(), &apps(), &projects(), None, NOW);
    for input in ["clipboard", "zwischenablage", "clipboard verlauf"] {
        assert!(run(input).candidates.iter().any(clip), "{input}: {:?}", names(&run(input)));
        assert!(run_with(no_clip, input).candidates.iter().all(|c| !clip(c)), "{input}: {:?}", names(&run_with(no_clip, input)));
        assert!(run_with(no_emoji, input).candidates.iter().any(clip), "{input}");
    }
    for input in ["emoji", "smiley"] {
        assert!(run(input).candidates.iter().any(emoji), "{input}: {:?}", names(&run(input)));
        assert!(run_with(no_emoji, input).candidates.iter().all(|c| !emoji(c)), "{input}: {:?}", names(&run_with(no_emoji, input)));
        assert!(run_with(no_clip, input).candidates.iter().any(emoji), "{input}");
    }
    // nichts anderes ändert sich
    let none = Features { clipboard_history: false, emoji: false, ..Features::default() };
    for input in ["lock", "ff", "tabbed", "3", "diagnose"] {
        assert_eq!(names(&run_with(none, input)), names(&run(input)), "{input}");
    }
    // leere Eingabe mit gemerkter Häufigkeit: eine fehlende Anzeige steht auch dort nicht
    let t = TempDir::new("widgets-missing-frecency");
    let f = Frecency::open(t.join("frecency.json"), FrecencySettings::default());
    f.record("show_widget", "emoji", NOW).unwrap();
    f.record("show_widget", "clipboard_history", NOW).unwrap();
    f.record("lock", "lock", NOW).unwrap();
    let with_f = |features: Features| resolve_with(features, "", &ctx(), &apps(), &projects(), Some(&f), NOW);
    assert!(with_f(Features::default()).candidates.iter().any(emoji));
    assert!(with_f(Features::default()).candidates.iter().any(clip));
    assert!(with_f(none).candidates.iter().all(|c| !emoji(c) && !clip(c)));
    assert!(with_f(none).candidates.iter().any(|c| c.intent.name() == "lock"));
}

#[test]
fn a_widget_from_the_model_says_when_its_program_is_missing() {
    let resolve_model = |features: Features, widget: Widget| {
        resolve_intent_with(features, &Intent::ShowWidget { widget }, "zeig emojis", &ctx(), &apps(), &projects(), None, NOW)
    };
    for widget in Widget::ALL {
        let c = resolve_model(Features::default(), *widget).unwrap_or_else(|e| panic!("{widget:?}: {e}"));
        assert_eq!(c.len(), 1, "{widget:?}");
        assert_eq!(c[0].intent, Intent::ShowWidget { widget: *widget });
        let off = Features { clipboard_history: false, emoji: false, ..Features::default() };
        assert_eq!(resolve_model(off, *widget), Err(NoTarget::WidgetMissing(*widget)));
    }
    // nur die fehlende Anzeige ist betroffen
    assert!(resolve_model(Features { emoji: false, ..Features::default() }, Widget::ClipboardHistory).is_ok());
    assert_eq!(
        NoTarget::WidgetMissing(Widget::ClipboardHistory).to_string(),
        "No clipboard history found: install Noctalia or check the command in Settings › Palette"
    );
    assert_eq!(
        NoTarget::WidgetMissing(Widget::Emoji).to_string(),
        "No emoji picker found: install rofi with rofi-emoji or check the command in Settings › Palette"
    );
}

#[test]
fn diagnose_from_the_model_resolves_without_a_probe_or_says_it_is_off() {
    let resolve_model =
        |features: Features, intent: &Intent| resolve_intent_with(features, intent, "irgendwas mit cups", &ctx(), &apps(), &projects(), None, NOW);
    for (intent, display, id) in [
        (diagnose(None), "Diagnose this computer", "diagnose"),
        (diagnose(Some(DiagProfile::Heat)), "Diagnose · Heat", "heat"),
        (with_term(DiagProfile::Unit, Some("cups"), None), "Diagnose unit cups", "unit"),
        (Intent::Diagnose { profile: None, unit: Some("cups".into()), container: None }, "Diagnose unit cups", "unit"),
        (Intent::Diagnose { profile: None, unit: None, container: Some("dataroom".into()) }, "Diagnose container dataroom", "container"),
    ] {
        let c = resolve_model(Features::default(), &intent).unwrap_or_else(|e| panic!("{intent:?}: {e}"));
        assert_eq!(c.len(), 1, "{intent:?}");
        assert_eq!(c[0].intent, intent);
        assert_eq!(c[0].class, Class::Ai);
        assert_eq!(c[0].target, Target::None);
        assert_eq!(c[0].target_id, id, "{intent:?}");
        assert_eq!(c[0].display, display, "{intent:?}");
        assert_eq!(resolve_model(Features { diagnose: false, ..Features::default() }, &intent), Err(NoTarget::DiagnosisOff));
    }
    assert_eq!(NoTarget::DiagnosisOff.to_string(), "Diagnosis is off (Settings › Diagnose)");
}
