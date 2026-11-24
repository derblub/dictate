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

//! Fokussiertes Fenster über niri-IPC (`$NIRI_SOCKET`, Anfrage `FocusedWindow`): `app_id` für die Terminal-Erkennung
//! der Ausgabe und das Profil beim `start` (PLAN-wayland.md 3.4, 6c Punkt 5). Eigene kurze Fristen statt
//! `niri_ipc::socket::Socket` (das blockiert ohne Frist): hängt niri, gilt der Fokus als unbekannt, und die Ausgabe
//! tippt (im Zweifel tippen, Regel 4.8).
//!
//! Einzige Aktionen (Entscheidung 13, W3): die Höhe des **eigenen** Panels setzen (`SetWindowHeight` mit fester ID)
//! und es hochschieben, wenn es nach dem Wachsen unten aus dem Ausgang ragte (`MoveFloatingWindow`, nur relativ, W4).
//! Die ID kommt aus `Windows`, nur ein Fenster mit `app_id` `dictate-panel` **und** der PID dieses Prozesses gilt
//! ([`own_window`], vor dem Schieben noch einmal in [`placement`]); ohne Treffer wird nichts geändert. Nie eine Aktion
//! ohne ID (die träfe das fokussierte Fenster).
//!
//! Lage (Befund Block C, W4): niri behält beim Wachsen die Oberkante, wo es das schwebende Fenster beim Öffnen mittig
//! platziert hat, und klemmt es nicht auf den Ausgang. Darum nach jeder Höhenänderung: Lage aus `Windows`
//! (`tile_pos_in_workspace_view`, gilt sofort nach einer Aktion, auch mit Animationen; Spike im verschachtelten niri),
//! Ausgang aus `Workspaces`, logische Höhe aus `Outputs`; nur bei Überstand um genau diesen hoch ([`shift_into_view`]),
//! nie nach unten, also kein Springen beim Streaming. Relativ (`AdjustFixed`), weil `SetFixed` im Arbeitsbereich zählt,
//! den niri über IPC nicht nennt (Leisten). Eine Leiste unten (exklusive Zone) kennt die Rechnung nicht.
//!
//! Befehlspalette (W5, Block B): Abfragen `Windows`/`Workspaces` für Kontext und Ausführer und die geschlossene Liste
//! der Aktionen `ui::pal_exec::NiriOp` ([`run_op_at`], jede mit der festen ID aus dem Kandidaten, wo niri eine kennt),
//! dazu der einzige Start über niri ([`spawn_at`], `Spawn`, nie `SpawnSh`). Alles nur in der `_at`-Form mit einem
//! Socket-Pfad; [`socket`] liefert in den Unit-Tests keinen (die Shell läuft oft in der echten Sitzung).

use crate::ui::pal_context::{NiriWindow, NiriWorkspace};
use crate::ui::pal_exec::{ColumnDisplay, Launch, NiriOp};
use niri_ipc::{Action, Request, Response, WorkspaceReferenceArg};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tracing::{info, warn};

/// Frist je Anfrage; unter Last antwortete das verschachtelte niri in bis zu 960 ms. Eine unbekannte Antwort heißt
/// tippen statt einfügen und beendet die Ausgabe bei der Fokusprüfung, darum lieber etwas länger warten
const TIMEOUT: Duration = Duration::from_millis(1500);
/// Längste Antwortzeile (ein Fenster mit Titel; Titel können lang sein)
const MAX_REPLY: u64 = 256 * 1024;

/// Das fokussierte Fenster: niri-ID und `app_id` (leer = keine)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub id: u64,
    pub app_id: Option<String>,
}

/// `app_id` des fokussierten Fensters; `None`: kein Fenster fokussiert, keine `app_id`, kein niri oder Fehler
pub fn focused_app_id() -> Option<String> {
    focused().and_then(|f| f.app_id)
}

/// Das fokussierte Fenster; `None`: keins fokussiert, kein niri oder Fehler
pub fn focused() -> Option<Focus> {
    let Ok(path) = socket() else {
        log_once(false, "NIRI_SOCKET is not set: focus unknown, text output types instead of pasting");
        return None;
    };
    match focused_at(&path) {
        Ok(f) => {
            log_once(true, "niri IPC available");
            f
        }
        Err(e) => {
            warn!("niri IPC: {e}");
            None
        }
    }
}

