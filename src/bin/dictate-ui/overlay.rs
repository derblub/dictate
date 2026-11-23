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

//! Overlay als Layer-Fläche (PLAN-wayland.md 3.3, Spike S4): Layer `overlay`, Namensraum `dictate-overlay` (für
//! `layer-rule` in `niri/dictate-binds.kdl`), `KeyboardMode::None` (nimmt nie den Fokus) und eine **leere
//! Eingaberegion** nach jedem Map und jeder Größenänderung (Klicks gehen an das Fenster darunter). Einzige Ausnahme:
//! solange der Boot-Hinweis steht (`ui::boot_hint`, Entscheidung Daniel 21.11.), nimmt die ganze Fläche Klicks an
//! ([`Overlay::set_clickable`]), ein Klick öffnet die Diagnose; danach wieder leer. Keine niri-Shader auf Layer-Flächen:
//! Bewegung nur clientseitig.
//!
//! Inhalt wie das Awesome-Overlay (`render` in `init.lua`): Pixelwelle links, rechts die Mitschrift (stabil in
//! `on_surface`, Tail in `on_surface_variant`, höchstens drei Zeilen, vorn gekürzt) oder ein Hinweis. Farben aus der
//! Noctalia-Palette (`ui::palette`), Schrift aus den GTK-Einstellungen, Ecken wie Noctalias Panels. Die Textspalte wächst während
//! einer Sichtbarkeit nur, damit das Overlay beim Sprechen nicht zappelt.
//!
//! Zeit bis zum ersten Bild wird gemessen und geloggt (Prozessstart und je Einblenden bis `after-paint`), für den
//! Live-Test auf NVIDIA.

