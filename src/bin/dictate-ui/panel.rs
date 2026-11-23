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

//! Panel der Textaktionen als schwebendes GTK-Fenster (PLAN-wayland.md 3.3 und 6d Punkt 2): `app_id` `dictate-panel`
//! (Fensterregel in `niri/dictate-binds.kdl`: schwebend, fokussiert), ohne Fensterdekoration, Inhalt je Zustand aus
//! dem Automaten `ui::panel`. Gezeichnet wird nur; jede Eingabe (Tasten, Klick, Hover, Fokusverlust, Schließen über
//! niri) geht als [`UiEv`] über den Kanal an den Hauptthread, nie direkt in den Automaten (keine Wiedereintritte, wenn
//! GTK beim Neuaufbau Signale schickt).
//!
//! Bedienelemente sind keine GTK-Knöpfe, sondern nicht fokussierbare Flächen: alle Tasten gehen an den Automaten
//! (Totzeit, `stale`-Tasten), Maus und Tastatur teilen eine Hervorhebung, ein Klick löst beim Loslassen auf demselben
//! Element aus. Kein Text ist auswählbar (eine eigene Auswahl übernähme PRIMARY); einzige Ausnahme ist das Eingabefeld
//! „What is wrong?“ der Diagnose (`field`, Lesart im Kopf von `diag_view.rs`).
//!
//! Größe (Entscheidung 13): Breite aus der Fensterregel, Höhe passend zum Inhalt bis [`MAX_HEIGHT`] (darüber scrollt die
//! Antwort), gesetzt über niri-IPC (`SetWindowHeight` nur auf das eigene Fenster, `niri::own_window`), damit niris
//! Resize-Shader wirkt. Während des Streamings wächst das Panel nur (kein Springen), beim Zustandswechsel passt es sich
//! an. Ragt es danach unten aus dem Ausgang, schiebt der Resizer es um den Überstand hoch (`niri::shift_into_view`). Farben aus der Noctalia-Palette (`ui::palette`), Schrift aus GTK, Ecken 12.
//!
//! Gemeinsame Hülle (PLAN-wayland.md 6e Punkt 3, Entscheidung Daniel 21.11.: dasselbe Fenster `dictate-panel`, ein
//! Panel zur Zeit): Kopf mit ✕, Zeilen und Knöpfe als [`clickable`] Flächen, Hervorhebung ohne Neuaufbau
//! ([`PanelUi::highlight_active`]), Lage-Log für Klicktests, Höhe über niri. Was gezeichnet wird, entscheidet die
//! [`Source`]: Textaktionen hier ([`PanelUi::render`]), die Diagnose in `diag_view.rs`, die Befehlspalette in
//! `pal_view.rs` (mit der Eingabezeile `PanelUi::pal_entry`, W5).
//!
//! Text der Markierung und der Antwort nie loggen.

use crate::actions::store::SecretText;
use crate::link::Msg;
use crate::ui::markdown::{self, Block, Look};
use crate::ui::palette::{Palette, RADIUS, RADIUS_CHIP, RADIUS_CONTROL};
use crate::ui::panel::{noun, plural, Item, ItemKind, Mods, Panel, Phase, Scroll, Source as TextSource};
use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// `app_id` des Panels (Fensterregel)
pub const APP_ID: &str = "dictate-panel";
/// Breite (wie `default-column-width` der Fensterregel)
const WIDTH: i32 = 720;
/// Breite der Diagnose: der Bericht steht in Mono mit Zeilennummern (unter Awesome ebenfalls 800; Entscheidung Daniel,
/// 21.11.)
const WIDTH_DIAGNOSE: i32 = 800;
/// Pango setzt beim Umbruch mitten im Wort sonst Trennstriche, die nicht im Text stehen: Bericht und Befehle zeigen genau,
/// was gesendet wird bzw. dasteht
pub(crate) const NO_HYPHENS: &str = "insert_hyphens=\"false\"";
/// Höchste Höhe des Panels in logischen Pixeln (darüber scrollt der Inhalt); höchstens 85 % des Bildschirms
const MAX_HEIGHT: i32 = 760;
/// Niedrigste Höhe
const MIN_HEIGHT: i32 = 160;
/// Abstand zweier Größenänderungen beim Streaming
const RESIZE_EVERY: Duration = Duration::from_millis(150);

/// Wer das Panel gerade zeigt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Actions,
    Diagnose,
    Palette,
}

impl Source {
    fn window_title(self) -> &'static str {
        match self {
            Source::Actions => "dictate: text actions",
            Source::Diagnose => "dictate: diagnose",
            Source::Palette => "dictate: command palette",
        }
    }

    /// Breite, gesetzt vor jedem Zeigen (`set_default_size`; niri übernimmt sie für das schwebende Fenster auch gegen
    /// `default-column-width` der Fensterregel, im verschachtelten niri belegt: 800 → 720 → 800 am selben Fenster). Die
    /// Fensterregel in `niri/dictate-binds.kdl` (720) gilt damit nur noch als Rückfall
    pub fn width(self) -> i32 {
        match self {
            Source::Actions => WIDTH,
            Source::Diagnose => WIDTH_DIAGNOSE,
            Source::Palette => WIDTH,
        }
    }
}

/// Eingaben des Panels an den Hauptthread
#[derive(Debug, Clone)]
pub enum UiEv {
    Key {
        name: String,
        mods: Mods,
        down: bool,
    },
    Activate(String),
    Hover(Option<String>),
    /// Panel hat den Fokus verloren (anderes Fenster gewählt)
    FocusLost,
    /// Schließen über niri (Mod+Q)
    CloseRequest,
    /// Sekundentakt während der Arbeit
    Tick,
    /// Diagnose: Inhalt des Eingabefelds geändert (nur echte Eingabe mit Fokus; `Debug` zeigt nur die Länge)
    FieldText(SecretText),
    /// Diagnose: das Eingabefeld hat den Fokus bekommen (Klick hinein)
    FieldFocus,
    /// Palette: die Eingabezeile hat sich geändert (ohne Text: der Hauptthread liest ihn beim Verarbeiten, sonst überholte
    /// ein Tippereignis im Kanal ein programmatisches Setzen aus Verlauf oder Stimme)
    PalChanged,
    /// Palette, Abgang: niri hat den durchsichtigen Frame der Öffnung `gen` (`PanelUi::vanish`, Entscheidung 17)
    Vanished(u64),
}

/// Tasten, die auch bei fokussiertem Eingabefeld an den Automaten gehen (alle anderen bekommt das Feld)
const FIELD_KEYS: [&str; 5] = ["Escape", "Return", "KP_Enter", "Tab", "ISO_Left_Tab"];

/// Tasten, die auch bei fokussierter Eingabezeile der Palette an den Automaten gehen (dazu Ctrl+J/M); alle anderen
/// bekommt die Zeile (Cursor, Löschen, Einfügen, Ctrl+C/G nach L24)
const PAL_KEYS: [&str; 7] = ["Escape", "Return", "KP_Enter", "Tab", "ISO_Left_Tab", "Up", "Down"];

fn pal_key(name: &str, state: gdk::ModifierType) -> bool {
    PAL_KEYS.contains(&name) || (state.contains(gdk::ModifierType::CONTROL_MASK) && matches!(name, "j" | "m" | "J" | "M"))
}

