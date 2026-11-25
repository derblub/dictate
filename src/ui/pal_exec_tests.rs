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

//! Tests des Ausführers gegen einen Rekorder als Wirt und Fake-Programme in einem Temp-Ordner (nie die echten
//! Programme, nie niri; Fälle aus `palette_exec.lua` und den Regeln in 6g).

use super::*;
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Ordner mit Fake-Programmen (leere ausführbare Dateien; gestartet wird hier nichts), einmal je Testlauf: die Tests
/// laufen parallel und ändern ihn nie
fn bin_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dictate-palexec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "alacritty",
            "kitty",
            "claude",
            "sh",
            "zsh",
            "xdg-open",
            "pactl",
            "playerctl",
            "systemctl",
            "loginctl",
            "noctalia",
            "rofi",
            "copyq",
            "bemoji",
            "dictate-settings",
            "firefox",
            "a=b",
        ] {
            let p = dir.join(name);
            std::fs::write(&p, "").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let p = dir.join("noexec");
        std::fs::write(&p, "").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        dir
    })
    .clone()
}

struct Rec {
    windows: Vec<u64>,
    workspaces: Vec<u64>,
    focused: Option<u64>,
    /// `FocusWindow` setzt den Fokus (wie niri)
    focus_follows: bool,
    niri_fails: bool,
    spawn_fails: bool,
    replies: VecDeque<RunResult>,
    /// Reihenfolge aller Wirkungen, auch `chosen`
    log: Rc<RefCell<Vec<String>>>,
    ops: Vec<NiriOp>,
    spawned: Vec<Vec<String>>,
    ran: Vec<Vec<String>>,
}

impl Rec {
    fn new() -> Rec {
        Rec {
            windows: vec![1, 2, 3],
            workspaces: vec![10, 11, 77],
            focused: Some(1),
            focus_follows: true,
            niri_fails: false,
            spawn_fails: false,
            replies: VecDeque::new(),
            log: Rc::default(),
            ops: Vec::new(),
            spawned: Vec::new(),
            ran: Vec::new(),
        }
    }

    fn nothing_done(&self) -> bool {
        self.ops.is_empty() && self.spawned.is_empty() && self.ran.is_empty()
    }
}

impl Host for Rec {
    fn windows(&mut self) -> Result<Vec<u64>, String> {
        Ok(self.windows.clone())
    }
    fn workspaces(&mut self) -> Result<Vec<u64>, String> {
        Ok(self.workspaces.clone())
    }
    fn focused_window(&mut self) -> Result<Option<u64>, String> {
        Ok(self.focused)
    }
    fn niri(&mut self, op: NiriOp) -> Result<(), String> {
        self.log.borrow_mut().push(format!("{op:?}"));
        self.ops.push(op);
        if self.niri_fails {
            return Err("refused".into());
        }
        if let (NiriOp::FocusWindow(id), true) = (op, self.focus_follows) {
            self.focused = Some(id);
        }
        Ok(())
    }
    fn spawn(&mut self, launch: Launch) -> Result<(), String> {
        self.log.borrow_mut().push("spawn".into());
        self.spawned.push(launch.into_argv());
        if self.spawn_fails {
            Err("refused".into())
        } else {
            Ok(())
        }
    }
    fn run(&mut self, exe: &Exe, args: &[&str]) -> RunResult {
        self.log.borrow_mut().push("run".into());
        let mut argv = vec![exe.path().to_string_lossy().into_owned()];
        argv.extend(args.iter().map(|a| a.to_string()));
        self.ran.push(argv);
        self.replies.pop_front().unwrap_or(RunResult::Ok(String::new()))
    }
}

fn cand(intent: serde_json::Value, target: serde_json::Value, class: &str) -> Candidate {
    Candidate::from_value(&json!({"intent": intent, "target": target, "display": "d", "detail": null, "score": 1.0, "class": class, "target_id": "t"}))
        .unwrap_or_else(|| panic!("candidate {intent} {target}"))
}

fn none() -> serde_json::Value {
    json!({"kind": "none"})
}

struct Run {
    out: Outcome,
    chosen: usize,
}

fn exec_with(c: &Candidate, ctx: &ExecCtx, programs: &Programs, host: &mut Rec) -> Run {
    let log = host.log.clone();
    let mut chosen = 0;
    let out = execute(c, ctx, programs, host, &mut || {
        chosen += 1;
        log.borrow_mut().push("chosen".into());
    });
    Run { out, chosen }
}

fn exec(c: &Candidate, host: &mut Rec) -> Run {
    exec_with(c, &ExecCtx { prior_focus: Some(1), prefs: search_prefs(), confirmed: true }, &Programs::with_dirs([bin_dir()]), host)
}

fn failed(r: &Run, part: &str) {
    match &r.out {
        Outcome::Failed(m) => assert!(m.contains(part), "{m:?} lacks {part:?}"),
        o => panic!("expected failure with {part:?}, got {o:?}"),
    }
    assert_eq!(r.chosen, 0);
}

