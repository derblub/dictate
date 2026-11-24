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

//! Thema des Einstellungsfensters aus der Noctalia-Palette (Entscheidung 9, W6 Block C; PLAN-wayland.md 6h): Farben
//! aus den Material-Rollen derselben Datei wie `dictate-ui` (`crate::ui::palette`, Rückfall Tokyo Night dunkel),
//! Hell/Dunkel nur aus `mode` der Palette (Vorlage V2, Regel 3; egui folgt sonst dem Systemthema), Ecken wie
//! Noctalias Panels (Lesart L42: Felder, Knöpfe, Zeilen 8, Chips 4, Popups 12), Schrift aus den GTK-Einstellungen
//! (L41, `super::fonts`), Typografie wie das Panel von `dictate-ui` (L40: keine Versalien, keine Laufweite, Größen als
//! Verhältnis zur Basisschrift, [`Size`]).
//!
//! [`Theme`] ist die einzige Stelle, an der Rollen zu Farben werden: die Widgets lesen ihre Farben nur über [`th`],
//! und [`Theme::text_pairs`] nennt jede Paarung von Text und Fläche, die sie zeichnen (Kontrasttest, Regel 5).
//! Das aktuelle Thema liegt je Thread (egui zeichnet nur im Hauptthread); [`apply`] setzt es zusammen mit dem Stil von
//! egui. [`Live`] beobachtet die Datei (`super::watch`, L43) und färbt im laufenden Fenster um; weil eframe die
//! Wurzel-`Ui` vor `logic` anlegt, zeichnet erst der Durchlauf nach dem Wechsel ganz im neuen Stil, und erst er
//! schreibt die Zeile von Messpunkt M2.

use super::{fonts, watch::Watch};
use crate::ui::palette::{Palette, Rgb, Source, FALLBACK, RADIUS, RADIUS_CHIP, RADIUS_CONTROL};
use crate::ui::wave::OFF_ALPHA;
use egui::{Color32, CornerRadius, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke, Style, TextStyle, Visuals};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Deckkraft (von 255) der Akzentflächen: gewählter Chip
pub const CHIP_FILL: u8 = 26;
/// aktive Zeile der Seitenleiste, Hover in Listen (wie `.dp-row.active` im Panel von `dictate-ui`: 16 %)
pub const ROW_FILL: u8 = 41;
/// markierter Text und gewählte Zeile in Auswahllisten
pub const SELECTION_FILL: u8 = 60;
/// Rahmen beim Hover (Chips, Knöpfe, Felder)
pub const HOVER_STROKE: u8 = 102;
/// Zustandsschicht beim Hover über dem Hauptknopf: `on_primary` mit 8 % wie bei Material 3
pub const STATE_LAYER: u8 = 20;

/// Farben des Fensters, aus den Rollen abgeleitet
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Inhalt und Aktionsleiste (`surface`)
    pub surface: Color32,
    /// Seitenleiste (`surface_container_low`)
    pub sidebar: Color32,
    /// Popups, Hinweisleiste (`surface_container`)
    pub raised: Color32,
    /// Felder, Auswahllisten, Zahlenfelder (`surface_container_high`)
    pub field: Color32,
    /// Text (`on_surface`)
    pub text: Color32,
    /// Nebentext, Labels, Platzhalter (`on_surface_variant`)
    pub soft: Color32,
    /// Akzent (`primary`)
    pub accent: Color32,
    /// Text auf dem Akzent, Hauptknopf (`on_primary`)
    pub on_accent: Color32,
    pub error: Color32,
    /// Rahmen und Trennlinien (`outline_variant`)
    pub line: Color32,
    /// Zwischenton der Pixelwelle (`Palette::mix`, wie im Overlay von `dictate-ui`)
    pub wave_mix: Color32,
}

fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

