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

//! Bausteine des Einstellungsfensters: Felder, Chips, Knöpfe, Slider, Pixel-Meter und Pixel-Säulen. Farben nur über
//! [`th`] (Rollen der Noctalia-Palette), Ecken nach Lesart L42 ([`CHIP`], [`CONTROL`]).

use super::theme::*;
use egui::{pos2, vec2, Color32, CornerRadius, FontFamily, Layout, Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2};

pub fn page_header(ui: &mut Ui, title: &str, intro: &str) {
    ui.label(text(title, Size::Title, fam_title(), th().text));
    ui.add_space(4.0);
    ui.add(egui::Label::new(secondary(intro)).wrap());
    ui.add_space(32.0);
}

/// Gruppe: Label (0,8 em, Gewicht 600, Nebentext; L40), darunter der Inhalt, danach 32 px Abstand. Keine Kästen.
pub fn group<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.label(group_label(label));
    ui.add_space(8.0);
    let r = add(ui);
    ui.add_space(32.0);
    r
}

/// Feld mit Label (1 em, Nebentext) darüber, optionalem Hinweis und Fehlermeldung (0,85 em) darunter.
pub fn field<R>(ui: &mut Ui, label: &str, hint: Option<&str>, err: Option<&str>, add: impl FnOnce(&mut Ui, bool) -> R) -> R {
    ui.label(secondary(label));
    ui.add_space(0.0);
    let r = add(ui, err.is_some());
    if let Some(h) = hint {
        ui.add(egui::Label::new(small(h)).wrap());
    }
    if let Some(e) = err {
        ui.add_space(0.0);
        ui.label(text(e, Size::Small, FontFamily::Proportional, th().error));
    }
    ui.add_space(16.0);
    r
}

/// Rahmen eines Feldes wird rot, wenn ein Fehler anliegt.
fn error_scope<R>(ui: &mut Ui, error: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        if error {
            let w = &mut ui.style_mut().visuals.widgets;
            for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                s.bg_stroke = Stroke::new(1.0, th().error);
            }
            ui.style_mut().visuals.selection.stroke = Stroke::new(1.0, th().error);
        }
        add(ui)
    })
    .inner
}

pub fn text_edit(ui: &mut Ui, val: &mut String, error: bool, width: f32) -> Response {
    error_scope(ui, error, |ui| ui.add(egui::TextEdit::singleline(val).desired_width(width).margin(Margin::symmetric(16, 10))))
}

/// Wie [`text_edit`], bei leerem Feld mit grauem Platzhalter `hint` (etwa der Vorgabewert, der dann gilt).
pub fn text_edit_hint(ui: &mut Ui, val: &mut String, hint: &str, error: bool, width: f32) -> Response {
    error_scope(ui, error, |ui| {
        ui.add(egui::TextEdit::singleline(val).hint_text(RichText::new(hint).color(th().soft)).desired_width(width).margin(Margin::symmetric(16, 10)))
    })
}

/// Wie [`mono_edit`], bei leerem Feld mit grauem Platzhalter `hint`.
pub fn mono_edit_hint(ui: &mut Ui, val: &mut String, hint: &str, error: bool, width: f32) -> Response {
    error_scope(ui, error, |ui| {
        ui.add(
            egui::TextEdit::singleline(val)
                .font(TextStyle::Monospace)
                .hint_text(RichText::new(hint).color(th().soft))
                .desired_width(width)
                .margin(Margin::symmetric(16, 10)),
        )
    })
}

/// Warnung unter einem Feld: nicht blockierend, daher hellgrau statt rot.
pub fn field_warning(ui: &mut Ui, msg: &str) {
    ui.add(egui::Label::new(text(format!("Check: {msg}"), Size::Small, FontFamily::Proportional, th().text)).wrap());
}

pub fn mono_edit(ui: &mut Ui, val: &mut String, error: bool, width: f32) -> Response {
    error_scope(ui, error, |ui| ui.add(egui::TextEdit::singleline(val).font(TextStyle::Monospace).desired_width(width).margin(Margin::symmetric(16, 10))))
}

pub fn text_area(ui: &mut Ui, val: &mut String, rows: usize, error: bool, width: f32) -> Response {
    error_scope(ui, error, |ui| ui.add(egui::TextEdit::multiline(val).desired_rows(rows).desired_width(width).margin(Margin::symmetric(16, 12))))
}

