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

//! Projekte: Git-Repositories unter konfigurierbaren Wurzeln.
//!
//! Ein Ordner ist ein Projekt, wenn er `.git` enthält: als Ordner oder als Datei, die mit `gitdir:` beginnt (Worktrees
//! und Submodule). In ein gefundenes Repository wird nicht abgestiegen. Gesucht wird bis zur Tiefe
//! [`ProjectScanConfig::max_depth`] unter der Wurzel (Vorgabe 2: `~/src/dictate` und `~/work/client/app` bei der
//! Vorgabewurzel `~`, dem Home-Verzeichnis; Tiefe 1 sind die Ordner direkt unter der Wurzel).
//! Nicht betreten werden versteckte Ordner (Name beginnt mit `.`, das schließt `.claude/worktrees` ein), `node_modules`,
//! `target` und alles weitere aus [`ProjectScanConfig::exclude`]; Symlinks (auf Ordner und `.git`) werden nicht
//! verfolgt. Ordnernamen, die kein UTF-8 sind, werden übergangen (sie ließen sich nicht als JSON schicken).
//!
//! `group` ist der Name der Ebene unmittelbar unter der Wurzel, in der das Projekt liegt (`src` für `~/src/dictate`);
//! Projekte direkt unter der Wurzel haben keine Gruppe.

use super::{Fingerprint, Probe, ProbeKind};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Vorgabe für die Wurzel: das Home-Verzeichnis (wie `palette.project_roots`, [`crate::config::PALETTE_DEFAULT_ROOT`]).
pub const DEFAULT_ROOT: &str = "~";
/// Vorgabe für die Tiefe.
pub const DEFAULT_MAX_DEPTH: usize = 2;
/// Vorgabe für die ausgeschlossenen Ordnernamen (zusätzlich zu allen versteckten).
pub const DEFAULT_EXCLUDE: [&str; 2] = ["node_modules", "target"];
/// Obergrenze für die gelesenen Ordner eines Durchlaufs (Schutz vor einer Wurzel wie `/`).
const MAX_DIRS: usize = 20_000;

/// Einstellungen der Projektsuche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScanConfig {
    /// Wurzeln (absolute Pfade; andere werden übergangen).
    pub roots: Vec<PathBuf>,
    /// Tiefste durchsuchte Ebene unter der Wurzel (1 = direkt unter der Wurzel).
    pub max_depth: usize,
    /// Ordnernamen, die nie betreten werden (versteckte Ordner immer).
    pub exclude: Vec<String>,
}

