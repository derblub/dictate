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

//! Wirt des Ausführers der Befehlspalette in `dictate-ui` (PLAN-wayland.md 6g, Block B): `ui::pal_exec::Host` über
//! niri-IPC (`niri.rs`) und Kurzläufer als Kindprozesse, dazu der Kontext für `pal_resolve`/`pal_ask` aus niri ohne das
//! eigene Panel (`ui::pal_context`, Lesart L4).
//!
//! - **Fenster ohne eigenes Panel:** `windows` nennt kein Fenster mit `app_id` `dictate-panel` **und** der PID dieses
//!   Prozesses (Sicherheitsregel 2); der Ausführer kann es darum nie treffen.
//! - **Kurzläufer** (`pactl`, `playerctl`, `systemctl`, `noctalia msg`): [`run_child`] ist die **einzige** Stelle in
//!   `dictate-ui`, die ein Programm mit nicht festem Namen startet, und nimmt nur `ui::pal_exec::Exe` (absoluter Pfad
//!   aus dem Suchpfad, nie Text aus Eingabe oder Modell; Test `no_shell_anywhere` in `primary.rs`). Kein stdin, Frist
//!   [`RUN_TIMEOUT`], danach `kill`; Ausgaben begrenzt; ein Enkel, der die Rohre offen hält, hält den Aufrufer nicht
//!   auf. Die Fehlerausgabe erscheint nur als erste Zeile im Overlay, nie im Log.
//! - **Langlebige Programme** gehen über `niri::spawn_at` (Vorlage E10), nicht über einen Kindprozess.
//!
//! In Tests gibt es keine Sitzung: [`NiriHost::session`] scheitert dort (`niri::socket`), Tests nehmen
//! [`NiriHost::at`] mit einem falschen niri im Temp-Ordner und Fake-Programme.

use crate::niri;
use crate::panel::APP_ID;
use crate::ui::pal_context::{self, Own};
use crate::ui::pal_exec::{Exe, Host, Launch, NiriOp, RunResult};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// Fenster, das vor dem Öffnen fokussiert war: niri-ID und `app_id`
pub type Focus = (u64, Option<String>);

/// Frist eines Kurzläufers (`systemctl` wartet etwa auf polkit, `pactl` auf PipeWire)
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// So lange wird nach dem Ende des Kindes noch auf seine Ausgaben gewartet (ein Enkel kann die Rohre offen halten)
const DRAIN: Duration = Duration::from_millis(500);
/// Längste gelesene Ausgabe je Rohr
const MAX_OUTPUT: u64 = 64 * 1024;
/// Längste gezeigte Fehlerzeile (Zeichen)
const MAX_ERROR_CHARS: usize = 160;

/// Der Wirt über niri-IPC
pub struct NiriHost {
    niri: PathBuf,
    pid: u32,
    run_timeout: Duration,
}

impl NiriHost {
    /// niri der Sitzung (`$NIRI_SOCKET`); in Tests nie
    pub fn session() -> Result<NiriHost, String> {
        Ok(NiriHost::at(niri::socket()?))
    }

    pub fn at(niri: PathBuf) -> NiriHost {
        NiriHost { niri, pid: std::process::id(), run_timeout: RUN_TIMEOUT }
    }

    fn own(&self) -> Own<'static> {
        Own { app_id: APP_ID, pid: self.pid }
    }

    /// Kontext-JSON für `pal_resolve`/`pal_ask` zu einem gegebenen Fokus (das Panel nimmt [`Self::opened`])
    #[cfg(test)]
    pub fn context(&self, prior_focus: Option<u64>) -> Result<String, String> {
        let windows = niri::windows_at(&self.niri)?;
        self.context_from(windows, prior_focus)
    }

    fn context_from(&self, windows: Vec<niri_ipc::Window>, prior_focus: Option<u64>) -> Result<String, String> {
        let workspaces = niri::workspaces_at(&self.niri)?.into_iter().map(niri::context_workspace).collect();
        Ok(pal_context::build(windows.into_iter().map(niri::context_window).collect(), workspaces, self.own(), prior_focus))
    }

    /// Beim Öffnen der Palette (vor dem Zeigen, im Hilfsthread): das fokussierte Fenster und der Kontext aus **einer**
    /// Fensterliste. Der Fokus ist nie das eigene Panel (`app_id` und PID; Sicherheitsregel 2): auch `ExecCtx` bekommt
    /// nur diesen gefilterten Wert. Scheitert niri, ist der Fokus unbekannt und der Kontext fehlt (`Err`: nur die Art
    /// ins Log)
    pub fn opened(&self) -> (Option<Focus>, Result<String, String>) {
        let windows = match niri::windows_at(&self.niri) {
            Ok(w) => w,
            Err(e) => return (None, Err(e)),
        };
        let own = self.own();
        // mit `app_id`: eine Übergabe an die Textaktionen (`text_action`) braucht sie für Replace (`replace_ok`)
        let focus = windows
            .iter()
            .find(|w| w.is_focused)
            .filter(|w| !(w.app_id.as_deref() == Some(own.app_id) && w.pid.and_then(|p| u32::try_from(p).ok()) == Some(own.pid)))
            .map(|w| (w.id, w.app_id.clone()));
        let context = self.context_from(windows, focus.as_ref().map(|f| f.0));
        (focus, context)
    }
}

