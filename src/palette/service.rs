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

//! Dienst der Befehlspalette im Daemon (Aufgabe 5 in `PLAN-palette.md`): hält Quellen und Häufigkeit und beantwortet
//! `pal_resolve`, `pal_chosen`, `pal_sources_refresh` und `pal_frecency_clear`; für `pal_ask` (Aufgabe 4) die Schritte
//! vor und nach dem Modell ([`Palette::prepare`], [`Palette::resolve_intent`]), den Aufruf selbst macht
//! [`super::ai`] auf dem Verbindungs-Thread.
//!
//! - **Lazy:** [`Sources`] und [`Frecency`] entstehen erst beim ersten Zugriff (der erste `pal_resolve` liest auch die
//!   Programme und Projekte von der Platte). Ist `[palette] enabled = false`, antwortet jede Anfrage mit
//!   [`PaletteError::Disabled`]; nichts wird gelesen oder gemerkt.
//! - **Sitzungsumgebung:** Der Dienst läuft unter systemd mit kürzerem `PATH` und ohne `XDG_CURRENT_DESKTOP` und
//!   `XDG_DATA_DIRS`; `TryExec`, `OnlyShowIn` und die Verzeichnisse mit den `.desktop`-Dateien (Flatpak, Nix, Snap)
//!   brauchen die Werte der Sitzung. Der Client (bis W7 das aus Awesome gestartete CLI, unter niri ab W5 das Panel)
//!   schickt sie bei jeder Anfrage mit ([`SessionEnv`]:
//!   `PATH`, `XDG_CURRENT_DESKTOP`, `XDG_DATA_DIRS`, `XDG_DATA_HOME`); ändern sie sich, liest der Dienst die Programme
//!   neu ([`Sources::reconfigure`]). Fehlt ein Wert, gilt der des Daemon-Prozesses (und dort die Vorgabe der Spec).
//! - **Konfigurationsänderungen** ([`Palette::configure`], vom Kern aufgerufen) setzen nur den Stand; angewendet wird
//!   beim nächsten Zugriff auf dem Verbindungs-Thread. Der Kern wartet so nie auf einen laufenden Durchlauf der Platte.
//! - **`pal_chosen` ist keine freie Eingabe:** Gemerkt wird nur ein Paar (Absicht, Ziel-ID), das der Dienst selbst in
//!   einer der letzten [`OFFERED_ROUNDS`] Antworten auf `pal_resolve` angeboten hat. Eingabetext kann so nie in die Datei
//!   der Häufigkeit gelangen, auch nicht über einen fehlerhaften oder feindlichen Client.
//! - **Nie loggen:** Eingabe und Kontext (Fenstertitel) kommen in keine Logzeile, auch nicht mit `DICTATE_DEBUG`; Fehler
//!   nennen nur Namen und Zahlen.

use super::catalog;
use super::intent::{Intent, Widget};
use super::resolve::{resolve_intent_with, resolve_with, Candidate, Context, ContextError, Features, NoTarget, Resolution};
use super::sources::{unix_now, DesktopEnv, Frecency, FrecencySettings, ProjectScanConfig, Sources, SourcesConfig, DEFAULT_MIN_CHECK_INTERVAL};
use super::widgets;
use crate::config::PaletteCfg;
use crate::protocol::SessionEnv;
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// So viele der letzten Antworten merkt sich der Dienst, um `pal_chosen` zu prüfen (jede Eingabe tippt ein Zeichen: die
/// angezeigte Liste stammt aus einer der letzten Antworten).
pub const OFFERED_ROUNDS: usize = 32;
/// Längster Wert der Sitzungsumgebung (`PATH`, `XDG_CURRENT_DESKTOP`, `XDG_DATA_DIRS`, `XDG_DATA_HOME`) in einer Anfrage
/// (Bytes).
pub const MAX_ENV_BYTES: usize = 16 * 1024;

/// Meldung bei `enabled = false`.
pub const DISABLED: &str = "The command palette is off (Settings › Palette)";

/// Prüft die Werte der Sitzungsumgebung: nicht länger als [`MAX_ENV_BYTES`], keine Steuerzeichen.
fn check_env(env: &SessionEnv) -> Result<(), PaletteError> {
    for (_, name, value) in env.fields() {
        if let Some(v) = value {
            if v.len() > MAX_ENV_BYTES || v.chars().any(char::is_control) {
                return Err(PaletteError::BadEnv(name));
            }
        }
    }
    Ok(())
}

/// Warum eine Anfrage der Palette scheitert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteError {
    /// `[palette] enabled = false`
    Disabled,
    /// Der Kontext ist ungültig (Größe, Felder, Grenzen).
    Context(ContextError),
    /// Ein Wert der Sitzungsumgebung (Name der Variable) ist zu lang oder hat Steuerzeichen.
    BadEnv(&'static str),
    /// Absicht nicht im Katalog (der Name ist gekürzt).
    UnknownIntent(String),
    /// Das Ziel wurde in keiner der letzten Antworten angeboten.
    UnknownTarget,
    /// Die Häufigkeit ließ sich nicht schreiben oder löschen.
    Storage(String),
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaletteError::Disabled => f.write_str(DISABLED),
            PaletteError::Context(e) => write!(f, "{e}"),
            PaletteError::BadEnv(name) => write!(f, "{name} of the session is not valid (too long or with control characters)"),
            PaletteError::UnknownIntent(name) => write!(f, "Unknown intent \"{name}\""),
            PaletteError::UnknownTarget => f.write_str("Unknown target: it was not offered by a recent pal_resolve"),
            PaletteError::Storage(e) => write!(f, "Cannot update the palette history: {e}"),
        }
    }
}

impl std::error::Error for PaletteError {}

