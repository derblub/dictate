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

//! Ansichten des Diagnose-Panels (PLAN-wayland.md 6e Punkt 3, „Rest Block B“ Punkt 2): zeichnet den Automaten
//! `ui::diag_panel` in die gemeinsame Hülle [`PanelUi`] (Fenster `dictate-panel`, Quelle [`Source::Diagnose`]). Texte und
//! Aufbau wie `render_*` im früheren `diagnose.lua` (Zeilen 579–1235).
//!
//! Jedes Stück aus Bericht, Fortschritt und Antwort geht durch `markdown::esc` (sonst scheitert `set_markup`, und die
//! Vorschau zeigte nicht, was gesendet wird). Kein Widget je Berichtszeile: je Abschnitt ein klickbarer Kopf (`sec:S<n>`)
//! und ein Mono-Label für den Körper. Bedienelemente tragen die Kennungen des Automaten (`profile:N`, `unit:N`, `item:N`,
//! `check:N`, `ev:F:E`, `sec:S<n>`, `symptom`), nie Namen.
//!
//! Eingabefeld „What is wrong?“: ein `gtk::Entry` an festem Platz der Hülle (`PanelUi::field`), nur in der Vorschau
//! sichtbar; solange der User darin tippt, setzt der Neuaufbau seinen Text nicht. Lesart (von Daniel am 21.11. bestätigt):
//! Eine Auswahl im Feld setzt PRIMARY (GTK), Einfügen per Strg+V bzw. Mittelklick liest CLIPBOARD bzw. PRIMARY – beides
//! nur auf ausdrückliche Handlung des Users im Feld, wie Tippen; dictate-ui liest dabei selbst nichts.
//!
//! Inhalte nie loggen: das Debug-Log nennt nur Ansicht, Dauer und Zahlen.

use crate::panel::{chip, clickable, hbox, label, markup, vbox, PanelUi, Source, NO_HYPHENS};
use crate::ui::diag_notes::{self, Counts, PhLook, NOTES_WARNING};
use crate::ui::diag_panel::{self, line_number, mark_line, params_of, split_report, DiagPanel, Mark, View};
use crate::ui::markdown::{self, Block};
use crate::ui::panel::{plural, ItemKind};
use gtk::prelude::*;
use gtk::{glib, pango};
use std::time::Instant;
use tracing::debug;

/// Status einer Probe im Fortschritt (`STATUS_TEXT` 714)
fn status_text(s: &str) -> &str {
    match s {
        "timeout" => "timed out",
        "needs_confirmation" => "needs confirmation",
        other => other,
    }
}

/// Bytes lesbar (`kib` in Lua)
pub fn kib(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

fn span(color: &str, text: &str) -> String {
    format!("<span foreground=\"{color}\">{}</span>", markdown::esc(text))
}

fn check_box(on: bool) -> gtk::Label {
    let l = gtk::Label::new(Some(if on { "✓" } else { "" }));
    l.add_css_class("dp-box");
    if on {
        l.add_css_class("on");
    }
    l.set_halign(gtk::Align::Center);
    l.set_valign(gtk::Align::Center);
    l
}

/// Eine Zeile mit Kennung: Kopf (Chip, Kästchen), Text (Markup) mit Unterzeile, rechts Nebentext
struct Row<'a> {
    id: String,
    lead: Option<gtk::Widget>,
    text: String,
    sub: Option<&'a str>,
    meta: Option<&'a str>,
    enabled: bool,
}

