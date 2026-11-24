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

//! Kontext der Befehlspalette aus niri (PLAN-wayland.md 6g, Block B, Lesart L4): was `dictate-ui` beim Öffnen über
//! Fenster und Arbeitsflächen weiß, als JSON für das Feld `context` von `pal_resolve`/`pal_ask`. Form und Grenzen
//! wie `palette::resolve::context` im Daemon (dort ein Spiegeltest auf Seite der Bibliothek: gleiche Grenzen, jede
//! Ausgabe besteht `Context::from_json`); dieses Modul kennt den Daemon nicht, weil `dictate-ui` nur `ui` einbindet.
//!
//! Regeln:
//! - **Ohne eigenes Panel:** ein Fenster mit der `app_id` des Panels **und** der PID dieses Prozesses fehlt
//!   (Sicherheitsregel 2; dasselbe Paar wie `niri::own_window`).
//! - **Fokus** ist das Fenster, das vor dem Öffnen fokussiert war (vom Aufrufer gemerkt), nie das Panel; `focused` je
//!   Fenster folgt daraus, nicht aus niri (dort wäre es das Panel, sobald es offen ist).
//! - **Kappen statt ablehnen:** Titel, `app_id`, Name und Monitor nach Zeichen auf die Grenzen des Daemons; höchstens
//!   [`MAX_WINDOWS`] Fenster, die zuletzt fokussierten zuerst (`focus_timestamp`); Arbeitsflächen mit `idx` 0, doppelter
//!   ID oder doppeltem `idx` je Monitor (nach dem Kappen der Namen) fallen weg, höchstens [`MAX_WORKSPACES`].
//! - **Budget in Bytes des fertigen JSON** ([`CONTEXT_BUDGET`]): die Arbeitsflächen bekommen höchstens ein Viertel
//!   (sonst verdrängten Namen voller Steuerzeichen alle Fenster), überzählige fallen von hinten weg; reicht der Rest
//!   nicht, fallen die am längsten nicht fokussierten Fenster weg. Steuerzeichen kosten maskiert 6 Bytes je Zeichen;
//!   gemessen wird darum die serialisierte Form, nicht die Zeichenzahl.
//!
//! Der Kontext lebt nur für die Anfragen dieses Panels im Speicher; er wird nie geloggt (Sicherheitsregel 4).

use serde::Serialize;

/// Grenzen wie `palette::resolve::context` (Spiegeltest im Daemon)
pub const MAX_WINDOWS: usize = 500;
pub const MAX_WORKSPACES: usize = 256;
pub const MAX_TITLE_CHARS: usize = 512;
pub const MAX_APP_ID_CHARS: usize = 128;
pub const MAX_NAME_CHARS: usize = 80;
/// Größtes Kontext-JSON (Bytes). Die ganze Zeile von `pal_resolve`/`pal_ask` darf 512 KiB lang sein, mit Eingabe und
/// den vier Werten der Sitzungsumgebung (je bis 16 KiB, maskiert bis doppelt so lang); ein Test im Daemon rechnet den
/// schlimmsten Fall nach
pub const CONTEXT_BUDGET: usize = 256 * 1024;

/// Ein Fenster, wie es niri meldet (`niri_ipc::Window`, nur die gebrauchten Felder)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiriWindow {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<i32>,
    pub workspace_id: Option<u64>,
    /// `focus_timestamp` (Sekunden, Nanosekunden; monoton); `None` = nie fokussiert
    pub focus_ts: Option<(u64, u32)>,
}

/// Eine Arbeitsfläche, wie sie niri meldet (`niri_ipc::Workspace`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiriWorkspace {
    pub id: u64,
    pub idx: u8,
    pub name: Option<String>,
    pub output: Option<String>,
    pub active: bool,
    pub focused: bool,
}

/// Das eigene Panel: `app_id` und PID dieses Prozesses
#[derive(Debug, Clone, Copy)]
pub struct Own<'a> {
    pub app_id: &'a str,
    pub pid: u32,
}

impl NiriWindow {
    fn is_own(&self, own: Own) -> bool {
        self.app_id.as_deref() == Some(own.app_id) && self.pid.and_then(|p| u32::try_from(p).ok()) == Some(own.pid)
    }
}

#[derive(Serialize)]
struct WorkspaceOut {
    id: u64,
    idx: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    active: bool,
    focused: bool,
}

#[derive(Serialize)]
struct WindowOut {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_id: Option<u64>,
    focused: bool,
}

#[derive(Serialize)]
struct ContextOut<'a> {
    workspaces: &'a [WorkspaceOut],
    windows: &'a [WindowOut],
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<u64>,
}

