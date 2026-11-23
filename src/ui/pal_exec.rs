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

//! Ausführer der Befehlspalette unter niri (PLAN-wayland.md 6g, Block B; Port von `palette_exec.lua`, Stand `30cad69`).
//!
//! Der Automat des Panels (Block C) ruft [`execute`], nachdem das Panel zu ist und eine nötige Bestätigung erledigt
//! ist. Je Absicht des Katalogs gibt es genau einen Zweig; das Ergebnis ist **ein** Rückgabewert ([`Outcome`]), also
//! „`done` genau einmal“ schon durch den Typ. `chosen` (= `pal_chosen`, die Häufigkeit) ruft [`execute`] höchstens
//! einmal und nur bei Erfolg (Lesart L7): nach `Handled` von niri, nach der Übergabe an niri `Spawn`, nach dem Ende
//! eines Kurzläufers mit Code 0. Die Absichten, die ein anderes Panel öffnen (`ask`, `text_action`,
//! `explain_clipboard`, `diagnose`), geben [`Outcome::Handoff`] zurück; `chosen` kommt dann vom Aufrufer, sobald das
//! Ziel-Panel offen ist. `logout` meldet `chosen` **vor** `Quit` (Lesart L14 in 6g: niri beendet die Sitzung und damit
//! `dictate-ui`; scheitert `Quit`, ist die Wahl trotzdem gezählt, wie unter Awesome).
//!
//! **Sicherheitsregeln** (6g, 1–3 und 6):
//! - **Ziel nur aus dem Kandidaten** ([`Target`]): Fenster und Arbeitsflächen nur nach fester niri-ID, nie neu
//!   aufgelöst, nie nach Name oder `idx` (L10). Die Spiegeltypen hier lesen `workspace` und `query` der Absicht gar
//!   nicht erst. Passt das Ziel nicht zur Absicht, geschieht nichts. Einzige Ausnahme: `screenshot` `window` kommt vom
//!   Daemon ohne Ziel und nimmt das Fenster, das vor dem Öffnen fokussiert war ([`ExecCtx::prior_focus`]).
//! - **Vorhanden?** Vor jeder Fensteraktion steht die ID noch in `Windows` (ohne das eigene Panel, das filtert der
//!   [`Host`]), vor jeder Aktion auf einer Arbeitsfläche in `Workspaces`; sonst „That window is gone“.
//! - **Aktionen ohne ID** (`SetColumnDisplay` wirkt auf die fokussierte Spalte, Lesart L15): erst `FocusWindow{id}`,
//!   dann prüfen, dass genau dieses Fenster den Fokus hat, sonst nichts. `FocusWorkspacePrevious` und die Aufnahmen
//!   von Bildschirm und Auswahl wirken naturgemäß auf den fokussierten Monitor.
//! - **Nur argv, nie Shell:** jedes Programm über [`Programs`] (ein Suchpfad, Tests legen ihn auf einen Ordner mit
//!   Fake-Programmen) zu einem absoluten Pfad aufgelöst ([`Exe`]); ohne Treffer kein Start („Program not found“), weil
//!   niri einen Startfehler nicht meldet (Spike E10). Langlebige Programme gehen als [`Launch`] an niri `Spawn`
//!   (eigene Scopes, überleben einen Neustart von `dictate-ui`), Kurzläufer mit Rückmeldung (`pactl`, `playerctl`,
//!   `systemctl`, `loginctl`, `noctalia msg`) laufen als Kind mit festem Programmnamen ([`Host::run`]). Ein Arbeitsordner geht nur
//!   über `/usr/bin/env -C <dir> -- argv…` (der einzige feste absolute Pfad), auch für ein Terminal. Das Terminal ist
//!   `palette.terminal` oder automatisch das erste aus [`TERMINALS`] (`PLAN-lizenz.md` O3, E4; vorher fest alacritty,
//!   E11), der Befehl darin immer ein aufgelöster absoluter Pfad als eigenes argv-Element; „Projekt öffnen“ startet
//!   `palette.project_command` oder die Shell des Users ohne Befehlstext (E5). Modelltext erreicht argv nur als geprüfte Adresse bzw.
//!   prozentkodierter Suchbegriff ([`super::pal_url`], Doppelprüfung zum Daemon).
//! - **Sperre und Anzeigen** (`PLAN-lizenz.md` O6): `lock` über logind (`loginctl lock-session`; ohne Argument die
//!   Sitzung `auto`, für einen Dienst die Anzeige-Sitzung des Users; jedes Sperrprogramm, das logind folgt, sperrt,
//!   Noctalia eingeschlossen). Zwischenablage und Emoji starten `palette.clipboard_command` bzw. `palette.emoji_command`
//!   über niri `Spawn`, leer die automatische Wahl (`noctalia msg panel-toggle clipboard` als Kurzläufer, `rofi -modi
//!   emoji -show emoji`). Ob eine Anzeige überhaupt angeboten wird, entscheidet der Daemon (`palette::widgets`).
//! - **Bestätigung:** Absichten der Klasse `confirm` (und jede, die laut Katalog eine braucht, auch wenn der Kandidat
//!   etwas anderes sagt) laufen nur mit [`ExecCtx::confirmed`].
//!
//! Dieses Modul loggt nichts; der Aufrufer loggt nur Absicht und [`Outcome::kind`] (Sicherheitsregel 4). Fehlertexte
//! sind kurz, englisch und für das Overlay; sie können Programmnamen und Ordner nennen, nie Eingabe oder Kontext.