/// Zahlenfeld (Ziehen oder Tippen) mit Einheit.
pub fn number<N: egui::emath::Numeric>(ui: &mut Ui, val: &mut N, min: N, max: N, suffix: &str, error: bool) -> Response {
    error_scope(ui, error, |ui| {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = vec2(16.0, 10.0);
            ui.spacing_mut().interact_size.y = 40.0;
            ui.add_sized([160.0, 40.0], egui::DragValue::new(val).range(min..=max).suffix(suffix.to_string()).speed(1.0))
        })
        .inner
    })
}

/// Dropdown mit Auswahl aus `(wert, anzeige)`.
pub fn combo(ui: &mut Ui, id: &str, val: &mut String, options: &[(String, String)], width: f32, error: bool) -> bool {
    let mut current = options.iter().find(|(v, _)| v == val).map(|(_, l)| l.clone()).unwrap_or_else(|| val.clone());
    if current.is_empty() {
        current = "Choose…".into();
    }
    let mut changed = false;
    error_scope(ui, error, |ui| {
        ui.spacing_mut().button_padding = vec2(16.0, 10.0);
        egui::ComboBox::from_id_salt(id).width(width).selected_text(body(current)).show_ui(ui, |ui| {
            ui.spacing_mut().button_padding = vec2(12.0, 6.0);
            for (v, l) in options {
                if ui.selectable_label(val == v, body(l.clone())).clicked() {
                    *val = v.clone();
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Ein Chip. Gewählt: Rahmen Akzent, Fläche Akzent 10 %, Text Akzent. Hover: Rahmen Akzent 40 %.
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let color = if selected { th().accent } else { th().soft };
    let galley = egui::WidgetText::from(text(label, Size::Body, FontFamily::Proportional, color)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let size = galley.size() + vec2(24.0, 16.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let stroke = if selected {
        th().accent
    } else if resp.hovered() {
        th().accent_a(HOVER_STROKE)
    } else {
        th().line
    };
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CHIP, th().accent_a(CHIP_FILL));
    }
    p.rect_stroke(rect, CHIP, Stroke::new(1.0, stroke), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    focus_ring(ui, &resp, rect, CHIP);
    resp
}

/// Fokusring 3 px außerhalb, Ecken der Form plus Abstand
fn focus_ring(ui: &Ui, resp: &Response, rect: Rect, corners: CornerRadius) {
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(3.0), grown(corners, 3.0), Stroke::new(2.0, th().accent), StrokeKind::Outside);
    }
}

/// Ein/Aus als Chip mit zwei Beschriftungen.
pub fn toggle(ui: &mut Ui, val: &mut bool, on: &str, off: &str) -> bool {
    toggle_chip(ui, val, on, off).clicked()
}

/// Wie [`toggle`], mit der Antwort des Chips (für das Lage-Log)
fn toggle_chip(ui: &mut Ui, val: &mut bool, on: &str, off: &str) -> Response {
    let r = chip(ui, if *val { on } else { off }, *val);
    if r.clicked() {
        *val = !*val;
    }
    r
}

/// Auswahl aus mehreren Chips.
pub fn chips(ui: &mut Ui, val: &mut String, options: &[(&str, String)]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (v, l) in options {
            if chip(ui, l, val == v).clicked() && val != v {
                *val = v.to_string();
                changed = true;
            }
        }
    });
    changed
}

/// Slider: Spur 8 px rund in Linienfarbe, Füllung Akzent, Daumen 24 px rund. Gibt `true` bei Änderung.
pub fn slider(ui: &mut Ui, val: &mut f64, min: f64, max: f64, step: f64, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - rect.left() - 12.0) / (rect.width() - 24.0)).clamp(0.0, 1.0) as f64;
            let mut v = min + t * (max - min);
            if step > 0.0 {
                v = (v / step).round() * step;
            }
            let v = v.clamp(min, max);
            if (v - *val).abs() > f64::EPSILON {
                *val = v;
                changed = true;
            }
        }
    }
    let t = ((*val - min) / (max - min)).clamp(0.0, 1.0) as f32;
    let cx = rect.left() + 12.0 + t * (rect.width() - 24.0);
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width(), 8.0));
    let p = ui.painter();
    p.rect_filled(rail, CornerRadius::same(4), th().line);
    p.rect_filled(Rect::from_min_max(rail.min, pos2(cx, rail.max.y)), CornerRadius::same(4), th().accent);
    p.circle_filled(pos2(cx, rect.center().y), 12.0, th().accent);
    if resp.has_focus() || resp.dragged() {
        p.circle_stroke(pos2(cx, rect.center().y), 15.0, Stroke::new(2.0, th().accent_a(HOVER_STROKE)));
    }
    changed
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Primary,
    Secondary,
}

