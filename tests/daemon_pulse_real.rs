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

//! Ende zu Ende über eine virtuelle Pulse-Quelle mit echtem Modell (GPU), wie beim Halten von Mod+e:
//! `cargo test --release --test daemon_pulse_real -- --ignored --nocapture --test-threads=1`
//!
//! Aufbau (Nullsenke, eigener Daemon, `start`/`stop`) in `pulse_common`. `start` kommt kurz vor dem Sprachbeginn,
//! `stop` direkt bzw. 100–300 ms nach dem Sprachende. `DICTATE_E2E_MAX_GAIN_DB=0` schaltet die Eingangsverstärkung
//! ab (Vergleich mit dem Stand davor), `DICTATE_E2E_DUMP=<dir>` schreibt das beim Daemon angekommene Audio je Lauf
//! als WAV (Diagnose).

mod daemon_common;
mod pulse_common;
mod test_root;

use daemon_common::temp_dir;
use dictate::config::Config;
use pulse_common::{wav_ms, E2e};
use std::path::PathBuf;
use std::process::Command;

/// Testaufnahme unter `$DICTATE_TEST_ROOT/audio/`; fehlt sie, bricht der Test mit Pfad und Hinweis ab.
fn existing_audio(name: &str) -> PathBuf {
    let path = test_root::audio(name);
    assert!(path.is_file(), "{} is missing ({})", path.display(), test_root::AUDIO_HINT);
    path
}

/// Ziffern und Wörter vereinheitlichen: „Test 1, 2, 3“, „Test 1,2,3“ und „Test eins, zwei, drei“ sind sinngleich.
fn norm(s: &str) -> String {
    let lower = s.to_lowercase();
    let words: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "eins" => "1".into(),
            "zwei" => "2".into(),
            "drei" => "3".into(),
            w => w.to_string(),
        })
        .collect();
    words.join(" ")
}

/// Leise Brio-Aufnahme („Test eins, zwei, drei“, Sprache ca. 0,65–2,69 s) und dieselbe um 10 dB leiser:
/// Stop direkt bzw. 100 und 300 ms nach dem Sprachende.
#[test]
#[ignore]
fn quiet_brio_over_pulse() {
    let quiet = temp_dir().join("live-brio-m10.wav");
    let brio = existing_audio("live-brio.wav");
    let st = Command::new("sox").args([brio.to_str().unwrap(), quiet.to_str().unwrap(), "gain", "-10"]).status().expect("sox");
    assert!(st.success());
    let e = E2e::start(Config::default());
    let mut bad = Vec::new();
    for (name, wav) in [("brio", brio.clone()), ("brio-m10", quiet.clone())] {
        for after in [0u64, 100, 300] {
            let raw = e.dictate(&wav, 150, 2690, after, &format!("{name}-{after}")).raw;
            println!("{name} stop +{after} ms: {raw:?}");
            if norm(&raw) != "test 1 2 3" {
                bad.push(format!("{name} +{after}: {raw:?}"));
            }
        }
    }
    let _ = std::fs::remove_file(&quiet);
    assert!(bad.is_empty(), "{bad:?}");
}

/// Normal ausgesteuerte Aufnahmen über denselben Weg (Vergleich vor/nach der Verstärkung): Schlüsselwörter wie
/// in `stream_real`, Stop 300 ms nach Dateiende.
#[test]
#[ignore]
fn reference_recordings_over_pulse() {
    let e = E2e::start(Config::default());
    let cases: [(&str, &[&str]); 3] = [
        // de05 endet mitten im lauten Wort „Kohlenwasserstoffe“ (Ausschnitt), das letzte Wort kippt schon vor der
        // Eingangsverstärkung je nach Lauf zu „Kohlenwasser“
        ("de05", &["sowohl", "Fluorchlor"]),
        ("de10", &["Benzin", "Kohlenwasserstoffe", "FCKW", "Entdeckungen", "wurden"]),
        ("de30", &["Kohlenwasserstoffe", "Verdienste gepriesen", "Vermächtnis", "Umwelthistoriker", "Erdgeschichte"]),
    ];
    let mut bad = Vec::new();
    for (name, words) in cases {
        let wav = existing_audio(&format!("{name}.wav"));
        let raw = e.dictate(&wav, 150, wav_ms(&wav), 300, name).raw;
        println!("{name}: {raw}");
        for w in words {
            if !raw.to_lowercase().contains(&w.to_lowercase()) {
                bad.push(format!("{name}: missing {w}"));
            }
        }
        assert!(!raw.contains("Vielen Dank"), "{name}: {raw}");
    }
    assert!(bad.is_empty(), "{bad:?}");
}
