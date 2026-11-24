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

//! Testordner der Integrationstests und von `src/diagnose/live_eval.rs` (`#[path]`): dieselbe Regel wie
//! `scripts/test-root.sh` (PLAN-lizenz.md, O7 und G3). `DICTATE_TEST_ROOT`, sonst
//! `${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test` (ein relatives `XDG_CACHE_HOME` zählt nach der XDG-Spezifikation
//! nicht); nur absolute Pfade ohne leere, `.`- und `..`-Teile aus `A-Z a-z 0-9 . _ - /`, ein Schrägstrich am Ende
//! entfällt. Auf blub zeigt die Variable auf `/home/data/stt-test`. Darunter liegen `audio/` (Testaudio, nicht im
//! Repo) und `logs/`. Die Auflösung ist eine reine Funktion der drei Werte (Tests setzen keine Umgebung).

#![allow(dead_code)]

use std::path::PathBuf;

/// Aufgelöster Testordner des Prozesses; bei einem untauglichen Wert Abbruch mit Meldung.
pub fn root() -> PathBuf {
    let get = |k: &str| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned());
    resolve(get("DICTATE_TEST_ROOT").as_deref(), get("XDG_CACHE_HOME").as_deref(), get("HOME").as_deref()).unwrap_or_else(|e| panic!("{e}"))
}

/// Datei unter `<root>/audio/`.
pub fn audio(rel: &str) -> PathBuf {
    root().join("audio").join(rel)
}

/// Zusatz für Meldungen, wenn Testaudio fehlt.
pub const AUDIO_HINT: &str = "test recordings are not in the repository; DICTATE_TEST_ROOT points to the test folder (see CONTRIBUTING.md)";

/// Regel ohne Umgebung: `root`, `xdg_cache_home`, `home` wie gelesen (`None` = nicht gesetzt).
pub fn resolve(root: Option<&str>, xdg_cache_home: Option<&str>, home: Option<&str>) -> Result<PathBuf, String> {
    let mut root = match root.filter(|r| !r.is_empty()) {
        Some(r) => r.to_string(),
        None => {
            let cache = match xdg_cache_home.filter(|c| c.starts_with('/')) {
                Some(c) => c.to_string(),
                None => match home.filter(|h| h.starts_with('/')) {
                    Some(h) => format!("{h}/.cache"),
                    None => return Err("DICTATE_TEST_ROOT is not set and HOME is not an absolute path; set DICTATE_TEST_ROOT.".into()),
                },
            };
            format!("{cache}/dictate-test")
        }
    };
    while root.len() > 1 && root.ends_with('/') {
        root.pop();
    }
    let Some(rest) = root.strip_prefix('/').filter(|r| !r.is_empty()) else {
        return Err(format!("DICTATE_TEST_ROOT must be an absolute path below / (got {root:?})."));
    };
    if !root.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/')) {
        return Err(format!("DICTATE_TEST_ROOT may only contain A-Z a-z 0-9 . _ - / (got {root:?}); set it to such a folder."));
    }
    if rest.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return Err(format!("DICTATE_TEST_ROOT must not contain empty, '.' or '..' parts (got {root:?})."));
    }
    Ok(PathBuf::from(root))
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::path::PathBuf;

    #[test]
    fn variable_wins_and_loses_a_trailing_slash() {
        assert_eq!(resolve(Some("/home/data/stt-test/"), Some("/c"), Some("/h")), Ok(PathBuf::from("/home/data/stt-test")));
        assert_eq!(resolve(Some("/srv/t"), None, None), Ok(PathBuf::from("/srv/t")));
    }

    #[test]
    fn default_under_the_cache_folder() {
        assert_eq!(resolve(None, Some("/c"), Some("/home/alice")), Ok(PathBuf::from("/c/dictate-test")));
        assert_eq!(resolve(Some(""), None, Some("/home/alice")), Ok(PathBuf::from("/home/alice/.cache/dictate-test")));
        // Relatives XDG_CACHE_HOME zählt nicht
        assert_eq!(resolve(None, Some("rel"), Some("/home/alice")), Ok(PathBuf::from("/home/alice/.cache/dictate-test")));
        assert!(resolve(None, None, None).is_err());
        assert!(resolve(None, Some("rel"), Some("")).is_err());
    }

    #[test]
    fn unsafe_roots_are_refused() {
        for r in ["rel/x", "/", "//", "/a/../b", "/a/./b", "/a//b", "/a/..", "/a b", "/ä", "/x+y", "/x*"] {
            assert!(resolve(Some(r), None, Some("/home/alice")).is_err(), "{r}");
        }
    }
}
