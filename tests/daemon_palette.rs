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

//! Befehlspalette im Daemon (`pal_resolve`, `pal_chosen`, `pal_sources_refresh`, `pal_frecency_clear`) mit Fixture-
//! Ordnern in einem Temp-Verzeichnis (`.desktop`-Einträge, Git-Projekte, ein Werkzeug für `TryExec`) und einem
//! Fixture-Kontext: Auflösung, Häufigkeit wirkt auf die Reihenfolge, ungültige Ziele werden abgelehnt, `enabled = false`,
//! `PATH` und Desktop aus der Anfrage, Konfigurationsänderungen, Grenzen und dass Eingabe und Kontext nie im Log stehen.
//! Dazu der KI-Pfad `pal_ask` gegen einen Ollama-Mock: Absicht → lokale Kandidaten, danach `pal_chosen`, kein Ziel
//! erfunden, kein Kontext im Anfragekörper, Abbruch beim Auflegen.
mod daemon_common;
mod mock_http;

use daemon_common::*;
use dictate::config::Config;
use dictate::palette::sources::DesktopEnv;
use mock_http::{Mock, Resp};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// Fixture: Programme unter `share/applications`, Projekte unter `git`, Werkzeuge unter `bin`.
struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        Fx { root: temp_dir() }
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn app(&self, id: &str, name: &str, extra: &str) {
        let exec = name.to_lowercase().replace(' ', "-");
        self.write(&format!("share/applications/{id}"), &format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}"));
    }

    fn project(&self, rel: &str) -> PathBuf {
        let p = self.root.join("git").join(rel);
        std::fs::create_dir_all(p.join(".git")).unwrap();
        p
    }

    /// Ausführbares Werkzeug unter `bin`; liefert das Verzeichnis.
    fn tool(&self, name: &str) -> PathBuf {
        let p = self.write(&format!("bin/{name}"), "#!/bin/sh\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        self.root.join("bin")
    }

    fn env(&self) -> DesktopEnv {
        DesktopEnv { data_home: None, data_dirs: vec![self.root.join("share")], ..DesktopEnv::default() }
    }

    fn cfg(&self) -> Config {
        let mut c = quick_cfg();
        c.palette.project_roots = vec![self.root.join("git").display().to_string()];
        c
    }

    fn start(&self, cfg: Config) -> Harness {
        self.start_seeded(cfg, &[])
    }

    /// Wie `start`, mit Dateien im Zustandsordner, die schon vor dem Start des Daemons dort liegen.
    fn start_seeded(&self, cfg: Config, files: &[(&str, &str)]) -> Harness {
        let ctl = Arc::new(Ctl::default());
        *ctl.palette_env.lock().unwrap() = Some(self.env());
        *ctl.state_files.lock().unwrap() = files.iter().map(|(n, c)| (n.to_string(), c.to_string())).collect();
        let h = Harness::start_with(cfg, ctl);
        // Der Socket ist gebunden, bevor der Start die alte Häufigkeit abholt (`daemon.rs`, `adopt_legacy`); angenommen
        // wird erst danach. Eine Antwort heißt also: der Start ist fertig, die Dateien sind in ihrem Endzustand. Ohne
        // diese Rundreise prüfte der Test zu früh (im CI-Container jedes Mal, lokal nie gesehen; U5c, strace).
        assert_eq!(h.req(r#"{"cmd":"status"}"#)["ok"], true);
        h
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Kontext wie ihn `dictate-ui` aus niri baut: drei Arbeitsflächen (die dritte die leere am Ende), zwei Fenster.
fn context() -> Value {
    json!({
        "workspaces": [
            {"id": 7, "idx": 1, "name": "web", "output": "DP-1"},
            {"id": 8, "idx": 2, "output": "DP-1", "active": true, "focused": true},
            {"id": 9, "idx": 3, "output": "DP-1"},
        ],
        "windows": [
            {"id": 101, "app_id": "Alacritty", "title": "~/git — zsh", "workspace_id": 8, "focused": true},
            {"id": 100, "app_id": "Firefox", "title": "Rust docs", "workspace_id": 7},
        ],
        "focus": 101,
    })
}

fn resolve(h: &Harness, input: &str, ctx: Option<&Value>, path: Option<&str>, desktop: Option<&str>) -> Value {
    let mut v = json!({"cmd": "pal_resolve", "input": input});
    if let Some(c) = ctx {
        v["context"] = c.clone();
    }
    if let Some(p) = path {
        v["path"] = json!(p);
    }
    if let Some(d) = desktop {
        v["desktop"] = json!(d);
    }
    h.req(&v.to_string())
}

fn chosen(h: &Harness, intent: &str, target_id: &str) -> Value {
    h.req(&json!({"cmd": "pal_chosen", "intent": intent, "target_id": target_id}).to_string())
}

/// `intent:target_id` je Kandidat
fn ids(r: &Value) -> Vec<String> {
    r["candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("keine Kandidaten: {r}"))
        .iter()
        .map(|c| format!("{}:{}", c["intent"]["intent"].as_str().unwrap(), c["target_id"].as_str().unwrap()))
        .collect()
}

fn error_of(r: &Value) -> &str {
    assert_eq!(r["ok"], false, "{r}");
    r["error"].as_str().unwrap_or_default()
}

fn state_file(h: &Harness) -> PathBuf {
    h.dir.join("state/dictate/frecency.json")
}

#[test]
fn resolves_windows_workspaces_apps_and_projects_from_a_fixture_context() {
    let fx = Fx::new();
    fx.app("firefox.desktop", "Firefox", "");
    fx.app("calc.desktop", "Calculator", "");
    let dataroom = fx.project("dataroom");
    fx.project("grp/blub");
    let h = fx.start(fx.cfg());
    let ctx = context();

    let r = resolve(&h, "fire", Some(&ctx), None, None);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(ids(&r)[..2], ["focus_window:firefox", "launch_app:firefox.desktop"]);
    assert_eq!(r["candidates"][0]["target"], json!({"kind": "window", "id": 100}));
    assert_eq!(r["candidates"][1]["target"], json!({"kind": "app", "id": "firefox.desktop", "argv": ["firefox"], "terminal": false}));
    assert_eq!(r["confident"], true);
    assert_eq!(r["web_search_url"], "https://duckduckgo.com/?q={query}");

    let r = resolve(&h, "3", Some(&ctx), None, None);
    assert_eq!(ids(&r)[0], "view_workspace:3");
    assert_eq!(r["candidates"][0]["target"], json!({"kind": "workspace", "id": 9}));
    assert_eq!(r["confident"], true);
    let r = resolve(&h, "move to web", Some(&ctx), None, None);
    assert_eq!(ids(&r)[0], "move_window_to_workspace:web");
    assert_eq!(r["candidates"][0]["target"], json!({"kind": "move_window", "id": 101, "workspace": 7}));

    let r = resolve(&h, "dataroom", Some(&ctx), None, None);
    assert_eq!(ids(&r)[0], format!("open_project:{}", dataroom.display()));
    assert_eq!(r["candidates"][0]["target"], json!({"kind": "project", "path": dataroom.display().to_string()}));
    // der Ordner einzeln: eigenes Präfix
    let r = resolve(&h, "folder dataroom", Some(&ctx), None, None);
    assert_eq!(ids(&r)[0], format!("open_folder:{}", dataroom.display()));

    // ohne Kontext: Programme und feste Regeln gehen trotzdem
    let r = resolve(&h, "lock", None, None, None);
    assert_eq!(ids(&r)[0], "lock:lock");
    assert_eq!(r["confident"], true);
    assert_eq!(ids(&resolve(&h, "calc", None, None, None))[0], "launch_app:calc.desktop");

    // Unsinn und die leere Eingabe: keine sichere Auswahl; leer zeigt die laufenden Fenster
    let r = resolve(&h, "zzqxjw", Some(&ctx), None, None);
    assert_eq!(r["confident"], false);
    let r = resolve(&h, "", Some(&ctx), None, None);
    assert!(ids(&r).contains(&"focus_window:firefox".to_string()), "{r}");
    // jede Antwort ist eine Zeile mit den Feldern des Protokolls
    for c in r["candidates"].as_array().unwrap() {
        for key in ["intent", "target", "display", "score", "class", "target_id"] {
            assert!(c.get(key).is_some(), "{key} fehlt: {c}");
        }
    }
}

#[test]
fn chosen_changes_the_order_of_equal_matches_and_the_file_is_private() {
    let fx = Fx::new();
    fx.app("terminal-aaa.desktop", "Terminal Aaa", "");
    fx.app("terminal-bbb.desktop", "Terminal Bbb", "");
    let h = fx.start(fx.cfg());
    let before = ids(&resolve(&h, "terminal", None, None, None));
    assert_eq!(before.len(), 2, "{before:?}");
    // der zweite wird gewählt und steht danach vorn
    let (first, second) = (before[0].clone(), before[1].clone());
    let (intent, target) = second.split_once(':').unwrap();
    let r = chosen(&h, intent, target);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["recorded"], true);
    let after = ids(&resolve(&h, "terminal", None, None, None));
    assert_eq!(after, [second.clone(), first.clone()], "Häufigkeit sortiert Gleichwertiges um");
    // auch bei leerer Eingabe: häufig gewählte Ziele zuerst
    assert_eq!(ids(&resolve(&h, "", None, None, None))[0], second);
    // Datei: 0600 im 0700-Ordner, nur die Ziel-ID, nie Eingabetext
    let path = state_file(&h);
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], 1);
    let keys: Vec<&String> = doc["entries"].as_object().unwrap().keys().collect();
    assert_eq!(keys, [&second], "{doc}");
    assert_eq!(doc["entries"][&second]["count"], 1);
}

#[test]
fn the_frecency_file_next_to_the_history_is_read_at_the_first_use() {
    let fx = Fx::new();
    fx.app("terminal-aaa.desktop", "Terminal Aaa", "");
    fx.app("terminal-bbb.desktop", "Terminal Bbb", "");
    let h = fx.start(fx.cfg());
    // eine Datei aus einer früheren Sitzung, noch vor dem ersten Zugriff abgelegt: Daemon und Datei überleben einander
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let doc = json!({"version": 1, "entries": {"launch_app:terminal-bbb.desktop": {"count": 5, "last": now}}});
    std::fs::create_dir_all(state_file(&h).parent().unwrap()).unwrap();
    std::fs::write(state_file(&h), doc.to_string()).unwrap();
    assert_eq!(ids(&resolve(&h, "terminal", None, None, None))[0], "launch_app:terminal-bbb.desktop");
    // ein weiterer Treffer zählt weiter, die Datei bleibt gültig
    assert_eq!(chosen(&h, "launch_app", "terminal-bbb.desktop")["recorded"], true);
    let back: Value = serde_json::from_str(&std::fs::read_to_string(state_file(&h)).unwrap()).unwrap();
    assert_eq!(back["entries"]["launch_app:terminal-bbb.desktop"]["count"], 6);
}

/// Farbdatei der Noctalia-Vorlage (`noctalia/dictate.json.tmpl`, alle Rollen), wie sie `dictate-ui` liest.
const NOCTALIA_PALETTE: &str = include_str!("wayland/palette-tokyo.json");

#[test]
fn a_chosen_target_leaves_the_noctalia_color_file_alone() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let h = fx.start(fx.cfg());
    // Noctalia schreibt die Farben nach `palette.json` im selben Ordner wie die Häufigkeit (`PLAN-wayland.md` 6h, V1)
    let colors = h.dir.join("state/dictate/palette.json");
    std::fs::create_dir_all(colors.parent().unwrap()).unwrap();
    std::fs::write(&colors, NOCTALIA_PALETTE).unwrap();
    resolve(&h, "alpha", None, None, None);
    assert_eq!(chosen(&h, "launch_app", "alpha.desktop")["recorded"], true);
    assert_eq!(std::fs::read_to_string(&colors).unwrap(), NOCTALIA_PALETTE, "die Farbdatei bleibt unverändert");
    assert!(dictate::ui::palette::Palette::parse(NOCTALIA_PALETTE).is_ok());
    // die Wahl steht in der eigenen Datei
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(state_file(&h)).unwrap()).unwrap();
    assert_eq!(doc["entries"]["launch_app:alpha.desktop"]["count"], 1, "{doc}");
}