use crate::config::OverlayCfg;
use crate::ui::palette::{Palette, Rgb};
use crate::ui::view::{self, size, Content, Tone, Wave};
use gtk::prelude::*;
use gtk::{cairo, gdk, glib, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;
use tracing::{debug, info};

pub struct Overlay {
    window: gtk::Window,
    card: gtk::Box,
    /// Die Fläche nimmt Klicks an (nur der Boot-Hinweis); gelesen bei jedem Map und jeder Größenänderung
    clickable: Rc<Cell<bool>>,
    wave_area: gtk::DrawingArea,
    label: gtk::Label,
    wave: Rc<RefCell<Wave>>,
    palette: Rc<RefCell<Palette>>,
    css: gtk::CssProvider,
    cfg: OverlayCfg,
    /// Breite der Textspalte in dieser Sichtbarkeit (wächst nur)
    col_w: i32,
    /// Faktor für 24 px Zeilenhöhe zur Schrift (Pango `line_height`)
    lh_factor: Option<f64>,
    /// Zeitpunkt des letzten Einblendens, bis zum nächsten `after-paint`
    shown_at: Rc<Cell<Option<Instant>>>,
    /// Vorwärmen läuft: die Fläche ist unsichtbar (Deckkraft 0) gemappt, bis das erste Bild gezeichnet ist
    warming: Rc<Cell<bool>>,
    /// Das erste Bild ist gezeichnet, das Ausblenden nach dem Vorwärmen steht noch aus
    warm_hide: Rc<Cell<bool>>,
}

/// Eingaberegion: leer (das Overlay bekommt keinen Zeiger, Klicks gehen an das Fenster darunter) oder, solange es
/// anklickbar ist, die ganze Fläche (`None`)
fn set_input_region(surface: &gdk::Surface, clickable: bool) {
    if clickable {
        surface.set_input_region(None);
    } else {
        surface.set_input_region(Some(&cairo::Region::create()));
    }
}

impl Overlay {
    /// Fehler, wenn der Compositor keine Layer-Shell hat: ein gewöhnliches Fenster würde den Fokus nehmen
    pub fn new(process_start: Instant) -> Result<Overlay, String> {
        if !gtk4_layer_shell::is_supported() {
            return Err("the compositor does not support wlr-layer-shell; dictate-ui needs it for the overlay".into());
        }
        let window = gtk::Window::new();
        window.init_layer_shell();
        if !window.is_layer_window() {
            return Err("could not create a layer surface for the overlay".into());
        }
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("dictate-overlay"));
        window.set_keyboard_mode(KeyboardMode::None);
        // Abstand vom Bildschirmrand, nicht von Leisten (wie `place` in init.lua: Bildschirmgeometrie)
        window.set_exclusive_zone(-1);
        window.set_focusable(false);
        window.set_can_focus(false);
        window.set_can_target(false);
        window.add_css_class("dictate-overlay");
        window.set_title(Some("dictate overlay"));

        let card = gtk::Box::new(gtk::Orientation::Horizontal, size::GAP);
        card.add_css_class("dictate-card");
        let (ww, wh) = view::wave_size();
        let wave_area = gtk::DrawingArea::new();
        wave_area.set_content_width(ww as i32);
        wave_area.set_content_height(wh as i32);
        wave_area.set_valign(gtk::Align::Center);
        let wave = Rc::new(RefCell::new(Wave::default()));
        let palette = Rc::new(RefCell::new(crate::ui::palette::FALLBACK));
        let (w2, p2) = (wave.clone(), palette.clone());
        wave_area.set_draw_func(move |_, cr, w, h| draw_wave(cr, w, h, &w2.borrow(), &p2.borrow()));
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        // Natürliche Breite klein halten: die Breite bestimmt allein `set_size_request` (Textspalte)
        label.set_max_width_chars(1);
        label.set_xalign(0.0);
        label.set_valign(gtk::Align::Center);
        label.set_visible(false);
        card.append(&wave_area);
        card.append(&label);
        window.set_child(Some(&card));

        let css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(&gtk::prelude::WidgetExt::display(&window), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let shown_at: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        // Eingaberegion nach jedem Map und jeder Größenänderung neu setzen (S4: GTK setzt sie nicht selbst)
        let clickable = Rc::new(Cell::new(false));
        let c = clickable.clone();
        window.connect_map(move |w| {
            if let Some(s) = w.surface() {
                set_input_region(&s, c.get());
            }
        });
        let warming = Rc::new(Cell::new(false));
        let warm_hide = Rc::new(Cell::new(false));
        let (shown, warm, hide, c) = (shown_at.clone(), warming.clone(), warm_hide.clone(), clickable.clone());
        window.connect_realize(move |w| {
            let Some(s) = w.surface() else { return };
            set_input_region(&s, c.get());
            let c = c.clone();
            s.connect_layout(move |s, _, _| set_input_region(s, c.get()));
            let (shown, warm, hide, weak) = (shown.clone(), warm.clone(), hide.clone(), w.downgrade());
            if let Some(clock) = w.frame_clock() {
                clock.connect_after_paint(move |_| {
                    let Some(t) = shown.take() else { return };
                    if warm.replace(false) {
                        // Vorwärmen fertig (Renderer und Swapchain stehen): nach diesem Bild wieder ausblenden
                        info!("overlay: renderer ready, first frame {} ms after process start (invisible warm-up)", process_start.elapsed().as_millis());
                        hide.set(true);
                        let (hide, weak) = (hide.clone(), weak.clone());
                        glib::idle_add_local_once(move || {
                            if let Some(w) = weak.upgrade() {
                                if hide.replace(false) {
                                    w.set_visible(false);
                                    w.set_opacity(1.0);
                                }
                            }
                        });
                    } else {
                        info!("overlay: shown in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
                    }
                });
            }
        });

        let mut o = Overlay {
            window,
            card,
            clickable,
            wave_area,
            label,
            wave,
            palette,
            css,
            cfg: OverlayCfg::default(),
            col_w: 0,
            lh_factor: None,
            shown_at,
            warming,
            warm_hide,
        };
        o.apply_cfg(OverlayCfg::default());
        Ok(o)
    }

    /// Einstellungen aus dem `overlay`-Event: Rand, Abstand, Deckkraft
    pub fn apply_cfg(&mut self, cfg: OverlayCfg) {
        let top = cfg.position == "top";
        self.window.set_anchor(Edge::Top, top);
        self.window.set_anchor(Edge::Bottom, !top);
        let margin = cfg.bottom_margin.min(10_000) as i32;
        self.window.set_margin(Edge::Top, if top { margin } else { 0 });
        self.window.set_margin(Edge::Bottom, if top { 0 } else { margin });
        self.cfg = cfg;
        self.load_css();
    }

    /// Neue Palette (Noctalia hat Schema oder Modus gewechselt): Fläche, Rand, Text und Welle umfärben
    pub fn set_palette(&mut self, p: Palette) {
        *self.palette.borrow_mut() = p;
        self.load_css();
        self.wave_area.queue_draw();
    }

    pub fn palette(&self) -> Palette {
        self.palette.borrow().clone()
    }

    fn load_css(&self) {
        let p = self.palette.borrow();
        // Unter Wayland gibt es immer Alpha: aus = deckend, an = `opacity`
        let alpha = if self.cfg.transparency { self.cfg.opacity.clamp(0.0, 1.0) } else { 1.0 };
        self.css.load_from_string(&format!(
            "window.dictate-overlay {{ background: transparent; }}\n\
             .dictate-card {{ background-color: {bg}; border: 1px solid {line}; border-radius: {radius}px; \
             padding: {py}px {px}px; color: {fg}; }}",
            bg = p.surface.css(alpha),
            line = p.outline_variant.hex(),
            radius = crate::ui::palette::RADIUS,
            py = size::PAD_Y - 1,
            px = size::PAD_X - 1,
            fg = p.on_surface.hex(),
        ));
    }

    pub fn visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Klicks annehmen (Boot-Hinweis) oder wieder durchlassen. Nur auf Handlung: der Rückruf von [`Overlay::on_click`]
    /// kommt nur, solange die Fläche anklickbar ist
    pub fn set_clickable(&self, on: bool) {
        if self.clickable.replace(on) == on {
            return;
        }
        self.window.set_can_target(on);
        self.card.set_cursor_from_name(on.then_some("pointer"));
        if let Some(s) = self.window.surface() {
            set_input_region(&s, on);
        }
        debug!("overlay: input region {}", if on { "whole surface" } else { "empty" });
    }

    /// Rückruf für einen Klick (Loslassen der linken Taste) auf die anklickbare Fläche
    pub fn on_click(&self, f: impl Fn() + 'static) {
        let g = gtk::GestureClick::new();
        g.set_button(gdk::BUTTON_PRIMARY);
        let c = self.clickable.clone();
        g.connect_released(move |_, _, _, _| {
            if c.get() {
                f();
            }
        });
        self.window.add_controller(g);
    }

    /// Größte Breite des Overlays (logische Pixel): `max_width`, höchstens 90 % des Bildschirms
    fn max_width(&self) -> i32 {
        let display = gtk::prelude::WidgetExt::display(&self.window);
        let monitor = self
            .window
            .surface()
            .and_then(|s| display.monitor_at_surface(&s))
            .or_else(|| display.monitors().item(0).and_then(|m| m.downcast::<gdk::Monitor>().ok()));
        let screen = monitor.map(|m| m.geometry().width()).unwrap_or(1920);
        (self.cfg.max_width.min(100_000) as i32).min((f64::from(screen) * size::MAX_WIDTH_FRAC) as i32)
    }

    fn line_factor(&mut self) -> f64 {
        if let Some(f) = self.lh_factor {
            return f;
        }
        let layout = self.label.create_pango_layout(Some("A"));
        let (_, h) = layout.pixel_size();
        let f = if h > 0 { f64::from(size::LINE_PX) / f64::from(h) } else { 1.5 };
        self.lh_factor = Some(f);
        f
    }

    /// Inhalt zeigen (oder ausblenden); `listening` wählt bei Mitschrift zwischen Pegel und Warten
    pub fn show(&mut self, content: &Content, listening: bool) {
        let Some(mode) = content.wave_mode(listening) else {
            // Während des Vorwärmens blendet das Vorwärmen selbst aus
            if self.window.is_visible() && !self.warming.get() && !self.warm_hide.get() {
                self.window.set_visible(false);
                debug!("overlay hidden");
            }
            self.col_w = 0;
            return;
        };
        self.wave.borrow_mut().set_mode(mode);
        self.wave_area.queue_draw();

        let max_col = self.max_width() - 2 * size::PAD_X - view::wave_size().0 as i32 - size::GAP;
        let lh = self.line_factor();
        let wrap = |m: &str| format!("<span line_height=\"{lh:.3}\">{m}</span>");
        let layout = self.label.create_pango_layout(None);
        // natürliche Breite ohne Umbruch (plus Luft wie in init.lua)
        let pal = self.palette();
        let natural = content.markup(&pal, |_| true).map(|m| {
            layout.set_markup(&wrap(&m));
            layout.set_width(-1);
            layout.pixel_size().0 + 6
        });
        match natural {
            None => {
                self.label.set_visible(false);
            }
            Some(nat) => {
                let col = nat.clamp(1, max_col.max(1)).max(self.col_w).min(max_col.max(1));
                self.col_w = col;
                layout.set_width(col * pango::SCALE);
                layout.set_wrap(pango::WrapMode::WordChar);
                let limit = size::LINE_PX * size::MAX_LINES + 2;
                let markup = content
                    .markup(&pal, |m| {
                        layout.set_markup(&wrap(m));
                        layout.pixel_size().1 <= limit
                    })
                    .unwrap_or_default();
                self.label.set_markup(&wrap(&markup));
                self.label.set_size_request(col, -1);
                self.label.set_visible(true);
            }
        }
        if self.warming.replace(false) | self.warm_hide.replace(false) {
            // Vorwärmen noch nicht fertig: die Fläche ist schon gemappt, nur sichtbar machen
            self.window.set_opacity(1.0);
            self.shown_at.set(Some(Instant::now()));
            debug!("overlay shown (during warm-up)");
        } else if !self.window.is_visible() {
            self.shown_at.set(Some(Instant::now()));
            self.window.set_visible(true);
            debug!("overlay shown");
        }
    }

    /// Neuer Pegel (0..1), nur während des Zuhörens
    pub fn set_level(&self, l: f64) {
        self.wave.borrow_mut().set_level(l);
        self.wave_area.queue_draw();
    }

    /// Vorwärmen: die Fläche einmal unsichtbar (Deckkraft 0, ohne Eingabe, ohne Fokus) mappen und nach dem ersten Bild
    /// wieder ausblenden. Live gemessen (20.11., NVIDIA): das erste Einblenden kostete 286 ms (GTK legt dabei Renderer
    /// und Vulkan-Swapchain an), jedes weitere 1 bis 7 ms; so zahlt der Start diese Zeit statt des ersten Diktats
    pub fn prepare(&self) {
        self.warming.set(true);
        self.shown_at.set(Some(Instant::now()));
        self.window.set_opacity(0.0);
        self.window.set_visible(true);
    }
}

/// Zelle mit 1 px Rundung (`gears.shape.rounded_rect` in init.lua)
fn cell_path(cr: &cairo::Context, x: f64, y: f64, s: f64) {
    let r = size::CELL_RADIUS;
    cr.new_sub_path();
    cr.arc(x + s - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + s - r, y + s - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + s - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

fn draw_wave(cr: &cairo::Context, width: i32, height: i32, wave: &Wave, pal: &Palette) {
    let (tw, th) = view::wave_size();
    let x0 = ((f64::from(width) - tw) / 2.0).floor();
    let y0 = ((f64::from(height) - th) / 2.0).floor();
    let set = |c: Rgb, a: f64| {
        let (r, g, b) = c.unit();
        cr.set_source_rgba(r, g, b, a);
    };
    for (i, row, tone) in wave.cells() {
        let x = x0 + i as f64 * size::PITCH;
        let y = y0 + f64::from(row + 2) * size::PITCH;
        match tone {
            // Fehler: gestrichelter Slot in Textfarbe (`DASH_ALPHA`)
            Tone::Dash => {
                set(pal.on_surface, view::DASH_ALPHA);
                cr.set_line_width(1.0);
                cr.set_dash(&[2.0, 2.0], 0.0);
                cr.rectangle(x + 0.5, y + 0.5, size::CELL - 1.0, size::CELL - 1.0);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
            }
            // leere Zelle in Textfarbe (`OFF_ALPHA`)
            Tone::Off => {
                set(pal.on_surface, view::OFF_ALPHA);
                cell_path(cr, x, y, size::CELL);
                let _ = cr.fill();
            }
            Tone::Accent | Tone::Mix | Tone::Rest => {
                set(
                    match tone {
                        Tone::Accent => pal.primary,
                        Tone::Mix => pal.mix(),
                        _ => pal.on_surface,
                    },
                    1.0,
                );
                cell_path(cr, x, y, size::CELL);
                let _ = cr.fill();
            }
        }
    }
}