fn search_prefs() -> ExecPrefs {
    ExecPrefs { web_search_url: "https://duckduckgo.com/?q={query}".into(), ..ExecPrefs::default() }
}

fn ok(r: &Run) {
    assert!(matches!(r.out, Outcome::Done(_)), "{:?}", r.out);
    assert_eq!(r.chosen, 1);
}

fn exe(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

// --- Ziel nur aus dem Kandidaten ------------------------------------------------------------------------------------

#[test]
fn workspace_comes_from_the_target_id_never_from_the_intent() {
    let mut h = Rec::new();
    let c = cand(json!({"intent": "view_workspace", "workspace": 3}), json!({"kind": "workspace", "id": 77}), "run");
    ok(&exec(&c, &mut h));
    assert_eq!(h.ops, vec![NiriOp::FocusWorkspace(77)]);
    // Ziel passt nicht zur Absicht: nichts
    let mut h = Rec::new();
    let c = cand(json!({"intent": "view_workspace", "workspace": 3}), json!({"kind": "window", "id": 1}), "run");
    failed(&exec(&c, &mut h), "no workspace");
    assert!(h.nothing_done());
    // verschwundene Arbeitsfläche
    let mut h = Rec::new();
    let c = cand(json!({"intent": "view_workspace", "workspace": "web"}), json!({"kind": "workspace", "id": 12}), "run");
    failed(&exec(&c, &mut h), "workspace is gone");
    assert!(h.nothing_done());
}

#[test]
fn move_takes_both_ids_and_checks_both() {
    let mut h = Rec::new();
    let c = cand(
        json!({"intent": "move_window_to_workspace", "workspace": 3, "query": "firefox"}),
        json!({"kind": "move_window", "id": 2, "workspace": 77}),
        "run",
    );
    ok(&exec(&c, &mut h));
    assert_eq!(h.ops, vec![NiriOp::MoveWindowToWorkspace { window: 2, workspace: 77 }]);
    for (target, why) in [
        (json!({"kind": "move_window", "id": 9, "workspace": 77}), "window is gone"),
        (json!({"kind": "move_window", "id": 2, "workspace": 99}), "workspace is gone"),
        (json!({"kind": "workspace", "id": 77}), "no window"),
    ] {
        let mut h = Rec::new();
        failed(&exec(&cand(json!({"intent": "move_window_to_workspace", "workspace": 3}), target, "run"), &mut h), why);
        assert!(h.nothing_done());
    }
}

#[test]
fn window_intents_need_a_present_window_from_the_target() {
    for (intent, op) in [
        (json!({"intent": "focus_window", "query": "x"}), NiriOp::FocusWindow(2)),
        (json!({"intent": "toggle_window", "what": "floating"}), NiriOp::ToggleFloating(2)),
        (json!({"intent": "toggle_window", "what": "fullscreen"}), NiriOp::Fullscreen(2)),
        (json!({"intent": "toggle_window", "what": "maximized"}), NiriOp::Maximize(2)),
        (json!({"intent": "close_window"}), NiriOp::CloseWindow(2)),
    ] {
        let mut h = Rec::new();
        ok(&exec(&cand(intent.clone(), json!({"kind": "window", "id": 2}), "run"), &mut h));
        assert_eq!(h.ops, vec![op]);
        // weg: Meldung, keine Ersatzwahl (auch nicht das Fenster vor dem Öffnen)
        let mut h = Rec::new();
        failed(&exec(&cand(intent.clone(), json!({"kind": "window", "id": 9}), "run"), &mut h), "That window is gone");
        assert!(h.nothing_done());
        let mut h = Rec::new();
        failed(&exec(&cand(intent, none(), "run"), &mut h), "no window to act on");
        assert!(h.nothing_done());
    }
}

#[test]
fn column_display_only_after_the_target_has_the_focus() {
    let mut h = Rec::new();
    ok(&exec(&cand(json!({"intent": "column_display", "display": "tabbed"}), json!({"kind": "window", "id": 3}), "run"), &mut h));
    assert_eq!(h.ops, vec![NiriOp::FocusWindow(3), NiriOp::SetColumnDisplay(ColumnDisplay::Tabbed)]);
    // der Fokus kam nicht an: nichts auf einer fremden Spalte
    let mut h = Rec::new();
    h.focus_follows = false;
    failed(&exec(&cand(json!({"intent": "column_display", "display": "normal"}), json!({"kind": "window", "id": 3}), "run"), &mut h), "focus");
    assert_eq!(h.ops, vec![NiriOp::FocusWindow(3)]);
}

#[test]
fn screenshots() {
    for (mode, op) in [("area", NiriOp::ScreenshotArea), ("full", NiriOp::ScreenshotScreen)] {
        let mut h = Rec::new();
        ok(&exec(&cand(json!({"intent": "screenshot", "mode": mode}), none(), "run"), &mut h));
        assert_eq!(h.ops, vec![op]);
    }
    // window ohne Ziel: das Fenster vor dem Öffnen
    let mut h = Rec::new();
    ok(&exec(&cand(json!({"intent": "screenshot", "mode": "window"}), none(), "run"), &mut h));
    assert_eq!(h.ops, vec![NiriOp::ScreenshotWindow(1)]);
    // ohne ein solches: nichts
    let mut h = Rec::new();
    let r = exec_with(
        &cand(json!({"intent": "screenshot", "mode": "window"}), none(), "run"),
        &ExecCtx { prior_focus: None, confirmed: true, ..ExecCtx::default() },
        &Programs::with_dirs([]),
        &mut h,
    );
    failed(&r, "no window");
    assert!(h.nothing_done());
}

#[test]
fn previous_workspace_and_refusals() {
    let mut h = Rec::new();
    ok(&exec(&cand(json!({"intent": "previous_workspace"}), none(), "run"), &mut h));
    assert_eq!(h.ops, vec![NiriOp::FocusWorkspacePrevious]);
    let mut h = Rec::new();
    h.niri_fails = true;
    failed(&exec(&cand(json!({"intent": "previous_workspace"}), none(), "run"), &mut h), "niri refused");
}

// --- Bestätigung und `chosen` -------------------------------------------------------------------------------------

#[test]
fn confirm_needs_the_confirmation_even_if_the_class_says_run() {
    let unconfirmed = ExecCtx { prior_focus: Some(1), confirmed: false, ..ExecCtx::default() };
    let programs = Programs::with_dirs([bin_dir()]);
    for (intent, target, class) in [
        (json!({"intent": "close_window"}), json!({"kind": "window", "id": 2}), "confirm"),
        (json!({"intent": "poweroff"}), none(), "run"),
        (json!({"intent": "reboot"}), none(), "run"),
        (json!({"intent": "suspend"}), none(), "confirm"),
        (json!({"intent": "logout"}), none(), "ai"),
        // die Klasse allein reicht auch
        (json!({"intent": "lock"}), none(), "confirm"),
    ] {
        let mut h = Rec::new();
        failed(&exec_with(&cand(intent, target, class), &unconfirmed, &programs, &mut h), "needs a confirmation");
        assert!(h.nothing_done());
    }
    // ohne Bestätigungspflicht läuft es ohne
    let mut h = Rec::new();
    ok(&exec_with(&cand(json!({"intent": "lock"}), none(), "run"), &unconfirmed, &programs, &mut h));
}

#[test]
fn logout_counts_before_quit() {
    let mut h = Rec::new();
    let log = h.log.clone();
    ok(&exec(&cand(json!({"intent": "logout"}), none(), "confirm"), &mut h));
    assert_eq!(*log.borrow(), vec!["chosen".to_string(), "Quit".to_string()]);
    let mut h = Rec::new();
    h.niri_fails = true;
    let r = exec(&cand(json!({"intent": "logout"}), none(), "confirm"), &mut h);
    assert!(matches!(&r.out, Outcome::Failed(m) if m.contains("Logging out")), "{:?}", r.out);
    // gezählt ist die Wahl trotzdem (Lesart L14)
    assert_eq!(r.chosen, 1);
}

#[test]
fn chosen_only_after_success() {
    let mut h = Rec::new();
    h.spawn_fails = true;
    let app = json!({"kind": "app", "id": "firefox.desktop", "argv": ["firefox", "--new-window"], "terminal": false});
    failed(&exec(&cand(json!({"intent": "launch_app", "query": "fire"}), app.clone(), "run"), &mut h), "Could not start firefox");
    let mut h = Rec::new();
    h.replies.push_back(RunResult::Failed("Access denied".into()));
    failed(&exec(&cand(json!({"intent": "reboot"}), none(), "confirm"), &mut h), "Reboot failed: Access denied");
    let mut h = Rec::new();
    let log = h.log.clone();
    ok(&exec(&cand(json!({"intent": "launch_app", "query": "fire"}), app, "run"), &mut h));
    assert_eq!(*log.borrow(), vec!["spawn".to_string(), "chosen".to_string()]);
}

// --- Programme -----------------------------------------------------------------------------------------------------

#[test]
fn launch_app_starts_the_resolved_program() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
    let run = |target: serde_json::Value, h: &mut Rec| exec_with(&cand(json!({"intent": "launch_app", "query": "x"}), target, "run"), &ctx, &programs, h);
    let firefox = exe(&dir, "firefox");
    let tmp = std::env::temp_dir();
    let tmp_s = tmp.to_string_lossy().into_owned();

    let mut h = Rec::new();
    ok(&run(json!({"kind": "app", "id": "f", "argv": ["firefox", "-P", "x y"], "terminal": false}), &mut h));
    assert_eq!(h.spawned, vec![vec![firefox.clone(), "-P".into(), "x y".into()]]);

    // Arbeitsordner über env -C … --
    let mut h = Rec::new();
    ok(&run(json!({"kind": "app", "id": "f", "argv": ["firefox"], "working_dir": tmp_s, "terminal": false}), &mut h));
    assert_eq!(h.spawned, vec![vec![ENV.to_string(), "-C".into(), tmp_s.clone(), "--".into(), firefox.clone()]]);

    // Terminal automatisch (das erste installierte aus TERMINALS, hier alacritty vor kitty): [env -C dir --] alacritty
    // [--working-directory dir] -e prog args
    let mut h = Rec::new();
    ok(&run(json!({"kind": "app", "id": "f", "argv": ["firefox", "a"], "working_dir": tmp_s, "terminal": true}), &mut h));
    assert_eq!(
        h.spawned,
        vec![vec![
            ENV.to_string(),
            "-C".into(),
            tmp_s.clone(),
            "--".into(),
            exe(&dir, "alacritty"),
            "--working-directory".into(),
            tmp_s.clone(),
            "-e".into(),
            firefox.clone(),
            "a".into()
        ]]
    );
    let mut h = Rec::new();
    ok(&run(json!({"kind": "app", "id": "f", "argv": ["firefox"], "terminal": true}), &mut h));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "alacritty"), "-e".into(), firefox.clone()]]);

    // Terminal aus der Config (`palette.terminal`): genau dieses argv vor dem Befehl, auch als absoluter Pfad
    for (terminal, expect) in [
        (vec!["kitty".to_string()], vec![exe(&dir, "kitty")]),
        (vec![exe(&dir, "kitty"), "--single-instance".into()], vec![exe(&dir, "kitty"), "--single-instance".into()]),
    ] {
        let ctx = ExecCtx { confirmed: true, prefs: ExecPrefs { terminal, ..ExecPrefs::default() }, ..ExecCtx::default() };
        let mut h = Rec::new();
        ok(&exec_with(
            &cand(json!({"intent": "launch_app", "query": "x"}), json!({"kind": "app", "id": "f", "argv": ["firefox", "a"], "terminal": true}), "run"),
            &ctx,
            &programs,
            &mut h,
        ));
        let mut want = expect;
        want.extend([firefox.clone(), "a".into()]);
        assert_eq!(h.spawned, vec![want]);
    }

    // absoluter Pfad bleibt, wenn ausführbar
    let mut h = Rec::new();
    ok(&run(json!({"kind": "app", "id": "f", "argv": [firefox.clone()], "terminal": false}), &mut h));
    assert_eq!(h.spawned, vec![vec![firefox.clone()]]);
}

