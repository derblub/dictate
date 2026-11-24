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

//! Katalog bekannter Modelle und Liste der installierten Dateien. Heruntergeladen wird im Einstellungsfenster
//! (Fortschritt, SHA-256-Prüfung), der Daemon liefert hier nur die Daten für `list_models`.

use serde_json::{json, Value};
use std::path::PathBuf;

/// Ein Modell, das sich über die Einstellungen herunterladen lässt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: &'static str,
    /// Dateiname im Modellverzeichnis
    pub file: &'static str,
    pub url: &'static str,
    /// SHA-256 der Datei (bei Hugging Face die LFS-`oid`); `None` = unbekannt, dann nur die Größe prüfen
    pub sha256: Option<&'static str>,
    /// Größe in Bytes
    pub size: u64,
    /// Kurzbeschreibung (Englisch, für die Anzeige)
    pub note: &'static str,
}

/// Bekannte Modelle (Stand 22.10.2026; Größen und Prüfsummen stammen von Hugging Face).
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "large-v3-turbo-q5_0",
        file: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: Some("394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
        size: 574_041_195,
        note: "Default. Whisper large-v3-turbo, 5-bit quantized: fast and accurate, about 1.2 GB of VRAM.",
    },
    CatalogEntry {
        id: "large-v3-turbo",
        file: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha256: Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        size: 1_624_555_275,
        note: "Whisper large-v3-turbo, full precision: slightly more accurate, about 2.5 GB of VRAM.",
    },
    CatalogEntry {
        id: "small",
        file: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha256: Some("1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        size: 487_601_967,
        note: "Whisper small: for weak GPUs or CPU-only use, noticeably less accurate.",
    },
    CatalogEntry {
        id: "medium-q5_0",
        file: "ggml-medium-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin",
        sha256: Some("19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f"),
        size: 539_212_467,
        note: "Whisper medium, 5-bit quantized: middle ground between small and large.",
    },
    CatalogEntry {
        id: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
        sha256: Some("29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf"),
        size: 885_098,
        note: "Silero voice activity detection (needed next to every Whisper model).",
    },
];

/// Modell aus dem Katalog nach Id.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Verzeichnis der Modelle: `~/.local/share/dictate/models`.
pub fn models_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("dictate/models")
}

/// Eine installierte Modelldatei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub file: String,
    pub size: u64,
}

/// Dateien `*.bin` in `dir`, nach Namen sortiert; ein fehlendes Verzeichnis ergibt eine leere Liste.
pub fn installed_in(dir: &std::path::Path) -> Vec<Installed> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Installed> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
        .filter_map(|e| {
            let md = e.metadata().ok().filter(|m| m.is_file())?;
            Some(Installed { file: e.file_name().to_string_lossy().into_owned(), size: md.len() })
        })
        .collect();
    v.sort_by(|a, b| a.file.cmp(&b.file));
    v
}

pub fn installed() -> Vec<Installed> {
    installed_in(&models_dir())
}

/// Antwortdaten für `list_models` (die Dateien kommen aus `dir`).
pub fn list_json_in(dir: &std::path::Path) -> Value {
    let installed: Vec<Value> = installed_in(dir).into_iter().map(|i| json!({"file": i.file, "size": i.size})).collect();
    let catalog: Vec<Value> =
        CATALOG.iter().map(|e| json!({"id": e.id, "file": e.file, "url": e.url, "sha256": e.sha256, "size": e.size, "note": e.note})).collect();
    json!({"installed": installed, "catalog": catalog, "dir": dir.display().to_string()})
}

pub fn list_json() -> Value {
    list_json_in(&models_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        assert!(CATALOG.len() >= 5);
        for e in CATALOG {
            assert!(e.url.starts_with("https://huggingface.co/"), "{}", e.id);
            assert!(e.url.ends_with(e.file), "{}", e.id);
            assert!(e.file.ends_with(".bin") && !e.file.contains('/'));
            assert!(e.size > 0);
            if let Some(h) = e.sha256 {
                assert_eq!(h.len(), 64);
                assert!(h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            }
        }
        let mut ids: Vec<_> = CATALOG.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert_eq!(find("small").unwrap().file, "ggml-small.bin");
        assert!(find("nope").is_none());
        // Die Defaults der Konfiguration zeigen auf Katalogdateien
        let c = crate::config::Config::default();
        assert!(c.model.ends_with(find("large-v3-turbo-q5_0").unwrap().file));
        assert!(c.vad_model.ends_with(find("silero-v5.1.2").unwrap().file));
    }

    #[test]
    fn installed_lists_only_bin_files() {
        let d = std::env::temp_dir().join(format!("dictate-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub.bin")).unwrap();
        std::fs::write(d.join("b.bin"), [0u8; 10]).unwrap();
        std::fs::write(d.join("a.bin"), [0u8; 3]).unwrap();
        std::fs::write(d.join("note.txt"), "x").unwrap();
        assert_eq!(installed_in(&d), vec![Installed { file: "a.bin".into(), size: 3 }, Installed { file: "b.bin".into(), size: 10 }]);
        let j = list_json_in(&d);
        assert_eq!(j["installed"][1]["file"], "b.bin");
        assert_eq!(j["catalog"][0]["id"], "large-v3-turbo-q5_0");
        assert!(j["catalog"][0]["sha256"].is_string());
        assert!(installed_in(&d.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
