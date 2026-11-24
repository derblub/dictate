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

//! Hilfen für die Tests der Proben (nur mit `cfg(test)`): Temp-Ordner, Fake-Programme und ein Fixture-System.
//!
//! **Keine Probe läuft im Test gegen das echte System.** [`World`] baut einen Ordner mit einem leeren Dateisystem
//! (`proc`, `sys`, Home, Ordner des temp-loggers) und einem leeren Ordner für Programme; der Kontext zeigt nur dorthin
//! (`ProbeEnv::with_bin_dir`, `ProbePaths::rooted`, feste Uhr). Ein Programm ohne Fake gibt es nicht (`NotFound`). Die
//! Daten aller Fixtures sind erfunden.

use super::collect::{CollectOptions, Collector, PingTargetCfg, ProbeContext};
use super::output::ProbeOutput;
use super::time::{days_from_civil, Clock, DAY};
use super::ProbePaths;
use crate::diagnose::argv::Program;
use crate::diagnose::probe::parse_validated;
use crate::diagnose::run::ProbeEnv;
use crate::pipeline::ai::CancelFlag;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Eigener Temp-Ordner je Test, beim Drop gelöscht.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("dictate-diag-probes-{}-{tag}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Verzeichnisse ohne Rechte (Test „nicht lesbar“) vorher freigeben
        fn open_up(p: &Path) {
            if let Ok(rd) = fs::read_dir(p) {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
                for e in rd.flatten() {
                    let path = e.path();
                    if path.is_dir() && !path.is_symlink() {
                        open_up(&path);
                    }
                }
            }
        }
        open_up(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Eine Antwort eines Fake-Programms: gilt, wenn die Argumente (mit Leerzeichen verbunden) `when` enthalten.
pub struct Arm {
    pub when: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: i32,
}

/// Antwort auf Aufrufe, deren Argumente `when` enthalten (leer: jeder Aufruf).
pub fn arm(when: &str, stdout: impl AsRef<[u8]>) -> Arm {
    Arm { when: when.to_string(), stdout: stdout.as_ref().to_vec(), stderr: Vec::new(), exit: 0 }
}

impl Arm {
    pub fn err(mut self, stderr: &str) -> Arm {
        self.stderr = stderr.as_bytes().to_vec();
        self
    }

    pub fn exit(mut self, code: i32) -> Arm {
        self.exit = code;
        self
    }
}

/// Jetzt in den Fixtures: 2026-11-05 12:00:00 (+0100).
pub const NOW_OFFSET: i32 = 3600;

pub fn now() -> i64 {
    days_from_civil(2026, 11, 5) * DAY + 11 * 3600
}

/// Sekunden seit der Epoche zu einer Ortszeit (+0100) in den Fixtures.
pub fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64, s: i64) -> i64 {
    days_from_civil(y, mo, d) * DAY + h * 3600 + mi * 60 + s - i64::from(NOW_OFFSET)
}

/// Mikrosekunden wie im JSON von `journalctl --list-boots`.
pub fn us(secs: i64) -> i64 {
    secs * 1_000_000
}

/// Die Boots der Fixtures: -2 (sauber), -1 (Absturz am 04.11. gegen 09:47), 0 (läuft). Zeiten in Ortszeit +0100.
pub fn boots_json() -> String {
    let b2 = (at(2026, 11, 2, 8, 0, 0), at(2026, 11, 3, 23, 30, 0));
    let b1 = (at(2026, 11, 4, 8, 12, 1), at(2026, 11, 4, 9, 46, 55));
    let b0 = (at(2026, 11, 4, 10, 5, 30), now() - 1);
    format!(
        r#"[{{"index":-2,"boot_id":"2e6ab35cf56744e99fb1a6d2a52a1111","first_entry":{},"last_entry":{}}},{{"index":-1,"boot_id":"7d04a9e4c1b94c0f8c1e5b3e0f6c2222","first_entry":{},"last_entry":{}}},{{"index":0,"boot_id":"c9b1d2e3f4a5465798a1b2c3d4e53333","first_entry":{},"last_entry":{}}}]"#,
        us(b2.0),
        us(b2.1),
        us(b1.0),
        us(b1.1),
        us(b0.0),
        us(b0.1)
    )
}

/// Ein Fixture-System für eine Probe.
pub struct World {
    dir: TempDir,
    pub ctx: ProbeContext,
    pub cancel: CancelFlag,
}

impl World {
    pub fn new(tag: &str) -> World {
        let dir = TempDir::new(tag);
        let root = dir.path().join("root");
        let bin = dir.path().join("bin");
        for sub in ["root/proc", "root/sys", "root/var/lib/systemd/pstore", "root/var/log", "root/home/u", "root/templog", "bin"] {
            fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        let env = ProbeEnv::system()
            .with_bin_dir(&bin)
            .unwrap()
            .with_home(Some(root.join("home/u")))
            .unwrap()
            .with_runtime_dir(Some(dir.path().join("run")))
            .unwrap();
        let paths = ProbePaths::rooted(root.clone(), root.join("home/u"), Some(root.join("templog"))).unwrap();
        let ctx = ProbeContext {
            env,
            paths,
            clock: Clock::fixed(now(), NOW_OFFSET),
            ping_targets: vec![
                PingTargetCfg { name: "gateway".into(), address: "192.0.2.1".into() },
                PingTargetCfg { name: "nas".into(), address: "nas.example.org".into() },
                PingTargetCfg { name: "1.1.1.1".into(), address: "1.1.1.1".into() },
            ],
            sample_interval: Duration::from_millis(20),
        };
        World { dir, ctx, cancel: Arc::new(AtomicBool::new(false)) }
    }

    pub fn base(&self) -> &Path {
        self.dir.path()
    }

    pub fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    /// Legt ein Fake-Programm an: das erste passende `arms`-Element antwortet, kein passendes ergibt Exit-Code 99.
    pub fn fake(&self, program: Program, arms: &[Arm]) {
        let bin = self.dir.path().join("bin");
        let mut script = String::from("#!/bin/sh\nargs=\"$*\"\ncase \"$args\" in\n");
        for (i, a) in arms.iter().enumerate() {
            let out = bin.join(format!("{}.{i}.out", program.name()));
            let err = bin.join(format!("{}.{i}.err", program.name()));
            fs::write(&out, &a.stdout).unwrap();
            fs::write(&err, &a.stderr).unwrap();
            let pattern = if a.when.is_empty() { "*".to_string() } else { format!("*\"{}\"*", a.when) };
            script.push_str(&format!("  {pattern}) cat '{}'; cat '{}' >&2; exit {};;\n", out.display(), err.display(), a.exit));
        }
        script.push_str("esac\necho \"fake ");
        script.push_str(program.name());
        script.push_str(": no answer for: $args\" >&2\nexit 99\n");
        let path = bin.join(program.name());
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Legt ein Fake-Programm mit eigenem Skript an (für Programme, die hängen, endlos schreiben oder auf Signale achten).
    pub fn script(&self, program: Program, body: &str) {
        let path = self.dir.path().join("bin").join(program.name());
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Schreibt eine Datei unter der Fixture-Wurzel (`proc/meminfo` ist `/proc/meminfo`).
    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// Führt eine Probe (als JSON) mit frischem Collector aus; Budget 30 s, Netzverkehr bestätigt.
    pub fn collect(&self, json: &str) -> ProbeOutput {
        let mut collector = Collector::new(&self.ctx, &self.cancel);
        self.collect_with(&mut collector, json)
    }

    pub fn collect_with(&self, collector: &mut Collector, json: &str) -> ProbeOutput {
        let probe = parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"));
        collector.collect(&probe, CollectOptions::new(Duration::from_secs(30)).confirmed())
    }
}

/// Zeilen mit Zeitstempel, Rechnername und Kennung im Format von `journalctl -o short-iso`, Ortszeit +0100.
pub fn journal_line(ts_local: &str, ident: &str, msg: &str) -> String {
    format!("{ts_local}+0100 blub {ident}: {msg}")
}
