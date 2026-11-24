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

//! Streamer mit deterministischem Mock-Transcriber (ohne GPU).
//!
//! Kodierung: ein Wort ist ein 250-ms-Block konstanter Amplitude `id / 1000`, Stille ist 0. Der Mock erkennt
//! vollständige Wörter (≥ 200 ms), ein am Pufferende abgeschnittenes Wort liefert er als Wortanfang. Reine Stille
//! „halluziniert“ er als „Vielen Dank.“, damit das VAD-Gating prüfbar ist.

use dictate::config::Config;
use dictate::engine::{DecodeOpts, Segment, Transcriber};
use dictate::stream::{Partial, Streamer, Tuning};
use std::ops::Range;

const RATE: usize = 16000;
const WORD: usize = 4000;
const GAP: usize = 800;
const THANKS: usize = 50;

fn vocab(id: usize) -> String {
    if id == THANKS {
        return "Vielen Dank.".into();
    }
    const W: [&str; 12] = ["null", "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliett", "kilo"];
    format!("{}{}", W[id % W.len()], id / W.len())
}

#[derive(Default)]
struct Mock {
    partial_calls: usize,
    final_calls: usize,
    vad_calls: usize,
    max_partial_len: usize,
    /// ungerade Zwischen-Durchläufe verfälschen das letzte vollständige Wort
    flaky: bool,
    ctxs: Vec<Option<i32>>,
}

/// Läufe gleicher Amplitude ungleich 0: (id, Bereich)
fn runs(pcm: &[f32]) -> Vec<(usize, Range<usize>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < pcm.len() {
        let v = pcm[i];
        let mut j = i;
        while j < pcm.len() && pcm[j] == v {
            j += 1;
        }
        if v != 0.0 {
            out.push(((v * 1000.0).round() as usize, i..j));
        }
        i = j;
    }
    out
}

impl Transcriber for Mock {
    fn transcribe(&mut self, pcm: &[f32], opts: &DecodeOpts) -> anyhow::Result<Vec<Segment>> {
        assert!(pcm.len() >= RATE, "decode input must be padded to >= 1 s");
        self.ctxs.push(opts.audio_ctx);
        if opts.single_segment {
            self.partial_calls += 1;
            self.max_partial_len = self.max_partial_len.max(pcm.len());
        } else {
            self.final_calls += 1;
        }
        let rs = runs(pcm);
        if rs.is_empty() {
            return Ok(vec![Segment { text: " Vielen Dank.".into(), t0_ms: 0, t1_ms: 0, no_speech_prob: 0.0 }]);
        }
        let mut words: Vec<String> = Vec::new();
        for (id, r) in &rs {
            let w = vocab(*id);
            if r.len() >= WORD * 4 / 5 {
                words.push(w);
            } else if r.end == pcm.len() || pcm[r.end..].iter().all(|&x| x == 0.0) && r.end + GAP > pcm.len() {
                let n = (w.chars().count() * r.len() / WORD).max(1);
                words.push(w.chars().take(n).collect());
            }
        }
        if opts.single_segment && self.flaky && self.partial_calls % 2 == 1 && words.len() >= 2 {
            let k = words.len() - 2;
            words[k].push('x');
        }
        Ok(vec![Segment { text: format!(" {}", words.join(" ")), t0_ms: 0, t1_ms: 0, no_speech_prob: 0.0 }])
    }

