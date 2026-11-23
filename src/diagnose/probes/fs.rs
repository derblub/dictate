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

//! Dateizugriff der Lese-Proben (`Exec::Read`): feste Orte, Pfadbasis austauschbar, jeder Zugriff gegen die im Katalog
//! deklarierte Pfadliste der Probe geprüft.
//!
//! [`ProbePaths`] legt fest, *wo* die festen Orte liegen: das Wurzelverzeichnis für `/proc`, `/sys`,
//! `/var/lib/systemd/pstore` und `/var/log/pacman.log` (in Produktion `/`, in Tests ein Fixture-Ordner), das
//! Home-Verzeichnis (für `~` in einer `Exec::Read`-Liste; bis W7 `~/.local/share/xorg`) und der Ordner des temp-loggers (`diagnose.temp_log_dir`).
//!
//! [`Fs`] gehört zu *einer* Probe und öffnet nur, was deren `Exec::Read`-Liste im Katalog nennt (`*` steht für einen Teil
//! eines Namens, `**` für beliebig viele Ordner): `/proc/*/stat` erlaubt nur `stat` direkt unter einem Eintrag von
//! `/proc`, nie `cmdline`. Ein anderer Pfad ist ein Fehler (`PermissionDenied`), auch wenn er im Code der Probe
//! steht; ein Test lässt jede Lese-Probe gegen ein Fixture laufen und prüft so beides. Geöffnet wird mit `O_NOFOLLOW`
//! (der letzte Teil darf kein Symlink sein) und `O_NONBLOCK` (eine FIFO an der Stelle einer Datei blockiert nicht), nur
//! reguläre Dateien, und gelesen wird nie mehr als die Grenze, die die Probe nennt. Ordner dürfen nur die Pfade
//! heißen, die einen erlaubten Pfad enthalten können (ab dem Ordner mit dem ersten Platzhalter).

use crate::diagnose::argv::Exec;
use crate::diagnose::catalog::ProbeSpec;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

/// Vorgabe des Temperaturloggers (`diagnose.temp_log_dir`): leer, kein Logger.
pub const DEFAULT_TEMP_LOG_DIR: &str = crate::config::DIAGNOSE_DEFAULT_TEMP_LOG_DIR;

/// Wo die festen Orte der Lese-Proben liegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePaths {
    root: PathBuf,
    home: PathBuf,
    /// `None`: kein Temperaturlogger eingetragen, `{temp_log_dir}/…` erlaubt nichts
    temp_log_dir: Option<PathBuf>,
}

impl ProbePaths {
    /// Produktion: Wurzel `/`, Home des Users (aus `HOME`, sonst Benutzerdatenbank), Ordner des Temperaturloggers aus
    /// der Config (`None`: keiner). Nicht in Tests: dort gibt es nur [`ProbePaths::rooted`] mit Fixture-Ordnern, damit keine Probe das echte
    /// System liest.
    #[cfg(not(test))]
    pub fn system(temp_log_dir: Option<PathBuf>) -> Result<ProbePaths, String> {
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(dirs::home_dir).ok_or("no home directory")?;
        ProbePaths::rooted("/".into(), home, temp_log_dir)
    }

