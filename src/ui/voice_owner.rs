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

//! Besitz der Aufnahme zwischen Fenster und Befehlspalette (W5 Teil C3, Lesart L23, `c552237:docs/w5-vorlagen/cmdpal-spec.md`
//! B5/D5): rein, ohne GTK. Die Events des Daemons tragen kein `target`; der Hauptthread von `dictate-ui` entscheidet
//! selbst, wohin Zustand, Mitschrift, Pegel und `final` gehen ([`Route`]).
//!
//! **Regeln** (aus `daemon.rs`: `start` während `finalizing` merkt eine nahtlose nächste Sitzung, `cancel` während
//! `finalizing` bricht das laufende `final` ab, `stop` löscht nur die gemerkte):
//! - Die Palette beansprucht eine Aufnahme nur, wenn der Client den Daemon ruhend sieht ([`VoiceOwner::claim`]); läuft oder
//!   endet gerade eine Aufnahme eines Fensters, bleibt Mod+E wie bisher (sonst träfe ein Abbruch der Palette das `final`
//!   des Fensters). Lesart L32.
//! - Der Besitz gilt **je Sitzung** und wird beim Wechsel nach `loading`/`listening` festgelegt; ein „verwirf das
//!   nächste `final`“ gilt nie über den Beginn der nächsten Sitzung hinaus (es verschluckte sonst ein Fensterdiktat).
//! - Abbruch der Palette (Esc, Schließen, Sperre, Mod+Ctrl+E): eine beanspruchte, noch nicht begonnene Sitzung wird
//!   verwaist (sie beginnt im Besitz [`Route::Discard`]), eine laufende geht an [`Route::Discard`]; ihr `final` wird
//!   verworfen, auch ein spätes mit Text. Endet die Sitzung der Palette ohne `final` (`idle`), ebenso.
//! - Scheitert das `start`/`toggle` der Palette ([`VoiceOwner::failed`]) oder bricht die Verbindung ab
//!   ([`VoiceOwner::reset`]), ist nichts mehr beansprucht.

/// Wohin ein Event der Aufnahme geht
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Overlay und Textausgabe wie bisher
    Window,
    /// Automat der Palette (`cmdpal::voice_*`), Overlay aus
    Palette,
    /// Nirgends hin (abgebrochene Aufnahme der Palette), Overlay aus
    Discard,
}

/// Besitz der nächsten, noch nicht begonnenen Sitzung
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    None,
    /// Die Palette hat `start`/`toggle` geschickt
    Palette,
    /// Sie hat es geschickt und danach abgebrochen (`cancel` ging hinterher)
    Orphan,
}

#[derive(Debug)]
pub struct VoiceOwner {
    next: Next,
    cur: Route,
    /// Letzter Zustand war `loading` oder `listening` (der Wechsel dorthin beginnt eine Sitzung)
    recording: bool,
    /// Laufende Nummer des `start`/`toggle` der Palette (`Commander`)
    seq: Option<u64>,
}

impl Default for VoiceOwner {
    fn default() -> Self {
        VoiceOwner::new()
    }
}

impl VoiceOwner {
    pub fn new() -> VoiceOwner {
        VoiceOwner { next: Next::None, cur: Route::Window, recording: false, seq: None }
    }

    /// Mod+E (Halten oder Umschalten) schickt `start`/`toggle`: gehört die Aufnahme der Palette? Nur bei offener
    /// Palette, ruhendem Daemon (`idle` = Zustand des Clients `idle`) und ohne laufende oder beanspruchte Sitzung der
    /// Palette. Bei `true` geht der Befehl mit `target:"palette"`, danach [`VoiceOwner::sent`].
    pub fn claim(&mut self, idle: bool, palette_open: bool) -> bool {
        if !palette_open || !idle || self.next != Next::None || (self.cur == Route::Palette && self.recording) {
            return false;
        }
        self.next = Next::Palette;
        true
    }

    /// Nummer des eben geschickten Befehls der Palette
    pub fn sent(&mut self, seq: u64) {
        self.seq = Some(seq);
    }