#[test]
fn launch_app_refuses_what_it_cannot_check() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    let long: Vec<String> = std::iter::once("firefox".to_string()).chain((0..MAX_ARGV).map(|i| i.to_string())).collect();
    for (target, why) in [
        (json!({"kind": "app", "id": "f", "argv": [], "terminal": false}), "no launch command"),
        (json!({"kind": "app", "id": "f", "argv": [""], "terminal": false}), "no launch command"),
        (json!({"kind": "app", "id": "f", "argv": ["firefox", "a\u{0}b"], "terminal": false}), "no launch command"),
        (json!({"kind": "app", "id": "f", "argv": long, "terminal": false}), "no launch command"),
        (json!({"kind": "app", "id": "f", "argv": ["missing"], "terminal": false}), "Program not found: missing"),
        (json!({"kind": "app", "id": "f", "argv": ["noexec"], "terminal": false}), "Program not found: noexec"),
        (json!({"kind": "app", "id": "f", "argv": ["./firefox"], "terminal": false}), "Program not found"),
        (json!({"kind": "app", "id": "f", "argv": ["bin/firefox"], "terminal": false}), "Program not found"),
        (json!({"kind": "app", "id": "f", "argv": ["firefox"], "working_dir": "relative/dir", "terminal": false}), "Folder not found"),
        (json!({"kind": "app", "id": "f", "argv": ["firefox"], "working_dir": "/nonexistent/dictate", "terminal": true}), "Folder not found"),
        (json!({"kind": "app", "id": "f", "argv": ["a=b"], "working_dir": tmp, "terminal": false}), "cannot start in its folder"),
        (json!({"kind": "project", "path": "/tmp"}), "no launch command"),
    ] {
        let mut h = Rec::new();
        failed(&exec_with(&cand(json!({"intent": "launch_app", "query": "x"}), target.clone(), "run"), &ctx, &programs, &mut h), why);
        assert!(h.nothing_done(), "{target}");
    }
    // ohne Terminal im Suchpfad
    let mut h = Rec::new();
    let no_term = Programs::with_dirs([]);
    let abs = exe(&dir, "firefox");
    failed(
        &exec_with(
            &cand(json!({"intent": "launch_app", "query": "x"}), json!({"kind": "app", "id": "f", "argv": [abs], "terminal": true}), "run"),
            &ctx,
            &no_term,
            &mut h,
        ),
        "No terminal found",
    );
    assert!(h.nothing_done());
    // ein eingetragenes Terminal, das fehlt, nennt seinen Namen (kein stiller Rückfall auf ein anderes)
    let mut h = Rec::new();
    let ctx =
        ExecCtx { confirmed: true, prefs: ExecPrefs { terminal: vec!["missing-term".into(), "-e".into()], ..ExecPrefs::default() }, ..ExecCtx::default() };
    failed(
        &exec_with(
            &cand(json!({"intent": "launch_app", "query": "x"}), json!({"kind": "app", "id": "f", "argv": ["firefox"], "terminal": true}), "run"),
            &ctx,
            &programs,
            &mut h,
        ),
        "Program not found: missing-term",
    );
    assert!(h.nothing_done());
}

