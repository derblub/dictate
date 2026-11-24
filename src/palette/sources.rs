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

//! Quellen der Befehlspalette (Aufgabe 3 in `PLAN-palette.md`): installierte Programme, Projekte und Häufigkeit.
//!
//! - [`desktop`]: Programme aus Desktop-Einträgen (`.desktop`) nach der Desktop Entry Specification, mit strenger
//!   Zerlegung von `Exec` in eine argv-Liste ([`desktop::split_exec`]). Nie ein Shell-String.
//! - [`projects`]: Git-Repositories unter konfigurierbaren Wurzeln.
//! - [`frecency`]: Häufigkeit und Aktualität gewählter Ziele (nur Ziel-IDs, nie Eingabetext).
//! - [`Sources`]: Cache für Programme und Projekte, der nur neu liest, wenn sich beobachtete Verzeichnisse oder Dateien
//!   geändert haben (mtime, höchstens alle [`DEFAULT_MIN_CHECK_INTERVAL`]); aus mehreren Threads nutzbar.
//!
//! Dieses Modul kennt weder Resolver noch Protokoll noch Daemon: Der Resolver (Aufgabe 2) bekommt die Daten als
//! [`desktop::AppIndex`] und `Vec<`[`projects::Project`]`>` und die Häufigkeit über [`frecency::Frecency::score`].
//! Konfigurierbare Werte (Projektwurzeln, Tiefe, Frecency an/aus) sind hier nur Parameter ([`SourcesConfig`],
//! [`projects::ProjectScanConfig`], [`frecency::FrecencySettings`]); die `[palette]`-Sektion der Config kommt später.
//!
//! **Kein inotify.** Eine Abhängigkeit mehr für etwas, das eine `stat`-Runde über wenige hundert Pfade alle zwei
//! Sekunden ebenso zuverlässig erledigt (kein Thread, keine verlorenen Ereignisse bei überlaufender Queue, keine
//! Sonderfälle bei neu angelegten Wurzeln); gemessen wird bei jeder Abfrage, aber nur einmal je Mindestabstand.

pub mod desktop;
pub mod frecency;
pub mod projects;

pub use desktop::{parse_exec_value, split_exec, App, AppIndex, AppScan, DesktopEnv, ExecContext, ExecError, Reject};
pub use frecency::{Frecency, FrecencySettings};
pub use projects::{scan_projects, Project, ProjectScan, ProjectScanConfig};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// Mindestabstand zwischen zwei mtime-Prüfungen (die Palette fragt bei jedem Tastendruck).
pub const DEFAULT_MIN_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Was an einem Pfad beobachtet wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeKind {
    /// Änderungszeit (Verzeichnis: Einträge hinzugekommen oder weg; Datei: Inhalt geändert).
    Mtime,
    /// Nur, ob der Pfad existiert (für `.git`: dessen Inneres ändert sich bei jedem Git-Aufruf, die Existenz nicht).
    Presence,
}

/// Messwert eines Pfades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stamp {
    Missing,
    Present,
    Mtime(SystemTime),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Probe {
    path: PathBuf,
    kind: ProbeKind,
    seen: Stamp,
}

impl Probe {
    fn take(path: &Path, kind: ProbeKind) -> Probe {
        Probe { path: path.to_path_buf(), kind, seen: Probe::measure(path, kind) }
    }

    fn measure(path: &Path, kind: ProbeKind) -> Stamp {
        match kind {
            ProbeKind::Mtime => match std::fs::metadata(path).and_then(|m| m.modified()) {
                Ok(t) => Stamp::Mtime(t),
                Err(_) => Stamp::Missing,
            },
            ProbeKind::Presence => match std::fs::symlink_metadata(path) {
                Ok(_) => Stamp::Present,
                Err(_) => Stamp::Missing,
            },
        }
    }
}

