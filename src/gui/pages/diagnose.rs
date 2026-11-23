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

//! Seite Diagnose: Linux-Diagnose (`[diagnose]`, `PLAN-diagnose.md` Aufgabe 10). Server und Schlüssel kommen von der
//! Seite AI; hier stehen Schalter, Modell, Frist, Größe des Berichts, Runden, die Quellen (Ordner des temp-loggers,
//! Ping-Ziele), das Pseudonymisieren und der Hinweis nach einem unsauberen Boot-Ende. Die Seite erklärt auch bei
//! ausgeschalteter Diagnose, was gesammelt wird und was vor dem Senden passiert: das liest man, bevor man einschaltet.
//! Alle Felder bleiben bedienbar (auch ausgeschaltet), damit ein Fehler aus einer von Hand geschriebenen Datei sichtbar
//! und behebbar ist.
//!
//! Ordner und Ping-Ziele prüft das Fenster schon beim Tippen mit denselben Funktionen wie `Config::validate`
//! ([`DiagnoseCfg::check_temp_log_dir`], [`DiagnoseCfg::check_ping_target_name`], [`PingTargetEntry::check_address`]);
//! beim Speichern meldet der Daemon dasselbe. Ein frisch angelegtes, noch leeres Feld gilt erst beim Speichern als Fehler.

use crate::config::{
    DiagnoseCfg, PingTargetEntry, DIAGNOSE_MAX_PING_TARGETS, DIAGNOSE_PROFILES, DIAGNOSE_REPORT_KIB_RANGE, DIAGNOSE_ROUNDS_RANGE, DIAGNOSE_TIMEOUT_RANGE,
};
use crate::gui::{layout_log, theme::*, widgets::*, App, Page};
use egui::Ui;

/// Fehler, die das Fenster schon beim Tippen kennt, mit Text wie vom Daemon (`diagnose.ping_targets[1].name: …`).
#[derive(Debug, Default, PartialEq)]
pub struct Problems {
    /// je Ziel: Meldung zum Namen und zur Adresse; ein leeres Feld ist frisch angelegt und wird erst beim Speichern beanstandet
    pub targets: Vec<(Option<String>, Option<String>)>,
    pub too_many_targets: Option<String>,
    pub temp_log_dir: Option<String>,
}

impl Problems {
    /// Die erste Meldung zur Liste der Ziele (Zahl, Name, Adresse).
    fn first_target(&self) -> Option<String> {
        self.too_many_targets.clone().or_else(|| self.targets.iter().flat_map(|(n, a)| [n, a]).flatten().next().cloned())
    }
}

pub fn check(d: &DiagnoseCfg) -> Problems {
    let targets = d
        .ping_targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let at = |field: &str, m: String| format!("diagnose.ping_targets[{i}].{field}: {m}");
            let name = if t.name.is_empty() { None } else { d.check_ping_target_name(i).err().map(|m| at("name", m)) };
            let address = if t.address.is_empty() { None } else { PingTargetEntry::check_address(&t.address).err().map(|m| at("address", m)) };
            (name, address)
        })
        .collect();
    let too_many_targets = (d.ping_targets.len() > DIAGNOSE_MAX_PING_TARGETS)
        .then(|| format!("diagnose.ping_targets: at most {DIAGNOSE_MAX_PING_TARGETS} targets, got {}", d.ping_targets.len()));
    let temp_log_dir = DiagnoseCfg::check_temp_log_dir(&d.temp_log_dir).err().map(|m| format!("diagnose.temp_log_dir: {m}"));
    Problems { targets, too_many_targets, temp_log_dir }
}

