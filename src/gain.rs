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

//! Eingangsverstärkung vor VAD und Whisper (begrenzte Pegelnormalisierung).
//!
//! Warum: Silero-VAD ist pegelabhängig. Ein leises Mikrofon (Brio bei −16 dB, Sprache um −41 dBFS im P90 der
//! 20-ms-Blöcke) liefert bei Schwelle 0,5 nur Bruchstücke der Sprache; das Festschreiben schneidet die Phrase dann
//! mitten im Satz, und der finale Durchlauf sieht nur die Bruchstücke („Test eins, zwei, drei“ → „da“). Whisper
//! selbst normalisiert sein Log-Mel-Spektrum und erkennt dasselbe Audio richtig.
//!
//! Wie: Die Verstärkung kommt aus dem Sprachpegel der laufenden Sitzung und gilt für jeden Durchlauf als ein
//! fester Faktor (kein Regeln innerhalb eines Fensters, also keine Pump-Artefakte):
//! - Pegel je 20-ms-Block (RMS in dBFS) der letzten [`HISTORY_BLOCKS`] Blöcke.
//! - Rauschboden = P10 aller Blöcke; als Sprache zählen Blöcke mindestens [`SNR_DB`] darüber (und über
//!   [`FLOOR_DB`]). Reines Rauschen erreicht das nie und wird deshalb nie auf Sprachpegel hochgezogen.
//! - Sprachpegel = P90 dieser Blöcke; Verstärkung = [`TARGET_DB`] minus Sprachpegel, begrenzt auf
//!   0 … `max_gain_db`. Lautes Audio wird nie abgesenkt (die Testaufnahmen de05/de10/de30 liegen im P90 bei
//!   −18 dBFS und bleiben unverändert). Rauschen steigt mit, der Abstand zur Sprache bleibt gleich.
//! - Der Streamer hält das Rohaudio und wendet den aktuellen Faktor erst beim VAD- bzw. Whisper-Aufruf an. So
//!   sieht schon der erste Durchlauf nach dem Sprachbeginn die Verstärkung, die aus genau dieser Sprache folgt;
//!   die VAD-Überlappung bewertet den Anfang beim nächsten Tick mit dem genaueren Faktor neu.
//!
//! Die Messung gilt je Sitzung (`reset` leert sie): ändert der User zwischen zwei Diktaten die Lautstärke, wirkt
//! das sofort.

use crate::config::MAX_GAIN_DB_LIMIT;
use std::collections::VecDeque;

/// Zielpegel des Sprachanteils (P90 der 20-ms-Block-RMS in dBFS); entspricht normal ausgesteuerter Sprache.
pub const TARGET_DB: f32 = -20.0;
/// Mindestabstand eines Sprachblocks zum Rauschboden
pub const SNR_DB: f32 = 12.0;
/// Darunter ist ein Block nie Sprache (digitale Stille, Rauschen)
pub const FLOOR_DB: f32 = -70.0;
/// So viele Sprachblöcke braucht eine Schätzung (60 ms); vorher bleibt es bei 0 dB
pub const MIN_SPEECH_BLOCKS: usize = 3;
/// 20 ms bei 16 kHz
pub const BLOCK: usize = 320;
/// Gedächtnis: 10 s
pub const HISTORY_BLOCKS: usize = 500;

#[derive(Debug, Clone)]
pub struct InputGain {
    max_gain_db: f32,
    /// Pegel der letzten Blöcke (dBFS)
    blocks: VecDeque<f32>,
    /// angefangener Block
    sum_sq: f64,
    n: usize,
}

/// Schätzung für Log und Messung
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub noise_db: f32,
    /// `None` = noch zu wenig Sprache
    pub speech_db: Option<f32>,
    pub gain_db: f32,
}

impl InputGain {
    /// `max_gain_db` 0 = aus
    pub fn new(max_gain_db: u32) -> Self {
        InputGain { max_gain_db: max_gain_db.min(MAX_GAIN_DB_LIMIT) as f32, blocks: VecDeque::with_capacity(HISTORY_BLOCKS), sum_sq: 0.0, n: 0 }
    }

    pub fn set_max_gain_db(&mut self, v: u32) {
        self.max_gain_db = v.min(MAX_GAIN_DB_LIMIT) as f32;
    }

    pub fn reset(&mut self) {
        self.blocks.clear();
        self.sum_sq = 0.0;
        self.n = 0;
    }

    /// Neue Samples (Rohaudio) messen
    pub fn observe(&mut self, pcm: &[f32]) {
        for &s in pcm {
            self.sum_sq += (s as f64) * (s as f64);
            self.n += 1;
            if self.n == BLOCK {
                let db = (10.0 * (self.sum_sq / BLOCK as f64).max(1e-12).log10()) as f32;
                if self.blocks.len() == HISTORY_BLOCKS {
                    self.blocks.pop_front();
                }
                self.blocks.push_back(db.max(-120.0));
                self.sum_sq = 0.0;
                self.n = 0;
            }
        }
    }