/// Zustand fürs Log (0 unbekannt, 1 da, 2 fehlt): Verfügbarkeit nur beim Wechsel melden
static STATE: AtomicU8 = AtomicU8::new(0);

fn log_once(up: bool, msg: &str) {
    let new = if up { 1 } else { 2 };
    if STATE.swap(new, Ordering::Relaxed) != new {
        info!("{msg}");
    }
}

/// Eine Anfrage, eine Antwort. Fehler ohne Inhalte der Antwort (serde zitiert sonst Werte, etwa Fenstertitel)
fn ask(path: &Path, req: &Request, max: u64) -> Result<Response, String> {
    let mut s = UnixStream::connect(path).map_err(|e| format!("{}: {e}", path.display()))?;
    s.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
    line.push('\n');
    s.write_all(line.as_bytes()).map_err(|e| format!("write: {e}"))?;
    let mut line = String::new();
    BufReader::new((&s).take(max)).read_line(&mut line).map_err(|e| format!("read: {e}"))?;
    let reply: niri_ipc::Reply = serde_json::from_str(line.trim()).map_err(|e| format!("unexpected reply ({:?} at line {})", e.classify(), e.line()))?;
    reply.map_err(|_| "niri answered with an error".to_string())
}

pub fn focused_at(path: &Path) -> Result<Option<Focus>, String> {
    match ask(path, &Request::FocusedWindow, MAX_REPLY)? {
        Response::FocusedWindow(w) => Ok(w.map(|w| Focus { id: w.id, app_id: w.app_id.filter(|a| !a.is_empty()) })),
        _ => Err("unexpected reply".into()),
    }
}

/// Längste Antwort auf `Windows` (alle Fenster samt Titeln)
const MAX_WINDOWS_REPLY: u64 = 8 * 1024 * 1024;

/// Socket der niri-Sitzung aus `$NIRI_SOCKET`. In den Unit-Tests nie (dort läuft die Shell oft in der echten Sitzung):
/// Tests nehmen die `_at`-Formen mit einem falschen niri im Temp-Ordner
pub fn socket() -> Result<std::path::PathBuf, String> {
    #[cfg(not(test))]
    return std::env::var_os(niri_ipc::socket::SOCKET_PATH_ENV).map(Into::into).ok_or_else(|| "NIRI_SOCKET is not set".to_string());
    #[cfg(test)]
    Err("no niri session in tests".to_string())
}

/// niri-ID des eigenen Fensters mit `app_id` (und der PID dieses Prozesses); `None`: keins (noch nicht gemappt)
pub fn own_window(app_id: &str) -> Result<Option<u64>, String> {
    own_window_at(&socket()?, app_id, std::process::id())
}

fn own_window_at(path: &Path, app_id: &str, pid: u32) -> Result<Option<u64>, String> {
    match ask(path, &Request::Windows, MAX_WINDOWS_REPLY)? {
        Response::Windows(ws) => {
            Ok(ws.iter().find(|w| w.app_id.as_deref() == Some(app_id) && w.pid.and_then(|p| u32::try_from(p).ok()) == Some(pid)).map(|w| w.id))
        }
        _ => Err("unexpected reply".into()),
    }
}

/// Höhe (logische Pixel) des Fensters `id` setzen; nur für das eigene Panel ([`own_window`])
pub fn set_window_height(id: u64, height: i32) -> Result<(), String> {
    let req = Request::Action(niri_ipc::Action::SetWindowHeight { id: Some(id), change: niri_ipc::SizeChange::SetFixed(height) });
    match ask(&socket()?, &req, MAX_REPLY)? {
        Response::Handled => Ok(()),
        _ => Err("unexpected reply".into()),
    }
}

/// Abstand des Panels zur Unterkante (und höchstens so weit zur Oberkante) beim Hochschieben, logische Pixel
const EDGE: f64 = 16.0;