impl PanelUi {
    /// Inhalt der Diagnose ganz neu aufbauen
    pub fn render_diag(&mut self, d: &mut DiagPanel) {
        let t0 = Instant::now();
        // gleicher Bericht in der Vorschau (↑/↓, Hover, Feld): der Bericht bleibt stehen, samt Messung (bei 48 KiB
        // kostete das Messen des neuen Berichts rund 650 ms)
        let shown_preview = self.shown.as_ref().is_some_and(|(s, v)| *s == Source::Diagnose && v == "Preview");
        let keep = (d.view == View::Preview && shown_preview)
            .then(|| self.report.as_ref().filter(|r| Some(&r.key) == report_key(d).as_ref()).map(|r| r.doc.clone().upcast::<gtk::Widget>()))
            .flatten();
        let changed = self.begin_keeping(Source::Diagnose, format!("{:?}", d.view), keep.as_ref());
        if matches!(d.view, View::Progress | View::Send) {
            self.start_secs();
        } else {
            self.stop_secs();
        }
        self.set_title(&self.diag_title(d));
        self.render_field(d);
        match d.view {
            View::Closed => {}
            View::Loading => self.body.append(&label("Loading the profiles…", &["dp-soft"])),
            View::Profiles => self.render_profiles(d),
            View::Items => self.render_items(d),
            View::Progress => self.render_progress(d),
            View::Preview => self.render_preview(d, keep.is_some()),
            View::Send => self.render_sending(d),
            View::Answer => self.render_answer(d),
            View::Error => self.render_diag_error(d),
        }
        if let Some((n, err)) = &d.notice {
            if matches!(d.view, View::Preview | View::Answer | View::Error) {
                let l = label(n, &[if *err { "dp-error" } else { "dp-soft" }]);
                l.set_margin_top(12);
                self.body.append(&l);
            }
        }
        // wie bei den Textaktionen: ohne Erkennung der Sperre bleiben Bericht, Klartext und Antwort beim Sperren stehen
        if self.lock_off() && d.view != View::Closed {
            let l = label("Lock detection is off: the report and the answer are not cleared when the screen locks.", &["dp-meta", "dp-error"]);
            l.set_margin_top(12);
            self.body.append(&l);
            debug!("diagnose: lock detection off notice shown");
        }
        self.render_diag_buttons(d);
        let built = t0.elapsed().as_millis();
        self.finish(d.active.as_deref(), changed, changed);
        if d.view == View::Preview {
            if let Some(want) = d.take_scroll() {
                self.scroll_to_section(&want.section, want.line);
            }
        }
        let kept = if keep.is_some() { ", report kept" } else { "" };
        debug!("diagnose: rendered {:?} in {} ms (content {built} ms{kept})", d.view, t0.elapsed().as_millis());
    }

    fn diag_title(&self, d: &DiagPanel) -> String {
        let profile = || d.profile.as_ref().and_then(|n| d.profiles.iter().find(|p| &p.name == n)).map(|p| format!("Diagnose · {}", p.label));
        match d.view {
            View::Items | View::Error => profile().unwrap_or_else(|| "Diagnose".into()),
            View::Progress if d.check_round => "Diagnose · Checks".into(),
            View::Progress => profile().unwrap_or_else(|| "Diagnose".into()),
            View::Preview => match d.preview.as_ref().map(|p| p.rounds).filter(|r| *r > 1) {
                Some(r) => format!("Diagnose · Report · round {r}"),
                None => "Diagnose · Report".into(),
            },
            View::Send | View::Answer => "Diagnose · Answer".into(),
            _ => "Diagnose".into(),
        }
    }

    /// Eingabefeld: nur in der Vorschau sichtbar; der Text folgt dem Automaten, außer der User tippt gerade
    fn render_field(&mut self, d: &DiagPanel) {
        if d.view != View::Preview {
            self.set_field_text("");
            self.field_box.set_visible(false);
            return;
        }
        let max = d.preview.as_ref().map_or(diag_panel::MAX_SYMPTOM_CHARS, |p| p.max_symptom_chars);
        self.field.set_max_length(i32::try_from(max).unwrap_or(i32::MAX));
        if !d.field.editing {
            self.set_field_text(&d.field.text);
        }
        let enabled = d.items().iter().find(|i| i.id == "symptom").is_some_and(|i| i.enabled);
        self.field.set_sensitive(enabled);
        self.field_box.set_visible(true);
        self.items.insert("symptom".into(), self.field.clone().upcast());
    }

    fn row(&mut self, r: Row) -> gtk::Box {
        let row = hbox(16);
        row.add_css_class("dp-row");
        if !r.enabled {
            row.add_css_class("disabled");
        }
        if let Some(l) = r.lead {
            row.append(&l);
        }
        let col = vbox(2);
        col.set_hexpand(true);
        col.set_valign(gtk::Align::Center);
        col.append(&markup(&r.text, &[]));
        if let Some(s) = r.sub.filter(|s| !s.is_empty()) {
            col.append(&label(s, &["dp-meta"]));
        }
        row.append(&col);
        if let Some(m) = r.meta {
            let l = label(m, &["dp-meta"]);
            l.set_wrap(false);
            l.set_valign(gtk::Align::Center);
            row.append(&l);
        }
        if r.enabled {
            clickable(&row, &r.id, &self.tx);
        }
        self.items.insert(r.id, row.clone().upcast());
        self.body.append(&row);
        row
    }

    fn group(&self, text: &str, meta: Option<String>) {
        let h = hbox(8);
        h.add_css_class("dp-group");
        let l = label(&text.to_uppercase(), &[]);
        l.set_hexpand(true);
        h.append(&l);
        if let Some(m) = meta {
            let l = label(&m, &["dp-meta"]);
            l.set_wrap(false);
            h.append(&l);
        }
        self.body.append(&h);
    }