impl Host for NiriHost {
    fn windows(&mut self) -> Result<Vec<u64>, String> {
        let own = self.own();
        Ok(niri::windows_at(&self.niri)?
            .into_iter()
            .filter(|w| !(w.app_id.as_deref() == Some(own.app_id) && w.pid.and_then(|p| u32::try_from(p).ok()) == Some(own.pid)))
            .map(|w| w.id)
            .collect())
    }

    fn workspaces(&mut self) -> Result<Vec<u64>, String> {
        Ok(niri::workspaces_at(&self.niri)?.into_iter().map(|w| w.id).collect())
    }

    fn focused_window(&mut self) -> Result<Option<u64>, String> {
        Ok(niri::focused_at(&self.niri)?.map(|f| f.id))
    }

    fn niri(&mut self, op: NiriOp) -> Result<(), String> {
        niri::run_op_at(&self.niri, op)
    }

    fn spawn(&mut self, launch: Launch) -> Result<(), String> {
        niri::spawn_at(&self.niri, launch)
    }

    fn run(&mut self, exe: &Exe, args: &[&str]) -> RunResult {
        run_child(exe, args, self.run_timeout)
    }
}

/// Ein Rohr, das ein eigener Thread begrenzt liest (den Rest verwirft er, damit das Kind nicht blockiert)
struct Drain {
    buf: Arc<Mutex<Vec<u8>>>,
    eof: mpsc::Receiver<()>,
}

impl Drain {
    fn new<R: Read + Send + 'static>(r: Option<R>) -> Drain {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (tx, eof) = mpsc::channel();
        if let Some(mut r) = r {
            let b = Arc::clone(&buf);
            let _ = std::thread::Builder::new().name("pal-run-pipe".into()).spawn(move || {
                let mut chunk = [0u8; 4096];
                loop {
                    match r.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut b = b.lock().unwrap_or_else(|e| e.into_inner());
                            let room = usize::try_from(MAX_OUTPUT).unwrap_or(usize::MAX).saturating_sub(b.len());
                            b.extend_from_slice(&chunk[..n.min(room)]);
                        }
                    }
                }
                let _ = tx.send(());
            });
        }
        Drain { buf, eof }
    }

    /// Was bis zum Ende des Rohrs (höchstens [`DRAIN`]) kam; hält ein Enkel das Rohr offen, das bis dahin Gelesene
    fn take(self) -> String {
        let _ = self.eof.recv_timeout(DRAIN);
        let b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&b).into_owned()
    }
}

/// Erste Zeile mit sichtbarem Inhalt, Steuerzeichen als Leerzeichen, höchstens [`MAX_ERROR_CHARS`] Zeichen
fn first_line(text: &str) -> Option<String> {
    let clean = |l: &str| l.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string();
    let line = text.lines().map(clean).find(|l| !l.is_empty())?;
    Some(match line.char_indices().nth(MAX_ERROR_CHARS - 3) {
        Some((i, _)) if line.chars().count() > MAX_ERROR_CHARS => format!("{}...", &line[..i]),
        _ => line,
    })
}

