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

//! Seite Palette: Befehlspalette (`[palette]`, `PLAN-palette.md`). Server und Schlüssel kommen von der Seite AI, Frist
//! und Längengrenze von `ask` und `explain_clipboard` von der Seite Actions (Entscheidung: gemeinsam, keine eigenen
//! Felder); hier stehen Schalter, Modell, Frist der Absichten, Projektwurzeln, Suchmaschine und die Häufigkeit.
//! Wurzeln und Suchadresse prüft das Fenster schon beim Tippen mit denselben Funktionen wie `Config::validate`
//! ([`PaletteCfg::check_root`], [`PaletteCfg::check_search_url`]); beim Speichern meldet der Daemon dasselbe.

use crate::config::{PaletteCfg, PALETTE_DEFAULT_SEARCH_URL, PALETTE_DEPTH_RANGE, PALETTE_MAX_ROOTS, PALETTE_TIMEOUT_RANGE};
use crate::gui::dto::Palette;
use crate::gui::{net::Tag, theme::*, widgets::*, App, Page, Status};
use egui::Ui;
use serde_json::json;

/// Fehler, die das Fenster schon beim Tippen kennt, mit Text wie vom Daemon (`palette.project_roots[1]: …`).
#[derive(Debug, Default, PartialEq)]
pub struct Problems {
    /// je Wurzel; ein leerer Eintrag ist frisch angelegt und wird erst beim Speichern beanstandet
    pub roots: Vec<Option<String>>,
    pub too_many_roots: Option<String>,
    pub search_url: Option<String>,
    /// Die argv-Felder (`palette.terminal`, `project_command`, `clipboard_command`, `emoji_command`): Meldung und Index
    /// des Eintrags (ein frisch angelegter, leerer Eintrag erst beim Speichern)
    pub terminal: Option<(String, Option<usize>)>,
    pub project_command: Option<(String, Option<usize>)>,
    pub clipboard_command: Option<(String, Option<usize>)>,
    pub emoji_command: Option<(String, Option<usize>)>,
}

/// Prüft eine argv-Liste wie der Daemon ([`PaletteCfg::check_argv`]); ein leerer Eintrag ist frisch angelegt und zählt
/// erst beim Speichern. Er steht für die Prüfung als Platzhalter da, damit Fehler in anderen Einträgen ihren Index behalten.
fn argv_problem(name: &str, argv: &[String]) -> Option<(String, Option<usize>)> {
    let filled: Vec<String> = argv.iter().map(|a| if a.is_empty() { "x".to_string() } else { a.clone() }).collect();
    match PaletteCfg::check_argv(&filled) {
        Ok(()) => None,
        Err((Some(i), m)) => Some((format!("palette.{name}[{i}]: {m}"), Some(i))),
        Err((None, m)) => Some((format!("palette.{name}: {m}"), None)),
    }
}

pub fn check(p: &Palette) -> Problems {
    let roots = p
        .project_roots
        .iter()
        .enumerate()
        .map(|(i, r)| match r.trim().is_empty() {
            true => None,
            false => PaletteCfg::check_root(r).err().map(|m| format!("palette.project_roots[{i}]: {m}")),
        })
        .collect();
    let too_many_roots =
        (p.project_roots.len() > PALETTE_MAX_ROOTS).then(|| format!("palette.project_roots: at most {PALETTE_MAX_ROOTS} roots, got {}", p.project_roots.len()));
    let search_url = PaletteCfg::check_search_url(&p.web_search_url).err().map(|m| format!("palette.web_search_url: {m}"));
    Problems {
        roots,
        too_many_roots,
        search_url,
        terminal: argv_problem("terminal", &p.terminal),
        project_command: argv_problem("project_command", &p.project_command),
        clipboard_command: argv_problem("clipboard_command", &p.clipboard_command),
        emoji_command: argv_problem("emoji_command", &p.emoji_command),
    }
}

/// Zeile der Tastentabelle: Taste (Mono), Wirkung.
fn key_row(ui: &mut Ui, key: &str, what: &str) {
    hairline(ui);
    ui.horizontal(|ui| {
        cell(ui, egui::vec2(160.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(mono(key));
        });
        ui.add(egui::Label::new(body(what)).wrap());
    });
}

