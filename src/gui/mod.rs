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

//! Einstellungsfenster `dictate-settings` (egui/eframe).

pub mod downloads;
pub mod dto;
pub mod fonts;
pub mod layout_log;
pub mod net;
pub mod pages;
pub mod store;
pub mod theme;
pub mod watch;
pub mod widgets;

use downloads::{DlState, Job, Progress};
use dto::Settings;
use net::{reply_error, Msg, Net, Tag};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use theme::*;
use widgets::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    General,
    Audio,
    Model,
    Recognition,
    Overlay,
    Commands,
    Vocabulary,
    Profiles,
    Ai,
    Actions,
    Palette,
    Diagnose,
    History,
    About,
}

impl Page {
    pub const ALL: [Page; 14] = [
        Page::General,
        Page::Audio,
        Page::Model,
        Page::Recognition,
        Page::Overlay,
        Page::Commands,
        Page::Vocabulary,
        Page::Profiles,
        Page::Ai,
        Page::Actions,
        Page::Palette,
        Page::Diagnose,
        Page::History,
        Page::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Audio => "Audio",
            Page::Model => "Model",
            Page::Recognition => "Recognition",
            Page::Overlay => "Overlay",
            Page::Commands => "Voice commands",
            Page::Vocabulary => "Vocabulary",
            Page::Profiles => "Profiles",
            Page::Ai => "AI",
            Page::Actions => "Actions",
            Page::Palette => "Palette",
            Page::Diagnose => "Diagnose",
            Page::History => "History",
            Page::About => "About",
        }
    }

    /// Name für `--page` (erstes Wort der Bezeichnung, klein)
    pub fn arg(self) -> String {
        self.label().to_lowercase().split(' ').next().unwrap_or_default().to_string()
    }

    pub fn from_arg(s: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.arg() == s)
    }
}

#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub description: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub file: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct CatalogItem {
    pub id: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub ts: String,
    pub app: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    None,
    Ok(String),
    Err(String),
}

pub struct DlEntry {
    pub progress: Progress,
    pub job: Option<Job>,
}

#[derive(Default)]
pub struct AiTest {
    pub input: String,
    pub output: Option<Result<String, String>>,
    pub busy: bool,
}

