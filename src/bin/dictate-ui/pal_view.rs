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

//! Ansicht der Befehlspalette (PLAN-wayland.md 6g, Block C, Teil C2): zeichnet den Automaten `ui::cmdpal` in die
//! gemeinsame Hülle [`PanelUi`] (Fenster `dictate-panel`, Quelle [`Source::Palette`]). Texte und Vorrang nach
//! `c552237:docs/w5-vorlagen/cmdpal-spec.md` Abschnitt C (aus `palette.lua`), Meldungen unter niri nach Lesart L25.
//!
//! Die Eingabezeile (`PanelUi::pal_entry`) steht außerhalb jedes Neuaufbaus und behält Fokus und Cursor; ihr Text kommt
//! nur vom User und von `Effect::SetInput` (Verlauf, Stimme). Bedienelemente tragen die Kennungen des Automaten
//! (`row:N`, `confirm`, `cancel`, `close`, dazu `input` für Klicktests), nie Namen. Die Auswahl mit ↑/↓ und Hover ändert
//! nur Hervorhebung und Fuß ([`PanelUi::highlight_pal`]), nie den Inhalt (ein Neuaufbau unter der Maus verschluckte
//! Klicks).
//!
//! Inhalte nie loggen: Eingabe, Anzeige und Detail der Kandidaten tragen Fenstertitel, Namen und Pfade; das Debug-Log
//! nennt nur Ansicht und Zahlen.

use crate::link::Msg;
use crate::panel::{chip, clickable, hbox, label, markup, vbox, PanelUi, Source, UiEv};
use crate::ui::cmdpal::{one_space, CmdPal, Problem, VoicePhase};
use crate::ui::markdown;
use crate::ui::pal_exec::{Candidate, Class, Intent};
use crate::ui::panel::{Item, ItemKind};
use gtk::prelude::*;
use gtk::{gdk, pango};
use std::time::Instant;
use tracing::{debug, info};

/// Längste gezeigte Zeile von Anzeige und Detail (Zeichen; das Label kürzt danach mit „…“ auf die Breite)
const MAX_LINE_CHARS: usize = 512;
/// Längste gezeigte Adresse in der Vorschau der KI (`open_url`)
const MAX_URL_CHARS: usize = 2048;

/// Teile, die ohne Neuaufbau nachgezogen werden
#[derive(Default)]
pub struct PalParts {
    /// Text der Aufnahme
    voice: Option<gtk::Label>,
    /// Pegel der Aufnahme
    level: Option<gtk::LevelBar>,
}

/// Ansicht für den Vorrang (spec C, „Render-Vorrang“) und den Schlüssel des Neuaufbaus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Voice,
    Asking,
    Error,
    List,
}

fn view_of(p: &CmdPal) -> View {
    if p.voice.is_some() {
        View::Voice
    } else if p.asking.is_some() {
        View::Asking
    } else if p.err.is_some() {
        View::Error
    } else {
        View::List
    }
}

/// Einzeilig: Steuerzeichen und Umbrüche zu Leerzeichen, gekürzt
fn line(text: &str) -> String {
    one_space(text, MAX_LINE_CHARS)
}

fn one_line(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label(&line(text), classes);
    l.set_wrap(false);
    l.set_ellipsize(pango::EllipsizeMode::End);
    l.set_single_line_mode(true);
    l
}

/// Adresse der Vorschau: ohne Steuerzeichen, höchstens [`MAX_URL_CHARS`], darf umbrechen
fn url_text(url: &str) -> String {
    url.chars().filter(|c| !c.is_control()).take(MAX_URL_CHARS).collect()
}

fn badge(c: &Candidate) -> Option<&'static str> {
    match c.class {
        Class::Confirm => Some("Confirm"),
        Class::Ai => Some("AI"),
        Class::Run if c.intent.needs_confirm() => Some("Confirm"),
        Class::Run => None,
    }
}

fn button(id: &str, text: &str, key: &str, kind: ItemKind) -> Item {
    Item { id: id.into(), label: text.into(), key: Some(key.into()), kind, enabled: true, left: false }
}

/// Deckkraft im Abgang: die kleinste, die GTK noch zeichnet (es speichert sie als Byte). Bei genau 0 baut GTK 4.24 für
/// das Fenster keinen Knoten und übergibt keinen neuen Frame; niri behielt den deckenden (gemessen in D3, P27)
const VANISHED_OPACITY: f64 = 1.0 / 255.0;