    fn para(&self, text: &str, classes: &[&str], top: i32) {
        let l = label(text, classes);
        l.set_margin_top(top);
        self.body.append(&l);
    }

    fn mono(text: &str) -> String {
        format!("<span font_family=\"monospace\">{}</span>", markdown::esc(text))
    }

    // ---- Ansichten -----------------------------------------------------------------------------------------------

    fn render_profiles(&mut self, d: &DiagPanel) {
        self.para("Collects read-only information about this machine. Nothing is sent until you check the report and press Enter.", &["dp-soft"], 0);
        self.group("Profile", None);
        for (i, p) in d.profiles.iter().enumerate() {
            let n = (i + 1).to_string();
            self.row(Row {
                id: format!("profile:{n}"),
                lead: Some(chip(&n).upcast()),
                text: markdown::esc(&p.label),
                sub: Some(&p.description),
                meta: None,
                enabled: true,
            });
        }
        self.para("Press a number or Enter. Esc closes.", &["dp-meta"], 16);
    }

    fn render_items(&mut self, d: &DiagPanel) {
        let p = d.profile.as_ref().and_then(|n| d.profiles.iter().find(|p| &p.name == n));
        if let Some(desc) = p.map(|p| p.description.as_str()).filter(|s| !s.is_empty()) {
            self.para(desc, &["dp-soft"], 0);
        }
        if p.is_some_and(|p| p.needs_unit) {
            self.group("Failed unit", d.units.as_ref().map(|u| plural(u.len() as u64, "unit", "units")));
            match &d.units {
                None => self.para("Looking for failed units…", &["dp-soft"], 0),
                Some(u) if u.is_empty() => {
                    self.para("No unit has failed right now, so this profile has nothing to look at. Pick another profile (⌫).", &["dp-soft"], 0)
                }
                Some(units) => {
                    for (i, u) in units.iter().enumerate() {
                        let meta = if u.searched {
                            "searched"
                        } else if u.user {
                            "user"
                        } else {
                            "system"
                        };
                        self.row(Row {
                            id: format!("unit:{}", i + 1),
                            lead: Some(check_box(d.unit.as_deref() == Some(u.unit.as_str())).upcast()),
                            text: Self::mono(&u.unit),
                            sub: None,
                            meta: Some(meta),
                            enabled: true,
                        });
                    }
                }
            }
        }
        let plan = d.plan.as_deref().unwrap_or_default();
        self.group("Sections", d.plan.as_ref().map(|_| format!("{} of {} selected", d.selected_count(), plan.len())));
        if d.plan_loading {
            self.para("Loading the sections…", &["dp-soft"], 0);
        } else if d.plan.is_none() {
            self.para("Pick a unit first.", &["dp-meta"], 0);
        } else {
            for it in plan {
                let params = it.probe.as_ref().map(params_of).unwrap_or_default();
                self.row(Row {
                    id: format!("item:{}", it.index),
                    lead: Some(check_box(it.selected).upcast()),
                    text: Self::mono(&it.label),
                    sub: Some(&params),
                    meta: it.network.then_some("network"),
                    enabled: true,
                });
            }
        }
        self.para(
            "Space adds or leaves out the marked section. Collecting runs read-only commands here, one after another, at low priority. Nothing is sent.",
            &["dp-meta"],
            16,
        );
    }

    /// Kopfzeile mit Sekunden rechts (Fortschritt, Senden); das Label der Sekunden setzt der Sekundentakt
    fn status_row(&mut self, text: &str) {
        let row = hbox(16);
        let t = label(text, &["dp-soft"]);
        t.set_hexpand(true);
        row.append(&t);
        let secs = label("", &["dp-meta"]);
        row.append(&secs);
        self.body.append(&row);
        self.secs = Some(secs);
    }

    /// Sekundentakt der Diagnose: nur das Label, kein Neuaufbau
    pub fn update_diag_secs(&self, d: &DiagPanel) {
        let Some(l) = &self.secs else { return };
        match d.view {
            View::Progress => {
                if let Some(p) = &d.progress {
                    l.set_text(&format!("{} s", p.started.elapsed().as_secs()));
                }
            }
            View::Send => {
                let host = d.preview.as_ref().map(|p| p.target.host.as_str()).unwrap_or("");
                let secs = d.send_started.map_or(0, |t| t.elapsed().as_secs());
                l.set_text(&format!("{host} · {secs} s"));
            }
            _ => {}
        }
    }

