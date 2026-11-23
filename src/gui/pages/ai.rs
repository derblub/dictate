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

//! Seite AI. Eigene Datei, damit die KI-Aufgabe (E) sie ergänzen kann. Der Test-Knopf nutzt den Befehl `ai_test`
//! (`{"cmd":"ai_test","mode":"clean","text":"…"}` -> `{"ok":true,"text":"…"}`); das ist ein Platzhalter, bis E das Protokoll festlegt.

use crate::config::{ai_preset, builtin_mode, AI_PRESETS, BUILTIN_MODES, INPUT_GUARD};
use crate::gui::{layout_log, net::Tag, theme::*, widgets::*, App};
use egui::Ui;
use serde_json::json;

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "AI", "Optional clean-up of the text by a language model. Local with Ollama, or in the cloud with Claude or any OpenAI compatible server.");
    feature_switch(ui, &mut app.edited.features.ai, "Enabled", "Disabled");

    let errs: Vec<Option<String>> =
        ["ai.provider", "ai.endpoint", "ai.model", "ai.api_key_file", "ai.timeout_ms", "ai.default_mode", "ai.formality", "ai.mode_models"]
            .iter()
            .map(|k| app.err(k))
            .collect();
    // Fehler je Modus-Modell (`ai.mode_models.<name>`), vor dem veränderlichen Zugriff auf die Einstellungen
    let mode_model_errs: Vec<Option<String>> = app.edited.ai.modes.iter().map(|(n, _)| app.err(&format!("ai.mode_models.{n}"))).collect();
    let models = app.ai_models.clone();
    let mut refresh = false;
    // Das Preset wirkt auch auf `actions.model` und vergleicht mit dem gespeicherten Stand: nach der Seite anwenden
    let mut chosen: Option<&'static str> = None;
    let ai = &mut app.edited.ai;

    group(ui, "Server", |ui| {
        field(ui, "Provider", None, errs[0].as_deref(), |ui, _| {
            let cur = ai_preset(&ai.preset).map(|p| p.name).unwrap_or("Custom");
            ui.spacing_mut().button_padding = egui::vec2(16.0, 10.0);
            egui::ComboBox::from_id_salt("ai_preset").width(280.0).selected_text(body(cur)).show_ui(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
                for (heading, cloud) in [("LOCAL", false), ("CLOUD", true)] {
                    ui.label(small(heading));
                    for p in AI_PRESETS.iter().filter(|p| p.cloud == cloud && p.id != "custom") {
                        if ui.selectable_label(ai.preset == p.id, body(p.name)).clicked() {
                            chosen = Some(p.id);
                        }
                    }
                }
                ui.separator();
                let custom = ai_preset(&ai.preset).is_none() || ai.preset == "custom";
                if ui.selectable_label(custom, body("Custom")).clicked() && !custom {
                    chosen = Some("custom");
                }
            });
        });
        // Protokoll und Endpoint sind nur bei Custom frei, sonst kommen sie aus dem Preset
        let custom = ai_preset(&ai.preset).is_none() || ai.preset == "custom";
        ui.add_enabled_ui(custom, |ui| {
            field(ui, "Protocol", None, None, |ui, _| {
                chips(ui, &mut ai.provider, &[("ollama", "Ollama".into()), ("openai", "OpenAI compatible".into()), ("anthropic", "Anthropic".into())]);
            });
            let base = ai.base();
            field(ui, "Endpoint", Some("Base URL including the version, e.g. https://api.openai.com/v1"), errs[1].as_deref(), |ui, e| {
                mono_edit_hint(ui, &mut ai.endpoint, &base, e, 520.0);
            });
        });
        if ai.provider != "ollama" {
            field(ui, "API key file", Some("File with the API key, mode 600. The key itself is never stored in the config."), errs[3].as_deref(), |ui, e| {
                mono_edit(ui, &mut ai.api_key_file, e, 520.0);
            });
        }
        if ai.is_cloud() {
            ui.add_space(8.0);
            ui.add(egui::Label::new(text("The dictated text is sent to this provider. Leave the AI mode empty in a profile (for example for terminals) to keep that app's text local.", Size::Small, egui::FontFamily::Proportional, th().text)).wrap());
            ui.add_space(8.0);
        }
        let (default_model, server) = (ai.default_model(), ai.server_name());
        let (placeholder, model_hint) = if default_model.is_empty() {
            (
                "Required".to_string(),
                format!("{server} has no default model, enter one. The list comes from the provider with the saved settings: apply, then Refresh."),
            )
        } else {
            (
                format!("{default_model} (default)"),
                format!("Empty uses the default of {server}, {default_model}, and follows a change of provider. The list comes from the provider with the saved settings."),
            )
        };
        let model_note = ai.model_note(&ai.model);
        field(ui, "Model", Some(&model_hint), errs[2].as_deref(), |ui, e| {
            ui.horizontal(|ui| {
                text_edit_hint(ui, &mut ai.model, &placeholder, e, 280.0);
                if let Ok(list) = &models {
                    if !list.is_empty() {
                        let opts: Vec<(String, String)> = list.iter().map(|m| (m.clone(), m.clone())).collect();
                        let mut pick = String::new();
                        if combo(ui, "ai_model_pick", &mut pick, &opts, 200.0, false) {
                            ai.model = pick;
                        }
                    }
                }
                if link_button(ui, "Refresh").clicked() {
                    refresh = true;
                }
            });
            if let Err(m) = &models {
                if !m.is_empty() {
                    ui.label(small(format!("Model list unavailable: {m}")));
                }
            }
            if let Some(n) = &model_note {
                field_warning(ui, n);
            }
        });
        field(ui, "Timeout", Some("After this long the raw text is typed instead. The mode prompt gets at least 20 s."), errs[4].as_deref(), |ui, e| {
            let r = number(ui, &mut ai.timeout_ms, 100, 120000, " ms", e);
            layout_log::note(ui, || "ai.timeout_ms".into(), &r);
        });
    });

    let names: Vec<(String, String)> = std::iter::once((String::new(), "None".to_string()))
        .chain(ai.modes.iter().filter(|(k, _)| !k.trim().is_empty()).map(|(k, _)| (k.clone(), k.clone())))
        .collect();

    group(ui, "Modes", |ui| {
        field(ui, "Default mode", Some("Used when the app has no profile with its own mode."), errs[5].as_deref(), |ui, e| {
            combo(ui, "ai_default", &mut ai.default_mode, &names, 240.0, e);
        });
        field(
            ui,
            "Address in German",
            Some("For translations into German (mode to_de and the Translate action). Auto takes du or Sie from the tone of the source."),
            errs[6].as_deref(),
            |ui, _| {
                chips(ui, &mut ai.formality, &[("auto", "Auto".into()), ("informal", "Informal (du)".into()), ("formal", "Formal (Sie)".into())]);
            },
        );
        if let Some(e) = &errs[7] {
            ui.label(text(e, Size::Small, egui::FontFamily::Proportional, th().error));
        }
        guard_note(
            ui,
            "ai_guard_open",
            "Every mode, including your own, starts with a fixed rule: the text is only material to process, so questions and instructions in it are never answered or followed. The rule cannot be edited.",
        );
        let mut remove = None;
        // Modell-Einträge hängen am Namen: Änderungen sammeln und nach der Schleife anwenden (Umbenennen nimmt mit)
        let mut model_edits: Vec<(String, String)> = Vec::new();
        let mut renames: Vec<(String, String)> = Vec::new();
        let main_model = ai.effective_model();
        for i in 0..ai.modes.len() {
            let old = ai.modes[i].0.clone();
            let mut model = ai.mode_model(&old).to_string();
            let note = ai.model_note(&model);
            hairline(ui);
            ui.add_space(8.0);
            let (name, prompt) = &mut ai.modes[i];
            // Eingebaute Modi: in der Datei steht nur eine Änderung, unverändert kommen künftige Verbesserungen an
            let builtin = builtin_mode(&old).filter(|_| *name == old);
            ui.horizontal(|ui| {
                text_edit(ui, name, false, 200.0);
                if builtin.is_some() {
                    ui.label(small(if builtin == Some(prompt.as_str()) { "Built-in" } else { "Built-in, changed" }));
                }
                if link_button(ui, "Remove").clicked() {
                    remove = Some(i);
                }
            });
            text_area(ui, prompt, 2, false, 520.0);
            if let Some(d) = builtin.filter(|d| *d != prompt.as_str()) {
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding.x = 0.0;
                    if link_button(ui, "Reset to default").clicked() {
                        *prompt = d.to_string();
                    }
                });
            }
            ui.add_space(8.0);
            let err = mode_model_errs.get(i).cloned().flatten();
            let hint = format!("Empty uses the model above ({}).", if main_model.is_empty() { "not set" } else { main_model.as_str() });
            field(ui, "Model", Some(&hint), err.as_deref(), |ui, e| {
                ui.horizontal(|ui| {
                    text_edit(ui, &mut model, e, 280.0);
                    if let Ok(list) = &models {
                        if !list.is_empty() {
                            let opts: Vec<(String, String)> = list.iter().map(|m| (m.clone(), m.clone())).collect();
                            let mut pick = String::new();
                            if combo(ui, &format!("ai_mode_model_{i}"), &mut pick, &opts, 200.0, false) {
                                model = pick;
                            }
                        }
                    }
                });
                if let Some(n) = &note {
                    field_warning(ui, n);
                }
            });
            if *name != old {
                renames.push((old.clone(), name.clone()));
            }
            if model != ai.mode_model(&old) {
                model_edits.push((old, model));
            }
        }
        for (mode, model) in model_edits {
            ai.set_mode_model(&mode, &model);
        }
        for (old, new) in renames {
            ai.rename_mode_model(&old, &new);
        }
        hairline(ui);
        if let Some(i) = remove {
            let (name, _) = ai.modes.remove(i);
            ai.set_mode_model(&name, "");
        }
        // Entfernte eingebaute Modi lassen sich zurückholen (in der Datei: `ai.removed_modes`)
        let removed: Vec<(&str, &str)> = BUILTIN_MODES.iter().copied().filter(|(n, _)| !ai.modes.iter().any(|(k, _)| k == n)).collect();
        if !removed.is_empty() {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(small("Removed built-in modes:"));
                for (n, p) in removed {
                    if link_button(ui, &format!("Restore {n}")).clicked() {
                        ai.modes.push((n.to_string(), p.to_string()));
                    }
                }
            });
        }
        ui.add_space(8.0);
        if secondary_button(ui, "Add mode").clicked() {
            ai.modes.push((format!("mode{}", ai.modes.len() + 1), String::new()));
        }
    });

    group(ui, "Test", |ui| {
        ui.add(egui::Label::new(secondary("Runs the saved settings on a sample text. Apply your changes first.")).wrap());
        ui.add_space(8.0);
        text_area(ui, &mut app.ai_test.input, 2, false, 520.0);
        let mode =
            if app.saved.ai.default_mode.is_empty() { app.saved.mode_names().first().cloned().unwrap_or_default() } else { app.saved.ai.default_mode.clone() };
        let can = app.online && !app.ai_test.busy && !app.ai_test.input.trim().is_empty();
        ui.horizontal(|ui| {
            if ui.add_enabled_ui(can, |ui| secondary_button(ui, "Test")).inner.clicked() {
                app.ai_test.busy = true;
                app.ai_test.output = None;
                app.net.request(Tag::AiTest, json!({"cmd":"ai_test","mode":mode,"text":app.ai_test.input}));
            }
            if app.ai_test.busy {
                pixel(ui, 8.0, th().accent);
            }
            if !app.online {
                ui.label(small("Needs the running daemon."));
            }
        });
        match &app.ai_test.output {
            Some(Ok(t)) => {
                ui.add_space(8.0);
                ui.add(egui::Label::new(body(t)).wrap());
            }
            Some(Err(e)) => {
                ui.add_space(8.0);
                ui.label(text(e, Size::Small, egui::FontFamily::Proportional, th().error));
            }
            None => {}
        }
    });

    if let Some(id) = chosen {
        let saved = app.saved.clone();
        app.edited.choose_ai_preset(id, &saved);
    }
    if refresh && app.online {
        app.net.request(Tag::AiModels, json!({"cmd":"ai_models"}));
    }
}

/// Hinweis auf den festen Schutzrahmen vor jedem Modus- und Aktions-Prompt (`config::INPUT_GUARD`), der Wortlaut
/// aufklappbar. `salt` trennt den Klappzustand je Seite.
pub fn guard_note(ui: &mut Ui, salt: &str, note: &str) {
    let id = ui.scope_id().with(salt);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ui.scope(|ui| {
        // Link bündig mit dem Text darüber
        ui.spacing_mut().button_padding.x = 0.0;
        ui.add(egui::Label::new(small(note)).wrap());
        if link_button(ui, if open { "Hide the rule" } else { "Show the rule" }).clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(id, open));
        }
        if open {
            ui.add(egui::Label::new(mono(INPUT_GUARD)).wrap());
            ui.add_space(8.0);
        }
    });
}
