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

//! Pixelwelle des Overlays ohne GTK und ohne `protocol`: Maße, Säulen, Töne und ihre Deckkraft. Einzige Quelle für
//! `dictate-ui` (gezeichnet in `src/bin/dictate-ui/overlay.rs`, über [`super::view`]) und die Vorschau des Overlays im
//! Einstellungsfenster (`src/gui/pages/overlay.rs`, per `#[path]` eingebunden; PLAN-wayland.md 6h, Lesart L45, Regel 2).
//! Übertragen aus dem früheren Awesome-Modul (`init.lua`: `make_wave`); Maße wie dort.

/// Maße in logischen Pixeln (wie `cfg` in `init.lua`)
pub mod size {
    pub const CELL: f64 = 6.0;
    pub const PITCH: f64 = 8.0;
    pub const PAD_X: i32 = 24;
    pub const PAD_Y: i32 = 16;
    pub const GAP: i32 = 16;
    pub const LINE_PX: i32 = 24;
    pub const MAX_LINES: i32 = 3;
    /// Anteil der Bildschirmbreite, den das Overlay höchstens einnimmt
    pub const MAX_WIDTH_FRAC: f64 = 0.9;
    /// Eckenradius einer Zelle
    pub const CELL_RADIUS: f64 = 1.0;
}

/// Deckkraft von `on_surface` für eine leere Zelle ([`Tone::Off`])
pub const OFF_ALPHA: f64 = 0.12;
/// Deckkraft von `on_surface` für den gestrichelten Slot ([`Tone::Dash`])
pub const DASH_ALPHA: f64 = 0.40;

/// Gewichte der sieben Säulen, Schwellen für drei bzw. fünf Zellen, Ruhestufe je Abstand zur Mitte
const WEIGHTS: [f64; 7] = [0.55, 0.85, 0.6, 1.0, 0.7, 0.9, 0.5];
pub const COLS: usize = WEIGHTS.len();
const T3: f64 = 0.18;
const T5: f64 = 0.45;
const REST_STAGE: [u8; 2] = [1, 2];

/// Breite und Höhe der Welle (sieben Säulen, fünf Zeilen)
pub fn wave_size() -> (f64, f64) {
    (COLS as f64 * size::PITCH - (size::PITCH - size::CELL), 5.0 * size::PITCH - (size::PITCH - size::CELL))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveMode {
    /// Pegel (listening)
    Listen,
    /// Warten (loading, finalizing, Hinweise): leere Zellen, letzte im Akzent
    Wait,
    /// Fehler: leere Zellen, letzte als gestrichelter Slot
    Error,
}

/// Ton einer Zelle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Mix,
    /// Ruheton (`on_surface`)
    Rest,
    Off,
    /// gestrichelter Rahmen statt Fläche
    Dash,
}

/// Pixelwelle: gezeichnet wird nur bei Pegel-Events und Zustandswechseln (kein Animations-Timer)
#[derive(Debug, Clone)]
pub struct Wave {
    pub mode: WaveMode,
    n: u64,
    sm: f64,
    /// Höhe je Säule (1, 3 oder 5 Zellen)
    h: [u8; COLS],
    /// Stufe je Säule und Abstand zur Mitte: 0 Akzent, 1 Mischung, 2 Ruheton
    stage: [[u8; 2]; COLS],
}

impl Default for Wave {
    fn default() -> Self {
        Wave { mode: WaveMode::Listen, n: 0, sm: 0.0, h: [1; COLS], stage: [[0; 2]; COLS] }
    }
}

/// Pegel in dBFS auf 0..1 (−60 dB = 0, −10 dB = 1), wie `handle_line` in `init.lua`
pub fn level_from_db(db: f32) -> f64 {
    ((f64::from(db) + 60.0) / 50.0).clamp(0.0, 1.0)
}

impl Wave {
    /// Neuer Pegel 0..1: bestehende Zellen gehen eine Stufe Richtung Ruheton, neue erscheinen im Akzent
    pub fn set_level(&mut self, l: f64) {
        self.mode = WaveMode::Listen;
        self.n += 1;
        self.sm = self.sm * 0.4 + l * 0.6;
        for (i, w) in WEIGHTS.iter().enumerate() {
            // deterministische Variation je Event (Lua zählt die Säulen ab 1)
            let wob = 0.88 + 0.12 * (self.n as f64 * 1.7 + (i + 1) as f64 * 2.3).sin();
            let v = w * self.sm * wob;
            let nh = if v < T3 {
                1
            } else if v < T5 {
                3
            } else {
                5
            };
            let oh = self.h[i];
            for d in 1..=2u8 {
                if nh > 2 * d {
                    let s = &mut self.stage[i][usize::from(d - 1)];
                    *s = if oh > 2 * d { (*s + 1).min(REST_STAGE[usize::from(d - 1)]) } else { 0 };
                }
            }
            self.h[i] = nh;
        }
    }

    pub fn set_mode(&mut self, mode: WaveMode) {
        if self.mode == mode {
            return;
        }
        if mode == WaveMode::Listen {
            *self = Wave::default();
        }
        self.mode = mode;
    }

    /// Zu zeichnende Zellen: (Säule 0..7, Zeile −2..2 mit 0 = Mitte, Ton)
    pub fn cells(&self) -> Vec<(usize, i32, Tone)> {
        let mut out = vec![];
        match self.mode {
            WaveMode::Wait | WaveMode::Error => {
                for i in 0..COLS {
                    let last = i == COLS - 1;
                    let tone = match (self.mode, last) {
                        (WaveMode::Error, true) => Tone::Dash,
                        (WaveMode::Wait, true) => Tone::Accent,
                        _ => Tone::Off,
                    };
                    out.push((i, 0, tone));
                }
            }
            WaveMode::Listen => {
                let tones = [Tone::Accent, Tone::Mix, Tone::Rest];
                for i in 0..COLS {
                    out.push((i, 0, Tone::Accent));
                    for d in 1..=2u8 {
                        if self.h[i] > 2 * d {
                            let tone = tones[usize::from(self.stage[i][usize::from(d - 1)])];
                            out.push((i, -i32::from(d), tone));
                            out.push((i, i32::from(d), tone));
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_levels_and_modes() {
        let mut w = Wave::default();
        assert_eq!(w.cells().len(), COLS, "rest: one accent cell per column");
        w.set_level(1.0);
        w.set_level(1.0);
        let cells = w.cells();
        assert!(cells.len() > COLS, "loud: taller columns");
        assert!(cells.iter().all(|c| c.1.abs() <= 2));
        // nach mehreren lauten Pegeln erreichen die äußeren Zellen den Ruheton
        for _ in 0..5 {
            w.set_level(1.0);
        }
        assert!(w.cells().iter().any(|c| c.2 == Tone::Rest));
        w.set_mode(WaveMode::Error);
        let cells = w.cells();
        assert_eq!(cells.len(), COLS);
        assert_eq!(cells[COLS - 1].2, Tone::Dash);
        w.set_mode(WaveMode::Wait);
        assert_eq!(w.cells()[COLS - 1].2, Tone::Accent);
        // zurück zum Pegel: Welle beginnt in Ruhe
        w.set_mode(WaveMode::Listen);
        assert_eq!(w.cells().len(), COLS);
        assert_eq!(level_from_db(-90.0), 0.0);
        assert_eq!(level_from_db(-10.0), 1.0);
        assert!((level_from_db(-35.0) - 0.5).abs() < 1e-9);
        assert_eq!(wave_size(), (54.0, 38.0));
    }
}