/// Zeile einer Tabelle ohne Spaltenkopf: Name, Erklärung.
fn table_row(ui: &mut Ui, name: &str, what: &str) {
    hairline(ui);
    ui.horizontal(|ui| {
        cell(ui, egui::vec2(160.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(body(name));
        });
        ui.add(egui::Label::new(secondary(what)).wrap());
    });
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

/// Liste mit grauem Punkt vor jedem Eintrag.
fn bullets(ui: &mut Ui, lines: &[&str]) {
    ui.scope(|ui| {
        // Zeilenhöhe der Schrift statt der 40-px-Bedienhöhe, damit die Liste dicht steht
        let row = ui.text_style_height(&egui::TextStyle::Body);
        ui.spacing_mut().interact_size.y = row;
        for line in lines {
            ui.horizontal_top(|ui| {
                // der Punkt (6 px) steht mittig auf der Höhe der ersten Zeile
                ui.vertical(|ui| {
                    ui.add_space((row - 6.0) / 2.0);
                    pixel(ui, 6.0, th().soft);
                });
                ui.add(egui::Label::new(secondary(*line)).wrap());
            });
        }
    });
}

/// Auswahl der Runden als Chips.
fn rounds_chips(ui: &mut Ui, rounds: &mut u32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for r in DIAGNOSE_ROUNDS_RANGE.0..=DIAGNOSE_ROUNDS_RANGE.1 {
            if chip(ui, &r.to_string(), *rounds == r).clicked() {
                *rounds = r;
            }
        }
    });
}

/// Breite der Felder der Ping-Ziele; die Kopfzeile richtet sich nach der Spalte Name.
const NAME_WIDTH: f32 = 180.0;
const ADDRESS_WIDTH: f32 = 300.0;