/// Liegt der Fokus im Eingabefeld (der `gtk::Entry` selbst hat ihn nie, sondern sein inneres `GtkText`)?
pub(crate) fn field_has_focus(e: &gtk::Entry) -> bool {
    e.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)
}

/// Tippt der User gerade in diesem Feld? Nur ein gezeigtes Feld zählt: GTK ließ den Fokus nach dem Verbergen an der
/// Eingabezeile der Palette hängen (`FOCUS_WITHIN` blieb), und die Weiche gab danach Buchstaben der Textaktionen an die
/// unsichtbare Zeile statt an den Automaten (`tests/wayland/palette.sh`, P11)
fn typing_in(e: &gtk::Entry) -> bool {
    e.is_mapped() && field_has_focus(e)
}

/// Befehle an den Thread, der die Höhe über niri setzt (niri kann bis 1,5 s brauchen)
enum ResizeCmd {
    /// Neues Fenster (nach dem Zeigen): ID neu suchen
    Mapped,
    Height(i32),
}

pub struct PanelUi {
    pub(crate) window: gtk::Window,
    title: gtk::Label,
    close_btn: gtk::Box,
    pub(crate) scroller: gtk::ScrolledWindow,
    pub(crate) body: gtk::Box,
    pub(crate) footer: gtk::Box,
    /// Eingabefeld „What is wrong?“ der Diagnose: fester Platz zwischen Kopf und Scrollbereich, außerhalb jedes
    /// Neuaufbaus (behält Fokus und Cursor), nur in der Vorschau sichtbar (`diag_view.rs`)
    pub(crate) field_box: gtk::Box,
    pub(crate) field: gtk::Entry,
    field_changed: glib::SignalHandlerId,
    /// Eingabezeile der Befehlspalette: fester Platz zwischen Kopf und Scrollbereich, außerhalb jedes Neuaufbaus, nur
    /// bei der Quelle Palette sichtbar (`pal_view.rs`)
    pub(crate) pal_box: gtk::Box,
    pub(crate) pal_entry: gtk::Entry,
    pal_changed: glib::SignalHandlerId,
    /// Teile der Palette, die ohne Neuaufbau nachgezogen werden (Aufnahme, Pegel)
    pub(crate) pal: crate::pal_view::PalParts,
    /// Antwort (Arbeit, Ergebnis): wird beim Streaming allein neu aufgebaut
    doc: Option<gtk::Box>,
    pub(crate) secs: Option<gtk::Label>,
    /// Hinweis unter den Aktionen (hängt von der Hervorhebung ab)
    hint: Option<gtk::Label>,
    /// Sperr-Erkennung aus: Zeile im Panel (Markierung und Antwort werden beim Sperren nicht gelöscht)
    lock_off: bool,
    secs_timer: Option<glib::SourceId>,
    pub(crate) items: HashMap<String, gtk::Widget>,
    css: gtk::CssProvider,
    pub(crate) palette: Palette,
    pub(crate) tx: async_channel::Sender<Msg>,
    resize: std::sync::mpsc::Sender<ResizeCmd>,
    /// Zuletzt verlangte Höhe und Zeitpunkt (Streaming: höchstens alle [`RESIZE_EVERY`])
    height: Option<i32>,
    resized_at: Option<Instant>,
    /// Eine Größenanpassung ist für nach der Pause vorgemerkt (der Zeitgeber meldet sich mit `Tick`; nie entfernt, er
    /// läuft einmal ab)
    resize_later: Rc<std::cell::Cell<bool>>,
    /// Quelle und Ansicht des letzten Neuaufbaus (`None`: verborgen); ein Wechsel stellt Scroll und Höhe neu
    pub(crate) shown: Option<(Source, String)>,
    /// Codeblöcke, deren Copy gerade gelang („Copied“ für 1,5 s)
    copied: Rc<RefCell<HashMap<usize, glib::SourceId>>>,
    /// Diagnose, Vorschau: der gezeichnete Bericht (bleibt bei ↑/↓ stehen, siehe `diag_view.rs`)
    pub(crate) report: Option<crate::diag_view::Report>,
    /// Abgang der Palette: Frame-Uhr und Handler, der auf den durchsichtigen Frame wartet (`pal_view.rs`)
    pub(crate) vanish_wait: Rc<RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>>,
}

pub(crate) fn clickable(w: &impl IsA<gtk::Widget>, id: &str, tx: &async_channel::Sender<Msg>) {
    let w = w.as_ref();
    let click = gtk::GestureClick::new();
    click.set_button(1);
    let (tx2, id2, ww) = (tx.clone(), id.to_string(), w.downgrade());
    click.connect_released(move |_, _, x, y| {
        // nur wenn Druck und Loslassen auf demselben Element liegen
        let inside = ww.upgrade().is_some_and(|w| x >= 0.0 && y >= 0.0 && x <= f64::from(w.width()) && y <= f64::from(w.height()));
        if inside {
            let _ = tx2.send_blocking(Msg::Panel(UiEv::Activate(id2.clone())));
        }
    });
    w.add_controller(click);
    let motion = gtk::EventControllerMotion::new();
    let (tx3, id3) = (tx.clone(), id.to_string());
    motion.connect_enter(move |_, _, _| {
        let _ = tx3.send_blocking(Msg::Panel(UiEv::Hover(Some(id3.clone()))));
    });
    w.add_controller(motion);
    w.set_cursor_from_name(Some("pointer"));
}

pub(crate) fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(pango::WrapMode::WordChar);
    l.set_selectable(false);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

pub(crate) fn markup(m: &str, classes: &[&str]) -> gtk::Label {
    let l = label("", classes);
    l.set_markup(m);
    l
}

pub(crate) fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

pub(crate) fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub(crate) fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub(crate) fn chip(key: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(key));
    l.add_css_class("dp-chip");
    l.set_valign(gtk::Align::Center);
    l
}

