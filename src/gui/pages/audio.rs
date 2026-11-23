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

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Audio", "Pick the microphone and check that it hears you.");
    let e_src = app.err("source");
    let e_gain = app.err("max_gain_db");

    group(ui, "Input", |ui| {
        let mut opts: Vec<(String, String)> = vec![(String::new(), "System default".into())];
        for src in &app.sources {
            let mut label = if src.description.is_empty() { src.name.clone() } else { src.description.clone() };
            if src.is_default {
                label.push_str("  (default)");
            }
            opts.push((src.name.clone(), label));
        }
        field(ui, "Microphone", Some("Takes effect with the next recording."), e_src.as_deref(), |ui, e| {
            combo(ui, "source", &mut app.edited.source, &opts, 520.0, e);
        });
        field(
            ui,
            "Boost quiet input up to",
            Some("Raises a quiet microphone to a normal speech level before speech detection and recognition. Never lowers loud input or lifts pure noise. 0 turns it off."),
            e_gain.as_deref(),
            |ui, e| {
                number(ui, &mut app.edited.max_gain_db, 0, 40, " dB", e);
            },
        );
    });

    group(ui, "Level", |ui| {
        if !app.online {
            ui.add(egui::Label::new(secondary("The live level needs the running daemon. Start dictate.service to see it.")).wrap());
            return;
        }
        let frac = ((app.level_db + 60.0) / 50.0).clamp(0.0, 1.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            // Lädt das Modell oder läuft die Endverarbeitung: der eine wartende Pixel statt des Pegels
            let waiting = matches!(app.daemon_state.as_str(), "loading" | "finalizing");
            pixel_cols(ui, if waiting { ColsMode::Waiting } else { ColsMode::Level { frac, speech: app.speech } });
            ui.label(mono(format!("{:>6.1} dB", app.level_db)));
            if app.speech {
                ui.label(text("Speech", Size::Small, egui::FontFamily::Proportional, th().accent));
            }
        });
        if let Some(e) = &app.meter_error {
            ui.add_space(8.0);
            ui.label(text(e.clone(), Size::Small, egui::FontFamily::Proportional, th().error));
        }
        ui.add_space(8.0);
        ui.label(small("Speak normally. The columns fill with your voice and take the accent color when speech is detected."));
    });
}
