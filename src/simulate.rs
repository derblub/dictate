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

//! `transcribe --stream`: Datei in 20-ms-Blöcken durch den Streamer speisen, Events wie der Daemon ausgeben.
//!
//! Virtuelle Uhr: Block i „kommt“ zum Zeitpunkt (i+1)·20 ms an. Ein `tick` verbraucht seine echte Rechenzeit auf
//! der virtuellen Uhr; was in dieser Zeit an Audio ankommt, wird danach auf einmal nachgeschoben – genau wie im
//! Daemon, wo der Engine-Thread während eines Durchlaufs nichts annimmt. Mit `--realtime` läuft die Wanduhr.
//!
//! Latenzmaße (stderr):
//! - `shown`: für jeden 20-ms-Block mit Sprache (VAD über die ganze Datei, vorab und ungemessen) die Zeit von
//!   seiner Ankunft bis zum Ende des ersten Durchlaufs, der ihn gesehen hat (= Anzeige als stable oder tail).
//! - `confirm`: für das k-te bestätigte Wort die Zeit vom ersten Anzeigen desselben Worts (normalisiert) an
//!   Position k in stable+tail bis zur Bestätigung. Positionen können sich beim Festschreiben verschieben,
//!   deshalb eine Näherung.
//! - `finalize`: von Dateiende („stop“) bis zum Endtext, inklusive eines dann noch laufenden Durchlaufs.

use crate::config::Config;
use crate::engine::{DecodeOpts, Engine, Transcriber};
use crate::filter::norm_word;
use crate::stream::{Partial, Stats, Streamer, Tuning, RATE};
use crate::wav;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const BLOCK: usize = RATE / 50; // 20 ms
const BLOCK_MS: f64 = 20.0;

struct Clock {
    realtime: bool,
    start: Instant,
    virt: f64,
}

impl Clock {
    fn new(realtime: bool) -> Self {
        Clock { realtime, start: Instant::now(), virt: 0.0 }
    }
    fn now(&self) -> f64 {
        if self.realtime {
            self.start.elapsed().as_secs_f64() * 1000.0
        } else {
            self.virt
        }
    }
    /// Rechenzeit verbuchen (bei Echtzeit ist sie schon vergangen)
    fn spend(&mut self, ms: f64) {
        if !self.realtime {
            self.virt += ms;
        }
    }
    fn wait_until(&mut self, t: f64) {
        if self.realtime {
            let now = self.now();
            if t > now {
                std::thread::sleep(Duration::from_secs_f64((t - now) / 1000.0));
            }
        } else {
            self.virt = self.virt.max(t);
        }
    }
}

