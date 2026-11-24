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

//! Verlauf: eine JSON-Zeile je Diktat (`ts`, `app`, `raw`, `text`) in `$XDG_STATE_HOME/dictate/history.jsonl`
//! (Standard `~/.local/state/dictate/history.jsonl`), Datei 0600, Verzeichnis 0700, gekürzt auf `max_entries`.
//!
//! Geschrieben wird nur, wenn `features.history` an ist; das prüft der Aufrufer.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Standardpfad nach XDG.
pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dictate/history.jsonl")
}

/// Zeitstempel in UTC, RFC 3339 mit Sekunden (`2026-10-22T18:04:05Z`).
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Tage seit 1970-01-01 → Datum (Howard Hinnant, civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Zähler „noch nicht gezählt“
const UNKNOWN: usize = usize::MAX;

pub struct History {
    path: PathBuf,
    /// Anzahl der Einträge in der Datei; gekürzt wird nur, wenn sie über `max` liegt (statt bei jedem Diktat die
    /// ganze Datei zu lesen). Beim ersten Anhängen einmal gezählt.
    count: AtomicUsize,
}

impl History {
    pub fn new(path: PathBuf) -> History {
        History { path, count: AtomicUsize::new(UNKNOWN) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Hängt einen Eintrag an und kürzt die Datei auf die letzten `max` Einträge.
    pub fn append(&self, app: Option<&str>, raw: &str, text: &str, max: usize) -> Result<()> {
        let line = json!({"ts": timestamp(SystemTime::now()), "app": app.unwrap_or(""), "raw": raw, "text": text}).to_string();
        if let Some(dir) = self.path.parent() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut f =
            std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path).with_context(|| format!("opening {}", self.path.display()))?;
        // eine schon vorhandene Datei mit weiteren Rechten wird enger gestellt
        f.set_permissions(std::fs::Permissions::from_mode(0o600)).with_context(|| format!("setting permissions of {}", self.path.display()))?;
        let known = self.count.load(Ordering::Relaxed);
        // Beim ersten Mal die vorhandenen Einträge zählen (vor dem Anhängen)
        let before = if known == UNKNOWN { self.count_lines() } else { known };
        writeln!(f, "{line}")?;
        drop(f);
        let now = before + 1;
        let max = max.max(1);
        if now > max {
            self.truncate(max)?;
            self.count.store(max, Ordering::Relaxed);
        } else {
            self.count.store(now, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Nicht leere Zeilen der Datei (fehlt sie, 0).
    fn count_lines(&self) -> usize {
        std::fs::read_to_string(&self.path).map(|s| s.lines().filter(|l| !l.trim().is_empty()).count()).unwrap_or(0)
    }

    /// Behält die letzten `max` Zeilen (atomar über eine temporäre Datei).
    fn truncate(&self, max: usize) -> Result<()> {
        let s = std::fs::read_to_string(&self.path)?;
        let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() <= max {
            return Ok(());
        }
        let keep = &lines[lines.len() - max..];
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&tmp)?;
            for l in keep {
                writeln!(f, "{l}")?;
            }
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Die letzten `limit` Einträge, neueste zuerst. Fehlt die Datei, ist die Liste leer; kaputte Zeilen werden
    /// übersprungen.
    pub fn entries(&self, limit: usize) -> Vec<Value> {
        let Ok(s) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        s.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(Value::is_object).take(limit).collect()
    }

    /// Löscht die Datei (fehlt sie, ist das kein Fehler).
    pub fn clear(&self) -> Result<()> {
        self.count.store(UNKNOWN, Ordering::Relaxed);
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("removing {}", self.path.display())),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("dictate-hist-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn append_list_truncate_clear() {
        let d = temp();
        let h = History::new(d.join("state/dictate/history.jsonl"));
        assert!(h.entries(10).is_empty());
        for i in 0..5 {
            h.append(Some("kitty"), &format!("raw {i}"), &format!("text {i}"), 3).unwrap();
        }
        let e = h.entries(10);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0]["text"], "text 4");
        assert_eq!(e[2]["raw"], "raw 2");
        assert_eq!(e[0]["app"], "kitty");
        assert_eq!(h.entries(1).len(), 1);
        let mode = std::fs::metadata(h.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(h.path().parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700);
        h.clear().unwrap();
        assert!(!h.path().exists());
        h.clear().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn truncates_only_when_over_the_limit() {
        let d = temp();
        let h = History::new(d.join("h/history.jsonl"));
        for i in 0..3 {
            h.append(None, "r", &format!("t{i}"), 3).unwrap();
        }
        let big = std::fs::read_to_string(h.path()).unwrap();
        assert_eq!(big.lines().count(), 3);
        // Datei von außen verlängern: solange der Zähler nicht über max liegt, wird nicht angefasst
        let ino = std::fs::metadata(h.path()).unwrap().ino();
        h.append(None, "r", "t3", 5).unwrap();
        assert_eq!(std::fs::metadata(h.path()).unwrap().ino(), ino, "append without truncation keeps the same file");
        assert_eq!(h.entries(100).len(), 4);
        // über dem Limit: auf max kürzen, danach wieder ohne Neuschreiben anhängen
        h.append(None, "r", "t4", 5).unwrap();
        h.append(None, "r", "t5", 5).unwrap();
        let e = h.entries(100);
        assert_eq!((e.len(), e[0]["text"].as_str()), (5, Some("t5")));
        let ino = std::fs::metadata(h.path()).unwrap().ino();
        h.append(None, "r", "t6", 6).unwrap();
        assert_eq!(std::fs::metadata(h.path()).unwrap().ino(), ino);
        // clear setzt den Zähler zurück
        h.clear().unwrap();
        h.append(None, "r", "n0", 2).unwrap();
        assert_eq!(h.entries(100).len(), 1);
        // eine bestehende Datei wird beim ersten Anhängen mitgezählt
        let h2 = History::new(h.path().to_path_buf());
        h2.append(None, "r", "n1", 2).unwrap();
        h2.append(None, "r", "n2", 2).unwrap();
        let e = h2.entries(100);
        assert_eq!((e.len(), e[0]["text"].as_str(), e[1]["text"].as_str()), (2, Some("n2"), Some("n1")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn opening_tightens_the_mode_to_0600() {
        let d = temp();
        let h = History::new(d.join("h/history.jsonl"));
        std::fs::create_dir_all(h.path().parent().unwrap()).unwrap();
        std::fs::write(h.path(), "").unwrap();
        std::fs::set_permissions(h.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        h.append(None, "r", "t", 5).unwrap();
        assert_eq!(std::fs::metadata(h.path()).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_792_692_245)), "2026-10-22T18:04:05Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
    }
}
