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

//! Seite Overlay mit einer Vorschau, die das Overlay von `dictate-ui` verkleinert nachzeichnet (Lesart L45): dieselben
//! Maße und dieselbe Pixelwelle aus `src/ui/wave.rs` (per `#[path]`, Regel 2), Ecken `RADIUS`, Fläche `surface` mit der
//! eingestellten Deckkraft, Rahmen `outline_variant`. Bildschirm und Overlay im selben Maßstab, damit Lage, Abstand und
//! Breite stimmen (Lesart L52). Die Vorschau ist ruhend (feste Pegel, kein Zeitgeber, Messpunkt M3).

use crate::gui::{theme::*, widgets::*, App};
use crate::ui::palette::{RADIUS, RADIUS_CONTROL};
use crate::ui::wave::{self, size, Tone, Wave};
use egui::{pos2, vec2, Pos2, Rect, Sense, Stroke, StrokeKind, Ui};

/// Bildschirm, für den die Vorschau gilt (logische Pixel); das Overlay selbst misst den echten Bildschirm
const SCREEN: (f32, f32) = (1920.0, 1080.0);
/// Zeilen der Mitschrift in der Vorschau: eine lange Mitschrift bricht an der größten Breite um
const LINES: i32 = 2;
/// Breite der Vorschau höchstens (die Inhaltsspalte ist 720 breit)
const MAX_PREVIEW_W: f32 = 720.0;

/// Lage der Vorschau: Maßstab, Bildschirm, Overlay (Karte), Ursprung der Welle, Textspalte
#[derive(Debug, PartialEq)]
struct Preview {
    s: f32,
    screen: Rect,
    card: Rect,
    wave: Pos2,
    text: Rect,
}

/// Wie `dictate-ui` das Overlay bei langer Mitschrift legt: größte Breite `max_width`, höchstens
/// `MAX_WIDTH_FRAC` des Bildschirms; Innenabstand, Welle, Abstand, Textspalte; Abstand zum Rand `margin` oben oder
/// unten. Ein Abstand, der das Overlay aus dem Bild schöbe, hält es in der Vorschau am Rand.
fn layout(origin: Pos2, width: f32, max_width: u32, margin: u32, top: bool) -> Preview {
    let s = width / SCREEN.0;
    let screen = Rect::from_min_size(origin, vec2(width, SCREEN.1 * s));
    let (ww, wh) = wave::wave_size();
    let (ww, wh) = (ww as f32, wh as f32);
    let (pad_x, pad_y, gap) = (size::PAD_X as f32, size::PAD_Y as f32, size::GAP as f32);
    let max_w = (max_width as f32).min(SCREEN.0 * size::MAX_WIDTH_FRAC as f32);
    let col = (max_w - 2.0 * pad_x - ww - gap).max(1.0);
    let card_w = 2.0 * pad_x + ww + gap + col;
    let text_h = (size::LINE_PX * LINES) as f32;
    let card_h = 2.0 * pad_y + wh.max(text_h);
    let y = if top { margin as f32 } else { SCREEN.1 - margin as f32 - card_h }.clamp(0.0, SCREEN.1 - card_h);
    let card = Rect::from_min_size(pos2(screen.center().x - card_w * s / 2.0, screen.top() + y * s), vec2(card_w, card_h) * s);
    let wave = pos2(card.left() + pad_x * s, card.center().y - wh * s / 2.0);
    let text = Rect::from_min_size(pos2(wave.x + (ww + gap) * s, card.center().y - text_h * s / 2.0), vec2(col, text_h) * s);
    Preview { s, screen, card, wave, text }
}

/// Welle mit festen, mittleren Pegeln wie beim Sprechen: Säulen mit einer, drei und fünf Zellen, Akzent und
/// Zwischenton
fn sample_wave() -> Wave {
    let mut w = Wave::default();
    for l in [0.4, 0.7, 0.8, 0.75] {
        w.set_level(l);
    }
    w
}

