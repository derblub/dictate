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

//! Befehlspalette (Baustein 3, `PLAN-palette.md`): freie Eingabe → feste Absicht → Ausführung im Panel (bis W7 in Lua
//! unter AwesomeWM, unter niri folgt das Panel mit W5, `PLAN-wayland.md`).
//!
//! Dieses Modul enthält bisher die Grundlage (Aufgabe 1): den Katalog der Absichten ([`catalog`]) und die Absichten
//! selbst mit ihrer Validierung ([`intent`]), dazu die Quellen (Aufgabe 3): Programme, Projekte und Häufigkeit
//! ([`sources`]) und der lokale Resolver (Aufgabe 2): Eingabe → sortierte Kandidaten ohne Modell ([`resolve`]), dazu der
//! Dienst im Daemon (Aufgabe 5): Quellen, Häufigkeit und die Antworten auf `pal_*` ([`service`]), dazu der KI-Pfad
//! (Aufgabe 4): freie Sprache → eine Absicht ([`ai`]), danach lokal aufgelöst ([`resolve::resolve_intent`]), dazu die
//! Erkennung der eingebauten Anzeigen Zwischenablage und Emoji ([`widgets`], `PLAN-lizenz.md` O6).
//!
//! Das Modell liefert nie ausführbaren Text, sondern nur eine [`Intent`]; die Rust-Validierung
//! ([`parse_validated`], [`Intent::validate`]) ist die einzige Schranke, unabhängig vom Anbieter.

pub mod ai;
pub mod catalog;
pub mod intent;
pub mod resolve;
pub mod service;
pub mod sources;
#[cfg(test)]
mod ui_mirror;
pub mod widgets;

pub use crate::protocol::SessionEnv;
pub use catalog::{Class, Group, IntentSpec};
pub use intent::{parse_validated, Intent, IntentError};
pub use resolve::{resolve, resolve_intent, resolve_intent_with, resolve_with, Candidate, Context, ContextError, Features, NoTarget, Resolution, Target};
pub use service::{Palette, PaletteError};
