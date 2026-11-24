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

//! Seite About: Logo nach Vorlage V3 (aus Rollen gezeichnet), Version, Urheber nach Vorlage V4 (Name, Ort und Link
//! als Text; das Zeichen der Marke entfällt, weil es nur hell auf dunkel vorliegt und nicht umgefärbt werden darf).

use crate::gui::theme::*;
use crate::gui::widgets::logo;
use crate::gui::App;
use egui::{FontFamily, Ui};

fn open(url: &str) {
    // Ohne Shell: die URL ist ein eigenes Argument
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn show(app: &mut App, ui: &mut Ui) {
    ui.add_space(8.0);
    logo(ui, 36.0);
    ui.add_space(8.0);
    ui.label(small(format!("Version {}", env!("CARGO_PKG_VERSION"))));
    ui.add_space(48.0);

    ui.label(group_label("Made by"));
    ui.add_space(8.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(body("Daniel Kurdoghlian"));
        ui.label(secondary("Pushing Pixels, Vienna"));
        if link(ui, "pushingpixels.at") {
            open("https://pushingpixels.at");
        }
    });

    let url = app.saved.support_url.clone();
    if url.starts_with("http://") || url.starts_with("https://") {
        ui.add_space(48.0);
        if link(ui, "Support dictate") {
            open(&url);
        }
    }
}

/// Link mit Pfeil „öffnet außerhalb“ (U+2197 mit U+FE0E, damit er als Text und nicht als Emoji erscheint; der Pfeil
/// in Mono wie bisher)
fn link(ui: &mut Ui, label: &str) -> bool {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let a = ui.add(egui::Link::new(text(label, Size::Body, FontFamily::Proportional, th().accent)));
        let b = ui.add(egui::Link::new(text("\u{2197}\u{FE0E}", Size::Body, FontFamily::Monospace, th().accent)));
        a.clicked() || b.clicked()
    })
    .inner
}
