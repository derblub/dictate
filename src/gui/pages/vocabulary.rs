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

use crate::gui::theme::*;
use crate::gui::{widgets::*, App};
use egui::Ui;

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Vocabulary", "Teach it your words and fix what it keeps getting wrong.");
    let s = &mut app.edited;
    feature_switch(ui, &mut s.features.vocabulary, "Enabled", "Disabled");
    group(ui, "Terms", |ui| {
        ui.add(egui::Label::new(secondary("Names and jargon. They go to the recognizer as a hint.")).wrap());
        ui.add_space(8.0);
        string_list(ui, &mut s.vocabulary.terms, "Add term", "Remove", false, 360.0);
    });
    group(ui, "Replacements", |ui| {
        ui.add(egui::Label::new(secondary("Applied after recognition, on whole words, ignoring case.")).wrap());
        ui.add_space(8.0);
        let mut remove = None;
        for (i, (from, to)) in s.vocabulary.replacements.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                text_edit(ui, from, false, 240.0);
                ui.label(secondary("becomes"));
                text_edit(ui, to, false, 240.0);
                if link_button(ui, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            s.vocabulary.replacements.remove(i);
        }
        if secondary_button(ui, "Add replacement").clicked() {
            s.vocabulary.replacements.push((String::new(), String::new()));
        }
    });
}