    fn render_progress(&mut self, d: &DiagPanel) {
        let Some(pr) = &d.progress else { return };
        let done = pr.done();
        let what = if pr.total > 0 { format!("Collecting {} of {}", (done + 1).min(pr.total), pr.total) } else { "Starting".into() };
        self.status_row(&what);
        self.update_diag_secs(d);
        let p = self.palette.clone();
        let (soft, tail, err) = (p.on_surface_variant.hex(), p.on_surface_variant.mix(p.surface, 0.35).hex(), p.error.hex());
        let list = vbox(4);
        list.set_margin_top(16);
        for i in 1..=pr.total {
            let row = hbox(12);
            row.set_size_request(-1, 24);
            let dot = vbox(0);
            dot.add_css_class("dp-dot");
            dot.set_valign(gtk::Align::Center);
            let (name, right) = match pr.rows.get(&i) {
                None => (span(&tail, "…"), String::new()),
                Some(r) => {
                    let params = if r.params.is_empty() { String::new() } else { span(&tail, &format!("  {}", r.params)) };
                    match &r.status {
                        None => {
                            dot.add_css_class("run");
                            (format!("{}{params}", markdown::esc(&r.probe)), span(&soft, "running"))
                        }
                        Some(st) => {
                            let bad = matches!(st.as_str(), "failed" | "timeout" | "unavailable");
                            dot.add_css_class(if bad {
                                "bad"
                            } else if st == "ok" {
                                "ok"
                            } else {
                                "off"
                            });
                            let right = format!("{} · {} · {} ms", status_text(st), kib(r.bytes), r.ms);
                            (format!("{}{params}", span(&soft, &r.probe)), span(if bad { &err } else { &tail }, &right))
                        }
                    }
                }
            };
            row.append(&dot);
            let n = markup(&name, &[]);
            n.set_hexpand(true);
            n.set_wrap(false);
            n.set_ellipsize(pango::EllipsizeMode::End);
            row.append(&n);
            row.append(&markup(&right, &[]));
            list.append(&row);
        }
        self.body.append(&list);
        self.para("Esc stops and keeps nothing.", &["dp-meta"], 24);
    }

    /// Vorschau: Ziel, Zusammenfassung, Legende, dann der Bericht, dann der Hinweis. `keep`: der Bericht steht schon
    /// (gleicher Bericht, nur ein anderer Abschnitt gewählt): nur die Köpfe neu stylen
    fn render_preview(&mut self, d: &DiagPanel, keep: bool) {
        let Some(pv) = &d.preview else { return };
        let p = self.palette.clone();
        let (fg, soft, err, acc) = (p.on_surface.hex(), p.on_surface_variant.hex(), p.error.hex(), p.primary.hex());
        let tail = p.on_surface_variant.mix(p.surface, 0.35).hex();
        let top = vbox(0);
        let para = |text: &str, classes: &[&str], m: i32| {
            let l = label(text, classes);
            l.set_margin_top(m);
            top.append(&l);
        };
        let t = &pv.target;
        let target = if t.model.is_empty() { t.host.clone() } else { format!("{} · {}", t.host, t.model) };
        let target = if target.is_empty() { "?".to_string() } else { target };
        let strong = |s: &str| format!("<b><span foreground=\"{fg}\">{}</span></b>", markdown::esc(s));
        let where_ = if t.cloud {
            let sent = d.reviewing();
            format!(
                "{}{}{}",
                span(&soft, if sent { "Sent to " } else { "Sends to " }),
                strong(&target),
                if sent { String::new() } else { span(&soft, " when you press Enter") }
            )
        } else {
            format!("{}{}", span(&soft, "Runs on "), strong(&target))
        };
        top.append(&markup(&where_, &[]));
        if !t.ready {
            para(t.problem.as_deref().unwrap_or("The AI server is not ready"), &["dp-error"], 8);
        }
        let summary = format!(
            "{} of {} · {} of {} · {} · {}",
            pv.included(),
            plural(pv.sections.len() as u64, "section", "sections"),
            kib(pv.bytes),
            kib(pv.budget),
            plural(pv.redactions, "redaction", "redactions"),
            plural(pv.placeholders, "placeholder", "placeholders")
        );
        para(&summary, &["dp-soft"], 8);
        let legend = format!(
            "<span font_family=\"monospace\">{}</span>{}<span font_family=\"monospace\">{}</span>{}",
            span(&err, "[REDACTED:kind]"),
            span(&tail, " secret removed   "),
            span(&acc, "user-1"),
            span(&tail, if pv.pseudonymize { " placeholder for a personal value" } else { " placeholders are off" })
        );
        let l = markup(&legend, &["dp-meta"]);
        l.set_margin_top(4);
        top.append(&l);
        if let (Some(first), Some(last)) = (d.new_ids.first(), d.new_ids.last()) {
            let ids = if first == last { first.clone() } else { format!("{first}–{last}") };
            let m = format!("{}{}", span(&acc, "▌ "), span(&soft, &format!("New from the checks: {ids} (marked at the left). Read them before you send.")));
            let l = markup(&m, &["dp-meta"]);
            l.set_margin_top(8);
            top.append(&l);
        }
        if keep {
            self.body.prepend(&top);
        } else {
            self.body.append(&top);
            let report = build_report(d, pv, &self.tx, (&fg, &soft, &tail, &err, &acc));
            self.body.append(&report.doc);
            self.report = Some(report);
        }
        self.style_heads(d);
        let hint = if d.reviewing() {
            "↑/↓ picks a section, Space leaves it out or adds it. This report was sent; change it to send again."
        } else {
            "↑/↓ picks a section, Space leaves it out or adds it. Esc closes without sending."
        };
        self.para(hint, &["dp-meta"], 16);
    }

