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

//! Ende zu Ende über eine virtuelle Pulse-Quelle (gemeinsam für `daemon_pulse_real` und `speech_testset`):
//! eigenes `module-null-sink`, Aufnahme per `paplay` hinein, eigener Daemon (eigener Socket, eigene Konfiguration,
//! eigener Verlauf) nimmt vom Monitor dieser Senke auf, `start`/`stop` wie beim Halten von Mod+e. Das Modul wird am
//! Ende wieder entladen; Standardquelle und -senke des Users bleiben unberührt (geprüft).
//! `DICTATE_E2E_MAX_GAIN_DB=0` schaltet die Eingangsverstärkung ab, `DICTATE_E2E_DUMP=<dir>` schreibt das beim
//! Daemon angekommene Audio je Lauf als WAV (Diagnose).
#![allow(dead_code)]

use crate::daemon_common::{self, temp_dir, Sub};
use dictate::audio::{AudioSource, Capture, PulseSource};
use dictate::config::Config;
use dictate::daemon::{run_at_path, Deps, RealBackend};
use dictate::protocol::{Event, State};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn pactl(args: &[&str]) -> String {
    let o = Command::new("pactl").args(args).output().expect("pactl");
    assert!(o.status.success(), "pactl {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Eigene Nullsenke; Drop entlädt sie und prüft, dass die Standardgeräte unverändert sind.
pub struct NullSink {
    pub name: String,
    module: String,
    default_source: String,
    default_sink: String,
}

impl NullSink {
    pub fn new() -> NullSink {
        let default_source = pactl(&["get-default-source"]);
        let default_sink = pactl(&["get-default-sink"]);
        let name = format!("dictate_e2e_{}", std::process::id());
        let module = pactl(&["load-module", "module-null-sink", &format!("sink_name={name}"), "sink_properties=device.description=dictate-e2e"]);
        NullSink { name, module, default_source, default_sink }
    }

    pub fn monitor(&self) -> String {
        format!("{}.monitor", self.name)
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        pactl(&["unload-module", &self.module]);
        assert_eq!(pactl(&["get-default-source"]), self.default_source, "default source changed");
        assert_eq!(pactl(&["get-default-sink"]), self.default_sink, "default sink changed");
    }
}

/// Echte Pulse-Aufnahme, die zusätzlich jeden Block mitschreibt (nur zur Diagnose).
struct Tap {
    inner: PulseSource,
    pcm: Arc<Mutex<Vec<f32>>>,
}

impl AudioSource for Tap {
    fn open(&self, source: &str, mut sink: Box<dyn FnMut(Capture) + Send>) -> Box<dyn Send> {
        let pcm = self.pcm.clone();
        pcm.lock().unwrap().clear();
        self.inner.open(
            source,
            Box::new(move |c| {
                if let Capture::Block(b) = &c {
                    pcm.lock().unwrap().extend_from_slice(b);
                }
                sink(c)
            }),
        )
    }
}

/// Ergebnis eines Diktats: fertiger Text (nach der Pipeline, so wie ihn `dictate-ui` ausgibt), Rohtext (Whisper)
/// und die danach gedrückten Tasten.
pub struct Dictation {
    pub text: String,
    pub raw: String,
    pub keys: Vec<String>,
}

pub struct E2e {
    sink: NullSink,
    sock: PathBuf,
    dir: PathBuf,
    pcm: Arc<Mutex<Vec<f32>>>,
    handle: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

impl E2e {
    pub fn start(cfg: Config) -> E2e {
        let sink = NullSink::new();
        let dir = temp_dir();
        let sock = dir.join("run/control.sock");
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let mut cfg = Config { source: sink.monitor(), idle_unload_s: 0, ..cfg };
        if let Some(v) = std::env::var("DICTATE_E2E_MAX_GAIN_DB").ok().and_then(|v| v.parse().ok()) {
            cfg.max_gain_db = v;
        }
        let deps = Deps {
            backend: Arc::new(RealBackend),
            audio: Arc::new(Tap { inner: PulseSource, pcm: pcm.clone() }),
            history: dir.join("state/history.jsonl"),
            act_ttl: dictate::actions::store::TTL,
            palette_env: None,
            diagnose: dictate::diagnose::service::DiagDeps::unavailable(),
        };
        let s2 = sock.clone();
        let handle = std::thread::spawn(move || run_at_path(cfg, None, &s2, deps, false));
        let t = Instant::now();
        while std::os::unix::net::UnixStream::connect(&sock).is_err() {
            assert!(t.elapsed() < Duration::from_secs(5), "daemon did not come up");
            std::thread::sleep(Duration::from_millis(5));
        }
        let e = E2e { sink, sock, dir, pcm, handle: Some(handle) };
        // Modell laden (wie der erste Tastendruck), danach ist es bereit
        let mut sub = Sub::connect(&e.sock);
        e.cmd("start");
        std::thread::sleep(Duration::from_millis(300));
        e.cmd("cancel");
        let t = Instant::now();
        loop {
            match sub.next() {
                Some(Event::State { state: State::Idle }) => break,
                Some(_) => {}
                None => assert!(t.elapsed() < Duration::from_secs(60), "model did not load"),
            }
        }
        e
    }

    pub fn cmd(&self, name: &str) -> String {
        daemon_common::raw_cmd(&self.sock, name)
    }

    /// Ein Diktat: `start`, nach `lead_ms` die Datei abspielen, `stop` bei `speech_end_ms + stop_after_ms` ab
    /// Abspielbeginn.
    pub fn dictate(&self, wav: &Path, lead_ms: u64, speech_end_ms: u64, stop_after_ms: u64, tag: &str) -> Dictation {
        // Events laufend lesen: ein Abonnent, der 30 s nichts abholt, wird vom Daemon getrennt
        let mut sub = Sub::connect(&self.sock);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Some(e) = sub.next() {
                if let Event::Final { text, raw, keys, cancelled, .. } = e {
                    let _ = tx.send((Dictation { text, raw, keys }, cancelled));
                    return;
                }
            }
        });
        self.cmd("start");
        std::thread::sleep(Duration::from_millis(lead_ms));
        let t0 = Instant::now();
        // Feste Lautstärke 0 dB und ohne Gedächtnis von WirePlumber: es stellt sonst je `application.name` die zuletzt
        // gespeicherte Lautstärke wieder her (am 20.11. lief `paplay` so mit −28,4 dB, und die leise Brio-Aufnahme in
        // `quiet_brio_over_pulse` kam leer an); `state.restore-props=false` verhindert Wiederherstellen und Speichern
        let mut play = Command::new("paplay")
            .arg(format!("--device={}", self.sink.name))
            .args(["--volume=65536", "--property=state.restore-props=false"])
            .arg(wav)
            .spawn()
            .expect("paplay");
        let stop_at = Duration::from_millis(speech_end_ms + stop_after_ms);
        std::thread::sleep(stop_at.saturating_sub(t0.elapsed()));
        self.cmd("stop");
        let _ = play.kill();
        let _ = play.wait();
        let (d, cancelled) = rx.recv_timeout(Duration::from_secs(30)).expect("no final event");
        assert!(!cancelled);
        if let Some(dir) = std::env::var_os("DICTATE_E2E_DUMP") {
            let pcm = self.pcm.lock().unwrap().clone();
            write_wav(&Path::new(&dir).join(format!("{tag}.wav")), &pcm);
        }
        d
    }
}

impl Drop for E2e {
    fn drop(&mut self) {
        let _ = self.cmd("shutdown");
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Länge einer 16-kHz-Mono-WAV in ms.
pub fn wav_ms(path: &Path) -> u64 {
    dictate::wav::read_16k_mono(path).unwrap().len() as u64 / 16
}

fn write_wav(path: &Path, pcm: &[f32]) {
    let mut b = Vec::with_capacity(44 + pcm.len() * 2);
    let data = (pcm.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&32000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}