#[test]
fn every_automatic_terminal_gets_the_folder_and_the_command() {
    // je Terminal der Tabelle ein Ordner, in dem nur es und die Shell liegen: genau dieses argv
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    let want: [(&str, Vec<String>); 9] = [
        ("xdg-terminal-exec", vec![]),
        ("alacritty", vec!["--working-directory".into(), tmp.clone(), "-e".into()]),
        ("kitty", vec!["--directory".into(), tmp.clone()]),
        ("foot", vec![format!("--working-directory={tmp}")]),
        ("ghostty", vec![format!("--working-directory={tmp}"), "-e".into()]),
        ("wezterm", vec!["start".into(), "--cwd".into(), tmp.clone(), "--".into()]),
        ("konsole", vec!["--workdir".into(), tmp.clone(), "-e".into()]),
        ("gnome-terminal", vec![format!("--working-directory={tmp}"), "--".into()]),
        ("xterm", vec!["-e".into()]),
    ];
    assert_eq!(want.each_ref().map(|(n, _)| *n), TERMINALS.map(|t| t.name), "the table and this test list the same terminals");
    for (name, args) in want {
        let dir = std::env::temp_dir().join(format!("dictate-palexec-term-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in [name, "sh"] {
            let p = dir.join(f);
            std::fs::write(&p, "").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let programs = Programs::with_dirs([dir.clone()]);
        let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
        let mut h = Rec::new();
        ok(&exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &ctx, &programs, &mut h));
        let mut expect = vec![ENV.to_string(), "-C".into(), tmp.clone(), "--".into(), exe(&dir, name)];
        expect.extend(args);
        expect.push(exe(&dir, "sh"));
        assert_eq!(h.spawned, vec![expect], "{name}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_terminal_path_with_equals_cannot_start_in_a_folder() {
    // env läse `a=b` nach `--` als Zuweisung: ein solches Terminal startet ohne Ordner, mit Ordner nie
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, prefs: ExecPrefs { terminal: vec!["a=b".into()], ..ExecPrefs::default() }, ..ExecCtx::default() };
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    let mut h = Rec::new();
    failed(
        &exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &ctx, &programs, &mut h),
        "cannot start in its folder",
    );
    assert!(h.nothing_done());
    let mut h = Rec::new();
    ok(&exec_with(
        &cand(json!({"intent": "launch_app", "query": "x"}), json!({"kind": "app", "id": "f", "argv": ["firefox"], "terminal": true}), "run"),
        &ctx,
        &programs,
        &mut h,
    ));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "a=b"), exe(&dir, "firefox")]]);
    // als Befehl im Terminal ist `a=b` nur ein argv-Element des Terminals: erlaubt
    let ctx = ExecCtx { confirmed: true, prefs: ExecPrefs { project_command: vec!["a=b".into()], ..ExecPrefs::default() }, ..ExecCtx::default() };
    let mut h = Rec::new();
    ok(&exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &ctx, &programs, &mut h));
    assert_eq!(h.spawned[0].last(), Some(&exe(&dir, "a=b")));
}