    /// Köpfe der Abschnitte: gewählt (hinterlegt, Balken), neu aus der Prüfrunde (Balken); Bedienflächen eintragen
    fn style_heads(&mut self, d: &DiagPanel) {
        let Some(r) = &self.report else { return };
        for t in &r.targets {
            let active = d.sec.as_deref() == Some(t.section.as_str());
            let fresh = d.new_ids.contains(&t.section);
            let set = |c: &str, on: bool| if on { t.head.add_css_class(c) } else { t.head.remove_css_class(c) };
            set("active", active);
            set("bar", active || fresh);
            self.items.insert(format!("sec:{}", t.section), t.head.clone());
        }
    }

    /// Nach dem Layout auf einen Abschnitt bzw. die Zeile mit dieser Nummer springen (`apply_scroll` 549); etwas Luft
    /// darüber, damit der Kopf sichtbar bleibt
    fn scroll_to_section(&self, section: &str, line: Option<u64>) {
        let Some(ScrollTarget { head, body, .. }) = self.report.as_ref().and_then(|r| r.targets.iter().find(|t| t.section == section)) else { return };
        let target: Option<(gtk::Widget, usize)> = match (line, body) {
            (Some(n), Some((label, nums))) => nums.iter().position(|x| *x == n).map(|i| (label.clone().upcast(), i)),
            _ => None,
        };
        let (head, body_w) = (head.downgrade(), self.body.downgrade());
        let adj = self.scroller.vadjustment();
        let frames = std::cell::Cell::new(0u32);
        let target = target.map(|(w, i)| (w.downgrade(), i));
        self.window.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            let Some(body) = body_w.upgrade() else { return glib::ControlFlow::Break };
            let y = match target.as_ref().and_then(|(w, i)| Some((w.upgrade()?, *i))) {
                Some((w, i)) => line_y(&w, i).and_then(|y| w.compute_point(&body, &gtk::graphene::Point::new(0.0, y))).map(|p| f64::from(p.y()) - 36.0),
                None => head.upgrade().and_then(|h| h.compute_bounds(&body)).map(|b| f64::from(b.y()) - 4.0),
            };
            if let Some(y) = y {
                adj.set_value(y.clamp(0.0, (adj.upper() - adj.page_size()).max(0.0)));
                debug!("diagnose: scrolled to {:.0}", y);
            }
            glib::ControlFlow::Break
        });
    }

    fn render_sending(&mut self, d: &DiagPanel) {
        self.status_row("Waiting for the answer");
        self.update_diag_secs(d);
        self.para("Esc stops and goes back to the report.", &["dp-meta"], 24);
    }

    fn render_answer(&mut self, d: &DiagPanel) {
        let Some(a) = &d.answer else { return };
        let p = self.palette.clone();
        let (fg, soft) = (p.on_surface.hex(), p.on_surface_variant.hex());
        let tail = p.on_surface_variant.mix(p.surface, 0.35).hex();
        let look = self.look();
        let ph = PhLook { accent: p.primary.hex(), mono: look.mono.clone(), code_bg: look.code_bg.clone(), tail: look.tail.clone() };
        let pv = d.preview.as_ref();
        let (round, max) = pv.map_or((1, 3), |p| (p.rounds, p.max_rounds));
        let model = d.answer_model.clone().or_else(|| pv.map(|p| p.target.model.clone())).filter(|m| !m.is_empty()).unwrap_or_else(|| "the AI".into());
        let top = hbox(16);
        let m = markup(&format!("{}<b><span foreground=\"{fg}\">{}</span></b>", span(&soft, "Answer from "), markdown::esc(&model)), &[]);
        m.set_hexpand(true);
        top.append(&m);
        top.append(&label(&format!("round {round} of {max}"), &["dp-meta"]));
        self.body.append(&top);
        if a.has_values() {
            let t = if d.plain {
                "Showing your real values (tinted) in place of the placeholders. They were not sent. p shows the placeholders."
            } else {
                "Names in the accent color such as user-1 are placeholders for personal values, as sent. p shows the real values here."
            };
            self.para(t, &["dp-meta"], 4);
        }
        let mut c = Counts::default();
        self.group("Findings", Some(plural(a.findings.len() as u64, "finding", "findings")));
        if a.findings.is_empty() {
            self.para("No clear cause in this report.", &["dp-soft"], 0);
        }
        for (fi, f) in a.findings.iter().enumerate() {
            let conf = if matches!(f.confidence.as_str(), "high" | "medium" | "low") { f.confidence.as_str() } else { "low" };
            let col = match conf {
                "high" => &fg,
                "medium" => &soft,
                _ => &tail,
            };
            let mut cause = diag_notes::cause_markup(&f.cause, &f.spans, d.plain, &ph, &mut c);
            let m = format!("<span size=\"small\" letter_spacing=\"1024\" weight=\"bold\" foreground=\"{col}\">{}</span>   {cause}", conf.to_uppercase());
            crate::actions::store::wipe(&mut cause);
            let l = markup(&m, &["dp-doc"]);
            l.set_margin_top(if fi > 0 { 16 } else { 8 });
            self.body.append(&l);
            for (ei, e) in f.evidence.iter().enumerate() {
                let id = format!("ev:{}:{}", fi + 1, ei + 1);
                let l = label(&format!("→ Evidence {}", e.text()), &["dp-ev"]);
                clickable(&l, &id, &self.tx);
                self.items.insert(id, l.clone().upcast());
                self.body.append(&l);
            }
        }
        if !a.notes.is_empty() {
            self.group("Notes", None);
            self.para(NOTES_WARNING, &["dp-meta"], 0);
            let notes = diag_notes::notes_items(&a.notes, &a.note_spans);
            let doc = vbox(0);
            doc.set_margin_top(8);
            let mut prev_item = false;
            for b in &notes.blocks {
                let is_item = matches!(b, Block::Item { .. });
                let w: gtk::Widget = match b {
                    Block::Gap => {
                        let g = vbox(0);
                        g.set_size_request(-1, 12);
                        g.upcast()
                    }
                    // kein Copy-Knopf für Befehle (Antwort 6), Platzhalter statt Werten
                    Block::Code { text, .. } => {
                        let row = hbox(8);
                        row.add_css_class("dp-code");
                        row.set_margin_top(8);
                        row.set_margin_bottom(8);
                        let mut t = notes.guarded_text(text, &mut c);
                        let mut m = format!("<span {NO_HYPHENS}>{}</span>", markdown::esc(if t.is_empty() { " " } else { &t }));
                        let l = markup(&m, &["dp-mono"]);
                        crate::actions::store::wipe(&mut t);
                        crate::actions::store::wipe(&mut m);
                        l.set_wrap_mode(pango::WrapMode::Char);
                        l.set_hexpand(true);
                        row.append(&l);
                        row.upcast()
                    }
                    Block::Item { level, marker, spans } => {
                        let row = hbox(8);
                        row.set_margin_start(*level as i32 * 24);
                        if prev_item {
                            row.set_margin_top(4);
                        }
                        let mk = label(marker, &["dp-marker"]);
                        mk.set_valign(gtk::Align::Start);
                        row.append(&mk);
                        let l = self.notes_label(&notes, spans, d.plain, &ph, &mut c, &[]);
                        l.set_hexpand(true);
                        row.append(&l);
                        row.upcast()
                    }
                    Block::Heading(s) => self.notes_label(&notes, s, d.plain, &ph, &mut c, &["dp-heading"]).upcast(),
                    Block::Quote(s) => self.notes_label(&notes, s, d.plain, &ph, &mut c, &["dp-quote"]).upcast(),
                    Block::Para(s) => self.notes_label(&notes, s, d.plain, &ph, &mut c, &[]).upcast(),
                };
                prev_item = is_item;
                doc.append(&w);
            }
            self.body.append(&doc);
        }
        let can = d.rounds_left();
        let picked = d.picked.len() as u64;
        self.group("Checks", (!a.checks.is_empty()).then(|| format!("{picked} of {} selected", a.checks.len())));
        if a.checks.is_empty() {
            self.para("No further checks proposed.", &["dp-soft"], 0);
        } else {
            for (i, ch) in a.checks.iter().enumerate() {
                self.row(Row {
                    id: format!("check:{}", i + 1),
                    lead: Some(check_box(d.picked.contains(&(i + 1))).upcast()),
                    text: Self::mono(&ch.label),
                    sub: Some(&ch.reason),
                    meta: ch.network.then_some("network"),
                    enabled: can,
                });
            }
            let h = if !can {
                format!("No more rounds in this diagnosis ({round} of {max}).")
            } else if d.selected_checks().iter().any(|c| c.network) {
                "Space selects a check. Enter runs the selected checks here; you see the next report before anything is sent. A selected check sends network traffic.".into()
            } else {
                "Space selects a check. Enter runs the selected checks here; you see the next report before anything is sent.".into()
            };
            self.para(&h, &["dp-meta"], 12);
        }
        debug!("diagnose: answer shows {} values, {} placeholders, {} guarded", c.values, c.placeholders, c.guarded);
    }

    fn notes_label(&self, notes: &diag_notes::Notes, spans: &[markdown::Span], plain: bool, ph: &PhLook, c: &mut Counts, extra: &[&str]) -> gtk::Label {
        let mut m = notes.spans_markup(spans, plain, ph, c);
        let mut classes = vec!["dp-doc"];
        classes.extend_from_slice(extra);
        let l = markup(&m, &classes);
        crate::actions::store::wipe(&mut m);
        l
    }

    fn render_diag_error(&mut self, d: &DiagPanel) {
        let Some(e) = &d.err else { return };
        self.body.append(&label(&e.title, &[if e.neutral { "dp-strong" } else { "dp-error" }]));
        if let Some(b) = &e.body {
            self.para(b, &["dp-soft"], 8);
        }
    }

    fn render_diag_buttons(&mut self, d: &DiagPanel) {
        let left = hbox(8);
        let right = hbox(8);
        left.set_hexpand(true);
        for it in d.items().into_iter().filter(|i| i.kind != ItemKind::Row) {
            let b = self.button(&it);
            if it.left {
                left.append(&b);
            } else {
                right.append(&b);
            }
        }
        self.footer.append(&left);
        self.footer.append(&right);
    }
}