fn button(ui: &mut Ui, label: &str, kind: Kind, enabled: bool) -> Response {
    let color = match kind {
        Kind::Primary => th().on_accent,
        Kind::Secondary => th().text,
    };
    let px = 12.0;
    let rich: RichText = text(label, Size::Body, FontFamily::Proportional, color);
    let galley = egui::WidgetText::from(rich).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
    let size = galley.size() + vec2(48.0, 2.0 * px);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let fade = if enabled { 1.0 } else { 0.3 };
    let p = ui.painter();
    match kind {
        // Gefüllter Knopf wie bei Material 3 ohne Glühen und Schatten; Hover als Zustandsschicht `on_primary` 8 %
        Kind::Primary => {
            p.rect_filled(rect, CONTROL, th().accent.gamma_multiply(fade));
            if enabled && resp.hovered() {
                p.rect_filled(rect, CONTROL, with_alpha(th().on_accent, STATE_LAYER));
            }
        }
        Kind::Secondary => {
            let c = if enabled && resp.hovered() { th().accent_a(HOVER_STROKE) } else { th().line };
            p.rect_stroke(rect, CONTROL, Stroke::new(1.0, c.gamma_multiply(fade)), StrokeKind::Inside);
        }
    }
    p.galley(rect.center() - galley.size() / 2.0, galley, color.gamma_multiply(fade));
    if enabled {
        if let Kind::Primary = kind {
            if resp.has_focus() {
                p.rect_stroke(rect.expand(3.0), grown(CONTROL, 3.0), Stroke::new(2.0, th().text), StrokeKind::Outside);
            }
        } else {
            focus_ring(ui, &resp, rect, CONTROL);
        }
    }
    resp
}

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Kind::Primary, enabled)
}

pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    button(ui, label, Kind::Secondary, true)
}

/// Kleiner Textknopf für Tabellenzeilen (Löschen, Kopieren).
pub fn link_button(ui: &mut Ui, label: &str) -> Response {
    let r = ui.add(
        egui::Button::new(text(label, Size::Small, FontFamily::Proportional, th().text))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(vec2(0.0, 40.0)),
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ein einzelner Pixel (Quadrat), etwa als Zustandspunkt
pub fn pixel(ui: &mut Ui, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_pixel(ui, rect, color);
    resp
}

fn paint_pixel(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_filled(rect, CornerRadius::same(1), color);
}

/// Pixel-Meter: 8 Zellen, Zelle 8 px auf 11 px Raster, gefüllt Akzent, leer Text 12 %.
pub fn pixel_meter(ui: &mut Ui, frac: f32) -> Response {
    let (cells, size, pitch) = (8usize, 8.0, 11.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(pitch * cells as f32 - (pitch - size), size), Sense::hover());
    let filled = (frac.clamp(0.0, 1.0) * cells as f32).round() as usize;
    for i in 0..cells {
        let r = Rect::from_min_size(rect.min + vec2(i as f32 * pitch, 0.0), Vec2::splat(size));
        paint_pixel(ui, r, if i < filled { th().accent } else { th().px_off() });
    }
    resp
}

/// Zustände der Pixel-Säulen.
#[derive(Clone, Copy, PartialEq)]
pub enum ColsMode {
    /// Zellen füllen sich nach dem Pegel (`0..=1`), mit `speech` im Akzent
    Level { frac: f32, speech: bool },
    /// Alles leer, die unterste Zelle der letzten Säule im Akzent
    Waiting,
}

/// Pixel-Säulen: 5 Säulen à 3 Zellen, Zelle 8 px auf 11 px Raster.
pub fn pixel_cols(ui: &mut Ui, mode: ColsMode) -> Response {
    let (cols, rows, size, pitch) = (5usize, 3usize, 8.0, 11.0);
    let w = pitch * cols as f32 - (pitch - size);
    let h = pitch * rows as f32 - (pitch - size);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let weights = [0.6f32, 0.85, 1.0, 0.8, 0.65];
    for (c, weight) in weights.iter().enumerate() {
        let filled = match mode {
            ColsMode::Level { frac, .. } => {
                let n = (frac.clamp(0.0, 1.0) * rows as f32 * weight + 0.5) as usize;
                n.min(rows).max(usize::from(frac > 0.08 && c == 2))
            }
            ColsMode::Waiting => 0,
        };
        for r in 0..rows {
            let x = rect.left() + c as f32 * pitch;
            let y = rect.bottom() - size - r as f32 * pitch;
            let cell = Rect::from_min_size(pos2(x, y), Vec2::splat(size));
            let color = match mode {
                ColsMode::Level { speech, .. } if r < filled => {
                    if speech {
                        th().accent
                    } else {
                        th().px_rest()
                    }
                }
                ColsMode::Waiting if c == cols - 1 && r == 0 => th().accent,
                _ => th().px_off(),
            };
            paint_pixel(ui, cell, color);
        }
    }
    resp
}

/// Trennlinie zwischen Tabellenzeilen: 1 px in Linienfarbe.
pub fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, th().line);
}