/// Merkt sich, was ein Durchlauf angesehen hat, und prüft später, ob es noch so ist. Gemessen wird *vor* dem Lesen
/// eines Verzeichnisses: ändert sich währenddessen etwas, erkennt es die nächste Prüfung (statt es zu verschlucken).
#[derive(Debug, Clone, Default)]
pub struct Fingerprint {
    probes: Vec<Probe>,
}

impl Fingerprint {
    /// Beobachtet die Änderungszeit von `path` (Verzeichnis oder Datei; folgt Symlinks). Ein fehlender Pfad zählt als
    /// Messwert: taucht er auf, ist das eine Änderung.
    pub fn watch_mtime(&mut self, path: &Path) {
        self.probes.push(Probe::take(path, ProbeKind::Mtime));
    }

    /// Beobachtet nur, ob `path` existiert (ohne Symlinks aufzulösen).
    pub fn watch_presence(&mut self, path: &Path) {
        self.probes.push(Probe::take(path, ProbeKind::Presence));
    }

    /// Nimmt eine bereits früher gemessene Beobachtung auf (für Aufrufer, die erst messen und dann entscheiden).
    fn push(&mut self, probe: Probe) {
        self.probes.push(probe);
    }

    /// Anzahl der beobachteten Pfade.
    pub fn len(&self) -> usize {
        self.probes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// `true`, wenn alle beobachteten Pfade noch denselben Messwert haben.
    pub fn is_current(&self) -> bool {
        self.probes.iter().all(|p| Probe::measure(&p.path, p.kind) == p.seen)
    }
}

/// Einstellungen aller Quellen außer der Häufigkeit (die hat eine eigene Datei und eigene Einstellungen).
#[derive(Debug, Clone)]
pub struct SourcesConfig {
    /// Umgebung für die Desktop-Einträge (XDG-Verzeichnisse, Desktop, Locale, `PATH`).
    pub desktop: DesktopEnv,
    pub projects: ProjectScanConfig,
    /// Mindestabstand zwischen zwei mtime-Prüfungen.
    pub min_check_interval: Duration,
}

impl SourcesConfig {
    /// Umgebung des laufenden Prozesses, Projektwurzel `~` (Home), Prüfabstand [`DEFAULT_MIN_CHECK_INTERVAL`].
    pub fn from_process() -> SourcesConfig {
        SourcesConfig { desktop: DesktopEnv::from_process(), projects: ProjectScanConfig::default(), min_check_interval: DEFAULT_MIN_CHECK_INTERVAL }
    }
}

impl Default for SourcesConfig {
    fn default() -> SourcesConfig {
        SourcesConfig::from_process()
    }
}

/// Ein zwischengespeichertes Ergebnis samt Fingerabdruck.
struct Slot<T> {
    value: Option<Arc<T>>,
    fingerprint: Fingerprint,
    /// Letzte mtime-Prüfung (oder letzter Durchlauf).
    checked: Option<Instant>,
    /// Wie oft wirklich gelesen wurde (für Tests und die Diagnose).
    scans: usize,
}

impl<T> Slot<T> {
    fn empty() -> Slot<T> {
        Slot { value: None, fingerprint: Fingerprint::default(), checked: None, scans: 0 }
    }

    /// Verwirft den Wert; der nächste Zugriff liest neu. Die Zählung der Durchläufe bleibt.
    fn invalidate(&mut self) {
        self.value = None;
        self.fingerprint = Fingerprint::default();
        self.checked = None;
    }

    fn store(&mut self, value: T, fingerprint: Fingerprint) -> Arc<T> {
        let value = Arc::new(value);
        self.value = Some(Arc::clone(&value));
        self.fingerprint = fingerprint;
        self.checked = Some(Instant::now());
        self.scans += 1;
        value
    }