    pub fn estimate(&self) -> Estimate {
        if self.blocks.is_empty() {
            return Estimate { noise_db: -120.0, speech_db: None, gain_db: 0.0 };
        }
        let mut all: Vec<f32> = self.blocks.iter().copied().collect();
        all.sort_by(f32::total_cmp);
        let noise = percentile(&all, 10.0);
        let thr = (noise + SNR_DB).max(FLOOR_DB);
        let speech: Vec<f32> = all.iter().copied().filter(|&d| d > thr).collect();
        if speech.len() < MIN_SPEECH_BLOCKS {
            return Estimate { noise_db: noise, speech_db: None, gain_db: 0.0 };
        }
        let level = percentile(&speech, 90.0);
        let gain_db = (TARGET_DB - level).clamp(0.0, self.max_gain_db);
        Estimate { noise_db: noise, speech_db: Some(level), gain_db }
    }

    /// Linearer Faktor für den nächsten Durchlauf
    pub fn factor(&self) -> f32 {
        db_to_factor(self.estimate().gain_db)
    }
}

pub fn db_to_factor(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Kopie mit Faktor; Spitzen über Vollaussteuerung werden begrenzt (seltene Knackser, Sprache liegt mit dem
/// Zielpegel etwa 15 dB darunter).
pub fn apply(pcm: &[f32], factor: f32) -> Vec<f32> {
    if factor == 1.0 {
        return pcm.to_vec();
    }
    pcm.iter().map(|s| (s * factor).clamp(-1.0, 1.0)).collect()
}

/// Nächster Rang in einer sortierten, nicht leeren Folge
fn percentile(sorted: &[f32], p: f32) -> f32 {
    let i = ((p / 100.0) * (sorted.len() - 1) as f32).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sinus mit gegebenem RMS-Pegel (dBFS)
    fn tone(db: f32, ms: usize) -> Vec<f32> {
        let amp = db_to_factor(db) * std::f32::consts::SQRT_2;
        (0..ms * 16).map(|i| amp * (i as f32 * 0.21).sin()).collect()
    }

    fn noise(db: f32, ms: usize) -> Vec<f32> {
        // deterministisches Rauschen (LCG), Gleichverteilung hat RMS = amp/√3
        let amp = db_to_factor(db) * 3f32.sqrt();
        let mut x: u32 = 12345;
        (0..ms * 16)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn quiet_speech_is_raised_to_target() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-74.0, 500));
        g.observe(&tone(-41.0, 1500));
        let e = g.estimate();
        assert!((e.speech_db.unwrap() + 41.0).abs() < 0.5, "{e:?}");
        assert!((e.gain_db - 21.0).abs() < 0.5, "{e:?}");
        assert!((e.noise_db + 74.0).abs() < 1.0, "{e:?}");
    }

    #[test]
    fn loud_speech_is_never_lowered() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-60.0, 300));
        g.observe(&tone(-12.0, 1000));
        assert_eq!(g.estimate().gain_db, 0.0);
        assert_eq!(g.factor(), 1.0);
    }

    #[test]
    fn gain_is_capped() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-90.0, 300));
        g.observe(&tone(-60.0, 1000));
        assert_eq!(g.estimate().gain_db, 30.0);
        g.set_max_gain_db(10);
        assert_eq!(g.estimate().gain_db, 10.0);
        g.set_max_gain_db(0);
        assert_eq!(g.estimate().gain_db, 0.0);
    }

    #[test]
    fn pure_noise_is_not_amplified() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-74.0, 3000));
        let e = g.estimate();
        assert_eq!(e.speech_db, None);
        assert_eq!(e.gain_db, 0.0);
        // auch nicht leises Rauschen mit gelegentlich lauteren Blöcken (Lüfter)
        let mut g = InputGain::new(30);
        for _ in 0..20 {
            g.observe(&noise(-74.0, 140));
            g.observe(&noise(-68.0, 20));
        }
        assert_eq!(g.estimate().gain_db, 0.0);
    }

    #[test]
    fn needs_some_speech_and_resets() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-74.0, 400));
        g.observe(&tone(-40.0, 40));
        assert_eq!(g.estimate().speech_db, None, "zwei Blöcke reichen nicht");
        g.observe(&tone(-40.0, 40));
        assert!(g.estimate().gain_db > 15.0);
        g.reset();
        assert_eq!(g.estimate().gain_db, 0.0);
    }

    #[test]
    fn history_forgets_old_level() {
        let mut g = InputGain::new(30);
        g.observe(&noise(-80.0, 1000));
        g.observe(&tone(-20.0, 3000));
        assert_eq!(g.estimate().gain_db, 0.0);
        // 10 s leisere Sprache verdrängen den alten Pegel
        for _ in 0..5 {
            g.observe(&noise(-80.0, 400));
            g.observe(&tone(-40.0, 1600));
        }
        assert!((g.estimate().gain_db - 20.0).abs() < 0.5, "{:?}", g.estimate());
    }

    #[test]
    fn apply_scales_and_limits() {
        assert_eq!(apply(&[0.1, -0.2], 1.0), vec![0.1, -0.2]);
        assert_eq!(apply(&[0.1, -0.2, 0.5], 4.0), vec![0.4, -0.8, 1.0]);
        assert!((db_to_factor(20.0) - 10.0).abs() < 1e-4);
    }
}