/// Titel im Abgang (eine Fensterregel in `niri/dictate-binds.kdl` passt auf ihn); `show` setzt den Titel der Quelle
pub const LEAVING_TITLE: &str = "dictate: leaving";

impl PanelUi {
    /// Abgang (Entscheidung 17): Panel durchsichtig ([`VANISHED_OPACITY`]), dann `UiEv::Vanished(gen)`, sobald GTK diesen Frame
    /// an niri übergeben hat und der nächste Takt der Frame-Uhr kam: das erste `after-paint` danach gehört zum
    /// durchsichtigen Frame (Commit), das zweite kommt erst nach dem `frame`-Rückruf von niri für diesen Commit (GDK
    /// unter Wayland hält die Uhr bis dahin an). Ohne Uhr oder ohne Takt führt der Automat nach `VANISH_MAX` trotzdem
    /// aus. Fokus und Eingabe bleiben am Fenster, der Automat nimmt im Abgang nichts an
    pub fn vanish(&mut self, gen: u64) {
        self.stop_vanish();
        self.window.set_opacity(VANISHED_OPACITY);
        // Fensterregel in niri/dictate-binds.kdl (Titel): ohne Fokusring, Rahmen und Schatten, Deckkraft 0
        self.window.set_title(Some(LEAVING_TITLE));
        let Some(clock) = self.window.frame_clock() else {
            info!("panel: no frame clock, running after the deadline");
            return;
        };
        let started = Instant::now();
        debug!("panel: turning transparent");
        let (slot, tx, paints) = (self.vanish_wait.clone(), self.tx.clone(), std::cell::Cell::new(0u8));
        let id = clock.connect_after_paint(move |c| {
            paints.set(paints.get() + 1);
            if paints.get() < 2 {
                debug!("panel: transparent frame painted after {} ms", started.elapsed().as_millis());
                c.request_phase(gdk::FrameClockPhase::AFTER_PAINT);
                return;
            }
            if let Some((c, id)) = slot.borrow_mut().take() {
                c.disconnect(id);
            }
            info!("panel: transparent after {} ms", started.elapsed().as_millis());
            let _ = tx.send_blocking(Msg::Panel(UiEv::Vanished(gen)));
        });
        clock.request_phase(gdk::FrameClockPhase::AFTER_PAINT);
        *self.vanish_wait.borrow_mut() = Some((clock, id));
    }

    /// Wartet ein Abgang auf die Frame-Uhr, den Handler lösen (Zeigen, Verbergen, neuer Abgang)
    pub(crate) fn stop_vanish(&self) {
        if let Some((c, id)) = self.vanish_wait.borrow_mut().take() {
            c.disconnect(id);
        }
    }

    /// Inhalt der Palette ganz neu aufbauen (die Eingabezeile bleibt stehen)
    pub fn render_pal(&mut self, p: &CmdPal, now: Instant) {
        let t0 = Instant::now();
        let view = view_of(p);
        let key = if p.confirm.is_some() { "Confirm".to_string() } else { format!("{view:?}") };
        let changed = self.begin(Source::Palette, key);
        self.pal = PalParts::default();
        self.set_title(match (view, p.voice.as_ref().map(|v| v.phase)) {
            (View::Voice, Some(VoicePhase::Loading)) => "Getting ready",
            (View::Voice, Some(VoicePhase::Listening)) => "Listening",
            (View::Voice, _) => "Transcribing",
            (View::Asking, _) => "Asking AI",
            _ => "Command palette",
        });
        if view == View::Asking {
            self.start_secs();
        } else {
            self.stop_secs();
        }
        // während der Aufnahme schreibt nur die Stimme in die Zeile
        self.pal_entry.set_editable(view != View::Voice);
        self.items.insert("input".into(), self.pal_entry.clone().upcast());
        match view {
            View::Voice => self.render_pal_voice(p),
            View::Asking => self.render_pal_asking(p, now),
            View::Error => {
                if let Some(e) = &p.err {
                    self.pal_problem(e, 0);
                }
            }
            View::List => self.render_pal_list(p),
        }
        if self.lock_off() {
            let l = label("Lock detection is off: the palette stays open when the screen locks.", &["dp-meta", "dp-error"]);
            l.set_margin_top(12);
            self.body.append(&l);
            debug!("palette: lock detection off notice shown");
        }
        self.render_pal_footer(p);
        self.pal_focus();
        let built = t0.elapsed().as_millis();
        // die Liste wächst und schrumpft mit den Treffern (wie unter Awesome)
        self.finish(pal_active(p).as_deref(), changed, true);
        debug!("palette: rendered {view:?} with {} rows in {} ms (content {built} ms)", p.cands.len(), t0.elapsed().as_millis());
    }