    /// Eigene Orte (Tests mit Fixtures). Alle Pfade müssen absolut sein und dürfen kein `..` enthalten.
    pub fn rooted(root: PathBuf, home: PathBuf, temp_log_dir: Option<PathBuf>) -> Result<ProbePaths, String> {
        for (what, p) in [("root", Some(&root)), ("home", Some(&home)), ("temp log directory", temp_log_dir.as_ref())] {
            if p.is_some_and(|p| !is_plain_absolute(p)) {
                return Err(format!("the {what} must be an absolute path without `..`"));
            }
        }
        Ok(ProbePaths { root, home, temp_log_dir })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Ordner des Temperaturloggers; `None`, wenn keiner eingetragen ist.
    pub fn temp_log_dir(&self) -> Option<&Path> {
        self.temp_log_dir.as_deref()
    }

    /// Ein fester Ort unter der Wurzel: `/proc/meminfo` wird `<root>/proc/meminfo`.
    pub fn under_root(&self, absolute: &str) -> PathBuf {
        self.root.join(absolute.trim_start_matches('/'))
    }
}

fn is_plain_absolute(p: &Path) -> bool {
    p.is_absolute() && p.components().all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

/// Ein Teil eines erlaubten Pfads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    /// Genau dieser Name.
    Lit(OsString),
    /// Name mit `*` (beliebig viele Zeichen).
    Glob(String),
    /// `**`: null oder mehr Ordner.
    Any,
}

/// Passt `name` auf das Muster mit `*`?
fn glob_match(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let first = parts[0];
    let last = parts[parts.len() - 1];
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

fn seg_matches(seg: &Seg, name: &std::ffi::OsStr) -> bool {
    match seg {
        Seg::Lit(l) => l == name,
        Seg::Glob(g) => name.to_str().is_some_and(|n| glob_match(g, n)),
        Seg::Any => true,
    }
}

/// Passen alle Namen genau auf die Teile? (`Any` darf null oder mehr Namen verbrauchen.)
fn segs_match(segs: &[Seg], names: &[&std::ffi::OsStr]) -> bool {
    match segs.split_first() {
        None => names.is_empty(),
        Some((Seg::Any, rest)) => (0..=names.len()).any(|k| segs_match(rest, &names[k..])),
        Some((seg, rest)) => match names.split_first() {
            Some((name, tail)) => seg_matches(seg, name) && segs_match(rest, tail),
            None => false,
        },
    }
}

/// Zerlegt einen Pfad in seine Namen (ohne Wurzel); `None`, wenn er nicht absolut ist oder `.`/`..` enthält.
fn names_of(path: &Path) -> Option<Vec<&std::ffi::OsStr>> {
    if !path.is_absolute() {
        return None;
    }
    let mut names = Vec::new();
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(n) => names.push(n),
            _ => return None,
        }
    }
    Some(names)
}

/// Gelesene Bytes einer Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRead {
    pub bytes: Vec<u8>,
    /// Die Datei hat mehr Bytes, als gelesen wurden.
    pub truncated: bool,
}

/// Ein Eintrag eines Ordners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    /// Der Eintrag ist selbst ein Symlink (ohne ihn zu verfolgen).
    pub symlink: bool,
    /// Ordner (ein Symlink wird verfolgt).
    pub is_dir: bool,
    /// Reguläre Datei (ein Symlink wird verfolgt).
    pub is_file: bool,
}

/// Dateizugriff einer Probe, begrenzt auf deren deklarierte Pfade.
#[derive(Debug)]
pub struct Fs {
    patterns: Vec<Vec<Seg>>,
    #[cfg(test)]
    accessed: std::sync::Mutex<Vec<PathBuf>>,
    probe: &'static str,
}

impl Fs {
    /// Der Zugriff der Probe `spec`: erlaubt sind genau die Pfade ihrer `Exec::Read`-Liste unter `paths`. Eine Probe
    /// mit Befehlen hat keinen Dateizugriff.
    pub fn for_spec(paths: &ProbePaths, spec: &ProbeSpec) -> Fs {
        match spec.exec {
            Exec::Read(list) => Fs::with_patterns(paths, list, spec.name),
            Exec::Commands(_) => Fs::with_patterns(paths, &[], spec.name),
        }
    }

    /// Der Zugriff der Auflösung: nur `/proc/uptime` (Alter des Boots für `since: boot` bei `docker_logs` und
    /// `coredumps`), sonst nichts.
    pub fn for_resolution(paths: &ProbePaths) -> Fs {
        Fs::with_patterns(paths, &["/proc/uptime"], "resolution")
    }

    fn with_patterns(paths: &ProbePaths, list: &[&str], probe: &'static str) -> Fs {
        Fs {
            patterns: list.iter().filter_map(|pattern| compile(paths, pattern)).collect(),
            #[cfg(test)]
            accessed: std::sync::Mutex::new(Vec::new()),
            probe,
        }
    }