impl Theme {
    pub fn from_palette(p: &Palette) -> Theme {
        Theme {
            dark: p.dark,
            surface: c(p.surface),
            sidebar: c(p.surface_container_low),
            raised: c(p.surface_container),
            field: c(p.surface_container_high),
            text: c(p.on_surface),
            soft: c(p.on_surface_variant),
            accent: c(p.primary),
            on_accent: c(p.on_primary),
            error: c(p.error),
            line: c(p.outline_variant),
            wave_mix: c(p.mix()),
        }
    }

    /// Akzent mit Deckkraft `a` (von 255)
    pub fn accent_a(&self, a: u8) -> Color32 {
        with_alpha(self.accent, a)
    }

    /// leere Zelle der Pixel-Säulen und -Meter: Text mit der Deckkraft von `Tone::Off` im Overlay (12 %)
    pub fn px_off(&self) -> Color32 {
        with_alpha(self.text, (OFF_ALPHA * 255.0).round() as u8)
    }

    /// Pegel ohne Sprache: Text 50 %
    pub fn px_rest(&self) -> Color32 {
        with_alpha(self.text, 128)
    }

    /// Jede Paarung von Text und Fläche, die die Widgets zeichnen, mit der tatsächlich gemischten Fläche (Akzent mit
    /// Deckkraft über ihrem Grund). Ausgenommen: abgeschaltete Bedienelemente (Deckkraft 0,3 bzw. egui
    /// `disabled_alpha`), die bewusst zurücktreten. Wer ein Widget mit neuer Paarung baut, trägt sie hier ein.
    #[cfg(test)]
    pub fn text_pairs(&self) -> Vec<(&'static str, Color32, Color32)> {
        let over = |bg: Color32, a: u8| blend(bg, self.accent, a);
        vec![
            ("text on surface", self.text, self.surface),
            ("text on sidebar", self.text, self.sidebar),
            ("text on raised", self.text, self.raised),
            ("text on field", self.text, self.field),
            ("soft on surface", self.soft, self.surface),
            ("soft on sidebar", self.soft, self.sidebar),
            ("soft on raised", self.soft, self.raised),
            ("soft on field (placeholder)", self.soft, self.field),
            ("accent on surface (links, labels)", self.accent, self.surface),
            ("accent on selected chip", self.accent, over(self.surface, CHIP_FILL)),
            ("error on surface", self.error, self.surface),
            ("on_accent on accent (primary button)", self.on_accent, self.accent),
            ("on_accent on hovered primary button", self.on_accent, blend(self.accent, self.on_accent, STATE_LAYER)),
            ("text on active sidebar row", self.text, over(self.sidebar, ROW_FILL)),
            ("text on selection in a field", self.text, over(self.field, SELECTION_FILL)),
            ("text on selected popup row", self.text, over(self.raised, SELECTION_FILL)),
            ("text on hovered popup row", self.text, self.field),
        ]
    }
}

/// `fg` mit Deckkraft `a` (von 255) über dem deckenden `bg`, im sRGB-Raum wie egui mischt
#[cfg(test)]
pub fn blend(bg: Color32, fg: Color32, a: u8) -> Color32 {
    let t = f32::from(a) / 255.0;
    let ch = |b: u8, f: u8| (f32::from(b) + (f32::from(f) - f32::from(b)) * t).round() as u8;
    Color32::from_rgb(ch(bg.r(), fg.r()), ch(bg.g(), fg.g()), ch(bg.b(), fg.b()))
}