    /// Fokus in die Eingabezeile (ohne Markieren: eine Markierung böte die Eingabe als PRIMARY an, W4 F1)
    fn pal_focus(&self) {
        if !crate::panel::field_has_focus(&self.pal_entry) {
            self.pal_entry.grab_focus_without_selecting();
            self.pal_entry.set_position(-1);
        }
    }

    fn pal_problem(&self, e: &Problem, top: i32) {
        let t = label(&e.title, &[if e.neutral { "dp-strong" } else { "dp-error" }]);
        t.set_margin_top(top);
        self.body.append(&t);
        if let Some(b) = &e.body {
            let l = label(b, &["dp-soft"]);
            l.set_margin_top(8);
            self.body.append(&l);
        }
    }

    fn render_pal_voice(&mut self, p: &CmdPal) {
        let Some(v) = &p.voice else { return };
        // einzeilig, vorn gekürzt: das Ende des Gesprochenen bleibt sichtbar (B5)
        let text = markup("", &["dp-doc"]);
        text.set_wrap(false);
        text.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        self.body.append(&text);
        let level = gtk::LevelBar::for_interval(0.0, 1.0);
        level.set_margin_top(12);
        level.set_visible(v.phase == VoicePhase::Listening);
        self.body.append(&level);
        self.pal = PalParts { voice: Some(text), level: Some(level) };
        self.pal_voice_text(p);
    }

    /// Text und Pegel der Aufnahme nachziehen (`Effect::VoiceText`, kein Neuaufbau)
    pub fn pal_voice_text(&self, p: &CmdPal) {
        let Some(v) = &p.voice else { return };
        if let Some(l) = &self.pal.voice {
            let tail = self.palette.on_surface_variant.hex();
            // leer: der Hinweis an seiner Stelle (beim Umwandeln nur noch „Esc cancels.“), sonst `stable` und grau `tail`
            let m = if v.stable.is_empty() && v.tail.is_empty() {
                let hint = if v.phase == VoicePhase::Finalizing { "Esc cancels." } else { "Speak your command. Esc cancels." };
                format!("<span foreground=\"{tail}\">{hint}</span>")
            } else if v.stable.is_empty() || v.tail.is_empty() {
                format!("{}<span foreground=\"{tail}\">{}</span>", markdown::esc(&v.stable), markdown::esc(&v.tail))
            } else {
                format!("{} <span foreground=\"{tail}\">{}</span>", markdown::esc(&v.stable), markdown::esc(&v.tail))
            };
            l.set_markup(&m);
        }
        if let Some(b) = &self.pal.level {
            b.set_visible(v.phase == VoicePhase::Listening);
            b.set_value(f64::from(v.level));
        }
    }

    fn render_pal_asking(&mut self, p: &CmdPal, now: Instant) {
        let row = hbox(16);
        let t = label("Esc cancels. Typing cancels too.", &["dp-soft"]);
        t.set_hexpand(true);
        row.append(&t);
        let secs = label("", &["dp-meta"]);
        row.append(&secs);
        self.body.append(&row);
        self.secs = Some(secs);
        self.update_pal_secs(p, now);
    }

    /// Sekunden der Frage an die KI (Sekundentakt)
    pub fn update_pal_secs(&self, p: &CmdPal, now: Instant) {
        if let (Some(l), Some(s)) = (&self.secs, p.ask_secs(now)) {
            l.set_text(&format!("{s} s"));
        }
    }