use serde::Deserialize;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::pal_url;

/// Höchstens so viele argv-Einträge aus einem Desktop-Eintrag (wie `palette_exec.lua`)
pub const MAX_ARGV: usize = 128;
/// Der einzige fest verdrahtete absolute Pfad: Start in einem Arbeitsordner (niri `Spawn` kennt keinen)
pub const ENV: &str = "/usr/bin/env";
/// Wie ein Terminal der automatischen Wahl seinen Arbeitsordner bekommt (zusätzlich zu `env -C`, das jedes Terminal
/// erbt, das einen neuen Prozess startet: Terminals mit einem Server oder einer laufenden Instanz nehmen sonst deren
/// Ordner)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirArg {
    /// Nur geerbt (`xdg-terminal-exec` startet das Terminal selbst, `xterm` hat keinen Schalter)
    Inherit,
    /// Schalter und Ordner als zwei Einträge (`--working-directory /p`)
    Pair(&'static str),
    /// Schalter und Ordner in einem Eintrag (`--working-directory=/p`)
    Joined(&'static str),
}

/// Ein Terminal der automatischen Wahl: argv = Programm, `pre`, Ordner nach [`DirArg`], `exec`, dann der Befehl
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Term {
    pub name: &'static str,
    pub pre: &'static [&'static str],
    pub dir: DirArg,
    pub exec: &'static [&'static str],
}

/// Terminals für `palette.terminal` leer (automatisch, `PLAN-lizenz.md` O3, E4): das erste installierte. Schalter nach
/// den Handbüchern der Terminals; `xdg-terminal-exec` (freedesktop-Entwurf) zuerst, es folgt der Wahl des Users.
pub const TERMINALS: [Term; 9] = [
    Term { name: "xdg-terminal-exec", pre: &[], dir: DirArg::Inherit, exec: &[] },
    Term { name: "alacritty", pre: &[], dir: DirArg::Pair("--working-directory"), exec: &["-e"] },
    Term { name: "kitty", pre: &[], dir: DirArg::Pair("--directory"), exec: &[] },
    Term { name: "foot", pre: &[], dir: DirArg::Joined("--working-directory="), exec: &[] },
    Term { name: "ghostty", pre: &[], dir: DirArg::Joined("--working-directory="), exec: &["-e"] },
    Term { name: "wezterm", pre: &["start"], dir: DirArg::Pair("--cwd"), exec: &["--"] },
    Term { name: "konsole", pre: &[], dir: DirArg::Pair("--workdir"), exec: &["-e"] },
    Term { name: "gnome-terminal", pre: &[], dir: DirArg::Joined("--working-directory="), exec: &["--"] },
    Term { name: "xterm", pre: &[], dir: DirArg::Inherit, exec: &["-e"] },
];

impl Term {
    /// Argumente zwischen Programm und Befehl
    fn args(&self, dir: Option<&Path>) -> Vec<String> {
        let mut out: Vec<String> = self.pre.iter().map(|a| a.to_string()).collect();
        match (dir, self.dir) {
            (Some(d), DirArg::Pair(flag)) => out.extend([flag.to_string(), dir_arg(d)]),
            (Some(d), DirArg::Joined(flag)) => out.push(format!("{flag}{}", dir_arg(d))),
            _ => {}
        }
        out.extend(self.exec.iter().map(|a| a.to_string()));
        out
    }
}
/// Standardausgang für `pactl`
const SINK: &str = "@DEFAULT_SINK@";

// --- Drahtformat (Spiegel von `palette::resolve::Candidate`; Spiegeltest auf Seite der Bibliothek) ---------------

/// Was nach Return geschieht (`palette::catalog::Class`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Run,
    Confirm,
    Ai,
}