    /// Liefert den gemerkten Wert; liest nur neu, wenn noch nichts da ist oder (frühestens nach `interval`) der
    /// Fingerabdruck nicht mehr stimmt. `scan` läuft unter der Sperre des Aufrufers: gleichzeitige Abfragen warten auf
    /// *einen* Durchlauf, statt mehrere zu starten.
    fn get(&mut self, interval: Duration, scan: impl FnOnce() -> (T, Fingerprint)) -> Arc<T> {
        let due = self.checked.is_none_or(|t| t.elapsed() >= interval);
        if let Some(value) = &self.value {
            if !due {
                return Arc::clone(value);
            }
            if self.fingerprint.is_current() {
                self.checked = Some(Instant::now());
                return Arc::clone(value);
            }
        }
        let (value, fingerprint) = scan();
        self.store(value, fingerprint)
    }
}

struct State {
    /// Als `Arc`, damit eine Abfrage die Einstellungen für den Durchlauf teilt statt kopiert (jede Abfrage kommt bei
    /// jedem Tastendruck).
    cfg: Arc<SourcesConfig>,
    apps: Slot<AppIndex>,
    projects: Slot<Vec<Project>>,
}

/// Wie oft [`Sources`] wirklich gelesen hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanCounts {
    pub apps: usize,
    pub projects: usize,
}

/// Cache für Programme und Projekte. Gibt unveränderliche Schnappschüsse ([`Arc`]) heraus, die der Aufrufer ohne Sperre
/// benutzen kann, und liest nur neu, wenn sich beobachtete Pfade geändert haben. Alle Methoden nehmen `&self`; die
/// Struktur ist `Send + Sync` und für den Verbindungs-Thread des Daemons gedacht.
pub struct Sources {
    state: Mutex<State>,
}

