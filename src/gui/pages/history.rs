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
use crate::gui::{net::Tag, widgets::*, App};
use egui::Ui;
use serde_json::json;

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "History", "What you dictated, kept on this computer only.");
    let e_max = app.err("history.max_entries");
    feature_switch(ui, &mut app.edited.features.history, "Enabled", "Disabled");

    if !app.edited.features.history {
        ui.add(egui::Label::new(secondary("History is off. Enable it and press Apply to keep new dictations. The file stays private to you.")).wrap());
        return;
    }
    group(ui, "Limit", |ui| {
        field(ui, "Entries to keep", None, e_max.as_deref(), |ui, e| {
            number(ui, &mut app.edited.history.max_entries, 1, 100000, "", e);
        });
    });
    group(ui, "Entries", |ui| {
        if !app.online {
            ui.label(secondary("The list needs the running daemon."));
            return;
        }
        ui.horizontal(|ui| {
            if app.confirm_clear {
                if secondary_button(ui, "Really delete all").clicked() {
                    app.net.request(Tag::HistoryClear, json!({"cmd":"history_clear"}));
                }
                if link_button(ui, "Keep").clicked() {
                    app.confirm_clear = false;
                }
            } else if secondary_button(ui, "Clear history").clicked() {
                app.confirm_clear = true;
            }
            if secondary_button(ui, "Reload").clicked() {
                app.net.request(Tag::History, json!({"cmd":"history","limit":200}));
            }
        });
        ui.add_space(8.0);
        if app.history.is_empty() {
            ui.label(secondary("Nothing here yet."));
        }
        let mut copy = None;
        for e in &app.history {
            hairline(ui);
            ui.horizontal_top(|ui| {
                cell(ui, egui::vec2(120.0, 24.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    ui.label(small(e.ts.replace('T', " ").chars().take(16).collect::<String>()));
                    if !e.app.is_empty() {
                        ui.label(mono(&e.app));
                    }
                });
                cell(ui, egui::vec2(480.0, 24.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(body(&e.text)).wrap());
                    ui.add_space(6.0);
                });
                if link_button(ui, "Copy").clicked() {
                    copy = Some(e.text.clone());
                }
            });
        }
        hairline(ui);
        if let Some(t) = copy {
            ui.ctx().copy_text(t);
        }
    });
}