pub struct App {
    pub ctx: egui::Context,
    pub net: Net,
    rx: Receiver<Msg>,
    pub page: Page,
    pub saved: Settings,
    pub edited: Settings,
    pub loaded: bool,
    pub online: bool,
    pub applying: bool,
    pub daemon_state: String,
    pub errors: HashMap<String, String>,
    pub status: Status,
    pub config_path: PathBuf,
    pub sources: Vec<Source>,
    pub installed: Vec<Installed>,
    pub catalog: Vec<CatalogItem>,
    pub ai_models: Result<Vec<String>, String>,
    pub history: Vec<HistoryEntry>,
    /// Befehlstabelle vom Daemon (de, en, effect); `None` = statische Tabelle der Seite
    pub commands: Option<Vec<[String; 3]>>,
    pub level_db: f32,
    pub speech: bool,
    /// Fehler des Daemons während der Pegelmessung (z. B. Quelle nicht zu öffnen)
    pub meter_error: Option<String>,
    meter_on: bool,
    meter_sent: Instant,
    pub downloads: HashMap<String, DlEntry>,
    pub ai_test: AiTest,
    pub profile_sel: usize,
    pub confirm_clear: bool,
    /// Seite Palette: Verlauf löschen wartet auf die Bestätigung; Ergebnis des letzten Knopfdrucks
    pub pal_confirm: bool,
    pub pal_notice: Option<(Tag, Status)>,
    /// Palette und Schrift; färbt im laufenden Fenster um (L43)
    theme: theme::Live,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, sock: PathBuf, start_page: Page) -> App {
        let theme = theme::Live::start(&cc.egui_ctx);
        let (tx, rx) = channel();
        let net = Net::new(sock, tx, cc.egui_ctx.clone());
        net.spawn_subscriber();
        App {
            ctx: cc.egui_ctx.clone(),
            net,
            rx,
            page: start_page,
            saved: Settings::default(),
            edited: Settings::default(),
            loaded: false,
            online: false,
            applying: false,
            daemon_state: String::new(),
            errors: HashMap::new(),
            status: Status::None,
            config_path: store::config_path(),
            sources: Vec::new(),
            installed: Vec::new(),
            catalog: Vec::new(),
            ai_models: Err(String::new()),
            history: Vec::new(),
            commands: None,
            level_db: -80.0,
            speech: false,
            meter_error: None,
            meter_on: false,
            meter_sent: Instant::now(),
            downloads: HashMap::new(),
            ai_test: AiTest::default(),
            profile_sel: 0,
            confirm_clear: false,
            pal_confirm: false,
            pal_notice: None,
            theme,
        }
    }

    pub fn dirty(&self) -> bool {
        self.edited != self.saved
    }

    pub fn err(&self, path: &str) -> Option<String> {
        self.errors.get(path).cloned()
    }

    /// Index des Listeneintrags, an dem der Fehler hängt (`actions.terminal_classes.2` → 2).
    pub fn err_index(&self, path: &str) -> Option<usize> {
        error_index(&self.errors, path)
    }

    /// Fehler an `path` oder an einem Element darunter (`actions.terminal_classes` trifft auch
    /// `actions.terminal_classes.2`, nicht aber `actions.terminal_classes_x`).
    pub fn err_under(&self, path: &str) -> Option<String> {
        error_under(&self.errors, path)
    }

    fn set_loaded(&mut self, s: Settings) {
        if !self.dirty() || !self.loaded {
            self.edited = s.clone();
        }
        self.saved = s;
        self.loaded = true;
    }

    fn load_offline(&mut self) {
        match store::load(&self.config_path) {
            Ok(s) => self.set_loaded(s),
            Err(e) => {
                self.status = Status::Err(e);
                self.loaded = true;
            }
        }
        self.scan_models_dir();
    }

    /// Ohne Daemon: installierte Modelle direkt aus dem Verzeichnis lesen.
    fn scan_models_dir(&mut self) {
        if self.online {
            return;
        }
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(downloads::models_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".bin") {
                    v.push(Installed { file: name, size: e.metadata().map(|m| m.len()).unwrap_or(0) });
                }
            }
        }
        v.sort_by(|a, b| a.file.cmp(&b.file));
        self.installed = v;
    }

    fn on_connected(&mut self, on: bool) {
        let was = self.online;
        self.online = on;
        self.meter_on = false;
        if on {
            self.net.request(Tag::GetConfig, json!({"cmd":"get_config"}));
            self.net.request(Tag::Sources, json!({"cmd":"list_sources"}));
            self.net.request(Tag::Models, json!({"cmd":"list_models"}));
            self.enter_page();
        } else {
            self.daemon_state.clear();
            if was || !self.loaded {
                self.load_offline();
            }
        }
    }

    /// Beim Betreten einer Seite nachladen, was sie braucht.
    pub fn enter_page(&mut self) {
        self.confirm_clear = false;
        self.pal_confirm = false;
        self.pal_notice = None;
        if !self.online {
            return;
        }
        match self.page {
            Page::History => self.net.request(Tag::History, json!({"cmd":"history","limit":200})),
            Page::Ai | Page::Actions | Page::Palette | Page::Diagnose => self.net.request(Tag::AiModels, json!({"cmd":"ai_models"})),
            Page::Model => self.net.request(Tag::Models, json!({"cmd":"list_models"})),
            Page::Commands => self.net.request(Tag::Commands, json!({"cmd":"commands"})),
            _ => {}
        }
    }

    pub fn goto(&mut self, p: Page) {
        if self.page != p {
            self.page = p;
            self.enter_page();
        }
    }

    pub fn apply(&mut self) {
        self.errors.clear();
        self.status = Status::None;
        if self.online {
            self.applying = true;
            self.net.request(Tag::Apply, json!({"cmd":"set_config","config":self.edited.to_value()}));
        } else {
            match store::save(&self.config_path, &self.edited) {
                Ok(()) => {
                    self.saved = self.edited.clone();
                    self.status = Status::Ok("Saved to the config file. Applies on the next daemon start.".into());
                }
                Err(e) => self.fail(e.msg, e.field),
            }
        }
    }

    pub fn revert(&mut self) {
        self.edited = self.saved.clone();
        self.errors.clear();
        self.status = Status::None;
    }

    fn fail(&mut self, msg: String, field: Option<String>) {
        if let Some(f) = field {
            let key = normalize_path(&f);
            self.errors.insert(key, msg.clone());
        }
        self.status = Status::Err(msg);
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Connected(on) => self.on_connected(on),
            Msg::Event(v) => self.on_event(v),
            Msg::Reply { tag, res } => self.on_reply(tag, res),
            Msg::Download(p) => self.on_download(p),
        }
    }

    fn on_event(&mut self, v: Value) {
        match v.get("event").and_then(Value::as_str) {
            Some("state") => {
                self.daemon_state = v.get("state").and_then(Value::as_str).unwrap_or("").to_string();
            }
            Some("level") => {
                self.level_db = v.get("db").and_then(Value::as_f64).unwrap_or(-80.0) as f32;
                self.speech = v.get("speech").and_then(Value::as_bool).unwrap_or(false);
                self.meter_error = None;
            }
            Some("error") if self.meter_on => {
                self.meter_error = v.get("msg").and_then(Value::as_str).map(str::to_string);
            }
            _ => {}
        }
    }

    fn on_reply(&mut self, tag: Tag, res: Result<Value, String>) {
        let v = match res {
            Ok(v) => v,
            Err(e) => {
                match tag {
                    Tag::Apply => {
                        self.applying = false;
                        self.fail(e, None);
                    }
                    Tag::AiModels => self.ai_models = Err(e),
                    Tag::PalClear | Tag::PalRefresh => self.pal_notice = Some((tag.clone(), Status::Err(e))),
                    Tag::AiTest => {
                        self.ai_test.busy = false;
                        self.ai_test.output = Some(Err(e));
                    }
                    _ => {}
                }
                return;
            }
        };
        let failure = reply_error(&v);
        match tag {
            Tag::GetConfig => match (failure, v.get("config")) {
                (Some((m, _)), _) => self.status = Status::Err(m),
                (None, Some(c)) => match Settings::from_value(c.clone()) {
                    Ok(s) => {
                        if let Some(p) = v.get("path").and_then(Value::as_str) {
                            self.config_path = PathBuf::from(p);
                        }
                        self.set_loaded(s);
                    }
                    Err(e) => self.status = Status::Err(format!("bad config from daemon: {e}")),
                },
                _ => {}
            },
            Tag::Apply => {
                self.applying = false;
                match failure {
                    Some((m, f)) => self.fail(m, f),
                    None => {
                        self.saved = self.edited.clone();
                        let model = v.get("reload").and_then(Value::as_str) == Some("model");
                        self.status = Status::Ok(if model { "Applied. The model is reloading." } else { "Applied." }.into());
                    }
                }
            }
            Tag::Sources => {
                self.sources = v
                    .get("sources")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|s| Source {
                                name: s.get("name").and_then(Value::as_str).unwrap_or("").into(),
                                description: s.get("description").and_then(Value::as_str).unwrap_or("").into(),
                                is_default: s.get("default").and_then(Value::as_bool).unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Tag::Models => {
                let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let n = |x: &Value, k: &str| x.get(k).and_then(Value::as_u64).unwrap_or(0);
                self.installed = v
                    .get("installed")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|i| Installed { file: s(i, "file"), size: n(i, "size") }).collect())
                    .unwrap_or_default();
                self.catalog = v
                    .get("catalog")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|i| CatalogItem { id: s(i, "id"), url: s(i, "url"), sha256: s(i, "sha256"), size: n(i, "size"), note: s(i, "note") })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Tag::AiModels => {
                self.ai_models = match failure {
                    Some((m, _)) => Err(m),
                    None => {
                        let arr = ["models", "tags", "data"]
                            .iter()
                            .find_map(|k| v.get(*k))
                            .and_then(|m| m.as_array().cloned().or_else(|| m.get("models").and_then(Value::as_array).cloned()));
                        Ok(arr
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|m| m.get("name").or_else(|| m.get("model")).and_then(Value::as_str).map(str::to_string))
                            .collect())
                    }
                };
            }
            Tag::History => {
                self.history = v
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|e| {
                                let g = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                                HistoryEntry { ts: g("ts"), app: g("app"), text: g("text") }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Tag::Commands => {
                // ältere Daemons kennen den Befehl nicht: dann bleibt die statische Tabelle
                self.commands = v
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|e| ["de", "en", "effect"].map(|k| e.get(k).and_then(Value::as_str).unwrap_or("").to_string())).collect());
            }
            Tag::HistoryClear => {
                self.history.clear();
                self.confirm_clear = false;
            }
            Tag::PalClear | Tag::PalRefresh => {
                self.pal_confirm = false;
                let status = match failure {
                    Some((m, _)) => Status::Err(m),
                    None if tag == Tag::PalClear => Status::Ok("History cleared.".into()),
                    None => Status::Ok("Apps and projects will be read again.".into()),
                };
                self.pal_notice = Some((tag, status));
            }
            Tag::AiTest => {
                self.ai_test.busy = false;
                self.ai_test.output = Some(match failure {
                    Some((m, _)) => Err(m),
                    None => Ok(v.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
                });
            }
        }
    }

    fn on_download(&mut self, p: Progress) {
        let finished = !matches!(p.state, DlState::Running);
        let done = matches!(p.state, DlState::Done);
        if let Some(e) = self.downloads.get_mut(&p.id) {
            e.progress = p;
            if finished {
                e.job = None;
            }
        }
        if done {
            if self.online {
                self.net.request(Tag::Models, json!({"cmd":"list_models"}));
            } else {
                self.scan_models_dir();
            }
        }
    }

    pub fn start_download(&mut self, c: &CatalogItem) {
        let job = downloads::start(c.id.clone(), c.url.clone(), c.sha256.clone(), c.size, downloads::models_dir(), self.net.sender(), self.ctx.clone());
        self.downloads
            .insert(c.id.clone(), DlEntry { progress: Progress { id: c.id.clone(), done: 0, total: c.size, state: DlState::Running }, job: Some(job) });
    }

    /// Pegel nur messen, solange die Audio-Seite sichtbar und der Daemon da ist.
    fn manage_meter(&mut self, ctx: &egui::Context) {
        let visible = ctx.input(|i| i.viewport().minimized != Some(true)) && self.page == Page::Audio && self.online;
        if visible {
            // Der Daemon schließt das Mikrofon nach 60 s von selbst, deshalb vorher erneuern
            if !self.meter_on || self.meter_sent.elapsed() > Duration::from_secs(45) {
                self.net.meter(true);
                self.meter_on = true;
                self.meter_sent = Instant::now();
                self.level_db = -80.0;
                self.speech = false;
                self.meter_error = None;
            }
            ctx.request_repaint_after(Duration::from_secs(46));
        } else if self.meter_on {
            self.net.meter(false);
            self.meter_on = false;
        }
    }
}