/// Ziel eines Kandidaten (`palette::resolve::Target`)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    None,
    Window {
        id: u64,
    },
    Workspace {
        id: u64,
    },
    MoveWindow {
        id: u64,
        workspace: u64,
    },
    App {
        id: String,
        argv: Vec<String>,
        #[serde(default)]
        working_dir: Option<PathBuf>,
        terminal: bool,
    },
    Project {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnDisplay {
    Normal,
    Tabbed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowToggle {
    Floating,
    Fullscreen,
    Maximized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Widget {
    ClipboardHistory,
    Emoji,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    Full,
    Window,
    Area,
}

/// Seiten von `dictate-settings --page` (`palette::intent::SettingsPage::NAMES`)
pub const SETTINGS_PAGES: [&str; 14] =
    ["general", "audio", "model", "recognition", "overlay", "voice", "vocabulary", "profiles", "ai", "actions", "palette", "diagnose", "history", "about"];
/// Profile der Diagnose (`palette::intent::DiagProfile::NAMES`)
pub const DIAG_PROFILES: [&str; 6] = ["last_crash", "now", "unit", "docker", "network", "heat"];

/// Absicht eines Kandidaten (`palette::intent::Intent`), nur mit den Feldern, die die Ausführung braucht. Suchbegriffe
/// (`query`, `workspace`) fehlen absichtlich: das Ziel steht in [`Target`] (L10). Unbekannte Felder überliest serde.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum Intent {
    ViewWorkspace {},
    FocusWindow {},
    MoveWindowToWorkspace {},
    ColumnDisplay {
        display: ColumnDisplay,
    },
    ToggleWindow {
        what: WindowToggle,
    },
    CloseWindow {},
    PreviousWorkspace {},
    LaunchApp {},
    OpenProject {},
    OpenFolder {},
    OpenUrl {
        url: String,
    },
    WebSearch {
        query: String,
    },
    Lock {},
    Volume {
        #[serde(default)]
        set: Option<u8>,
        #[serde(default)]
        step: Option<i8>,
        #[serde(default)]
        mute: Option<bool>,
    },
    Media {
        action: MediaKey,
    },
    ShowWidget {
        widget: Widget,
    },
    DictateSettings {
        #[serde(default)]
        page: Option<String>,
    },
    Screenshot {
        mode: ScreenshotMode,
    },
    Logout {},
    Reboot {},
    Poweroff {},
    Suspend {},
    Ask {
        question: String,
    },
    TextAction {
        action: String,
    },
    ExplainClipboard {},
    Diagnose {
        #[serde(default)]
        profile: Option<String>,
        #[serde(default)]
        unit: Option<String>,
        #[serde(default)]
        container: Option<String>,
    },
}

impl Intent {
    /// Name auf dem Draht (für `pal_chosen` und das Log)
    pub fn name(&self) -> &'static str {
        match self {
            Intent::ViewWorkspace {} => "view_workspace",
            Intent::FocusWindow {} => "focus_window",
            Intent::MoveWindowToWorkspace {} => "move_window_to_workspace",
            Intent::ColumnDisplay { .. } => "column_display",
            Intent::ToggleWindow { .. } => "toggle_window",
            Intent::CloseWindow {} => "close_window",
            Intent::PreviousWorkspace {} => "previous_workspace",
            Intent::LaunchApp {} => "launch_app",
            Intent::OpenProject {} => "open_project",
            Intent::OpenFolder {} => "open_folder",
            Intent::OpenUrl { .. } => "open_url",
            Intent::WebSearch { .. } => "web_search",
            Intent::Lock {} => "lock",
            Intent::Volume { .. } => "volume",
            Intent::Media { .. } => "media",
            Intent::ShowWidget { .. } => "show_widget",
            Intent::DictateSettings { .. } => "dictate_settings",
            Intent::Screenshot { .. } => "screenshot",
            Intent::Logout {} => "logout",
            Intent::Reboot {} => "reboot",
            Intent::Poweroff {} => "poweroff",
            Intent::Suspend {} => "suspend",
            Intent::Ask { .. } => "ask",
            Intent::TextAction { .. } => "text_action",
            Intent::ExplainClipboard {} => "explain_clipboard",
            Intent::Diagnose { .. } => "diagnose",
        }
    }

    /// Braucht laut Katalog eine zweite Bestätigung (`Class::Confirm` in `palette::catalog`; Spiegeltest)
    pub fn needs_confirm(&self) -> bool {
        matches!(self, Intent::CloseWindow {} | Intent::Logout {} | Intent::Reboot {} | Intent::Poweroff {} | Intent::Suspend {})
    }

    /// Das Panel wird erst unsichtbar, dann ausgeführt, dann geschlossen (Entscheidung 17: `screenshot` `full` und
    /// `area`, sonst nähme niri das Panel oder seine Schließ-Animation mit; `window` nimmt nur das Zielfenster)
    pub fn vanishes_first(&self) -> bool {
        matches!(self, Intent::Screenshot { mode: ScreenshotMode::Full | ScreenshotMode::Area })
    }
}

/// Ein Kandidat aus der Antwort auf `pal_resolve`/`pal_ask`
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Candidate {
    pub intent: Intent,
    pub target: Target,
    pub display: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub class: Class,
    pub target_id: String,
}

impl Candidate {
    /// Liest einen Kandidaten; `None` bei falscher Form (ohne die Meldung von serde, die zitierte Werte wie Titel)
    pub fn from_value(v: &serde_json::Value) -> Option<Candidate> {
        serde_json::from_value(v.clone()).ok()
    }
}

// --- Programme ----------------------------------------------------------------------------------------------------

/// Ein ausführbares Programm mit absolutem Pfad; entsteht nur in [`Programs`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exe(PathBuf);

impl Exe {
    pub fn path(&self) -> &Path {
        &self.0
    }

    fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

/// Die eine Naht für Programme: Suchpfad, über den der Ausführer jedes Programm zu einem absoluten Pfad auflöst.
/// Wird einmal oben erzeugt ([`Programs::session`] oder ein Testordner) und durchgereicht
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Programs {
    dirs: Vec<PathBuf>,
    /// Shell des Users (`SHELL`) für „Projekt öffnen“ ohne `palette.project_command`; `None` = `sh` im Suchpfad
    shell: Option<PathBuf>,
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

impl Programs {
    /// `PATH` und `SHELL` dieser Sitzung (nur absolute Ordner bzw. ein absoluter Pfad; systemd setzt `SHELL` auch für
    /// Dienste des Users). In den Unit-Tests leer: dort findet nichts ein echtes Programm
    pub fn session() -> Programs {
        #[cfg(not(test))]
        let (path, shell) = (std::env::var_os("PATH").unwrap_or_default(), std::env::var_os("SHELL").map(PathBuf::from));
        #[cfg(test)]
        let (path, shell) = (std::ffi::OsString::new(), None);
        Programs { shell, ..Programs::with_dirs(std::env::split_paths(&path)) }
    }

    /// Eigene Shell des Users (Tests): ein absoluter Pfad oder ein Name im Suchpfad; nicht ausführbar = `sh`
    pub fn with_shell(self, shell: impl Into<PathBuf>) -> Programs {
        Programs { shell: Some(shell.into()), ..self }
    }

    /// Eigener Suchpfad (Tests, Ordner mit Fake-Programmen); relative Ordner zählen nicht, ebenso Ordner, deren Pfad
    /// kein UTF-8 ist (argv an niri ist Text: ein verlustbehaftet umgewandelter Pfad träfe kein Programm, und niri
    /// meldete trotzdem `Handled`)
    pub fn with_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Programs {
        let mut out: Vec<PathBuf> = Vec::new();
        for d in dirs {
            if d.is_absolute() && d.to_str().is_some() && !out.contains(&d) {
                out.push(d);
            }
        }
        Programs { dirs: out, shell: None }
    }

    /// Absoluter Pfad eines ausführbaren Programms: ein Name im Suchpfad oder ein absoluter Pfad; ein relativer Pfad
    /// mit `/` gilt nie
    pub fn find(&self, name: &str) -> Option<Exe> {
        if name.is_empty() || name.contains('\0') {
            return None;
        }
        if name.contains('/') {
            let p = Path::new(name);
            return (p.is_absolute() && is_executable(p)).then(|| Exe(p.to_path_buf()));
        }
        self.dirs.iter().map(|d| d.join(name)).find(|p| is_executable(p)).map(Exe)
    }

    /// Die Shell des Users für ein Terminal ohne eigenen Befehl: `SHELL`, falls absolut und ausführbar, sonst `sh` im
    /// Suchpfad. Sie bekommt nie einen Befehlstext (`-c`), sie ist nur das Programm im Terminal
    pub fn shell(&self) -> Option<Exe> {
        match &self.shell {
            Some(s) if s.to_str().is_some() => self.find(&s.to_string_lossy()).or_else(|| self.find("sh")),
            _ => self.find("sh"),
        }
    }

    /// `/usr/bin/env`, falls ausführbar
    pub fn env(&self) -> Option<Exe> {
        let p = Path::new(ENV);
        is_executable(p).then(|| Exe(p.to_path_buf()))
    }
}

/// argv aus einem Desktop-Eintrag: 1 bis [`MAX_ARGV`] Einträge, kein NUL, `argv[0]` nicht leer
pub fn valid_argv(argv: &[String]) -> bool {
    !argv.is_empty() && argv.len() <= MAX_ARGV && argv.iter().all(|a| !a.contains('\0')) && !argv[0].is_empty()
}

/// Absoluter, lesbarer Ordner
fn is_abs_dir(p: &Path) -> bool {
    p.is_absolute() && p.to_str().is_some() && !p.as_os_str().as_bytes().contains(&0) && std::fs::read_dir(p).is_ok()
}

fn dir_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Ein Start über niri `Spawn`: `argv[0]` ist immer ein absoluter Pfad aus [`Programs`]. Entsteht nur hier
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch(Vec<String>);

impl Launch {
    fn new(exe: &Exe, args: impl IntoIterator<Item = String>) -> Launch {
        Launch(std::iter::once(exe.arg()).chain(args).collect())
    }

    pub fn argv(&self) -> &[String] {
        &self.0
    }

    pub fn into_argv(self) -> Vec<String> {
        self.0
    }
}

// --- Wirt -----------------------------------------------------------------------------------------------------------

/// niri-Aktionen des Ausführers (geschlossene Liste; `niri.rs` bildet sie auf `niri_ipc::Action` ab, jede mit fester
/// ID, wo niri eine kennt)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NiriOp {
    FocusWindow(u64),
    CloseWindow(u64),
    FocusWorkspace(u64),
    FocusWorkspacePrevious,
    MoveWindowToWorkspace {
        window: u64,
        workspace: u64,
    },
    ToggleFloating(u64),
    Fullscreen(u64),
    Maximize(u64),
    /// Wirkt auf die fokussierte Spalte: nur nach geprüftem Fokus (Modulkopf)
    SetColumnDisplay(ColumnDisplay),
    /// niris Auswahl (wie Print)
    ScreenshotArea,
    /// fokussierter Bildschirm (wie Ctrl+Print)
    ScreenshotScreen,
    /// ein Fenster (wie Alt+Print)
    ScreenshotWindow(u64),
    Quit,
}

/// Ergebnis eines Kurzläufers
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// Code 0, Standardausgabe (begrenzt)
    Ok(String),
    /// Kein Start, Frist überschritten oder Code ≠ 0: erste Zeile der Fehlerausgabe bzw. kurzer Grund
    Failed(String),
}

/// Was der Ausführer von außen braucht (in `dictate-ui` niri-IPC und Kindprozesse, in Tests ein Rekorder)
pub trait Host {
    /// IDs aller Fenster ohne das eigene Panel
    fn windows(&mut self) -> Result<Vec<u64>, String>;
    /// IDs aller Arbeitsflächen
    fn workspaces(&mut self) -> Result<Vec<u64>, String>;
    /// Das fokussierte Fenster
    fn focused_window(&mut self) -> Result<Option<u64>, String>;
    fn niri(&mut self, op: NiriOp) -> Result<(), String>;
    /// Übergabe an niri `Spawn`
    fn spawn(&mut self, launch: Launch) -> Result<(), String>;
    /// Kurzläufer als Kind, mit Frist; wartet auf das Ende
    fn run(&mut self, exe: &Exe, args: &[&str]) -> RunResult;
}

// --- Ausführung ---------------------------------------------------------------------------------------------------

/// Was der Ausführer über die Sitzung des Panels weiß
#[derive(Debug, Clone, Default)]
pub struct ExecCtx {
    /// Das Fenster, das vor dem Öffnen fokussiert war (nie das Panel)
    pub prior_focus: Option<u64>,
    /// Was der Daemon aus `[palette]` mitschickt (Suchadresse, Terminal, Befehl im Projekt)
    pub prefs: ExecPrefs,
    /// Der User hat eine Bestätigung gegeben (zweites Return)
    pub confirmed: bool,
}

/// Spiegel von `palette::service::ExecPrefs`: steht in jeder Antwort auf `pal_resolve` und `pal_ask` neben den
/// Kandidaten (Schlüssel `web_search_url`, `terminal`, `project_command`, `clipboard_command`, `emoji_command`)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecPrefs {
    /// Vorlage der Suchmaschine (`palette.web_search_url`)
    pub web_search_url: String,
    /// `palette.terminal`: argv bis vor den Befehl; leer = automatisch ([`TERMINALS`])
    pub terminal: Vec<String>,
    /// `palette.project_command`: argv im Projektordner; leer = die Shell des Users
    pub project_command: Vec<String>,
    /// `palette.clipboard_command`: argv des Verlaufs der Zwischenablage; leer = Noctalia
    pub clipboard_command: Vec<String>,
    /// `palette.emoji_command`: argv des Emoji-Wählers; leer = rofi mit rofi-emoji
    pub emoji_command: Vec<String>,
}