impl Sources {
    pub fn new(cfg: SourcesConfig) -> Sources {
        Sources { state: Mutex::new(State { cfg: Arc::new(cfg), apps: Slot::empty(), projects: Slot::empty() }) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Ein Thread, der beim Lesen abstürzt, lässt nur einen unfertigen Cache zurück, der beim nächsten Mal neu
        // gelesen wird; die Sperre zu vergiften würde die Palette dauerhaft lahmlegen.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Installierte Programme (aktuell nach mtime-Prüfung).
    pub fn apps(&self) -> Arc<AppIndex> {
        let mut st = self.lock();
        let cfg = Arc::clone(&st.cfg);
        st.apps.get(cfg.min_check_interval, || {
            let scan = desktop::scan_apps(&cfg.desktop);
            (scan.apps, scan.fingerprint)
        })
    }

    /// Projekte (aktuell nach mtime-Prüfung), nach Pfad sortiert.
    pub fn projects(&self) -> Arc<Vec<Project>> {
        let mut st = self.lock();
        let cfg = Arc::clone(&st.cfg);
        st.projects.get(cfg.min_check_interval, || {
            let scan = projects::scan_projects(&cfg.projects);
            (scan.projects, scan.fingerprint)
        })
    }

    /// Verwirft beide Zwischenspeicher: der nächste Zugriff liest neu (für `pal_sources_refresh`).
    pub fn refresh(&self) {
        let mut st = self.lock();
        st.apps.invalidate();
        st.projects.invalidate();
    }

    /// Neue Einstellungen (z. B. geänderte Projektwurzeln); verwirft die Zwischenspeicher.
    pub fn reconfigure(&self, cfg: SourcesConfig) {
        let mut st = self.lock();
        st.cfg = Arc::new(cfg);
        st.apps.invalidate();
        st.projects.invalidate();
    }

    /// Wie oft seit dem Anlegen wirklich gelesen wurde (für Tests und die Diagnose).
    pub fn scan_counts(&self) -> ScanCounts {
        let st = self.lock();
        ScanCounts { apps: st.apps.scans, projects: st.projects.scans }
    }
}

/// Unix-Zeit in Sekunden (für [`Frecency`]).
pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Temp-Verzeichnisse für die Fixture-Tests (tempfile steht nicht in den Abhängigkeiten).
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Eigenes Verzeichnis unter `std::env::temp_dir()`, wird beim Verwerfen gelöscht.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> TempDir {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("dictate-palette-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn join(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }

        /// Schreibt eine Datei (samt Ordnern) und gibt ihren Pfad zurück.
        pub fn write(&self, rel: &str, content: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, content).unwrap();
            p
        }

        pub fn mkdir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Setzt die Änderungszeit auf einen festen Wert (Sekunden nach 2017), damit Tests nicht von der Auflösung der
    /// Dateisystemuhr abhängen: zwei Schreibvorgänge kurz hintereinander können dieselbe mtime bekommen.
    pub fn set_mtime(path: &Path, n: u64) {
        let t: SystemTime = UNIX_EPOCH + Duration::from_secs(1_500_000_000 + n);
        std::fs::File::open(path).unwrap().set_modified(t).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{set_mtime, TempDir};
    use super::*;

    fn entry(name: &str, exec: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n")
    }

    /// Umgebung, die nur auf den Fixture-Ordner zeigt, mit Prüfabstand null.
    fn config(t: &TempDir) -> SourcesConfig {
        let desktop = DesktopEnv { data_home: Some(t.join("home")), data_dirs: vec![t.join("sys")], ..DesktopEnv::default() };
        let projects = ProjectScanConfig { roots: vec![t.join("git")], ..ProjectScanConfig::default() };
        SourcesConfig { desktop, projects, min_check_interval: Duration::ZERO }
    }

    #[test]
    fn apps_are_scanned_once_and_again_only_after_a_change() {
        let t = TempDir::new("cache-apps");
        t.write("sys/applications/a.desktop", &entry("Alpha", "alpha"));
        set_mtime(&t.join("sys/applications"), 1);
        let sources = Sources::new(config(&t));

        let first = sources.apps();
        assert_eq!(first.len(), 1);
        for _ in 0..5 {
            let again = sources.apps();
            assert!(Arc::ptr_eq(&first, &again), "unverändert: derselbe Schnappschuss, kein neuer Durchlauf");
        }
        assert_eq!(sources.scan_counts().apps, 1);

        // neue Datei: die mtime des Ordners ändert sich
        t.write("sys/applications/b.desktop", &entry("Beta", "beta"));
        set_mtime(&t.join("sys/applications"), 2);
        let second = sources.apps();
        assert_eq!(second.len(), 2);
        assert_eq!(sources.scan_counts().apps, 2);
        assert_eq!(first.len(), 1, "der alte Schnappschuss bleibt unverändert benutzbar");
        sources.apps();
        assert_eq!(sources.scan_counts().apps, 2);

        // Inhalt einer vorhandenen Datei geändert (Ordner-mtime bleibt): die Datei-mtime zählt
        t.write("sys/applications/a.desktop", &entry("Alpha Two", "alpha"));
        set_mtime(&t.join("sys/applications/a.desktop"), 3);
        assert_eq!(sources.apps().get("a.desktop").unwrap().name, "Alpha Two");
        assert_eq!(sources.scan_counts().apps, 3);

        // Datei gelöscht
        std::fs::remove_file(t.join("sys/applications/b.desktop")).unwrap();
        set_mtime(&t.join("sys/applications"), 4);
        assert_eq!(sources.apps().len(), 1);
        assert_eq!(sources.scan_counts().apps, 4);
    }

    #[test]
    fn a_root_that_appears_later_is_noticed() {
        let t = TempDir::new("cache-late-root");
        let sources = Sources::new(config(&t));
        assert!(sources.apps().is_empty());
        assert!(sources.projects().is_empty());
        t.write("home/applications/x.desktop", &entry("X", "x"));
        assert_eq!(sources.apps().len(), 1, "fehlendes Verzeichnis erschienen");
        t.mkdir("git/proj/.git");
        assert_eq!(sources.projects().len(), 1, "fehlende Wurzel erschienen");
    }

    #[test]
    fn projects_are_scanned_once_and_again_only_after_a_change() {
        let t = TempDir::new("cache-projects");
        t.mkdir("git/grp/one/.git");
        set_mtime(&t.join("git"), 1);
        set_mtime(&t.join("git/grp"), 1);
        let sources = Sources::new(config(&t));

        let first = sources.projects();
        assert_eq!(first.len(), 1);
        for _ in 0..5 {
            assert!(Arc::ptr_eq(&first, &sources.projects()));
        }
        assert_eq!(sources.scan_counts().projects, 1);

        // neues Repository in einer Gruppe
        t.mkdir("git/grp/two/.git");
        set_mtime(&t.join("git/grp"), 2);
        assert_eq!(sources.projects().len(), 2);
        assert_eq!(sources.scan_counts().projects, 2);

        // `.git` eines bekannten Repositories verschwindet; weder `git/grp` noch `git` ändern ihre mtime
        std::fs::remove_dir_all(t.join("git/grp/one/.git")).unwrap();
        let names: Vec<_> = sources.projects().iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["two"]);
        assert_eq!(sources.scan_counts().projects, 3);

        // Änderungen *im* Repository (Git-Betrieb, Editor) lösen keinen Durchlauf aus
        t.write("git/grp/two/src/main.rs", "fn main() {}");
        t.write("git/grp/two/.git/HEAD", "ref: refs/heads/main");
        sources.projects();
        assert_eq!(sources.scan_counts().projects, 3);

        // ein bisher leerer Ordner wird mit `git init` zum Repository
        t.mkdir("git/grp/three");
        set_mtime(&t.join("git/grp"), 3);
        assert_eq!(sources.projects().len(), 1);
        t.mkdir("git/grp/three/.git");
        set_mtime(&t.join("git/grp/three"), 4);
        assert_eq!(sources.projects().len(), 2);
    }

    #[test]
    fn min_check_interval_limits_how_often_the_disk_is_looked_at() {
        let t = TempDir::new("cache-interval");
        t.write("sys/applications/a.desktop", &entry("Alpha", "alpha"));
        let mut cfg = config(&t);
        cfg.min_check_interval = Duration::from_secs(3600);
        let sources = Sources::new(cfg);
        assert_eq!(sources.apps().len(), 1);

        t.write("sys/applications/b.desktop", &entry("Beta", "beta"));
        set_mtime(&t.join("sys/applications"), 7);
        assert_eq!(sources.apps().len(), 1, "innerhalb des Mindestabstands wird nicht geprüft");

        sources.refresh();
        assert_eq!(sources.apps().len(), 2, "refresh liest sofort neu");
        assert_eq!(sources.scan_counts().apps, 2);
    }

    #[test]
    fn reconfigure_applies_new_roots() {
        let t = TempDir::new("cache-reconf");
        t.mkdir("git/a/.git");
        t.mkdir("other/b/.git");
        let sources = Sources::new(config(&t));
        assert_eq!(sources.projects()[0].name, "a");
        let mut cfg = config(&t);
        cfg.projects.roots = vec![t.join("other")];
        sources.reconfigure(cfg);
        assert_eq!(sources.projects()[0].name, "b");
    }

    #[test]
    fn concurrent_readers_trigger_a_single_scan() {
        let t = TempDir::new("cache-threads");
        for i in 0..20 {
            t.write(&format!("sys/applications/app{i}.desktop"), &entry(&format!("App {i}"), "app"));
        }
        t.mkdir("git/x/.git");
        let sources = Arc::new(Sources::new(config(&t)));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let s = Arc::clone(&sources);
                std::thread::spawn(move || (s.apps().len(), s.projects().len()))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), (20, 1));
        }
        assert_eq!(sources.scan_counts(), ScanCounts { apps: 1, projects: 1 });
    }

    #[test]
    fn sources_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Sources>();
        assert_send_sync::<Frecency>();
        assert_send_sync::<AppIndex>();
    }
}
