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

//! Tests mit echtem Modell (brauchen GPU und Modelle): `cargo test --release -- --ignored --nocapture`

mod test_root;

use dictate::config::Config;
use dictate::engine::{DecodeOpts, Engine, Transcriber};
use std::time::Instant;

fn audio(name: &str) -> Vec<f32> {
    let path = test_root::audio(&format!("{name}.wav"));
    dictate::wav::read_16k_mono(&path).unwrap_or_else(|e| panic!("{}: {e} ({})", path.display(), test_root::AUDIO_HINT))
}

#[test]
#[ignore]
fn vad_ranges_and_cost() {
    let mut e = Engine::load(&Config::default()).unwrap();
    let pcm = audio("de10");
    let t = Instant::now();
    let r = e.speech_ranges(&pcm).unwrap();
    println!("de10: {} ranges in {} ms: {:?}", r.len(), t.elapsed().as_millis(), r);
    let t = Instant::now();
    let _ = e.speech_ranges(&pcm).unwrap();
    println!("second call {} ms", t.elapsed().as_millis());
    assert!(!r.is_empty());
    assert!(r.last().unwrap().end <= pcm.len());
    // Stille liefert keine Sprache
    let silence = vec![0.0f32; 16000 * 3];
    assert!(e.speech_ranges(&silence).unwrap().is_empty());
}

#[test]
#[ignore]
fn transcribe_de05() {
    let mut e = Engine::load(&Config::default()).unwrap();
    let opts = DecodeOpts { audio_ctx: None, beam_size: 5, prompt: "", single_segment: true };
    let segs = e.transcribe(&audio("de05"), &opts).unwrap();
    println!("{segs:?}");
    assert!(segs.iter().any(|s| s.text.contains("Fluorchlor")));
}

/// Sehr kurze Eingaben (unter einem VAD-Fenster von 512 Samples bzw. unter einem Block) dürfen weder panikieren
/// noch scheitern.
#[test]
#[ignore]
fn very_short_inputs() {
    let mut e = Engine::load(&Config::default()).unwrap();
    let opts = DecodeOpts { audio_ctx: None, beam_size: 1, prompt: "", single_segment: true };
    for n in [1usize, 320, 511] {
        let pcm = vec![0.01f32; n];
        let r = e.speech_ranges(&pcm).unwrap_or_else(|err| panic!("speech_ranges({n}): {err:#}"));
        println!("speech_ranges({n}) = {r:?}");
        let t = e.transcribe(&pcm, &opts).unwrap_or_else(|err| panic!("transcribe({n}): {err:#}"));
        println!("transcribe({n}) = {} segments", t.len());
    }
}

/// `finish` auf einer 20-ms-Aufnahme (ein Block) liefert leeren Text.
#[test]
#[ignore]
fn streamer_finish_on_20ms() {
    let mut e = Engine::load(&Config::default()).unwrap();
    let mut s = dictate::stream::Streamer::new(&Config::default());
    s.push(&vec![0.01f32; 320]);
    let text = s.finish(&mut e).unwrap();
    println!("finish(20 ms) = {text:?}");
    assert!(text.is_empty());
}

/// Sprache je Sitzung (Profil) ohne neues Laden: Konfiguration Deutsch, Sitzung Englisch. Auf deutscher Sprache
/// schreibt Whisper mit erzwungenem Englisch eine Übersetzung statt des deutschen Texts.
#[test]
#[ignore]
fn session_language_overrides_config() {
    let mut e = Engine::load(&Config::default()).unwrap();
    let opts = DecodeOpts { audio_ctx: None, beam_size: 1, prompt: "", single_segment: false };
    let pcm = audio("de05");
    let text = |e: &mut Engine| e.transcribe(&pcm, &opts).unwrap().iter().map(|s| s.text.clone()).collect::<String>();
    let de = text(&mut e);
    e.set_session_language(Some("en"));
    let en = text(&mut e);
    e.set_session_language(None);
    let back = text(&mut e);
    println!("de: {de}\nen: {en}\nback: {back}");
    assert_ne!(de, en);
    assert_eq!(de, back);
}