/// Lage des eigenen schwebenden Fensters auf seinem Ausgang (logische Pixel)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Oberkante der Kachel in der Ansicht des Arbeitsbereichs (Ursprung oben links am Ausgang)
    pub top: f64,
    /// Kachel minus Fenster (Rahmen von niri)
    pub frame: f64,
    /// Logische Höhe des Ausgangs
    pub output_height: f64,
}

/// Verschiebung nach oben (negativ), damit ein Fenster der Höhe `height` unten in den Ausgang passt; `None`: passt
/// schon. Nie nach unten; die Oberkante bleibt mindestens bei `EDGE` (oder wo sie schon höher steht)
pub fn shift_into_view(p: &Placement, height: i32) -> Option<f64> {
    let tile = f64::from(height) + p.frame;
    let bottom = p.top + tile;
    let limit = p.output_height - EDGE;
    if bottom <= limit + 0.5 {
        return None;
    }
    let top = (limit - tile).max(EDGE.min(p.top));
    let dy = top - p.top;
    (dy < -0.5).then_some(dy)
}

/// Lage des eigenen Fensters `id` (`app_id` und PID wie bei [`own_window`]); `None`: nicht da, nicht schwebend oder
/// Ausgang unbekannt
pub fn placement(id: u64, app_id: &str) -> Result<Option<Placement>, String> {
    placement_at(&socket()?, id, app_id, std::process::id())
}

fn placement_at(path: &Path, id: u64, app_id: &str, pid: u32) -> Result<Option<Placement>, String> {
    let w = match ask(path, &Request::Windows, MAX_WINDOWS_REPLY)? {
        Response::Windows(ws) => {
            ws.into_iter().find(|w| w.id == id && w.app_id.as_deref() == Some(app_id) && w.pid.and_then(|p| u32::try_from(p).ok()) == Some(pid))
        }
        _ => return Err("unexpected reply".into()),
    };
    let Some(w) = w.filter(|w| w.is_floating) else { return Ok(None) };
    let (Some((_, top)), Some(ws)) = (w.layout.tile_pos_in_workspace_view, w.workspace_id) else { return Ok(None) };
    let output = match ask(path, &Request::Workspaces, MAX_WINDOWS_REPLY)? {
        Response::Workspaces(all) => all.into_iter().find(|x| x.id == ws).and_then(|x| x.output),
        _ => return Err("unexpected reply".into()),
    };
    let Some(output) = output else { return Ok(None) };
    let height = match ask(path, &Request::Outputs, MAX_WINDOWS_REPLY)? {
        Response::Outputs(all) => all.get(&output).and_then(|o| o.logical.as_ref()).map(|l| f64::from(l.height)),
        _ => return Err("unexpected reply".into()),
    };
    Ok(height.map(|output_height| Placement { top, frame: w.layout.tile_size.1 - f64::from(w.layout.window_size.1), output_height }))
}

/// Das schwebende Fenster `id` um `dy` logische Pixel senkrecht verschieben; nur für das eigene Panel ([`placement`])
pub fn move_window_by(id: u64, dy: f64) -> Result<(), String> {
    let req = Request::Action(niri_ipc::Action::MoveFloatingWindow {
        id: Some(id),
        x: niri_ipc::PositionChange::AdjustFixed(0.0),
        y: niri_ipc::PositionChange::AdjustFixed(dy),
    });
    match ask(&socket()?, &req, MAX_REPLY)? {
        Response::Handled => Ok(()),
        _ => Err("unexpected reply".into()),
    }
}

// --- Befehlspalette (W5, PLAN-wayland.md 6g, Block B) -------------------------------------------------------------

/// Alle Fenster (samt Titeln; nie loggen)
pub fn windows_at(path: &Path) -> Result<Vec<niri_ipc::Window>, String> {
    match ask(path, &Request::Windows, MAX_WINDOWS_REPLY)? {
        Response::Windows(ws) => Ok(ws),
        _ => Err("unexpected reply".into()),
    }
}