/// Höchstens `max` Zeichen
fn cap(s: Option<String>, max: usize) -> Option<String> {
    s.map(|s| match s.char_indices().nth(max) {
        Some((i, _)) => s[..i].to_string(),
        None => s,
    })
}

fn to_json(workspaces: &[WorkspaceOut], windows: &[WindowOut], focus: Option<u64>) -> String {
    serde_json::to_string(&ContextOut { workspaces, windows, focus }).expect("Kontext serialisierbar")
}

/// Bytes eines Elements in der Liste (mit Komma)
fn size_of<T: Serialize>(item: &T) -> usize {
    serde_json::to_string(item).map_or(0, |s| s.len()) + 1
}

/// Baut das Kontext-JSON (Modulkopf). `focus` ist das vor dem Öffnen fokussierte Fenster; fehlt es in der Liste (oder
/// ist es das eigene Panel), hat der Kontext keinen Fokus.
pub fn build(windows: Vec<NiriWindow>, workspaces: Vec<NiriWorkspace>, own: Own, focus: Option<u64>) -> String {
    build_within(windows, workspaces, own, focus, CONTEXT_BUDGET)
}

fn build_within(windows: Vec<NiriWindow>, workspaces: Vec<NiriWorkspace>, own: Own, focus: Option<u64>, budget: usize) -> String {
    // Arbeitsflächen: gültig und eindeutig nach dem Kappen, eine fokussierte
    let mut ws_out: Vec<WorkspaceOut> = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut places = std::collections::HashSet::new();
    let mut seen_focus = false;
    for w in workspaces {
        let output = cap(w.output, MAX_NAME_CHARS);
        if w.idx == 0 || ids.contains(&w.id) || !places.insert((output.clone().unwrap_or_default(), w.idx)) {
            continue;
        }
        if ws_out.len() == MAX_WORKSPACES {
            break;
        }
        ids.insert(w.id);
        let focused = w.focused && !seen_focus;
        seen_focus |= focused;
        ws_out.push(WorkspaceOut { id: w.id, idx: w.idx, name: cap(w.name, MAX_NAME_CHARS), output, active: w.active, focused });
    }

    // Fenster: ohne eigenes Panel, eindeutig, zuletzt fokussierte zuerst
    let mut windows: Vec<NiriWindow> = windows.into_iter().filter(|w| !w.is_own(own)).collect();
    windows.sort_by_key(|w| std::cmp::Reverse(w.focus_ts));
    let mut ids = std::collections::HashSet::new();
    windows.retain(|w| ids.insert(w.id));
    windows.truncate(MAX_WINDOWS);
    let focus = focus.filter(|f| windows.iter().any(|w| w.id == *f));
    let mut win_out: Vec<WindowOut> = windows
        .into_iter()
        .map(|w| WindowOut {
            id: w.id,
            app_id: cap(w.app_id, MAX_APP_ID_CHARS),
            title: cap(w.title, MAX_TITLE_CHARS),
            workspace_id: w.workspace_id,
            focused: Some(w.id) == focus,
        })
        .collect();

    // Budget: Arbeitsflächen höchstens ein Viertel, dann Fenster von hinten (am längsten nicht fokussiert; das
    // fokussierte steht vorn und fällt als letztes). Schätzung je Element (das letzte einer Liste hat kein Komma, die
    // Schätzung kann um ein Byte zu klein sein), danach exakt nachmessen
    let mut ws_bytes: usize = ws_out.iter().map(size_of).sum();
    while ws_bytes > budget / 4 && !ws_out.is_empty() {
        ws_bytes -= size_of(&ws_out.pop().expect("nicht leer"));
    }
    let mut total = to_json(&ws_out, &win_out, focus).len();
    while total > budget && !win_out.is_empty() {
        total -= size_of(&win_out.pop().expect("nicht leer"));
    }
    loop {
        let focus = focus.filter(|f| win_out.iter().any(|w| w.id == *f));
        let json = to_json(&ws_out, &win_out, focus);
        if json.len() <= budget || (win_out.is_empty() && ws_out.is_empty()) {
            return json;
        }
        if win_out.pop().is_none() {
            ws_out.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const OWN: Own = Own { app_id: "dictate-panel", pid: 4242 };

    fn win(id: u64, app: &str, title: &str, ws: u64, ts: u64) -> NiriWindow {
        NiriWindow {
            id,
            app_id: Some(app.into()),
            title: Some(title.into()),
            pid: Some(1000 + i32::try_from(id).unwrap()),
            workspace_id: Some(ws),
            focus_ts: Some((ts, 0)),
        }
    }

    fn ws(id: u64, idx: u8, output: &str) -> NiriWorkspace {
        NiriWorkspace { id, idx, name: None, output: Some(output.into()), active: idx == 1, focused: false }
    }

    fn parse(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    fn ids(v: &Value, list: &str) -> Vec<u64> {
        v[list].as_array().unwrap().iter().map(|w| w["id"].as_u64().unwrap()).collect()
    }

    #[test]
    fn leaves_out_the_own_panel_and_orders_by_focus() {
        let mut own = win(9, "dictate-panel", "Command palette", 1, 99);
        own.pid = Some(4242);
        // gleiche app_id, fremde PID: ein anderes Programm, bleibt
        let other_panel = win(8, "dictate-panel", "x", 1, 1);
        let v = parse(&build(
            vec![win(1, "firefox", "Mail", 1, 5), own, win(2, "Alacritty", "zsh", 2, 7), other_panel],
            vec![ws(1, 1, "DP-1"), ws(2, 2, "DP-1")],
            OWN,
            Some(2),
        ));
        assert_eq!(ids(&v, "windows"), vec![2, 1, 8]);
        assert_eq!(v["focus"], 2);
        assert_eq!(v["windows"][0]["focused"], true);
        assert_eq!(v["windows"][1]["focused"], false);
        assert_eq!(v["windows"][0]["workspace_id"], 2);
        assert_eq!(v["workspaces"][1]["idx"], 2);
        assert_eq!(v["workspaces"][0]["output"], "DP-1");
        assert!(v["workspaces"][0].get("name").is_none());
    }

    #[test]
    fn the_panel_is_never_the_focus() {
        let mut own = win(9, "dictate-panel", "p", 1, 99);
        own.pid = Some(4242);
        let v = parse(&build(vec![own, win(1, "a", "t", 1, 1)], vec![ws(1, 1, "DP-1")], OWN, Some(9)));
        assert!(v.get("focus").is_none());
        assert!(v["windows"].as_array().unwrap().iter().all(|w| w["focused"] == false));
        // unbekannter Fokus ebenso
        let v = parse(&build(vec![win(1, "a", "t", 1, 1)], vec![], OWN, Some(77)));
        assert!(v.get("focus").is_none());
    }

    #[test]
    fn caps_by_characters() {
        let long = "ü".repeat(MAX_TITLE_CHARS + 10);
        let mut w = win(1, &"a".repeat(MAX_APP_ID_CHARS + 1), &long, 1, 1);
        w.focus_ts = None;
        let mut space = ws(1, 1, &"o".repeat(MAX_NAME_CHARS + 5));
        space.name = Some("n".repeat(MAX_NAME_CHARS * 2));
        let v = parse(&build(vec![w], vec![space], OWN, None));
        assert_eq!(v["windows"][0]["title"].as_str().unwrap().chars().count(), MAX_TITLE_CHARS);
        assert_eq!(v["windows"][0]["app_id"].as_str().unwrap().chars().count(), MAX_APP_ID_CHARS);
        assert_eq!(v["workspaces"][0]["name"].as_str().unwrap().chars().count(), MAX_NAME_CHARS);
        assert_eq!(v["workspaces"][0]["output"].as_str().unwrap().chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn workspaces_stay_valid() {
        let mut focused = ws(5, 2, "DP-1");
        focused.focused = true;
        let mut second_focus = ws(6, 3, "DP-1");
        second_focus.focused = true;
        // zwei Monitore, deren Namen nach dem Kappen gleich sind: der zweite idx 1 fiele doppelt an
        let a = "x".repeat(MAX_NAME_CHARS) + "A";
        let b = "x".repeat(MAX_NAME_CHARS) + "B";
        let mut none_output = ws(10, 1, "");
        none_output.output = None;
        let mut none_output_dup = ws(11, 1, "");
        none_output_dup.output = None;
        let v = parse(&build(
            vec![],
            vec![
                ws(1, 1, "DP-1"),
                ws(1, 4, "DP-1"),
                ws(2, 0, "DP-1"),
                ws(3, 1, "DP-1"),
                focused,
                second_focus,
                ws(7, 1, &a),
                ws(8, 1, &b),
                none_output,
                none_output_dup,
            ],
            OWN,
            None,
        ));
        assert_eq!(ids(&v, "workspaces"), vec![1, 5, 6, 7, 10]);
        let focused: Vec<bool> = v["workspaces"].as_array().unwrap().iter().map(|w| w["focused"].as_bool().unwrap()).collect();
        assert_eq!(focused, vec![false, true, false, false, false]);
        let many: Vec<NiriWorkspace> = (1..=300).map(|i| ws(i, u8::try_from(i % 250 + 1).unwrap(), &format!("O{}", i / 250))).collect();
        assert_eq!(parse(&build(vec![], many, OWN, None))["workspaces"].as_array().unwrap().len(), MAX_WORKSPACES);
    }

    #[test]
    fn at_most_max_windows_most_recent_first() {
        let all: Vec<NiriWindow> = (1..=MAX_WINDOWS as u64 + 20).map(|i| win(i, "a", "t", 1, i)).collect();
        let v = parse(&build(all, vec![], OWN, Some(3)));
        let got = ids(&v, "windows");
        assert_eq!(got.len(), MAX_WINDOWS);
        assert_eq!(got[0], MAX_WINDOWS as u64 + 20);
        // das älteste fokussierte fiel weg, und damit der Fokus
        assert!(!got.contains(&3));
        assert!(v.get("focus").is_none());
        // doppelte IDs nur einmal
        let v = parse(&build(vec![win(1, "a", "t", 1, 2), win(1, "b", "u", 1, 1)], vec![], OWN, None));
        assert_eq!(ids(&v, "windows"), vec![1]);
        assert_eq!(v["windows"][0]["app_id"], "a");
    }

    #[test]
    fn keeps_the_byte_budget_with_escaped_titles() {
        // Steuerzeichen kosten maskiert 6 Bytes: 512 Zeichen werden über 3 KB
        let title = "\u{1}".repeat(MAX_TITLE_CHARS);
        let all: Vec<NiriWindow> = (1..=MAX_WINDOWS as u64).map(|i| win(i, "a", &title, 1, i)).collect();
        let json = build(all, vec![ws(1, 1, "DP-1")], OWN, Some(MAX_WINDOWS as u64));
        assert!(json.len() <= CONTEXT_BUDGET, "{}", json.len());
        let v = parse(&json);
        let got = ids(&v, "windows");
        assert!(got.len() < MAX_WINDOWS && got.len() > 10, "{}", got.len());
        // die jüngsten bleiben, der Fokus auch
        assert_eq!(got[0], MAX_WINDOWS as u64);
        assert_eq!(v["focus"], MAX_WINDOWS as u64);
        // nahe am Budget, nicht weit darunter (je Fenster rund 3,1 KB)
        assert!(json.len() > CONTEXT_BUDGET - 4 * 1024, "{}", json.len());
    }

    #[test]
    fn workspaces_get_a_quarter_and_everything_always_fits() {
        let mut big = ws(1, 1, &"\u{2}".repeat(MAX_NAME_CHARS));
        big.name = Some("\u{3}".repeat(MAX_NAME_CHARS));
        // die große Arbeitsfläche (rund 1 KB maskiert) sprengt das Viertel und fällt, die Fenster bleiben
        let small = build_within(vec![win(1, "a", "t", 1, 1)], vec![ws(2, 2, "DP-1"), big.clone()], OWN, Some(1), 400);
        assert!(small.len() <= 400, "{small}");
        let v = parse(&small);
        assert_eq!(ids(&v, "windows"), vec![1]);
        assert_eq!(ids(&v, "workspaces"), vec![2]);
        assert_eq!(v["focus"], 1);
        // reicht es auch für die Fenster nicht, fallen sie bis auf das fokussierte
        let one = build_within(vec![win(2, "b", "u", 1, 2)], vec![], OWN, Some(2), usize::MAX).len();
        let v = parse(&build_within(vec![win(1, "a", "t", 1, 1), win(2, "b", "u", 1, 2)], vec![], OWN, Some(2), one + 5));
        assert_eq!(ids(&v, "windows"), vec![2]);
        // knappes Budget: das Viertel reicht für keine Arbeitsfläche, das Fenster bleibt
        let v = parse(&build_within(vec![win(1, "a", "t", 1, 1)], vec![ws(2, 2, "DP-1"), big.clone()], OWN, Some(1), 120));
        assert!(v["workspaces"].as_array().unwrap().is_empty());
        assert_eq!(ids(&v, "windows"), vec![1]);
        // jede Grenze in Bytes hält, auch wo die Schätzung um ein Byte daneben läge
        for budget in 30..400 {
            let json = build_within(vec![win(1, "a", "t", 1, 1), win(2, "b", "u", 1, 2)], vec![ws(2, 2, "DP-1"), big.clone()], OWN, Some(2), budget);
            assert!(json.len() <= budget, "{budget}: {json}");
        }
        // ein zu kleines Budget: leerer Kontext statt Überlauf
        let tiny = build_within(vec![win(1, "a", "t", 1, 1)], vec![big], OWN, None, 40);
        assert_eq!(tiny, r#"{"workspaces":[],"windows":[]}"#);
    }
}
