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

//! Teile von `dictate-ui` (Wayland-Oberfläche, PLAN-wayland.md) ohne GTK und ohne Wayland: Haltelogik, Tastenzustand,
//! Steuer-Socket der Kürzel, das Modell des Overlays und die Logik der Textausgabe (Warteschlange, Wege, Belegung der
//! virtuellen Tastatur). Die Bibliothek enthält sie für die Unit-Tests; `src/bin/dictate-ui/` bindet sie per Pfad ein
//! (ohne Whisper/CUDA). Darum nur `std`, `libc`, `serde_json`, `config`, `protocol` und
//! `actions` (Überschreiben von Text, Codeblöcke) als Abhängigkeiten. Dazu ab W3 der Automat und das Markdown des
//! Panels der Textaktionen (`panel`, `markdown`) und die Sperr-Erkennung über logind (`lock`, ohne D-Bus).
//! Ab W5 die Befehlspalette ohne GTK und ohne niri-Typen: Kontext aus niri (`pal_context`), Ausführer mit Suchpfad
//! und Wirt (`pal_exec`), Prüfung der Adressen (`pal_url`), dazu der Automat ihres Panels (`cmdpal`) und der Besitz der
//! Aufnahme zwischen Fenster und Palette (`voice_owner`).

pub mod boot_hint;
pub mod cmdpal;
pub mod ctl;
pub mod diag_notes;
pub mod diag_panel;
pub mod fake_keys;
pub mod fsutil;
pub mod gate;
pub mod hold;
pub mod keymap;
pub mod keys;
pub mod lock;
pub mod markdown;
pub mod output;
pub mod pal_context;
pub mod pal_exec;
pub mod pal_url;
pub mod palette;
pub mod panel;
pub mod sysfs;
pub mod view;
pub mod voice_owner;
pub mod wave;
