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

//! Gemeinsame Testhilfen: Mock-Session (ohne GPU), Mock-Audio, Daemon-Harness und Event-Leser.
#![allow(dead_code)]

use anyhow::Result;
use dictate::audio::{AudioSource, Capture};
use dictate::config::Config;
use dictate::daemon::{run_at_path, Backend, Deps, Session};
use dictate::protocol::{Event, State};
use dictate::stream::Partial;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Steuerung und Zähler für die Attrappen.
#[derive(Default)]
pub struct Ctl {
    pub load_delay_ms: AtomicU64,
    pub load_fail: AtomicBool,
    pub tick_panic: AtomicBool,
    pub partial_bytes: AtomicUsize,
    pub audio_fail: AtomicBool,
    pub loads: AtomicUsize,
    pub unloads: AtomicUsize,
    pub opens: AtomicUsize,
    pub live_recordings: AtomicUsize,
    pub resets: AtomicUsize,
    /// `tick` schlägt fehl
    pub tick_fail: AtomicBool,
    /// Der Audio-Thread hängt (wie ein blockierendes `read`), bis `release_hang` gesetzt ist, und schickt dann
    /// einen riesigen Block
    pub hang_read: AtomicBool,
    pub release_hang: AtomicBool,
    /// Text, den `finish` liefert (Standard: „Grüße samples=N“)
    pub finish_text: Mutex<Option<String>>,
    /// Bestätigter Text jedes Zwischenstands (Standard: „WortN“ bzw. `partial_bytes`)
    pub partial_text: Mutex<Option<String>>,
    /// Zuletzt per `set_log_text` gesetzt (`None` = nie aufgerufen)
    pub log_text: Mutex<Option<bool>>,
    /// Zuletzt per `set_language` gesetzte Sprache (`Some(None)` = Konfiguration)
    pub language: Mutex<Option<Option<String>>>,
    /// Lebensdauer der Markierung (Textaktionen) in ms; 0 = `actions::store::TTL`
    pub act_ttl_ms: AtomicU64,
    /// Programmumgebung der Befehlspalette (eigene Verzeichnisse statt der des Test-Prozesses)
    pub palette_env: Mutex<Option<dictate::palette::sources::DesktopEnv>>,
    /// Diagnose: Fixture-Kontext, Identität, eigene Sperre, Lebensdauer; `None` = ohne Proben (`DiagDeps::unavailable`),
    /// nie gegen das echte System
    pub diag: Mutex<Option<dictate::diagnose::service::DiagDeps>>,
    /// Dateien im Zustandsordner (`state/dictate/<Name>`, neben `history.jsonl`), angelegt vor dem Start des Daemons
    pub state_files: Mutex<Vec<(String, String)>>,
}

pub struct MockBackend(pub Arc<Ctl>);

struct MockSession {
    ctl: Arc<Ctl>,
    samples: usize,
    since_tick: usize,
    ticks: usize,
}

impl Backend for MockBackend {
    fn load(&self, _cfg: &Config) -> Result<Box<dyn Session>> {
        std::thread::sleep(Duration::from_millis(self.0.load_delay_ms.load(Ordering::SeqCst)));
        if self.0.load_fail.load(Ordering::SeqCst) {
            anyhow::bail!("mock load failure");
        }
        self.0.loads.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(MockSession { ctl: self.0.clone(), samples: 0, since_tick: 0, ticks: 0 }))
    }
}

impl Drop for MockSession {
    fn drop(&mut self) {
        self.ctl.unloads.fetch_add(1, Ordering::SeqCst);
    }
}

impl Session for MockSession {
    fn push(&mut self, pcm: &[f32]) {
        self.samples += pcm.len();
        self.since_tick += pcm.len();
    }
    fn due(&self) -> bool {
        self.since_tick >= 1600
    }
    fn tick(&mut self) -> Result<Option<Partial>> {
        assert!(!self.ctl.tick_panic.load(Ordering::SeqCst), "mock tick panic");
        if self.ctl.tick_fail.load(Ordering::SeqCst) {
            anyhow::bail!("mock tick failure");
        }
        self.since_tick = 0;
        self.ticks += 1;
        let n = self.ctl.partial_bytes.load(Ordering::SeqCst);
        let stable = match self.ctl.partial_text.lock().unwrap().clone() {
            Some(t) => t,
            None if n > 0 => "x".repeat(n),
            None => format!("Wort{}", self.ticks),
        };
        Ok(Some(Partial { stable, tail: "Rest".into() }))
    }
    fn finish(&mut self) -> Result<String> {
        match self.ctl.finish_text.lock().unwrap().clone() {
            Some(t) => Ok(t),
            None => Ok(format!("Grüße samples={}", self.samples)),
        }
    }
    fn set_language(&mut self, lang: Option<&str>) {
        *self.ctl.language.lock().unwrap() = Some(lang.map(str::to_string));
    }
    fn set_log_text(&mut self, on: bool) {
        *self.ctl.log_text.lock().unwrap() = Some(on);
    }
    fn reset(&mut self) {
        self.ctl.resets.fetch_add(1, Ordering::SeqCst);
        self.samples = 0;
        self.since_tick = 0;
        self.ticks = 0;
    }
}