impl PanelUi {
    pub fn new(tx: async_channel::Sender<Msg>, palette: Palette) -> PanelUi {
        let window = gtk::Window::new();
        window.set_title(Some("dictate: text actions"));
        window.set_decorated(false);
        window.set_default_size(WIDTH, -1);
        window.add_css_class("dictate-panel");
        let outer = vbox(0);
        let header = hbox(8);
        header.add_css_class("dp-header");
        let title = label("Text actions", &["dp-title"]);
        title.set_hexpand(true);
        title.set_valign(gtk::Align::Center);
        let close_btn = hbox(0);
        close_btn.add_css_class("dp-close");
        close_btn.set_halign(gtk::Align::End);
        close_btn.set_valign(gtk::Align::Center);
        let x = gtk::Label::new(Some("✕"));
        x.set_halign(gtk::Align::Center);
        x.set_valign(gtk::Align::Center);
        x.set_size_request(32, 32);
        close_btn.append(&x);
        clickable(&close_btn, "close", &tx);
        header.append(&title);
        header.append(&close_btn);
        let body = vbox(0);
        body.add_css_class("dp-body");
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_propagate_natural_height(true);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&body));
        let footer = hbox(8);
        footer.add_css_class("dp-footer");
        // Eingabefeld der Diagnose (siehe `field_box`)
        let field_box = vbox(6);
        field_box.add_css_class("dp-field-box");
        field_box.append(&label("WHAT IS WRONG?", &["dp-group-plain"]));
        let field = gtk::Entry::new();
        field.add_css_class("dp-field");
        field.set_placeholder_text(Some("Optional: the problem in a few words"));
        // kein Undo: Strg+Z holte sonst nach Schließen oder Sperre (Feld geleert) ein altes Symptom zurück
        field.set_enable_undo(false);
        // Eingabemethoden (fcitx5, ibus) sollen das ungefilterte Symptom nicht lernen (Durchsicht W4, F3)
        field.set_input_hints(gtk::InputHints::PRIVATE);
        field_box.append(&field);
        field_box.append(&label("Tab or click to type. Enter applies it to the report below, Esc discards the change.", &["dp-meta"]));
        field_box.set_visible(false);
        let (t6, fw) = (tx.clone(), field.downgrade());
        let field_changed = field.connect_changed(move |e| {
            // nur echte Eingabe (Fokus im Feld); programmatisches `set_text` blockiert den Handler zusätzlich
            if fw.upgrade().is_some_and(|f| field_has_focus(&f)) {
                let _ = t6.send_blocking(Msg::Panel(UiEv::FieldText(SecretText::new(e.text().to_string()))));
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let t7 = tx.clone();
        focus.connect_enter(move |_| {
            let _ = t7.send_blocking(Msg::Panel(UiEv::FieldFocus));
        });
        field.add_controller(focus);
        // Eingabezeile der Palette (siehe `pal_box`): wie das Feld der Diagnose ohne Undo, privat für Eingabemethoden,
        // höchstens so lang wie die Eingabe im Daemon
        let pal_box = vbox(6);
        pal_box.add_css_class("dp-field-box");
        let pal_entry = gtk::Entry::new();
        pal_entry.add_css_class("dp-field");
        pal_entry.set_placeholder_text(Some("Search windows, apps and projects, or type a command"));
        pal_entry.set_max_length(i32::try_from(crate::ui::cmdpal::MAX_INPUT).unwrap_or(i32::MAX));
        pal_entry.set_enable_undo(false);
        pal_entry.set_input_hints(gtk::InputHints::PRIVATE);
        pal_box.append(&pal_entry);
        pal_box.set_visible(false);
        let t8 = tx.clone();
        let pal_changed = pal_entry.connect_changed(move |_| {
            // programmatisches Setzen blockiert den Handler (`set_pal_text`)
            let _ = t8.send_blocking(Msg::Panel(UiEv::PalChanged));
        });
        outer.append(&header);
        outer.append(&pal_box);
        outer.append(&field_box);
        outer.append(&scroller);
        outer.append(&footer);
        window.set_child(Some(&outer));

        // Tasten im Capture: vor jedem Kind, alle gehen an den Automaten. Ausnahme: liegt der Fokus im Eingabefeld der
        // Diagnose, bekommt das Feld alle Tasten außer `FIELD_KEYS`, und der Automat sieht sie nicht (ein „c“ im Feld
        // löste sonst Copy aus). Ebenso die Eingabezeile der Palette mit ihrer eigenen Liste (`PAL_KEYS`). Das Loslassen
        // geht wie bisher an den Automaten, außer im Feld der Diagnose (für die Palette speist es nur die Totzeit,
        // `Gate::release`). Entschieden wird hier synchron am Fokus, nicht am Zustand des Automaten.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (t2, fw2, pw2) = (tx.clone(), field.downgrade(), pal_entry.downgrade());
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(name) = key.name() else { return glib::Propagation::Stop };
            if fw2.upgrade().is_some_and(|f| typing_in(&f)) && !FIELD_KEYS.contains(&name.as_str()) {
                return glib::Propagation::Proceed;
            }
            if pw2.upgrade().is_some_and(|e| typing_in(&e)) && !pal_key(&name, state) {
                return glib::Propagation::Proceed;
            }
            let _ = t2.send_blocking(Msg::Panel(UiEv::Key { name: name.to_string(), mods: mods(state), down: true }));
            glib::Propagation::Stop
        });
        let (t3, fw3) = (tx.clone(), field.downgrade());
        keys.connect_key_released(move |_, key, _, state| {
            let Some(name) = key.name() else { return };
            if fw3.upgrade().is_some_and(|f| typing_in(&f)) && !FIELD_KEYS.contains(&name.as_str()) {
                return;
            }
            let _ = t3.send_blocking(Msg::Panel(UiEv::Key { name: name.to_string(), mods: mods(state), down: false }));
        });
        window.add_controller(keys);
        let t4 = tx.clone();
        window.connect_is_active_notify(move |w| {
            if !w.is_active() && w.is_visible() {
                let _ = t4.send_blocking(Msg::Panel(UiEv::FocusLost));
            }
        });
        let t5 = tx.clone();
        window.connect_close_request(move |_| {
            let _ = t5.send_blocking(Msg::Panel(UiEv::CloseRequest));
            glib::Propagation::Stop
        });

        let css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(&WidgetExt::display(&window), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        let (rtx, rrx) = std::sync::mpsc::channel::<ResizeCmd>();
        std::thread::Builder::new().name("panel-size".into()).spawn(move || resizer(rrx)).expect("thread");
        let ui = PanelUi {
            window,
            title,
            close_btn,
            scroller,
            body,
            footer,
            field_box,
            field,
            field_changed,
            pal_box,
            pal_entry,
            pal_changed,
            pal: Default::default(),
            doc: None,
            secs: None,
            hint: None,
            lock_off: false,
            secs_timer: None,
            items: HashMap::new(),
            css,
            palette: palette.clone(),
            tx,
            resize: rtx,
            height: None,
            resized_at: None,
            resize_later: Rc::new(std::cell::Cell::new(false)),
            shown: None,
            copied: Rc::new(RefCell::new(HashMap::new())),
            report: None,
            vanish_wait: Rc::new(RefCell::new(None)),
        };
        ui.set_palette(palette);
        ui
    }

    /// Zustand der Sperr-Erkennung (vor jedem Neuaufbau gesetzt); `true`: neu aufbauen nötig
    pub fn set_lock_off(&mut self, off: bool) -> bool {
        std::mem::replace(&mut self.lock_off, off) != off
    }

    pub(crate) fn lock_off(&self) -> bool {
        self.lock_off
    }

    pub fn set_palette(&self, p: Palette) {
        self.css.load_from_string(&css(&p));
    }

    pub fn update_palette(&mut self, p: Palette) {
        // der Bericht trägt Farben im Markup: beim nächsten Zeichnen neu
        self.report = None;
        self.palette = p.clone();
        self.set_palette(p);
    }

    pub(crate) fn look(&self) -> Look {
        let p = &self.palette;
        Look { tail: p.on_surface_variant.hex(), code_bg: p.outline_variant.hex(), mono: "monospace".into(), error: p.error.hex(), accent: p.primary.hex() }
    }

    pub fn show(&mut self, source: Source) {
        // nach einem Abgang der Palette wieder deckend (vor dem Zeigen, nie in `hide`: ein deckender Frame vor dem
        // Verbergen wäre wieder im Bild)
        self.stop_vanish();
        self.window.set_opacity(1.0);
        self.window.set_title(Some(source.window_title()));
        self.window.set_default_size(source.width(), -1);
        // die Eingabezeile nur bei der Palette (vor dem Zeigen: sonst misst niri die erste Höhe ohne sie)
        self.pal_box.set_visible(source == Source::Palette);
        self.height = None;
        self.window.present();
        let _ = self.resize.send(ResizeCmd::Mapped);
        info!("panel shown");
    }

    pub fn hide(&mut self) {
        self.stop_secs();
        self.stop_vanish();
        // Fokus abgeben: GTK behielte ihn sonst an einem Feld, das das nächste Panel verbirgt (siehe `typing_in`)
        gtk::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
        self.window.set_visible(false);
        self.set_field_text("");
        self.field_box.set_visible(false);
        self.set_pal_text("");
        self.pal_entry.set_editable(true);
        self.pal_box.set_visible(false);
        self.pal = Default::default();
        clear(&self.body);
        clear(&self.footer);
        self.doc = None;
        self.hint = None;
        self.report = None;
        self.items.clear();
        for (_, id) in self.copied.borrow_mut().drain() {
            id.remove();
        }
        self.shown = None;
        debug!("panel hidden");
    }

    /// Text des Eingabefelds setzen, ohne dass `changed` ihn als Eingabe meldet
    pub(crate) fn set_field_text(&self, t: &str) {
        if self.field.text() == t {
            return;
        }
        self.field.block_signal(&self.field_changed);
        self.field.set_text(t);
        self.field.unblock_signal(&self.field_changed);
    }

    /// Text der Eingabezeile der Palette setzen (Verlauf, Stimme, Leeren), ohne dass `changed` ihn als Eingabe meldet;
    /// der Cursor steht danach am Ende
    pub(crate) fn set_pal_text(&self, t: &str) {
        if self.pal_entry.text() != t {
            self.pal_entry.block_signal(&self.pal_changed);
            self.pal_entry.set_text(t);
            self.pal_entry.unblock_signal(&self.pal_changed);
        }
        self.pal_entry.set_position(-1);
    }

    /// Fokus ins Eingabefeld (Cursor ans Ende) bzw. heraus (`Effect::Field`)
    pub fn field_focus(&self, on: bool) {
        if on {
            if !field_has_focus(&self.field) {
                // ohne Markieren: `grab_focus` markierte den ganzen Inhalt (`gtk-entry-select-on-focus`), und die
                // Markierung böte das ungefilterte Symptom als PRIMARY an (Durchsicht W4, F1)
                self.field.grab_focus_without_selecting();
            }
            self.field.set_position(-1);
        } else if field_has_focus(&self.field) {
            gtk::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
        }
    }

    /// Sekundentakt (`UiEv::Tick`) starten, falls er nicht läuft
    pub(crate) fn start_secs(&mut self) {
        if self.secs_timer.is_none() {
            let tx = self.tx.clone();
            self.secs_timer = Some(glib::timeout_add_local(Duration::from_secs(1), move || {
                let _ = tx.send_blocking(Msg::Panel(UiEv::Tick));
                glib::ControlFlow::Continue
            }));
        }
    }

    pub(crate) fn stop_secs(&mut self) {
        if let Some(id) = self.secs_timer.take() {
            id.remove();
        }
        self.secs = None;
    }

    /// Neuaufbau beginnen: Inhalt, Knöpfe und Bedienflächen leeren (✕ bleibt). `true`: Quelle oder Ansicht hat
    /// gewechselt
    pub(crate) fn begin(&mut self, source: Source, view: String) -> bool {
        self.begin_keeping(source, view, None)
    }

    /// Wie [`Self::begin`], aber `keep` (ein Kind des Inhalts) bleibt stehen, samt seiner gemessenen Größe (Bericht der
    /// Diagnose bei ↑/↓)
    pub(crate) fn begin_keeping(&mut self, source: Source, view: String, keep: Option<&gtk::Widget>) -> bool {
        let key = Some((source, view));
        let changed = self.shown != key;
        self.shown = key;
        match keep {
            Some(k) => {
                let mut c = self.body.first_child();
                while let Some(w) = c {
                    c = w.next_sibling();
                    if &w != k {
                        self.body.remove(&w);
                    }
                }
            }
            None => {
                clear(&self.body);
                self.report = None;
            }
        }
        clear(&self.footer);
        self.doc = None;
        self.hint = None;
        self.items.clear();
        self.items.insert("close".into(), self.close_btn.clone().upcast());
        changed
    }

    /// Neuaufbau abschließen: Hervorhebung, beim Wechsel nach oben, Höhe (`shrink`: auch kleiner), Lage-Log
    pub(crate) fn finish(&mut self, active: Option<&str>, changed: bool, shrink: bool) {
        self.highlight_active(active);
        if changed {
            self.scroller.vadjustment().set_value(0.0);
        }
        self.fit(shrink);
        self.log_layout();
    }

    pub(crate) fn set_title(&self, t: &str) {
        self.title.set_text(&t.to_uppercase());
    }

    /// Inhalt der Textaktionen ganz neu aufbauen
    pub fn render(&mut self, p: &Panel) {
        let phase_changed = self.begin(Source::Actions, format!("{:?}", p.phase));
        if p.phase != Phase::Working {
            self.stop_secs();
        }
        let title = match (p.phase, &p.action) {
            (Phase::Working | Phase::Result, Some(a)) => a.label.clone(),
            (Phase::Error, Some(a)) if p.err.as_ref().is_some_and(|e| e.retry) => a.label.clone(),
            _ => match p.source {
                TextSource::Selection => "Text actions",
                TextSource::Question => "Ask",
                TextSource::Clipboard => "Explain clipboard",
            }
            .to_string(),
        };
        self.set_title(&title);
        match p.phase {
            Phase::Closed => {}
            Phase::Grabbing => {
                let what = match p.source {
                    TextSource::Selection => "Reading the selection…",
                    TextSource::Question => "Checking the question…",
                    TextSource::Clipboard => "Reading the clipboard…",
                };
                self.body.append(&label(what, &["dp-soft"]));
            }
            Phase::Select => self.render_select(p),
            Phase::Working => self.render_working(p),
            Phase::Result => self.render_result(p),
            Phase::Error => self.render_error(p),
        }
        if self.lock_off && p.phase != Phase::Closed {
            let l = label(
                &format!("Lock detection is off: the {} and the answer are not cleared when the screen locks.", noun(p.source)),
                &["dp-meta", "dp-error"],
            );
            l.set_margin_top(12);
            self.body.append(&l);
            debug!("panel: lock detection off notice shown");
        }
        self.render_buttons(p);
        if let Some(h) = &self.hint {
            h.set_text(&select_hint(p));
        }
        // Beim Zustandswechsel passt sich die Höhe an; beim Start der Arbeit schrumpft sie nicht (die Antwort wächst
        // gleich wieder hinein)
        self.finish(p.active.as_deref(), phase_changed, phase_changed && p.phase != Phase::Working);
    }

    /// Nur mit Debug-Log (Prüfschritt `wayland`): Lage der Bedienflächen im Fenster nach dem übernächsten Bild, damit
    /// der Test mit der Maus genau trifft. Nur Kennungen und Zahlen, nie Inhalt.
    pub(crate) fn log_layout(&self) {
        if !tracing::enabled!(tracing::Level::DEBUG) {
            return;
        }
        let items: Vec<(String, glib::WeakRef<gtk::Widget>)> = self.items.iter().map(|(id, w)| (id.clone(), w.downgrade())).collect();
        let win = self.window.downgrade();
        let frames = std::cell::Cell::new(0u32);
        self.window.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            if let Some(win) = win.upgrade() {
                for (id, w) in &items {
                    if let Some(b) = w.upgrade().and_then(|w| w.compute_bounds(&win)) {
                        debug!("panel: layout {id} {:.0} {:.0} {:.0} {:.0}", b.x(), b.y(), b.width(), b.height());
                    }
                }
            }
            glib::ControlFlow::Break
        });
    }

    fn render_select(&mut self, p: &Panel) {
        let Some(g) = &p.grab else { return };
        let soft = self.palette.on_surface_variant.hex();
        let info = if p.source == TextSource::Question {
            format!("<b>Your question</b><span foreground=\"{soft}\">, {}</span>", plural(g.chars, "character", "characters"))
        } else {
            let from = match p.source {
                TextSource::Clipboard => " from the clipboard".to_string(),
                _ => p.target.app_id.as_deref().map(|a| format!(" from {a}")).unwrap_or_default(),
            };
            format!(
                "<b>{}</b><span foreground=\"{soft}\">, {}{}</span>",
                plural(g.chars, "character", "characters"),
                plural(g.lines, "line", "lines"),
                markdown::esc(&from)
            )
        };
        self.body.append(&markup(&info, &[]));
        if !g.any_allowed() && !g.secret.is_empty() {
            let b = vbox(8);
            b.add_css_class("dp-secret");
            b.set_margin_top(16);
            b.append(&label("Nothing was sent", &["dp-strong"]));
            for k in &g.secret {
                b.append(&label(&secret_name(k), &["dp-error"]));
            }
            let why = match (p.source, g.cloud) {
                (TextSource::Question, _) => format!(
                    "Your AI server is in the cloud ({}), so a question with a secret is not sent. Ask again without the secret, or switch to a local server.",
                    g.host
                ),
                (TextSource::Clipboard, _) => format!(
                    "Your AI server is in the cloud ({}), so clipboard text with a secret is not sent. Copy the text without the secret, or switch to a local server.",
                    g.host
                ),
                (TextSource::Selection, true) => format!(
                    "Your AI server is in the cloud ({}), so actions are off for selections with secrets. Select the text without the secret, or switch to a local server.",
                    g.host
                ),
                (TextSource::Selection, false) => {
                    "Actions are off for selections with secrets (Settings › Actions › Privacy). Select the text without the secret.".to_string()
                }
            };
            b.append(&label(&why, &["dp-soft"]));
            self.body.append(&b);
            return;
        }
        let where_ = if g.cloud {
            format!("Sends to <b>{}</b> when you pick an action", markdown::esc(&g.host))
        } else {
            format!("Runs on <b>{}</b>", markdown::esc(&g.host))
        };
        let w = markup(&where_, &["dp-soft"]);
        w.set_margin_top(4);
        self.body.append(&w);
        if !g.secret.is_empty() {
            let names: Vec<String> = g.secret.iter().map(|k| secret_name(k)).collect();
            let l = label(&format!("Contains: {}. It stays on this machine.", names.join(", ")), &["dp-soft"]);
            l.set_margin_top(4);
            self.body.append(&l);
        }
        let pv = vbox(4);
        pv.add_css_class("dp-preview");
        pv.set_margin_top(16);
        pv.append(&label(&g.preview, &[]));
        let shown = g.preview.lines().count() as u64;
        if g.lines > shown {
            pv.append(&label(&plural(g.lines - shown, "more line", "more lines"), &["dp-meta"]));
        }
        self.body.append(&pv);
        if g.actions.is_empty() {
            self.body.append(&label("No actions configured. Open Settings › Actions.", &["dp-soft"]));
        }
        let items = p.items();
        for (replace, head) in [(false, "Answer in this panel"), (true, "Replace the selection")] {
            let list: Vec<_> = g.actions.iter().filter(|a| a.replaces() == replace).collect();
            if list.is_empty() {
                continue;
            }
            let base = group_model(&list);
            let h = hbox(8);
            h.add_css_class("dp-group");
            let hl = label(&head.to_uppercase(), &[]);
            hl.set_hexpand(true);
            h.append(&hl);
            h.append(&label(&base, &["dp-meta"]));
            self.body.append(&h);
            for a in list {
                let id = format!("action:{}", a.name);
                let Some(it) = items.iter().find(|i| i.id == id) else { continue };
                let row = hbox(16);
                row.add_css_class("dp-row");
                if !it.enabled {
                    row.add_css_class("disabled");
                }
                row.append(&chip(&a.key));
                let l = label(&a.label, &[]);
                l.set_hexpand(true);
                l.set_valign(gtk::Align::Center);
                row.append(&l);
                if !a.allowed {
                    row.append(&label("Blocked", &["dp-meta"]));
                } else if !a.model.is_empty() && a.model != base {
                    row.append(&label(&a.model, &["dp-meta"]));
                }
                if it.enabled {
                    clickable(&row, &id, &self.tx);
                }
                self.items.insert(id, row.clone().upcast());
                self.body.append(&row);
            }
        }
        let l = label(&select_hint(p), &["dp-meta"]);
        l.set_margin_top(16);
        self.body.append(&l);
        self.hint = Some(l);
    }

    /// Frage der Palette über der Antwort: eine Zeile, gekürzt, damit klar ist, worauf geantwortet wird (Lua
    /// `question_line`)
    fn question_line(&self, p: &Panel) {
        if p.source != TextSource::Question || p.question.trim().is_empty() {
            return;
        }
        let mut q = p.question.split_whitespace().collect::<Vec<_>>().join(" ");
        if q.chars().count() > 160 {
            let mut cut: String = q.chars().take(157).collect();
            cut.push('…');
            crate::actions::store::wipe(&mut q);
            q = cut;
        }
        let l = label(&q, &["dp-soft"]);
        l.set_margin_bottom(16);
        self.body.append(&l);
        crate::actions::store::wipe(&mut q);
    }

    fn render_working(&mut self, p: &Panel) {
        let Some(a) = &p.action else { return };
        let g = p.grab.as_ref();
        self.question_line(p);
        let title = if a.replaces() || a.name.starts_with("mode:") {
            format!("Rewriting {}", plural(g.map_or(0, |g| g.chars), "character", "characters"))
        } else if p.text.is_empty() {
            "Waiting for the answer".to_string()
        } else {
            "Answering".to_string()
        };
        let row = hbox(16);
        let t = label(&title, &["dp-soft"]);
        t.set_hexpand(true);
        row.append(&t);
        let secs = label("", &["dp-meta"]);
        row.append(&secs);
        self.body.append(&row);
        self.secs = Some(secs);
        self.update_secs(p);
        self.start_secs();
        let doc = vbox(0);
        doc.set_margin_top(16);
        self.body.append(&doc);
        self.doc = Some(doc);
        self.fill_doc(p, true);
    }

    /// Sekunden im Kopf (Sekundentakt)
    pub fn update_secs(&self, p: &Panel) {
        if let (Some(l), Some(t0)) = (&self.secs, p.run_started) {
            let host = p.grab.as_ref().map(|g| g.host.as_str()).unwrap_or("");
            l.set_text(&format!("{host} · {} s", t0.elapsed().as_secs()));
        }
    }

    fn render_result(&mut self, p: &Panel) {
        self.question_line(p);
        let replace = p.result_output.as_deref() == Some("replace");
        let head = if replace {
            label(if !p.diff.is_empty() && p.show_marks { "NEW TEXT, CHANGES MARKED" } else { "NEW TEXT" }, &["dp-group-accent"])
        } else {
            let model =
                p.action.as_ref().map(|a| a.model.clone()).filter(|m| !m.is_empty()).or_else(|| p.grab.as_ref().map(|g| g.model.clone())).unwrap_or_default();
            markup(&format!("Answer from <b>{}</b>", markdown::esc(&model)), &["dp-soft"])
        };
        self.body.append(&head);
        let doc = vbox(0);
        doc.set_margin_top(16);
        self.body.append(&doc);
        self.doc = Some(doc);
        self.fill_doc(p, false);
        if p.truncated {
            self.body.append(&label("Cut at the length limit.", &["dp-meta"]));
        } else if let Some(n) = &p.note {
            self.body.append(&label(n, &["dp-meta"]));
        }
        if replace {
            let app = p.target.app_id.clone().unwrap_or_else(|| "the window".into());
            let hint = match p.replace_blocker() {
                Some(why) => why.to_string(),
                None => format!("Replaces the selection in {app}. Keep that window focused after the panel closes."),
            };
            let l = label(&hint, &["dp-meta"]);
            l.set_margin_top(12);
            self.body.append(&l);
        }
        if let Some(n) = &p.notice {
            let l = label(n, &["dp-error"]);
            l.set_margin_top(12);
            self.body.append(&l);
        }
    }

    fn render_error(&mut self, p: &Panel) {
        let Some(e) = &p.err else { return };
        let t = label(&e.title, &[if e.neutral { "dp-strong" } else { "dp-error" }]);
        self.body.append(&t);
        if let Some(b) = &e.body {
            let l = label(b, &["dp-soft"]);
            l.set_margin_top(8);
            self.body.append(&l);
        }
    }

    fn render_buttons(&mut self, p: &Panel) {
        let left = hbox(8);
        let right = hbox(8);
        left.set_hexpand(true);
        for it in p.items().into_iter().filter(|i| i.kind != ItemKind::Row) {
            let b = self.button(&it);
            if it.left {
                left.append(&b);
            } else {
                right.append(&b);
            }
        }
        if p.phase == Phase::Working {
            let hint = if p.text.is_empty() { "Esc stops and keeps your selection." } else { "The answer appears as it arrives." };
            left.append(&label(hint, &["dp-meta"]));
        }
        self.footer.append(&left);
        self.footer.append(&right);
    }

    pub(crate) fn button(&mut self, it: &Item) -> gtk::Box {
        let b = hbox(8);
        b.add_css_class("dp-btn");
        b.add_css_class(match it.kind {
            ItemKind::Primary => "primary",
            ItemKind::Secondary => "secondary",
            ItemKind::Ghost | ItemKind::Row => "ghost",
        });
        let l = gtk::Label::new(Some(&it.label));
        b.append(&l);
        if let Some(k) = &it.key {
            b.append(&chip(k));
        }
        if it.enabled {
            clickable(&b, &it.id, &self.tx);
        } else {
            b.add_css_class("disabled");
        }
        self.items.insert(it.id.clone(), b.clone().upcast());
        b
    }

    /// Hervorhebung der Textaktionen setzen (ohne Neuaufbau, auch der Hinweis der Auswahl)
    pub fn highlight(&self, p: &Panel) {
        if let Some(h) = &self.hint {
            h.set_text(&select_hint(p));
        }
        self.highlight_active(p.active.as_deref());
    }

    /// Hervorhebung ohne Neuaufbau (beide Quellen): genau die Fläche `active` trägt `active`
    pub(crate) fn highlight_active(&self, active: Option<&str>) {
        for (id, w) in &self.items {
            if Some(id.as_str()) == active {
                w.add_css_class("active");
            } else {
                w.remove_css_class("active");
            }
        }
    }

    /// Antwort neu füllen (Streaming oder Ergebnis); wer am Ende stand, bleibt am Ende
    fn fill_doc(&mut self, p: &Panel, streaming: bool) {
        let Some(doc) = self.doc.clone() else { return };
        clear(&doc);
        let look = self.look();
        let replace = p.result_output.as_deref() == Some("replace");
        if replace && !streaming {
            let m = if !p.diff.is_empty() && p.show_marks { markdown::marked_markup(&p.diff, &look) } else { markdown::esc(&p.text) };
            doc.append(&markup(&m, &["dp-doc"]));
            return;
        }
        let mut prev_item = false;
        for b in markdown::parse(&p.text) {
            let is_item = matches!(b, Block::Item { .. });
            let w: gtk::Widget = match &b {
                Block::Gap => {
                    let g = vbox(0);
                    g.set_size_request(-1, 12);
                    g.upcast()
                }
                Block::Code { text, index } => {
                    let row = hbox(8);
                    row.add_css_class("dp-code");
                    row.set_margin_top(8);
                    row.set_margin_bottom(8);
                    let t = markdown::esc(if text.is_empty() { " " } else { text });
                    let l = markup(&format!("<span {NO_HYPHENS}>{t}</span>"), &["dp-mono"]);
                    l.set_wrap_mode(pango::WrapMode::Char);
                    l.set_hexpand(true);
                    row.append(&l);
                    if !streaming {
                        let id = format!("copy_block:{index}");
                        let done = self.copied.borrow().contains_key(index);
                        let c = label(if done { "Copied" } else { "Copy" }, &["dp-copy"]);
                        c.set_valign(gtk::Align::Start);
                        clickable(&c, &id, &self.tx);
                        self.items.insert(id, c.clone().upcast());
                        row.append(&c);
                    }
                    row.upcast()
                }
                Block::Item { level, marker, spans } => {
                    let row = hbox(8);
                    row.set_margin_start(*level as i32 * 24);
                    if prev_item {
                        row.set_margin_top(4);
                    }
                    let m = label(marker, &["dp-marker"]);
                    m.set_valign(gtk::Align::Start);
                    row.append(&m);
                    let l = markup(&markdown::spans_markup(spans, &look), &["dp-doc"]);
                    l.set_hexpand(true);
                    row.append(&l);
                    row.upcast()
                }
                Block::Heading(spans) => markup(&markdown::spans_markup(spans, &look), &["dp-doc", "dp-heading"]).upcast(),
                Block::Quote(spans) => markup(&markdown::spans_markup(spans, &look), &["dp-doc", "dp-quote"]).upcast(),
                Block::Para(spans) => markup(&markdown::spans_markup(spans, &look), &["dp-doc"]).upcast(),
            };
            prev_item = is_item;
            doc.append(&w);
        }
    }

    /// Streaming: nur die Antwort neu
    pub fn stream(&mut self, p: &Panel) {
        let adj = self.scroller.vadjustment();
        let follow = adj.value() + adj.page_size() >= adj.upper() - 4.0;
        if self.doc.is_none() || p.phase != Phase::Working {
            return self.render(p);
        }
        self.fill_doc(p, true);
        self.fit(false);
        if follow {
            let a = adj.clone();
            glib::idle_add_local_once(move || a.set_value(a.upper() - a.page_size()));
        }
    }

    /// Codeblock `index` kopiert: „Copied“ für 1,5 s
    pub fn block_copied(&mut self, index: usize, p: &Panel) {
        let tx = self.tx.clone();
        let copied = self.copied.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            copied.borrow_mut().remove(&index);
            let _ = tx.send_blocking(Msg::Panel(UiEv::Tick));
        });
        if let Some(old) = self.copied.borrow_mut().insert(index, id) {
            old.remove();
        }
        self.render(p);
    }

    pub fn copied_shown(&self) -> bool {
        !self.copied.borrow().is_empty()
    }

    pub fn scroll(&self, s: Scroll) {
        let a = self.scroller.vadjustment();
        let line = 24.0;
        let page = (a.page_size() - line).max(line);
        let v = match s {
            Scroll::LineUp => a.value() - line,
            Scroll::LineDown => a.value() + line,
            Scroll::PageUp => a.value() - page,
            Scroll::PageDown => a.value() + page,
            Scroll::Top => 0.0,
            Scroll::Bottom => a.upper(),
        };
        a.set_value(v.clamp(0.0, (a.upper() - a.page_size()).max(0.0)));
    }

    /// Höhe passend zum Inhalt über niri (Entscheidung 13). `shrink`: auch kleiner werden (Zustandswechsel außer zur
    /// Arbeit); sonst nur wachsen, beim Streaming höchstens alle [`RESIZE_EVERY`]
    pub(crate) fn fit(&mut self, shrink: bool) {
        if !self.window.is_visible() {
            return;
        }
        if !shrink {
            if let Some(t) = self.resized_at.filter(|t| t.elapsed() < RESIZE_EVERY) {
                // Streaming: gesammelt nach Ablauf der Pause
                if !self.resize_later.replace(true) {
                    let (tx, pending) = (self.tx.clone(), self.resize_later.clone());
                    glib::timeout_add_local_once(RESIZE_EVERY.saturating_sub(t.elapsed()), move || {
                        pending.set(false);
                        let _ = tx.send_blocking(Msg::Panel(UiEv::Tick));
                    });
                }
                return;
            }
        }
        // Natürliche Höhe des ganzen Inhalts (der Scrollbereich gibt die seines Inhalts weiter)
        let outer = self.window.child().expect("child");
        let width = self.shown.as_ref().map_or(WIDTH, |(s, _)| s.width());
        let (_, natural, _, _) = outer.measure(gtk::Orientation::Vertical, width);
        let max = self.max_height();
        let mut want = natural.clamp(MIN_HEIGHT, max);
        if !shrink {
            want = want.max(self.height.unwrap_or(0));
        }
        if self.height == Some(want) {
            return;
        }
        debug!("panel: height {want} (content {natural}, max {max})");
        self.height = Some(want);
        self.resized_at = Some(Instant::now());
        let _ = self.resize.send(ResizeCmd::Height(want));
    }

    /// Spätere Größenanpassung (nach der Pause beim Streaming)
    pub fn fit_pending(&mut self) {
        if self.resize_later.get() {
            return;
        }
        if self.window.is_visible() && self.shown.is_some() {
            self.fit(false);
        }
    }

    fn max_height(&self) -> i32 {
        let monitor = self.window.surface().and_then(|s| WidgetExt::display(&self.window).monitor_at_surface(&s));
        match monitor {
            Some(m) => MAX_HEIGHT.min(m.geometry().height() * 85 / 100),
            None => MAX_HEIGHT,
        }
    }
}

