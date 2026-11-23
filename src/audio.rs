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

//! Audioaufnahme über libpulse simple (16 kHz mono f32), nur während des Zuhörens geöffnet.

use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: u32 = 16000;
/// 20 ms pro Block
pub const BLOCK_SAMPLES: usize = 320;

/// Meldungen der Aufnahme an den Daemon.
#[derive(Debug)]
pub enum Capture {
    /// Stream offen, erster Block gelesen; Dauer ab dem Öffnen
    Opened(Duration),
    Block(Vec<f32>),
    Failed(String),
}

/// Quelle von Audioblöcken; austauschbar für Tests. Der Rückgabewert beendet die Aufnahme beim Drop.
pub trait AudioSource: Send + Sync + 'static {
    fn open(&self, source: &str, sink: Box<dyn FnMut(Capture) + Send>) -> Box<dyn Send>;

    /// Verfügbare Eingabequellen (ohne Monitore); Standard: keine.
    fn list_sources(&self) -> Result<Vec<SourceInfo>, String> {
        Ok(Vec::new())
    }
}

/// Eine Eingabequelle für die Auswahl im Einstellungsfenster.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceInfo {
    /// Pulse-Name, gehört in `source` der Konfiguration
    pub name: String,
    pub description: String,
    /// Aktuelle Standardquelle des Servers
    pub default: bool,
}

/// Echte Aufnahme über PulseAudio/pipewire-pulse.
pub struct PulseSource;

impl AudioSource for PulseSource {
    fn open(&self, source: &str, sink: Box<dyn FnMut(Capture) + Send>) -> Box<dyn Send> {
        Box::new(Recording::start(source, sink))
    }

    fn list_sources(&self) -> Result<Vec<SourceInfo>, String> {
        list_sources()
    }
}

/// So lange wartet `list_sources` höchstens auf den Pulse-Server.
const LIST_TIMEOUT: Duration = Duration::from_secs(3);

/// Listet die Eingabequellen über libpulse (Introspect). Monitor-Quellen (Ausgänge abgreifen) fehlen, die
/// Standardquelle ist markiert.
pub fn list_sources() -> Result<Vec<SourceInfo>, String> {
    use libpulse_binding::callbacks::ListResult;
    use libpulse_binding::context::{Context, FlagSet, State as CtxState};
    use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let mut ml = Mainloop::new().ok_or("cannot create the audio main loop")?;
    let mut ctx = Context::new(&ml, "dictate-sources").ok_or("cannot create the audio context")?;
    ctx.connect(None, FlagSet::NOFLAGS, None).map_err(|e| format!("cannot connect to the audio server: {e}"))?;
    let deadline = Instant::now() + LIST_TIMEOUT;
    let step = |ml: &mut Mainloop| -> Result<(), String> {
        if Instant::now() >= deadline {
            return Err("the audio server does not answer".into());
        }
        match ml.iterate(false) {
            IterateResult::Success(0) => std::thread::sleep(Duration::from_millis(2)),
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => return Err("audio main loop stopped".into()),
        }
        Ok(())
    };
    loop {
        step(&mut ml)?;
        match ctx.get_state() {
            CtxState::Ready => break,
            CtxState::Failed | CtxState::Terminated => return Err("cannot connect to the audio server".into()),
            _ => {}
        }
    }

    let items: Rc<RefCell<Vec<(String, String, bool)>>> = Rc::default();
    let listed = Rc::new(Cell::new(false));
    let default_name: Rc<RefCell<Option<String>>> = Rc::default();
    let got_server = Rc::new(Cell::new(false));
    let intro = ctx.introspect();
    let _op1 = {
        let (items, listed) = (items.clone(), listed.clone());
        intro.get_source_info_list(move |r| match r {
            ListResult::Item(i) => {
                let name = i.name.as_deref().unwrap_or("").to_string();
                let desc = i.description.as_deref().unwrap_or("").to_string();
                items.borrow_mut().push((name, desc, i.monitor_of_sink.is_some()));
            }
            ListResult::End | ListResult::Error => listed.set(true),
        })
    };
    let _op2 = {
        let (default_name, got_server) = (default_name.clone(), got_server.clone());
        intro.get_server_info(move |si| {
            *default_name.borrow_mut() = si.default_source_name.as_deref().map(str::to_string);
            got_server.set(true);
        })
    };
    while !(listed.get() && got_server.get()) {
        step(&mut ml)?;
    }
    ctx.disconnect();

    let dflt = default_name.borrow().clone().unwrap_or_default();
    let out = items
        .borrow()
        .iter()
        .filter(|(name, _, monitor)| !monitor && !name.is_empty())
        .map(|(name, desc, _)| SourceInfo {
            default: *name == dflt,
            description: if desc.is_empty() { name.clone() } else { desc.clone() },
            name: name.clone(),
        })
        .collect();
    Ok(out)
}