#[test]
fn projects_folders_urls_and_searches() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { prefs: search_prefs(), confirmed: true, ..ExecCtx::default() };
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    let run = |intent: serde_json::Value, target: serde_json::Value, h: &mut Rec| exec_with(&cand(intent, target, "run"), &ctx, &programs, h);

    // ohne `palette.project_command`: die Shell im Terminal, im Projektordner (ohne SHELL: sh im Suchpfad)
    let in_project = |rest: &[String]| {
        let mut v =
            vec![ENV.to_string(), "-C".into(), tmp.clone(), "--".into(), exe(&dir, "alacritty"), "--working-directory".into(), tmp.clone(), "-e".into()];
        v.extend(rest.iter().cloned());
        v
    };
    let mut h = Rec::new();
    ok(&run(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), &mut h));
    assert_eq!(h.spawned, vec![in_project(&[exe(&dir, "sh")])]);
    // SHELL des Users, wenn ausführbar; sonst wieder sh
    for (shell, want) in [(exe(&dir, "zsh"), exe(&dir, "zsh")), (exe(&dir, "noexec"), exe(&dir, "sh")), ("zsh".to_string(), exe(&dir, "zsh"))] {
        let mut h = Rec::new();
        let with_shell = programs.clone().with_shell(&shell);
        ok(&exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &ctx, &with_shell, &mut h));
        assert_eq!(h.spawned, vec![in_project(&[want])], "{shell}");
    }
    // mit `palette.project_command`: dieser Befehl, aufgelöst zu einem absoluten Pfad, mit seinen Argumenten
    let with_cmd = ExecCtx { prefs: ExecPrefs { project_command: vec!["claude".into(), "--continue".into()], ..search_prefs() }, ..ctx.clone() };
    let mut h = Rec::new();
    ok(&exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &with_cmd, &programs, &mut h));
    assert_eq!(h.spawned, vec![in_project(&[exe(&dir, "claude"), "--continue".into()])]);
    let missing = ExecCtx { prefs: ExecPrefs { project_command: vec!["missing".into()], ..search_prefs() }, ..ctx.clone() };
    let mut h = Rec::new();
    failed(
        &exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &missing, &programs, &mut h),
        "Program not found: missing",
    );
    assert!(h.nothing_done());
    // scheitert der Start, nennt die Meldung das Terminal, nicht env
    let mut h = Rec::new();
    h.spawn_fails = true;
    failed(&run(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), &mut h), "Could not start alacritty");
    // weder SHELL noch sh: eine Meldung, nichts startet
    let mut h = Rec::new();
    let only_term = Programs::with_dirs([dir.join("no-such-dir")]);
    failed(
        &exec_with(&cand(json!({"intent": "open_project", "query": "p"}), json!({"kind": "project", "path": tmp}), "run"), &ctx, &only_term, &mut h),
        "No shell found",
    );
    assert!(h.nothing_done());
    let mut h = Rec::new();
    ok(&run(json!({"intent": "open_folder", "query": "p"}), json!({"kind": "project", "path": tmp}), &mut h));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "xdg-open"), tmp.clone()]]);
    let mut h = Rec::new();
    failed(&run(json!({"intent": "open_folder", "query": "p"}), json!({"kind": "project", "path": "/nonexistent/p"}), &mut h), "Project folder not found");
    failed(&run(json!({"intent": "open_project", "query": "p"}), none(), &mut h), "no folder");
    assert!(h.nothing_done());

    let mut h = Rec::new();
    ok(&run(json!({"intent": "open_url", "url": "https://example.org/a?b=1"}), none(), &mut h));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "xdg-open"), "https://example.org/a?b=1".into()]]);
    // die Doppelprüfung: was der Daemon nie durchließe, startet auch hier nicht
    for bad in ["file:///etc/passwd", "--help", "https://example.org/a b", "javascript:alert(1)"] {
        let mut h = Rec::new();
        assert!(matches!(run(json!({"intent": "open_url", "url": bad}), none(), &mut h).out, Outcome::Failed(_)));
        assert!(h.nothing_done(), "{bad}");
    }

    let mut h = Rec::new();
    ok(&run(json!({"intent": "web_search", "query": "grüße & co"}), none(), &mut h));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "xdg-open"), "https://duckduckgo.com/?q=gr%C3%BC%C3%9Fe%20%26%20co".into()]]);
    let mut h = Rec::new();
    let r =
        exec_with(&cand(json!({"intent": "web_search", "query": "x"}), none(), "run"), &ExecCtx { confirmed: true, ..ExecCtx::default() }, &programs, &mut h);
    failed(&r, "No search engine");
    assert!(h.nothing_done());
}