/// Was `dictate-ui` zum Ausführen aus der Config braucht; steht in jeder Antwort auf `pal_resolve` und `pal_ask` neben
/// den Kandidaten (Schlüssel wie die Felder, Spiegel `ui::pal_exec::ExecPrefs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecPrefs {
    /// Suchadresse für `web_search` (mit `{query}`; der Aufrufer setzt den prozentkodierten Suchbegriff ein)
    pub web_search_url: String,
    /// `palette.terminal` (leer = automatisch)
    pub terminal: Vec<String>,
    /// `palette.project_command` (leer = Shell)
    pub project_command: Vec<String>,
    /// `palette.clipboard_command` (leer = Noctalia)
    pub clipboard_command: Vec<String>,
    /// `palette.emoji_command` (leer = rofi-emoji)
    pub emoji_command: Vec<String>,
}

impl ExecPrefs {
    pub fn of(cfg: &PaletteCfg) -> ExecPrefs {
        ExecPrefs {
            web_search_url: cfg.web_search_url.clone(),
            terminal: cfg.terminal.clone(),
            project_command: cfg.project_command.clone(),
            clipboard_command: cfg.clipboard_command.clone(),
            emoji_command: cfg.emoji_command.clone(),
        }
    }

    /// Trägt die Felder in eine Antwort ein.
    pub fn add_to(&self, reply: &mut serde_json::Value) {
        if let (Some(obj), Ok(serde_json::Value::Object(mine))) = (reply.as_object_mut(), serde_json::to_value(self)) {
            obj.extend(mine);
        }
    }
}

/// Antwort auf `pal_resolve`: die Kandidaten und was zum Ausführen gebraucht wird ([`ExecPrefs`]).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub resolution: Resolution,
    pub prefs: ExecPrefs,
}

/// Stand einer Anfrage nach [`Palette::prepare`]: Kontext, Quellen, Häufigkeit und Suchadresse.
pub struct Prepared {
    ctx: Context,
    sources: Arc<Sources>,
    frecency: Arc<Frecency>,
    /// Was zum Ausführen gebraucht wird (wie in [`Resolved`])
    pub prefs: ExecPrefs,
    /// Welche Anzeigen angeboten werden, festgestellt bei dieser Anfrage (`diagnose` gilt hier nicht)
    widgets: Features,
}

/// Was in den letzten Antworten angeboten wurde: Paare (Absicht, Ziel-ID).
#[derive(Default)]
struct Offered {
    rounds: VecDeque<Vec<(String, String)>>,
}

impl Offered {
    fn note(&mut self, candidates: &[Candidate]) {
        if candidates.is_empty() {
            return;
        }
        if self.rounds.len() == OFFERED_ROUNDS {
            self.rounds.pop_front();
        }
        self.rounds.push_back(candidates.iter().map(|c| (c.intent.name().to_string(), c.target_id.clone())).collect());
    }

    fn contains(&self, intent: &str, target_id: &str) -> bool {
        self.rounds.iter().any(|round| round.iter().any(|(i, t)| i == intent && t == target_id))
    }
}

/// Womit die [`Sources`] zuletzt eingestellt wurden.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourcesKey {
    projects: ProjectScanConfig,
    desktop: DesktopEnv,
}

struct State {
    cfg: PaletteCfg,
    /// `[diagnose] enabled`: aus = die Absicht `diagnose` wird nicht angeboten ([`Features::diagnose`])
    diagnose: bool,
    sources: Option<Arc<Sources>>,
    /// Stand der Einstellung von `sources`; weicht er vom gewünschten ab, wird beim nächsten Zugriff umgestellt
    applied: Option<SourcesKey>,
    frecency: Option<Arc<Frecency>>,
    offered: Offered,
    /// Zuletzt erkannte Anzeigen: jede Taste im Panel ist eine Anfrage, gesucht wird erst wieder nach `widgets_recheck`
    /// oder mit anderem Suchpfad bzw. anderen Befehlen
    widgets_seen: Option<WidgetsSeen>,
    /// Abstand der Suche ([`WIDGETS_RECHECK`]; Tests setzen null)
    widgets_recheck: Duration,
}

/// So lange gilt eine Erkennung der Anzeigen (wie der Prüfabstand der Quellen): wenige `stat` je Tastendruck wären
/// billig, ein hängender Ordner im Suchpfad (Netzlaufwerk) hielte aber sonst jede Taste auf
pub const WIDGETS_RECHECK: Duration = DEFAULT_MIN_CHECK_INTERVAL;

/// Ergebnis einer Erkennung mit dem, wovon es abhängt
struct WidgetsSeen {
    path: Vec<PathBuf>,
    clipboard_command: Vec<String>,
    emoji_command: Vec<String>,
    at: Instant,
    found: Features,
}

impl WidgetsSeen {
    fn still(&self, path: &[PathBuf], cfg: &PaletteCfg, recheck: Duration) -> Option<Features> {
        let same = self.path == path && self.clipboard_command == cfg.clipboard_command && self.emoji_command == cfg.emoji_command;
        (same && self.at.elapsed() < recheck).then_some(self.found)
    }
}

struct Shared {
    /// Programmumgebung des Daemon-Prozesses (Verzeichnisse, Locale, Vorgaben für `PATH` und Desktop)
    base: DesktopEnv,
    /// `ROFI_PLUGIN_PATH` des Daemon-Prozesses für die Suche nach rofi-emoji ([`super::widgets`]); nur, wenn `base` die
    /// Umgebung des Prozesses ist (Tests geben eine eigene und suchen nur unter dem Präfix von rofi)
    rofi_plugin_path: Option<OsString>,
    frecency_path: PathBuf,
    state: Mutex<State>,
}