/// Ergebnis einer Simulation
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub text: String,
    pub audio_ms: f64,
    pub finalize_ms: f64,
    /// Latenz Ankunft → erste Anzeige je Sprachblock (ms)
    pub shown_ms: Vec<f64>,
    /// Anzeige → Bestätigung je Wort (ms)
    pub confirm_ms: Vec<f64>,
    /// Dauer jedes Ticks (ms)
    pub tick_ms: Vec<f64>,
    /// Wie oft ein bereits angezeigter stabiler Text beim Festschreiben korrigiert wurde
    pub stable_revisions: u64,
    pub partials: u64,
    pub stats: Stats,
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Treibt `streamer` mit `pcm` wie der Daemon und ruft `emit` für jedes Event (Protokollformat).
pub fn simulate(t: &mut dyn Transcriber, streamer: &mut Streamer, pcm: &[f32], realtime: bool, emit: &mut dyn FnMut(&Value)) -> Result<Report> {
    if pcm.is_empty() {
        bail!("no audio");
    }
    // Referenz-Sprachbereiche für die Latenzmessung, außerhalb der Uhr
    let speech = t.speech_ranges(pcm)?;
    streamer.reset();
    let n_blocks = pcm.len().div_ceil(BLOCK);
    let arrival = |i: usize| (i + 1) as f64 * BLOCK_MS;
    let mut clock = Clock::new(realtime);
    let mut rep = Report { audio_ms: pcm.len() as f64 * 1000.0 / RATE as f64, ..Default::default() };
    // (Zeit, abgedeckte Samples) nach jedem Tick
    let mut coverage: Vec<(f64, u64)> = Vec::new();
    // je Position: (Zeitpunkt, Wort) beim ersten Anzeigen dieses Worts an dieser Position
    let mut shown_at: Vec<Vec<(f64, String)>> = Vec::new();
    // je Position: (Zeitpunkt, Wort) beim Bestätigen
    let mut stable_at: Vec<(f64, String)> = Vec::new();
    let mut last = Partial::default();

    emit(&json!({"event":"state","state":"listening"}));
    let mut next = 0;
    loop {
        let now = clock.now();
        while next < n_blocks && arrival(next) <= now {
            let a = next * BLOCK;
            streamer.push(&pcm[a..(a + BLOCK).min(pcm.len())]);
            next += 1;
        }
        if next == n_blocks {
            break;
        }
        if streamer.due() {
            let t0 = Instant::now();
            let r = streamer.tick(t)?;
            let d = t0.elapsed().as_secs_f64() * 1000.0;
            clock.spend(d);
            rep.tick_ms.push(d);
            let now = clock.now();
            coverage.push((now, streamer.stats().covered));
            if let Some(p) = r {
                rep.partials += 1;
                // korrigiert ein Festschreiben schon angezeigten stabilen Text?
                if !p.stable.starts_with(&last.stable) {
                    rep.stable_revisions += 1;
                }
                let all: Vec<String> = p.stable.split_whitespace().chain(p.tail.split_whitespace()).map(norm_word).collect();
                for (k, w) in all.iter().enumerate() {
                    if shown_at.len() <= k {
                        shown_at.push(Vec::new());
                    }
                    if !shown_at[k].iter().any(|(_, x)| x == w) {
                        shown_at[k].push((now, w.clone()));
                    }
                }
                for (k, w) in all.iter().take(word_count(&p.stable)).enumerate() {
                    if stable_at.len() == k {
                        stable_at.push((now, w.clone()));
                    }
                }
                emit(&json!({"event":"partial","stable":p.stable,"tail":p.tail}));
                last = p;
            }
        } else {
            clock.wait_until(arrival(next));
        }
    }
    // Dateiende = stop
    let t_stop = arrival(n_blocks - 1);
    emit(&json!({"event":"state","state":"finalizing"}));
    let t0 = Instant::now();
    let text = streamer.finish(t)?;
    clock.spend(t0.elapsed().as_secs_f64() * 1000.0);
    rep.finalize_ms = clock.now() - t_stop.min(clock.now());
    rep.stats = streamer.stats().clone();
    emit(&json!({
        "event":"final","text":text,"cancelled":false,
        "audio_ms":rep.audio_ms.round() as u64,"finalize_ms":rep.finalize_ms.round() as u64
    }));
    emit(&json!({"event":"state","state":"idle"}));

    for i in 0..n_blocks {
        let (a, b) = (i * BLOCK, ((i + 1) * BLOCK).min(pcm.len()));
        let mid = (a + b) / 2;
        if !speech.iter().any(|r| r.contains(&mid)) {
            continue;
        }
        if let Some(&(tc, _)) = coverage.iter().find(|(_, c)| *c >= b as u64) {
            rep.shown_ms.push(tc - arrival(i));
        }
    }
    for (k, (ts, w)) in stable_at.iter().enumerate() {
        if let Some((sh, _)) = shown_at.get(k).and_then(|v| v.iter().find(|(_, x)| x == w)) {
            rep.confirm_ms.push(ts - sh);
        }
    }
    rep.text = text;
    Ok(rep)
}

/// Perzentil (nächster Rang) einer unsortierten Liste
pub fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let idx = ((p / 100.0) * s.len() as f64).ceil() as usize;
    s[idx.clamp(1, s.len()) - 1]
}

/// Stellschrauben aus `DICTATE_TUNE` („partial_ctx_min=256,final_beam=2,speculative_ms=off“), nur für Messungen.
/// Schlüssel wie die Felder von [`Tuning`]; `off` setzt optionale Werte auf `None`.
pub fn apply_tune_env(tun: &mut Tuning, spec: &str) -> Result<()> {
    for kv in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').with_context(|| format!("DICTATE_TUNE: expected key=value, got {kv:?}"))?;
        let off = v == "off" || v == "none";
        let num = || v.parse::<u64>().with_context(|| format!("DICTATE_TUNE: bad number in {kv:?}"));
        match k {
            "partial_beam" => tun.partial_beam = num()? as usize,
            "partial_ctx_min" => tun.partial_ctx_min = num()? as i32,
            "partial_ctx_margin" => tun.partial_ctx_margin = num()? as i32,
            "final_ctx_margin" => tun.final_ctx_margin = if off { None } else { Some(num()? as i32) },
            "final_beam" => tun.final_beam = num()? as usize,
            "vad_overlap_ms" => tun.vad_overlap_ms = num()?,
            "final_pad_ms" => tun.final_pad_ms = num()?,
            "edge_fade_ms" => tun.edge_fade_ms = num()?,
            "span_join_ms" => tun.span_join_ms = num()?,
            "speculative_ms" => tun.speculative_ms = if off { None } else { Some(num()?) },
            "soft_phrase_ms" => tun.soft_phrase_ms = num()?,
            "soft_silence_ms" => tun.soft_silence_ms = num()?,
            "prompt_chars" => tun.prompt_chars = num()? as usize,
            _ => bail!("DICTATE_TUNE: unknown key {k:?}"),
        }
    }
    Ok(())
}