#[test]
fn system_commands_run_fixed_programs_from_the_search_path() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
    let p = |name: &str| exe(&dir, name);
    for (intent, argv) in [
        (json!({"intent": "lock"}), vec![p("loginctl"), "lock-session".into()]),
        (json!({"intent": "reboot"}), vec![p("systemctl"), "reboot".into()]),
        (json!({"intent": "poweroff"}), vec![p("systemctl"), "poweroff".into()]),
        (json!({"intent": "suspend"}), vec![p("systemctl"), "suspend".into()]),
        (json!({"intent": "media", "action": "play_pause"}), vec![p("playerctl"), "play-pause".into()]),
        (json!({"intent": "media", "action": "next"}), vec![p("playerctl"), "next".into()]),
        (json!({"intent": "media", "action": "previous"}), vec![p("playerctl"), "previous".into()]),
        (json!({"intent": "show_widget", "widget": "clipboard_history"}), vec![p("noctalia"), "msg".into(), "panel-toggle".into(), "clipboard".into()]),
        (json!({"intent": "volume", "set": 150}), vec![p("pactl"), "set-sink-volume".into(), "@DEFAULT_SINK@".into(), "100%".into()]),
        (json!({"intent": "volume", "set": 30}), vec![p("pactl"), "set-sink-volume".into(), "@DEFAULT_SINK@".into(), "30%".into()]),
        (json!({"intent": "volume", "mute": true}), vec![p("pactl"), "set-sink-mute".into(), "@DEFAULT_SINK@".into(), "1".into()]),
        (json!({"intent": "volume", "mute": false}), vec![p("pactl"), "set-sink-mute".into(), "@DEFAULT_SINK@".into(), "0".into()]),
        (json!({"intent": "volume", "step": -10}), vec![p("pactl"), "set-sink-volume".into(), "@DEFAULT_SINK@".into(), "-10%".into()]),
    ] {
        let mut h = Rec::new();
        ok(&exec_with(&cand(intent.clone(), none(), "run"), &ctx, &programs, &mut h));
        assert_eq!(h.ran, vec![argv], "{intent}");
        assert!(h.ops.is_empty() && h.spawned.is_empty());
    }
    // ohne Programm: Meldung, kein Start
    let mut h = Rec::new();
    failed(&exec_with(&cand(json!({"intent": "lock"}), none(), "run"), &ctx, &Programs::with_dirs([]), &mut h), "Program not found: loginctl");
    assert!(h.nothing_done());
}