/// Liste von Texten mit Entfernen-Knopf und „Hinzufügen“.
pub fn string_list(ui: &mut Ui, items: &mut Vec<String>, add_label: &str, remove_label: &str, mono: bool, width: f32) -> bool {
    string_list_marked(ui, items, add_label, remove_label, mono, width, None)
}

/// Wie [`string_list`]; der Eintrag `bad` (Index aus der Fehlermeldung des Daemons) bekommt den roten Rahmen.
pub fn string_list_marked(ui: &mut Ui, items: &mut Vec<String>, add_label: &str, remove_label: &str, mono: bool, width: f32, bad: Option<usize>) -> bool {
    string_list_flagged(ui, items, add_label, remove_label, mono, width, |i| bad == Some(i))
}

/// Wie [`string_list`]; jeder Eintrag, für den `bad(index)` gilt, bekommt den roten Rahmen (mehrere zugleich).
pub fn string_list_flagged(
    ui: &mut Ui,
    items: &mut Vec<String>,
    add_label: &str,
    remove_label: &str,
    mono: bool,
    width: f32,
    bad: impl Fn(usize) -> bool,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (i, item) in items.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let err = bad(i);
            let r = if mono { mono_edit(ui, item, err, width) } else { text_edit(ui, item, err, width) };
            changed |= r.changed();
            if link_button(ui, remove_label).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        items.remove(i);
        changed = true;
    }
    if secondary_button(ui, add_label).clicked() {
        items.push(String::new());
        changed = true;
    }
    changed
}

/// Kopfzeile einer Seite mit Ein/Aus-Chip rechts neben dem Text.
pub fn feature_switch(ui: &mut Ui, val: &mut bool, on: &str, off: &str) {
    ui.horizontal(|ui| {
        let r = toggle_chip(ui, val, on, off);
        super::layout_log::note(ui, || "feature_switch".into(), &r);
    });
    ui.add_space(24.0);
}

/// Zelle fester Mindestgröße (Tabellenspalte).
pub fn cell<R>(ui: &mut Ui, size: Vec2, layout: Layout, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_size(size);
        add(ui)
    })
    .inner
}

/// Ecken einer um `by` vergrößerten Form (Fokusring)
fn grown(c: CornerRadius, by: f32) -> CornerRadius {
    let g = |r: u8| (f32::from(r) + by).round().min(255.0) as u8;
    CornerRadius { nw: g(c.nw), ne: g(c.ne), sw: g(c.sw), se: g(c.se) }
}

/// Zelle des Pixelzeichens von dictate
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlyphCell {
    /// Text 12 % (im SVG Weiß mit `fill-opacity` 0.12)
    Off,
    /// Text 50 % (0.5)
    Rest,
    /// Akzent (im SVG der Verlauf `url(#g)`)
    Accent,
}

/// Pixelzeichen von dictate (Vorlage V3): dieselben 5 × 3 Zellen wie `assets/logo/glyph-on-dark.svg` (Test
/// `logo_follows_the_svg`), aber aus Rollen gezeichnet statt als festes Bild, damit es zu jedem Schema passt. Das
/// Programm-Icon bleibt das SVG (Icons werden nicht umgefärbt).
pub const GLYPH: [[GlyphCell; 5]; 3] = {
    use GlyphCell::{Accent as A, Off as O, Rest as R};
    [[O, O, O, O, O], [R, R, R, A, R], [O, O, O, O, O]]
};

/// Maße des Lockups in `assets/logo/lockup-on-dark.svg` (Einheiten des SVG): Zelle, Raster, Ecken, Höhe des Zeichens,
/// Abstand zur Wortmarke, Schriftgröße (Gewicht 600), Grundlinie und Laufweite der Wortmarke
pub mod lockup {
    pub const CELL: f32 = 20.0;
    pub const PITCH: f32 = 26.0;
    pub const RADIUS: f32 = 2.0;
    pub const HEIGHT: f32 = 72.0;
    pub const GAP: f32 = 40.0;
    pub const FONT: f32 = 84.0;
    pub const BASELINE: f32 = 62.0;
    pub const TRACKING: f32 = -2.0;
}