/// Auswahl der Suchtiefe als Chips 1 bis 4.
fn depth_chips(ui: &mut Ui, depth: &mut u32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for d in PALETTE_DEPTH_RANGE.0..=PALETTE_DEPTH_RANGE.1 {
            if chip(ui, &d.to_string(), *depth as usize == d).clicked() {
                *depth = d as u32;
            }
        }
    });
}

/// Ergebnis eines Knopfdrucks (Verlauf löschen, neu einlesen) neben dem Knopf.
fn notice(ui: &mut Ui, s: &Status) {
    match s {
        Status::Err(m) => {
            ui.add(egui::Label::new(text(m, Size::Small, egui::FontFamily::Proportional, th().error)).truncate());
        }
        Status::Ok(m) => {
            pixel(ui, 8.0, th().accent);
            ui.label(small(m));
        }
        Status::None => {}
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Palette", "One input line for windows and workspaces, apps and projects, web searches and questions. Mod+Alt+Space opens it.");
    feature_switch(ui, &mut app.edited.palette.enabled, "Enabled", "Disabled");
    if !app.edited.palette.enabled {
        ui.add(egui::Label::new(secondary("The palette is off. Turn it on and press Apply.")).wrap());
        return;
    }

    let problems = check(&app.edited.palette);
    let err_model = app.err("palette.model");
    let err_timeout = app.err("palette.timeout_ms");
    let err_depth = app.err("palette.project_depth");
    let err_roots =
        problems.too_many_roots.clone().or_else(|| problems.roots.iter().flatten().next().cloned()).or_else(|| app.err_under("palette.project_roots"));
    let bad_server_root = app.err_index("palette.project_roots");
    let err_url = problems.search_url.clone().or_else(|| app.err("palette.web_search_url"));
    let argv_err = |local: &Option<(String, Option<usize>)>, path: &str| {
        let msg = local.as_ref().map(|(m, _)| m.clone()).or_else(|| app.err_under(path));
        let bad = local.as_ref().and_then(|(_, i)| *i).or_else(|| app.err_index(path));
        (msg, bad)
    };
    let (err_terminal, bad_terminal) = argv_err(&problems.terminal, "palette.terminal");
    let (err_command, bad_command) = argv_err(&problems.project_command, "palette.project_command");
    let (err_clipboard, bad_clipboard) = argv_err(&problems.clipboard_command, "palette.clipboard_command");
    let (err_emoji, bad_emoji) = argv_err(&problems.emoji_command, "palette.emoji_command");
    let err_frecency = app.err("palette.frecency");
    let models = app.ai_models.clone();
    let (online, saved_on) = (app.online, app.saved.palette.enabled);
    let confirm = app.pal_confirm;
    let notice_for = |tag: Tag| app.pal_notice.as_ref().filter(|(t, _)| *t == tag).map(|(_, s)| s.clone());
    let (refresh_notice, clear_notice) = (notice_for(Tag::PalRefresh), notice_for(Tag::PalClear));
    let (mut open_ai, mut open_actions, mut clear, mut refresh) = (false, false, false, false);
    let mut set_confirm = None;

    let ai = &app.edited.ai;
    let (host, cloud, ai_model) = (ai.host(), ai.is_cloud(), ai.effective_model());
    let model_note = ai.model_note(&app.edited.palette.model);
    let p = &mut app.edited.palette;

    group(ui, "AI", |ui| {
        ui.horizontal(|ui| {
            ui.label(mono(&host));
            ui.label(small(if cloud { "Cloud" } else { "Local" }));
        });
        ui.add_space(8.0);
        let line = if cloud {
            "The AI turns a line that no entry matches clearly into a command. The line goes to this provider, see Privacy below."
        } else {
            "The AI turns a line that no entry matches clearly into a command. The server is local, so the line stays on this computer."
        };
        ui.add(egui::Label::new(small(line)).wrap());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(small("Set on the AI page."));
            if link_button(ui, "Open the AI page").clicked() {
                open_ai = true;
            }
        });
        ui.add_space(16.0);

        let shown = if ai_model.is_empty() { "not set yet" } else { ai_model.as_str() };
        let hint = format!("Empty uses the AI model ({shown}). Also used to answer questions and explain the clipboard.");
        field(ui, "Model", Some(&hint), err_model.as_deref(), |ui, e| {
            ui.horizontal(|ui| {
                text_edit(ui, &mut p.model, e, 280.0);
                if let Ok(list) = &models {
                    if !list.is_empty() {
                        let opts: Vec<(String, String)> = list.iter().map(|m| (m.clone(), m.clone())).collect();
                        let mut pick = String::new();
                        if combo(ui, "palette_model_pick", &mut pick, &opts, 200.0, false) {
                            p.model = pick;
                        }
                    }
                }
            });
            if let Some(n) = &model_note {
                field_warning(ui, n);
            }
        });
        field(ui, "Timeout", Some("How long the AI may take to turn a line into a command."), err_timeout.as_deref(), |ui, e| {
            number(ui, &mut p.timeout_ms, PALETTE_TIMEOUT_RANGE.0 as u32, PALETTE_TIMEOUT_RANGE.1 as u32, " ms", e);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(small("Questions (?) and Explain clipboard keep the timeout and the length limit of the Actions page."));
            if link_button(ui, "Open the Actions page").clicked() {
                open_actions = true;
            }
        });
    });

    group(ui, "Projects", |ui| {
        field(
            ui,
            "Project roots",
            Some("Git repositories under these folders show up as projects; starting one opens a terminal in its folder (see Terminal below). A leading ~ is your home folder."),
            err_roots.as_deref(),
            |ui, _| {
                string_list_flagged(ui, &mut p.project_roots, "Add root", "Remove", true, 400.0, |i| {
                    problems.roots.get(i).is_some_and(Option::is_some) || bad_server_root == Some(i)
                });
            },
        );
        field(
            ui,
            "Search depth",
            Some("1 looks directly inside each root, 2 one folder deeper. Worktrees under .claude/worktrees are skipped."),
            err_depth.as_deref(),
            |ui, _| depth_chips(ui, &mut p.project_depth),
        );
        ui.add_enabled_ui(online && saved_on, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if secondary_button(ui, "Rescan apps and projects").clicked() {
                    refresh = true;
                }
                if let Some(s) = &refresh_notice {
                    notice(ui, s);
                }
            });
        });
        ui.add(
            egui::Label::new(small(if online {
                "Apps and projects update on their own. Rescan reads them again right away."
            } else {
                "Rescan needs the running daemon."
            }))
            .wrap(),
        );
    });

    group(ui, "Terminal", |ui| {
        field(
            ui,
            "Terminal",
            Some("The terminal program, then its arguments up to the command it runs, one per row (for example alacritty, then -e; or kitty alone). Used for projects and for apps that need a terminal. Empty picks the first installed of xdg-terminal-exec, alacritty, kitty, foot, ghostty, wezterm, konsole, gnome-terminal and xterm."),
            err_terminal.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut p.terminal, "Add argument", "Remove", true, 400.0, bad_terminal);
            },
        );
        field(
            ui,
            "Command in projects",
            Some("What opening a project runs in the terminal, in the project folder, one argument per row (for example claude). Empty opens your shell."),
            err_command.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut p.project_command, "Add argument", "Remove", true, 400.0, bad_command);
            },
        );
    });

    group(ui, "Clipboard and emoji", |ui| {
        field(
            ui,
            "Clipboard history",
            Some("The program that shows your clipboard history, then its arguments, one per row (for example copyq, then toggle). Empty uses the clipboard panel of Noctalia. The palette offers the clipboard history only if the program is installed."),
            err_clipboard.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut p.clipboard_command, "Add argument", "Remove", true, 400.0, bad_clipboard);
            },
        );
        field(
            ui,
            "Emoji picker",
            Some("The program that lets you pick an emoji, then its arguments, one per row (for example bemoji). Empty uses rofi with the rofi-emoji plugin. The palette offers the emoji picker only if the program is installed."),
            err_emoji.as_deref(),
            |ui, _| {
                string_list_marked(ui, &mut p.emoji_command, "Add argument", "Remove", true, 400.0, bad_emoji);
            },
        );
    });

    group(ui, "Web search", |ui| {
        field(
            ui,
            "Search engine address",
            Some(
                "Used by \"g words\" and by searches the AI suggests. {query} marks where the words go: exactly once, in the path, the query or the fragment.",
            ),
            err_url.as_deref(),
            |ui, e| {
                mono_edit(ui, &mut p.web_search_url, e, 560.0);
            },
        );
        if p.web_search_url != PALETTE_DEFAULT_SEARCH_URL {
            ui.scope(|ui| {
                ui.spacing_mut().button_padding.x = 0.0;
                if link_button(ui, "Reset to default").clicked() {
                    p.web_search_url = PALETTE_DEFAULT_SEARCH_URL.to_string();
                }
            });
        }
    });

    group(ui, "Frequent picks", |ui| {
        field(
            ui,
            "Remember what you pick",
            Some("Entries you pick often move up. Only the target is counted (an app, a project, a command), never what you typed. Kept in ~/.local/state/dictate/frecency.json, readable only by you."),
            err_frecency.as_deref(),
            |ui, _| {
                toggle(ui, &mut p.frecency, "Remember", "Do not remember");
            },
        );
        ui.add_enabled_ui(online && saved_on, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if confirm {
                    if secondary_button(ui, "Really delete").clicked() {
                        clear = true;
                    }
                    if link_button(ui, "Keep").clicked() {
                        set_confirm = Some(false);
                    }
                } else if secondary_button(ui, "Clear history").clicked() {
                    set_confirm = Some(true);
                }
                if let Some(s) = &clear_notice {
                    notice(ui, s);
                }
            });
        });
        if !online {
            ui.add(egui::Label::new(small("Clearing needs the running daemon.")).wrap());
        } else if !saved_on {
            ui.add(egui::Label::new(small("Press Apply first: the running daemon still has the palette off.")).wrap());
        }
    });

    group(ui, "Privacy", |ui| {
        ui.scope(|ui| {
            // Zeilenhöhe der Schrift statt der 40-px-Bedienhöhe, damit die Liste dicht steht
            let row = ui.text_style_height(&egui::TextStyle::Body);
            ui.spacing_mut().interact_size.y = row;
            for line in [
                "To the AI go only the line you type or say and the list of commands, never window titles or the names of your apps and projects.",
                "A line that looks like a secret is never sent to a cloud server.",
                "The AI only picks a command from a fixed list. What runs is decided on this computer, never text the AI makes up.",
                "Explain clipboard is the only command that reads the clipboard. It shows the text first and sends it only after you confirm.",
            ] {
                ui.horizontal_top(|ui| {
                    // der Punkt (6 px) steht mittig auf der Höhe der ersten Zeile
                    ui.vertical(|ui| {
                        ui.add_space((row - 6.0) / 2.0);
                        pixel(ui, 6.0, th().soft);
                    });
                    ui.add(egui::Label::new(secondary(line)).wrap());
                });
            }
        });
    });

    group(ui, "Keys", |ui| {
        ui.label(secondary("Mod+Alt+Space opens the panel. Keys in the panel:"));
        ui.add_space(8.0);
        key_row(ui, "Enter", "Run the selected entry, or ask the AI when nothing matches clearly");
        key_row(ui, "Ctrl+Enter", "Ask the AI, even when an entry matches");
        key_row(ui, "Hold Mod+E", "Speak a command, release to put it in the line");
        key_row(ui, "Esc", "Cancel the AI or the recording, then close");
        hairline(ui);
        ui.add_space(8.0);
        ui.add(egui::Label::new(small("A spoken command only fills the line. Enter runs it, as if you had typed it.")).wrap());
    });

    if let Some(c) = set_confirm {
        app.pal_confirm = c;
    }
    if clear {
        app.pal_confirm = false;
        app.pal_notice = None;
        app.net.request(Tag::PalClear, json!({"cmd":"pal_frecency_clear"}));
    }
    if refresh {
        app.pal_notice = None;
        app.net.request(Tag::PalRefresh, json!({"cmd":"pal_sources_refresh"}));
    }
    if open_ai {
        app.goto(Page::Ai);
    }
    if open_actions {
        app.goto(Page::Actions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn palette(f: impl FnOnce(&mut Palette)) -> Palette {
        let mut p = Palette::default();
        f(&mut p);
        p
    }

    /// Meldung des Daemons beim Speichern (`Config::validate`) für dieselbe Seite.
    fn daemon_says(p: &Palette) -> Option<String> {
        Config::from_value(json!({ "palette": p })).err().map(|e| e.to_string())
    }

    #[test]
    fn defaults_have_no_problems() {
        assert_eq!(check(&Palette::default()), Problems { roots: vec![None], ..Problems::default() });
    }

    #[test]
    fn local_check_says_what_the_daemon_says() {
        for roots in [vec!["relative/dir"], vec!["/ok", "~user/x"], vec!["/a\nb"], vec!["~", "~/src", "/srv/git"]] {
            let p = palette(|p| p.project_roots = roots.iter().map(|r| r.to_string()).collect());
            let local = check(&p).roots.into_iter().flatten().next();
            assert_eq!(local, daemon_says(&p), "{roots:?}");
        }
        let many = palette(|p| p.project_roots = (0..=PALETTE_MAX_ROOTS).map(|i| format!("/r{i}")).collect());
        assert_eq!(check(&many).too_many_roots, daemon_says(&many));
        for url in [
            "http://duckduckgo.com/?q={query}",
            "https://duckduckgo.com/",
            "https://{query}.example.org/",
            "https://example.org/?q={query}&r={query}",
            "https://example.org/s?q={query}",
        ] {
            let p = palette(|p| p.web_search_url = url.into());
            assert_eq!(check(&p).search_url, daemon_says(&p), "{url}");
        }
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let many: Vec<String> = (0..=crate::config::PALETTE_MAX_ARGV).map(|i| format!("a{i}")).collect();
        for list in [argv(&["-e"]), argv(&["bin/term"]), argv(&["kitty", "a\nb"]), argv(&[" kitty"]), many, argv(&["alacritty", "-e"])] {
            let p = palette(|p| p.terminal = list.clone());
            assert_eq!(check(&p).terminal.map(|(m, _)| m), daemon_says(&p), "{list:?}");
            let p = palette(|p| p.project_command = list.clone());
            assert_eq!(check(&p).project_command.map(|(m, _)| m), daemon_says(&p), "{list:?}");
            let p = palette(|p| p.clipboard_command = list.clone());
            assert_eq!(check(&p).clipboard_command.map(|(m, _)| m), daemon_says(&p), "{list:?}");
            let p = palette(|p| p.emoji_command = list.clone());
            assert_eq!(check(&p).emoji_command.map(|(m, _)| m), daemon_says(&p), "{list:?}");
        }
        // ein frisch angelegter leerer Eintrag erst beim Speichern
        let p = palette(|p| p.terminal = argv(&["kitty", ""]));
        assert_eq!(check(&p).terminal, None);
        assert_eq!(daemon_says(&p).as_deref(), Some("palette.terminal[1]: must not be empty"));
        // … verdeckt aber keinen Fehler davor: „-e“ bleibt rot, auch mit einer leeren Zeile dahinter
        let p = palette(|p| p.terminal = argv(&["-e", ""]));
        assert_eq!(check(&p).terminal.map(|(_, i)| i), Some(Some(0)));
        let p = palette(|p| p.terminal = argv(&["kitty -e"]));
        assert_eq!(check(&p).terminal.map(|(m, _)| m), daemon_says(&p));
    }

    #[test]
    fn a_new_empty_root_is_not_an_error_yet() {
        // „Add root“ legt einen leeren Eintrag an; rot wird er erst, wenn der Daemon ihn beim Speichern ablehnt
        let p = palette(|p| p.project_roots.push(String::new()));
        assert_eq!(check(&p).roots, vec![None, None]);
        assert_eq!(daemon_says(&p).as_deref(), Some("palette.project_roots[1]: must not be empty"));
    }

    #[test]
    fn two_bad_roots_are_both_flagged() {
        let p = palette(|p| p.project_roots = vec!["a".into(), "/ok".into(), "b".into()]);
        let flags: Vec<bool> = check(&p).roots.iter().map(Option::is_some).collect();
        assert_eq!(flags, [true, false, true]);
    }
}
