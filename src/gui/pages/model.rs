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

use crate::gui::{downloads, downloads::DlState, theme::*, widgets::*, App, CatalogItem};
use egui::Ui;

fn human(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.1} GB", n as f64 / (1u64 << 30) as f64)
    } else if n >= 1 << 20 {
        format!("{:.0} MB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", n / 1024)
    }
}

/// Pfad eines Modells in der Config: `~/...` wenn es im Home liegt.
fn config_path_for(file: &str) -> String {
    let p = downloads::models_dir().join(file);
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(|r| r.to_path_buf())) {
        Some(rel) => format!("~/{}", rel.display()),
        None => p.display().to_string(),
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Model", "Which recognition model runs, and how long it stays in memory.");
    let errs: Vec<Option<String>> = ["model", "vad_model", "threads", "idle_unload_s"].iter().map(|k| app.err(k)).collect();

    group(ui, "Active model", |ui| {
        let s = &mut app.edited;
        field(ui, "Model file", None, errs[0].as_deref(), |ui, e| {
            mono_edit(ui, &mut s.model, e, 520.0);
        });
        field(ui, "Voice activity model", Some("Keeps silence from turning into invented text."), errs[1].as_deref(), |ui, e| {
            mono_edit(ui, &mut s.vad_model, e, 520.0);
        });
        field(ui, "CPU threads", None, errs[2].as_deref(), |ui, e| {
            number(ui, &mut s.threads, 1, 64, "", e);
        });
        field(ui, "Unload when idle", Some("Frees GPU memory after this long without use. 0 keeps it loaded."), errs[3].as_deref(), |ui, e| {
            number(ui, &mut s.idle_unload_s, 0, 86400, " s", e);
        });
    });

    group(ui, "Installed", |ui| {
        if app.installed.is_empty() {
            ui.label(secondary("No models found in ~/.local/share/dictate/models."));
        }
        let mut pick = None;
        for m in &app.installed {
            hairline(ui);
            ui.horizontal(|ui| {
                cell(ui, egui::vec2(380.0, 40.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(mono(&m.file));
                });
                cell(ui, egui::vec2(80.0, 40.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(small(human(m.size)));
                });
                let active = app.edited.model.ends_with(&m.file);
                if chip(ui, if active { "In use" } else { "Use" }, active).clicked() && !active {
                    pick = Some(config_path_for(&m.file));
                }
            });
        }
        if !app.installed.is_empty() {
            hairline(ui);
        }
        if let Some(p) = pick {
            app.edited.model = p;
        }
    });

    group(ui, "Download", |ui| {
        if app.catalog.is_empty() {
            ui.add(egui::Label::new(secondary("The catalog comes from the running daemon.")).wrap());
            return;
        }
        let catalog: Vec<CatalogItem> = app.catalog.clone();
        let mut start = None;
        let mut cancel = None;
        for c in &catalog {
            hairline(ui);
            let file = downloads::file_name_from_url(&c.url).unwrap_or_default();
            let have = app.installed.iter().any(|i| i.file == file);
            ui.horizontal(|ui| {
                cell(ui, egui::vec2(380.0, 56.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    ui.label(body(&c.id));
                    match app.downloads.get(&c.id).map(|d| &d.progress.state) {
                        Some(DlState::Failed(m)) => {
                            ui.label(text(m.clone(), Size::Small, egui::FontFamily::Proportional, th().error));
                        }
                        _ => {
                            ui.label(small(format!("{}{}", human(c.size), if c.note.is_empty() { String::new() } else { format!("  ·  {}", c.note) })));
                        }
                    }
                });
                cell(ui, egui::vec2(300.0, 56.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    match app.downloads.get(&c.id).map(|d| (d.progress.clone(), d.job.is_some())) {
                        Some((p, true)) => {
                            let frac = if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 };
                            pixel_meter(ui, frac);
                            ui.label(small(format!("{:.0}%", frac * 100.0)));
                            if link_button(ui, "Cancel").clicked() {
                                cancel = Some(c.id.clone());
                            }
                        }
                        other => {
                            let _ = &other;
                            if have {
                                chip(ui, "Installed", true);
                            } else if secondary_button(ui, "Download").clicked() {
                                start = Some(c.clone());
                            }
                        }
                    }
                });
            });
        }
        hairline(ui);
        if let Some(c) = start {
            app.start_download(&c);
        }
        if let Some(id) = cancel {
            if let Some(d) = app.downloads.get(&id) {
                if let Some(j) = &d.job {
                    j.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        ui.add_space(8.0);
        ui.label(small("Files are checked against their SHA-256 sum before they are kept."));
    });
}