impl Default for ProjectScanConfig {
    fn default() -> ProjectScanConfig {
        ProjectScanConfig {
            roots: vec![crate::config::expand_tilde(DEFAULT_ROOT)],
            max_depth: DEFAULT_MAX_DEPTH,
            exclude: DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Ein Git-Projekt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Absoluter Pfad des Ordners; zugleich die Ziel-ID der Häufigkeit.
    pub path: PathBuf,
    /// Ordnername.
    pub name: String,
    /// Ebene unter der Wurzel (siehe Modulkopf); `None` direkt unter der Wurzel.
    pub group: Option<String>,
}

impl Project {
    /// Ziel-ID für die Häufigkeit (`open_project:<Pfad>`): der Pfad, immer UTF-8.
    pub fn target_id(&self) -> &str {
        self.path.to_str().unwrap_or_default()
    }
}

/// Ergebnis eines Durchlaufs.
#[derive(Debug, Clone)]
pub struct ProjectScan {
    /// Nach Pfad sortiert, jeder Pfad einmal (auch bei überlappenden Wurzeln).
    pub projects: Vec<Project>,
    /// Was der Durchlauf angesehen hat (für den Cache).
    pub fingerprint: Fingerprint,
    /// `true`, wenn die Obergrenze für Ordner erreicht wurde und die Liste unvollständig ist.
    pub truncated: bool,
}

struct Scan<'a> {
    cfg: &'a ProjectScanConfig,
    projects: Vec<Project>,
    seen: HashSet<PathBuf>,
    fingerprint: Fingerprint,
    dirs: usize,
    truncated: bool,
}

impl Scan<'_> {
    fn visit(&mut self, dir: &Path, depth: usize, group: Option<&str>) {
        if self.dirs >= MAX_DIRS {
            self.truncated = true;
            return;
        }
        self.dirs += 1;
        // Erst messen, dann entscheiden (siehe `Fingerprint`): ein Repository wird über die Existenz von `.git` beobachtet,
        // nicht über die Änderungszeit des Ordners oder von `.git`, die sich bei jeder Arbeit darin ändern; jeder andere
        // Ordner über seine Änderungszeit (Einträge hinzugekommen, darunter ein neues `.git`).
        let git = dir.join(".git");
        let presence = Probe::take(&git, ProbeKind::Presence);
        let mtime = Probe::take(dir, ProbeKind::Mtime);
        if is_git_marker(&git) {
            self.fingerprint.push(presence);
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                if self.seen.insert(dir.to_path_buf()) {
                    self.projects.push(Project { path: dir.to_path_buf(), name: name.to_string(), group: group.map(str::to_string) });
                }
            }
            return;
        }
        self.fingerprint.push(mtime);
        if depth >= self.cfg.max_depth {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut children: Vec<(String, PathBuf)> = read
            .filter_map(Result::ok)
            // `file_type` folgt keinen Symlinks: ein Link auf einen Ordner ist kein Ordner
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
            .filter(|(name, _)| !name.starts_with('.') && !self.cfg.exclude.iter().any(|x| x == name))
            .collect();
        children.sort();
        // Gruppe der Kinder: unter der Wurzel keine, eine Ebene tiefer der Name dieses Ordners, darunter dieselbe
        let child_group = match depth {
            0 => None,
            1 => dir.file_name().and_then(|n| n.to_str()),
            _ => group,
        };
        for (_, path) in children {
            self.visit(&path, depth + 1, child_group);
        }
    }
}

/// `.git` ist ein Ordner oder eine Datei, die mit `gitdir:` beginnt; ein Symlink zählt nicht.
fn is_git_marker(git: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(git) else { return false };
    if meta.is_dir() {
        return true;
    }
    if !meta.is_file() {
        return false;
    }
    let mut head = Vec::with_capacity(8);
    std::fs::File::open(git).and_then(|f| f.take(7).read_to_end(&mut head)).is_ok() && head == b"gitdir:"
}

