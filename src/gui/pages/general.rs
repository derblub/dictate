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

/// Sprachen zur Auswahl (Code, englischer Name); auch für die Antwortsprache der Textaktionen.
pub const LANGUAGES: [(&str, &str); 14] = [
    ("de", "German"),
    ("en", "English"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("cs", "Czech"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("auto", "Detect automatically"),
];

/// Alle Kürzel aus `niri/dictate-binds.kdl` in derselben Reihenfolge (Lesart L47, Test `shortcuts_match_the_kdl`)
const SHORTCUTS: [(&str, &str); 7] = [
    ("Mod+E", "Hold to speak, release to type"),
    ("Mod+Shift+E", "Hands-free on or off"),
    ("Mod+Ctrl+E", "Cancel, type nothing"),
    ("Mod+Ctrl+Shift+E", "Open this window"),
    ("Mod+I", "Text actions on the selected text"),
    ("Mod+Shift+I", "Diagnose this computer"),
    ("Mod+Alt+Space", "Command palette"),
];

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "General", "Language and typing. Everything else lives on its own page.");
    let (e_lang, e_delay, e_support) = (app.err("language"), app.err("typing.delay_ms"), app.err("support_url"));
    let (e_output, e_kb) = (app.err("typing.output"), app.err_under("typing.keyboard_only_app_ids"));
    let bad_kb = app.err_index("typing.keyboard_only_app_ids");
    let s = &mut app.edited;

    group(ui, "Language", |ui| {
        let mut opts: Vec<(String, String)> = LANGUAGES.iter().map(|(c, n)| (c.to_string(), format!("{n}  ({c})"))).collect();
        if !opts.iter().any(|(c, _)| *c == s.language) {
            opts.push((s.language.clone(), s.language.clone()));
        }
        field(ui, "Spoken language", Some("Used by the recognizer. A profile can override it per app."), e_lang.as_deref(), |ui, e| {
            combo(ui, "lang", &mut s.language, &opts, 360.0, e);
        });
    });

    group(ui, "Typing", |ui| {
        field(ui, "Delay between keystrokes", Some("Raise it if an app drops characters."), e_delay.as_deref(), |ui, e| {
            number(ui, &mut s.typing.delay_ms, 0, 500, " ms", e);
        });
        field(ui, "Space after each dictation", None, None, |ui, _| {
            toggle(ui, &mut s.typing.trailing_space, "Add a space", "No space");
        });
        field(
            ui,
            "Without an input method (Wayland)",
            Some("When the text field does not accept the input method, outside terminals the text is pasted through the clipboard (your clipboard comes back afterwards) or typed key by key. Unknown windows and terminals are always typed."),
            e_output.as_deref(),
            |ui, _| {
                chips(ui, &mut s.typing.output, &[("paste", "Paste".to_string()), ("type", "Type".to_string())]);
            },
        );
        field(
            ui,
            "Apps that never get the input method (Wayland)",
            Some("Wayland app_id, exact, case does not matter. For apps that accept the input method but handle it badly."),
            e_kb.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut s.typing.keyboard_only_app_ids, "Add app_id", "Remove", true, 280.0, bad_kb);
            },
        );
    });

    group(ui, "Shortcuts", |ui| {
        ui.label(secondary("Set in niri (dictate-binds.kdl). Read only here."));
        ui.add_space(8.0);
        for (k, d) in SHORTCUTS {
            hairline(ui);
            ui.horizontal(|ui| {
                cell(ui, egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(mono(k));
                });
                ui.label(body(d));
            });
        }
        hairline(ui);
    });

    group(ui, "Support link", |ui| {
        field(ui, "Link shown on the About page", Some("Optional. Leave empty to hide it."), e_support.as_deref(), |ui, e| {
            text_edit(ui, &mut s.support_url, e, 520.0);
        });
    });

    group(ui, "File", |ui| {
        ui.label(mono(app.config_path.display().to_string()));
    });
}

#[cfg(test)]
mod tests {
    use super::SHORTCUTS;

    /// Kürzel im Block `binds` der KDL in ihrer Reihenfolge (Zeilen mit `spawn`, Kommentare ausgenommen)
    fn kdl_binds(kdl: &str) -> Vec<&str> {
        let start = kdl.find("\nbinds {").expect("binds-Block");
        let body = &kdl[start..kdl[start..].find("\n}").map(|e| start + e).expect("Ende von binds")];
        body.lines().map(|l| l.split("//").next().unwrap_or("").trim()).filter(|l| l.contains("spawn")).map(|l| l.split_whitespace().next().unwrap()).collect()
    }

    #[test]
    fn shortcuts_match_the_kdl() {
        let kdl = include_str!("../../../niri/dictate-binds.kdl");
        let page: Vec<&str> = SHORTCUTS.iter().map(|(k, _)| *k).collect();
        assert_eq!(page, kdl_binds(kdl), "Seite General und niri/dictate-binds.kdl");
        assert_eq!(kdl.matches("binds {").count(), 1, "ein binds-Block");
    }
}