/// Laufende Aufnahme; Drop beendet den Thread (höchstens ein Block Verzögerung) und schließt den Stream.
/// Hängt Pulse, wird der Thread nach `JOIN_TIMEOUT` abgekoppelt; seine Blöcke tragen die alte Sitzungsgeneration.
pub struct Recording {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Recording {
    pub fn start(source: &str, mut sink: Box<dyn FnMut(Capture) + Send>) -> Recording {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let source = source.to_string();
        let handle = std::thread::Builder::new()
            .name("dictate-audio".into())
            .spawn(move || {
                if let Err(e) = record_loop(&source, &stop2, &mut sink) {
                    if !stop2.load(Ordering::Relaxed) {
                        sink(Capture::Failed(e));
                    }
                }
            })
            .expect("Audio-Thread starten");
        Recording { stop, handle: Some(handle) }
    }
}

/// So lange wartet `Drop` höchstens auf den Audio-Thread, bevor er abgekoppelt wird.
pub const JOIN_TIMEOUT: Duration = Duration::from_millis(500);

/// Wartet höchstens `timeout` darauf, dass der Thread endet. Bei Erfolg wird er gejoint (`true`), sonst
/// abgekoppelt (`false`): ein in `Simple::new` oder `read` hängender Pulse-Thread darf den Kern nicht blockieren.
pub fn join_with_timeout(h: JoinHandle<()>, timeout: Duration) -> bool {
    let t0 = Instant::now();
    while !h.is_finished() {
        if t0.elapsed() >= timeout {
            return false; // Handle fällt hier, der Thread läuft für sich weiter
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = h.join();
    true
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            if !join_with_timeout(h, JOIN_TIMEOUT) {
                tracing::warn!("audio thread does not stop within {} ms, detaching it", JOIN_TIMEOUT.as_millis());
            }
        }
    }
}

fn record_loop(source: &str, stop: &AtomicBool, sink: &mut (dyn FnMut(Capture) + Send)) -> Result<(), String> {
    let spec = Spec { format: Format::F32le, channels: 1, rate: SAMPLE_RATE };
    let bytes = BLOCK_SAMPLES * 4;
    // Kleines Fragment: Latenz ~20 ms; alles andere auf Server-Default
    let attr = BufferAttr { maxlength: u32::MAX, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: bytes as u32 };
    let dev = if source.is_empty() { None } else { Some(source) };
    let t0 = Instant::now();
    let simple = Simple::new(None, "dictate", Direction::Record, dev, "dictation", &spec, None, Some(&attr))
        .map_err(|e| format!("cannot open audio source{}: {e}", dev.map(|d| format!(" '{d}'")).unwrap_or_default()))?;
    let mut buf = vec![0u8; bytes];
    let mut first = true;
    while !stop.load(Ordering::Relaxed) {
        simple.read(&mut buf).map_err(|e| format!("audio read failed: {e}"))?;
        if first {
            first = false;
            sink(Capture::Opened(t0.elapsed()));
        }
        let block: Vec<f32> = buf.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        sink(Capture::Block(block));
    }
    Ok(())
}

/// Pegel pro Messfenster (~100 ms): RMS in dBFS plus Sprach-Flag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    pub db: f32,
    pub speech: bool,
}

/// Schwelle für „Sprache“ (RMS über 100 ms). Raumrauschen liegt bei etwa −77 dBFS.
pub const SPEECH_DB: f32 = -50.0;
const WINDOW_SAMPLES: usize = SAMPLE_RATE as usize / 10;
/// Nachlauf, damit das Flag zwischen Silben nicht flackert
const HANGOVER_WINDOWS: u32 = 2;

pub fn rms_db(pcm: &[f32]) -> f32 {
    if pcm.is_empty() {
        return -120.0;
    }
    let ms = pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len() as f32;
    (10.0 * ms.max(1e-12).log10()).max(-120.0)
}

/// Fasst Blöcke zu höchstens 10 Pegelwerten pro Sekunde zusammen.
#[derive(Default)]
pub struct LevelMeter {
    sum_sq: f64,
    n: usize,
    hang: u32,
}

impl LevelMeter {
    pub fn push(&mut self, pcm: &[f32]) -> Option<Level> {
        self.sum_sq += pcm.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>();
        self.n += pcm.len();
        if self.n < WINDOW_SAMPLES {
            return None;
        }
        let ms = (self.sum_sq / self.n as f64).max(1e-12);
        let db = ((10.0 * ms.log10()) as f32).max(-120.0);
        self.sum_sq = 0.0;
        self.n = 0;
        if db > SPEECH_DB {
            self.hang = HANGOVER_WINDOWS;
        } else {
            self.hang = self.hang.saturating_sub(1);
        }
        Some(Level { db: (db * 10.0).round() / 10.0, speech: db > SPEECH_DB || self.hang > 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 0.3).sin()).collect()
    }

    /// Braucht einen laufenden Pulse-/PipeWire-Server.
    #[test]
    #[ignore]
    fn list_sources_real() {
        let v = list_sources().unwrap();
        eprintln!("{v:?}");
        assert!(!v.is_empty());
        assert!(v.iter().all(|s| !s.name.ends_with(".monitor")));
        assert!(v.iter().filter(|s| s.default).count() <= 1);
    }

    #[test]
    fn rms_of_full_scale_square_is_zero_db() {
        assert!((rms_db(&vec![1.0; 100])).abs() < 0.01);
        assert_eq!(rms_db(&[]), -120.0);
        assert_eq!(rms_db(&[0.0; 10]), -120.0);
    }

    #[test]
    fn meter_emits_at_most_10_hz() {
        let mut m = LevelMeter::default();
        let mut n = 0;
        for _ in 0..50 {
            // 50 Blöcke = 1 s
            if m.push(&sine(0.1, BLOCK_SAMPLES)).is_some() {
                n += 1;
            }
        }
        assert!((9..=10).contains(&n), "{n}");
    }

    #[test]
    fn speech_flag_with_hangover() {
        let mut m = LevelMeter::default();
        let loud = |m: &mut LevelMeter| (0..5).filter_map(|_| m.push(&sine(0.1, BLOCK_SAMPLES))).last();
        let quiet = |m: &mut LevelMeter| (0..5).filter_map(|_| m.push(&sine(0.0001, BLOCK_SAMPLES))).last();
        assert!(loud(&mut m).unwrap().speech);
        // Nachlauf: die ersten zwei leisen Fenster gelten noch als Sprache
        assert!(quiet(&mut m).unwrap().speech);
        quiet(&mut m);
        let l = quiet(&mut m).unwrap();
        assert!(!l.speech);
        assert!(l.db < -70.0);
    }
}