impl ExecPrefs {
    /// Übernimmt die Felder einer Antwort, die da und gültig geformt sind (Text bzw. Liste aus Texten); andere bleiben
    pub fn update(&mut self, reply: &serde_json::Value) {
        if let Some(u) = reply["web_search_url"].as_str() {
            self.web_search_url = u.to_string();
        }
        let list = |v: &serde_json::Value| v.as_array().and_then(|a| a.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<Vec<_>>>());
        if let Some(t) = list(&reply["terminal"]) {
            self.terminal = t;
        }
        if let Some(c) = list(&reply["project_command"]) {
            self.project_command = c;
        }
        if let Some(c) = list(&reply["clipboard_command"]) {
            self.clipboard_command = c;
        }
        if let Some(c) = list(&reply["emoji_command"]) {
            self.emoji_command = c;
        }
    }
}

/// Ein anderes Panel übernimmt (Block D); `chosen` erst, wenn es offen ist
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    Ask { question: String },
    TextAction { action: &'static str },
    ExplainClipboard,
    Diagnose { profile: Option<&'static str>, unit: Option<String>, container: Option<String> },
}

/// Ergebnis einer Ausführung
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Gelungen, optional mit Hinweis fürs Overlay
    Done(Option<&'static str>),
    /// Gescheitert: Meldung fürs Overlay
    Failed(String),
    Handoff(Handoff),
}

impl Outcome {
    /// Für das Log (ohne Text)
    pub fn kind(&self) -> &'static str {
        match self {
            Outcome::Done(_) => "done",
            Outcome::Failed(_) => "failed",
            Outcome::Handoff(_) => "handoff",
        }
    }
}