/// Was den gezeichneten Bericht bestimmt: gleich = der Bericht bleibt stehen (nur ein anderer Abschnitt gewählt)
#[derive(PartialEq, Eq)]
pub(crate) struct ReportKey {
    id: String,
    preview: u64,
    evidence: Option<(String, u64, u64)>,
    new_ids: Vec<String>,
}

fn report_key(d: &DiagPanel) -> Option<ReportKey> {
    let pv = d.preview.as_ref()?;
    Some(ReportKey {
        id: pv.id.clone(),
        preview: pv.preview,
        evidence: d.evidence.as_ref().map(|e| (e.section.clone(), e.from, e.to)),
        new_ids: d.new_ids.clone(),
    })
}

/// Sprungziel: Kopf des Abschnitts, Körper mit den Nummern seiner Zeilen (in Reihenfolge)
pub(crate) struct ScrollTarget {
    section: String,
    head: gtk::Widget,
    body: Option<(gtk::Label, Vec<u64>)>,
}

/// Der gezeichnete Bericht der Vorschau
pub(crate) struct Report {
    key: ReportKey,
    doc: gtk::Box,
    targets: Vec<ScrollTarget>,
}

/// Bericht zeichnen: Kopf des Berichts (grau), je Abschnitt ein klickbarer Kopf (■/□) und ein Mono-Label für den Körper
/// (Zeilennummer grau, Schwärzung rot hinterlegt, Platzhalter im Akzent, Beleg-Zeilen hinterlegt). Klicks prüft der
/// Automat (`enabled`), darum ist jeder Kopf klickbar.
fn build_report(d: &DiagPanel, pv: &diag_panel::PreviewInfo, tx: &async_channel::Sender<crate::link::Msg>, colors: (&str, &str, &str, &str, &str)) -> Report {
    let (fg, soft, tail, err, acc) = colors;
    let limits = diag_panel::limits(pv);
    let (pre, secs) = split_report(&pv.text, &pv.sections);
    let doc = vbox(0);
    doc.add_css_class("dp-report");
    doc.set_margin_top(16);
    if !pre.is_empty() {
        let mut m: Vec<String> = pre.iter().map(|l| marked(l, &limits, err, acc)).collect();
        let mut all = m.join("\n");
        let l = markup(&format!("<span foreground=\"{tail}\" {NO_HYPHENS}>{all}</span>"), &[]);
        l.set_wrap_mode(pango::WrapMode::Char);
        doc.append(&l);
        m.iter_mut().for_each(crate::actions::store::wipe);
        crate::actions::store::wipe(&mut all);
    }
    let mut targets = vec![];
    for s in &secs {
        let included = pv.sections.iter().find(|x| x.id == s.id).is_none_or(|i| i.included);
        let fresh = d.new_ids.contains(&s.id);
        let first = s.head.first().map(|h| h.strip_prefix("=== ").unwrap_or(h)).unwrap_or("");
        let mut head = format!("{}  {}", if included { span(acc, "■") } else { span(tail, "□") }, span(if included { fg } else { tail }, first));
        if let Some(second) = s.head.get(1) {
            head.push_str(&format!("\n    {}", span(tail, second)));
        }
        let h = markup(&format!("<span {NO_HYPHENS}>{head}</span>"), &[]);
        h.set_wrap_mode(pango::WrapMode::Char);
        h.set_hexpand(true);
        let hb = hbox(0);
        hb.add_css_class("dp-sec");
        hb.append(&h);
        clickable(&hb, &format!("sec:{}", s.id), tx);
        doc.append(&hb);
        let mut body = None;
        if !s.body.is_empty() {
            let ev = d.evidence.as_ref().filter(|e| e.section == s.id);
            let mut nums = Vec::with_capacity(s.body.len());
            let mut lines = Vec::with_capacity(s.body.len());
            for (i, l) in s.body.iter().enumerate() {
                let (n, m) = match (line_number(l), l.find('|')) {
                    (Some((n, _)), Some(bar)) => (n, format!("{}{}", span(tail, &l[..=bar]), marked(&l[bar + 1..], &limits, err, acc))),
                    _ => (i as u64 + 1, marked(l, &limits, err, acc)),
                };
                nums.push(n);
                let m = match ev {
                    Some(e) if n >= e.from && n <= e.to => format!("<span background=\"{acc}\" bgalpha=\"22%\">{}</span>", if m.is_empty() { " " } else { &m }),
                    _ => m,
                };
                lines.push(m);
            }
            let mut all = lines.join("\n");
            lines.iter_mut().for_each(crate::actions::store::wipe);
            let b = markup(&format!("<span foreground=\"{soft}\" {NO_HYPHENS}>{all}</span>"), &[]);
            crate::actions::store::wipe(&mut all);
            b.set_wrap_mode(pango::WrapMode::Char);
            let bb = vbox(0);
            bb.add_css_class("dp-sec-body");
            if fresh {
                bb.add_css_class("bar");
            }
            bb.append(&b);
            doc.append(&bb);
            body = Some((b, nums));
        }
        targets.push(ScrollTarget { section: s.id.clone(), head: hb.upcast(), body });
    }
    Report { key: report_key(d).expect("preview"), doc, targets }
}