    fn speech_ranges(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<Range<usize>>> {
        self.vad_calls += 1;
        let mut out: Vec<Range<usize>> = Vec::new();
        for (_, r) in runs(pcm) {
            match out.last_mut() {
                Some(last) if r.start <= last.end + RATE / 10 => last.end = r.end,
                _ => out.push(r),
            }
        }
        Ok(out)
    }
}

fn speak(ids: &[usize]) -> Vec<f32> {
    let mut v = Vec::new();
    for &id in ids {
        v.extend(std::iter::repeat_n(id as f32 / 1000.0, WORD));
        v.extend(std::iter::repeat_n(0.0, GAP));
    }
    v
}

fn silence(ms: usize) -> Vec<f32> {
    vec![0.0; ms * RATE / 1000]
}

fn words(ids: &[usize]) -> String {
    ids.iter().map(|&i| vocab(i)).collect::<Vec<_>>().join(" ")
}

/// Speist Audio in 20-ms-Blöcken ein und tickt, sobald `due()`; liefert alle Partials
fn feed(s: &mut Streamer, m: &mut Mock, pcm: &[f32]) -> Vec<Partial> {
    let mut out = Vec::new();
    for b in pcm.chunks(320) {
        s.push(b);
        if s.due() {
            if let Some(p) = s.tick(m).unwrap() {
                out.push(p);
            }
        }
    }
    out
}

/// Die Amplitude kodiert das Wort: Eingangsverstärkung und weiche Schnittkanten würden den Code verfälschen.
fn cfg() -> Config {
    Config { max_gain_db: 0, ..Config::default() }
}

fn streamer(c: &Config) -> Streamer {
    Streamer::with_tuning(c, Tuning { edge_fade_ms: 0, ..Tuning::from_config(c) })
}

#[test]
fn silence_gives_no_text_and_no_decoding() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let ps = feed(&mut s, &mut m, &silence(3000));
    assert!(ps.iter().all(|p| p.stable.is_empty() && p.tail.is_empty()), "{ps:?}");
    assert_eq!(s.finish(&mut m).unwrap(), "");
    assert_eq!(m.partial_calls + m.final_calls, 0, "whisper must not run on silence");
    assert!(m.vad_calls > 0);
}

#[test]
fn thanks_hallucination_is_filtered() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[THANKS]);
    pcm.extend(silence(1000));
    let ps = feed(&mut s, &mut m, &pcm);
    assert!(ps.iter().all(|p| !p.stable.contains("Dank") && !p.tail.contains("Dank")), "{ps:?}");
    assert_eq!(s.finish(&mut m).unwrap(), "");
    assert!(m.partial_calls + m.final_calls > 0);
}

#[test]
fn stable_prefix_never_shrinks() {
    let mut s = streamer(&cfg());
    let mut m = Mock { flaky: true, ..Default::default() };
    let ids: Vec<usize> = (1..=11).collect();
    let mut pcm = speak(&ids[..6]);
    pcm.extend(silence(900));
    pcm.extend(speak(&ids[6..]));
    let ps = feed(&mut s, &mut m, &pcm);
    assert!(ps.len() > 5);
    let mut prev = String::new();
    for p in &ps {
        assert!(p.stable.starts_with(&prev), "stable shrank: {prev:?} -> {:?}", p.stable);
        assert!(!p.stable.contains('x') || p.stable.contains("xtrot"), "flaky word confirmed: {:?}", p.stable);
        prev = p.stable.clone();
    }
    assert_eq!(s.finish(&mut m).unwrap(), words(&ids));
}

#[test]
fn commits_at_pauses() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2, 3, 4]);
    pcm.extend(silence(1000));
    let ps = feed(&mut s, &mut m, &pcm);
    assert_eq!(s.stats().commits, 1, "pause must commit the phrase");
    let last = ps.last().unwrap();
    assert_eq!(last.stable, words(&[1, 2, 3, 4]));
    assert_eq!(last.tail, "");
    let ps2 = feed(&mut s, &mut m, &speak(&[5, 6, 7]));
    assert!(ps2.iter().all(|p| p.stable.starts_with(&words(&[1, 2, 3, 4]))));
    assert_eq!(s.finish(&mut m).unwrap(), words(&[1, 2, 3, 4, 5, 6, 7]));
    assert_eq!(s.stats().commits, 2);
}

#[test]
fn speculative_final_makes_finish_cheap() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2, 3]);
    pcm.extend(silence(620));
    feed(&mut s, &mut m, &pcm);
    assert_eq!(s.stats().speculative_passes, 1);
    let calls = m.partial_calls + m.final_calls;
    assert_eq!(s.finish(&mut m).unwrap(), words(&[1, 2, 3]));
    assert_eq!(m.partial_calls + m.final_calls, calls, "finish must reuse the speculative pass");
    assert_eq!(s.stats().speculative_hits, 1);
}

#[test]
fn speculative_result_discarded_when_speech_resumes() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2]);
    pcm.extend(silence(620));
    pcm.extend(speak(&[3, 4]));
    feed(&mut s, &mut m, &pcm);
    assert_eq!(s.stats().speculative_passes, 1);
    assert_eq!(s.finish(&mut m).unwrap(), words(&[1, 2, 3, 4]));
    assert_eq!(s.stats().speculative_hits, 0);
}