/// Kontrastverhältnis nach WCAG 2 (1 bis 21) zweier deckender Farben
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn lum(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

thread_local! {
    static CURRENT: Cell<Theme> = Cell::new(Theme::from_palette(&FALLBACK));
    /// Basisschrift in logischen Pixeln (aus `gtk-font-name`, vor dem Laden die Vorgabe von GTK)
    static BASE: Cell<f32> = Cell::new(fonts::describe(fonts::GTK_DEFAULT, 96.0).1);
}

/// Palette aus `path` laden (Regel 4: nur eine gewöhnliche Datei bis 64 KiB, sonst Rückfall); Herkunft bzw. Grund
/// des Rückfalls nach stderr, wie `dictate-ui` ins Log
pub fn load(path: &Path) -> (Palette, Source) {
    let (p, src) = Palette::load(path);
    log_source(path, &p, &src);
    (p, src)
}

/// Was ein neues Lesen der Datei auslöst: ins Log, sobald sich Herkunft oder Grund des Rückfalls ändern (Regel 4: auch
/// ein neuer Grund, während schon die eingebaute Palette gilt; Befund der Durchsicht in Block E), neu gestylt nur bei
/// einer anderen Palette
fn change(old: (&Palette, &Source), new: (&Palette, &Source)) -> (bool, bool) {
    (old.1 != new.1 || old.0 != new.0, old.0 != new.0)
}

fn log_source(path: &Path, p: &Palette, src: &Source) {
    match src {
        Source::File => eprintln!("dictate-settings: palette from {} ({})", path.display(), if p.dark { "dark" } else { "light" }),
        Source::Fallback(why) => eprintln!("dictate-settings: palette: built-in Tokyo Night ({why})"),
    }
}

/// Palette im laufenden Fenster (Lesart L43, Vorlage V2): Beobachter auf der Datei, neu gestylt wird nur, wenn sich
/// die gelesene Palette unterscheidet; danach eine Zeile für Messpunkt M2
pub struct Live {
    path: PathBuf,
    current: Palette,
    src: Source,
    watch: Watch,
    /// Ereignis, dessen Palette gesetzt, aber noch nicht in einem ganzen Durchlauf gezeichnet ist
    waiting: Option<Instant>,
}

impl Live {
    /// Schriften und Palette laden, Thema setzen. Der Beobachter startet vor dem Laden und weckt nach dem
    /// Scharfstellen einmal: eine Änderung zwischen Laden und Scharfstellen geht so nicht verloren.
    pub fn start(ctx: &egui::Context) -> Live {
        let path = crate::ui::palette::path();
        let c = ctx.clone();
        let watch = Watch::spawn(&path, "dictate-settings", move || c.request_repaint());
        let (current, src) = load(&path);
        install(ctx, &current);
        Live { path, current, src, watch, waiting: None }
    }

    /// In `logic`: nach einem Ereignis die Datei lesen und nur bei einer anderen Palette umfärben
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(at) = self.watch.take() else { return };
        let (p, src) = Palette::load(&self.path);
        let (log, restyle) = change((&self.current, &self.src), (&p, &src));
        if log {
            log_source(&self.path, &p, &src);
        }
        self.src = src;
        if !restyle {
            return;
        }
        apply(ctx, &p);
        self.current = p;
        self.waiting = Some(at);
    }

    /// Am Ende von `ui`: zeichnete dieser Durchlauf ganz im Stil der neuen Palette (die Wurzel-`Ui` des Durchlaufs, in
    /// dem `poll` umfärbte, trägt noch den alten), Zeile „theme: palette applied after N ms“ (vom ersten Ereignis
    /// an der Datei bis hierher; die Übergabe an den Compositor folgt danach)
    pub fn painted(&mut self, ui: &egui::Ui) {
        let Some(at) = self.waiting else { return };
        if **ui.style() == *ui.ctx().global_style() {
            eprintln!("dictate-settings: theme: palette applied after {} ms", at.elapsed().as_millis());
            self.waiting = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
}

/// Das aktuelle Thema (zuletzt mit [`apply`] gesetzt, sonst Tokyo Night dunkel)
pub fn th() -> Theme {
    CURRENT.with(Cell::get)
}

/// Größen als Verhältnis zur Basisschrift wie im Panel von `dictate-ui` (L40: `.dp-group` 0,8 em, `.dp-meta` 0,85 em,
/// `.dp-heading` 1,1 em); Zuordnung der Stellen in PLAN-wayland.md 6h, Lesart L50
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// Fließtext, Feldlabels, Seitenleiste, Knöpfe und Chips (1 em, Gewicht 400 wie `.dp-btn`)
    Body,
    /// Hinweise, Meldungen unter Feldern, Textknöpfe in Tabellen, Mono (0,85 em wie `.dp-meta`, `.dp-report`)
    Small,
    /// Gruppenlabels (0,8 em, Gewicht 600, `on_surface_variant`)
    Label,
    /// Seitentitel (1,1 em, Gewicht 600)
    Title,
}

impl Size {
    pub fn em(self) -> f32 {
        match self {
            Size::Body => 1.0,
            Size::Small => 0.85,
            Size::Label => 0.8,
            Size::Title => 1.1,
        }
    }

    /// Größe in logischen Pixeln bei der aktuellen Basisschrift
    pub fn px(self) -> f32 {
        BASE.with(Cell::get) * self.em()
    }
}

/// Familie mit Gewicht 600 (Seitentitel, Gruppenlabels); dieselbe Datei mit Achse `wght` oder die statische Datei,
/// die fontconfig für `semibold` nennt
pub fn fam_title() -> FontFamily {
    FontFamily::Name("title".into())
}

/// Text in einer Größe aus [`Size`] und Familie.
pub fn text(s: impl Into<String>, size: Size, fam: FontFamily, color: Color32) -> RichText {
    RichText::new(s).font(FontId::new(size.px(), fam)).color(color)
}

pub fn body(s: impl Into<String>) -> RichText {
    text(s, Size::Body, FontFamily::Proportional, th().text)
}

pub fn secondary(s: impl Into<String>) -> RichText {
    text(s, Size::Body, FontFamily::Proportional, th().soft)
}

pub fn small(s: impl Into<String>) -> RichText {
    text(s, Size::Small, FontFamily::Proportional, th().soft)
}

/// Gruppenlabel: wie geschrieben (keine Versalien, keine Laufweite), 0,8 em, Gewicht 600, Nebentext (L40)
pub fn group_label(s: &str) -> RichText {
    text(s, Size::Label, fam_title(), th().soft)
}

pub fn mono(s: impl Into<String>) -> RichText {
    text(s, Size::Small, FontFamily::Monospace, th().text)
}

/// Farbe mit Deckkraft `a` (von 255)
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Schriften laden (einmal beim Start): Basis und Titel aus `gtk-font-name` (L41), Mono `monospace`, dazu DejaVu Sans
/// als Rückfall für Zeichen, die der Basis fehlen (etwa „↔“ in „Translate DE ↔ EN“). Eine Datei, die nicht lädt, fehlt
/// nur (Grund im Log); dann gilt die nächste der Kette. egui bringt seit U2 keine eigene Schrift mehr mit
/// (`default_fonts` aus, PLAN-lizenz.md): liefert fontconfig gar nichts, sucht [`fonts::fallback_face`] ohne
/// fontconfig, und [`complete`] schließt die Lücken; ohne jede Schrift endet das Fenster mit einer klaren Meldung statt
/// leerer Kästchen.
pub fn install(ctx: &egui::Context, palette: &Palette) {
    let spec = fonts::gtk_font();
    BASE.with(|b| b.set(spec.px));
    let mut defs = FontDefinitions::empty();
    let base_pattern = fonts::pattern(&spec.families, None);
    let title_pattern = fonts::pattern(&spec.families, Some("semibold"));
    let mut found = fonts::resolve_all(&[&base_pattern, &title_pattern, "monospace", "DejaVu Sans"]).into_iter();
    let mut next = || found.next().unwrap_or_else(|| Err("fc-match gave no answer".into()));
    let add = |defs: &mut FontDefinitions, key: &str, face: Result<fonts::Face, String>, weight: f32, fam: FontFamily, front: bool| -> Option<fonts::Face> {
        let loaded = face.and_then(|face| fonts::load(&face, weight).map(|d| (face, d)));
        match loaded {
            Ok((face, data)) => {
                defs.font_data.insert(key.into(), std::sync::Arc::new(data));
                let list = defs.families.entry(fam).or_default();
                if front {
                    list.insert(0, key.into());
                } else {
                    list.push(key.into());
                }
                Some(face)
            }
            Err(e) => {
                eprintln!("dictate-settings: font {key}: {e}");
                None
            }
        }
    };
    let base = add(&mut defs, "base", next(), 400.0, FontFamily::Proportional, true);
    let title = add(&mut defs, "title", next(), 600.0, fam_title(), true);
    add(&mut defs, "mono", next(), 400.0, FontFamily::Monospace, true);
    add(&mut defs, "symbols", next(), 400.0, FontFamily::Proportional, false);
    let mut base = base;
    if family_empty(&defs, &FontFamily::Proportional) {
        let face = fonts::fallback_face(&fonts::fallback_dirs()).ok_or_else(|| "no fallback font file found".to_string());
        base = add(&mut defs, "fallback", face, 400.0, FontFamily::Proportional, true);
    }
    if let Err(e) = complete(&mut defs) {
        eprintln!("dictate-settings: {e}");
        std::process::exit(1);
    }
    let name = |f: &Option<fonts::Face>| f.as_ref().map_or_else(|| "none".to_string(), |f| fonts::file_name(&f.path));
    eprintln!("dictate-settings: font {} {:.2} px from {} (base {}, title {})", spec.families.join(", "), spec.px, spec.origin, name(&base), name(&title));
    ctx.set_fonts(defs);

    apply(ctx, palette);
}

fn family_empty(defs: &FontDefinitions, fam: &FontFamily) -> bool {
    defs.families.get(fam).is_none_or(|l| l.is_empty())
}

/// Ketten vervollständigen: der Titel fällt auf die Kette der Basis zurück (Zeichen, die der fetten Datei fehlen), Mono
/// ohne eigene Datei auf die Basis. Ohne jede proportionale Schrift ein Fehler mit Abhilfe: egui zeichnete sonst nur
/// leere Kästchen (seit U2 ohne eigene Schriften)
fn complete(defs: &mut FontDefinitions) -> Result<(), String> {
    let chain: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if chain.is_empty() {
        return Err("no usable font: fc-match found none and no fallback file (such as DejaVuSans.ttf) was found \
                    under /usr/share/fonts; install a font package (for example ttf-dejavu) and start again"
            .into());
    }
    for fam in [fam_title(), FontFamily::Monospace] {
        let list = defs.families.entry(fam).or_default();
        for f in &chain {
            if !list.contains(f) {
                list.push(f.clone());
            }
        }
    }
    Ok(())
}

/// Thema aus `palette` setzen: aktuelles [`Theme`], Hell/Dunkel nur aus `mode` (egui folgte sonst dem System), Stil
/// des Modus; wirkt ab dem nächsten Frame
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let t = Theme::from_palette(palette);
    CURRENT.with(|c| c.set(t));
    let theme = if t.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    ctx.set_style_of(theme, style(&t));
    ctx.request_repaint();
}