/// Logo: Pixelzeichen `height` hoch, daneben die Wortmarke „dictate“ als Text in der Titelschrift (Gewicht 600), in
/// den Verhältnissen des Lockups (Schriftgröße, Abstand und Grundlinie relativ zur Höhe des Zeichens)
pub fn logo(ui: &mut Ui, height: f32) -> Response {
    let k = height / lockup::HEIGHT;
    let cols = GLYPH[0].len() as f32;
    let glyph_w = (cols * lockup::PITCH - (lockup::PITCH - lockup::CELL)) * k;
    let rich = RichText::new("dictate").font(egui::FontId::new(lockup::FONT * k, fam_title())).color(th().text).extra_letter_spacing(lockup::TRACKING * k);
    let galley = egui::WidgetText::from(rich).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, TextStyle::Body);
    // Grundlinie der ersten Zeile innerhalb der Galley
    let baseline = galley.rows.first().map_or(galley.size().y, |r| r.pos.y + r.row.glyphs.first().map_or(r.row.size.y, |g| g.pos.y));
    let text_dy = lockup::BASELINE * k - baseline;
    let top_extra = (-text_dy).max(0.0);
    let h = (top_extra + height).max(top_extra + text_dy + galley.size().y);
    let (rect, resp) = ui.allocate_exact_size(vec2(glyph_w + lockup::GAP * k + galley.size().x, h), Sense::hover());
    let p = ui.painter();
    let top = rect.top() + top_extra;
    for (r, row) in GLYPH.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let min = pos2(rect.left() + c as f32 * lockup::PITCH * k, top + r as f32 * lockup::PITCH * k);
            let color = match cell {
                GlyphCell::Off => th().px_off(),
                GlyphCell::Rest => th().px_rest(),
                GlyphCell::Accent => th().accent,
            };
            p.rect_filled(Rect::from_min_size(min, Vec2::splat(lockup::CELL * k)), lockup::RADIUS * k, color);
        }
    }
    p.galley(pos2(rect.left() + glyph_w + lockup::GAP * k, top + text_dy), galley, th().text);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wert eines Attributs in einem SVG-Element (nur für die eigenen, einfachen Dateien unter `assets/logo/`)
    fn attr<'a>(el: &'a str, name: &str) -> Option<&'a str> {
        let key = format!(" {name}=\"");
        let i = el.find(&key)? + key.len();
        Some(&el[i..i + el[i..].find('"')?])
    }

    #[test]
    fn logo_follows_the_svg() {
        for svg in [include_str!("../../assets/logo/glyph-on-dark.svg"), include_str!("../../assets/logo/lockup-on-dark.svg")] {
            let mut grid = vec![];
            for el in svg.split('<').filter(|e| e.starts_with("rect ")) {
                let num = |n: &str| attr(el, n).unwrap().parse::<f32>().unwrap();
                assert_eq!((num("width"), num("height"), num("rx")), (lockup::CELL, lockup::CELL, lockup::RADIUS));
                let (c, r) = (num("x") / lockup::PITCH, num("y") / lockup::PITCH);
                assert!(c.fract() == 0.0 && r.fract() == 0.0, "Zelle auf dem Raster: {el}");
                let cell = match (attr(el, "fill"), attr(el, "fill-opacity")) {
                    (Some("url(#g)"), None) => GlyphCell::Accent,
                    (_, Some("0.12")) => GlyphCell::Off,
                    (_, Some("0.5")) => GlyphCell::Rest,
                    other => panic!("unbekannte Zelle {other:?}"),
                };
                grid.push((r as usize, c as usize, cell));
            }
            let want: Vec<_> = GLYPH.iter().enumerate().flat_map(|(r, row)| row.iter().enumerate().map(move |(c, &x)| (r, c, x))).collect();
            assert_eq!(grid, want);
        }
        let lockup_svg = include_str!("../../assets/logo/lockup-on-dark.svg");
        let t = lockup_svg.split('<').find(|e| e.starts_with("text ")).unwrap();
        let num = |n: &str| attr(t, n).unwrap().parse::<f32>().unwrap();
        let glyph_w = GLYPH[0].len() as f32 * lockup::PITCH - (lockup::PITCH - lockup::CELL);
        assert_eq!(num("x") - glyph_w, lockup::GAP);
        assert_eq!((num("y"), num("font-size"), num("letter-spacing")), (lockup::BASELINE, lockup::FONT, lockup::TRACKING));
        assert_eq!(attr(t, "font-weight"), Some("600"), "Titelschrift");
        assert!(t.ends_with(">dictate"), "Wortmarke klein geschrieben");
        assert_eq!(GLYPH.len() as f32 * lockup::PITCH - (lockup::PITCH - lockup::CELL), lockup::HEIGHT);
    }
}