/// Liefert alle 2 ms einen 20-ms-Block (10x Echtzeit), bis die Aufnahme gedroppt wird.
pub struct MockAudio(pub Arc<Ctl>);

struct MockRec {
    stop: Arc<AtomicBool>,
    h: Option<JoinHandle<()>>,
    ctl: Arc<Ctl>,
}

impl Drop for MockRec {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.h.take() {
            // wie die echte Aufnahme: höchstens kurz warten, dann abkoppeln
            dictate::audio::join_with_timeout(h, dictate::audio::JOIN_TIMEOUT);
        }
        self.ctl.live_recordings.fetch_sub(1, Ordering::SeqCst);
    }
}

impl AudioSource for MockAudio {
    fn list_sources(&self) -> Result<Vec<dictate::audio::SourceInfo>, String> {
        Ok(vec![
            dictate::audio::SourceInfo { name: "alsa_input.mock".into(), description: "Mock Microphone".into(), default: true },
            dictate::audio::SourceInfo { name: "alsa_input.other".into(), description: "Other Input".into(), default: false },
        ])
    }

    fn open(&self, _source: &str, mut sink: Box<dyn FnMut(Capture) + Send>) -> Box<dyn Send> {
        let ctl = self.0.clone();
        ctl.opens.fetch_add(1, Ordering::SeqCst);
        ctl.live_recordings.fetch_add(1, Ordering::SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let fail = ctl.audio_fail.load(Ordering::SeqCst);
        let ctl2 = ctl.clone();
        let h = std::thread::spawn(move || {
            if fail {
                sink(Capture::Failed("cannot open audio source 'mock'".into()));
                return;
            }
            sink(Capture::Opened(Duration::from_millis(1)));
            let mut i = 0usize;
            while !stop2.load(Ordering::SeqCst) {
                if ctl2.hang_read.load(Ordering::SeqCst) {
                    while !ctl2.release_hang.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    // veralteter Block einer früheren Sitzung
                    sink(Capture::Block(vec![0.1; 1_000_000]));
                    return;
                }
                let b: Vec<f32> = (0..320).map(|k| 0.1 * ((i + k) as f32 * 0.3).sin()).collect();
                i += 320;
                sink(Capture::Block(b));
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Box::new(MockRec { stop, h: Some(h), ctl })
    }
}

pub struct Harness {
    pub sock: PathBuf,
    pub dir: PathBuf,
    /// Konfigurationsdatei des Daemons (nur bei `start_with_file`)
    pub cfg_path: Option<PathBuf>,
    pub ctl: Arc<Ctl>,
    handle: Option<JoinHandle<Result<()>>>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("dictate-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

impl Harness {
    pub fn start(cfg: Config) -> Harness {
        Harness::start_with(cfg, Arc::new(Ctl::default()))
    }

    pub fn start_with(cfg: Config, ctl: Arc<Ctl>) -> Harness {
        Harness::start_full(cfg, ctl, false)
    }

    /// Daemon mit Konfigurationsdatei (leer, in einem Temp-Verzeichnis), damit `set_config` und `reload` schreiben bzw. lesen.
    pub fn start_with_file(cfg: Config) -> Harness {
        Harness::start_full(cfg, Arc::new(Ctl::default()), true)
    }

    fn start_full(cfg: Config, ctl: Arc<Ctl>, with_file: bool) -> Harness {
        let dir = temp_dir();
        let cfg_path = with_file.then(|| dir.join("cfg/config.toml"));
        let cp = cfg_path.clone();
        let sock = dir.join("run/control.sock");
        for (name, content) in ctl.state_files.lock().unwrap().iter() {
            std::fs::create_dir_all(dir.join("state/dictate")).unwrap();
            std::fs::write(dir.join("state/dictate").join(name), content).unwrap();
        }
        let act_ttl = match ctl.act_ttl_ms.load(Ordering::SeqCst) {
            0 => dictate::actions::store::TTL,
            ms => Duration::from_millis(ms),
        };
        let deps = Deps {
            backend: Arc::new(MockBackend(ctl.clone())),
            audio: Arc::new(MockAudio(ctl.clone())),
            history: dir.join("state/dictate/history.jsonl"),
            act_ttl,
            palette_env: ctl.palette_env.lock().unwrap().clone(),
            diagnose: ctl.diag.lock().unwrap().clone().unwrap_or_else(dictate::diagnose::service::DiagDeps::unavailable),
        };
        let s2 = sock.clone();
        let handle = std::thread::spawn(move || run_at_path(cfg, cp, &s2, deps, false));
        let t = Instant::now();
        while UnixStream::connect(&sock).is_err() {
            assert!(t.elapsed() < Duration::from_secs(5), "daemon did not come up");
            std::thread::sleep(Duration::from_millis(5));
        }
        Harness { sock, dir, cfg_path, ctl, handle: Some(handle) }
    }

    /// Verlaufsdatei dieses Daemons
    pub fn history_path(&self) -> PathBuf {
        self.dir.join("state/dictate/history.jsonl")
    }

    /// Roher Befehl, liefert die Antwortzeile.
    pub fn cmd(&self, name: &str) -> String {
        raw_cmd(&self.sock, name)
    }

    /// Beliebige Anfrage als JSON-Zeile; liefert die geparste Antwort.
    pub fn req(&self, line: &str) -> serde_json::Value {
        let mut s = UnixStream::connect(&self.sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut out = String::new();
        BufReader::new(s).read_line(&mut out).unwrap();
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("bad reply {out:?}: {e}"))
    }

    /// Wartet, bis `pred` auf dem Zähler/Zustand zutrifft.
    pub fn wait_until(&self, what: &str, pred: impl Fn() -> bool) {
        let t = Instant::now();
        while !pred() {
            assert!(t.elapsed() < Duration::from_secs(5), "timeout waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn state(&self) -> String {
        let r: serde_json::Value = serde_json::from_str(&self.cmd("status")).unwrap();
        r["state"].as_str().unwrap().to_string()
    }

    pub fn wait_state(&self, want: &str) {
        let t = Instant::now();
        while self.state() != want {
            assert!(t.elapsed() < Duration::from_secs(5), "state {want} not reached, now {}", self.state());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn subscribe(&self) -> Sub {
        Sub::connect(&self.sock)
    }

    /// Wartet, bis `run_at` zurückgekehrt ist.
    pub fn join(&mut self) -> Result<()> {
        self.handle.take().unwrap().join().unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = raw_cmd(&self.sock, "shutdown");
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn raw_cmd(sock: &std::path::Path, name: &str) -> String {
    let mut s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    writeln!(s, "{{\"cmd\":\"{name}\"}}").unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    line.trim().to_string()
}

pub struct Sub {
    r: BufReader<UnixStream>,
}

impl Sub {
    pub fn connect(sock: &std::path::Path) -> Sub {
        let mut s = UnixStream::connect(sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        writeln!(s, "{{\"cmd\":\"subscribe\"}}").unwrap();
        Sub { r: BufReader::new(s) }
    }

    pub fn next_line(&mut self) -> Option<String> {
        let mut l = String::new();
        match self.r.read_line(&mut l) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(l.trim().to_string()),
        }
    }

    pub fn next(&mut self) -> Option<Event> {
        self.next_line().map(|l| serde_json::from_str(&l).unwrap())
    }

    /// Liest Events, bis `pred` passt; liefert alle gelesenen (ohne Level-Rauschen: alle).
    pub fn until(&mut self, pred: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut v = Vec::new();
        while let Some(e) = self.next() {
            let hit = pred(&e);
            v.push(e);
            if hit {
                return v;
            }
        }
        panic!("event not seen, got {v:?}");
    }

    /// Schickt eine Anfrage über diese (Subscriber-)Verbindung.
    pub fn send(&mut self, line: &str) {
        writeln!(self.r.get_mut(), "{line}").unwrap();
    }

    /// Liest Zeilen, bis eine mit `"ok"` kommt (Antwort auf eine Anfrage auf dieser Verbindung).
    pub fn reply(&mut self) -> serde_json::Value {
        loop {
            let l = self.next_line().expect("connection closed before the reply");
            let v: serde_json::Value = serde_json::from_str(&l).unwrap();
            if v.get("ok").is_some() {
                return v;
            }
        }
    }

    pub fn until_state(&mut self, s: State) -> Vec<Event> {
        self.until(|e| matches!(e, Event::State { state } if *state == s))
    }
}

pub fn quick_cfg() -> Config {
    Config { idle_unload_s: 0, ..Config::default() }
}
