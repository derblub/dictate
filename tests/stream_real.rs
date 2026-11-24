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

//! Streaming mit echtem Modell (GPU, Modelle unter ~/.local/share/dictate/models):
//! `cargo test --release --test stream_real -- --ignored --nocapture --test-threads=1`

mod test_root;

use dictate::config::Config;
use dictate::engine::{DecodeOpts, Engine, Transcriber};
use dictate::simulate::{percentile, simulate};
use dictate::stream::Streamer;
use std::time::Instant;

fn audio(name: &str) -> Vec<f32> {
    let path = test_root::audio(&format!("{name}.wav"));
    dictate::wav::read_16k_mono(&path).unwrap_or_else(|e| panic!("{}: {e} ({})", path.display(), test_root::AUDIO_HINT))
}

fn engine() -> Engine {
    let mut e = Engine::load(&Config::default()).unwrap();
    let warm = vec![0.0f32; 16000];
    e.transcribe(&warm, &DecodeOpts { audio_ctx: None, beam_size: 1, prompt: "", single_segment: true }).unwrap();
    e
}

fn text(segs: &[dictate::engine::Segment]) -> String {
    segs.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ")
}

/// Zeiten und Texte einzelner Zwischen-Durchläufe je Länge, Vorlauf und `audio_ctx` (Messhilfe, prüft nichts).
/// `BENCH_CTX` = Liste von Rändern in Frames (über der Länge), `BENCH_PAD` = Vorlauf-Stille in ms.
#[test]
#[ignore]
fn bench_partial_ctx() {
    let mut e = engine();
    let pcm = audio("de10");
    let list = |k: &str, d: &str| -> Vec<i32> { std::env::var(k).unwrap_or_else(|_| d.into()).split(',').map(|s| s.parse().unwrap()).collect() };
    let margins = list("BENCH_CTX", "32,64,128,256,9999");
    let pads = list("BENCH_PAD", "0,300");
    for secs in [2usize, 3, 5, 8, 10] {
        for &pad in &pads {
            let mut p = vec![0.0f32; pad as usize * 16];
            p.extend_from_slice(&pcm[..secs * 16000]);
            if p.len() < 16000 {
                p.resize(16000, 0.0);
            }
            for &m in &margins {
                let frames = p.len().div_ceil(320) as i32;
                let ctx = if m >= 1500 { None } else { Some((frames + m).min(1500)) };
                let opts = DecodeOpts { audio_ctx: ctx, beam_size: 1, prompt: "", single_segment: true };
                let mut ms = Vec::new();
                let mut out = String::new();
                for _ in 0..3 {
                    let t = Instant::now();
                    out = text(&e.transcribe(&p, &opts).unwrap());
                    ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                println!("len {secs:2}s pad {pad:3} ctx {:5}: {:4.0} ms | {out}", ctx.unwrap_or(1500), percentile(&ms, 50.0));
            }
        }
    }
}

/// Zeiten finaler Durchläufe je Beam und `audio_ctx` (Messhilfe, prüft nichts)
#[test]
#[ignore]
fn bench_final_beam() {
    let mut e = engine();
    let pcm = audio("de10");
    for secs in [3usize, 5, 10] {
        let mut p = vec![0.0f32; 300 * 16];
        p.extend_from_slice(&pcm[..secs * 16000]);
        p.extend(vec![0.0f32; 200 * 16]);
        for beam in [1usize, 2, 3, 5] {
            for ctx in [None, Some((p.len().div_ceil(320) + 128) as i32)] {
                let opts = DecodeOpts { audio_ctx: ctx, beam_size: beam, prompt: "", single_segment: false };
                let mut ms = Vec::new();
                let mut out = String::new();
                for _ in 0..3 {
                    let t = Instant::now();
                    out = text(&e.transcribe(&p, &opts).unwrap());
                    ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                println!("FINAL len {secs:2}s ctx {:5} beam {beam}: {:4.0} ms | {out}", ctx.unwrap_or(1500), percentile(&ms, 50.0));
            }
        }
    }
    let t = Instant::now();
    let _ = e.speech_ranges(&pcm).unwrap();
    println!("VAD 10 s: {} ms", t.elapsed().as_millis());
}

fn run_stream(name: &str) -> dictate::simulate::Report {
    let mut e = engine();
    let pcm = audio(name);
    let cfg = Config::default();
    let mut s = Streamer::new(&cfg);
    let mut emit = |_: &serde_json::Value| {};
    let rep = simulate(&mut e, &mut s, &pcm, false, &mut emit).unwrap();
    println!(
        "{name}: shown p50 {:.0} p95 {:.0} ms, finalize {:.0} ms, passes {}/{}/{}\n  {}",
        percentile(&rep.shown_ms, 50.0),
        percentile(&rep.shown_ms, 95.0),
        rep.finalize_ms,
        rep.stats.partial_passes,
        rep.stats.final_passes,
        rep.stats.speculative_passes,
        rep.text
    );
    // stable wächst innerhalb einer Phrase nur; korrigiert wird höchstens beim Festschreiben
    assert!(rep.stable_revisions <= rep.stats.commits, "{} revisions, {} commits", rep.stable_revisions, rep.stats.commits);
    rep
}

#[test]
#[ignore]
fn stream_de10() {
    let rep = run_stream("de10");
    for w in ["Benzin", "Kohlenwasserstoffe", "FCKW", "Entdeckungen", "wurden"] {
        assert!(rep.text.contains(w), "missing {w}: {}", rep.text);
    }
    assert!(!rep.text.contains("Vielen Dank"));
}

#[test]
#[ignore]
fn stream_de30() {
    let rep = run_stream("de30");
    for w in ["Kohlenwasserstoffe", "Verdienste gepriesen", "Vermächtnis", "Umwelthistoriker", "Erdgeschichte"] {
        assert!(rep.text.contains(w), "missing {w}: {}", rep.text);
    }
    assert!(rep.stats.commits >= 2, "expected commits at pauses during speech");
}