/// Lage der Zeile `i` (ab 0) im Label: Oberkante in Koordinaten des Labels
fn line_y(w: &gtk::Widget, i: usize) -> Option<f32> {
    let l = w.downcast_ref::<gtk::Label>()?;
    let text = l.text();
    let idx = if i == 0 { 0 } else { text.match_indices('\n').nth(i - 1).map(|(k, _)| k + 1)? };
    let layout = l.layout();
    let rect = layout.index_to_pos(i32::try_from(idx).ok()?);
    let (_, oy) = l.layout_offsets();
    Some(rect.y() as f32 / pango::SCALE as f32 + oy as f32)
}

/// Eine Berichtszeile mit markierten Schwärzungen (`error`, hinterlegt) und Platzhaltern (`primary`)
fn marked(line: &str, limits: &std::collections::HashMap<String, u64>, err: &str, acc: &str) -> String {
    let mut out = String::new();
    for (kind, piece) in mark_line(line, limits) {
        let t = markdown::esc(piece);
        match kind {
            Mark::Plain => out.push_str(&t),
            Mark::Redacted => out.push_str(&format!("<span foreground=\"{err}\" background=\"{err}\" bgalpha=\"14%\">{t}</span>")),
            Mark::Placeholder => out.push_str(&format!("<span foreground=\"{acc}\">{t}</span>")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kib_reads_like_lua() {
        assert_eq!(kib(0), "0 B");
        assert_eq!(kib(1023), "1023 B");
        assert_eq!(kib(1024), "1.0 KiB");
        assert_eq!(kib(49152), "48.0 KiB");
    }

    #[test]
    fn marked_escapes_every_piece() {
        let mut lim = std::collections::HashMap::new();
        lim.insert("user".to_string(), 1);
        let m = marked("<x> user-1 & [REDACTED:token] user-2", &lim, "#f00", "#0aa");
        assert!(m.starts_with("&lt;x&gt; <span foreground=\"#0aa\">user-1</span> &amp; <span"), "{m}");
        assert!(m.contains(">[REDACTED:token]</span>"), "{m}");
        // über der Grenze: kein Platzhalter
        assert!(m.ends_with(" user-2"), "{m}");
    }

    #[test]
    fn status_texts() {
        assert_eq!(status_text("timeout"), "timed out");
        assert_eq!(status_text("ok"), "ok");
    }
}