#[test]
fn finish_returns_whole_session_text() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2]);
    pcm.extend(silence(800));
    pcm.extend(speak(&[3]));
    pcm.extend(silence(800));
    pcm.extend(speak(&[4, 5]));
    feed(&mut s, &mut m, &pcm);
    assert_eq!(s.finish(&mut m).unwrap(), words(&[1, 2, 3, 4, 5]));
    // danach ist die Sitzung leer
    assert_eq!(s.partial(), Partial::default());
    assert_eq!(s.finish(&mut m).unwrap(), "");
}

#[test]
fn reset_clears_everything() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2, 3]);
    pcm.extend(silence(1000));
    pcm.extend(speak(&[4, 5]));
    feed(&mut s, &mut m, &pcm);
    assert!(!s.partial().stable.is_empty());
    s.reset();
    assert_eq!(s.partial(), Partial::default());
    assert!(!s.due());
    assert_eq!(s.stats().commits, 0);
    let calls = m.partial_calls + m.final_calls;
    assert_eq!(s.finish(&mut m).unwrap(), "");
    assert_eq!(m.partial_calls + m.final_calls, calls);
}

#[test]
fn long_phrase_is_cut_at_pauses_without_losing_words() {
    let c = Config { max_phrase_ms: 3000, ..cfg() };
    // Pausen von 200 ms reichen nicht zum Festschreiben, nur die Überlänge schneidet
    let tun = Tuning { speculative_ms: None, soft_phrase_ms: 60_000, edge_fade_ms: 0, ..Tuning::from_config(&c) };
    let mut s = Streamer::with_tuning(&c, tun);
    let mut m = Mock::default();
    let ids: Vec<usize> = (1..=20).collect();
    let mut pcm = Vec::new();
    for pair in ids.chunks(2) {
        pcm.extend(speak(pair));
        pcm.extend(silence(200));
    }
    feed(&mut s, &mut m, &pcm);
    assert!(s.stats().commits >= 2, "commits: {}", s.stats().commits);
    assert_eq!(s.finish(&mut m).unwrap(), words(&ids));
}

#[test]
fn soft_commit_on_short_pause_in_long_phrase() {
    let c = cfg();
    let tun = Tuning { speculative_ms: None, soft_phrase_ms: 3000, soft_silence_ms: 300, edge_fade_ms: 0, ..Tuning::from_config(&c) };
    let mut s = Streamer::with_tuning(&c, tun);
    let mut m = Mock::default();
    let mut pcm = speak(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    pcm.extend(silence(400));
    pcm.extend(speak(&[13]));
    feed(&mut s, &mut m, &pcm);
    assert_eq!(s.stats().commits, 1);
    assert_eq!(s.finish(&mut m).unwrap(), words(&(1..=13).collect::<Vec<_>>()));
}

#[test]
fn leading_silence_is_trimmed_and_ctx_is_small() {
    let mut s = streamer(&cfg());
    let mut m = Mock::default();
    let mut pcm = silence(5000);
    pcm.extend(speak(&[1, 2, 3]));
    feed(&mut s, &mut m, &pcm);
    assert!(m.max_partial_len < 2 * RATE, "partial input {} samples", m.max_partial_len);
    assert!(m.ctxs.iter().all(|c| c.is_some_and(|c| c < 1500)), "{:?}", m.ctxs);
    assert_eq!(s.finish(&mut m).unwrap(), words(&[1, 2, 3]));
    // finaler Durchlauf mit vollem Kontext
    assert_eq!(*m.ctxs.last().unwrap(), None);
}

/// Transcriber, der immer scheitert (Festschreiben schlägt fehl).
struct Failing;

impl Transcriber for Failing {
    fn transcribe(&mut self, _pcm: &[f32], _opts: &DecodeOpts) -> anyhow::Result<Vec<Segment>> {
        anyhow::bail!("mock failure")
    }
    fn speech_ranges(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<Range<usize>>> {
        Ok(std::iter::once(0..pcm.len()).collect())
    }
}

#[test]
fn phrase_overrun_is_reported_when_commits_keep_failing() {
    let cfg = Config { max_phrase_ms: 2000, ..cfg() };
    let mut s = streamer(&cfg);
    let block = vec![0.1f32; RATE / 5];
    let mut t = Failing;
    let mut ticks_failed = 0;
    for i in 0..40 {
        s.push(&block);
        if s.due() && s.tick(&mut t).is_err() {
            ticks_failed += 1;
        }
        // 2 s Grenze, ab dem Doppelten (4 s = Block 20) Überlauf
        assert_eq!(s.overrun(), i >= 20, "block {i}");
    }
    assert!(ticks_failed > 0);
}
