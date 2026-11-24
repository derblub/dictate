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

use crate::gui::{dto::Profile, theme::*, widgets::*, App};
use egui::Ui;

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Profiles", "Different rules for different apps. The first matching profile wins.");
    feature_switch(ui, &mut app.edited.features.profiles, "Enabled", "Disabled");

    let n = app.edited.profiles.len();
    if app.profile_sel >= n {
        app.profile_sel = n.saturating_sub(1);
    }

    group(ui, "Profiles", |ui| {
        if n == 0 {
            ui.label(secondary("No profiles yet. Without a match, the global settings apply."));
            ui.add_space(8.0);
        }
        ui.horizontal_wrapped(|ui| {
            for (i, p) in app.edited.profiles.iter().enumerate() {
                let name = if p.name.is_empty() { "(unnamed)".to_string() } else { p.name.clone() };
                if chip(ui, &name, i == app.profile_sel).clicked() {
                    app.profile_sel = i;
                }
            }
            if secondary_button(ui, "Add profile").clicked() {
                let name = format!("profile{}", app.edited.profiles.len() + 1);
                app.edited.profiles.push(Profile { name, ..Default::default() });
                app.profile_sel = app.edited.profiles.len() - 1;
            }
        });
    });

    if n == 0 {
        return;
    }
    let i = app.profile_sel;
    // `match_class` meldet der Daemon je Eintrag (`profiles[0].match_class[1]`)
    let errs: Vec<Option<String>> = ["name", "match_class", "punctuation", "language"].iter().map(|k| app.err_under(&format!("profiles.{i}.{k}"))).collect();
    let bad_class = app.err_index(&format!("profiles.{i}.match_class"));
    let modes = app.edited.mode_names();
    let mut delete = false;
    let p = &mut app.edited.profiles[i];

    group(ui, "Match", |ui| {
        field(ui, "Name", Some("A profile named default is used when nothing else matches."), errs[0].as_deref(), |ui, e| {
            text_edit(ui, &mut p.name, e, 360.0);
        });
        field(
            ui,
            "App IDs",
            errs[1].as_deref().map_or(Some("Exact app_id (or the X11 class of a program under XWayland), case does not matter."), |_| None),
            errs[1].as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut p.match_class, "Add app_id", "Remove", true, 280.0, bad_class);
            },
        );
        ui.horizontal(|ui| {
            ui.label(small("Find an app_id: run"));
            ui.label(mono("niri msg windows"));
            ui.label(small("and look for App ID."));
            if link_button(ui, "Copy").clicked() {
                ui.ctx().copy_text("niri msg windows".into());
            }
        });
        ui.add_space(16.0);
    });

    group(ui, "Text", |ui| {
        field(ui, "Punctuation", None, errs[2].as_deref(), |ui, _| {
            chips(ui, &mut p.punctuation, &[("keep", "Keep".into()), ("strip", "Strip".into())]);
        });
        field(ui, "Capital first letter", None, None, |ui, _| {
            toggle(ui, &mut p.capitalize_first, "Yes", "No");
        });
        field(ui, "Space after the text", None, None, |ui, _| {
            toggle(ui, &mut p.trailing_space, "Yes", "No");
        });
        field(ui, "Language", Some("Empty uses the global language."), errs[3].as_deref(), |ui, e| {
            text_edit(ui, &mut p.language, e, 120.0);
        });
        field(ui, "AI mode", Some("Needs the AI feature."), None, |ui, _| {
            let mut opts = vec![(String::new(), "None".to_string())];
            opts.extend(modes.iter().map(|m| (m.clone(), m.clone())));
            combo(ui, "prof_ai", &mut p.ai_mode, &opts, 240.0, false);
        });
    });

    if secondary_button(ui, "Delete profile").clicked() {
        delete = true;
    }
    if delete {
        app.edited.profiles.remove(i);
    }
}