    /// Ein `start`/`toggle` ist gescheitert: war es das der Palette, beginnt keine Sitzung für sie
    pub fn failed(&mut self, seq: u64) {
        if self.seq == Some(seq) {
            self.seq = None;
            self.next = Next::None;
        }
    }

    /// Zustand des Daemons; liefert, wohin dieses Event (und alles bis zum nächsten) geht. `palette_open`: das Panel
    /// ist offen (eine beanspruchte Sitzung bei geschlossenem Panel beginnt verworfen)
    pub fn state(&mut self, recording: bool, finalizing: bool, palette_open: bool) -> Route {
        if recording && !self.recording {
            // neue Sitzung: ein altes „verwerfen“ endet hier
            self.cur = match std::mem::replace(&mut self.next, Next::None) {
                Next::None => Route::Window,
                Next::Palette if palette_open => Route::Palette,
                Next::Palette | Next::Orphan => Route::Discard,
            };
            self.seq = None;
        } else if !recording && !finalizing && self.cur == Route::Palette {
            // Sitzung der Palette ohne `final` zu Ende (`idle`, `error`): ein spätes `final` wird verworfen
            self.cur = Route::Discard;
        }
        self.recording = recording;
        self.cur
    }

    /// Mitschrift oder Pegel
    pub fn route(&self) -> Route {
        self.cur
    }

    /// `final`: wohin es geht; danach gehört nichts mehr der Palette (die nächste Sitzung entscheidet neu)
    pub fn final_event(&mut self) -> Route {
        self.recording = false;
        std::mem::replace(&mut self.cur, Route::Window)
    }

    /// Die Palette bricht ihre Aufnahme ab (Esc, Schließen, Sperre, Mod+Ctrl+E). `true`: es gab eine beanspruchte oder
    /// laufende, der Aufrufer schickt `cancel` (nach dem `start` der Reihe nach, siehe `Commander`)
    pub fn cancel(&mut self) -> bool {
        let mut any = false;
        if self.next == Next::Palette {
            self.next = Next::Orphan;
            any = true;
        }
        if self.cur == Route::Palette {
            self.cur = Route::Discard;
            any = true;
        }
        any
    }

    /// Das Overlay zeigt die Aufnahme nicht (sie gehört der Palette oder wird verworfen)
    pub fn hides_overlay(&self) -> bool {
        self.cur != Route::Window
    }

    /// Verbindung zum Daemon weg: nichts gehört mehr der Palette
    pub fn reset(&mut self) {
        *self = VoiceOwner::new();
    }
}

#[cfg(test)]
mod tests {
    use super::{Route, VoiceOwner};

    // Zustände wie `dictate-ui` sie meldet: (recording, finalizing)
    const LOADING: (bool, bool) = (true, false);
    const LISTENING: (bool, bool) = (true, false);
    const FINALIZING: (bool, bool) = (false, true);
    const IDLE: (bool, bool) = (false, false);

    fn st(o: &mut VoiceOwner, s: (bool, bool), open: bool) -> Route {
        o.state(s.0, s.1, open)
    }

    #[test]
    fn a_window_recording_stays_with_the_window() {
        let mut o = VoiceOwner::new();
        assert_eq!(st(&mut o, LOADING, false), Route::Window);
        assert_eq!(st(&mut o, LISTENING, false), Route::Window);
        assert_eq!(o.route(), Route::Window);
        assert_eq!(st(&mut o, FINALIZING, false), Route::Window);
        assert_eq!(o.final_event(), Route::Window);
        assert!(!o.hides_overlay());
    }

