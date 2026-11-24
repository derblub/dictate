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

//! Lizenzköpfe im ganzen Repository (PLAN-lizenz.md, U1; Entscheidungen Daniels vom 23.11.2026): jede `.rs`-Datei
//! beginnt mit dem vollen Kopf (Copyright, GPL-3.0-or-later, Zusatzerlaubnis nach GPLv3 Abschnitt 7 für NVIDIA CUDA,
//! Mustertext der GPL-FAQ `#GPLIncompatibleLibs`), jedes Shell- und Python-Skript (auch ohne Endung, erkannt am
//! Shebang) trägt nach dem Shebang den Kurzkopf. Durchlaufen wird das Arbeitsverzeichnis ohne `target/` und versteckte
//! Ordner, also auch Spikes, das verschachtelte Hilfs-Crate und neue, noch nicht versionierte Dateien. Ein neuer Kopf
//! (anderes Jahr, andere Bibliothek) ist eine bewusste Änderung an diesem Test und an allen Dateien zugleich.

use std::fs;
use std::path::{Path, PathBuf};

/// Kopf jeder `.rs`-Datei, byte-genau; danach folgt eine Leerzeile und der bisherige Inhalt (meist `//!`).
const RS_HEADER: &str = "\
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

";

/// Kurzkopf der Skripte, direkt nach dem Shebang (oder in Zeile 1, wenn das Skript nur eingebunden wird).
const SCRIPT_HEADER: &str = "\
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
";

#[derive(Default)]
struct Found {
    rust: Vec<PathBuf>,
    scripts: Vec<PathBuf>,
}

fn walk(dir: &Path, found: &mut Found) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())).map(|e| e.unwrap()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            if name != "target" && !name.starts_with('.') {
                walk(&path, found);
            }
        } else if kind.is_file() {
            if name.ends_with(".rs") {
                found.rust.push(path);
            } else if name.ends_with(".sh") || name.ends_with(".py") || starts_with_shebang(&path) {
                found.scripts.push(path);
            }
        }
    }
}

/// Shebang mit Pfad (`#!/`), nicht ein inneres Attribut `#![…]`.
fn starts_with_shebang(path: &Path) -> bool {
    let mut head = [0u8; 3];
    match fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)) {
        Ok(3) => &head == b"#!/",
        _ => false,
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn found() -> Found {
    let mut found = Found::default();
    walk(&root(), &mut found);
    found
}

fn rel(path: &Path) -> String {
    path.strip_prefix(root()).unwrap_or(path).display().to_string()
}

/// Ohne Treffer bewiese der Test nichts: Mindestzahl und bekannte Dateien je Bereich.
fn assert_covers(paths: &[PathBuf], min: usize, known: &[&str], what: &str) {
    assert!(paths.len() >= min, "only {} {what} found (expected at least {min}); does the walk still see the tree?", paths.len());
    for k in known {
        assert!(paths.iter().any(|p| rel(p) == *k), "{what}: {k} not found by the walk");
    }
}

#[test]
fn every_rust_file_carries_the_license_header() {
    let found = found();
    assert_covers(
        &found.rust,
        170,
        &["src/lib.rs", "src/bin/dictate-ui/main.rs", "spikes/wayland-gtk/src/main.rs", "tests/wayland/nested/src/main.rs", "tests/license_headers.rs"],
        ".rs files",
    );
    let missing: Vec<String> = found
        .rust
        .iter()
        .filter(|p| !fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).starts_with(RS_HEADER))
        .map(|p| rel(p))
        .collect();
    assert!(missing.is_empty(), "{} .rs files without the license header (copy it from tests/license_headers.rs):\n{}", missing.len(), missing.join("\n"));
}

#[test]
fn every_script_carries_the_short_header() {
    let found = found();
    assert_covers(&found.scripts, 29, &["scripts/check.sh", "tests/wayland/fake-logind", "tests/gui/lib.sh", "tests/gui/mock_daemon.py"], "scripts");
    let missing: Vec<String> = found
        .scripts
        .iter()
        .filter(|p| {
            let text = fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let body = if text.starts_with("#!/") { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
            !body.starts_with(SCRIPT_HEADER)
        })
        .map(|p| rel(p))
        .collect();
    assert!(missing.is_empty(), "{} scripts without the short license header (after the shebang):\n{}", missing.len(), missing.join("\n"));
}