/// Der Dienst; billig zu kopieren (geteilter Zustand), aus allen Verbindungs-Threads und vom Kern nutzbar.
#[derive(Clone)]
pub struct Palette {
    shared: Arc<Shared>,
}

/// Quellen-Einstellung aus Config, Programmumgebung und Sitzung.
fn sources_key(base: &DesktopEnv, cfg: &PaletteCfg, env: &SessionEnv) -> SourcesKey {
    let mut desktop = base.clone();
    // Dieselben Regeln wie für die Umgebung des Prozesses (nur absolute Einträge, Trennung mit `:`)
    let parsed = DesktopEnv::from_lookup(
        |k| match k {
            "PATH" => env.path.clone(),
            "XDG_CURRENT_DESKTOP" => env.desktop.clone(),
            "XDG_DATA_DIRS" => env.data_dirs.clone(),
            "XDG_DATA_HOME" => env.data_home.clone(),
            _ => None,
        },
        None,
    );
    if env.path.is_some() {
        desktop.path = parsed.path;
    }
    if env.desktop.is_some() {
        desktop.current_desktops = parsed.current_desktops;
    }
    // Ist `XDG_DATA_DIRS` gesetzt, aber leer oder ohne absoluten Eintrag, gilt die Vorgabe der Spec (wie im Prozess)
    if env.data_dirs.is_some() {
        desktop.data_dirs = parsed.data_dirs;
    }
    // `XDG_DATA_HOME` ersetzt nur ein absoluter Wert; sonst bleibt der des Daemons (Vorgabe `~/.local/share`)
    if parsed.data_home.is_some() {
        desktop.data_home = parsed.data_home;
    }
    SourcesKey { projects: ProjectScanConfig { roots: cfg.root_paths(), max_depth: cfg.project_depth, ..ProjectScanConfig::default() }, desktop }
}