    #[test]
    fn mod_e_in_the_open_palette_records_for_it() {
        let mut o = VoiceOwner::new();
        assert!(!o.claim(true, false), "closed palette: the window");
        assert!(o.claim(true, true));
        o.sent(7);
        assert!(!o.claim(true, true), "only one claim");
        assert_eq!(st(&mut o, LOADING, true), Route::Palette);
        assert!(o.hides_overlay());
        assert_eq!(st(&mut o, LISTENING, true), Route::Palette);
        assert!(!o.claim(true, true), "its own recording runs");
        assert_eq!(st(&mut o, FINALIZING, true), Route::Palette);
        assert_eq!(o.final_event(), Route::Palette);
        assert_eq!(st(&mut o, IDLE, true), Route::Window);
        // die nächste Aufnahme ohne Mod+E in der Palette gehört wieder dem Fenster
        assert_eq!(st(&mut o, LOADING, true), Route::Window);
    }

    #[test]
    fn no_claim_while_a_window_recording_runs_or_ends() {
        let mut o = VoiceOwner::new();
        st(&mut o, LISTENING, false);
        assert!(!o.claim(false, true), "listening");
        st(&mut o, FINALIZING, true);
        assert!(!o.claim(false, true), "finalizing: a cancel would hit the final of the window");
        assert_eq!(o.final_event(), Route::Window);
        st(&mut o, IDLE, true);
        assert!(o.claim(true, true), "idle again");
    }

    #[test]
    fn closing_before_the_recording_starts_orphans_it() {
        let mut o = VoiceOwner::new();
        assert!(o.claim(true, true));
        o.sent(3);
        assert!(o.cancel(), "cancel must follow the start");
        assert!(!o.cancel(), "only once");
        assert_eq!(st(&mut o, LOADING, false), Route::Discard);
        assert!(o.hides_overlay());
        assert_eq!(o.route(), Route::Discard, "partial and level go nowhere");
        assert_eq!(o.final_event(), Route::Discard, "late final with text is dropped");
        st(&mut o, IDLE, false);
        // danach diktiert das Fenster wieder normal
        assert_eq!(st(&mut o, LOADING, false), Route::Window);
        assert_eq!(o.final_event(), Route::Window);
    }

    #[test]
    fn esc_during_the_recording_drops_the_late_final() {
        let mut o = VoiceOwner::new();
        o.claim(true, true);
        o.sent(1);
        st(&mut o, LISTENING, true);
        assert!(o.cancel());
        assert_eq!(o.route(), Route::Discard);
        assert_eq!(o.final_event(), Route::Discard);
        assert_eq!(st(&mut o, IDLE, true), Route::Window);
        assert!(o.claim(true, true), "Mod+E again in the open palette");
    }

    #[test]
    fn discard_never_outlives_the_next_session() {
        let mut o = VoiceOwner::new();
        o.claim(true, true);
        st(&mut o, LISTENING, true);
        o.cancel();
        // das `final` der abgebrochenen Sitzung kam nie (Fehler im Daemon): die nächste Sitzung des Fensters zählt
        st(&mut o, IDLE, false);
        assert_eq!(st(&mut o, LOADING, false), Route::Window);
        assert_eq!(o.final_event(), Route::Window);
    }

    #[test]
    fn idle_without_final_ends_the_palette_session() {
        let mut o = VoiceOwner::new();
        o.claim(true, true);
        st(&mut o, LISTENING, true);
        assert_eq!(st(&mut o, IDLE, true), Route::Discard, "a late final after idle is dropped");
        assert!(!o.cancel(), "nothing left to cancel");
        assert_eq!(o.final_event(), Route::Discard);
    }

    #[test]
    fn a_failed_start_claims_nothing() {
        let mut o = VoiceOwner::new();
        o.claim(true, true);
        o.sent(5);
        o.failed(4);
        assert!(!o.claim(true, true), "another command failed: still claimed");
        o.failed(5);
        assert!(o.claim(true, true), "own start failed: free again");
        o.sent(6);
        o.failed(6);
        assert_eq!(st(&mut o, LOADING, true), Route::Window, "a session from elsewhere belongs to the window");
    }

    #[test]
    fn reset_on_disconnect() {
        let mut o = VoiceOwner::new();
        o.claim(true, true);
        st(&mut o, LISTENING, true);
        o.reset();
        assert!(!o.hides_overlay());
        assert_eq!(st(&mut o, LOADING, true), Route::Window);
    }
}