/// `profiles[2].name` -> `profiles.2.name`
pub fn normalize_path(p: &str) -> String {
    p.replace('[', ".").replace(']', "")
}

/// Siehe [`App::err_index`]; `errors` hat Schlüssel nach [`normalize_path`].
fn error_index(errors: &HashMap<String, String>, path: &str) -> Option<usize> {
    let prefix = format!("{path}.");
    errors.keys().find_map(|k| k.strip_prefix(&prefix).and_then(|rest| rest.parse().ok()))
}

/// Siehe [`App::err_under`].
fn error_under(errors: &HashMap<String, String>, path: &str) -> Option<String> {
    errors.get(path).cloned().or_else(|| {
        let prefix = format!("{path}.");
        errors.iter().filter(|(k, _)| k.starts_with(&prefix)).min_by(|a, b| a.0.cmp(b.0)).map(|(_, m)| m.clone())
    })
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m);
        }
        self.theme.poll(ctx);
        self.manage_meter(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        use egui::{Frame, Margin, Panel};
        Panel::left("sidebar")
            .exact_size(240.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(th().sidebar).inner_margin(Margin { left: 0, right: 0, top: 32, bottom: 24 }))
            .show(ui, |ui| self.sidebar(ui));

        if !self.online {
            Panel::top("banner").show_separator_line(false).frame(Frame::new().fill(th().raised).inner_margin(Margin::symmetric(48, 12))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    pixel(ui, 8.0, th().soft);
                    ui.label(body("Daemon not running. Changes apply on the next start."));
                });
            });
        }

        // Auf About gibt es nichts zu speichern
        if self.page != Page::About {
            Panel::bottom("actions").show_separator_line(false).frame(Frame::new().fill(th().surface).inner_margin(Margin::symmetric(48, 16))).show(ui, |ui| {
                hairline(ui);
                ui.add_space(8.0);
                self.action_bar(ui);
            });
        }

        egui::CentralPanel::default().frame(Frame::new().fill(th().surface).inner_margin(Margin { left: 48, right: 48, top: 40, bottom: 0 })).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(720.0);
                ui.set_width(ui.available_width().min(720.0));
                self.page_content(ui);
                ui.add_space(24.0);
            });
        });
        layout_log::flush();
        self.theme.painted(ui);
    }
}