/// Sucht die Projekte (siehe Modulkopf). Fehlende oder unlesbare Ordner werden übergangen.
pub fn scan_projects(cfg: &ProjectScanConfig) -> ProjectScan {
    let mut scan = Scan { cfg, projects: Vec::new(), seen: HashSet::new(), fingerprint: Fingerprint::default(), dirs: 0, truncated: false };
    for root in &cfg.roots {
        if !root.is_absolute() {
            tracing::warn!("palette: project root {} is not absolute, skipped", root.display());
            continue;
        }
        scan.visit(root, 0, None);
    }
    if scan.truncated {
        tracing::warn!("palette: project search stopped after {MAX_DIRS} directories");
    }
    let mut projects = scan.projects;
    projects.sort_by(|a, b| a.path.cmp(&b.path));
    tracing::debug!("palette: {} projects", projects.len());
    ProjectScan { projects, fingerprint: scan.fingerprint, truncated: scan.truncated }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{set_mtime, TempDir};
    use super::*;

    fn cfg(t: &TempDir) -> ProjectScanConfig {
        ProjectScanConfig { roots: vec![t.path().to_path_buf()], ..ProjectScanConfig::default() }
    }

    /// `(Gruppe oder "", Name)` in Pfadreihenfolge.
    fn found(scan: &ProjectScan) -> Vec<(String, String)> {
        scan.projects.iter().map(|p| (p.group.clone().unwrap_or_default(), p.name.clone())).collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(g, n)| (g.to_string(), n.to_string())).collect()
    }

    #[test]
    fn finds_repositories_up_to_depth_two() {
        let t = TempDir::new("proj-depth");
        t.mkdir("blub/dictate/.git");
        t.mkdir("pushingpixels/dataroom/.git");
        t.mkdir("clientsite/.git");
        t.mkdir("blub/notarepo/src");
        t.mkdir("blub/deep/er/proj/.git");
        t.mkdir("plain/also/nothing");
        let scan = scan_projects(&cfg(&t));
        assert_eq!(found(&scan), pairs(&[("blub", "dictate"), ("", "clientsite"), ("pushingpixels", "dataroom")]));
        assert_eq!(scan.projects[0].path, t.join("blub/dictate"));
        assert_eq!(scan.projects[0].target_id(), t.join("blub/dictate").to_str().unwrap());
        assert!(!scan.truncated);

        // tiefere Suche: Ebene 4 (`blub/deep/er/proj`) bleibt draußen, Ebene 3 kommt dazu; die Gruppe bleibt die Ebene
        // unter der Wurzel
        let deeper = scan_projects(&ProjectScanConfig { max_depth: 3, ..cfg(&t) });
        assert_eq!(found(&deeper), pairs(&[("blub", "dictate"), ("", "clientsite"), ("pushingpixels", "dataroom")]));
        t.mkdir("blub/deep/proj3/.git");
        let deeper = scan_projects(&ProjectScanConfig { max_depth: 3, ..cfg(&t) });
        assert_eq!(found(&deeper), pairs(&[("blub", "proj3"), ("blub", "dictate"), ("", "clientsite"), ("pushingpixels", "dataroom")]));
        let shallow = scan_projects(&ProjectScanConfig { max_depth: 1, ..cfg(&t) });
        assert_eq!(found(&shallow), pairs(&[("", "clientsite")]));
    }

    #[test]
    fn git_may_be_a_file() {
        let t = TempDir::new("proj-gitfile");
        t.write("blub/worktree/.git", "gitdir: /home/git/blub/main/.git/worktrees/worktree\n");
        t.write("blub/submodule/.git", "gitdir: ../.git/modules/submodule");
        t.write("blub/junk/.git", "this is not a git file\n");
        t.write("blub/empty/.git", "");
        let scan = scan_projects(&cfg(&t));
        assert_eq!(found(&scan), pairs(&[("blub", "submodule"), ("blub", "worktree")]));
    }

    #[test]
    fn does_not_descend_into_repositories() {
        let t = TempDir::new("proj-nested");
        t.mkdir("blub/outer/.git");
        t.mkdir("blub/outer/vendor/inner/.git");
        t.mkdir("topgroup/.git");
        t.mkdir("topgroup/sub/.git");
        let scan = scan_projects(&ProjectScanConfig { max_depth: 4, ..cfg(&t) });
        assert_eq!(found(&scan), pairs(&[("blub", "outer"), ("", "topgroup")]));
    }

    #[test]
    fn excludes_hidden_node_modules_target_and_worktrees() {
        let t = TempDir::new("proj-exclude");
        t.mkdir("blub/real/.git");
        t.mkdir("blub/node_modules/pkg/.git");
        t.mkdir("blub/target/foo/.git");
        t.mkdir(".hidden/x/.git");
        t.mkdir("blub/.cache/y/.git");
        t.mkdir("blub/real2/.git");
        t.mkdir("blub/real2/.claude/worktrees/w1/.git");
        t.mkdir("node_modules/.git");
        t.mkdir("target/.git");
        t.mkdir("blub/.claude/worktrees/w2/.git");
        let scan = scan_projects(&ProjectScanConfig { max_depth: 5, ..cfg(&t) });
        assert_eq!(found(&scan), pairs(&[("blub", "real"), ("blub", "real2")]));

        // eigene Ausschlüsse ersetzen die Vorgabe
        let custom = scan_projects(&ProjectScanConfig { exclude: vec!["real".into(), "node_modules".into(), "target".into()], max_depth: 5, ..cfg(&t) });
        assert_eq!(found(&custom), pairs(&[("blub", "real2")]));
        let only_real = scan_projects(&ProjectScanConfig { exclude: vec!["real".into()], max_depth: 5, ..cfg(&t) });
        assert_eq!(found(&only_real), pairs(&[("blub", "pkg"), ("blub", "real2"), ("blub", "foo"), ("", "node_modules"), ("", "target")]));
    }

    #[test]
    fn symlinks_are_not_followed() {
        let t = TempDir::new("proj-links");
        let other = TempDir::new("proj-links-other");
        other.mkdir("repo/.git");
        t.mkdir("blub");
        std::os::unix::fs::symlink(other.join("repo"), t.join("blub/linked")).unwrap();
        std::os::unix::fs::symlink(other.path(), t.join("linkgroup")).unwrap();
        t.mkdir("blub/symgit");
        std::os::unix::fs::symlink(other.join("repo/.git"), t.join("blub/symgit/.git")).unwrap();
        t.mkdir("blub/real/.git");
        std::os::unix::fs::symlink(t.path(), t.join("blub/loop")).unwrap();
        let scan = scan_projects(&cfg(&t));
        assert_eq!(found(&scan), pairs(&[("blub", "real")]));
    }

    #[test]
    fn roots_may_be_repositories_overlap_and_be_missing() {
        let t = TempDir::new("proj-roots");
        t.mkdir("a/one/.git");
        t.mkdir("b/.git");
        t.mkdir("c/two/.git");
        let cfg = ProjectScanConfig {
            roots: vec![t.join("a"), t.join("b"), t.join("missing"), PathBuf::from("relative/root"), t.path().to_path_buf(), t.join("a")],
            ..ProjectScanConfig::default()
        };
        let scan = scan_projects(&cfg);
        // Wurzel `b` ist selbst ein Repository (ohne Gruppe); `a/one` wird von der ersten Wurzel gefunden (Ebene 1,
        // ohne Gruppe) und bei der überlappenden Wurzel `t` nicht noch einmal aufgenommen
        let got: Vec<(PathBuf, Option<String>)> = scan.projects.iter().map(|p| (p.path.clone(), p.group.clone())).collect();
        assert_eq!(got, vec![(t.join("a/one"), None), (t.join("b"), None), (t.join("c/two"), Some("c".to_string()))]);
        assert_eq!(scan.projects.iter().filter(|p| p.name == "one").count(), 1);
    }

    #[test]
    fn missing_root_is_observed_until_it_appears() {
        let t = TempDir::new("proj-missing");
        let cfg = ProjectScanConfig { roots: vec![t.join("later")], ..ProjectScanConfig::default() };
        let scan = scan_projects(&cfg);
        assert!(scan.projects.is_empty());
        assert!(scan.fingerprint.is_current());
        t.mkdir("later");
        assert!(!scan.fingerprint.is_current());
    }

    #[test]
    fn fingerprint_watches_dirs_for_new_entries_and_git_for_existence() {
        let t = TempDir::new("proj-fp");
        t.mkdir("g/p/.git");
        t.mkdir("g/q");
        let scan = scan_projects(&cfg(&t));
        assert!(scan.fingerprint.is_current());
        // Arbeit im Repository ändert nichts am Fingerabdruck
        t.write("g/p/file.txt", "x");
        t.write("g/p/.git/index.lock", "x");
        assert!(scan.fingerprint.is_current());
        // `.git` weg: Änderung
        std::fs::remove_dir_all(t.join("g/p/.git")).unwrap();
        assert!(!scan.fingerprint.is_current());
        // neues `.git` in einem Ordner ohne Repository: Änderung (über die Änderungszeit von `g/q`, die sich dabei ändert)
        let scan = scan_projects(&cfg(&t));
        assert!(scan.fingerprint.is_current());
        set_mtime(&t.join("g/q"), 1);
        let scan = scan_projects(&cfg(&t));
        t.mkdir("g/q/.git");
        set_mtime(&t.join("g/q"), 2);
        assert!(!scan.fingerprint.is_current());
    }

    #[test]
    fn unreadable_directories_are_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new("proj-perm");
        t.mkdir("blub/ok/.git");
        t.mkdir("secret/inner/.git");
        std::fs::set_permissions(t.join("secret"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let scan = scan_projects(&cfg(&t));
        std::fs::set_permissions(t.join("secret"), std::fs::Permissions::from_mode(0o755)).unwrap();
        // als root lässt sich der Ordner trotzdem lesen
        let as_root = unsafe { libc::geteuid() } == 0;
        let want = if as_root { pairs(&[("blub", "ok"), ("secret", "inner")]) } else { pairs(&[("blub", "ok")]) };
        assert_eq!(found(&scan), want);
    }
}
