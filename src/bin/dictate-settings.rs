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

//! `dictate-settings`: Einstellungsfenster für dictate (egui). Einzelinstanz per Lock-Datei.
//!
//! Die Konfigurationslogik (`config.rs`) wird direkt eingebunden, damit das Fenster keine Whisper-/CUDA-Bibliothek
//! mitlinkt.

#[allow(dead_code)]
#[path = "../actions/mod.rs"]
mod actions;
#[allow(dead_code)]
#[path = "../config.rs"]
mod config;
#[path = "../gui/mod.rs"]
mod gui;
/// Palette von Noctalia und Pixelwelle des Overlays aus derselben Quelle wie `dictate-ui` (Pfad, Parser, Rückfall,
/// Maße; PLAN-wayland.md 6h, Regel 2, Lesart L45): `#[path]` am eingebetteten Modul macht `src/ui/` zu seinem Ordner,
/// `palette.rs` findet `super::fsutil` dort
#[path = "../ui"]
mod ui {
    pub mod fsutil;
    #[allow(dead_code)]
    pub mod palette;
    #[allow(dead_code)]
    pub mod wave;
}

use std::io::Write;
use std::path::PathBuf;

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("dictate")
}

fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DICTATE_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    runtime_dir().join("control.sock")
}

/// Lock-Datei, solange der Prozess lebt (Freigabe beim Beenden durch das Betriebssystem).
fn single_instance() -> Result<std::fs::File, String> {
    let dir = runtime_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = std::env::var_os("DICTATE_SETTINGS_LOCK").map(PathBuf::from).unwrap_or_else(|| dir.join("settings.lock"));
    let mut f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).read(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => {
            let _ = f.set_len(0);
            let _ = writeln!(f, "{}", std::process::id());
            Ok(f)
        }
        Err(_) => Err("dictate-settings is already running".into()),
    }
}

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../assets/logo/icon-512.png")).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() -> eframe::Result<()> {
    let lock = match single_instance() {
        Ok(l) => l,
        Err(m) => {
            eprintln!("dictate-settings: {m}");
            std::process::exit(1);
        }
    };
    // Mit `--page NAME` direkt auf eine Seite starten (Tests, Desktop-Aktionen)
    let args: Vec<String> = std::env::args().collect();
    let start = args.windows(2).find(|w| w[0] == "--page").and_then(|w| gui::Page::from_arg(&w[1])).unwrap_or(gui::Page::General);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Dictate Settings")
        .with_app_id("dictate-settings")
        .with_inner_size([1180.0, 860.0])
        .with_min_inner_size([820.0, 560.0]);
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let sock = socket_path();
    let r = eframe::run_native("dictate-settings", options, Box::new(move |cc| Ok(Box::new(gui::App::new(cc, sock, start)))));
    drop(lock);
    r
}