/// Alle Arbeitsflächen
pub fn workspaces_at(path: &Path) -> Result<Vec<niri_ipc::Workspace>, String> {
    match ask(path, &Request::Workspaces, MAX_WINDOWS_REPLY)? {
        Response::Workspaces(ws) => Ok(ws),
        _ => Err("unexpected reply".into()),
    }
}

/// Fenster in der Form des Kontexts (`ui::pal_context`)
pub fn context_window(w: niri_ipc::Window) -> NiriWindow {
    NiriWindow { id: w.id, app_id: w.app_id, title: w.title, pid: w.pid, workspace_id: w.workspace_id, focus_ts: w.focus_timestamp.map(|t| (t.secs, t.nanos)) }
}

/// Arbeitsfläche in der Form des Kontexts
pub fn context_workspace(w: niri_ipc::Workspace) -> NiriWorkspace {
    NiriWorkspace { id: w.id, idx: w.idx, name: w.name, output: w.output, active: w.is_active, focused: w.is_focused }
}

/// Die Aktionen des Ausführers (geschlossene Liste `NiriOp`): jede mit der festen ID aus dem Kandidaten, wo niri eine
/// kennt. `SetColumnDisplay`, `FocusWorkspacePrevious` und die Aufnahmen von Bildschirm und Auswahl wirken auf den
/// Fokus (der Ausführer prüft ihn vorher, `ui::pal_exec`). Aufnahmen wie Print, Ctrl+Print und Alt+Print (Vorgaben
/// von niri: auf die Platte und in die Zwischenablage, mit Zeiger)
fn action_of(op: NiriOp) -> Action {
    match op {
        NiriOp::FocusWindow(id) => Action::FocusWindow { id },
        NiriOp::CloseWindow(id) => Action::CloseWindow { id: Some(id) },
        NiriOp::FocusWorkspace(id) => Action::FocusWorkspace { reference: WorkspaceReferenceArg::Id(id) },
        NiriOp::FocusWorkspacePrevious => Action::FocusWorkspacePrevious {},
        NiriOp::MoveWindowToWorkspace { window, workspace } => {
            Action::MoveWindowToWorkspace { window_id: Some(window), reference: WorkspaceReferenceArg::Id(workspace), focus: false }
        }
        NiriOp::ToggleFloating(id) => Action::ToggleWindowFloating { id: Some(id) },
        NiriOp::Fullscreen(id) => Action::FullscreenWindow { id: Some(id) },
        NiriOp::Maximize(id) => Action::MaximizeWindowToEdges { id: Some(id) },
        NiriOp::SetColumnDisplay(d) => Action::SetColumnDisplay {
            display: match d {
                ColumnDisplay::Normal => niri_ipc::ColumnDisplay::Normal,
                ColumnDisplay::Tabbed => niri_ipc::ColumnDisplay::Tabbed,
            },
        },
        NiriOp::ScreenshotArea => Action::Screenshot { show_pointer: true, path: None },
        NiriOp::ScreenshotScreen => Action::ScreenshotScreen { write_to_disk: true, show_pointer: true, path: None },
        NiriOp::ScreenshotWindow(id) => Action::ScreenshotWindow { id: Some(id), write_to_disk: true, show_pointer: true, path: None },
        NiriOp::Quit => Action::Quit { skip_confirmation: true },
    }
}

fn handled(path: &Path, action: Action) -> Result<(), String> {
    match ask(path, &Request::Action(action), MAX_REPLY)? {
        Response::Handled => Ok(()),
        _ => Err("unexpected reply".into()),
    }
}

/// Eine Aktion des Ausführers
pub fn run_op_at(path: &Path, op: NiriOp) -> Result<(), String> {
    handled(path, action_of(op))
}