fn preview(ui: &mut Ui, max_width: u32, margin: u32, top: bool, alpha: f32) {
    let width = ui.available_width().min(MAX_PREVIEW_W);
    let (rect, _) = ui.allocate_exact_size(vec2(width, width * SCREEN.1 / SCREEN.0), Sense::hover());
    let pv = layout(rect.min, width, max_width, margin, top);
    let p = ui.painter();
    p.rect_filled(pv.screen, RADIUS_CONTROL as f32, th().sidebar);
    p.rect_stroke(pv.screen, RADIUS_CONTROL as f32, Stroke::new(1.0, th().line), StrokeKind::Inside);
    let r = RADIUS as f32 * pv.s;
    p.rect_filled(pv.card, r, with_alpha(th().surface, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8));
    p.rect_stroke(pv.card, r, Stroke::new(1.0, th().line), StrokeKind::Inside);
    let (cell, pitch, cr) = (size::CELL as f32 * pv.s, size::PITCH as f32 * pv.s, size::CELL_RADIUS as f32 * pv.s);
    for (i, row, tone) in sample_wave().cells() {
        let c = Rect::from_min_size(pv.wave + vec2(i as f32 * pitch, (row + 2) as f32 * pitch), vec2(cell, cell));
        match tone {
            Tone::Accent => {
                p.rect_filled(c, cr, th().accent);
            }
            Tone::Mix => {
                p.rect_filled(c, cr, th().wave_mix);
            }
            Tone::Rest => {
                p.rect_filled(c, cr, th().text);
            }
            Tone::Off => {
                p.rect_filled(c, cr, th().px_off());
            }
            Tone::Dash => {
                p.rect_stroke(c, 0.0, Stroke::new(1.0, with_alpha(th().text, (wave::DASH_ALPHA * 255.0).round() as u8)), StrokeKind::Inside);
            }
        }
    }
    // Text als Balken (bei diesem Maßstab wäre Schrift unleserlich): erste Zeile ganz, zweite mit stabilem Teil in
    // `on_surface` und Tail in `on_surface_variant` wie die Mitschrift
    let line = size::LINE_PX as f32 * pv.s;
    let bar = (line * 0.4).max(2.0);
    let w = pv.text.width();
    let row = |n: f32| pv.text.top() + n * line + (line - bar) / 2.0;
    let bars = [(0.0, 0.0, 0.97, th().text), (1.0, 0.0, 0.42, th().text), (1.0, 0.45, 0.25, th().soft)];
    for (n, from, len, color) in bars {
        let b = Rect::from_min_size(pos2(pv.text.left() + from * w, row(n)), vec2(len * w, bar));
        p.rect_filled(b, bar / 2.0, color);
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    page_header(ui, "Overlay", "Where the live text appears while you speak.");
    let errs: Vec<Option<String>> =
        ["overlay.position", "overlay.max_width", "overlay.transparency", "overlay.opacity", "overlay.bottom_margin"].iter().map(|k| app.err(k)).collect();
    let o = &mut app.edited.overlay;

    group(ui, "Preview", |ui| {
        let alpha = if o.transparency { o.opacity as f32 } else { 1.0 };
        preview(ui, o.max_width, o.bottom_margin, o.position == "top", alpha);
    });

    group(ui, "Placement", |ui| {
        field(ui, "Position", None, errs[0].as_deref(), |ui, _| {
            chips(ui, &mut o.position, &[("bottom", "Bottom".into()), ("top", "Top".into())]);
        });
        field(ui, "Distance from the screen edge", None, errs[4].as_deref(), |ui, e| {
            number(ui, &mut o.bottom_margin, 0, 1000, " px", e);
        });
        field(ui, "Widest the text may grow", Some("Longer text wraps to a second line."), errs[1].as_deref(), |ui, e| {
            number(ui, &mut o.max_width, 200, 4000, " px", e);
        });
    });

    group(ui, "Transparency", |ui| {
        field(ui, "Background", None, errs[2].as_deref(), |ui, _| {
            toggle(ui, &mut o.transparency, "See-through", "Solid");
        });
        field(ui, "Opacity", Some("Only for a see-through background."), errs[3].as_deref(), |ui, _| {
            ui.horizontal(|ui| {
                slider(ui, &mut o.opacity, 0.1, 1.0, 0.05, 320.0);
                ui.label(mono(format!("{:.2}", o.opacity)));
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_scales_the_overlay_like_dictate_ui() {
        let o = pos2(10.0, 20.0);
        let p = layout(o, 720.0, 720, 48, false);
        assert_eq!(p.s, 0.375);
        assert_eq!(p.screen, Rect::from_min_size(o, vec2(720.0, 405.0)));
        // größte Breite 720 → 270 in der Vorschau, mittig; Höhe 2 × 16 + 2 Zeilen à 24 = 80 → 30
        assert_eq!(p.card.width(), 270.0);
        assert_eq!(p.card.height(), 30.0);
        assert_eq!(p.card.center().x, p.screen.center().x);
        assert_eq!(p.card.bottom(), p.screen.bottom() - 48.0 * 0.375);
        // Welle und Text liegen innen, Textspalte endet am Innenabstand
        assert!(p.card.contains(p.wave) && p.card.contains_rect(p.text));
        assert_eq!(p.wave.x - p.card.left(), 24.0 * 0.375);
        assert!((p.card.right() - p.text.right() - 24.0 * 0.375).abs() < 1e-4);
        // oben: Abstand vom oberen Rand
        assert_eq!(layout(o, 720.0, 720, 48, true).card.top(), p.screen.top() + 48.0 * 0.375);
        // höchstens 90 % des Bildschirms, auch bei größerem max_width
        assert!((layout(o, 720.0, 4000, 48, false).card.width() - 1728.0 * 0.375).abs() < 1e-3);
        // ein zu großer Abstand hält das Overlay am Rand
        assert_eq!(layout(o, 720.0, 720, 1000, false).card.top(), p.screen.top());
        assert_eq!(layout(o, 720.0, 720, 1000, true).card.bottom(), p.screen.bottom());
    }

    #[test]
    fn sample_wave_looks_like_speech() {
        let cells = sample_wave().cells();
        let tones: Vec<Tone> = cells.iter().map(|c| c.2).collect();
        for t in [Tone::Accent, Tone::Mix] {
            assert!(tones.contains(&t), "{t:?} fehlt");
        }
        let heights: std::collections::BTreeSet<usize> = (0..wave::COLS).map(|i| cells.iter().filter(|c| c.0 == i).count()).collect();
        assert!(heights.len() >= 2, "Säulen verschieden hoch: {heights:?}");
        assert!(heights.contains(&5) && !heights.contains(&0), "{heights:?}");
    }
}