    fn denied(&self, path: &Path) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, format!("probe {} may not open {}", self.probe, path.display()))
    }

    /// Ist die Datei durch die Liste der Probe erlaubt?
    pub fn allows_file(&self, path: &Path) -> bool {
        match names_of(path) {
            Some(names) => self.patterns.iter().any(|p| segs_match(p, &names)),
            None => false,
        }
    }

    /// Darf der Ordner aufgelistet werden? Nur, wenn er Teile eines erlaubten Pfads enthalten kann: der Ordner, der den
    /// ersten Platzhalter (`*` oder `**`) des Musters enthält, und alles darunter bis zur Datei. Die festen Vorfahren
    /// (`/var/lib/systemd` über dem pstore) und Muster ohne Platzhalter (eine einzelne Datei) erlauben kein Auflisten.
    pub fn allows_dir(&self, path: &Path) -> bool {
        let Some(names) = names_of(path) else { return false };
        self.patterns.iter().any(|p| match p.iter().position(|s| !matches!(s, Seg::Lit(_))) {
            Some(first) => (first..p.len()).any(|k| segs_match(&p[..k], &names)),
            None => false,
        })
    }

    #[cfg(test)]
    fn record(&self, path: &Path) {
        self.accessed.lock().unwrap().push(path.to_path_buf());
    }

    /// Alle bisher geöffneten Pfade (Tests).
    #[cfg(test)]
    pub fn accessed(&self) -> Vec<PathBuf> {
        self.accessed.lock().unwrap().clone()
    }

    /// Öffnet eine erlaubte reguläre Datei zum Lesen (`O_NOFOLLOW`, `O_NONBLOCK`).
    pub fn open(&self, path: &Path) -> io::Result<File> {
        if !self.allows_file(path) {
            return Err(self.denied(path));
        }
        #[cfg(test)]
        self.record(path);
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a regular file", path.display())));
        }
        Ok(file)
    }

    /// Liest höchstens `max` Bytes vom Anfang. (Dateien in `/proc` und `/sys` melden die Größe 0; gelesen wird bis zum
    /// Ende bzw. bis zur Grenze.)
    pub fn read(&self, path: &Path, max: usize) -> io::Result<FileRead> {
        let file = self.open(path)?;
        let mut bytes = Vec::new();
        file.take(max as u64 + 1).read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max;
        bytes.truncate(max);
        Ok(FileRead { bytes, truncated })
    }

    /// Liest höchstens die letzten `max` Bytes. `truncated` heißt: der Anfang fehlt (die erste Zeile ist dann meist
    /// angeschnitten; der Aufrufer wirft sie weg).
    pub fn read_tail(&self, path: &Path, max: usize) -> io::Result<FileRead> {
        let mut file = self.open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(max as u64);
        if start > 0 {
            file.seek(SeekFrom::Start(start))?;
        }
        let mut bytes = Vec::new();
        file.take(max as u64).read_to_end(&mut bytes)?;
        Ok(FileRead { bytes, truncated: start > 0 })
    }

    /// Größe und Änderungszeit einer erlaubten Datei (ohne Symlink zu verfolgen).
    pub fn file_meta(&self, path: &Path) -> io::Result<(u64, std::time::SystemTime)> {
        if !self.allows_file(path) {
            return Err(self.denied(path));
        }
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a regular file", path.display())));
        }
        Ok((meta.len(), meta.modified()?))
    }

    /// Einträge eines erlaubten Ordners, nach Name sortiert. Nicht lesbare Einträge fehlen.
    pub fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        if !self.allows_dir(path) {
            return Err(self.denied(path));
        }
        #[cfg(test)]
        self.record(path);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let Ok(entry) = entry else { continue };
            let entry_path = entry.path();
            let symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            let followed = std::fs::metadata(&entry_path);
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path,
                symlink,
                is_dir: followed.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                is_file: followed.as_ref().map(|m| m.is_file()).unwrap_or(false),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
}