/// Hinweis unter den Aktionen: was die hervorgehobene Aktion sendet
fn select_hint(p: &Panel) -> String {
    let Some(g) = &p.grab else { return String::new() };
    match p.active.as_deref().and_then(|id| id.strip_prefix("action:")).and_then(|n| g.actions.iter().find(|a| a.name == n)) {
        Some(a) if g.cloud => format!("Click an action or press its key. {} sends {}.", a.label, plural(g.chars, "character", "characters")),
        Some(a) => format!("Click an action or press its key. {} runs on {}.", a.label, g.host),
        None => "Click an action or press its key.".to_string(),
    }
}

/// Beschreibung einer Geheimnis-Art (`secrets::describe`), sonst die Kennung lesbar gemacht
fn secret_name(kind: &str) -> String {
    match crate::actions::secrets::describe(kind) {
        Some(d) => d.to_string(),
        None => {
            let s = kind.replace('_', " ");
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn group_model(list: &[&crate::ui::panel::ActionInfo]) -> String {
    let mut count: HashMap<&str, usize> = HashMap::new();
    let mut best = ("", 0);
    for a in list {
        let n = count.entry(a.model.as_str()).or_default();
        *n += 1;
        if *n > best.1 {
            best = (a.model.as_str(), *n);
        }
    }
    best.0.to_string()
}

fn mods(state: gdk::ModifierType) -> Mods {
    Mods {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        logo: state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK | gdk::ModifierType::HYPER_MASK),
    }
}

/// Thread für die Höhe: sucht die eigene Fenster-ID und setzt die zuletzt verlangte Höhe (ältere verfallen)
fn resizer(rx: std::sync::mpsc::Receiver<ResizeCmd>) {
    let mut id: Option<u64> = None;
    while let Ok(mut cmd) = rx.recv() {
        // nur der neueste Wunsch zählt
        let mut height: Option<i32>;
        loop {
            match cmd {
                ResizeCmd::Mapped => {
                    id = None;
                    height = None;
                }
                ResizeCmd::Height(h) => height = Some(h),
            }
            match rx.try_recv() {
                Ok(c) => cmd = c,
                Err(_) => break,
            }
        }
        let Some(h) = height else { continue };
        if id.is_none() {
            // das Fenster erscheint bei niri erst nach dem Mappen: kurz nachfragen
            for _ in 0..10 {
                match crate::niri::own_window(APP_ID) {
                    Ok(Some(w)) => {
                        id = Some(w);
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                    Err(e) => {
                        debug!("panel: niri: {e}");
                        break;
                    }
                }
            }
        }
        let Some(w) = id else {
            debug!("panel: own window not found, height {h} not set");
            continue;
        };
        if let Err(e) = crate::niri::set_window_height(w, h) {
            warn!("panel: cannot set the height: {e}");
            id = None;
            continue;
        }
        // Oberkante bleibt bei niri: ragt das Panel jetzt unten hinaus, um den Überstand hochschieben (Befund Block C)
        match crate::niri::placement(w, APP_ID) {
            Ok(Some(p)) => {
                if let Some(dy) = crate::niri::shift_into_view(&p, h) {
                    debug!("panel: move up {:.0} (top {:.0}, height {h}, output {:.0})", -dy, p.top, p.output_height);
                    if let Err(e) = crate::niri::move_window_by(w, dy) {
                        warn!("panel: cannot move the window: {e}");
                    }
                }
            }
            Ok(None) => debug!("panel: placement unknown, not moved"),
            Err(e) => debug!("panel: niri: {e}"),
        }
    }
}

/// Jede Klassenregel steht unter `window.dictate-panel`: sonst gewinnt `window.dictate-panel label { color: inherit }`
/// (spezifischer) und Farben wie `.dp-error`, `.dp-soft`, `.dp-meta` wirkten auf Labels nie (Fund im Rauchtest W4)
fn css(p: &Palette) -> String {
    let fg = p.on_surface.hex();
    let soft = p.on_surface_variant.hex();
    let bg = p.surface.hex();
    let line = p.outline_variant.hex();
    let acc = p.primary.hex();
    // Text und Rahmen auf gefüllten Akzentflächen (Hauptknopf, angekreuztes Kästchen): `on_primary` wie bei Material 3
    // und im Einstellungsfenster (Entscheidung 22), nicht `surface`
    let on_acc = p.on_primary.hex();
    let err = p.error.hex();
    let acc16 = p.primary.css(0.16);
    let line35 = p.outline_variant.css(0.45);
    // Ecken nach Lesart L42 aus derselben Quelle wie Overlay und Einstellungsfenster
    let (r, r8, r4) = (RADIUS, RADIUS_CONTROL, RADIUS_CHIP);
    format!(
        "window.dictate-panel {{ background-color: {bg}; color: {fg}; border-radius: {r}px; border: 1px solid {line}; }}
window.dictate-panel label {{ color: inherit; }}
window.dictate-panel .dp-header {{ padding: 16px 16px 8px 24px; }}
window.dictate-panel .dp-title {{ color: {soft}; font-weight: 600; letter-spacing: 1px; font-size: 0.85em; }}
window.dictate-panel .dp-close {{ min-width: 32px; min-height: 32px; border-radius: {r8}px; color: {soft}; }}
window.dictate-panel .dp-close:hover, window.dictate-panel .dp-close.active {{ background-color: {acc16}; color: {fg}; }}
window.dictate-panel .dp-body {{ padding: 4px 24px 16px 24px; }}
window.dictate-panel .dp-footer {{ padding: 8px 24px 20px 24px; }}
window.dictate-panel .dp-soft {{ color: {soft}; }}
window.dictate-panel .dp-meta {{ color: {soft}; font-size: 0.85em; }}
window.dictate-panel .dp-strong {{ font-weight: 600; font-size: 1.1em; }}
window.dictate-panel .dp-error {{ color: {err}; }}
window.dictate-panel .dp-preview {{ background-color: {line35}; border-radius: {r8}px; padding: 16px; }}
window.dictate-panel .dp-group {{ color: {soft}; font-size: 0.8em; font-weight: 600; letter-spacing: 1px; margin-top: 20px; margin-bottom: 6px; }}
window.dictate-panel .dp-group-accent {{ color: {acc}; font-size: 0.8em; font-weight: 600; letter-spacing: 1px; }}
window.dictate-panel .dp-row {{ min-height: 40px; padding: 0 12px; margin: 0 -12px; border-radius: {r8}px; }}
window.dictate-panel .dp-row.active {{ background-color: {acc16}; }}
window.dictate-panel .dp-row.disabled {{ opacity: 0.5; }}
window.dictate-panel .dp-chip {{ font-family: monospace; font-size: 0.85em; border: 1px solid {line}; border-radius: {r4}px; padding: 0 6px; min-width: 12px; }}
window.dictate-panel .dp-btn {{ padding: 6px 14px; border-radius: {r8}px; }}
window.dictate-panel .dp-btn.primary {{ background-color: {acc}; color: {on_acc}; }}
window.dictate-panel .dp-btn.primary .dp-chip {{ border-color: {on_acc}; }}
window.dictate-panel .dp-btn.secondary {{ border: 1px solid {line}; }}
window.dictate-panel .dp-btn.active {{ outline: 2px solid {acc}; outline-offset: 2px; }}
window.dictate-panel .dp-btn:hover {{ background-color: {acc16}; }}
window.dictate-panel .dp-btn.primary:hover {{ background-color: {acc}; }}
window.dictate-panel .dp-btn.disabled {{ opacity: 0.5; }}
window.dictate-panel .dp-secret {{ border: 1px solid {err}; border-radius: {r8}px; padding: 16px; }}
window.dictate-panel .dp-doc {{ line-height: 1.4; }}
window.dictate-panel .dp-heading {{ font-weight: 600; font-size: 1.1em; }}
window.dictate-panel .dp-quote {{ border-left: 3px solid {line}; padding-left: 12px; color: {soft}; }}
window.dictate-panel .dp-marker {{ color: {soft}; min-width: 16px; }}
window.dictate-panel .dp-code {{ background-color: {line35}; border-radius: {r8}px; padding: 12px 16px; }}
window.dictate-panel .dp-mono {{ font-family: monospace; }}
window.dictate-panel .dp-copy {{ color: {soft}; font-size: 0.85em; padding: 0 6px; border-radius: {r4}px; }}
window.dictate-panel .dp-copy:hover, window.dictate-panel .dp-copy.active {{ background-color: {acc16}; color: {fg}; }}
window.dictate-panel .dp-field-box {{ padding: 4px 24px 12px 24px; }}
window.dictate-panel .dp-group-plain {{ color: {soft}; font-size: 0.8em; font-weight: 600; letter-spacing: 1px; }}
window.dictate-panel entry.dp-field {{ min-height: 36px; border-radius: {r8}px; border: 1px solid {line}; background-color: {line35}; color: {fg}; padding: 0 10px; box-shadow: none; outline: none; }}
window.dictate-panel entry.dp-field:focus-within, window.dictate-panel entry.dp-field.active {{ border-color: {acc}; }}
window.dictate-panel entry.dp-field:disabled {{ opacity: 0.5; }}
window.dictate-panel .dp-box {{ min-width: 14px; min-height: 14px; border: 2px solid {soft}; border-radius: {r4}px; font-size: 0.75em; font-weight: 700; }}
window.dictate-panel .dp-box.on {{ background-color: {acc}; border-color: {acc}; color: {on_acc}; }}
window.dictate-panel .dp-dot {{ min-width: 6px; min-height: 6px; border-radius: 3px; background-color: {line}; }}
window.dictate-panel .dp-dot.run {{ background-color: {acc}; }}
window.dictate-panel .dp-dot.ok {{ background-color: {soft}; }}
window.dictate-panel .dp-dot.bad {{ background-color: {err}; }}
window.dictate-panel .dp-report {{ font-family: monospace; font-size: 0.85em; }}
window.dictate-panel .dp-sec {{ padding: 4px 4px 4px 8px; border-radius: 0 6px 6px 0; border-left: 3px solid transparent; margin-top: 12px; }}
window.dictate-panel .dp-sec.active {{ background-color: {line35}; }}
window.dictate-panel .dp-sec.bar, window.dictate-panel .dp-sec-body.bar {{ border-left-color: {acc}; }}
window.dictate-panel .dp-sec:hover {{ background-color: {acc16}; }}
window.dictate-panel .dp-sec-body {{ margin-left: 24px; padding-left: 8px; border-left: 3px solid transparent; margin-top: 4px; }}
window.dictate-panel .dp-ev {{ font-family: monospace; font-size: 0.85em; padding: 3px 8px; margin-left: 24px; margin-top: 4px; border-radius: 0 6px 6px 0; border-left: 3px solid transparent; color: {soft}; }}
window.dictate-panel .dp-ev.active {{ background-color: {line35}; border-left-color: {acc}; color: {fg}; }}
window.dictate-panel .dp-ev:hover {{ background-color: {acc16}; }}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::palette::{Rgb, FALLBACK};

    /// Entscheidung 22: auf gefüllten Akzentflächen steht `on_primary`, nie `surface` (Palette mit unterscheidbaren
    /// Rollen, damit die Prüfung nicht an Tokyo Night hängt, wo beide fast gleich sind)
    #[test]
    fn text_on_the_accent_is_on_primary() {
        let p = Palette { on_primary: Rgb(1, 2, 3), surface: Rgb(4, 5, 6), primary: Rgb(7, 8, 9), ..FALLBACK };
        let c = css(&p);
        assert!(c.contains(".dp-btn.primary { background-color: #070809; color: #010203; }"), "{c}");
        assert!(c.contains(".dp-btn.primary .dp-chip { border-color: #010203; }"));
        assert!(c.contains(".dp-box.on { background-color: #070809; border-color: #070809; color: #010203; }"));
        for line in c.lines().filter(|l| l.contains("background-color: #070809")) {
            assert!(!line.contains("color: #040506;"), "surface als Text auf dem Akzent: {line}");
        }
    }
}