/// Der einzige Start über niri (`Spawn`, Vorlage E10; nie `SpawnSh`): `argv[0]` ist ein absoluter Pfad aus
/// `ui::pal_exec::Programs`. niri meldet einen Startfehler nicht (`Handled` auch ohne Programm, Spike E10)
pub fn spawn_at(path: &Path, launch: Launch) -> Result<(), String> {
    handled(path, Action::Spawn { command: launch.into_argv() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// Ein falsches niri im Temp-Ordner (nie das echte `$NIRI_SOCKET`): beantwortet eine Anfrage mit `reply`
    fn fake(reply: &'static str) -> (std::path::PathBuf, std::thread::JoinHandle<String>) {
        let dir = std::env::temp_dir().join(format!("dictate-niri-test-{}-{}", std::process::id(), reply.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("niri.sock");
        let l = UnixListener::bind(&path).unwrap();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            (&s).write_all(reply.as_bytes()).unwrap();
            line
        });
        (path, h)
    }

    #[test]
    fn asks_for_the_focused_window() {
        let (p, h) = fake(
            r#"{"Ok":{"FocusedWindow":{"id":3,"title":"t","app_id":"Alacritty","pid":1,"workspace_id":1,"is_focused":true,"is_floating":false,"is_urgent":false,"layout":{"pos_in_scrolling_layout":null,"tile_size":[1.0,1.0],"window_size":[1,1],"tile_pos_in_workspace_view":null,"window_offset_in_tile":[0.0,0.0]},"focus_timestamp":null}}}
"#,
        );
        assert_eq!(focused_at(&p), Ok(Some(Focus { id: 3, app_id: Some("Alacritty".into()) })));
        assert_eq!(h.join().unwrap().trim(), r#""FocusedWindow""#);
    }

    #[test]
    fn own_window_needs_app_id_and_pid() {
        let w = |id: u64, app: &str, pid: u32| {
            format!(
                r#"{{"id":{id},"title":"t","app_id":"{app}","pid":{pid},"workspace_id":1,"is_focused":false,"is_floating":true,"is_urgent":false,"layout":{{"pos_in_scrolling_layout":null,"tile_size":[1.0,1.0],"window_size":[1,1],"tile_pos_in_workspace_view":null,"window_offset_in_tile":[0.0,0.0]}},"focus_timestamp":null}}"#
            )
        };
        let me = std::process::id();
        let reply = format!("{{\"Ok\":{{\"Windows\":[{},{},{}]}}}}\n", w(1, "dictate-panel", me + 1), w(2, "other", me), w(3, "dictate-panel", me));
        let reply: &'static str = Box::leak(reply.into_boxed_str());
        let (p, h) = fake(reply);
        assert_eq!(own_window_at(&p, "dictate-panel", me), Ok(Some(3)));
        assert_eq!(h.join().unwrap().trim(), r#""Windows""#);
        let (p, h) = fake("{\"Ok\":{\"Windows\":[]}}\n");
        assert_eq!(own_window_at(&p, "dictate-panel", me), Ok(None));
        h.join().unwrap();
    }

    /// Ein falsches niri, das nacheinander je Verbindung eine Antwort aus `replies` gibt; liefert die Anfragen
    fn fake_seq(tag: &str, replies: Vec<String>) -> (std::path::PathBuf, std::thread::JoinHandle<Vec<String>>) {
        let dir = std::env::temp_dir().join(format!("dictate-niri-seq-{}-{tag}", std::process::id()));
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

    fn window(id: u64, app: &str, pid: u32, floating: bool, top: f64, tile_h: f64, win_h: i32) -> String {
        format!(
            r#"{{"id":{id},"title":"t","app_id":"{app}","pid":{pid},"workspace_id":7,"is_focused":true,"is_floating":{floating},"is_urgent":false,"layout":{{"pos_in_scrolling_layout":null,"tile_size":[800.0,{tile_h}],"window_size":[800,{win_h}],"tile_pos_in_workspace_view":[400.0,{top}],"window_offset_in_tile":[0.0,0.0]}},"focus_timestamp":null}}"#
        )
    }

    const WORKSPACES: &str = r#"{"Ok":{"Workspaces":[{"id":3,"idx":1,"name":null,"output":"DP-1","is_urgent":false,"is_active":true,"is_focused":false,"active_window_id":null},{"id":7,"idx":2,"name":null,"output":"DP-4","is_urgent":false,"is_active":true,"is_focused":true,"active_window_id":2}]}}"#;

    fn outputs() -> String {
        let o = |name: &str, h: u32| {
            format!(
                r#""{name}":{{"name":"{name}","make":"m","model":"m","serial":null,"physical_size":null,"modes":[],"current_mode":null,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":false,"logical":{{"x":0,"y":0,"width":1600,"height":{h},"scale":1.625,"transform":"Normal"}}}}"#
            )
        };
        format!("{{\"Ok\":{{\"Outputs\":{{{},{}}}}}}}\n", o("DP-1", 500), o("DP-4", 899))
    }

    #[test]
    fn placement_reads_the_output_of_the_own_window() {
        let me = std::process::id();
        let wins = format!(
            "{{\"Ok\":{{\"Windows\":[{},{}]}}}}\n",
            window(1, "dictate-panel", me + 1, true, 0.0, 10.0, 10),
            window(2, "dictate-panel", me, true, 150.0, 602.0, 600)
        );
        let (p, h) = fake_seq("place", vec![wins, format!("{WORKSPACES}\n"), outputs()]);
        assert_eq!(placement_at(&p, 2, "dictate-panel", me), Ok(Some(Placement { top: 150.0, frame: 2.0, output_height: 899.0 })));
        assert_eq!(h.join().unwrap(), vec![r#""Windows""#, r#""Workspaces""#, r#""Outputs""#]);
    }

    #[test]
    fn placement_only_for_the_own_floating_window() {
        let me = std::process::id();
        // fremde PID mit derselben ID: nichts, keine weitere Anfrage
        let wins = format!("{{\"Ok\":{{\"Windows\":[{}]}}}}\n", window(2, "dictate-panel", me + 1, true, 150.0, 600.0, 600));
        let (p, h) = fake_seq("foreign", vec![wins]);
        assert_eq!(placement_at(&p, 2, "dictate-panel", me), Ok(None));
        assert_eq!(h.join().unwrap().len(), 1);
        // gekachelt: nichts verschieben
        let wins = format!("{{\"Ok\":{{\"Windows\":[{}]}}}}\n", window(2, "dictate-panel", me, false, 150.0, 600.0, 600));
        let (p, h) = fake_seq("tiled", vec![wins]);
        assert_eq!(placement_at(&p, 2, "dictate-panel", me), Ok(None));
        h.join().unwrap();
    }

    #[test]
    fn shifts_up_only_by_the_overflow() {
        let p = Placement { top: 150.0, frame: 0.0, output_height: 899.0 };
        // passt (Unterkante 150 + 700 = 850 ≤ 883)
        assert_eq!(shift_into_view(&p, 700), None);
        assert_eq!(shift_into_view(&p, 733), None);
        // ragt hinaus: genau um den Überstand hoch
        assert_eq!(shift_into_view(&p, 800), Some(-67.0));
        // mit Rahmen zählt die Kachel
        assert_eq!(shift_into_view(&Placement { frame: 4.0, ..p }, 800), Some(-71.0));
        // höher als der Ausgang: Oberkante bei EDGE, nie darüber
        assert_eq!(shift_into_view(&p, 1000), Some(EDGE - 150.0));
        // steht schon höher als EDGE: nie nach unten
        assert_eq!(shift_into_view(&Placement { top: 4.0, ..p }, 1000), None);
        // bereits am Rand nach einem früheren Schieben: nichts mehr
        assert_eq!(shift_into_view(&Placement { top: 83.0, ..p }, 800), None);
    }

    #[test]
    fn no_window_or_error() {
        let (p, h) = fake("{\"Ok\":{\"FocusedWindow\":null}}\n");
        assert_eq!(focused_at(&p), Ok(None));
        h.join().unwrap();
        let (p, h) = fake("{\"Err\":\"nope\"}\n");
        assert!(focused_at(&p).is_err());
        h.join().unwrap();
        // Fehler zitieren keine Werte der Antwort (Fenstertitel)
        let (p, h) = fake("{\"Ok\":{\"FocusedWindow\":{\"id\":\"geheimer Titel\"}}}\n");
        let e = focused_at(&p).unwrap_err();
        assert!(!e.contains("geheim"), "{e}");
        h.join().unwrap();
        assert!(focused_at(Path::new("/nonexistent/niri.sock")).is_err());
    }
}