#[test]
fn the_start_moves_an_old_frecency_file_and_keeps_a_color_file() {
    let fx = Fx::new();
    fx.app("terminal-aaa.desktop", "Terminal Aaa", "");
    fx.app("terminal-bbb.desktop", "Terminal Bbb", "");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let old = json!({"version": 1, "entries": {"launch_app:terminal-bbb.desktop": {"count": 5, "last": now}}}).to_string();
    // bis W6 lag die Häufigkeit unter dem Namen der Farbdatei; der Start holt sie ab, noch vor jedem Palettenbefehl
    let h = fx.start_seeded(fx.cfg(), &[("palette.json", &old)]);
    let colors = h.dir.join("state/dictate/palette.json");
    assert_eq!(std::fs::metadata(&colors).unwrap().len(), 0, "die alte Häufigkeit ist umgezogen, die Datei geleert (L54)");
    assert_eq!(std::fs::read_to_string(state_file(&h)).unwrap(), old, "byte-gleich");
    assert_eq!(std::fs::metadata(state_file(&h)).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(ids(&resolve(&h, "terminal", None, None, None))[0], "launch_app:terminal-bbb.desktop", "und gilt weiter");
    drop(h);

    // auch mit ausgeschalteter Häufigkeit (Lesart L48): sonst überschriebe Noctalia sie
    let mut off = fx.cfg();
    off.palette.frecency = false;
    let h = fx.start_seeded(off, &[("palette.json", &old)]);
    assert_eq!(std::fs::read_to_string(state_file(&h)).unwrap(), old);
    drop(h);

    // eine Farbdatei bleibt, wie sie ist; es entsteht keine Häufigkeit
    let h = fx.start_seeded(fx.cfg(), &[("palette.json", NOCTALIA_PALETTE)]);
    assert_eq!(std::fs::read_to_string(h.dir.join("state/dictate/palette.json")).unwrap(), NOCTALIA_PALETTE);
    assert!(!state_file(&h).exists());
    drop(h);

    // beide vorhanden: nichts verschieben
    let newer = json!({"version": 1, "entries": {"launch_app:terminal-aaa.desktop": {"count": 2, "last": now}}}).to_string();
    let h = fx.start_seeded(fx.cfg(), &[("palette.json", &old), ("frecency.json", &newer)]);
    assert_eq!(std::fs::read_to_string(h.dir.join("state/dictate/palette.json")).unwrap(), old);
    assert_eq!(std::fs::read_to_string(state_file(&h)).unwrap(), newer);
    assert_eq!(ids(&resolve(&h, "terminal", None, None, None))[0], "launch_app:terminal-aaa.desktop", "es gilt die neue Datei");
}

/// Entscheidung 21: liegen beim Start eine alte Häufigkeit unter `palette.json` und `frecency.json` (dann zieht nichts
/// um), löscht `pal_frecency_clear` beide; ein Neustart holt nichts zurück. Eine Farbdatei bleibt unberührt.
#[test]
fn clear_also_removes_an_old_frecency_and_keeps_a_color_file() {
    let fx = Fx::new();
    fx.app("terminal-aaa.desktop", "Terminal Aaa", "");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let old = json!({"version": 1, "entries": {"launch_app:terminal-aaa.desktop": {"count": 5, "last": now}}}).to_string();
    let newer = json!({"version": 1, "entries": {"launch_app:terminal-aaa.desktop": {"count": 2, "last": now}}}).to_string();
    let h = fx.start_seeded(fx.cfg(), &[("palette.json", &old), ("frecency.json", &newer)]);
    let legacy = h.dir.join("state/dictate/palette.json");
    assert!(legacy.exists(), "beide vorhanden: der Start zieht nichts um");
    let r = h.req(r#"{"cmd":"pal_frecency_clear"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert!(!state_file(&h).exists());
    // mit geleerter `palette.json` findet ein Neustart nichts zum Übernehmen (`adopt_legacy` → NotFrecency, Unit-Test in
    // frecency.rs)
    assert_eq!(std::fs::metadata(&legacy).unwrap().len(), 0, "die alte Häufigkeit ist mit gelöscht (geleert, L54)");
    drop(h);

    // die Farbdatei von Noctalia bleibt beim Löschen, wie sie ist
    let h = fx.start_seeded(fx.cfg(), &[("palette.json", NOCTALIA_PALETTE), ("frecency.json", &newer)]);
    assert_eq!(h.req(r#"{"cmd":"pal_frecency_clear"}"#)["ok"], true);
    assert!(!state_file(&h).exists());
    assert_eq!(std::fs::read_to_string(h.dir.join("state/dictate/palette.json")).unwrap(), NOCTALIA_PALETTE);
}

#[test]
fn chosen_refuses_free_text_unknown_intents_and_unoffered_targets() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let h = fx.start(fx.cfg());
    // noch nichts angeboten
    assert!(error_of(&chosen(&h, "launch_app", "alpha.desktop")).contains("Unknown target"));
    resolve(&h, "alpha", None, None, None);
    // Absicht nicht im Katalog, Ziel nie angeboten, Ziel einer anderen Absicht, freier Text, Pfad
    assert!(error_of(&chosen(&h, "launch_apps", "alpha.desktop")).contains("Unknown intent"));
    assert!(error_of(&chosen(&h, "run_shell", "alpha.desktop")).contains("Unknown intent"));
    assert!(error_of(&chosen(&h, "launch_app", "evil.desktop")).contains("Unknown target"));
    assert!(error_of(&chosen(&h, "open_project", "alpha.desktop")).contains("Unknown target"));
    assert!(error_of(&chosen(&h, "launch_app", "my private sentence")).contains("Unknown target"));
    assert!(error_of(&chosen(&h, "launch_app", "/etc/passwd")).contains("Unknown target"));
    // fehlende Felder meldet schon das Protokoll
    assert!(error_of(&h.req(r#"{"cmd":"pal_chosen","intent":"launch_app"}"#)).contains("target_id"));
    assert!(error_of(&h.req(r#"{"cmd":"pal_chosen","target_id":"alpha.desktop"}"#)).contains("intent"));
    assert!(!state_file(&h).exists(), "abgelehnte Wahlen schreiben nichts");
    // der angebotene Treffer geht
    assert_eq!(chosen(&h, "launch_app", "alpha.desktop")["recorded"], true);
    let file = std::fs::read_to_string(state_file(&h)).unwrap();
    assert!(file.contains("launch_app:alpha.desktop") && !file.contains("private") && !file.contains("evil") && !file.contains("passwd"), "{file}");
}

#[test]
fn frecency_off_records_nothing_and_clear_deletes_the_file() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let mut cfg = fx.cfg();
    cfg.palette.frecency = false;
    let h = fx.start(cfg.clone());
    resolve(&h, "alpha", None, None, None);
    let r = chosen(&h, "launch_app", "alpha.desktop");
    assert_eq!((r["ok"].as_bool(), r["recorded"].as_bool()), (Some(true), Some(false)), "{r}");
    assert!(!state_file(&h).exists());

    // zur Laufzeit einschalten (Einstellungsfenster): gilt sofort
    cfg.palette.frecency = true;
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    resolve(&h, "alpha", None, None, None);
    assert_eq!(chosen(&h, "launch_app", "alpha.desktop")["recorded"], true);
    assert!(state_file(&h).exists());
    // Löschen: Datei weg, Reihenfolge wieder neutral
    let r = h.req(r#"{"cmd":"pal_frecency_clear"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert!(!state_file(&h).exists());
    // ein zweites Löschen ohne Datei ist kein Fehler
    assert_eq!(h.req(r#"{"cmd":"pal_frecency_clear"}"#)["ok"], true);
}

#[test]
fn disabled_palette_answers_every_command_with_a_clear_error() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let mut cfg = fx.cfg();
    cfg.palette.enabled = false;
    let h = fx.start(cfg.clone());
    let commands = [
        json!({"cmd": "pal_resolve", "input": "alpha"}),
        json!({"cmd": "pal_chosen", "intent": "launch_app", "target_id": "alpha.desktop"}),
        json!({"cmd": "pal_sources_refresh"}),
        json!({"cmd": "pal_frecency_clear"}),
    ];
    for c in &commands {
        let r = h.req(&c.to_string());
        assert!(error_of(&r).contains("command palette is off") && error_of(&r).contains("Settings › Palette"), "{c}: {r}");
    }
    assert!(!state_file(&h).exists());

    // Einschalten und Ausschalten zur Laufzeit gilt sofort
    cfg.palette.enabled = true;
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    assert_eq!(ids(&resolve(&h, "alpha", None, None, None))[0], "launch_app:alpha.desktop");
    assert_eq!(h.req(r#"{"cmd":"pal_sources_refresh"}"#)["ok"], true);
    cfg.palette.enabled = false;
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    assert!(error_of(&resolve(&h, "alpha", None, None, None)).contains("off"));
    // auch während des Diktats gilt die Einstellung sofort (kein Aufschub wie beim Modell)
    h.cmd("start");
    h.wait_state("listening");
    cfg.palette.enabled = true;
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string());
    assert_eq!(r["deferred"], true, "{r}");
    assert_eq!(resolve(&h, "alpha", None, None, None)["ok"], true);
    h.cmd("cancel");
}

#[test]
fn path_and_desktop_of_the_request_decide_try_exec_and_only_show_in() {
    let fx = Fx::new();
    fx.app("needs-tool.desktop", "Needs Tool", "TryExec=fixture-tool\n");
    fx.app("awesome-only.desktop", "Awesome Only", "OnlyShowIn=awesome;\n");
    fx.app("plain.desktop", "Plain", "");
    let bin = fx.tool("fixture-tool");
    let h = fx.start(fx.cfg());
    let bin = bin.display().to_string();
    let seen = |path: Option<&str>, desktop: Option<&str>| -> Vec<String> {
        // je Eintrag fragen: nur was der Daemon kennt, taucht auf
        let mut found = Vec::new();
        for q in ["needs", "awesome", "plain"] {
            found.extend(ids(&resolve(&h, q, None, path, desktop)).into_iter().filter(|i| i.starts_with("launch_app:") && i.contains(q)));
        }
        found
    };
    assert_eq!(seen(Some(&bin), Some("awesome")), ["launch_app:needs-tool.desktop", "launch_app:awesome-only.desktop", "launch_app:plain.desktop"]);
    // anderes PATH und anderer Desktop in der nächsten Anfrage: der Zwischenspeicher wird neu aufgebaut
    assert_eq!(seen(Some("/nonexistent:relative/dir"), Some("GNOME")), ["launch_app:plain.desktop"]);
    assert_eq!(seen(Some(&bin), Some("GNOME")), ["launch_app:needs-tool.desktop", "launch_app:plain.desktop"]);
    assert_eq!(seen(Some("/nonexistent"), Some("KDE:awesome")), ["launch_app:awesome-only.desktop", "launch_app:plain.desktop"]);
    // ohne Angabe gilt die Umgebung des Daemons (im Test-Prozess liegt `fixture-tool` nicht im PATH)
    assert_eq!(seen(None, Some("awesome")), ["launch_app:awesome-only.desktop", "launch_app:plain.desktop"]);
    // ungültige Werte werden abgelehnt, nicht stillschweigend ignoriert
    assert!(error_of(&resolve(&h, "x", None, Some("/bin\n/usr"), None)).contains("PATH"));
    assert!(error_of(&resolve(&h, "x", None, None, Some("a\tb"))).contains("XDG_CURRENT_DESKTOP"));
    assert!(error_of(&resolve(&h, "x", None, Some(&"/".repeat(20_000)), None)).contains("PATH"));
}

#[test]
fn sources_refresh_and_config_changes_apply_without_a_restart() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    fx.project("one");
    let other = fx.root.join("other");
    std::fs::create_dir_all(other.join("two/.git")).unwrap();
    let mut cfg = fx.cfg();
    let h = fx.start(cfg.clone());
    assert_eq!(ids(&resolve(&h, "alpha", None, None, None))[0], "launch_app:alpha.desktop");
    assert!(!ids(&resolve(&h, "beta", None, None, None)).contains(&"launch_app:beta.desktop".to_string()));
    fx.app("beta.desktop", "Beta", "");
    // pal_sources_refresh liest sofort neu
    assert_eq!(h.req(r#"{"cmd":"pal_sources_refresh"}"#)["ok"], true);
    assert_eq!(ids(&resolve(&h, "beta", None, None, None))[0], "launch_app:beta.desktop");

    // andere Projektwurzel und Tiefe über set_config
    assert!(ids(&resolve(&h, "one", None, None, None))[0].starts_with("open_project:"));
    cfg.palette.project_roots = vec![other.display().to_string()];
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    assert_eq!(ids(&resolve(&h, "two", None, None, None))[0], format!("open_project:{}", other.join("two").display()));
    assert!(!ids(&resolve(&h, "one", None, None, None)).iter().any(|i| i == &format!("open_project:{}", fx.root.join("git/one").display())));
    // die Suchadresse kommt aus der Konfiguration
    cfg.palette.web_search_url = "https://example.org/find?s={query}".into();
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    assert_eq!(resolve(&h, "g rust", None, None, None)["web_search_url"], "https://example.org/find?s={query}");
    // Terminal und Befehl im Projekt ebenso (leer als Vorgabe: automatisch bzw. Shell)
    assert_eq!(
        (resolve(&h, "g rust", None, None, None)["terminal"].clone(), resolve(&h, "g rust", None, None, None)["project_command"].clone()),
        (json!([]), json!([]))
    );
    cfg.palette.terminal = vec!["kitty".into()];
    cfg.palette.project_command = vec!["claude".into(), "--continue".into()];
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    let r = resolve(&h, "g rust", None, None, None);
    assert_eq!((r["terminal"].clone(), r["project_command"].clone()), (json!(["kitty"]), json!(["claude", "--continue"])));
    // Zwischenablage und Emoji: leer = automatisch; angeboten nur, was im PATH der Anfrage gefunden wird (O6)
    assert_eq!((r["clipboard_command"].clone(), r["emoji_command"].clone()), (json!([]), json!([])));
    let bin = fx.root.join("widget-bin");
    std::fs::create_dir_all(&bin).unwrap();
    // der Daemon merkt sich die Erkennung 2 s je Suchpfad und Befehl: jede Anfrage mit einem anderen (leeren) Ordner dahinter
    let round = std::cell::Cell::new(0);
    let widgets = |input: &str| {
        round.set(round.get() + 1);
        let path = format!("{}:{}", bin.display(), fx.root.join(format!("empty-{}", round.get())).display());
        ids(&resolve(&h, input, None, Some(&path), None)).into_iter().filter(|i| i.starts_with("show_widget:")).collect::<Vec<_>>()
    };
    assert!(widgets("clipboard").is_empty() && widgets("emoji").is_empty());
    let exe = |name: &str| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(bin.join(name), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    exe("noctalia");
    exe("rofi");
    assert_eq!(widgets("clipboard"), ["show_widget:clipboard_history"]);
    assert!(widgets("emoji").is_empty(), "rofi ohne rofi-emoji");
    cfg.palette.clipboard_command = vec!["copyq".into(), "toggle".into()];
    cfg.palette.emoji_command = vec!["bemoji".into()];
    assert_eq!(h.req(&json!({"cmd": "set_config", "config": {"palette": cfg.palette}}).to_string())["ok"], true);
    assert!(widgets("clipboard").is_empty(), "der eigene Befehl fehlt, Noctalia zählt dann nicht");
    exe("copyq");
    exe("bemoji");
    assert_eq!((widgets("clipboard"), widgets("emoji")), (vec!["show_widget:clipboard_history".to_string()], vec!["show_widget:emoji".to_string()]));
    let r = resolve(&h, "emoji", None, Some(&bin.display().to_string()), None);
    assert_eq!((r["clipboard_command"].clone(), r["emoji_command"].clone()), (json!(["copyq", "toggle"]), json!(["bemoji"])));
    // ungültige Einstellungen lehnt set_config mit dem Feld ab und lässt die laufenden unverändert
    let mut bad = cfg.palette.clone();
    bad.web_search_url = "http://insecure.example/?q={query}".into();
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": bad}}).to_string());
    assert_eq!((r["ok"].as_bool(), r["field"].as_str()), (Some(false), Some("palette.web_search_url")), "{r}");
    bad.web_search_url = cfg.palette.web_search_url.clone();
    bad.terminal = vec!["kitty -e".into()];
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": bad}}).to_string());
    assert_eq!(r["field"], "palette.terminal[0]", "{r}");
    assert!(r["error"].as_str().unwrap_or_default().contains("its own entry"), "{r}");
    bad.terminal = vec!["kitty".into(), "".into()];
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": bad}}).to_string());
    assert_eq!(r["field"], "palette.terminal[1]", "{r}");
    bad.terminal = cfg.palette.terminal.clone();
    bad.project_depth = 9;
    let r = h.req(&json!({"cmd": "set_config", "config": {"palette": bad}}).to_string());
    assert_eq!(r["field"], "palette.project_depth", "{r}");
    assert_eq!(resolve(&h, "g rust", None, None, None)["web_search_url"], "https://example.org/find?s={query}");
}

#[test]
fn bad_context_and_oversized_requests_are_refused() {
    let fx = Fx::new();
    let h = fx.start(fx.cfg());
    for (ctx, needle) in [
        (json!({"workspaces": [{"id": 1, "idx": 0}]}), "at least 1"),
        (json!({"windows": [{"id": 1}, {"id": 1}]}), "appears twice"),
        (json!({"unknown_field": 1}), "not valid"),
        (json!({"tags": []}), "context has an unknown field"),
        (json!({"windows": [{"id": 1, "title": "x".repeat(600)}]}), "too long"),
        (json!({"windows": [{"id": 1, "title": ["valuemarker-d1"]}]}), "windows[0].title must be a string"),
    ] {
        let r = resolve(&h, "x", Some(&ctx), None, None);
        assert!(error_of(&r).contains(needle), "{ctx}: {r}");
        assert!(!r.to_string().contains("valuemarker"), "die Meldung zitiert den Wert: {r}");
    }
    // Kontext kein Objekt: schon das Protokoll lehnt ab
    assert!(error_of(&h.req(r#"{"cmd":"pal_resolve","input":"x","context":[1]}"#)).contains("object"));
    assert!(error_of(&h.req(r#"{"cmd":"pal_resolve","context":{}}"#)).contains("input"));
    // Eine große, aber zulässige Zeile (200 Fenster, lange Titel; über 64 KiB) geht durch
    let many: Vec<Value> =
        (0..200).map(|i| json!({"id": 1000 + i, "app_id": format!("App{i}"), "title": "t".repeat(400), "workspace_id": 1 + i % 9})).collect();
    let big = json!({"windows": many});
    assert!(big.to_string().len() > 64 * 1024);
    let r = resolve(&h, "app199", Some(&big), None, None);
    assert_eq!(r["ok"], true, "{}", r.to_string().chars().take(200).collect::<String>());
    assert_eq!(ids(&r)[0], "focus_window:app199");
    // Zeilen über der Grenze für pal_resolve (512 KiB) beendet der Daemon mit einer Meldung
    let huge = json!({"cmd": "pal_resolve", "input": "x".repeat(600 * 1024)});
    let mut s = std::os::unix::net::UnixStream::connect(&h.sock).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    writeln!(s, "{huge}").unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(s), &mut line).unwrap();
    assert!(line.contains("request too large"), "{line}");
    // und der Daemon lebt weiter
    assert_eq!(resolve(&h, "lock", None, None, None)["ok"], true);
}

/// Zeilengrenze von `pal_resolve` und `pal_ask` (512 KiB, `MAX_PAL_RESOLVE_LINE` im Daemon) genau an der Kante: eine
/// Zeile von genau 512 KiB wird gelesen und inhaltlich beantwortet, ein Byte mehr beendet die Verbindung mit „request
/// too large“. (Bis W7 deckte das CLI nur 256 KiB Eingabe ab.)
#[test]
fn pal_resolve_and_pal_ask_lines_up_to_the_limit_are_read() {
    let fx = Fx::new();
    let h = fx.start(fx.cfg());
    const LIMIT: usize = 512 * 1024;
    let line_of = |cmd: &str, len: usize| {
        let base = json!({"cmd": cmd, "input": ""}).to_string();
        json!({"cmd": cmd, "input": "x".repeat(len - base.len())}).to_string()
    };
    let send = |line: &str| {
        let mut s = std::os::unix::net::UnixStream::connect(&h.sock).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut reply = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(s), &mut reply).unwrap();
        reply
    };
    for cmd in ["pal_resolve", "pal_ask"] {
        let at = line_of(cmd, LIMIT);
        assert_eq!(at.len(), LIMIT);
        let r = send(&at);
        assert!(!r.is_empty() && !r.contains("request too large"), "{cmd} at the limit: {}", r.chars().take(200).collect::<String>());
        let over = line_of(cmd, LIMIT + 1);
        assert!(send(&over).contains("request too large"), "{cmd} over the limit");
    }
    // und der Daemon lebt weiter
    assert_eq!(resolve(&h, "lock", None, None, None)["ok"], true);
}

#[test]
fn palette_commands_work_on_a_subscriber_connection() {
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let h = fx.start(fx.cfg());
    let mut sub = h.subscribe();
    sub.send(r#"{"cmd":"pal_resolve","input":"alpha"}"#);
    let r = sub.reply();
    assert_eq!(ids(&r)[0], "launch_app:alpha.desktop");
    sub.send(r#"{"cmd":"pal_chosen","intent":"launch_app","target_id":"alpha.desktop"}"#);
    assert_eq!(sub.reply()["recorded"], true);
}

// ---------------------------------------------------------------------------------------------------------------
// KI-Pfad (`pal_ask`)

/// Ollama-Mock, der immer mit der Absicht `intent` (flaches Schema, fehlende Felder null) antwortet
fn ollama_intent(intent: Value) -> Mock {
    ollama_intent_after(intent, 0)
}

/// Wie [`ollama_intent`], die Antwort kommt erst nach `delay_ms`
fn ollama_intent_after(intent: Value, delay_ms: u64) -> Mock {
    Mock::start(move |_| {
        let mut v = serde_json::Map::new();
        for k in dictate::palette::ai::flat_schema()["properties"].as_object().unwrap().keys() {
            v.insert(k.clone(), intent.get(k).cloned().unwrap_or(Value::Null));
        }
        Resp::ok(json!({"message": {"role": "assistant", "content": Value::Object(v).to_string()}, "done_reason": "stop"})).delayed(delay_ms)
    })
}

/// Konfiguration mit `[ai]` auf dem Mock (lokal, Ollama)
fn ai_cfg(fx: &Fx, m: &Mock) -> Config {
    let mut c = fx.cfg();
    c.ai.provider = "ollama".into();
    c.ai.preset = String::new();
    c.ai.endpoint = m.url.clone();
    c.ai.model = "m1".into();
    c.palette.timeout_ms = 3000;
    c
}

fn ask(h: &Harness, input: &str, ctx: Option<&Value>) -> Value {
    let mut v = json!({"cmd": "pal_ask", "input": input});
    if let Some(c) = ctx {
        v["context"] = c.clone();
    }
    h.req(&v.to_string())
}

#[test]
fn pal_ask_resolves_the_intent_locally_and_offers_the_candidates() {
    let fx = Fx::new();
    fx.app("firefox.desktop", "Firefox", "");
    let m = ollama_intent(json!({"intent": "focus_window", "query": "firefox"}));
    let h = fx.start(ai_cfg(&fx, &m));
    let ctx = json!({
        "workspaces": [{"id": 1, "idx": 1, "name": "wsmarker-7", "output": "outputmarker-8"}],
        "windows": [
            {"id": 100, "app_id": "Firefox", "title": "titlemarker-42", "workspace_id": 1},
            {"id": 101, "app_id": "Alacritty", "title": "zsh", "workspace_id": 1, "focused": true},
        ],
        "focus": 101,
    });
    let r = ask(&h, "geh zum browser bitte", Some(&ctx));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["intent"], json!({"intent": "focus_window", "query": "firefox"}));
    assert_eq!(ids(&r), ["focus_window:firefox"], "nur das Fenster, kein Programmstart");
    assert_eq!(r["candidates"][0]["target"], json!({"kind": "window", "id": 100}));
    assert_eq!(r["web_search_url"], "https://duckduckgo.com/?q={query}");
    assert_eq!((r["terminal"].clone(), r["project_command"].clone()), (json!([]), json!([])), "wie bei pal_resolve");
    assert_eq!((r["clipboard_command"].clone(), r["emoji_command"].clone()), (json!([]), json!([])), "wie bei pal_resolve");
    for key in ["intent", "target", "display", "detail", "score", "class", "target_id"] {
        assert!(r["candidates"][0].get(key).is_some(), "{key} fehlt: {r}");
    }
    // nur Eingabe und Katalog im Anfragekörper, nie der Kontext
    let body = &m.requests()[0].body;
    assert!(body.contains("geh zum browser bitte"));
    for marker in ["titlemarker-42", "wsmarker-7", "outputmarker-8", "firefox.desktop"] {
        assert!(!body.contains(marker), "{marker} ging an das Modell");
    }
    // die Kandidaten sind angemeldet: die Wahl wird angenommen
    assert_eq!(chosen(&h, "focus_window", "firefox")["recorded"], true);
}

#[test]
fn pal_ask_never_invents_a_target_and_reports_errors_as_json() {
    let fx = Fx::new();
    fx.app("firefox.desktop", "Firefox", "");
    let m = ollama_intent(json!({"intent": "launch_app", "query": "blender"}));
    let h = fx.start(ai_cfg(&fx, &m));
    let r = ask(&h, "starte blender", None);
    assert_eq!(error_of(&r), "No match for an application");
    assert!(!r.to_string().contains("blender"), "{r}");
    assert_eq!(chosen(&h, "launch_app", "blender")["ok"], false);

    // ungültiger Kontext: kein Modellaufruf
    let n = m.requests().len();
    let r = ask(&h, "x", Some(&json!({"workspaces": [{"id": 1, "idx": 0}]})));
    assert!(error_of(&r).contains("at least 1"), "{r}");
    assert_eq!(m.requests().len(), n);
    // leere Eingabe
    assert_eq!(error_of(&ask(&h, "  ", None)), "Nothing to ask");
    // ausgeschaltete Palette
    let mut off = ai_cfg(&fx, &m);
    off.palette.enabled = false;
    let h2 = fx.start(off);
    assert!(error_of(&ask(&h2, "lock", None)).contains("palette is off"));

    // erfundene Absicht des Modells
    let bad = ollama_intent(json!({"intent": "format_disk"}));
    let h3 = fx.start(ai_cfg(&fx, &bad));
    assert!(error_of(&ask(&h3, "x", None)).contains("not in the catalog"));
}

#[test]
fn closing_the_connection_cancels_pal_ask() {
    use std::io::Write as _;
    let fx = Fx::new();
    // die Antwort käme erst nach 5 s; der Mock merkt, wann der Daemon die HTTP-Verbindung schließt
    let m = Mock::start(|_| Resp::stream("application/json", vec![(0, br#"{"message":{"content":"{}"}}"#.to_vec())]).delayed(5000));
    let mut cfg = ai_cfg(&fx, &m);
    cfg.palette.timeout_ms = 20_000;
    let h = fx.start(cfg);
    let mut s = std::os::unix::net::UnixStream::connect(&h.sock).unwrap();
    writeln!(s, "{}", json!({"cmd": "pal_ask", "input": "lock"})).unwrap();
    h.wait_until("Anfrage am Mock", || !m.requests().is_empty());
    let closed_at = std::time::Instant::now();
    drop(s);
    h.wait_until("Abbruch beim Auflegen", || !m.closed.lock().unwrap().is_empty());
    let noticed = m.closed.lock().unwrap()[0];
    assert!(noticed.duration_since(closed_at) < std::time::Duration::from_millis(1500), "{:?}", noticed.duration_since(closed_at));
    // der Daemon antwortet danach weiter
    assert_eq!(h.req(r#"{"cmd":"status"}"#)["ok"], true);
}

#[test]
fn pal_ask_hangup_is_noticed_after_more_data_or_a_half_close() {
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};
    // der Client schickt nach der Anfrage noch ein Byte (die nächste Anfrage), beendet nur das Schreiben (und liest noch)
    // oder beides; der Daemon bemerkt das spätere Auflegen trotzdem sofort, auch wenn der Mock nie antwortet
    type Step = fn(&mut UnixStream);
    let steps: [(&str, Step); 3] = [
        ("one more byte", |s| s.write_all(b"x").unwrap()),
        ("half close", |s| s.shutdown(std::net::Shutdown::Write).unwrap()),
        ("byte, then half close", |s| {
            s.write_all(b"x").unwrap();
            s.shutdown(std::net::Shutdown::Write).unwrap();
        }),
    ];
    for (what, step) in steps {
        let fx = Fx::new();
        // die Antwort käme erst nach 5 s; der Mock merkt, wann der Daemon die HTTP-Verbindung schließt
        let m = Mock::start(|_| Resp::stream("application/json", vec![(0, br#"{"message":{"content":"{}"}}"#.to_vec())]).delayed(5000));
        let mut cfg = ai_cfg(&fx, &m);
        cfg.palette.timeout_ms = 20_000;
        let h = fx.start(cfg);
        let mut s = UnixStream::connect(&h.sock).unwrap();
        writeln!(s, "{}", json!({"cmd": "pal_ask", "input": "lock"})).unwrap();
        h.wait_until("Anfrage am Mock", || !m.requests().is_empty());
        step(&mut s);
        // der Wächter hat mehrere Runden Zeit, daran zu scheitern; die Anfrage läuft weiter
        std::thread::sleep(Duration::from_millis(300));
        assert!(m.closed.lock().unwrap().is_empty(), "{what}: der Aufruf wurde zu früh abgebrochen");
        let closed_at = Instant::now();
        drop(s);
        h.wait_until("Abbruch beim Auflegen", || !m.closed.lock().unwrap().is_empty());
        let noticed = m.closed.lock().unwrap()[0];
        assert!(noticed.duration_since(closed_at) < Duration::from_millis(1500), "{what}: {:?}", noticed.duration_since(closed_at));
        assert_eq!(h.req(r#"{"cmd":"status"}"#)["ok"], true, "{what}");
    }
}

#[test]
fn a_half_closed_client_still_gets_the_answer() {
    use std::io::BufRead as _;
    let fx = Fx::new();
    let m = ollama_intent_after(json!({"intent": "lock"}), 600);
    let h = fx.start(ai_cfg(&fx, &m));
    let mut s = std::os::unix::net::UnixStream::connect(&h.sock).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    writeln!(s, "{}", json!({"cmd": "pal_ask", "input": "lock the screen"})).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
    let mut line = String::new();
    std::io::BufReader::new(&s).read_line(&mut line).unwrap();
    let r: Value = serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["intent"], json!({"intent": "lock"}));
}

#[test]
fn pal_ask_reports_server_errors_without_server_text() {
    let fx = Fx::new();
    let m = Mock::start(|_| Resp::status(400, json!({"error": "cannot handle: hunter2 words"})));
    let h = fx.start(ai_cfg(&fx, &m));
    let r = ask(&h, "hunter2 words", None);
    assert_eq!(error_of(&r), "AI error: HTTP 400 (the server rejected the request)");
    assert!(!r.to_string().contains("hunter2") && !r.to_string().contains("cannot handle"), "{r}");
}

#[test]
fn pal_ask_takes_an_address_only_with_its_host_from_the_input() {
    let fx = Fx::new();
    let m = ollama_intent(json!({"intent": "open_url", "url": "https://example.org/docs"}));
    let h = fx.start(ai_cfg(&fx, &m));
    let r = ask(&h, "Open EXAMPLE.org docs", None);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["intent"], json!({"intent": "open_url", "url": "https://example.org/docs"}));
    assert_eq!((ids(&r), r["candidates"][0]["target"].clone()), (vec!["open_url:open_url".to_string()], json!({"kind": "none"})));
    assert_eq!(chosen(&h, "open_url", "open_url")["recorded"], true);
    // das Modell erfindet den Host: nichts davon kommt zurück
    let r = ask(&h, "open the example documentation", None);
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(error_of(&r), "No match for a web address");
    assert!(!r.to_string().contains("example.org"), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------
// Nie loggen

static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct LogWriter;

impl Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Alles bis TRACE in den Speicher (der Daemon läuft in diesem Prozess).
fn capture_logs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let sub = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_ansi(false).with_writer(|| LogWriter).finish();
        tracing::subscriber::set_global_default(sub).expect("kein anderer Subscriber in diesem Test-Binary");
    });
}

#[test]
fn input_and_context_never_reach_the_log() {
    capture_logs();
    let fx = Fx::new();
    fx.app("alpha.desktop", "Alpha", "");
    let h = fx.start(fx.cfg());
    let ctx = json!({
        "workspaces": [{"id": 1, "idx": 1, "name": "wsname-marker-7", "output": "outputmarker-6"}],
        "windows": [{"id": 5, "app_id": "appidmarker", "title": "title-marker-9 bank statement", "workspace_id": 1}],
    });
    for input in ["inputmarker-3 alpha", "?inputmarker-3 what is this", "g inputmarker-3", "inputmarker-3"] {
        assert_eq!(resolve(&h, input, Some(&ctx), Some("/pathmarker/bin"), Some("deskmarker"))["ok"], true);
    }
    // auch Fehlerwege und die Wahl
    assert_eq!(resolve(&h, "inputmarker-3", Some(&json!({"bogus": "title-marker-9"})), None, None)["ok"], false);
    assert_eq!(chosen(&h, "launch_app", "alpha.desktop")["ok"], false);
    resolve(&h, "alpha", None, None, None);
    assert_eq!(chosen(&h, "launch_app", "alpha.desktop")["ok"], true);
    // der Name einer Arbeitsfläche ist Ziel-ID: Auflösen und Wahl loggen ihn nicht
    let r = resolve(&h, "workspace wsname", Some(&ctx), None, None);
    assert_eq!(ids(&r)[0], "view_workspace:wsname-marker-7", "{r}");
    assert_eq!(chosen(&h, "view_workspace", "wsname-marker-7")["recorded"], true);
    // KI-Pfad: Eingabe, Kontext und Antwort des Modells (Suchbegriff) nie im Log
    let m = ollama_intent(json!({"intent": "launch_app", "query": "answermarker-5"}));
    let h_ai = fx.start(ai_cfg(&fx, &m));
    assert_eq!(ask(&h_ai, "inputmarker-3 alpha", Some(&ctx))["ok"], false);
    let ok = ollama_intent(json!({"intent": "launch_app", "query": "alpha"}));
    let h_ok = fx.start(ai_cfg(&fx, &ok));
    assert_eq!(ask(&h_ok, "inputmarker-3 alpha", Some(&ctx))["ok"], true);
    assert_eq!(h.req(r#"{"cmd":"pal_chosen","intent":"inputmarker-3","target_id":"x"}"#)["ok"], false);
    h.wait_until("Daemon bereit", || String::from_utf8_lossy(&LOG.lock().unwrap()).contains("daemon ready"));
    let log = String::from_utf8_lossy(&LOG.lock().unwrap()).to_string();
    assert!(log.contains("daemon ready"), "der Mitschnitt funktioniert nicht: {log}");
    assert!(log.contains("pal ask with m1: launch_app"), "der KI-Pfad schreibt seine Zeile: {log}");
    for marker in
        ["inputmarker-3", "title-marker-9", "wsname-marker-7", "outputmarker-6", "appidmarker", "pathmarker", "deskmarker", "bank statement", "answermarker-5"]
    {
        assert!(!log.contains(marker), "{marker} steht im Log:\n{log}");
    }
}

#[test]
fn the_fixture_context_is_valid() {
    // sonst prüften die Tests oben nur die Fehlerwege
    assert!(dictate::palette::Context::from_json(&context().to_string()).is_ok());
}