impl Palette {
    /// `base`: Programmumgebung (`None` = die des Prozesses); `frecency_path`: Datei der Häufigkeit.
    pub fn new(cfg: PaletteCfg, frecency_path: PathBuf, base: Option<DesktopEnv>) -> Palette {
        let state = State {
            cfg,
            diagnose: false,
            sources: None,
            applied: None,
            frecency: None,
            offered: Offered::default(),
            widgets_seen: None,
            widgets_recheck: WIDGETS_RECHECK,
        };
        let rofi_plugin_path = if base.is_none() { std::env::var_os("ROFI_PLUGIN_PATH") } else { None };
        let base = base.unwrap_or_else(DesktopEnv::from_process);
        Palette { shared: Arc::new(Shared { base, rofi_plugin_path, frecency_path, state: Mutex::new(state) }) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Ein abgestürzter Thread lässt höchstens einen halb gefüllten Zwischenspeicher zurück
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn open_frecency(&self, cfg: &PaletteCfg) -> Arc<Frecency> {
        Arc::new(Frecency::open(self.shared.frecency_path.clone(), FrecencySettings { enabled: cfg.frecency, ..FrecencySettings::default() }))
    }

    /// Neue Einstellungen (vom Kern bei jeder Konfigurationsänderung, sofort, auch während einer Sitzung). Setzt nur den
    /// Stand; Quellen werden beim nächsten Zugriff umgestellt, die Häufigkeit neu geöffnet, wenn sich `frecency` ändert.
    pub fn configure(&self, cfg: &PaletteCfg) {
        let mut st = self.lock();
        if st.cfg == *cfg {
            return;
        }
        if st.cfg.frecency != cfg.frecency {
            st.frecency = None;
        }
        if !cfg.enabled {
            // nichts länger im Speicher halten als nötig; beim erneuten Einschalten entsteht alles neu
            st.sources = None;
            st.applied = None;
            st.frecency = None;
            st.offered = Offered::default();
        }
        st.cfg = cfg.clone();
    }

    /// Stellt ein, ob die Diagnose eingeschaltet ist (`[diagnose] enabled`, vom Kern bei jeder Konfigurationsänderung):
    /// aus = die Palette bietet die Absicht `diagnose` nicht an, und eine Absicht des Modells dazu ergibt die Meldung der
    /// Diagnose ([`NoTarget::DiagnosisOff`]). Vorgabe aus, bis der Kern es setzt.
    pub fn set_diagnose_enabled(&self, enabled: bool) {
        self.lock().diagnose = enabled;
    }

    /// Anzeigen nach den gefundenen Programmen im Suchpfad der Sitzung (die Diagnose setzt erst [`Palette::features`])
    fn widgets(&self, cfg: &PaletteCfg, path: &[PathBuf]) -> Features {
        let found = |w| widgets::available(w, cfg, path, self.shared.rofi_plugin_path.as_deref());
        Features { diagnose: false, clipboard_history: found(Widget::ClipboardHistory), emoji: found(Widget::Emoji) }
    }

    /// Was angeboten wird: die Diagnose nach dem Stand jetzt (ein Umschalten gilt auch während eines Modellaufrufs), die
    /// Anzeigen nach den Programmen bei [`Palette::prepare`]
    fn features(&self, p: &Prepared) -> Features {
        Features { diagnose: self.lock().diagnose, ..p.widgets }
    }

    /// Eingabe + Kontext (JSON-Text des Kontexts, `None` = leer) lokal auflösen.
    pub fn resolve(&self, input: &str, context: Option<&str>, env: &SessionEnv) -> Result<Resolved, PaletteError> {
        let p = self.prepare(context, env)?;
        let (apps, projects) = (p.sources.apps(), p.sources.projects());
        let resolution = resolve_with(self.features(&p), input, &p.ctx, &apps, &projects, p.frecency.enabled().then_some(&*p.frecency), unix_now());
        self.lock().offered.note(&resolution.candidates);
        Ok(Resolved { resolution, prefs: p.prefs })
    }

    /// Erster Schritt des KI-Pfads (`pal_ask`), vor dem Modellaufruf: eingeschaltet, Umgebung und Kontext gültig,
    /// Quellen bereit. So kostet ein ungültiger Kontext keinen Aufruf. Der Kontext bleibt im Ergebnis und geht nie an
    /// das Modell.
    pub fn prepare(&self, context: Option<&str>, env: &SessionEnv) -> Result<Prepared, PaletteError> {
        check_env(env)?;
        let (sources, frecency, reconfigure, prefs, (cfg, path), seen) = {
            let mut st = self.lock();
            if !st.cfg.enabled {
                return Err(PaletteError::Disabled);
            }
            let want = sources_key(&self.shared.base, &st.cfg, env);
            let make = |key: &SourcesKey| SourcesConfig {
                desktop: key.desktop.clone(),
                projects: key.projects.clone(),
                min_check_interval: DEFAULT_MIN_CHECK_INTERVAL,
            };
            let mut reconfigure = None;
            let sources = match (&st.sources, &st.applied) {
                (Some(s), Some(applied)) if *applied == want => Arc::clone(s),
                (Some(s), _) => {
                    reconfigure = Some(make(&want));
                    Arc::clone(s)
                }
                (None, _) => {
                    let s = Arc::new(Sources::new(make(&want)));
                    st.sources = Some(Arc::clone(&s));
                    s
                }
            };
            let path = want.desktop.path.clone();
            st.applied = Some(want);
            let cfg = st.cfg.clone();
            let frecency = Arc::clone(st.frecency.get_or_insert_with(|| self.open_frecency(&cfg)));
            let seen = st.widgets_seen.as_ref().and_then(|w| w.still(&path, &cfg, st.widgets_recheck));
            (sources, frecency, reconfigure, ExecPrefs::of(&cfg), (cfg, path), seen)
        };
        // Programme suchen (`stat`) außerhalb der Sperre, höchstens alle `WIDGETS_RECHECK`
        let widgets = match seen {
            Some(found) => found,
            None => {
                let found = self.widgets(&cfg, &path);
                let (clipboard_command, emoji_command) = (cfg.clipboard_command.clone(), cfg.emoji_command.clone());
                self.lock().widgets_seen = Some(WidgetsSeen { path, clipboard_command, emoji_command, at: Instant::now(), found });
                found
            }
        };
        // Außerhalb der Sperre: Umstellen und Lesen der Platte können dauern, der Kern darf nicht darauf warten
        if let Some(cfg) = reconfigure {
            sources.reconfigure(cfg);
        }
        let ctx = match context {
            Some(c) => Context::from_json(c).map_err(PaletteError::Context)?,
            None => Context::default(),
        };
        Ok(Prepared { ctx, sources, frecency, prefs, widgets })
    }

    /// Zweiter Schritt des KI-Pfads: die validierte Absicht des Modells lokal gegen Kontext und Quellen auflösen
    /// ([`resolve_intent`]; kein Treffer = [`NoTarget`], nie ein erfundenes Ziel, eine Adresse nur mit einem Host aus
    /// `input`, der Eingabe des Users). Die Kandidaten werden wie bei `pal_resolve` angemeldet, damit `pal_chosen` sie
    /// annimmt.
    pub fn resolve_intent(&self, p: &Prepared, intent: &Intent, input: &str) -> Result<Vec<Candidate>, NoTarget> {
        let (apps, projects) = (p.sources.apps(), p.sources.projects());
        let candidates =
            resolve_intent_with(self.features(p), intent, input, &p.ctx, &apps, &projects, p.frecency.enabled().then_some(&*p.frecency), unix_now())?;
        self.lock().offered.note(&candidates);
        Ok(candidates)
    }

    /// Der User hat das Ziel `target_id` der Absicht `intent` gewählt. `Ok(true)` = gemerkt, `Ok(false)` = die
    /// Häufigkeit ist aus. Geprüft wird vor dem Schreiben: Absicht im Katalog, Paar in den letzten Antworten angeboten.
    pub fn chosen(&self, intent: &str, target_id: &str) -> Result<bool, PaletteError> {
        let frecency = {
            let mut st = self.lock();
            if !st.cfg.enabled {
                return Err(PaletteError::Disabled);
            }
            if catalog::spec(intent).is_none() {
                return Err(PaletteError::UnknownIntent(intent.chars().filter(|c| !c.is_control()).take(40).collect()));
            }
            if !st.offered.contains(intent, target_id) {
                return Err(PaletteError::UnknownTarget);
            }
            if !st.cfg.frecency {
                return Ok(false);
            }
            let cfg = st.cfg.clone();
            Arc::clone(st.frecency.get_or_insert_with(|| self.open_frecency(&cfg)))
        };
        // Schreiben (fsync) ohne die Sperre
        frecency.record(intent, target_id, unix_now()).map_err(|e| PaletteError::Storage(format!("{e:#}")))?;
        Ok(true)
    }

    /// Programme und Projekte beim nächsten Zugriff neu lesen.
    pub fn refresh(&self) -> Result<(), PaletteError> {
        let sources = {
            let st = self.lock();
            if !st.cfg.enabled {
                return Err(PaletteError::Disabled);
            }
            st.sources.clone()
        };
        if let Some(s) = sources {
            s.refresh();
        }
        Ok(())
    }

    /// Die gemerkte Häufigkeit löschen (Speicher und Datei).
    pub fn clear_history(&self) -> Result<(), PaletteError> {
        let frecency = {
            let mut st = self.lock();
            if !st.cfg.enabled {
                return Err(PaletteError::Disabled);
            }
            let cfg = st.cfg.clone();
            Arc::clone(st.frecency.get_or_insert_with(|| self.open_frecency(&cfg)))
        };
        frecency.clear().map_err(|e| PaletteError::Storage(format!("{e:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::super::sources::testutil::TempDir;
    use super::*;

    fn entry(name: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={}\n{extra}", name.to_lowercase().replace(' ', "-"))
    }

    /// Dienst mit Fixture-Ordnern: Programme unter `share/applications`, Projekte unter `git`.
    fn palette(t: &TempDir, mutate: impl FnOnce(&mut PaletteCfg)) -> Palette {
        let mut cfg = PaletteCfg { project_roots: vec![t.join("git").display().to_string()], ..PaletteCfg::default() };
        mutate(&mut cfg);
        let base = DesktopEnv { data_home: None, data_dirs: vec![t.join("share")], ..DesktopEnv::default() };
        let p = Palette::new(cfg, t.join("state/frecency.json"), Some(base));
        // jede Anfrage sucht die Programme neu (der Zwischenspeicher hat einen eigenen Test)
        p.lock().widgets_recheck = Duration::ZERO;
        p
    }

    fn ids(r: &Resolved) -> Vec<String> {
        r.resolution.candidates.iter().map(|c| format!("{}:{}", c.intent.name(), c.target_id)).collect()
    }

    #[test]
    fn config_defaults_match_the_sources() {
        // `config.rs` kennt die Quellen nicht (Einstellungsfenster): zwei Stellen, ein Test
        let (cfg, scan) = (PaletteCfg::default(), ProjectScanConfig::default());
        assert_eq!((cfg.root_paths(), cfg.project_depth), (scan.roots, scan.max_depth));
    }

    #[test]
    fn resolves_apps_and_projects_and_returns_the_search_url() {
        let t = TempDir::new("svc-resolve");
        t.write("share/applications/firefox.desktop", &entry("Firefox", ""));
        t.mkdir("git/dataroom/.git");
        let p = palette(&t, |_| {});
        let r = p.resolve("fire", None, &SessionEnv::default()).unwrap();
        assert_eq!(ids(&r)[0], "launch_app:firefox.desktop");
        assert!(r.resolution.confident);
        let none = ExecPrefs {
            web_search_url: "https://duckduckgo.com/?q={query}".into(),
            terminal: vec![],
            project_command: vec![],
            clipboard_command: vec![],
            emoji_command: vec![],
        };
        assert_eq!(r.prefs, none);
        let r = p.resolve("dataroom", None, &SessionEnv::default()).unwrap();
        assert_eq!(ids(&r)[0], format!("open_project:{}", t.join("git/dataroom").display()));
    }

    #[test]
    fn a_context_is_validated_by_the_resolver() {
        let t = TempDir::new("svc-context");
        let p = palette(&t, |_| {});
        let ctx = r#"{"workspaces":[{"id":3,"idx":1,"name":"web"}],"windows":[{"id":5,"app_id":"Firefox","title":"Home","workspace_id":3,"focused":false}]}"#;
        let r = p.resolve("firefox", Some(ctx), &SessionEnv::default()).unwrap();
        assert_eq!(ids(&r)[0], "focus_window:firefox");
        for bad in ["[]", "{\"workspaces\":[{\"id\":1,\"idx\":0}]}", "{\"tags\":[]}", "{\"unknown\":1}", "nonsense"] {
            assert!(matches!(p.resolve("x", Some(bad), &SessionEnv::default()), Err(PaletteError::Context(_))), "{bad}");
        }
    }

    #[test]
    fn disabled_refuses_everything_and_frees_the_state() {
        let t = TempDir::new("svc-disabled");
        let p = palette(&t, |c| c.enabled = false);
        let env = SessionEnv::default();
        assert_eq!(p.resolve("x", None, &env).unwrap_err(), PaletteError::Disabled);
        assert_eq!(p.chosen("launch_app", "x.desktop").unwrap_err(), PaletteError::Disabled);
        assert_eq!(p.refresh().unwrap_err(), PaletteError::Disabled);
        assert_eq!(p.clear_history().unwrap_err(), PaletteError::Disabled);
        assert!(PaletteError::Disabled.to_string().contains("Settings › Palette"));
        // Einschalten per configure gilt sofort; Ausschalten wieder
        let mut on = PaletteCfg::default();
        on.project_roots.clear();
        p.configure(&on);
        assert!(p.resolve("x", None, &env).is_ok());
        p.configure(&PaletteCfg { enabled: false, ..on });
        assert_eq!(p.resolve("x", None, &env).unwrap_err(), PaletteError::Disabled);
        let st = p.lock();
        assert!(st.sources.is_none() && st.offered.rounds.is_empty());
    }

    #[test]
    fn widgets_follow_the_programs_in_the_session_path() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new("svc-widgets");
        let exe = |rel: &str| {
            let p = t.write(rel, "#!/bin/sh\n");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let p = palette(&t, |_| {});
        let env = SessionEnv { path: Some(t.join("usr/bin").display().to_string()), ..SessionEnv::default() };
        let offered = |p: &Palette, input: &str| ids(&p.resolve(input, None, &env).unwrap()).iter().any(|i| i.starts_with("show_widget:"));
        let from_model = |p: &Palette, widget: Widget| {
            let prepared = p.prepare(None, &env).unwrap();
            p.resolve_intent(&prepared, &Intent::ShowWidget { widget }, "zeig").map(|c| c.len())
        };
        // nichts im Suchpfad: weder angeboten noch vom Modell
        assert!(!offered(&p, "clipboard") && !offered(&p, "emoji"));
        assert_eq!(from_model(&p, Widget::ClipboardHistory), Err(NoTarget::WidgetMissing(Widget::ClipboardHistory)));
        assert_eq!(from_model(&p, Widget::Emoji), Err(NoTarget::WidgetMissing(Widget::Emoji)));
        // Noctalia da: Zwischenablage; rofi ohne Plugin: kein Emoji; mit Plugin unter dem Präfix: Emoji (je Anfrage neu)
        exe("usr/bin/noctalia");
        exe("usr/bin/rofi");
        assert!(offered(&p, "clipboard") && !offered(&p, "emoji"));
        assert_eq!(from_model(&p, Widget::ClipboardHistory), Ok(1));
        t.write("usr/lib/rofi/emoji.so", "");
        assert!(offered(&p, "emoji"));
        assert_eq!(from_model(&p, Widget::Emoji), Ok(1));
        // der Suchpfad der Sitzung zählt, nicht der des Daemons
        let other = SessionEnv { path: Some(t.join("elsewhere").display().to_string()), ..SessionEnv::default() };
        assert!(ids(&p.resolve("clipboard", None, &other).unwrap()).iter().all(|i| !i.starts_with("show_widget:")));
        // ein eigener Befehl ersetzt die automatische Wahl: fehlt er, wird nichts angeboten, auch wenn Noctalia da ist
        let mut cfg = PaletteCfg { project_roots: vec![], clipboard_command: vec!["copyq".into(), "toggle".into()], ..PaletteCfg::default() };
        cfg.emoji_command = vec!["bemoji".into()];
        p.configure(&cfg);
        assert!(!offered(&p, "clipboard") && !offered(&p, "emoji"));
        exe("usr/bin/copyq");
        exe("usr/bin/bemoji");
        assert!(offered(&p, "clipboard") && offered(&p, "emoji"));
        let r = p.resolve("clipboard", None, &env).unwrap();
        assert_eq!((r.prefs.clipboard_command, r.prefs.emoji_command), (vec!["copyq".to_string(), "toggle".to_string()], vec!["bemoji".to_string()]));
    }

    #[test]
    fn widgets_are_looked_up_again_only_after_the_recheck_or_with_another_path_or_command() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new("svc-widgets-cache");
        let p = palette(&t, |_| {});
        p.lock().widgets_recheck = Duration::from_secs(3600);
        let env = SessionEnv { path: Some(t.join("bin").display().to_string()), ..SessionEnv::default() };
        let clip = |p: &Palette, env: &SessionEnv| p.prepare(None, env).unwrap().widgets.clipboard_history;
        assert!(!clip(&p, &env));
        let noctalia = t.write("bin/noctalia", "#!/bin/sh\n");
        std::fs::set_permissions(&noctalia, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!clip(&p, &env), "within the recheck interval the earlier answer stands");
        // anderer Suchpfad: sofort neu
        let both = SessionEnv { path: Some(format!("{}:{}", t.join("bin").display(), t.join("more").display())), ..SessionEnv::default() };
        assert!(clip(&p, &both));
        // anderer Befehl: sofort neu (copyq fehlt)
        p.configure(&PaletteCfg { project_roots: vec![], clipboard_command: vec!["copyq".into()], ..PaletteCfg::default() });
        assert!(!clip(&p, &both));
        // nach Ablauf neu
        p.configure(&PaletteCfg { project_roots: vec![], ..PaletteCfg::default() });
        assert!(clip(&p, &both));
        std::fs::remove_file(&noctalia).unwrap();
        assert!(clip(&p, &both), "still cached");
        p.lock().widgets_recheck = Duration::ZERO;
        assert!(!clip(&p, &both));
    }

    #[test]
    fn the_diagnosis_is_offered_only_while_it_is_on_and_chosen_is_remembered() {
        let t = TempDir::new("svc-diagnose");
        let p = palette(&t, |_| {});
        let env = SessionEnv::default();
        // Vorgabe aus, bis der Kern es setzt: nichts zur Diagnose, auch nicht aus dem Modell
        let r = p.resolve("diagnose", None, &env).unwrap();
        assert!(ids(&r).iter().all(|i| !i.starts_with("diagnose:")), "{:?}", ids(&r));
        let prepared = p.prepare(None, &env).unwrap();
        let intent = Intent::Diagnose { profile: None, unit: None, container: None };
        assert_eq!(p.resolve_intent(&prepared, &intent, "warum").unwrap_err(), NoTarget::DiagnosisOff);
        // an: angeboten, und `pal_chosen` nimmt genau das angebotene Paar an
        p.set_diagnose_enabled(true);
        let r = p.resolve("absturz", None, &env).unwrap();
        assert_eq!(ids(&r)[0], "diagnose:last_crash");
        assert_eq!(p.chosen("diagnose", "last_crash"), Ok(true));
        assert_eq!(p.chosen("diagnose", "evil").unwrap_err(), PaletteError::UnknownTarget);
        let found = p.resolve_intent(&prepared, &intent, "warum").unwrap();
        assert_eq!((found[0].intent.name(), found[0].target_id.as_str()), ("diagnose", "diagnose"));
        assert_eq!(p.chosen("diagnose", "diagnose"), Ok(true));
        // wieder aus: gilt sofort
        p.set_diagnose_enabled(false);
        assert!(ids(&p.resolve("absturz", None, &env).unwrap()).iter().all(|i| !i.starts_with("diagnose:")));
        assert_eq!(p.resolve_intent(&prepared, &intent, "warum").unwrap_err(), NoTarget::DiagnosisOff);
        let file = std::fs::read_to_string(t.join("state/frecency.json")).unwrap();
        assert!(file.contains("diagnose:last_crash") && !file.contains("evil"), "{file}");
    }

    #[test]
    fn chosen_only_accepts_what_was_offered() {
        let t = TempDir::new("svc-chosen");
        t.write("share/applications/alpha.desktop", &entry("Alpha", ""));
        let p = palette(&t, |_| {});
        let env = SessionEnv::default();
        // noch nichts angeboten
        assert_eq!(p.chosen("launch_app", "alpha.desktop").unwrap_err(), PaletteError::UnknownTarget);
        p.resolve("alpha", None, &env).unwrap();
        assert_eq!(p.chosen("launch_app", "alpha.desktop"), Ok(true));
        // Absicht unbekannt, Ziel nie angeboten, Ziel einer anderen Absicht, freier Text
        assert_eq!(p.chosen("launch_ap", "alpha.desktop").unwrap_err(), PaletteError::UnknownIntent("launch_ap".into()));
        assert_eq!(p.chosen("launch_app", "evil.desktop").unwrap_err(), PaletteError::UnknownTarget);
        assert_eq!(p.chosen("open_project", "alpha.desktop").unwrap_err(), PaletteError::UnknownTarget);
        assert_eq!(p.chosen("launch_app", "my secret password").unwrap_err(), PaletteError::UnknownTarget);
        assert_eq!(p.chosen("launch_app", "").unwrap_err(), PaletteError::UnknownTarget);
        // Steuerzeichen im Namen der Absicht kommen nicht in die Meldung
        assert_eq!(p.chosen("a\nb", "x").unwrap_err(), PaletteError::UnknownIntent("ab".into()));
        let file = std::fs::read_to_string(t.join("state/frecency.json")).unwrap();
        assert!(file.contains("launch_app:alpha.desktop") && !file.contains("secret") && !file.contains("evil"), "{file}");
    }

    #[test]
    fn offered_pairs_age_out_after_many_rounds() {
        let t = TempDir::new("svc-offered");
        t.write("share/applications/alpha.desktop", &entry("Alpha", ""));
        t.write("share/applications/beta.desktop", &entry("Beta", ""));
        let p = palette(&t, |_| {});
        let env = SessionEnv::default();
        p.resolve("alpha", None, &env).unwrap();
        for _ in 0..OFFERED_ROUNDS - 1 {
            p.resolve("beta", None, &env).unwrap();
        }
        assert_eq!(p.chosen("launch_app", "alpha.desktop"), Ok(true), "gerade noch in den letzten Antworten");
        p.resolve("beta", None, &env).unwrap();
        assert_eq!(p.chosen("launch_app", "alpha.desktop").unwrap_err(), PaletteError::UnknownTarget);
        assert_eq!(p.chosen("launch_app", "beta.desktop"), Ok(true));
    }

    #[test]
    fn frecency_off_records_nothing_and_clear_always_works() {
        let t = TempDir::new("svc-frecency");
        t.write("share/applications/alpha.desktop", &entry("Alpha", ""));
        let p = palette(&t, |c| c.frecency = false);
        let env = SessionEnv::default();
        p.resolve("alpha", None, &env).unwrap();
        assert_eq!(p.chosen("launch_app", "alpha.desktop"), Ok(false));
        assert!(!t.join("state/frecency.json").exists());
        // einschalten gilt sofort
        let mut cfg = p.lock().cfg.clone();
        cfg.frecency = true;
        p.configure(&cfg);
        assert_eq!(p.chosen("launch_app", "alpha.desktop"), Ok(true));
        assert!(t.join("state/frecency.json").exists());
        p.clear_history().unwrap();
        assert!(!t.join("state/frecency.json").exists());
    }

    #[test]
    fn session_path_and_desktop_decide_try_exec_and_only_show_in() {
        let t = TempDir::new("svc-env");
        t.write("share/applications/tool.desktop", &entry("Tool", "TryExec=fixture-tool\n"));
        t.write("share/applications/awesome-only.desktop", &entry("Awesome Only", "OnlyShowIn=awesome;\n"));
        let bin = t.mkdir("bin");
        let tool = t.write("bin/fixture-tool", "#!/bin/sh\n");
        std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let p = palette(&t, |_| {});
        // die Programme, die der Dienst nach dieser Anfrage kennt
        let seen = |env: &SessionEnv| {
            p.resolve("", None, env).unwrap();
            let apps = p.lock().sources.as_ref().unwrap().apps();
            let mut found: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
            found.sort();
            found
        };
        let with = SessionEnv { path: Some(bin.display().to_string()), desktop: Some("awesome".into()), ..SessionEnv::default() };
        let without = SessionEnv { path: Some("/nonexistent:relative/dir".into()), desktop: Some("GNOME".into()), ..SessionEnv::default() };
        assert_eq!(seen(&with), ["awesome-only.desktop", "tool.desktop"]);
        assert!(seen(&without).is_empty());
        assert_eq!(seen(&with), ["awesome-only.desktop", "tool.desktop"], "zurück: neu gelesen");
        // ohne Angabe gilt die Umgebung des Daemons (hier keine: TryExec scheitert, OnlyShowIn blendet aus)
        assert!(seen(&SessionEnv::default()).is_empty());
        // ein gleichbleibender Wert liest nicht neu
        let before = p.lock().sources.as_ref().unwrap().scan_counts().apps;
        seen(&SessionEnv::default());
        assert_eq!(p.lock().sources.as_ref().unwrap().scan_counts().apps, before);
    }

    #[test]
    fn session_data_dirs_and_data_home_decide_where_programs_are_read() {
        let t = TempDir::new("svc-datadirs");
        t.write("share/applications/base.desktop", &entry("Base", ""));
        t.write("flatpak/applications/flat.desktop", &entry("Flat", ""));
        t.write("sys/applications/sys.desktop", &entry("Sys", ""));
        t.write("home/applications/mine.desktop", &entry("Mine", ""));
        let p = palette(&t, |_| {});
        let seen = |env: &SessionEnv| {
            p.resolve("", None, env).unwrap();
            let apps = p.lock().sources.as_ref().unwrap().apps();
            let mut found: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
            found.sort();
            found
        };
        let dirs = |d: &str| SessionEnv { data_dirs: Some(d.into()), ..SessionEnv::default() };
        // ohne Angabe die Verzeichnisse des Daemons (hier nur `share`)
        assert_eq!(seen(&SessionEnv::default()), ["base.desktop"]);
        // die der Sitzung ersetzen sie, in Rangfolge; relative Einträge entfallen
        let (flat, sys) = (t.join("flatpak").display().to_string(), t.join("sys").display().to_string());
        assert_eq!(seen(&dirs(&format!("{flat}:relative/dir::{sys}"))), ["flat.desktop", "sys.desktop"]);
        // Änderung: neu gelesen; zurück zu `None`: wieder die des Daemons
        assert_eq!(seen(&dirs(&flat)), ["flat.desktop"]);
        assert_eq!(seen(&SessionEnv::default()), ["base.desktop"]);
        // gesetzt, aber leer oder ohne absoluten Eintrag: die Vorgabe der Spec (`/usr/local/share:/usr/share`), nicht die
        // des Daemons (hier gibt es dort nichts aus der Fixture)
        for empty in ["", "relative/dir"] {
            let found = seen(&dirs(empty));
            assert!(!found.contains(&"base.desktop".to_string()) && !found.contains(&"flat.desktop".to_string()), "{empty:?}: {found:?}");
        }
        // `XDG_DATA_HOME` kommt vor den Verzeichnissen; ein relativer Wert ändert nichts
        let home = t.join("home").display().to_string();
        let both = SessionEnv { data_dirs: Some(flat.clone()), data_home: Some(home.clone()), ..SessionEnv::default() };
        assert_eq!(seen(&both), ["flat.desktop", "mine.desktop"]);
        let home_only = SessionEnv { data_home: Some(home), ..SessionEnv::default() };
        assert_eq!(seen(&home_only), ["base.desktop", "mine.desktop"]);
        let relative = SessionEnv { data_home: Some("relative/home".into()), ..SessionEnv::default() };
        assert_eq!(seen(&relative), ["base.desktop"]);
        // ein gleichbleibender Wert liest nicht neu
        let before = p.lock().sources.as_ref().unwrap().scan_counts().apps;
        seen(&relative);
        assert_eq!(p.lock().sources.as_ref().unwrap().scan_counts().apps, before);
    }

    #[test]
    fn invalid_session_values_are_refused() {
        let t = TempDir::new("svc-badenv");
        let p = palette(&t, |_| {});
        for env in [
            SessionEnv { path: Some("/bin\0/x".into()), ..SessionEnv::default() },
            SessionEnv { path: Some("/bin\n".into()), ..SessionEnv::default() },
            SessionEnv { path: Some("/".repeat(MAX_ENV_BYTES + 1)), ..SessionEnv::default() },
            SessionEnv { desktop: Some("a\tb".into()), ..SessionEnv::default() },
            SessionEnv { data_dirs: Some("/usr/share:\n/x".into()), ..SessionEnv::default() },
            SessionEnv { data_dirs: Some("/".repeat(MAX_ENV_BYTES + 1)), ..SessionEnv::default() },
            SessionEnv { data_home: Some("/home/\u{7f}u".into()), ..SessionEnv::default() },
        ] {
            assert!(matches!(p.resolve("x", None, &env), Err(PaletteError::BadEnv(_))), "{env:?}");
        }
        // genau die erlaubte Länge geht
        let at_limit = SessionEnv { data_dirs: Some("/".repeat(MAX_ENV_BYTES)), ..SessionEnv::default() };
        assert!(p.resolve("x", None, &at_limit).is_ok());
        assert!(PaletteError::BadEnv("PATH").to_string().starts_with("PATH of the session"));
        let named = SessionEnv { data_home: Some("/a\tb".into()), ..SessionEnv::default() };
        assert_eq!(p.resolve("x", None, &named).unwrap_err(), PaletteError::BadEnv("XDG_DATA_HOME"));
    }

    #[test]
    fn project_settings_apply_at_the_next_access() {
        let t = TempDir::new("svc-roots");
        t.mkdir("git/one/.git");
        t.mkdir("other/two/.git");
        let p = palette(&t, |_| {});
        let env = SessionEnv::default();
        let names = |p: &Palette| p.lock().sources.as_ref().unwrap().projects().iter().map(|x| x.name.clone()).collect::<Vec<_>>();
        p.resolve("one", None, &env).unwrap();
        assert_eq!(names(&p), ["one"]);
        let mut cfg = p.lock().cfg.clone();
        cfg.project_roots = vec![t.join("other").display().to_string()];
        p.configure(&cfg);
        p.resolve("two", None, &env).unwrap();
        assert_eq!(names(&p), ["two"]);
    }

    #[test]
    fn refresh_rereads_at_once() {
        let t = TempDir::new("svc-refresh");
        t.write("share/applications/a.desktop", &entry("Alpha", ""));
        let p = palette(&t, |_| {});
        let env = SessionEnv::default();
        let known = |p: &Palette| {
            p.resolve("", None, &env).unwrap();
            let apps = p.lock().sources.as_ref().unwrap().apps();
            apps.len()
        };
        p.refresh().unwrap(); // vor dem ersten Zugriff: nichts zu tun
        assert_eq!(known(&p), 1);
        t.write("share/applications/b.desktop", &entry("Beta", ""));
        // innerhalb des Prüfabstands unbemerkt, nach refresh sofort sichtbar
        assert_eq!(known(&p), 1);
        p.refresh().unwrap();
        assert_eq!(known(&p), 2);
    }

    #[test]
    fn palette_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + Clone>() {}
        assert_send_sync::<Palette>();
    }
}
