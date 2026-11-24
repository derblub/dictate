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
    page_header(ui, "Recognition", "How pauses, limits and unwanted phrases are handled.");
    let errs: Vec<Option<String>> = ["commit_silence_ms", "max_phrase_ms", "step_ms", "final_beam_size", "max_listen_s", "initial_prompt", "blocklist"]
        .iter()
        .map(|k| app.err(k))
        .collect();
    let s = &mut app.edited;
    group(ui, "Timing", |ui| {
        field(ui, "Pause that ends a phrase", Some("Silence after which a phrase is fixed in place."), errs[0].as_deref(), |ui, e| {
            number(ui, &mut s.commit_silence_ms, 100, 5000, " ms", e);
        });
        field(ui, "Longest phrase", Some("A phrase is cut after this long, even without a pause."), errs[1].as_deref(), |ui, e| {
            number(ui, &mut s.max_phrase_ms, 1000, 60000, " ms", e);
        });
        field(ui, "Update interval", Some("How often the live text is recomputed."), errs[2].as_deref(), |ui, e| {
            number(ui, &mut s.step_ms, 50, 2000, " ms", e);
        });
        field(ui, "Longest recording", Some("0 means no limit."), errs[4].as_deref(), |ui, e| {
            number(ui, &mut s.max_listen_s, 0, 3600, " s", e);
        });
    });
    group(ui, "Quality", |ui| {
        field(ui, "Final pass beam size", Some("1 is fastest. Higher values are slower and slightly more exact."), errs[3].as_deref(), |ui, e| {
            number(ui, &mut s.final_beam_size, 1, 8, "", e);
        });
        field(ui, "Initial prompt", Some("A hint for the recognizer, for example names or jargon."), errs[5].as_deref(), |ui, e| {
            text_area(ui, &mut s.initial_prompt, 3, e, 520.0);
        });
    });
    group(ui, "Blocklist", |ui| {
        ui.add(egui::Label::new(secondary("Phrases the recognizer invents in silence. They are dropped when they are the whole result.")).wrap());
        ui.add_space(8.0);
        string_list(ui, &mut s.blocklist, "Add phrase", "Remove", false, 440.0);
    });
}