impl App {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(0.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            // Logo nach Vorlage V3: Pixelzeichen aus Rollen, Wortmarke als Text (keine Versalien mehr)
            logo(ui, 16.0);
        });
        ui.add_space(24.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut go = None;
        for p in Page::ALL {
            let item = sidebar_item(ui, p.label(), self.page == p);
            layout_log::note(ui, || format!("nav:{}", p.arg()), &item);
            if item.clicked() {
                go = Some(p);
            }
        }
        if let Some(p) = go {
            self.goto(p);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                let (c, label) = if self.online {
                    (
                        th().accent,
                        format!("{} {}", "Daemon running", if self.daemon_state.is_empty() { String::new() } else { format!("({})", self.daemon_state) }),
                    )
                } else {
                    (th().soft, "Daemon stopped".to_string())
                };
                ui.add_space(0.0);
                pixel(ui, 8.0, c);
                ui.label(small(label.trim().to_string()));
            });
        });
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let dirty = self.dirty();
            let apply = primary_button(ui, "Apply", dirty && !self.applying);
            layout_log::note(ui, || "apply".into(), &apply);
            if apply.clicked() {
                self.apply();
            }
            let revert = ui.add_enabled_ui(dirty, |ui| secondary_button(ui, "Revert")).inner;
            layout_log::note(ui, || "revert".into(), &revert);
            if revert.clicked() {
                self.revert();
            }
            match self.status.clone() {
                Status::Err(m) => {
                    ui.add(egui::Label::new(text(m, Size::Small, egui::FontFamily::Proportional, th().error)).truncate());
                }
                Status::Ok(m) if !dirty => {
                    pixel(ui, 8.0, th().accent);
                    ui.label(small(m));
                }
                _ if dirty => {
                    pixel(ui, 8.0, th().soft);
                    ui.label(small("Unsaved changes"));
                }
                _ => {}
            }
        });
    }

    fn page_content(&mut self, ui: &mut egui::Ui) {
        if !self.loaded {
            ui.label(secondary("Loading settings…"));
            return;
        }
        match self.page {
            Page::General => pages::general::show(self, ui),
            Page::Audio => pages::audio::show(self, ui),
            Page::Model => pages::model::show(self, ui),
            Page::Recognition => pages::recognition::show(self, ui),
            Page::Overlay => pages::overlay::show(self, ui),
            Page::Commands => pages::commands::show(self, ui),
            Page::Vocabulary => pages::vocabulary::show(self, ui),
            Page::Profiles => pages::profiles::show(self, ui),
            Page::Ai => pages::ai::show(self, ui),
            Page::Actions => pages::actions::show(self, ui),
            Page::Palette => pages::palette::show(self, ui),
            Page::Diagnose => pages::diagnose::show(self, ui),
            Page::History => pages::history::show(self, ui),
            Page::About => pages::about::show(self, ui),
        }
    }
}