/// Übersetzt einen Pfad der Katalogliste in Teile: `/…` unter der Wurzel, `~/…` unter dem Home, `{temp_log_dir}/…`.
/// `None` (nichts erlaubt) für `{temp_log_dir}/…` ohne eingetragenen Ordner.
fn compile(paths: &ProbePaths, pattern: &str) -> Option<Vec<Seg>> {
    let (base, rest): (&Path, &str) = if let Some(rest) = pattern.strip_prefix("~/") {
        (paths.home(), rest)
    } else if let Some(rest) = pattern.strip_prefix("{temp_log_dir}/") {
        (paths.temp_log_dir()?, rest)
    } else {
        (paths.root(), pattern.trim_start_matches('/'))
    };
    let mut segs: Vec<Seg> = base
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(Seg::Lit(n.to_os_string())),
            _ => None,
        })
        .collect();
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        segs.push(match part {
            "**" => Seg::Any,
            p if p.contains('*') => Seg::Glob(p.to_string()),
            p => Seg::Lit(OsString::from(p)),
        });
    }
    Some(segs)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::diagnose::catalog;
    use crate::diagnose::probes::testkit::TempDir;
    use std::os::unix::fs::symlink;

    fn paths(root: &Path) -> ProbePaths {
        ProbePaths::rooted(root.to_path_buf(), root.join("home/u"), Some(root.join("templog"))).unwrap()
    }

    fn fs_for(root: &Path, probe: &str) -> Fs {
        Fs::for_spec(&paths(root), catalog::spec(probe).unwrap())
    }

    #[test]
    fn glob_rules() {
        assert!(glob_match("dmesg*", "dmesg-efi_pstore-1"));
        assert!(glob_match("dmesg*", "dmesg"));
        assert!(!glob_match("dmesg*", "xdmesg"));
        assert!(glob_match("temps-*.csv", "temps-2026-11-04.csv"));
        assert!(!glob_match("temps-*.csv", "temps-2026-11-04.csv.bak"));
        assert!(!glob_match("temps-*.csv", "temps-.cs"));
        assert!(glob_match("temp*_input", "temp1_input"));
        assert!(glob_match("nvme*", "nvme0"));
        assert!(glob_match("a*b*c", "aXbYc"));
        assert!(!glob_match("a*b*c", "aXcYb"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exact2"));
    }

    #[test]
    fn only_declared_paths_open() {
        let dir = TempDir::new("fs-declared");
        let root = dir.path();
        let mem = fs_for(root, "mem");
        assert!(mem.allows_file(&root.join("proc/meminfo")));
        assert!(mem.allows_file(&root.join("proc/pressure/memory")));
        assert!(!mem.allows_file(&root.join("proc/cmdline")), "kernel_info's file, not mem's");
        assert!(!mem.allows_file(&root.join("proc/meminfo/../cmdline")));
        assert!(!mem.allows_file(&root.join("etc/shadow")));
        assert!(!mem.allows_file(Path::new("proc/meminfo")), "relative");
        // Ordner: Muster ohne Platzhalter erlauben kein Auflisten
        assert!(!mem.allows_dir(&root.join("proc")));
        assert!(!mem.allows_dir(&root.join("proc/pressure")));
        assert!(!mem.allows_dir(&root.join("etc")));

        let top = fs_for(root, "top_procs");
        assert!(top.allows_file(&root.join("proc/123/stat")));
        assert!(top.allows_file(&root.join("proc/123/status")));
        assert!(!top.allows_file(&root.join("proc/123/cmdline")), "never the command line");
        assert!(!top.allows_file(&root.join("proc/123/environ")));
        assert!(!top.allows_file(&root.join("proc/123/fd/0")));
        assert!(!top.allows_file(&root.join("proc/stat")), "load's file");
        assert!(top.allows_dir(&root.join("proc")));
        assert!(top.allows_dir(&root.join("proc/123")));

        let pstore = fs_for(root, "pstore");
        let base = root.join("var/lib/systemd/pstore");
        assert!(pstore.allows_file(&base.join("1793782077/001/dmesg.txt")));
        assert!(pstore.allows_file(&base.join("1793782077/001/dmesg-efi_pstore-1")));
        assert!(pstore.allows_file(&base.join("dmesg-x")));
        assert!(!pstore.allows_file(&base.join("1793782077/001/console-ramoops")));
        assert!(!pstore.allows_file(&root.join("var/lib/systemd/other/dmesg.txt")));
        assert!(pstore.allows_dir(&base));
        assert!(pstore.allows_dir(&base.join("1793782077/001")));
        assert!(!pstore.allows_dir(&root.join("var/lib/systemd")), "a fixed ancestor above the pstore is not listable");
        assert!(!pstore.allows_dir(&root.join("var/lib")));

        let temps = fs_for(root, "temps_tail");
        assert!(temps.allows_file(&root.join("templog/temps-2026-11-04.csv")));
        assert!(!temps.allows_file(&root.join("templog/other.csv")));
        assert!(!temps.allows_file(&root.join("templog/sub/temps-1.csv")));
        assert!(temps.allows_dir(&root.join("templog")));

        // Eine Probe mit Befehlen darf nichts öffnen
        let boots = fs_for(root, "boots");
        assert!(!boots.allows_file(&root.join("proc/meminfo")));
        assert!(!boots.allows_dir(&root.join("proc")));
        assert_eq!(boots.read(&root.join("proc/meminfo"), 10).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn reading_is_limited_and_never_follows_a_symlink_or_opens_a_fifo() {
        let dir = TempDir::new("fs-read");
        let root = dir.path();
        std::fs::create_dir_all(root.join("proc")).unwrap();
        std::fs::write(root.join("proc/meminfo"), "0123456789\nabcdef\n").unwrap();
        let mem = fs_for(root, "mem");
        let all = mem.read(&root.join("proc/meminfo"), 1000).unwrap();
        assert_eq!((all.bytes.len(), all.truncated), (18, false));
        let part = mem.read(&root.join("proc/meminfo"), 5).unwrap();
        assert_eq!((part.bytes.as_slice(), part.truncated), (&b"01234"[..], true));
        let tail = mem.read_tail(&root.join("proc/meminfo"), 7).unwrap();
        assert_eq!((tail.bytes.as_slice(), tail.truncated), (&b"abcdef\n"[..], true));
        let whole_tail = mem.read_tail(&root.join("proc/meminfo"), 100).unwrap();
        assert!(!whole_tail.truncated);
        assert_eq!(mem.file_meta(&root.join("proc/meminfo")).unwrap().0, 18);
        assert!(mem.read(&root.join("proc/loadavg"), 10).is_err(), "missing is an error, not a panic");

        // Symlink am Ende: nicht verfolgen
        std::fs::write(root.join("secret"), "x").unwrap();
        symlink(root.join("secret"), root.join("proc/pressure_link")).unwrap();
        std::fs::create_dir_all(root.join("proc/pressure")).unwrap();
        symlink(root.join("secret"), root.join("proc/pressure/memory")).unwrap();
        let err = mem.read(&root.join("proc/pressure/memory"), 10).unwrap_err();
        assert!(err.raw_os_error().is_some(), "{err}");
        assert!(mem.file_meta(&root.join("proc/pressure/memory")).is_err());

        // Eine FIFO an der Stelle einer Datei: kein Blockieren, kein Lesen
        let fifo = root.join("proc/pressure/cpu");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let err = mem.read(&fifo, 10).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");

        // Ein Ordner statt einer Datei
        std::fs::create_dir_all(root.join("proc/pressure/io")).unwrap();
        assert!(mem.read(&root.join("proc/pressure/io"), 10).is_err());
    }

    #[test]
    fn directory_listing_is_sorted_and_reports_links() {
        let dir = TempDir::new("fs-list");
        let root = dir.path();
        std::fs::create_dir_all(root.join("proc/20")).unwrap();
        std::fs::create_dir_all(root.join("proc/3")).unwrap();
        std::fs::write(root.join("proc/meminfo"), "x").unwrap();
        symlink(root.join("proc/3"), root.join("proc/self")).unwrap();
        let top = fs_for(root, "top_procs");
        let entries = top.read_dir(&root.join("proc")).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["20", "3", "meminfo", "self"]);
        let selfe = entries.iter().find(|e| e.name == "self").unwrap();
        assert!(selfe.symlink && selfe.is_dir);
        let mem = entries.iter().find(|e| e.name == "meminfo").unwrap();
        assert!(mem.is_file && !mem.is_dir && !mem.symlink);
        assert!(top.read_dir(&root.join("etc")).is_err());
        assert!(top.read_dir(&root.join("proc/20/fd")).is_err(), "fd is not an ancestor of a declared file");
    }

    #[test]
    fn paths_must_be_absolute_and_plain() {
        assert!(ProbePaths::rooted("/".into(), "/home/u".into(), Some("/t".into())).is_ok());
        assert!(ProbePaths::rooted("/".into(), "/home/u".into(), None).is_ok(), "no temperature logger");
        assert!(ProbePaths::rooted("rel".into(), "/home/u".into(), Some("/t".into())).is_err());
        assert!(ProbePaths::rooted("/".into(), "/home/../root".into(), Some("/t".into())).is_err());
        assert!(ProbePaths::rooted("/".into(), "/home/u".into(), Some("t".into())).is_err());
        let p = ProbePaths::rooted("/fx".into(), "/fx/home".into(), Some("/fx/t".into())).unwrap();
        assert_eq!(p.under_root("/proc/meminfo"), Path::new("/fx/proc/meminfo"));
        assert_eq!(p.under_root("var/log/pacman.log"), Path::new("/fx/var/log/pacman.log"));
    }

    #[test]
    fn every_read_pattern_of_the_catalog_compiles_to_something() {
        let p = paths(Path::new("/fx"));
        let mut count = 0;
        for spec in catalog::all() {
            let fs = Fs::for_spec(&p, spec);
            if let Exec::Read(list) = spec.exec {
                assert_eq!(fs.patterns.len(), list.len(), "{}", spec.name);
                count += list.len();
                for pattern in fs.patterns.iter() {
                    assert!(pattern.len() >= 2, "{}", spec.name);
                }
            } else {
                assert!(fs.patterns.is_empty(), "{}", spec.name);
            }
        }
        assert!(count >= 12);
    }
}