fn style(t: &Theme) -> Style {
    let mut style = Style { visuals: if t.dark { Visuals::dark() } else { Visuals::light() }, ..Style::default() };
    style.text_styles = [
        (TextStyle::Heading, FontId::new(Size::Title.px(), fam_title())),
        (TextStyle::Body, FontId::new(Size::Body.px(), FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(Size::Body.px(), FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(Size::Small.px(), FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(Size::Small.px(), FontFamily::Monospace)),
    ]
    .into();
    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(8.0, 8.0);
    sp.button_padding = egui::vec2(16.0, 8.0);
    sp.interact_size = egui::vec2(40.0, 40.0);
    sp.text_edit_width = 360.0;
    sp.window_margin = Margin::same(16);
    sp.combo_width = 360.0;
    // Schmale, zurückhaltende Bildlaufleiste: erscheint nur bei Bedarf, Linie statt Balken
    sp.scroll = egui::style::ScrollStyle::floating();
    sp.scroll.floating_width = 4.0;
    sp.scroll.floating_allocated_width = 8.0;
    sp.scroll.foreground_color = false;

    let v = &mut style.visuals;
    v.dark_mode = t.dark;
    v.override_text_color = None;
    v.weak_text_color = Some(t.soft);
    v.panel_fill = t.surface;
    v.window_fill = t.raised;
    v.extreme_bg_color = t.field;
    v.text_edit_bg_color = Some(t.field);
    v.faint_bg_color = t.raised;
    v.code_bg_color = t.field;
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.text;
    v.error_fg_color = t.error;
    v.window_corner_radius = CornerRadius::same(RADIUS as u8);
    v.menu_corner_radius = CornerRadius::same(RADIUS as u8);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.selection.bg_fill = t.accent_a(SELECTION_FILL);
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.text_cursor.stroke = Stroke::new(2.0, t.accent);

    let r = CornerRadius::same(RADIUS_CONTROL as u8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.surface;
    w.noninteractive.weak_bg_fill = t.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    w.noninteractive.corner_radius = r;
    for (s, stroke) in [(&mut w.inactive, t.line), (&mut w.hovered, t.accent_a(HOVER_STROKE)), (&mut w.active, t.accent), (&mut w.open, t.accent)] {
        s.bg_fill = t.field;
        s.weak_bg_fill = t.field;
        s.bg_stroke = Stroke::new(1.0, stroke);
        s.fg_stroke = Stroke::new(1.0, t.text);
        s.corner_radius = r;
    }
    style
}

/// Ecken der Chips und Kästchen (L42)
pub const CHIP: CornerRadius = CornerRadius::same(RADIUS_CHIP as u8);
/// Ecken der Felder, Knöpfe und Zeilen (L42)
pub const CONTROL: CornerRadius = CornerRadius::same(RADIUS_CONTROL as u8);

#[cfg(test)]
mod tests {
    use super::*;

    /// U2: ohne Schriften von egui fallen Titel und Mono auf die Basis zurück; ohne jede Basis ein Fehler mit Abhilfe
    #[test]
    fn complete_fills_title_and_mono_from_the_base_chain() {
        let mut defs = FontDefinitions::empty();
        assert!(complete(&mut defs).unwrap_err().contains("install a font package"));
        let data = || std::sync::Arc::new(egui::FontData::from_static(epaint_default_fonts::HACK_REGULAR));
        defs.font_data.insert("base".into(), data());
        defs.font_data.insert("symbols".into(), data());
        defs.families.insert(FontFamily::Proportional, vec!["base".into(), "symbols".into()]);
        defs.families.insert(fam_title(), vec!["title".into()]);
        complete(&mut defs).unwrap();
        assert_eq!(defs.families[&fam_title()], ["title", "base", "symbols"]);
        assert_eq!(defs.families[&FontFamily::Monospace], ["base", "symbols"]);
        defs.families.insert(FontFamily::Monospace, vec!["mono".into()]);
        complete(&mut defs).unwrap();
        assert_eq!(defs.families[&FontFamily::Monospace], ["mono", "base", "symbols"]);
    }

    const DARK: &str = include_str!("../../tests/gui/fixtures/palette-dark.json");
    const LIGHT: &str = include_str!("../../tests/gui/fixtures/palette-light.json");

    fn theme(json: &str) -> Theme {
        Theme::from_palette(&Palette::parse(json).unwrap())
    }

    /// Regel 4: ein neuer Grund für den Rückfall kommt ins Log, auch wenn schon die eingebaute Palette gilt; gleiche
    /// Lage schreibt nichts, neu gestylt wird nur bei anderer Palette
    #[test]
    fn a_new_fallback_reason_is_logged_without_restyling() {
        let missing = Source::Fallback("missing".into());
        let broken = Source::Fallback("not JSON".into());
        assert_eq!(change((&FALLBACK, &missing), (&FALLBACK, &broken)), (true, false));
        assert_eq!(change((&FALLBACK, &broken), (&FALLBACK, &broken)), (false, false));
        let light = Palette::parse(LIGHT).unwrap();
        assert_eq!(change((&FALLBACK, &broken), (&light, &Source::File)), (true, true));
        assert_eq!(change((&light, &Source::File), (&light, &Source::File)), (false, false));
    }

    /// Regel 5 und Messpunkt M5: Kontrast jeder Textpaarung (Ausgabe mit `--nocapture`). Dunkel mindestens 4,5:1;
    /// hell nur gemessen, die Farben bleiben exakt wie Noctalia sie liefert (Entscheidung 21)
    #[test]
    fn text_pairs_have_contrast() {
        let mut below = Vec::new();
        for (what, fg, bg) in theme(DARK).text_pairs() {
            let r = contrast(fg, bg);
            println!("M5 Tokyo Night dark: {what} {r:.2}");
            if r < 4.5 {
                below.push(what);
            }
        }
        assert!(below.is_empty(), "unter 4,5:1 bei Tokyo Night dunkel: {below:?}");
        for (what, fg, bg) in theme(LIGHT).text_pairs() {
            println!("M5 Tokyo Night light (only measured): {what} {:.2}", contrast(fg, bg));
        }
    }

    /// L40: Größen als Verhältnis zur Basisschrift
    #[test]
    fn sizes_follow_the_base() {
        BASE.with(|b| b.set(16.0));
        assert_eq!((Size::Body.px(), Size::Label.px(), Size::Small.px()), (16.0, 12.8, 13.6));
        assert!((Size::Title.px() - 17.6).abs() < 1e-4);
        let l = group_label("Made by");
        assert_eq!(l.text(), "Made by", "keine Versalien");
    }

    #[test]
    fn contrast_and_blend() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
        assert_eq!(blend(Color32::BLACK, Color32::WHITE, 255), Color32::WHITE);
        assert_eq!(blend(Color32::BLACK, Color32::WHITE, 0), Color32::BLACK);
        assert_eq!(blend(Color32::from_rgb(0, 0, 0), Color32::from_rgb(200, 100, 50), 128), Color32::from_rgb(100, 50, 25));
    }

    /// Vorlage V2, Regel 3: Hell/Dunkel nur aus `mode`; das Systemthema von egui spielt keine Rolle
    #[test]
    fn mode_decides_light_and_dark() {
        let ctx = egui::Context::default();
        let light = Palette::parse(LIGHT).unwrap();
        apply(&ctx, &light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(th(), Theme::from_palette(&light));
        let style = ctx.global_style();
        assert!(!style.visuals.dark_mode);
        assert_eq!(style.visuals.panel_fill, Color32::from_rgb(0xe1, 0xe2, 0xe7));
        assert_eq!(style.visuals.widgets.inactive.corner_radius, CONTROL);
        assert_eq!(style.visuals.menu_corner_radius, CornerRadius::same(12));
        apply(&ctx, &FALLBACK);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_eq!(ctx.global_style().visuals.panel_fill, Color32::from_rgb(0x1a, 0x1b, 0x26));
        assert_eq!(th().accent, Color32::from_rgb(0x7a, 0xa2, 0xf7));
    }

    /// Das Fenster selbst rundet niri (Lesart L42): die Fensterregel für `dictate-settings` in der KDL trägt `RADIUS`
    /// und schneidet zu; ohne sie bliebe das Fenster eckig (Regel 7: KDL und Binary nur zusammen)
    #[test]
    fn window_rule_rounds_the_settings_window() {
        let kdl = include_str!("../../niri/dictate-binds.kdl");
        let rule = kdl
            .split("window-rule {")
            .skip(1)
            .map(|r| &r[..r.find("\n}").unwrap()])
            .find(|r| r.contains("match app-id=\"^dictate-settings$\""))
            .expect("Fensterregel für dictate-settings");
        let lines: Vec<&str> = rule.lines().map(str::trim).collect();
        assert!(lines.contains(&format!("geometry-corner-radius {RADIUS}").as_str()), "{rule}");
        assert!(lines.contains(&"clip-to-geometry true"), "{rule}");
        assert!(lines.contains(&"open-floating true"), "{rule}");
    }
}