pub fn run(cfg: &Config, file: &Path, realtime: bool) -> Result<()> {
    let mut pcm = wav::read_16k_mono(file)?;
    // Messhilfe: Stille anhängen, als ließe der Nutzer die Taste erst so spät nach dem letzten Wort los
    if let Some(ms) = std::env::var("DICTATE_SIM_TAIL_MS").ok().and_then(|v| v.parse::<usize>().ok()) {
        pcm.extend(std::iter::repeat_n(0.0, ms * RATE / 1000));
    }
    let mut tun = Tuning::from_config(cfg);
    if let Ok(s) = std::env::var("DICTATE_TUNE") {
        apply_tune_env(&mut tun, &s)?;
    }
    let mut engine = Engine::load(cfg)?;
    // Aufwärmen: der erste CUDA-Durchlauf ist deutlich langsamer (der Daemon sollte das nach dem Laden auch tun)
    let warm = vec![0.0f32; RATE];
    engine.transcribe(&warm, &DecodeOpts { audio_ctx: None, beam_size: 1, prompt: "", single_segment: true })?;
    let _ = engine.speech_ranges(&warm)?;

    let mut streamer = Streamer::with_tuning(cfg, tun);
    let stdout = std::io::stdout();
    let mut emit = |v: &Value| {
        let mut out = stdout.lock();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    let rep = simulate(&mut engine, &mut streamer, &pcm, realtime, &mut emit)?;
    print_report(&rep, streamer.tuning());
    Ok(())
}

fn print_report(rep: &Report, tun: &Tuning) {
    let s = &rep.stats;
    let p = |v: &[f64], q| percentile(v, q);
    eprintln!("audio {:.1} s, {} ticks, {} partial events", rep.audio_ms / 1000.0, s.ticks, rep.partials);
    eprintln!(
        "shown latency   p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms ({} speech blocks)",
        p(&rep.shown_ms, 50.0),
        p(&rep.shown_ms, 95.0),
        p(&rep.shown_ms, 100.0),
        rep.shown_ms.len()
    );
    eprintln!("confirm delay   p50 {:.0} ms, p95 {:.0} ms ({} words)", p(&rep.confirm_ms, 50.0), p(&rep.confirm_ms, 95.0), rep.confirm_ms.len());
    eprintln!("tick duration   p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms", p(&rep.tick_ms, 50.0), p(&rep.tick_ms, 95.0), p(&rep.tick_ms, 100.0));
    let avg = |tot: f64, n: u64| if n == 0 { 0.0 } else { tot / n as f64 };
    eprintln!(
        "passes: partial {} (avg {:.0} ms), final {} (avg {:.0} ms), speculative {} ({} used), commits {}, VAD {} calls {:.0} ms",
        s.partial_passes,
        avg(s.partial_ms, s.partial_passes),
        s.final_passes,
        avg(s.final_ms, s.final_passes),
        s.speculative_passes,
        s.speculative_hits,
        s.commits,
        s.vad_calls,
        s.vad_ms
    );
    eprintln!("finalize {:.0} ms after end of audio, stable revisions {}", rep.finalize_ms, rep.stable_revisions);
    eprintln!("text: {}", rep.text);
    let summary = json!({
        "audio_ms": rep.audio_ms, "finalize_ms": rep.finalize_ms,
        "shown_p50": p(&rep.shown_ms, 50.0), "shown_p95": p(&rep.shown_ms, 95.0), "shown_max": p(&rep.shown_ms, 100.0),
        "confirm_p50": p(&rep.confirm_ms, 50.0), "confirm_p95": p(&rep.confirm_ms, 95.0),
        "tick_p95": p(&rep.tick_ms, 95.0), "tick_max": p(&rep.tick_ms, 100.0),
        "partial_passes": s.partial_passes, "partial_avg_ms": avg(s.partial_ms, s.partial_passes),
        "final_passes": s.final_passes, "final_avg_ms": avg(s.final_ms, s.final_passes),
        "speculative": s.speculative_passes, "speculative_hits": s.speculative_hits, "commits": s.commits,
        "vad_ms": s.vad_ms, "stable_revisions": rep.stable_revisions,
        "tuning": format!("{tun:?}"), "text": rep.text,
    });
    eprintln!("summary {summary}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let v = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(percentile(&v, 50.0), 3.0);
        assert_eq!(percentile(&v, 95.0), 5.0);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert!(percentile(&[], 50.0).is_nan());
    }

    #[test]
    fn tune_env_parses() {
        let mut t = Tuning::from_config(&Config::default());
        apply_tune_env(&mut t, "final_beam=2, speculative_ms=off,final_ctx_margin=256").unwrap();
        assert_eq!(t.final_beam, 2);
        assert_eq!(t.speculative_ms, None);
        assert_eq!(t.final_ctx_margin, Some(256));
        assert!(apply_tune_env(&mut t, "nope=1").is_err());
    }
}