#[test]
fn volume_steps_never_go_above_100() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
    let reading = "Volume: front-left: 62259 /  95% / -1.34 dB,   front-right: 58982 /  90% / -2.75 dB\n        balance -0.05\n";
    let step = |n: i64, reply: Option<&str>| {
        let mut h = Rec::new();
        if let Some(r) = reply {
            h.replies.push_back(RunResult::Ok(r.into()));
        }
        let r = exec_with(&cand(json!({"intent": "volume", "step": n}), none(), "run"), &ctx, &programs, &mut h);
        (r, h.ran)
    };
    let (r, ran) = step(10, Some(reading));
    ok(&r);
    assert_eq!(ran[0][1..], ["get-sink-volume", "@DEFAULT_SINK@"]);
    assert_eq!(ran[1][1..], ["set-sink-volume", "@DEFAULT_SINK@", "+5%"]);
    let (r, ran) = step(3, Some(reading));
    ok(&r);
    assert_eq!(ran[1][3], "+3%");
    let (r, ran) = step(5, Some("Volume: mono: 65536 / 100% / 0.00 dB"));
    assert_eq!(r.out, Outcome::Done(Some("The volume is already at 100%")));
    assert_eq!(ran.len(), 1);
    let (r, _) = step(5, Some("Volume: mono: 98304 / 150% / 10.57 dB"));
    assert_eq!(r.out, Outcome::Done(Some("The volume is already at 100%")));
    let (r, ran) = step(5, Some("no numbers"));
    failed(&r, "unexpected pactl output");
    assert_eq!(ran.len(), 1);
    let (r, _) = step(0, None);
    failed(&r, "step is 0");
    let mut h = Rec::new();
    failed(&exec_with(&cand(json!({"intent": "volume"}), none(), "run"), &ctx, &programs, &mut h), "incomplete");
    assert!(h.nothing_done());
}

#[test]
fn sink_percent_takes_the_loudest_channel() {
    assert_eq!(sink_percent("Volume: front-left: 62259 /  95% / -1.34 dB,   front-right: 58982 /  90% / -2.75 dB"), Some(95));
    assert_eq!(sink_percent("Volume: mono: 98304 / 150% / 10.57 dB"), Some(150));
    assert_eq!(sink_percent("%"), None);
    assert_eq!(sink_percent("x"), None);
    assert_eq!(sink_percent("7%"), Some(7));
}

#[test]
fn widgets_and_settings() {
    let dir = bin_dir();
    let programs = Programs::with_dirs([dir.clone()]);
    let ctx = ExecCtx { confirmed: true, ..ExecCtx::default() };
    let spawned = |intent: serde_json::Value| {
        let mut h = Rec::new();
        let r = exec_with(&cand(intent, none(), "run"), &ctx, &programs, &mut h);
        (r, h.spawned)
    };
    let (r, s) = spawned(json!({"intent": "show_widget", "widget": "emoji"}));
    ok(&r);
    assert_eq!(s, vec![vec![exe(&dir, "rofi"), "-modi".into(), "emoji".into(), "-show".into(), "emoji".into()]]);
    // eingestellte Befehle (`palette.clipboard_command`, `palette.emoji_command`): über niri, nie als Kurzläufer
    let set = ExecCtx {
        prefs: ExecPrefs { clipboard_command: vec!["copyq".into(), "toggle".into()], emoji_command: vec!["bemoji".into()], ..ExecPrefs::default() },
        ..ctx.clone()
    };
    for (widget, argv) in [("clipboard_history", vec![exe(&dir, "copyq"), "toggle".into()]), ("emoji", vec![exe(&dir, "bemoji")])] {
        let mut h = Rec::new();
        ok(&exec_with(&cand(json!({"intent": "show_widget", "widget": widget}), none(), "run"), &set, &programs, &mut h));
        assert_eq!(h.spawned, vec![argv], "{widget}");
        assert!(h.ran.is_empty() && h.ops.is_empty(), "{widget}");
    }
    // ein absoluter Pfad gilt ohne Suchpfad; ein fehlendes Programm wird nicht gestartet
    let abs = ExecCtx { prefs: ExecPrefs { emoji_command: vec![exe(&dir, "bemoji"), "-t".into()], ..ExecPrefs::default() }, ..ctx.clone() };
    let mut h = Rec::new();
    ok(&exec_with(&cand(json!({"intent": "show_widget", "widget": "emoji"}), none(), "run"), &abs, &Programs::with_dirs([]), &mut h));
    assert_eq!(h.spawned, vec![vec![exe(&dir, "bemoji"), "-t".into()]]);
    let missing = ExecCtx { prefs: ExecPrefs { clipboard_command: vec!["missing".into()], ..ExecPrefs::default() }, ..ctx.clone() };
    let mut h = Rec::new();
    let r = exec_with(&cand(json!({"intent": "show_widget", "widget": "clipboard_history"}), none(), "run"), &missing, &programs, &mut h);
    failed(&r, "Program not found: missing");
    assert!(h.nothing_done());
    let (r, s) = spawned(json!({"intent": "dictate_settings", "page": "palette"}));
    ok(&r);
    assert_eq!(s, vec![vec![exe(&dir, "dictate-settings"), "--page".into(), "palette".into()]]);
    let (r, s) = spawned(json!({"intent": "dictate_settings"}));
    ok(&r);
    assert_eq!(s, vec![vec![exe(&dir, "dictate-settings")]]);
    let (r, s) = spawned(json!({"intent": "dictate_settings", "page": "--evil"}));
    failed(&r, "Unknown settings page");
    assert!(s.is_empty());
    // derselbe Start für „Settings“ in den Panels (`app::open_settings`); ohne das Programm im Suchpfad keiner
    assert_eq!(settings_launch(&programs, Some("actions")).unwrap().argv(), [exe(&dir, "dictate-settings"), "--page".into(), "actions".into()]);
    assert_eq!(settings_launch(&programs, None).unwrap().argv(), [exe(&dir, "dictate-settings")]);
    assert_eq!(settings_launch(&Programs::with_dirs([]), Some("actions")), None);
    let mut h = Rec::new();
    failed(&exec_with(&cand(json!({"intent": "dictate_settings"}), none(), "run"), &ctx, &Programs::with_dirs([]), &mut h), "not installed");
}

