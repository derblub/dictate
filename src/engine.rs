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

//! Whisper-Engine (whisper.cpp über whisper-rs, CUDA) mit Silero-VAD.
//!
//! `Engine` ist `Send` (alle Teile sind es), aber nicht `Sync`: Aufrufe brauchen `&mut self`, es läuft
//! immer höchstens ein Durchlauf gleichzeitig. Zustand und VAD-Kontext werden einmal beim Laden
//! angelegt und wiederverwendet. Drop gibt Modell, Zustand und VRAM frei.

use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use std::ops::Range;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub no_speech_prob: f32,
}

#[derive(Debug, Clone)]
pub struct DecodeOpts<'a> {
    /// Encoder-Kontext in Frames (1500 = volle 30 s); kleiner = schneller, nur für Zwischenstände.
    pub audio_ctx: Option<i32>,
    /// 1 = Greedy, >1 = Beam Search
    pub beam_size: usize,
    pub prompt: &'a str,
    /// Ein Segment ohne Zeitstempel. Gilt zugleich als „Zwischenstand“: kein Temperatur-Fallback (jeder
    /// Fallback ist ein weiterer kompletter Decoder-Lauf, im Test bis 2,7 s statt 150 ms) und eine Obergrenze für
    /// die Tokenzahl nach Audiolänge, damit Wiederholungsschleifen nicht den Takt sprengen.
    pub single_segment: bool,
}

pub trait Transcriber {
    fn transcribe(&mut self, pcm: &[f32], opts: &DecodeOpts) -> Result<Vec<Segment>>;
    /// Sprachabschnitte als Sample-Bereiche (16 kHz) laut Silero-VAD
    fn speech_ranges(&mut self, pcm: &[f32]) -> Result<Vec<Range<usize>>>;
}

pub struct Engine {
    // Reihenfolge = Drop-Reihenfolge: erst Zustand und VAD, dann der Kontext
    state: WhisperState,
    vad: WhisperVadContext,
    _ctx: WhisperContext,
    language: String,
    /// Sprache dieser Sitzung (Profil), sonst `language`
    session_language: Option<String>,
    threads: i32,
}

impl Engine {
    pub fn load(cfg: &Config) -> Result<Engine> {
        // whisper.cpp/ggml-Logs über tracing (target whisper_rs::*), nicht auf stderr
        whisper_rs::install_logging_hooks();
        let model = cfg.model_path();
        let vad_model = cfg.vad_model_path();
        if !model.is_file() {
            bail!("model not found: {}", model.display());
        }
        if !vad_model.is_file() {
            bail!("VAD model not found: {}", vad_model.display());
        }
        let ctx =
            WhisperContext::new_with_params(&model, WhisperContextParameters::default()).map_err(|e| anyhow!("loading model {}: {e}", model.display()))?;
        let state = ctx.create_state().map_err(|e| anyhow!("creating whisper state: {e}"))?;
        let mut vp = WhisperVadContextParams::new();
        vp.set_n_threads(1);
        // CPU, 1 Thread: Silero ist sequentiell, mehr Threads kosten nur (4 Threads: 156 ms, 1 Thread: 48 ms
        // für 10 s). Auf der GPU stürzt der VAD-Kontext ab (SIGABRT, 22.10.2026).
        vp.set_use_gpu(false);
        let vad = WhisperVadContext::new(vad_model.to_str().context("VAD model path is not UTF-8")?, vp)
            .map_err(|e| anyhow!("loading VAD model {}: {e}", vad_model.display()))?;
        Ok(Engine { state, vad, _ctx: ctx, language: cfg.language.clone(), session_language: None, threads: cfg.threads.max(1) as i32 })
    }
}

impl Engine {
    /// Sprache für die folgenden Durchläufe (`None` = aus der Konfiguration). whisper.cpp nimmt die Sprache je
    /// `full()`-Aufruf, ein Wechsel braucht kein neues Laden.
    pub fn set_session_language(&mut self, lang: Option<&str>) {
        self.session_language = lang.map(str::to_string);
    }
}

impl Transcriber for Engine {
    fn transcribe(&mut self, pcm: &[f32], opts: &DecodeOpts) -> Result<Vec<Segment>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        let strategy = if opts.beam_size > 1 {
            SamplingStrategy::BeamSearch { beam_size: opts.beam_size as i32, patience: -1.0 }
        } else {
            SamplingStrategy::Greedy { best_of: 1 }
        };
        let mut p = FullParams::new(strategy);
        p.set_n_threads(self.threads);
        p.set_language(Some(self.session_language.as_deref().unwrap_or(&self.language)));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_single_segment(opts.single_segment);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        if let Some(c) = opts.audio_ctx {
            p.set_audio_ctx(c);
        }
        if !opts.prompt.is_empty() {
            p.set_initial_prompt(opts.prompt);
        }
        if opts.single_segment {
            p.set_temperature_inc(0.0);
            // ~3 Wörter/s, deutsche Komposita brauchen bis zu 4 Tokens pro Wort
            p.set_max_tokens((pcm.len() / 16000 * 12 + 24) as i32);
        }
        self.state.full(p, pcm).map_err(|e| anyhow!("whisper_full failed: {e}"))?;
        let mut out = Vec::new();
        for seg in self.state.as_iter() {
            let text = seg.to_str_lossy().map_err(|e| anyhow!("segment text: {e}"))?.into_owned();
            out.push(Segment {
                text,
                // whisper liefert Zentisekunden
                t0_ms: seg.start_timestamp() * 10,
                t1_ms: seg.end_timestamp() * 10,
                no_speech_prob: seg.no_speech_probability(),
            });
        }
        Ok(out)
    }

    fn speech_ranges(&mut self, pcm: &[f32]) -> Result<Vec<Range<usize>>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        let mut params = WhisperVadParams::new();
        // Schwelle und Pausen wie im Vortest (Silero-Defaults, kurze Pausen zusammenhalten)
        params.set_threshold(0.5);
        params.set_min_speech_duration(250);
        params.set_min_silence_duration(100);
        params.set_speech_pad(30);
        let segs = self.vad.segments_from_samples(params, pcm).map_err(|e| anyhow!("VAD failed: {e}"))?;
        let mut out = Vec::new();
        for s in segs {
            // Zentisekunden -> Samples bei 16 kHz
            let a = ((s.start * 160.0) as usize).min(pcm.len());
            let b = ((s.end * 160.0) as usize).min(pcm.len());
            if b > a {
                out.push(a..b);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<super::Engine>();
    }
}
