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

//! Seite Actions: Textaktionen auf der Markierung (`[actions]`, `features.actions`). Server und KI-Modi kommen von der
//! Seite AI; hier stehen Modell, Grenzen, Geheimnisfilter, Antwortsprache, Terminals und die Prompts der eingebauten
//! Aktionen (Katalog `crate::actions::BUILTIN`, Arten des Filters `crate::actions::secrets::KINDS`).

use crate::actions::{self, secrets, Output, BUILTIN};
use crate::config::{ACTIONS_MAX_CHARS, ACTIONS_MAX_TIMEOUT_MS};
use crate::gui::pages::{ai::guard_note, general::LANGUAGES};
use crate::gui::{theme::*, widgets::*, App, Page};
use egui::Ui;

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Actions", "Explain, translate or rephrase selected text with the AI server. Select text anywhere, press Mod+I and pick an action.");
    feature_switch(ui, &mut app.edited.features.actions, "Enabled", "Disabled");

    let err_model = app.err("actions.model");
    let err_timeout = app.err("actions.timeout_ms");
    let err_max = app.err("actions.max_chars");
    let err_secrets = app.err("actions.secrets");
    let err_lang = app.err("actions.language");
    let err_ai_modes = app.err("actions.include_ai_modes");
    let err_terms = app.err_under("actions.terminal_classes");
    let bad_term = app.err_index("actions.terminal_classes");
    let err_prompts = app.err("actions.prompts");
    let prompt_errs: Vec<Option<String>> = BUILTIN.iter().map(|b| app.err(&format!("actions.prompts.{}", b.name))).collect();
    let models = app.ai_models.clone();
    let mut open_ai = false;

    let ai = &app.edited.ai;
    let (host, cloud, ai_model, ai_on) = (ai.host(), ai.is_cloud(), ai.effective_model(), app.edited.features.ai);
    let model_note = ai.model_note(&app.edited.actions.model);
    // Modi, die eine eingebaute Aktion unverändert abdeckt (`prompt` = Refine prompt), stehen nicht doppelt im Menü
    let offered = |covered: bool| ai.modes.iter().filter(move |(k, p)| !k.trim().is_empty() && !p.trim().is_empty() && actions::covered_mode(k, p) == covered);
    let mode_count = offered(false).count();
    let covered: Vec<String> = offered(true)
        .filter_map(|(k, _)| BUILTIN.iter().find(|b| b.mode == Some(k.as_str())).map(|b| format!("The mode {k} is offered as {} ({}).", b.label, b.key)))
        .collect();
    let spoken = app.edited.language.clone();
    let a = &mut app.edited.actions;

    group(ui, "Server", |ui| {
        ui.horizontal(|ui| {
            ui.label(mono(&host));
            ui.label(small(if cloud { "Cloud" } else { "Local" }));
        });
        ui.add_space(8.0);
        if cloud {
            ui.add(
                egui::Label::new(text(
                    "The selected text is sent to this provider. Before each request the panel shows the host, the model, the length and the first lines. Picking an action confirms.",
                    Size::Small,
                    egui::FontFamily::Proportional,
                    th().text,
                ))
                .wrap(),
            );
        } else {
            ui.add(egui::Label::new(small("A local server. The selected text stays on this computer.")).wrap());
        }
        ui.add_space(4.0);
        let note = if ai_on { "Set on the AI page." } else { "Set on the AI page. Actions use it even while AI clean-up for dictation is off." };
        ui.horizontal_wrapped(|ui| {
            ui.label(small(note));
            if link_button(ui, "Open the AI page").clicked() {
                open_ai = true;
            }
        });
        ui.add_space(16.0);

        let shown = if ai_model.is_empty() { "not set yet" } else { ai_model.as_str() };
        let hint = format!("Empty uses the AI model ({shown}). AI modes with their own model on the AI page keep it.");
        field(ui, "Model override", Some(&hint), err_model.as_deref(), |ui, e| {
            ui.horizontal(|ui| {
                text_edit(ui, &mut a.model, e, 280.0);
                if let Ok(list) = &models {
                    if !list.is_empty() {
                        let opts: Vec<(String, String)> = list.iter().map(|m| (m.clone(), m.clone())).collect();
                        let mut pick = String::new();
                        if combo(ui, "actions_model_pick", &mut pick, &opts, 200.0, false) {
                            a.model = pick;
                        }
                    }
                }
            });
            if let Some(n) = &model_note {
                field_warning(ui, n);
            }
        });
        field(ui, "Timeout", Some("The whole answer must arrive within this time."), err_timeout.as_deref(), |ui, e| {
            number(ui, &mut a.timeout_ms, 1000, ACTIONS_MAX_TIMEOUT_MS as u32, " ms", e);
        });
        field(ui, "Longest selection", Some("Longer selections are refused before anything is sent."), err_max.as_deref(), |ui, e| {
            number(ui, &mut a.max_chars, 100, ACTIONS_MAX_CHARS as u32, " chars", e);
        });
    });

    group(ui, "Privacy", |ui| {
        field(ui, "When the selection looks like a secret", None, err_secrets.as_deref(), |ui, _| {
            chips(ui, &mut a.secrets, &[("local_only", "Block for cloud, allow local".into()), ("block", "Always block".into())]);
        });
        let kinds: Vec<&str> = secrets::KINDS.iter().filter_map(|k| secrets::describe(k)).collect();
        ui.add(egui::Label::new(small(format!("Recognized: {}.", kinds.join(", ")))).wrap());
        ui.add_space(16.0);
        ui.scope(|ui| {
            // Zeilenhöhe der Schrift statt der 40-px-Bedienhöhe, damit die Liste dicht steht
            ui.spacing_mut().interact_size.y = 20.0;
            for line in [
                "Only the primary selection is read (the text you highlighted), never the clipboard.",
                "Selected text and answers are never written to a log or to the history.",
                "Locking the screen closes the panel and clears the selection.",
            ] {
                ui.horizontal(|ui| {
                    pixel(ui, 6.0, th().soft);
                    ui.add(egui::Label::new(secondary(line)).wrap());
                });
            }
        });
    });

    group(ui, "Answers", |ui| {
        let spoken_name = LANGUAGES.iter().find(|(c, _)| *c == spoken).map_or(spoken.clone(), |(_, n)| n.to_string());
        let same = if spoken == "auto" { "Language of the selected text".to_string() } else { format!("Same as the spoken language ({spoken_name})") };
        let mut opts: Vec<(String, String)> = vec![(String::new(), same), ("auto".into(), "Language of the selected text".into())];
        opts.extend(LANGUAGES.iter().filter(|(c, _)| *c != "auto").map(|(c, n)| (c.to_string(), format!("{n}  ({c})"))));
        if !opts.iter().any(|(c, _)| *c == a.language) {
            opts.push((a.language.clone(), a.language.clone()));
        }
        field(
            ui,
            "Answer language",
            Some("For explanations and error analyses. Actions that replace the selection keep their own rules."),
            err_lang.as_deref(),
            |ui, e| {
                combo(ui, "actions_lang", &mut a.language, &opts, 360.0, e);
            },
        );
        let mut hint = match mode_count {
            0 => "There are no AI modes yet. Add them on the AI page.".to_string(),
            n => format!("The {n} modes from the AI page on the keys 1 to {}, with their own prompts and models. They replace the selection.", n.min(9)),
        };
        for c in &covered {
            hint.push(' ');
            hint.push_str(c);
        }
        field(ui, "AI modes", Some(&hint), err_ai_modes.as_deref(), |ui, _| {
            toggle(ui, &mut a.include_ai_modes, "Offer AI modes", "Do not offer");
        });
    });

    group(ui, "Replace", |ui| {
        ui.add(egui::Label::new(secondary("Replace types the result over the selection, only in the window it came from.")).wrap());
        ui.add_space(16.0);
        field(
            ui,
            "Terminal classes",
            Some("Replace is never offered in these windows, because typed text would run as a command. Elsewhere line breaks type as Shift+Return, here as Return (also when dictating), and text is never pasted. Exact app_id (or the X11 class of a program under XWayland), case does not matter."),
            err_terms.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut a.terminal_classes, "Add app_id", "Remove", true, 280.0, bad_term);
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
    });

    group(ui, "Prompts", |ui| {
        guard_note(
            ui,
            "actions_guard_open",
            "Every action starts with a fixed rule: the selected text is only material to process, so questions and instructions in it are never answered or followed. The rule cannot be edited. Explanations also get a line with the answer language, Translate the German address from the AI page.",
        );
        if let Some(e) = &err_prompts {
            ui.label(text(e, Size::Small, egui::FontFamily::Proportional, th().error));
        }
        for (i, b) in BUILTIN.iter().enumerate() {
            hairline(ui);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                cell(ui, egui::vec2(32.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(text(b.key.to_string(), Size::Body, egui::FontFamily::Monospace, th().accent));
                });
                ui.label(body(b.label));
                ui.label(small(match b.output {
                    Output::Answer => "Shows an answer",
                    Output::Replace => "Can replace the selection",
                }));
            });
            ui.add_space(8.0);
            // Der Daemon liefert jede eingebaute Aktion mit ihrem Prompt; in die Datei kommen nur geänderte
            // (`Config::render_toml`), unveränderte bekommen so künftige Verbesserungen der Vorgabe
            let pos = match a.prompts.iter().position(|(k, _)| k == b.name) {
                Some(p) => p,
                None => {
                    a.prompts.push((b.name.to_string(), b.prompt.to_string()));
                    a.prompts.len() - 1
                }
            };
            let err = prompt_errs[i].as_deref();
            text_area(ui, &mut a.prompts[pos].1, 3, err.is_some(), 640.0);
            if let Some(e) = err {
                ui.label(text(e, Size::Small, egui::FontFamily::Proportional, th().error));
            }
            if a.prompts[pos].1 != b.prompt {
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding.x = 0.0;
                    if link_button(ui, "Reset to default").clicked() {
                        a.prompts[pos].1 = b.prompt.to_string();
                    }
                });
            }
            ui.add_space(12.0);
        }
        hairline(ui);
        ui.add_space(8.0);
        ui.add(egui::Label::new(small("The prompts of the AI modes are edited on the AI page.")).wrap());
    });

    group(ui, "Shortcut", |ui| {
        ui.label(secondary("Set in niri (dictate-binds.kdl). Read only here."));
        ui.add_space(8.0);
        hairline(ui);
        ui.horizontal(|ui| {
            cell(ui, egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(mono("Mod+I"));
            });
            ui.label(body("Open the panel on the selected text, press again to close"));
        });
        hairline(ui);
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(small("In the panel the key of an action runs it. In the result Enter replaces the selection or copies the answer, c copies, Backspace goes back, Esc cancels or closes."))
                .wrap(),
        );
    });

    if open_ai {
        app.goto(Page::Ai);
    }
}