#[test]
fn panels_take_over_without_counting() {
    for (intent, want) in [
        (json!({"intent": "ask", "question": "why?"}), Handoff::Ask { question: "why?".into() }),
        (json!({"intent": "text_action", "action": "translate"}), Handoff::TextAction { action: "translate" }),
        (json!({"intent": "explain_clipboard"}), Handoff::ExplainClipboard),
        (
            json!({"intent": "diagnose", "profile": "unit", "unit": "nginx"}),
            Handoff::Diagnose { profile: Some("unit"), unit: Some("nginx".into()), container: None },
        ),
        (json!({"intent": "diagnose"}), Handoff::Diagnose { profile: None, unit: None, container: None }),
    ] {
        let mut h = Rec::new();
        let r = exec(&cand(intent.clone(), none(), "ai"), &mut h);
        assert_eq!(r.out, Outcome::Handoff(want), "{intent}");
        assert_eq!(r.chosen, 0);
        assert!(h.nothing_done());
    }
    for (intent, why) in [
        (json!({"intent": "ask", "question": "  "}), "Nothing to ask"),
        (json!({"intent": "text_action", "action": "rm -rf"}), "Unknown text action"),
        (json!({"intent": "diagnose", "profile": "root"}), "Unknown diagnosis profile"),
    ] {
        let mut h = Rec::new();
        failed(&exec(&cand(intent, none(), "ai"), &mut h), why);
    }
}

// --- Drahtformat und Suchpfad ---------------------------------------------------------------------------------------

#[test]
fn candidates_ignore_search_terms_and_reject_unknown_intents() {
    let c = cand(json!({"intent": "focus_window", "query": "firefox"}), json!({"kind": "window", "id": 4}), "run");
    assert_eq!(c.intent, Intent::FocusWindow {});
    assert_eq!(c.intent.name(), "focus_window");
    for bad in [
        json!({"intent": {"intent": "sh"}, "target": none(), "display": "d", "class": "run", "target_id": "t"}),
        json!({"intent": {"intent": "focus_window"}, "target": {"kind": "shell"}, "display": "d", "class": "run", "target_id": "t"}),
        json!({"intent": {"intent": "focus_window"}, "target": none(), "display": "d", "class": "now", "target_id": "t"}),
        json!({"intent": {"intent": "focus_window"}, "target": none(), "class": "run", "target_id": "t"}),
    ] {
        assert!(Candidate::from_value(&bad).is_none(), "{bad}");
    }
}

#[test]
fn programs_come_only_from_the_given_search_path() {
    // in Tests sieht die Sitzung keinen Ordner (nie die echten Programme)
    assert_eq!(Programs::session(), Programs::with_dirs([]));
    assert!(Programs::session().find("sh").is_none());
    let dir = bin_dir();
    let not_utf8 = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/dictate-\xff"));
    let p = Programs::with_dirs([PathBuf::from("relative"), not_utf8.clone(), dir.clone(), dir.clone()]);
    assert_eq!(p, Programs::with_dirs([dir.clone()]));
    assert!(!is_abs_dir(&not_utf8));
    assert_eq!(p.find("pactl").unwrap().path(), dir.join("pactl"));
    assert!(p.find("noexec").is_none());
    assert!(p.find("").is_none());
    assert!(p.find("pa\0ctl").is_none());
    assert!(p.find("../pactl").is_none());
    assert!(valid_argv(&["a".into()]));
    assert!(!valid_argv(&[]));
}