    fn render_pal_list(&mut self, p: &CmdPal) {
        if let Some(e) = &p.ask_err {
            self.pal_problem(e, 0);
        }
        if p.preview.is_some() && !p.cands.is_empty() {
            let l = label("AI SUGGESTION", &["dp-group-accent"]);
            l.set_margin_top(if p.ask_err.is_some() { 16 } else { 0 });
            self.body.append(&l);
        }
        if p.cands.is_empty() {
            let has_text = p.input.chars().any(|c| !c.is_whitespace());
            if p.ask_err.is_none() && p.shown_for.as_deref() == Some(p.input.as_str()) {
                self.body.append(&label(if has_text { "No matches" } else { "Nothing to show yet" }, &["dp-soft"]));
            }
            return;
        }
        for (i, c) in p.cands.iter().enumerate() {
            let id = format!("row:{i}");
            let row = hbox(16);
            row.add_css_class("dp-row");
            let col = vbox(2);
            col.set_hexpand(true);
            col.set_valign(gtk::Align::Center);
            let first = p.preview.is_some() && i == 0;
            let display = if first { format!("→ {}", line(&c.display)) } else { line(&c.display) };
            col.append(&one_line(&display, &[]));
            match (&c.intent, first) {
                // die Vorschau der KI zeigt die volle Adresse, die geöffnet würde
                (Intent::OpenUrl { url }, true) => col.append(&label(&url_text(url), &["dp-meta"])),
                _ => {
                    if let Some(d) = c.detail.as_deref().filter(|d| !d.trim().is_empty()) {
                        col.append(&one_line(d, &["dp-meta"]));
                    }
                }
            }
            row.append(&col);
            if let Some(b) = badge(c) {
                let l = label(b, &["dp-meta"]);
                l.set_wrap(false);
                l.set_valign(gtk::Align::Center);
                row.append(&l);
            }
            clickable(&row, &id, &self.tx);
            self.items.insert(id, row.clone().upcast());
            self.body.append(&row);
        }
    }

    /// Fuß: Tastenhinweise, bei einer Bestätigung stattdessen der Bestätigungsblock mit Knöpfen
    fn render_pal_footer(&mut self, p: &CmdPal) {
        if let Some(c) = p.confirm.and_then(|i| p.cands.get(i)) {
            let block = vbox(8);
            block.set_hexpand(true);
            block.append(&one_line(&c.display, &["dp-strong"]));
            block.append(&label("Press Enter again to confirm. Esc cancels.", &["dp-meta"]));
            let buttons = hbox(8);
            buttons.set_halign(gtk::Align::End);
            buttons.append(&self.button(&button("cancel", "Cancel", "Esc", ItemKind::Secondary)));
            buttons.append(&self.button(&button("confirm", "Confirm", "Enter", ItemKind::Primary)));
            block.append(&buttons);
            self.footer.append(&block);
            return;
        }
        self.pal_hints(p);
    }

    /// Tastenhinweise neu (Chip und Text je Paar, `CmdPal::footer`)
    fn pal_hints(&self, p: &CmdPal) {
        crate::panel::clear(&self.footer);
        let hints = hbox(16);
        for (key, text) in p.footer() {
            let pair = hbox(6);
            pair.append(&chip(key));
            let l = label(text, &["dp-meta"]);
            l.set_wrap(false);
            l.set_valign(gtk::Align::Center);
            pair.append(&l);
            hints.append(&pair);
        }
        self.footer.append(&hints);
    }

    /// Auswahl bewegt oder Eingabe geändert: nur Hervorhebung und Fuß (`Effect::Highlight`, kein Neuaufbau)
    pub fn highlight_pal(&self, p: &CmdPal) {
        if p.confirm.is_none() {
            self.pal_hints(p);
        }
        self.highlight_active(pal_active(p).as_deref());
    }
}

/// Hervorgehobene Fläche: der Knopf „Confirm“ während einer Bestätigung, sonst die gewählte Zeile
fn pal_active(p: &CmdPal) -> Option<String> {
    if p.confirm.is_some() {
        return Some("confirm".into());
    }
    if p.voice.is_some() || p.asking.is_some() || p.err.is_some() {
        return None;
    }
    p.sel.map(|i| format!("row:{i}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_single_and_short() {
        assert_eq!(line("a\nb\tc\u{2028}d"), "a b c d");
        assert_eq!(line(&"x".repeat(MAX_LINE_CHARS + 9)).chars().count(), MAX_LINE_CHARS);
        assert_eq!(url_text("https://e.org/\u{7}a\nb"), "https://e.org/ab");
        assert_eq!(url_text(&"u".repeat(MAX_URL_CHARS * 2)).chars().count(), MAX_URL_CHARS);
    }
}