/// Seitenleisten-Eintrag wie `.dp-row` im Panel von `dictate-ui`: Zeile 40 px, 12 px eingerückt, Ecken 8; aktiv und
/// beim Hover Akzent 16 % darunter und Text statt Nebentext, aktiv dazu ein 7-px-Akzentpixel vor dem Text.
fn sidebar_item(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let (cell, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let rect = cell.shrink2(egui::vec2(12.0, 0.0));
    let t = th();
    let p = ui.painter();
    let hot = active || resp.hovered();
    if hot {
        p.rect_filled(rect, CONTROL, t.accent_a(ROW_FILL));
    }
    let x = cell.left() + 24.0;
    if active {
        let px = egui::Rect::from_center_size(egui::pos2(x + 3.5, rect.center().y), egui::vec2(7.0, 7.0));
        p.rect_filled(px, egui::CornerRadius::same(1), t.accent);
    }
    let color = if hot { t.text } else { t.soft };
    p.text(egui::pos2(x + 19.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::new(Size::Body.px(), egui::FontFamily::Proportional), color);
    if resp.has_focus() {
        p.rect_stroke(rect, CONTROL, egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_errors_reach_the_field_and_the_entry() {
        // so meldet der Daemon Fehler in Listen (`Config::validate`), so legt `fail` sie ab
        let errors: HashMap<String, String> =
            [(normalize_path("actions.terminal_classes[7]"), "empty".to_string()), (normalize_path("profiles[1].match_class[0]"), "empty class".to_string())]
                .into();
        assert_eq!(error_under(&errors, "actions.terminal_classes").as_deref(), Some("empty"));
        assert_eq!(error_index(&errors, "actions.terminal_classes"), Some(7));
        assert_eq!(error_under(&errors, "profiles.1.match_class").as_deref(), Some("empty class"));
        assert_eq!(error_index(&errors, "profiles.1.match_class"), Some(0));
        assert_eq!(error_under(&errors, "profiles.0.match_class"), None);
        assert_eq!(error_under(&errors, "actions.terminal"), None, "only whole path segments match");
        assert_eq!(error_index(&errors, "actions.prompts"), None);
    }

    #[test]
    fn every_page_opens_by_name() {
        for p in Page::ALL {
            assert_eq!(Page::from_arg(p.label().to_lowercase().split(' ').next().unwrap()), Some(p));
        }
        assert_eq!(Page::from_arg("actions"), Some(Page::Actions));
        // Bis W7 rief `palette_exec.lua` `dictate-settings --page palette` auf, unter niri ab W5 das Panel (Katalog: `SettingsPage::Palette`)
        assert_eq!(Page::from_arg("palette"), Some(Page::Palette));
        assert_eq!(Page::from_arg("diagnose"), Some(Page::Diagnose));
        assert_eq!(Page::from_arg("voice"), Some(Page::Commands));
    }
}