/// Zwischenergebnis eines Zweigs; `Quit` schickt `chosen` vor der Aktion
enum Step {
    Done(Option<&'static str>),
    Failed(String),
    Handoff(Handoff),
    Logout,
}

fn fail(msg: impl Into<String>) -> Step {
    Step::Failed(msg.into())
}

/// Führt `c` aus (Modulkopf). `chosen` läuft höchstens einmal und nur bei Erfolg, bei `logout` vor `Quit`
pub fn execute(c: &Candidate, ctx: &ExecCtx, programs: &Programs, host: &mut dyn Host, chosen: &mut dyn FnMut()) -> Outcome {
    if (c.class == Class::Confirm || c.intent.needs_confirm()) && !ctx.confirmed {
        return Outcome::Failed("This command needs a confirmation".into());
    }
    match step(c, ctx, programs, host) {
        Step::Done(note) => {
            chosen();
            Outcome::Done(note)
        }
        Step::Failed(msg) => Outcome::Failed(msg),
        Step::Handoff(h) => Outcome::Handoff(h),
        Step::Logout => {
            chosen();
            match host.niri(NiriOp::Quit) {
                Ok(()) => Outcome::Done(None),
                Err(_) => Outcome::Failed("Logging out failed".into()),
            }
        }
    }
}

/// Fenster `id`, falls es noch da ist
fn window(host: &mut dyn Host, id: u64) -> Result<u64, Step> {
    match host.windows() {
        Ok(ids) if ids.contains(&id) => Ok(id),
        Ok(_) => Err(fail("That window is gone")),
        Err(_) => Err(fail("niri did not answer")),
    }
}

fn workspace(host: &mut dyn Host, id: u64) -> Result<u64, Step> {
    match host.workspaces() {
        Ok(ids) if ids.contains(&id) => Ok(id),
        Ok(_) => Err(fail("That workspace is gone")),
        Err(_) => Err(fail("niri did not answer")),
    }
}

/// Das Zielfenster einer Fensterabsicht: nur aus dem Kandidaten
fn target_window(c: &Candidate) -> Result<u64, Step> {
    match c.target {
        Target::Window { id } => Ok(id),
        _ => Err(fail("There is no window to act on")),
    }
}

fn niri(host: &mut dyn Host, op: NiriOp) -> Step {
    match host.niri(op) {
        Ok(()) => Step::Done(None),
        Err(_) => fail("niri refused the command"),
    }
}

fn spawn(host: &mut dyn Host, launch: Launch) -> Step {
    // über `env -C <dir> -- prog …` gestartet: die Meldung nennt das Programm, nicht env
    let argv = launch.argv();
    let prog = match argv.iter().position(|a| a == "--") {
        Some(i) if argv[0] == ENV && i + 1 < argv.len() => &argv[i + 1],
        _ => &argv[0],
    };
    let name = Path::new(prog).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match host.spawn(launch) {
        Ok(()) => Step::Done(None),
        Err(_) => fail(format!("Could not start {name}")),
    }
}

fn find(programs: &Programs, name: &str) -> Result<Exe, Step> {
    programs.find(name).ok_or_else(|| fail(format!("Program not found: {name}")))
}

/// Kurzläufer mit festem Programmnamen; Fehler als „<label> failed: <Grund>“
fn run(programs: &Programs, host: &mut dyn Host, prog: &str, args: &[&str], label: &str) -> Result<String, Step> {
    let exe = find(programs, prog)?;
    match host.run(&exe, args) {
        RunResult::Ok(out) => Ok(out),
        RunResult::Failed(why) => Err(fail(format!("{label} failed: {why}"))),
    }
}

fn done_or(r: Result<String, Step>) -> Step {
    match r {
        Ok(_) => Step::Done(None),
        Err(s) => s,
    }
}

fn step(c: &Candidate, ctx: &ExecCtx, programs: &Programs, host: &mut dyn Host) -> Step {
    let r = (|| -> Result<Step, Step> {
        Ok(match &c.intent {
            // Fenster und Arbeitsflächen
            Intent::ViewWorkspace {} => {
                let Target::Workspace { id } = c.target else { return Err(fail("There is no workspace to show")) };
                {
                    let id = workspace(host, id)?;
                    niri(host, NiriOp::FocusWorkspace(id))
                }
            }
            Intent::FocusWindow {} => {
                let id = window(host, target_window(c)?)?;
                niri(host, NiriOp::FocusWindow(id))
            }
            Intent::MoveWindowToWorkspace {} => {
                let Target::MoveWindow { id, workspace: ws } = c.target else { return Err(fail("There is no window to act on")) };
                let window = window(host, id)?;
                let workspace = workspace(host, ws)?;
                niri(host, NiriOp::MoveWindowToWorkspace { window, workspace })
            }
            Intent::ColumnDisplay { display } => {
                let id = window(host, target_window(c)?)?;
                if host.niri(NiriOp::FocusWindow(id)).is_err() {
                    return Err(fail("niri refused the command"));
                }
                // SetColumnDisplay kennt keine ID: nur, wenn genau dieses Fenster jetzt fokussiert ist
                match host.focused_window() {
                    Ok(Some(f)) if f == id => niri(host, NiriOp::SetColumnDisplay(*display)),
                    _ => fail("The window did not take the focus"),
                }
            }
            Intent::ToggleWindow { what } => {
                let id = window(host, target_window(c)?)?;
                niri(
                    host,
                    match what {
                        WindowToggle::Floating => NiriOp::ToggleFloating(id),
                        WindowToggle::Fullscreen => NiriOp::Fullscreen(id),
                        WindowToggle::Maximized => NiriOp::Maximize(id),
                    },
                )
            }
            Intent::CloseWindow {} => {
                let id = window(host, target_window(c)?)?;
                niri(host, NiriOp::CloseWindow(id))
            }
            Intent::PreviousWorkspace {} => niri(host, NiriOp::FocusWorkspacePrevious),

            // Programme und Projekte
            Intent::LaunchApp {} => {
                let Target::App { argv, working_dir, terminal, .. } = &c.target else { return Err(fail("This program has no launch command")) };
                spawn(host, launch_app(programs, &ctx.prefs.terminal, argv, working_dir.as_deref(), *terminal)?)
            }
            Intent::OpenProject {} => {
                let path = project(c)?;
                let (cmd, args) = match ctx.prefs.project_command.split_first() {
                    Some((prog, args)) => (find(programs, prog)?, args.to_vec()),
                    None => (programs.shell().ok_or_else(|| fail("No shell found: set a command for projects"))?, Vec::new()),
                };
                spawn(host, in_terminal(programs, &ctx.prefs.terminal, Some(path), &cmd, args)?)
            }
            Intent::OpenFolder {} => {
                let path = project(c)?;
                spawn(host, Launch::new(&find(programs, "xdg-open")?, [dir_arg(path)]))
            }
            Intent::OpenUrl { url } => {
                pal_url::valid_web_url(url).map_err(fail)?;
                spawn(host, Launch::new(&find(programs, "xdg-open")?, [url.clone()]))
            }
            Intent::WebSearch { query } => {
                let url = pal_url::search_url(&ctx.prefs.web_search_url, query).map_err(fail)?;
                spawn(host, Launch::new(&find(programs, "xdg-open")?, [url]))
            }

            // System
            // logind: ohne Argument die Sitzung `auto` (für diesen Dienst die Anzeige-Sitzung des Users)
            Intent::Lock {} => done_or(run(programs, host, "loginctl", &["lock-session"], "Locking the screen")),
            Intent::Volume { set, step, mute } => volume(programs, host, *set, *step, *mute)?,
            Intent::Media { action } => {
                let cmd = match action {
                    MediaKey::PlayPause => "play-pause",
                    MediaKey::Next => "next",
                    MediaKey::Previous => "previous",
                };
                done_or(run(programs, host, "playerctl", &[cmd], "playerctl"))
            }
            Intent::ShowWidget { widget } => {
                let set = match widget {
                    Widget::ClipboardHistory => &ctx.prefs.clipboard_command,
                    Widget::Emoji => &ctx.prefs.emoji_command,
                };
                match (set.split_first(), widget) {
                    // eingestellt: wie ein Programm über niri (ein Wähler bleibt offen, bis der User wählt)
                    (Some((prog, args)), _) => spawn(host, Launch::new(&find(programs, prog)?, args.to_vec())),
                    (None, Widget::ClipboardHistory) => {
                        done_or(run(programs, host, "noctalia", &["msg", "panel-toggle", "clipboard"], "The clipboard history"))
                    }
                    (None, Widget::Emoji) => spawn(host, Launch::new(&find(programs, "rofi")?, ["-modi", "emoji", "-show", "emoji"].map(String::from))),
                }
            }
            Intent::DictateSettings { page } => {
                let page = match page {
                    None => None,
                    Some(p) => Some(*SETTINGS_PAGES.iter().find(|x| **x == p.as_str()).ok_or_else(|| fail("Unknown settings page"))?),
                };
                let exe = programs.find("dictate-settings").ok_or_else(|| fail("dictate-settings is not installed"))?;
                let args: Vec<String> = page.map(|p| vec!["--page".to_string(), p.to_string()]).unwrap_or_default();
                spawn(host, Launch::new(&exe, args))
            }
            Intent::Screenshot { mode } => match mode {
                ScreenshotMode::Area => niri(host, NiriOp::ScreenshotArea),
                ScreenshotMode::Full => niri(host, NiriOp::ScreenshotScreen),
                ScreenshotMode::Window => {
                    // vom Daemon ohne Ziel: das Fenster vor dem Öffnen
                    let id = match c.target {
                        Target::Window { id } => id,
                        Target::None => ctx.prior_focus.ok_or_else(|| fail("There is no window to act on"))?,
                        _ => return Err(fail("There is no window to act on")),
                    };
                    let id = window(host, id)?;
                    niri(host, NiriOp::ScreenshotWindow(id))
                }
            },
            Intent::Logout {} => Step::Logout,
            Intent::Reboot {} => done_or(run(programs, host, "systemctl", &["reboot"], "Reboot")),
            Intent::Poweroff {} => done_or(run(programs, host, "systemctl", &["poweroff"], "Power off")),
            // auf blub per sleep.conf gesperrt: systemd meldet die Ablehnung hier
            Intent::Suspend {} => done_or(run(programs, host, "systemctl", &["suspend"], "Suspend")),

            // Andere Panels
            Intent::Ask { question } => {
                if question.trim().is_empty() {
                    return Err(fail("Nothing to ask"));
                }
                Step::Handoff(Handoff::Ask { question: question.clone() })
            }
            Intent::TextAction { action } => {
                let b = crate::actions::BUILTIN.iter().find(|b| b.name == action).ok_or_else(|| fail("Unknown text action"))?;
                Step::Handoff(Handoff::TextAction { action: b.name })
            }
            Intent::ExplainClipboard {} => Step::Handoff(Handoff::ExplainClipboard),
            Intent::Diagnose { profile, unit, container } => {
                let profile = match profile {
                    None => None,
                    Some(p) => Some(*DIAG_PROFILES.iter().find(|x| **x == p.as_str()).ok_or_else(|| fail("Unknown diagnosis profile"))?),
                };
                Step::Handoff(Handoff::Diagnose { profile, unit: unit.clone(), container: container.clone() })
            }
        })
    })();
    r.unwrap_or_else(|s| s)
}

fn project(c: &Candidate) -> Result<&Path, Step> {
    let Target::Project { path } = &c.target else { return Err(fail("This project has no folder")) };
    if !is_abs_dir(path) {
        return Err(fail(format!("Project folder not found: {}", path.display())));
    }
    Ok(path)
}

/// Start eines Desktop-Eintrags: Terminal davor (`Terminal=true`, E11) bzw. Arbeitsordner über `env -C` (E10)
fn launch_app(programs: &Programs, terminal: &[String], argv: &[String], working_dir: Option<&Path>, in_term: bool) -> Result<Launch, Step> {
    if !valid_argv(argv) {
        return Err(fail("This program has no launch command"));
    }
    let prog = find(programs, &argv[0])?;
    if let Some(dir) = working_dir {
        if !is_abs_dir(dir) {
            return Err(fail(format!("Folder not found: {}", dir.display())));
        }
    }
    let rest = argv[1..].to_vec();
    if in_term {
        return in_terminal(programs, terminal, working_dir, &prog, rest);
    }
    in_dir(programs, working_dir, &prog, rest)
}

/// Das Terminal: `palette.terminal` (argv bis vor den Befehl; der Ordner kommt nur über `env -C`) oder, wenn leer,
/// das erste installierte aus [`TERMINALS`] mit seinem Ordnerschalter. `argv[0]` wird wie jedes Programm über
/// [`Programs`] zu einem absoluten Pfad
fn terminal_of(programs: &Programs, terminal: &[String], dir: Option<&Path>) -> Result<(Exe, Vec<String>), Step> {
    match terminal.split_first() {
        Some((prog, args)) => Ok((find(programs, prog)?, args.to_vec())),
        None => TERMINALS
            .iter()
            .find_map(|t| programs.find(t.name).map(|exe| (exe, t.args(dir))))
            .ok_or_else(|| fail("No terminal found: set one on the Palette page")),
    }
}

/// `cmd args` in einem neuen Terminal, im Ordner `dir`. Der Befehl ist immer ein absoluter Pfad (nie als Schalter des
/// Terminals lesbar) und steht als eigenes argv-Element, nie als Text für eine Shell
fn in_terminal(programs: &Programs, terminal: &[String], dir: Option<&Path>, cmd: &Exe, args: Vec<String>) -> Result<Launch, Step> {
    let (term, mut term_args) = terminal_of(programs, terminal, dir)?;
    term_args.push(cmd.arg());
    term_args.extend(args);
    in_dir(programs, dir, &term, term_args)
}

/// Startet `prog args`, mit Arbeitsordner über `env -C` (niri `Spawn` kennt keinen)
fn in_dir(programs: &Programs, dir: Option<&Path>, prog: &Exe, args: Vec<String>) -> Result<Launch, Step> {
    let Some(dir) = dir else { return Ok(Launch::new(prog, args)) };
    let env = programs.env().ok_or_else(|| fail("Program not found: env"))?;
    // nach `--` liest env `NAME=VALUE` noch als Zuweisung: ein Programmpfad mit `=` liefe nicht
    if prog.arg().contains('=') {
        return Err(fail("This program cannot start in its folder"));
    }
    let mut env_args = vec!["-C".to_string(), dir_arg(dir), "--".into(), prog.arg()];
    env_args.extend(args);
    Ok(Launch::new(&env, env_args))
}

/// Lautstärke des Standardausgangs in Prozent (der größte Kanal) aus `pactl get-sink-volume`
pub fn sink_percent(out: &str) -> Option<u32> {
    let b = out.as_bytes();
    let mut best: Option<u32> = None;
    for (i, _) in out.match_indices('%') {
        let start = b[..i].iter().rposition(|c| !c.is_ascii_digit()).map_or(0, |p| p + 1);
        if start < i {
            if let Ok(p) = out[start..i].parse::<u32>() {
                best = Some(best.map_or(p, |b| b.max(p)));
            }
        }
    }
    best
}

fn volume(programs: &Programs, host: &mut dyn Host, set: Option<u8>, step: Option<i8>, mute: Option<bool>) -> Result<Step, Step> {
    if let Some(v) = set {
        let arg = format!("{}%", v.min(100));
        return Ok(done_or(run(programs, host, "pactl", &["set-sink-volume", SINK, &arg], "Setting the volume")));
    }
    if let Some(m) = mute {
        return Ok(done_or(run(programs, host, "pactl", &["set-sink-mute", SINK, if m { "1" } else { "0" }], "Muting")));
    }
    let Some(step) = step else { return Err(fail("The volume command is incomplete")) };
    if step == 0 {
        return Err(fail("The volume step is 0"));
    }
    let mut step = i32::from(step);
    // nie über 100 %: erst lesen, dann höchstens bis 100 anheben (relativ, damit die Balance bleibt)
    if step > 0 {
        let out = run(programs, host, "pactl", &["get-sink-volume", SINK], "Reading the volume")?;
        let cur = sink_percent(&out).ok_or_else(|| fail("Reading the volume failed: unexpected pactl output"))?;
        step = step.min(100 - i32::try_from(cur.min(100)).unwrap_or(100));
        if step <= 0 {
            return Ok(Step::Done(Some("The volume is already at 100%")));
        }
    }
    let arg = format!("{}{}%", if step > 0 { "+" } else { "-" }, step.unsigned_abs());
    Ok(done_or(run(programs, host, "pactl", &["set-sink-volume", SINK, &arg], "Changing the volume")))
}

#[cfg(test)]
#[path = "pal_exec_tests.rs"]
mod tests;
