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

use crate::gui::{theme::*, widgets::*, App};
use egui::Ui;

/// Rückfall, wenn kein Daemon läuft; sonst kommt die Tabelle per `{"cmd":"commands"}` aus der Pipeline
/// (`src/pipeline/commands.rs`, dort die Quelle der Wahrheit).
const COMMANDS: [(&str, &str, &str); 13] = [
    ("neue Zeile", "new line", "Line break"),
    ("neuer Absatz", "new paragraph", "Blank line"),
    ("Komma", "comma", "Types ,"),
    ("Punkt", "period", "Types ."),
    ("Fragezeichen", "question mark", "Types ?"),
    ("Ausrufezeichen", "exclamation mark", "Types !"),
    ("Doppelpunkt", "colon", "Types :"),
    ("Semikolon", "semicolon", "Types ;"),
    ("Bindestrich", "hyphen", "Types - (no spaces)"),
    ("lösch das letzte Wort", "delete last word", "Removes the last word of this dictation"),
    ("alles löschen", "scratch that", "Discards this dictation so far"),
    ("rückgängig", "undo that", "Deletes the previous dictation (say it alone)"),
    ("wörtlich …", "literally …", "Types the next word as it is, e.g. \"literally comma\""),
];

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Voice commands", "Spoken words that act instead of being typed.");
    feature_switch(ui, &mut app.edited.features.voice_commands, "Enabled", "Disabled");
    let rows: Vec<[&str; 3]> = match &app.commands {
        Some(c) if !c.is_empty() => c.iter().map(|r| [r[0].as_str(), r[1].as_str(), r[2].as_str()]).collect(),
        _ => COMMANDS.iter().map(|c| [c.0, c.1, c.2]).collect(),
    };
    group(ui, "Commands", |ui| {
        ui.horizontal(|ui| {
            for (w, l) in [(200.0f32, "German"), (200.0, "English"), (0.0, "Effect")] {
                cell(ui, egui::vec2(f32::max(w, 40.0), 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(group_label(l));
                });
            }
        });
        for [de, en, effect] in &rows {
            hairline(ui);
            ui.horizontal(|ui| {
                cell(ui, egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(body(*de));
                });
                cell(ui, egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(body(*en));
                });
                ui.label(secondary(*effect));
            });
        }
        hairline(ui);
        ui.add_space(8.0);
        ui.add(egui::Label::new(small("Case and punctuation do not matter. Say \"literally\" first to type a command word as it is.")).wrap());
    });
}