/// Tabelle der Ping-Ziele: Name, Adresse, Entfernen. `flags[i]` = (Name fehlerhaft, Adresse fehlerhaft).
fn ping_targets(ui: &mut Ui, targets: &mut Vec<PingTargetEntry>, flags: &[(bool, bool)]) {
    let mut remove = None;
    if !targets.is_empty() {
        ui.horizontal(|ui| {
            cell(ui, egui::vec2(NAME_WIDTH, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(small("Name, sent to the AI"));
            });
            ui.label(small("Address, stays here"));
        });
    }
    for (i, t) in targets.iter_mut().enumerate() {
        let (bad_name, bad_address) = flags.get(i).copied().unwrap_or_default();
        ui.horizontal(|ui| {
            let name = mono_edit_hint(ui, &mut t.name, "nas", bad_name, NAME_WIDTH);
            layout_log::note(ui, || format!("diagnose.ping_targets[{i}].name"), &name);
            let address = mono_edit_hint(ui, &mut t.address, "192.0.2.10 or nas.example.org", bad_address, ADDRESS_WIDTH);
            layout_log::note(ui, || format!("diagnose.ping_targets[{i}].address"), &address);
            let r = link_button(ui, "Remove");
            layout_log::note(ui, || format!("diagnose.ping_targets[{i}].remove"), &r);
            if r.clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        targets.remove(i);
    }
    ui.add_enabled_ui(targets.len() < DIAGNOSE_MAX_PING_TARGETS, |ui| {
        let r = secondary_button(ui, "Add target");
        layout_log::note(ui, || "diagnose.add_target".into(), &r);
        if r.clicked() {
            targets.push(PingTargetEntry { name: String::new(), address: String::new() });
        }
    });
}

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(
        ui,
        "Diagnose",
        "Find out why something broke. It reads the state and the logs of this computer, shows you the report and asks the AI for likely causes. Open it with Mod+Shift+I.",
    );
    feature_switch(ui, &mut app.edited.diagnose.enabled, "Enabled", "Disabled");
    if !app.edited.diagnose.enabled {
        ui.add(egui::Label::new(secondary("Diagnose is off and nothing is collected. Turn it on and press Apply.")).wrap());
        ui.add_space(24.0);
    }

    let problems = check(&app.edited.diagnose);
    let err_model = app.err("diagnose.model");
    let err_timeout = app.err("diagnose.timeout_ms");
    let err_kib = app.err("diagnose.report_kib");
    let err_rounds = app.err("diagnose.rounds");
    let err_dir = problems.temp_log_dir.clone().or_else(|| app.err("diagnose.temp_log_dir"));
    let err_pseudo = app.err("diagnose.pseudonymize");
    let err_hint = app.err("diagnose.boot_hint");
    // der Daemon meldet Name und Adresse eines Ziels einzeln (`diagnose.ping_targets.1.name`)
    let server_flags: Vec<(bool, bool)> = (0..app.edited.diagnose.ping_targets.len())
        .map(|i| (app.err(&format!("diagnose.ping_targets.{i}.name")).is_some(), app.err(&format!("diagnose.ping_targets.{i}.address")).is_some()))
        .collect();
    let flags: Vec<(bool, bool)> = server_flags
        .iter()
        .enumerate()
        .map(|(i, (n, a))| {
            let (ln, la) = problems.targets.get(i).map_or((false, false), |(n, a)| (n.is_some(), a.is_some()));
            (*n || ln, *a || la)
        })
        .collect();
    let err_targets = problems.first_target().or_else(|| app.err_under("diagnose.ping_targets"));
    let models = app.ai_models.clone();
    let mut open_ai = false;

    let ai = &app.edited.ai;
    let (host, cloud) = (ai.host(), ai.is_cloud());
    let default_model = ai.diagnose_model("");
    // dieselbe Regel wie `DiagnoseCfg::model_for`: bei einem Claude-Server gilt Sonnet 5.5 statt des Modells von `ai`
    let claude_server = ai.default_model() == crate::config::ANTHROPIC_DEFAULT_MODEL;
    let model_note = ai.model_note(&app.edited.diagnose.model);
    let d = &mut app.edited.diagnose;

    group(ui, "What it does", |ui| {
        ui.add(egui::Label::new(secondary("In the panel you pick what to look at. Each choice is a set of read-only checks:")).wrap());
        ui.add_space(8.0);
        for p in &DIAGNOSE_PROFILES {
            table_row(ui, p.label, p.description);
        }
        hairline(ui);
        ui.add_space(16.0);
        bullets(
            ui,
            &[
                "Read only. Every check is a fixed command with fixed options. Neither you nor the AI types a command, and sudo is never used.",
                "The panel shows the whole report before anything is sent. Only that text goes out, and only after you press Enter. You can leave sections out.",
                "Passwords, keys and tokens in the report are blacked out first, also for a local server.",
                "The AI can suggest more checks from the same fixed list. Each one runs only after you confirm it.",
                "Nothing is stored. The report and the answer stay in memory for at most 30 minutes and are never written to a log or to the history. Copy report puts the report on the clipboard when you ask.",
            ],
        );
    });

    group(ui, "AI", |ui| {
        ui.horizontal(|ui| {
            ui.label(mono(&host));
            ui.label(small(if cloud { "Cloud" } else { "Local" }));
        });
        ui.add_space(8.0);
        let line = if cloud {
            "The report goes to this provider, after you have seen it in the panel. The panel names the host and the model first."
        } else {
            "The server is local, but the panel still shows the report first, and only that text is sent."
        };
        ui.add(egui::Label::new(small(line)).wrap());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(small("Server and key are set on the AI page."));
            if link_button(ui, "Open the AI page").clicked() {
                open_ai = true;
            }
        });
        ui.add_space(16.0);

        let hint = match (default_model.as_str(), claude_server) {
            ("", _) => "Empty uses the model of the AI page, which is not set yet.".to_string(),
            (m, true) => format!("Empty uses {m}: the default for a Claude server, because the report is long."),
            (m, false) => format!("Empty uses {m}, the model of the AI page."),
        };
        field(ui, "Model", Some(&hint), err_model.as_deref(), |ui, e| {
            ui.horizontal(|ui| {
                text_edit(ui, &mut d.model, e, 280.0);
                if let Ok(list) = &models {
                    if !list.is_empty() {
                        let opts: Vec<(String, String)> = list.iter().map(|m| (m.clone(), m.clone())).collect();
                        let mut pick = String::new();
                        if combo(ui, "diagnose_model_pick", &mut pick, &opts, 200.0, false) {
                            d.model = pick;
                        }
                    }
                }
            });
            if let Some(n) = &model_note {
                field_warning(ui, n);
            }
        });
        field(ui, "Timeout", Some("How long the AI may take for one round."), err_timeout.as_deref(), |ui, e| {
            number(ui, &mut d.timeout_ms, DIAGNOSE_TIMEOUT_RANGE.0, DIAGNOSE_TIMEOUT_RANGE.1, " ms", e);
        });
        field(
            ui,
            "Report size",
            Some("The most the AI gets of the report. Every check has its own share, long logs are cut at the start and repeated lines are merged. A bigger report costs more and takes longer."),
            err_kib.as_deref(),
            |ui, e| {
                number(ui, &mut d.report_kib, DIAGNOSE_REPORT_KIB_RANGE.0, DIAGNOSE_REPORT_KIB_RANGE.1, " KiB", e);
            },
        );
        field(
            ui,
            "Rounds",
            Some("The first answer, plus one more round each time you confirm checks the AI suggests. With 1 the AI answers once and suggests no checks."),
            err_rounds.as_deref(),
            |ui, _| rounds_chips(ui, &mut d.rounds),
        );
    });

    group(ui, "Sources", |ui| {
        field(
            ui,
            "Temperature log folder",
            Some("If you run a temperature logger: the folder of its CSV files (temps-YYYY-MM-DD.csv, first column the local time). Only files with that name are read. Empty = no temperature log. A leading ~ is your home folder."),
            err_dir.as_deref(),
            |ui, e| {
                mono_edit(ui, &mut d.temp_log_dir, e, 560.0);
            },
        );
        field(
            ui,
            "Ping targets",
            Some("The name is sent to the AI; the address is not. Only these targets are ever pinged, and only after you confirm a check that names one. With none, the AI cannot suggest a ping."),
            err_targets.as_deref(),
            |ui, _| ping_targets(ui, &mut d.ping_targets, &flags),
        );
    });

    group(ui, "Personal data", |ui| {
        field(
            ui,
            "Placeholders",
            Some("Your user name, host name, IP and MAC addresses, Wi-Fi names, e-mail addresses and home folders become placeholders such as user-1 or ip-3, the same value always the same placeholder. The answer is shown with the placeholders; press p in the panel to see the real values. Names in free text can slip through, so look at the preview."),
            err_pseudo.as_deref(),
            |ui, _| {
                toggle(ui, &mut d.pseudonymize, "Replace with placeholders", "Send as is");
            },
        );
        if !d.pseudonymize {
            let to = if cloud { host.as_str() } else { "the local server" };
            ui.add(
                egui::Label::new(text(
                    format!("Off: these values go to {to} as they are. Passwords, keys and tokens are still blacked out."),
                    Size::Small,
                    egui::FontFamily::Proportional,
                    th().text,
                ))
                .wrap(),
            );
        }
    });

    group(ui, "Notice", |ui| {
        field(
            ui,
            "After an unclean shutdown",
            Some("When the previous boot ended without a normal shutdown (crash, freeze, power loss), the overlay says so once after login: \"Last boot ended uncleanly · Mod+Shift+I to diagnose\". While it shows, a click on it opens the diagnosis, like Mod+Shift+I. Nothing is collected or sent; only the end of the previous boot's journal is looked at, once per boot. Needs Diagnose to be enabled."),
            err_hint.as_deref(),
            |ui, _| {
                toggle(ui, &mut d.boot_hint, "Show the notice", "Do not show");
            },
        );
    });

    group(ui, "Keys", |ui| {
        ui.label(secondary("Mod+Shift+I opens and closes the panel. Keys in the panel:"));
        ui.add_space(8.0);
        key_row(ui, "Enter", "Collect, then send the report as shown");
        key_row(ui, "Space", "Tick an entry, leave a section out, pick a check");
        key_row(ui, "Tab", "Type what is wrong (in the preview); Enter applies it");
        key_row(ui, "c", "Copy the report, without sending it");
        key_row(ui, "p", "Show real values instead of placeholders in the answer");
        key_row(ui, "Esc", "Stop and go back, or close without sending");
        hairline(ui);
    });

    if open_ai {
        app.goto(Page::Ai);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use serde_json::json;

    fn diag(f: impl FnOnce(&mut DiagnoseCfg)) -> DiagnoseCfg {
        let mut d = DiagnoseCfg::default();
        f(&mut d);
        d
    }

    fn target(name: &str, address: &str) -> PingTargetEntry {
        PingTargetEntry { name: name.into(), address: address.into() }
    }

    /// Meldung des Daemons beim Speichern (`Config::validate`) für dieselbe Seite.
    fn daemon_says(d: &DiagnoseCfg) -> Option<String> {
        Config::from_value(json!({ "diagnose": d })).err().map(|e| e.to_string())
    }

    #[test]
    fn defaults_have_no_problems() {
        assert_eq!(check(&DiagnoseCfg::default()), Problems::default());
    }

    #[test]
    fn local_check_says_what_the_daemon_says() {
        for dir in ["/var/log/temps", "~/logs", "relative/dir", "/a/../b", "", "  ", "/a\nb", "~user/x"] {
            let d = diag(|d| d.temp_log_dir = dir.into());
            // leer heißt „kein Temperaturlogger“ und ist gültig, bei beiden
            assert_eq!(check(&d).temp_log_dir, daemon_says(&d), "{dir:?}");
        }
        let long_name = "x".repeat(65);
        let cases: [(&str, &str); 9] = [
            ("nas", "nas.example.org"),
            ("gateway", "192.0.2.1"),
            ("v6", "2001:db8::1"),
            ("-bad", "192.0.2.1"),
            ("has space", "192.0.2.1"),
            ("nas", "-oProxyCommand"),
            ("nas", "not a host"),
            ("nas", "a..b"),
            (long_name.as_str(), "192.0.2.1"),
        ];
        for (name, address) in cases {
            let d = diag(|d| d.ping_targets = vec![target(name, address)]);
            assert_eq!(check(&d).first_target(), daemon_says(&d), "{name:?} {address:?}");
        }
        let many = diag(|d| d.ping_targets = (0..=DIAGNOSE_MAX_PING_TARGETS).map(|i| target(&format!("t{i}"), "192.0.2.1")).collect());
        assert_eq!(check(&many).too_many_targets, daemon_says(&many));
    }

    #[test]
    fn a_new_empty_target_is_not_an_error_yet() {
        // „Add target“ legt einen leeren Eintrag an; rot wird er erst, wenn der Daemon ihn beim Speichern ablehnt
        let d = diag(|d| d.ping_targets = vec![target("nas", "nas.example.org"), target("", "")]);
        assert_eq!(check(&d).targets, vec![(None, None), (None, None)]);
        assert_eq!(daemon_says(&d).as_deref(), Some("diagnose.ping_targets[1].name: must not be empty"));
        // halb ausgefüllt: das gefüllte Feld wird schon geprüft, das leere erst beim Speichern
        let d = diag(|d| d.ping_targets = vec![target("-x", "")]);
        assert_eq!(check(&d).targets[0].1, None);
        assert!(check(&d).targets[0].0.is_some());
    }

    #[test]
    fn name_and_address_are_flagged_independently() {
        let d = diag(|d| d.ping_targets = vec![target("-bad", "not a host"), target("ok", "192.0.2.1"), target("OK", "192.0.2.2")]);
        let flags: Vec<(bool, bool)> = check(&d).targets.iter().map(|(n, a)| (n.is_some(), a.is_some())).collect();
        // das Fenster markiert beide Felder der ersten Zeile zugleich, der Daemon nennt nur das erste
        assert_eq!(flags, [(true, true), (false, false), (true, false)]);
        assert!(check(&d).targets[2].0.as_deref().unwrap().contains("duplicate target name"));
        assert_eq!(daemon_says(&d).as_deref(), check(&d).targets[0].0.as_deref());
    }
}