/// Startet einen Kurzläufer und wartet höchstens `timeout` (Modulkopf)
pub fn run_child(exe: &Exe, args: &[&str], timeout: Duration) -> RunResult {
    let mut child = match Command::new(exe.path()).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => return RunResult::Failed(format!("could not start ({})", e.kind())),
    };
    let out = Drain::new(child.stdout.take());
    let err = Drain::new(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return RunResult::Failed("no answer in time".into());
            }
            Err(e) => return RunResult::Failed(format!("lost the program ({})", e.kind())),
        }
    };
    let stdout = out.take();
    let stderr = err.take();
    if status.success() {
        return RunResult::Ok(stdout);
    }
    RunResult::Failed(first_line(&stderr).unwrap_or_else(|| match status.code() {
        Some(c) => format!("exit code {c}"),
        None => "stopped by a signal".into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pal_exec::{execute, Candidate, ExecCtx, Outcome, Programs};
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::OnceLock;

    /// Ein falsches niri im Temp-Ordner (nie das echte): je Verbindung eine Antwort aus `replies`; liefert die Anfragen
    fn fake_niri(tag: &str, replies: Vec<String>) -> (PathBuf, std::thread::JoinHandle<Vec<String>>) {
        let dir = std::env::temp_dir().join(format!("dictate-palhost-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("niri.sock");
        let l = UnixListener::bind(&path).unwrap();
        let h = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for r in replies {
                let (s, _) = l.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                (&s).write_all(r.as_bytes()).unwrap();
                asked.push(line.trim().to_string());
            }
            asked
        });
        (path, h)
    }

    const HANDLED: &str = "{\"Ok\":\"Handled\"}\n";

    fn window(id: u64, app: &str, pid: u32, title: &str) -> String {
        format!(
            r#"{{"id":{id},"title":"{title}","app_id":"{app}","pid":{pid},"workspace_id":7,"is_focused":false,"is_floating":false,"is_urgent":false,"layout":{{"pos_in_scrolling_layout":null,"tile_size":[1.0,1.0],"window_size":[1,1],"tile_pos_in_workspace_view":null,"window_offset_in_tile":[0.0,0.0]}},"focus_timestamp":{{"secs":{id},"nanos":0}}}}"#
        )
    }

    fn windows_reply(ws: &[String]) -> String {
        format!("{{\"Ok\":{{\"Windows\":[{}]}}}}\n", ws.join(","))
    }

    const WORKSPACES: &str = "{\"Ok\":{\"Workspaces\":[{\"id\":7,\"idx\":1,\"name\":null,\"output\":\"DP-1\",\"is_urgent\":false,\"is_active\":true,\"is_focused\":true,\"active_window_id\":2}]}}\n";

    #[test]
    fn the_own_panel_is_neither_a_window_nor_in_the_context() {
        let me = std::process::id();
        let wins = windows_reply(&[window(1, APP_ID, me, "Command palette"), window(2, APP_ID, me + 1, "other"), window(3, "firefox", me, "Mail")]);
        let (p, h) = fake_niri("own", vec![wins.clone(), wins, WORKSPACES.into()]);
        let mut host = NiriHost::at(p);
        assert_eq!(host.windows(), Ok(vec![2, 3]));
        let ctx: serde_json::Value = serde_json::from_str(&host.context(Some(1)).unwrap()).unwrap();
        let ids: Vec<u64> = ctx["windows"].as_array().unwrap().iter().map(|w| w["id"].as_u64().unwrap()).collect();
        assert_eq!(ids, vec![3, 2]);
        // der Fokus war das Panel: keiner
        assert!(ctx.get("focus").is_none());
        assert_eq!(ctx["workspaces"][0]["focused"], true);
        assert_eq!(h.join().unwrap(), vec![r#""Windows""#, r#""Windows""#, r#""Workspaces""#]);
    }

    #[test]
    fn the_focus_at_opening_is_never_the_own_panel() {
        let me = std::process::id();
        let focused = |w: String| w.replace("\"is_focused\":false", "\"is_focused\":true");
        // fokussiert ist ein fremdes Fenster: es ist der Fokus, auch im Kontext
        let wins = windows_reply(&[window(1, APP_ID, me, "Command palette"), focused(window(3, "firefox", me, "Mail"))]);
        let (p, h) = fake_niri("open-other", vec![wins, WORKSPACES.into()]);
        let (focus, ctx) = NiriHost::at(p).opened();
        assert_eq!(focus, Some((3, Some("firefox".to_string()))));
        let ctx: serde_json::Value = serde_json::from_str(&ctx.unwrap()).unwrap();
        assert_eq!(ctx["focus"], 3);
        assert_eq!(h.join().unwrap(), vec![r#""Windows""#, r#""Workspaces""#]);
        // fokussiert ist das eigene Panel: kein Fokus (ein fremdes Panel mit gleicher app_id wäre nur ein Fenster)
        let wins = windows_reply(&[focused(window(1, APP_ID, me, "Command palette")), window(3, "firefox", me, "Mail")]);
        let (p, h) = fake_niri("open-own", vec![wins, WORKSPACES.into()]);
        let (focus, ctx) = NiriHost::at(p).opened();
        assert_eq!(focus, None);
        let ctx: serde_json::Value = serde_json::from_str(&ctx.unwrap()).unwrap();
        assert!(ctx.get("focus").is_none());
        h.join().unwrap();
        // niri antwortet nicht: ohne Fokus und ohne Kontext
        let (focus, ctx) = NiriHost::at(std::env::temp_dir().join("dictate-palhost-none/niri.sock")).opened();
        assert_eq!(focus, None);
        assert!(ctx.is_err());
    }

    #[test]
    fn every_action_carries_its_fixed_id() {
        use crate::ui::pal_exec::ColumnDisplay;
        let cases = [
            (NiriOp::FocusWindow(5), r#"{"Action":{"FocusWindow":{"id":5}}}"#),
            (NiriOp::CloseWindow(5), r#"{"Action":{"CloseWindow":{"id":5}}}"#),
            (NiriOp::FocusWorkspace(77), r#"{"Action":{"FocusWorkspace":{"reference":{"Id":77}}}}"#),
            (NiriOp::FocusWorkspacePrevious, r#"{"Action":{"FocusWorkspacePrevious":{}}}"#),
            (
                NiriOp::MoveWindowToWorkspace { window: 5, workspace: 77 },
                r#"{"Action":{"MoveWindowToWorkspace":{"window_id":5,"reference":{"Id":77},"focus":false}}}"#,
            ),
            (NiriOp::ToggleFloating(5), r#"{"Action":{"ToggleWindowFloating":{"id":5}}}"#),
            (NiriOp::Fullscreen(5), r#"{"Action":{"FullscreenWindow":{"id":5}}}"#),
            (NiriOp::Maximize(5), r#"{"Action":{"MaximizeWindowToEdges":{"id":5}}}"#),
            (NiriOp::SetColumnDisplay(ColumnDisplay::Tabbed), r#"{"Action":{"SetColumnDisplay":{"display":"Tabbed"}}}"#),
            (NiriOp::SetColumnDisplay(ColumnDisplay::Normal), r#"{"Action":{"SetColumnDisplay":{"display":"Normal"}}}"#),
            (NiriOp::ScreenshotArea, r#"{"Action":{"Screenshot":{"show_pointer":true,"path":null}}}"#),
            (NiriOp::ScreenshotScreen, r#"{"Action":{"ScreenshotScreen":{"write_to_disk":true,"show_pointer":true,"path":null}}}"#),
            (NiriOp::ScreenshotWindow(5), r#"{"Action":{"ScreenshotWindow":{"id":5,"write_to_disk":true,"show_pointer":true,"path":null}}}"#),
            (NiriOp::Quit, r#"{"Action":{"Quit":{"skip_confirmation":true}}}"#),
        ];
        let (p, h) = fake_niri("ops", vec![HANDLED.to_string(); cases.len()]);
        let mut host = NiriHost::at(p);
        for (op, _) in cases {
            assert_eq!(host.niri(op), Ok(()));
        }
        let asked = h.join().unwrap();
        for ((op, want), got) in cases.iter().zip(asked) {
            assert_eq!(got, *want, "{op:?}");
        }
        // eine Ablehnung von niri ist ein Fehler
        let (p, h) = fake_niri("refused", vec!["{\"Err\":\"no such window\"}\n".into()]);
        assert!(NiriHost::at(p).niri(NiriOp::CloseWindow(9)).is_err());
        h.join().unwrap();
    }

    /// Fake-Programme, einmal je Testlauf geschrieben (danach nur noch ausgeführt: kein „Text file busy“)
    fn fakes() -> &'static Path {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("dictate-palhost-bin-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for (name, body) in [
                ("ok", "printf 'Volume: front-left: 1 /  42%% / x\\n'; printf '%s\\n' \"$@\" > \"$0.args\""),
                ("fails", "echo '' >&2; echo '  Access denied  ' >&2; echo second >&2; exit 3"),
                ("silent", "exit 4"),
                ("slow", "sleep 5"),
                ("chatty", "head -c 300000 /dev/zero; head -c 300000 /dev/zero >&2; exit 1"),
                ("forks", "sleep 3 & echo started"),
                ("firefox", "exit 0"),
            ] {
                let p = dir.join(name);
                std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            dir
        })
    }

    fn exe(name: &str) -> Exe {
        Programs::with_dirs([fakes().to_path_buf()]).find(name).unwrap()
    }

    /// `run_child` mit Wiederholung, falls ein anderer Test gerade forkt, während eine Datei offen ist (ETXTBSY)
    fn run(name: &str, args: &[&str], timeout: Duration) -> RunResult {
        for _ in 0..50 {
            match run_child(&exe(name), args, timeout) {
                RunResult::Failed(e) if e.contains("busy") => std::thread::sleep(Duration::from_millis(20)),
                r => return r,
            }
        }
        panic!("{name} stayed busy");
    }

    #[test]
    fn short_runners_report_output_and_errors() {
        let r = run("ok", &["a b", "--x"], RUN_TIMEOUT);
        assert_eq!(r, RunResult::Ok("Volume: front-left: 1 /  42% / x\n".into()));
        assert_eq!(std::fs::read_to_string(fakes().join("ok.args")).unwrap(), "a b\n--x\n");
        assert_eq!(run("fails", &[], RUN_TIMEOUT), RunResult::Failed("Access denied".into()));
        assert_eq!(run("silent", &[], RUN_TIMEOUT), RunResult::Failed("exit code 4".into()));
        // Ausgaben werden begrenzt gelesen, das Kind blockiert nicht
        assert_eq!(run("chatty", &[], RUN_TIMEOUT), RunResult::Failed("exit code 1".into()));
    }

    #[test]
    fn short_runners_end_at_the_deadline() {
        let t = Instant::now();
        assert_eq!(run("slow", &[], Duration::from_millis(200)), RunResult::Failed("no answer in time".into()));
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        // ein Enkel hält die Rohre offen: das Ergebnis kommt trotzdem bald
        let t = Instant::now();
        assert_eq!(run("forks", &[], RUN_TIMEOUT), RunResult::Ok("started\n".into()));
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn first_line_is_short() {
        assert_eq!(first_line("\n  \n x \n y"), Some("x".into()));
        assert_eq!(first_line(""), None);
        let long = first_line(&"ä".repeat(500)).unwrap();
        assert_eq!(long.chars().count(), MAX_ERROR_CHARS);
        assert!(long.ends_with("..."));
        assert_eq!(first_line(&"a".repeat(MAX_ERROR_CHARS)).unwrap().len(), MAX_ERROR_CHARS);
        // Steuerzeichen zählen nicht als Inhalt
        assert_eq!(first_line("\0\0\u{1b}\nreal\tline"), Some("real line".into()));
    }

    #[test]
    fn launch_goes_to_niri_spawn_with_the_absolute_path() {
        let (p, h) = fake_niri("spawn", vec![HANDLED.into()]);
        let mut host = NiriHost::at(p);
        let c = Candidate::from_value(&json!({
            "intent": {"intent": "launch_app", "query": "fire"},
            "target": {"kind": "app", "id": "firefox.desktop", "argv": ["firefox", "--new-window", "a b"], "terminal": false},
            "display": "Firefox", "class": "run", "target_id": "firefox.desktop"
        }))
        .unwrap();
        let mut chosen = 0;
        let ctx = ExecCtx { confirmed: false, ..ExecCtx::default() };
        let out = execute(&c, &ctx, &Programs::with_dirs([fakes().to_path_buf()]), &mut host, &mut || chosen += 1);
        assert_eq!(out, Outcome::Done(None));
        assert_eq!(chosen, 1);
        let firefox = fakes().join("firefox").to_string_lossy().into_owned();
        let want = json!({"Action": {"Spawn": {"command": [firefox, "--new-window", "a b"]}}}).to_string();
        assert_eq!(h.join().unwrap(), vec![want]);
    }

    #[test]
    fn no_session_in_tests() {
        assert!(NiriHost::session().is_err());
    }
}
