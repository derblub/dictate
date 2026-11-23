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

//! Daemon: Steuer-Socket, Zustandsmaschine, Engine-Thread, Broadcast an Subscriber.
//!
//! Threads (im Leerlauf alle blockiert, kein Polling):
//! - Kern (`Core::run`): besitzt Zustand und Subscriber, wartet blockierend auf `Msg`; einzige Zeitquelle ist die
//!   Frist für das Entladen des Modells (`recv_timeout`).
//! - Engine-Thread (`worker`): besitzt die `Session` (Engine + Streamer), arbeitet `Work` ab, nie mehrere Durchläufe
//!   gleichzeitig; Audio, das während eines Durchlaufs eintrifft, wird danach gesammelt nachgeschoben.
//! - Audio-Thread (`audio::Recording`): nur während des Zuhörens, schickt Blöcke direkt an den Engine-Thread
//!   und Pegel an den Kern.
//! - Accept-Thread und je Verbindung ein Thread; je Subscriber zusätzlich ein Schreib-Thread. KI-Aufrufe
//!   (`ai_test`, `ai_models`, Textaktionen `act_run`) laufen auf dem Verbindungs-Thread, der Kern liefert nur kurz die
//!   Einstellungen.
//! - Befehlspalette (`pal_*`, `palette::Palette`): wie die KI-Aufrufe auf dem Verbindungs-Thread; der Kern gibt nur die
//!   Einstellungen weiter (`Palette::configure`) und wartet nie auf Quellen oder Häufigkeit.
//! - Diagnose (`diag_*`, `diagnose::service::Diagnose`): Sammeln und Prüfen auf dem Verbindungs-Thread (bis zu 15 s,
//!   Fortschritt als Zwischenzeilen, Abbruch beim Auflegen); der Kern liefert nur die Einstellungen. Ein eigener
//!   Ablauf-Thread löscht die Sitzung nach höchstens 30 min.
//! - Ablauf-Thread der Textaktionen (`actions::store::Sweeper`): löscht die Markierung nach `actions::store::TTL`.
//! - Signal-Thread (nur im echten Betrieb): SIGTERM/SIGINT → `Msg::Quit`.

use crate::actions::store::{self as act_store, ActResult, NewSelection, ResultError, RunError, SecretText, Store as ActStore, TargetKey};
use crate::audio::{AudioSource, Capture, Level, LevelMeter, PulseSource};
use crate::config::{reload_kind, Config, OverlayCfg, ReloadKind, SCHEMA};
use crate::diagnose::service::{CollectRequest, DiagDeps, Diagnose};
use crate::engine::{DecodeOpts, Engine, Transcriber};
use crate::palette::sources::frecency;
use crate::palette::{Palette, PaletteError};
use crate::pipeline::ai::CancelFlag;
use crate::pipeline::history::History;
use crate::pipeline::Typed;
use crate::protocol::{self, Cmd, Event, Reply, Request, State};
use crate::stream::{Partial, Streamer};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Streamer-Nutzung hinter einem Trait, damit der Daemon ohne GPU testbar ist.
/// Eine Session besitzt Engine und Streamer; Drop gibt das Modell (VRAM) frei.
pub trait Session: Send {
    fn push(&mut self, pcm: &[f32]);
    fn due(&self) -> bool;
    fn tick(&mut self) -> Result<Option<Partial>>;
    fn finish(&mut self) -> Result<String>;
    fn reset(&mut self);
    /// Wächst die offene Phrase unkontrolliert (Festschreiben scheitert dauerhaft)?
    fn overrun(&self) -> bool {
        false
    }
    /// Übernimmt geänderte Einstellungen, die kein neues Laden des Modells brauchen (Schrittweite, Pause,
    /// Blocklist, Prompt …); wirkt ab dem nächsten Durchlauf.
    fn configure(&mut self, _cfg: &Config) {}
    /// Dürfen Texte dieser Sitzung ins Debug-Log (siehe `Work::Begin`); gilt bis zum nächsten Aufruf.
    fn set_log_text(&mut self, _on: bool) {}
    /// Sprache dieser Sitzung aus dem Profil (`None` = aus der Konfiguration); gilt bis zum nächsten Aufruf.
    fn set_language(&mut self, _lang: Option<&str>) {}
}

/// Lädt das Modell und liefert eine Session.
pub trait Backend: Send + Sync + 'static {
    fn load(&self, cfg: &Config) -> Result<Box<dyn Session>>;
}

pub struct RealBackend;

struct RealSession {
    engine: Engine,
    streamer: Streamer,
}

impl Backend for RealBackend {
    fn load(&self, cfg: &Config) -> Result<Box<dyn Session>> {
        let mut engine = Engine::load(cfg)?;
        // Aufwärmen: der erste CUDA-Durchlauf ist deutlich langsamer, das soll nicht die erste Phrase treffen
        let warm = vec![0.0f32; 16_000];
        let opts = DecodeOpts { audio_ctx: None, beam_size: 1, prompt: "", single_segment: true };
        engine.transcribe(&warm, &opts)?;
        let _ = engine.speech_ranges(&warm)?;
        Ok(Box::new(RealSession { engine, streamer: Streamer::new(cfg) }))
    }
}

impl Session for RealSession {
    fn push(&mut self, pcm: &[f32]) {
        self.streamer.push(pcm)
    }
    fn due(&self) -> bool {
        self.streamer.due()
    }
    fn tick(&mut self) -> Result<Option<Partial>> {
        self.streamer.tick(&mut self.engine)
    }
    fn finish(&mut self) -> Result<String> {
        self.streamer.finish(&mut self.engine)
    }
    fn reset(&mut self) {
        self.streamer.reset()
    }
    fn overrun(&self) -> bool {
        self.streamer.overrun()
    }
    fn configure(&mut self, cfg: &Config) {
        self.streamer.apply_config(cfg)
    }
    fn set_log_text(&mut self, on: bool) {
        self.streamer.set_log_text(on)
    }
    fn set_language(&mut self, lang: Option<&str>) {
        self.engine.set_session_language(lang)
    }
}

#[derive(Clone)]
pub struct Deps {
    pub backend: Arc<dyn Backend>,
    pub audio: Arc<dyn AudioSource>,
    /// Verlaufsdatei (`history.jsonl`)
    pub history: PathBuf,
    /// Lebensdauer einer Markierung der Textaktionen ohne Benutzung (`actions::store::TTL`; Tests kürzer)
    pub act_ttl: Duration,
    /// Programmumgebung der Befehlspalette (XDG-Verzeichnisse, Locale); `None` = die des Prozesses, Tests setzen eigene
    /// Verzeichnisse. Die Datei der Häufigkeit (`frecency.json`) liegt neben der Verlaufsdatei.
    pub palette_env: Option<crate::palette::sources::DesktopEnv>,
    /// Diagnose: Kontext der Proben, Identität, Sperre der Sammlungen, Lebensdauer der Sitzung (Tests: Fixtures)
    pub diagnose: DiagDeps,
}

impl Deps {
    pub fn real() -> Deps {
        Deps {
            backend: Arc::new(RealBackend),
            audio: Arc::new(PulseSource),
            history: crate::pipeline::history::default_path(),
            act_ttl: act_store::TTL,
            palette_env: None,
            diagnose: DiagDeps::system(),
        }
    }
}

/// Das zuletzt getippte Diktat (Länge, App, Zeitpunkt), solange seitdem nichts anderes getippt wurde; für
/// „rückgängig“. Der Kern setzt es beim `final`-Event, der Engine-Thread liest es in der Pipeline.
type LastTyped = Arc<Mutex<Option<Typed>>>;

/// Echter Betrieb: Standard-Socket, SIGTERM/SIGINT beenden sauber. `config_path` ist die TOML-Datei, in die
/// `set_config` schreibt und aus der `reload` liest.
pub fn run(cfg: Config, config_path: Option<PathBuf>) -> Result<()> {
    run_at_path(cfg, config_path, &protocol::socket_path(), Deps::real(), true)
}

// ---------------------------------------------------------------------------------------------------------------
// Nachrichten

enum Work {
    Load,
    /// Beginn einer Aufnahmesitzung mit dieser Generation und dem App-Namen (`app_id`) für das Profil;
    /// Audio anderer Generationen wird verworfen. Das dritte Feld erlaubt Texte im Debug-Log (`DICTATE_DEBUG` und eine
    /// Aufnahme für ein Fenster; die Eingabe der Befehlspalette nie, W5 Durchsicht N1)
    Begin(u64, Option<String>, bool),
    /// Neue Einstellungen (Reihenfolge zu `Load`/`Unload` wie im Kern gesendet)
    Config(Box<Config>),
    Audio(u64, Vec<f32>),
    Finish,
    Cancel,
    Unload,
    Quit,
}

impl FinalMsg {
    fn cancelled(audio_ms: u64, finalize_ms: u64) -> FinalMsg {
        FinalMsg { text: String::new(), raw: String::new(), keys: Vec::new(), app: None, cancelled: true, audio_ms, finalize_ms }
    }
}

struct FinalMsg {
    /// Fertiger Text nach der Pipeline
    text: String,
    /// Text vor der Pipeline (Ausgabe von `Streamer::finish`)
    raw: String,
    /// Tastennamen (X-Keysym-Namen), die nach dem Text gedrückt werden
    keys: Vec<String>,
    /// `app_id` der Sitzung (für den Verlauf)
    app: Option<String>,
    cancelled: bool,
    audio_ms: u64,
    finalize_ms: u64,
}

enum Msg {
    /// Anfrage; die Verbindungs-Id nennt den Besitzer, wenn sie über eine Subscriber-Verbindung kam
    Req(Request, Option<u64>, Sender<Reply>),
    Subscribe(u64, SyncSender<Arc<str>>),
    Unsub(u64),
    Level(u64, Level),
    /// Pegel der reinen Pegelmessung (`meter`), Generation der Messaufnahme
    MeterLevel(u64, Level),
    MeterFailed(u64, String),
    AudioOpened(u64, Duration),
    AudioFailed(u64, String),
    Loaded(Result<(), String>),
    Partial(Partial),
    Final(FinalMsg),
    /// Nicht fatale Fehlermeldung (z. B. ein fehlgeschlagener Zwischen-Durchlauf)
    Error(String),
    /// Der Engine-Thread hat einen Panic abgefangen; die Session ist weg
    Panicked(String),
    /// Die Engine scheitert dauerhaft (mehrere Durchläufe in Folge bzw. unkontrolliert wachsende Phrase)
    EngineFailed(String),
    /// KI-Einstellungen für `ai_models`/`ai_test` (die Verbindung ruft selbst, damit der Kern nicht wartet)
    AiCfg(Sender<crate::config::AiCfg>),
    /// Ganze Konfiguration für die Textaktionen (`features.actions`, `actions`, `ai`, `language`); wie bei `AiCfg`
    /// ruft die Verbindung selbst
    ActCfg(Sender<Box<Config>>),
    Quit,
}

// ---------------------------------------------------------------------------------------------------------------
// Engine-Thread

struct Worker {
    session: Option<Box<dyn Session>>,
    samples: u64,
    last: Option<Partial>,
    tick_failed: Option<Instant>,
    /// Fehlgeschlagene Durchläufe in Folge
    tick_fails: u32,
    /// `EngineFailed` ist für diese Sitzung schon gemeldet
    failed_sent: bool,
    /// Generation der laufenden Aufnahme; `None` = keine
    gen: Option<u64>,
    /// App-Name (`app_id`) der laufenden Sitzung
    app: Option<String>,
    /// Texte der laufenden Sitzung dürfen ins Debug-Log (`Work::Begin`); ohne Sitzung nie
    log_text: bool,
    core: Sender<Msg>,
    backend: Arc<dyn Backend>,
    cfg: Config,
    last_typed: LastTyped,
    ai_cancel: CancelFlag,
}

impl Worker {
    fn audio_ms(&self) -> u64 {
        self.samples / 16
    }

    /// Verarbeitet einen Auftrag; `true` = beenden.
    fn handle(&mut self, w: Work, rx: &Receiver<Work>, next: &mut Option<Work>) -> bool {
        match w {
            Work::Quit => return true,
            Work::Load => {
                let t = Instant::now();
                let r = self.backend.load(&self.cfg);
                match r {
                    Ok(mut s) => {
                        info!("model loaded in {} ms", t.elapsed().as_millis());
                        if self.gen.is_some() {
                            // Sitzung hat vor dem Laden begonnen
                            s.set_language(crate::pipeline::profile::language(&self.cfg, self.app.as_deref()).as_deref());
                        }
                        s.set_log_text(self.gen.is_some() && self.log_text);
                        self.session = Some(s);
                        let _ = self.core.send(Msg::Loaded(Ok(())));
                    }
                    Err(e) => {
                        let _ = self.core.send(Msg::Loaded(Err(format!("cannot load model: {e:#}"))));
                    }
                }
            }
            Work::Unload => {
                self.session = None;
                self.samples = 0;
                self.last = None;
            }
            Work::Begin(g, app, log_text) => {
                self.gen = Some(g);
                self.log_text = log_text;
                if let Some(s) = self.session.as_mut() {
                    s.set_language(crate::pipeline::profile::language(&self.cfg, app.as_deref()).as_deref());
                    s.set_log_text(log_text);
                }
                self.app = app;
            }
            Work::Config(cfg) => {
                if let Some(s) = self.session.as_mut() {
                    s.configure(&cfg);
                }
                self.cfg = *cfg;
            }
            Work::Audio(g, mut block) => {
                if self.gen != Some(g) {
                    return false; // Block eines abgekoppelten Audio-Threads einer früheren Sitzung
                }
                // Rückstau sammeln: alles, was schon wartet, in einem Rutsch schieben
                loop {
                    match rx.try_recv() {
                        Ok(Work::Audio(g2, more)) => {
                            if g2 == g {
                                block.extend_from_slice(&more);
                            }
                        }
                        Ok(other) => {
                            *next = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                self.samples += block.len() as u64;
                let mut fail: Option<String> = None;
                if let Some(s) = self.session.as_mut() {
                    s.push(&block);
                    let backoff = self.tick_failed.is_some_and(|t| t.elapsed() < Duration::from_secs(1));
                    if !backoff && s.due() {
                        match s.tick() {
                            Ok(Some(p)) => {
                                self.tick_fails = 0;
                                if self.last.as_ref() != Some(&p) {
                                    self.last = Some(p.clone());
                                    let _ = self.core.send(Msg::Partial(p));
                                }
                            }
                            Ok(None) => self.tick_fails = 0,
                            Err(e) => {
                                self.tick_failed = Some(Instant::now());
                                self.tick_fails += 1;
                                let _ = self.core.send(Msg::Error(format!("transcription failed: {e:#}")));
                                if self.tick_fails >= MAX_TICK_FAILS {
                                    fail = Some(format!("transcription failed {} times in a row, stopping", self.tick_fails));
                                }
                            }
                        }
                    }
                    if s.overrun() {
                        fail = Some("transcription cannot keep up, stopping".into());
                    }
                }
                if let Some(m) = fail {
                    self.fail_engine(m);
                }
            }
            Work::Finish => {
                let audio_ms = self.audio_ms();
                let t = Instant::now();
                let finished = self.session.as_mut().map(|s| {
                    let r = s.finish();
                    s.reset();
                    r
                });
                let app = self.app.take();
                let msg = match finished {
                    Some(Ok(raw)) => {
                        let (text, keys, error) = self.postprocess(&raw, app.as_deref());
                        if let Some(e) = error {
                            let _ = self.core.send(Msg::Error(e));
                        }
                        FinalMsg { text, raw, keys, app, cancelled: false, audio_ms, finalize_ms: t.elapsed().as_millis() as u64 }
                    }
                    Some(Err(e)) => {
                        let _ = self.core.send(Msg::Error(format!("transcription failed: {e:#}")));
                        FinalMsg::cancelled(audio_ms, t.elapsed().as_millis() as u64)
                    }
                    None => FinalMsg::cancelled(audio_ms, 0),
                };
                self.reset_counters();
                let _ = self.core.send(Msg::Final(msg));
            }
            Work::Cancel => {
                let audio_ms = self.audio_ms();
                if let Some(s) = self.session.as_mut() {
                    s.reset();
                }
                self.reset_counters();
                let _ = self.core.send(Msg::Final(FinalMsg::cancelled(audio_ms, 0)));
            }
        }
        false
    }

    /// Nachbearbeitung zwischen `Streamer::finish` und dem `final`-Event (`crate::pipeline`). Läuft im
    /// Engine-Thread, der Kern bleibt währenddessen im Zustand `finalizing` und frei für Befehle. Liefert den
    /// fertigen Text (inklusive Leerzeichen am Ende) und die Tastennamen (X-Keysym-Namen), die danach gedrückt werden.
    /// `app` ist die `app_id` aus `start`. Den Verlauf schreibt der Kern.
    fn postprocess(&mut self, raw: &str, app: Option<&str>) -> (String, Vec<String>, Option<String>) {
        let last = self.last_typed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let out = crate::pipeline::process(&self.cfg, raw, app, last.as_ref(), Some(&self.ai_cancel), self.log_text);
        (out.text, out.keys, out.error)
    }

    fn reset_counters(&mut self) {
        self.samples = 0;
        self.last = None;
        self.tick_failed = None;
        self.tick_fails = 0;
        self.failed_sent = false;
        self.gen = None;
        self.app = None;
        self.log_text = false;
    }

    /// Meldet dem Kern einmal je Sitzung, dass die Aufnahme beendet werden muss.
    fn fail_engine(&mut self, msg: String) {
        if !self.failed_sent {
            self.failed_sent = true;
            let _ = self.core.send(Msg::EngineFailed(msg));
        }
    }
}

/// So viele fehlgeschlagene Durchläufe in Folge beenden die Aufnahme mit einem Fehler.
const MAX_TICK_FAILS: u32 = 3;

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into())
}

fn worker_main(rx: Receiver<Work>, core: Sender<Msg>, backend: Arc<dyn Backend>, cfg: Config, last_typed: LastTyped, ai_cancel: CancelFlag) {
    let mut w = Worker {
        session: None,
        samples: 0,
        last: None,
        tick_failed: None,
        tick_fails: 0,
        failed_sent: false,
        gen: None,
        app: None,
        log_text: false,
        core,
        backend,
        cfg,
        last_typed,
        ai_cancel,
    };
    let mut next: Option<Work> = None;
    loop {
        let job = match next.take() {
            Some(j) => j,
            None => match rx.recv() {
                Ok(j) => j,
                Err(_) => break,
            },
        };
        let ends_session = matches!(job, Work::Finish | Work::Cancel);
        let r = catch_unwind(AssertUnwindSafe(|| w.handle(job, &rx, &mut next)));
        match r {
            Ok(true) => break,
            Ok(false) => {}
            Err(p) => {
                // Session verwerfen; ein wartender Finish/Cancel bekommt trotzdem sein final-Event
                w.session = None;
                let audio_ms = w.audio_ms();
                w.reset_counters();
                let _ = w.core.send(Msg::Panicked(format!("engine thread panicked: {}", panic_text(p.as_ref()))));
                if ends_session {
                    let _ = w.core.send(Msg::Final(FinalMsg::cancelled(audio_ms, 0)));
                }
            }
        }
    }
    // Drop von `w.session` gibt das Modell frei
}

// ---------------------------------------------------------------------------------------------------------------
// Kern

#[derive(Clone, Copy, PartialEq, Eq)]
enum Model {
    Unloaded,
    Loading,
    Ready,
}

/// Messaufnahme für `meter`: Mikrofon nur für den Pegel.
struct Meter {
    /// Subscriber-Verbindung, die `meter` angefordert hat; schließt sie, endet die Messung
    owner: Option<u64>,
    expires: Instant,
    rec: Option<Box<dyn Send>>,
}

/// So lange (ms) läuft `meter` höchstens ohne erneutes `on`. Nur für Tests veränderbar.
pub static METER_TIMEOUT_MS: AtomicU64 = AtomicU64::new(60_000);

struct Core {
    /// Laufende Konfiguration (so, wie sie Modell und Engine-Thread gerade verwenden)
    cfg: Config,
    /// Während einer Sitzung eingegangene Konfiguration; gilt ab dem Ende der Sitzung
    pending: Option<Config>,
    config_path: Option<PathBuf>,
    /// Zuletzt gemeldete Overlay-Einstellungen
    overlay: OverlayCfg,
    /// Zuletzt gemeldete Terminal-Klassen (`actions.terminal_classes`, Event `terminals`)
    terminals: Vec<String>,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    work: Sender<Work>,
    worker: Option<std::thread::JoinHandle<()>>,
    audio: Arc<dyn AudioSource>,
    subs: Vec<(u64, SyncSender<Arc<str>>)>,
    model: Model,
    /// Anzahl der an den Engine-Thread geschickten, noch nicht gemeldeten `Load`-Aufträge
    loads_inflight: u32,
    recording: Option<Box<dyn Send>>,
    rec_gen: u64,
    finalizing: bool,
    cancel_req: bool,
    pending_start: bool,
    /// App-Name für die nächste Sitzung (bei `start` während `finalizing`)
    pending_app: Option<String>,
    /// Die laufende Aufnahme gehört einem Fenster: ihr final kommt in den Verlauf und gilt als getippt. Aus für eine
    /// Aufnahme der Befehlspalette (`Request::PaletteVoice`): kein Eingabetext der Palette wird gespeichert.
    rec_window: bool,
    /// Dasselbe für die aufgeschobene nächste Sitzung (`pending_start`)
    pending_window: bool,
    /// Ziel des gerade bearbeiteten `start`/`toggle` (nur während [`Core::command`] aus `PaletteVoice` falsch)
    next_window: bool,
    error: Option<String>,
    last_state: State,
    idle_since: Instant,
    started: Option<Instant>,
    meter: Option<Meter>,
    meter_gen: u64,
    history: History,
    last_typed: LastTyped,
    ai_cancel: CancelFlag,
    /// Befehlspalette: der Kern gibt nur die Einstellungen weiter
    palette: Palette,
}

use crate::debug_text;

impl Core {
    fn state(&self) -> State {
        if self.finalizing {
            State::Finalizing
        } else if self.error.is_some() {
            State::Error
        } else if self.model == Model::Loading {
            State::Loading
        } else if self.recording.is_some() {
            State::Listening
        } else {
            State::Idle
        }
    }

    fn emit(&mut self, ev: &Event) {
        let line: Arc<str> = format!("{}\n", ev.to_json()).into();
        // Langsame oder tote Subscriber fliegen raus, der Kern blockiert nie
        self.subs.retain(|(id, s)| match s.try_send(line.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                warn!("dropping slow subscriber {id}");
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        });
    }

    fn publish_state(&mut self) {
        let s = self.state();
        if s != self.last_state {
            self.last_state = s;
            self.emit(&Event::State { state: s });
        }
    }

    /// Läuft gerade eine Sitzung (Zuhören oder Finalisieren)? Dann wird eine neue Konfiguration aufgeschoben.
    fn busy(&self) -> bool {
        self.recording.is_some() || self.finalizing
    }

    /// Nächste Frist für Aufnahmelimit bzw. Entladen des Modells.
    fn main_deadline(&self) -> Option<Instant> {
        if self.finalizing {
            return None;
        }
        if self.recording.is_some() {
            let started = self.started?;
            return (self.cfg.max_listen_s > 0).then(|| started + Duration::from_secs(self.cfg.max_listen_s));
        }
        if self.cfg.idle_unload_s == 0 || self.model != Model::Ready {
            return None;
        }
        Some(self.idle_since + Duration::from_secs(self.cfg.idle_unload_s))
    }

    /// Nächste Frist des Kerns: die der Sitzung bzw. des Leerlaufs oder das Ende der Pegelmessung.
    fn deadline(&self) -> Option<Instant> {
        match (self.main_deadline(), self.meter.as_ref().map(|m| m.expires)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn run(&mut self) {
        loop {
            let msg = match self.deadline() {
                Some(d) => match self.rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                    Ok(m) => m,
                    Err(RecvTimeoutError::Timeout) => {
                        let now = Instant::now();
                        if self.meter.as_ref().is_some_and(|m| m.expires <= now) {
                            debug!("meter timed out");
                            self.meter_off();
                        }
                        if self.main_deadline().is_some_and(|d| d <= now) {
                            if self.recording.is_some() {
                                // Aufnahmelimit erreicht: wie `stop`, der Text wird finalisiert und gesendet
                                info!("listening for {} s, stopping automatically", self.cfg.max_listen_s);
                                self.end_listening(false);
                            } else {
                                info!("idle for {} s, unloading model", self.cfg.idle_unload_s);
                                let _ = self.work.send(Work::Unload);
                                self.model = Model::Unloaded;
                            }
                        }
                        self.publish_state();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match self.rx.recv() {
                    Ok(m) => m,
                    Err(_) => break,
                },
            };
            if self.handle(msg) {
                break;
            }
            self.after_message();
            self.publish_state();
        }
        self.shutdown();
    }

    /// Nach jeder Nachricht: aufgeschobene Konfiguration übernehmen, wenn keine Sitzung mehr läuft, und die
    /// Messaufnahme dem Zustand anpassen.
    fn after_message(&mut self) {
        if !self.busy() {
            if let Some(cfg) = self.pending.take() {
                self.apply(cfg);
            }
        }
        self.sync_meter();
    }

    fn shutdown(&mut self) {
        self.meter = None;
        self.recording = None;
        let _ = self.work.send(Work::Quit);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
        self.subs.clear();
    }

    /// Schickt einen `Load`-Auftrag an den Engine-Thread.
    fn request_load(&mut self) {
        let _ = self.work.send(Work::Load);
        self.loads_inflight += 1;
        self.model = Model::Loading;
    }

    /// `window`: die Aufnahme gehört einem Fenster (sonst der Befehlspalette, deren Text nie ins Log darf)
    fn start_listening(&mut self, app: Option<String>, window: bool) -> Result<(), String> {
        self.error = None;
        if self.model == Model::Unloaded {
            self.request_load();
        }
        self.rec_gen += 1;
        let gen = self.rec_gen;
        let tx = self.tx.clone();
        let work = self.work.clone();
        let mut meter = LevelMeter::default();
        let _ = self.work.send(Work::Begin(gen, app, debug_text() && window));
        let sink = Box::new(move |c: Capture| match c {
            Capture::Block(b) => {
                if let Some(l) = meter.push(&b) {
                    let _ = tx.send(Msg::Level(gen, l));
                }
                let _ = work.send(Work::Audio(gen, b));
            }
            Capture::Opened(d) => {
                let _ = tx.send(Msg::AudioOpened(gen, d));
            }
            Capture::Failed(e) => {
                let _ = tx.send(Msg::AudioFailed(gen, e));
            }
        });
        self.started = Some(Instant::now());
        // Erst die Diktat-Aufnahme öffnen, dann erst die Messaufnahme schließen (`sync_meter`): die Messung darf
        // den Start nie verzögern oder stören. Während des Zuhörens kommen die Pegel aus der Aufnahme.
        self.recording = Some(self.audio.open(&self.cfg.source, sink));
        self.sync_meter();
        Ok(())
    }

    /// Beendet die Aufnahme und schickt `Finish` oder `Cancel` an den Engine-Thread.
    fn end_listening(&mut self, cancel: bool) {
        // Drop wartet höchstens 500 ms auf den Audio-Thread; danach ist er abgekoppelt, und seine Blöcke
        // (alte Generation) verwirft der Engine-Thread
        self.recording = None;
        self.finalizing = true;
        self.cancel_req = cancel;
        self.ai_cancel.store(false, Ordering::Relaxed);
        let _ = self.work.send(if cancel { Work::Cancel } else { Work::Finish });
    }

    fn command(&mut self, cmd: Cmd, app: Option<String>) -> Reply {
        match cmd {
            Cmd::Status => Reply::ok(self.state()),
            Cmd::Shutdown => Reply::ok(self.state()),
            Cmd::Toggle => {
                if self.recording.is_some() {
                    self.command(Cmd::Stop, None)
                } else {
                    self.command(Cmd::Start, app)
                }
            }
            Cmd::Start => {
                if self.finalizing {
                    // Nach dem final-Event nahtlos neu starten
                    self.pending_start = true;
                    self.pending_app = app;
                    self.pending_window = self.next_window;
                } else if self.recording.is_none() {
                    if let Err(e) = self.start_listening(app, self.next_window) {
                        return Reply::err(e);
                    }
                    self.rec_window = self.next_window;
                }
                Reply::ok(self.state_after_start())
            }
            Cmd::Stop | Cmd::Cancel => {
                let cancel = cmd == Cmd::Cancel;
                self.pending_start = false;
                if self.recording.is_some() {
                    self.end_listening(cancel);
                } else if self.finalizing && cancel {
                    self.cancel_req = true;
                    // läuft gerade ein KI-Aufruf, muss der Engine-Thread nicht bis zum Timeout warten
                    self.ai_cancel.store(true, Ordering::Relaxed);
                }
                Reply::ok(self.state_after_start())
            }
        }
    }

    fn state_after_start(&self) -> State {
        self.state()
    }

    // --- Konfiguration -------------------------------------------------------------------------------------

    /// Die Konfiguration, die der User zuletzt gespeichert hat (auch wenn sie wegen einer Sitzung noch wartet).
    fn desired(&self) -> &Config {
        self.pending.as_ref().unwrap_or(&self.cfg)
    }

    fn get_config(&self) -> Reply {
        Reply::data(json!({
            "config": self.desired().to_json(),
            "path": self.config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            "schema": SCHEMA,
        }))
    }

    fn config_error(e: &anyhow::Error) -> Reply {
        match e.downcast_ref::<crate::config::ConfigError>() {
            Some(ce) => Reply::field_err(ce.to_string(), ce.field.clone()),
            None => Reply::err(format!("{e:#}")),
        }
    }

    fn set_config(&mut self, patch: Value) -> Reply {
        // Schlüssel der obersten Ebene ersetzen die bisherigen ganz, fehlende bleiben wie sie sind
        let mut base = self.desired().to_json();
        let (Some(obj), Value::Object(p)) = (base.as_object_mut(), patch) else {
            return Reply::err("config must be an object");
        };
        // Alter Alias `trailing_space` in der Anfrage gilt nur, wenn sie `typing` nicht selbst mitbringt
        let legacy = (!p.contains_key("typing")).then(|| p.get("trailing_space").cloned()).flatten();
        for (k, v) in p {
            if k != "trailing_space" {
                obj.insert(k, v);
            }
        }
        if let (Some(v), Some(Value::Object(t))) = (legacy, obj.get_mut("typing")) {
            t.insert("trailing_space".into(), v);
        }
        let new = match Config::from_value(base).and_then(|c| c.validate_new().map(|()| c)) {
            Ok(c) => c,
            Err(e) => return Reply::field_err(e.to_string(), e.field),
        };
        if let Some(path) = &self.config_path {
            if let Err(e) = new.save(path) {
                return Reply::err(format!("cannot write {}: {e:#}", path.display()));
            }
        }
        self.install(new)
    }

    fn reload(&mut self) -> Reply {
        let Some(path) = self.config_path.clone() else {
            return Reply::err("the daemon runs without a config file");
        };
        let loaded = if path.exists() { Config::load(Some(&path)) } else { Ok(Config::default()) };
        match loaded {
            Ok(c) => self.install(c),
            Err(e) => Core::config_error(&e),
        }
    }

    /// Macht `new` zur Konfiguration: sofort, wenn keine Sitzung läuft, sonst nach ihrem Ende. Die Overlay-
    /// Einstellungen gehen in jedem Fall sofort an die Subscriber.
    fn install(&mut self, new: Config) -> Reply {
        let kind = reload_kind(&self.cfg, &new);
        self.publish_overlay(&new.overlay);
        self.publish_terminals(&new.actions.terminal_classes);
        // wie das Overlay sofort: die Palette hat nichts mit einer laufenden Aufnahme zu tun
        self.palette.configure(&new.palette);
        self.palette.set_diagnose_enabled(new.diagnose.enabled);
        let deferred = self.busy();
        if deferred {
            self.pending = Some(new);
        } else {
            self.pending = None;
            self.apply(new);
        }
        Reply::data(json!({"reload": kind.as_str(), "deferred": deferred}))
    }

    /// Meldet geänderte Terminal-Klassen an alle Subscriber (sofort, wie das Overlay: sie gelten beim Tippen).
    fn publish_terminals(&mut self, classes: &[String]) {
        if classes != self.terminals.as_slice() {
            self.terminals = classes.to_vec();
            let ev = Event::Terminals { classes: self.terminals.clone() };
            self.emit(&ev);
        }
    }

    /// Meldet geänderte Overlay-Einstellungen an alle Subscriber.
    fn publish_overlay(&mut self, o: &OverlayCfg) {
        if *o != self.overlay {
            self.overlay = o.clone();
            let ev = Event::Overlay { overlay: o.clone() };
            self.emit(&ev);
        }
    }

    /// Übernimmt `new` in den laufenden Betrieb (nur ohne Sitzung aufrufen).
    fn apply(&mut self, new: Config) {
        if new == self.cfg {
            return;
        }
        let kind = reload_kind(&self.cfg, &new);
        let source_changed = new.source != self.cfg.source;
        info!("config changed ({})", kind.as_str());
        self.cfg = new.clone();
        let _ = self.work.send(Work::Config(Box::new(new)));
        if kind == ReloadKind::Model && self.model != Model::Unloaded {
            // Altes Modell zuerst freigeben (VRAM), dann im Hintergrund neu laden
            let _ = self.work.send(Work::Unload);
            self.request_load();
        }
        if source_changed {
            // Messaufnahme auf die neue Quelle umstellen
            if let Some(m) = self.meter.as_mut() {
                m.rec = None;
            }
        }
    }

    // --- Pegelmessung ----------------------------------------------------------------------------------------

    fn meter_request(&mut self, on: bool, owner: Option<u64>) -> Reply {
        if on {
            let expires = Instant::now() + Duration::from_millis(METER_TIMEOUT_MS.load(Ordering::Relaxed));
            match self.meter.as_mut() {
                Some(m) => {
                    m.expires = expires;
                    m.owner = owner;
                }
                None => self.meter = Some(Meter { owner, expires, rec: None }),
            }
            self.sync_meter();
        } else {
            self.meter_off();
        }
        Reply::ok(self.state())
    }

    fn meter_off(&mut self) {
        self.meter = None;
    }

    /// Das Mikrofon für die Messung ist offen, wenn `meter` an ist und keine Sitzung läuft.
    fn sync_meter(&mut self) {
        let want = !self.busy();
        let Some(m) = self.meter.as_mut() else { return };
        if !want {
            m.rec = None;
            return;
        }
        if m.rec.is_some() {
            return;
        }
        self.meter_gen += 1;
        let gen = self.meter_gen;
        let tx = self.tx.clone();
        let mut lm = LevelMeter::default();
        let sink = Box::new(move |c: Capture| match c {
            Capture::Block(b) => {
                if let Some(l) = lm.push(&b) {
                    let _ = tx.send(Msg::MeterLevel(gen, l));
                }
            }
            Capture::Failed(e) => {
                let _ = tx.send(Msg::MeterFailed(gen, e));
            }
            Capture::Opened(_) => {}
        });
        m.rec = Some(self.audio.open(&self.cfg.source, sink));
    }

    // --- Verlauf ----------------------------------------------------------------------------------------

    /// Nach einem getippten Diktat: Länge für „rückgängig“ merken und (bei `features.history`) den Verlauf
    /// schreiben.
    fn record_final(&mut self, text: &str, raw: &str, keys: &[String], app: Option<&str>) {
        {
            let mut last = self.last_typed.lock().unwrap_or_else(|e| e.into_inner());
            if !keys.is_empty() {
                // „rückgängig“ hat das vorige Diktat gelöscht; ein zweites Mal geht nicht
                *last = None;
            } else if !text.is_empty() {
                *last = Some(Typed::new(text, app));
            }
        }
        let typed = text.trim_end_matches(' ');
        if self.cfg.features.history && !typed.trim().is_empty() {
            if let Err(e) = self.history.append(app, raw, typed, self.cfg.history.max_entries) {
                let msg = format!("cannot write history: {e:#}");
                warn!("{msg}");
                self.emit(&Event::Error { msg });
            }
        }
    }

    fn history_clear(&mut self) -> Reply {
        match self.history.clear() {
            Ok(()) => Reply::data(json!({})),
            Err(e) => Reply::err(format!("{e:#}")),
        }
    }

    fn request(&mut self, req: Request, owner: Option<u64>) -> Reply {
        match req {
            Request::Cmd(c) => self.command(c, None),
            Request::Start { app } => self.command(Cmd::Start, Some(app)),
            Request::Toggle { app } => self.command(Cmd::Toggle, Some(app)),
            Request::PaletteVoice { toggle, app } => {
                self.next_window = false;
                let r = self.command(if toggle { Cmd::Toggle } else { Cmd::Start }, app);
                self.next_window = true;
                r
            }
            Request::GetConfig => self.get_config(),
            Request::SetConfig(v) => self.set_config(v),
            Request::Reload => self.reload(),
            Request::History { limit } => {
                let entries = if self.cfg.features.history { self.history.entries(limit) } else { Vec::new() };
                Reply::data(json!({"entries": entries}))
            }
            Request::HistoryClear => self.history_clear(),
            Request::Meter { on } => self.meter_request(on, owner),
            // Diese Anfragen beantwortet die Verbindung selbst, ohne den Kern
            Request::Subscribe
            | Request::ListSources
            | Request::ListModels
            | Request::AiModels
            | Request::AiTest { .. }
            | Request::Commands
            | Request::ActGrab { .. }
            | Request::ActRun { .. }
            | Request::ActCancel { .. }
            | Request::ActResult { .. }
            | Request::ActClear
            | Request::PalResolve { .. }
            | Request::PalAsk { .. }
            | Request::PalChosen { .. }
            | Request::PalSourcesRefresh
            | Request::PalFrecencyClear
            | Request::DiagPlan { .. }
            | Request::DiagUnits
            | Request::DiagCollect { .. }
            | Request::DiagPreview { .. }
            | Request::DiagCheck { .. }
            | Request::DiagSend { .. }
            | Request::DiagCopy { .. }
            | Request::DiagCancel { .. }
            | Request::DiagClear
            | Request::DiagBootHint => Reply::err("internal: wrong handler"),
        }
    }

    /// `true` = Kern beenden
    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Quit => return true,
            Msg::AiCfg(tx) => {
                let _ = tx.send(self.desired().ai.clone());
            }
            Msg::ActCfg(tx) => {
                let _ = tx.send(Box::new(self.desired().clone()));
            }
            Msg::Req(req, owner, reply) => {
                let shutdown = req == Request::Cmd(Cmd::Shutdown);
                let r = self.request(req, owner);
                let _ = reply.send(r);
                if shutdown {
                    return true;
                }
            }
            Msg::Subscribe(id, sub) => {
                let state: Arc<str> = format!("{}\n", Event::State { state: self.state() }.to_json()).into();
                let overlay: Arc<str> = format!("{}\n", Event::Overlay { overlay: self.overlay.clone() }.to_json()).into();
                let terminals: Arc<str> = format!("{}\n", Event::Terminals { classes: self.terminals.clone() }.to_json()).into();
                if sub.try_send(state).is_ok() && sub.try_send(overlay).is_ok() && sub.try_send(terminals).is_ok() {
                    self.subs.push((id, sub));
                }
            }
            Msg::Unsub(id) => {
                self.subs.retain(|(i, _)| *i != id);
                if self.meter.as_ref().is_some_and(|m| m.owner == Some(id)) {
                    self.meter_off();
                }
            }
            Msg::Level(gen, l) => {
                if gen == self.rec_gen && self.state() == State::Listening {
                    self.emit(&Event::Level { db: l.db, speech: l.speech });
                }
            }
            Msg::MeterLevel(gen, l) => {
                if gen == self.meter_gen && !self.busy() && self.meter.is_some() {
                    self.emit(&Event::Level { db: l.db, speech: l.speech });
                }
            }
            Msg::MeterFailed(gen, e) => {
                if gen == self.meter_gen && self.meter.is_some() {
                    self.meter_off();
                    self.emit(&Event::Error { msg: e });
                }
            }
            Msg::AudioOpened(gen, d) => {
                if gen == self.rec_gen {
                    let since = self.started.map(|t| t.elapsed().as_millis()).unwrap_or(0);
                    debug!("audio open: first block after {} ms ({} ms since start)", d.as_millis(), since);
                }
            }
            Msg::AudioFailed(gen, e) => {
                if gen == self.rec_gen && self.recording.is_some() {
                    self.recording = None;
                    self.error = Some(e.clone());
                    // Sitzung verwerfen, ohne ein final zu verlangen
                    let _ = self.work.send(Work::Cancel);
                    self.finalizing = true;
                    self.cancel_req = true;
                    self.emit(&Event::Error { msg: e });
                }
            }
            Msg::Loaded(Ok(())) => {
                self.loads_inflight = self.loads_inflight.saturating_sub(1);
                if self.loads_inflight == 0 {
                    self.model = Model::Ready;
                    self.idle_since = Instant::now();
                }
            }
            Msg::Loaded(Err(e)) => {
                warn!("{e}");
                self.loads_inflight = self.loads_inflight.saturating_sub(1);
                self.model = Model::Unloaded;
                self.recording = None;
                self.error = Some(e.clone());
                self.emit(&Event::Error { msg: e });
            }
            Msg::Partial(p) => {
                if self.recording.is_some() && !self.finalizing {
                    // Eingabe der Befehlspalette nie, auch nicht mit DICTATE_DEBUG (W5 Durchsicht N1)
                    if debug_text() && self.rec_window {
                        debug!("partial: {:?} | {:?}", p.stable, p.tail);
                    }
                    self.emit(&Event::Partial { stable: p.stable, tail: p.tail });
                }
            }
            Msg::Final(f) => {
                self.finalizing = false;
                let cancelled = f.cancelled || self.cancel_req;
                self.cancel_req = false;
                let (text, raw, keys) = if cancelled { (String::new(), String::new(), Vec::new()) } else { (f.text, f.raw, f.keys) };
                info!("final: {} chars, audio {} ms, finalize {} ms, cancelled={}", text.chars().count(), f.audio_ms, f.finalize_ms, cancelled);
                if debug_text() && self.rec_window {
                    debug!("final text: {text:?} (raw {raw:?}, keys {keys:?})");
                }
                if !cancelled && self.rec_window {
                    self.record_final(&text, &raw, &keys, f.app.as_deref());
                } else if !cancelled {
                    info!("final belongs to the command palette: not in the history, not counted as typed");
                }
                self.emit(&Event::Final { text, cancelled, audio_ms: f.audio_ms, finalize_ms: f.finalize_ms, raw, keys });
                self.idle_since = Instant::now();
                // Aufgeschobene Konfiguration gilt, bevor eine nahtlos folgende Sitzung startet
                if let Some(cfg) = self.pending.take() {
                    self.apply(cfg);
                }
                if std::mem::take(&mut self.pending_start) {
                    let app = self.pending_app.take();
                    if self.start_listening(app, self.pending_window).is_ok() {
                        self.rec_window = self.pending_window;
                    }
                }
            }
            Msg::Error(e) => {
                warn!("{e}");
                self.emit(&Event::Error { msg: e });
            }
            Msg::EngineFailed(e) => {
                if self.recording.is_some() {
                    self.recording = None;
                    self.error = Some(e.clone());
                    self.pending_start = false;
                    warn!("{e}");
                    let _ = self.work.send(Work::Cancel);
                    self.finalizing = true;
                    self.cancel_req = true;
                    self.emit(&Event::Error { msg: e });
                }
            }
            Msg::Panicked(e) => {
                warn!("{e}");
                self.model = Model::Unloaded;
                self.loads_inflight = 0;
                self.recording = None;
                self.error = Some(e.clone());
                self.emit(&Event::Error { msg: e });
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Socket

fn ensure_dir(path: &Path) -> Result<()> {
    let Some(dir) = path.parent() else { return Ok(()) };
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    if !dir.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).with_context(|| format!("creating {}", dir.display()))?;
    } else if dir.file_name().is_some_and(|n| n == "dictate") {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_line(s: &mut UnixStream, line: &str) -> std::io::Result<()> {
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()
}

/// Längste Anfragezeile (außer `set_config`)
const MAX_LINE: usize = 64 * 1024;
/// Längste `set_config`-Zeile (Vokabular und Modi können groß werden)
const MAX_SET_CONFIG_LINE: usize = 1024 * 1024;
/// Längste `act_grab`-Zeile: `actions.max_chars` (höchstens 30000 Zeichen) mit bis zu 4 Byte je Zeichen und
/// JSON-Maskierung; die inhaltliche Grenze prüft der Befehl selbst
const MAX_ACT_GRAB_LINE: usize = 256 * 1024;
/// Längste `pal_resolve`- und `pal_ask`-Zeile: Eingabe und Kontext bis 256 KiB (so viel nahm der frühere Client der
/// Palette an; W5 übernimmt die Grenze), dazu die Umgebung der Sitzung (vier Werte, je höchstens
/// `palette::service::MAX_ENV_BYTES`) und die JSON-Maskierung
/// (`ui::pal_context::CONTEXT_BUDGET` hält das ein; Test `the_worst_line_fits_the_daemon` in `palette::ui_mirror`)
pub(crate) const MAX_PAL_RESOLVE_LINE: usize = 512 * 1024;

enum Line {
    Eof,
    Text(String),
    /// Länger als erlaubt: der Rest der Zeile ist ungelesen, die Verbindung muss schließen
    TooLarge,
}

/// Liest eine Zeile mit Längenbegrenzung ([`MAX_SET_CONFIG_LINE`]; ob mehr als [`MAX_LINE`] zulässig ist, entscheidet
/// [`check_size`] nach dem Parsen).
fn read_limited_line(r: &mut BufReader<UnixStream>) -> std::io::Result<Line> {
    let mut buf = String::new();
    let n = r.by_ref().take(MAX_SET_CONFIG_LINE as u64 + 1).read_line(&mut buf)?;
    if n == 0 {
        return Ok(Line::Eof);
    }
    if !buf.ends_with('\n') && n > MAX_SET_CONFIG_LINE {
        act_store::wipe(&mut buf);
        return Ok(Line::TooLarge);
    }
    let t = buf.trim();
    if t.len() == buf.len() {
        return Ok(Line::Text(buf));
    }
    // die Zeile kann eine Markierung sein (`act_grab`): den ungekürzten Puffer nicht unüberschrieben freigeben
    let mut out = String::with_capacity(t.len());
    out.push_str(t);
    act_store::wipe(&mut buf);
    Ok(Line::Text(out))
}

/// Zeilen über [`MAX_LINE`] sind nur für `set_config`, `act_grab` (bis [`MAX_ACT_GRAB_LINE`]), `pal_resolve` und
/// `pal_ask` (bis [`MAX_PAL_RESOLVE_LINE`]) zulässig.
fn check_size(line: &str, req: &Request) -> bool {
    line.len() <= MAX_LINE
        || matches!(req, Request::SetConfig(_))
        || (matches!(req, Request::ActGrab { .. }) && line.len() <= MAX_ACT_GRAB_LINE)
        || (matches!(req, Request::PalResolve { .. } | Request::PalAsk { .. }) && line.len() <= MAX_PAL_RESOLVE_LINE)
}

fn too_large() -> String {
    Reply::err("request too large").to_line()
}

/// Führt eine Anfrage aus. Was keinen Kernzustand braucht (Quellen, Modelle), beantwortet die Verbindung selbst,
/// damit der Kern nie auf Pulse oder die Platte wartet.
/// `lines` öffnet bei Bedarf einen Schreiber auf dieselbe Verbindung für Zwischenzeilen vor der Antwort (`act_run`),
/// `peer` eine Kopie der Verbindung, an der `pal_ask` bemerkt, dass der Client aufgelegt hat.
#[allow(clippy::too_many_arguments)]
fn execute(
    req: Request,
    owner: Option<u64>,
    core: &Sender<Msg>,
    audio: &dyn AudioSource,
    acts: &ActStore,
    palette: &Palette,
    diag: &Diagnose,
    lines: &dyn Fn() -> Option<LineOut>,
    peer: &dyn Fn() -> Option<UnixStream>,
) -> Reply {
    match req {
        Request::ListSources => match audio.list_sources() {
            Ok(v) => Reply::data(json!({"sources": v})),
            Err(e) => Reply::err(e),
        },
        Request::ListModels => Reply::data(crate::models::list_json()),
        Request::Commands => Reply::data(json!({"commands": crate::pipeline::commands_table()})),
        // Der HTTP-Aufruf läuft auf dieser Verbindung; der Kern liefert nur kurz die Einstellungen
        Request::AiModels => match ai_cfg(core) {
            Err(r) => r,
            Ok(cfg) => match crate::pipeline::ai::list_models(&cfg) {
                Ok((list, failure)) => {
                    let models: Vec<Value> = list.into_iter().map(|n| json!({"name": n})).collect();
                    let mut v = json!({"models": models});
                    if let Some(e) = failure {
                        v["note"] = json!(format!("static list ({e})"));
                    }
                    Reply::data(v)
                }
                Err(e) => Reply::err(e.to_string()),
            },
        },
        Request::AiTest { mode, text } => match ai_cfg(core) {
            Err(r) => r,
            Ok(cfg) => match crate::pipeline::ai::rewrite(&cfg, &mode, &text) {
                Ok(t) => Reply::data(json!({"text": t})),
                Err(e) => Reply::err(e.to_string()),
            },
        },
        Request::ActGrab { text, app, source } => act_grab(text, app, source, core, acts),
        Request::ActRun { id, action } => act_run(&id, &action, core, acts, lines),
        Request::ActCancel { id } => {
            if let Some(id) = parse_act_id(&id) {
                if acts.cancel(id) {
                    info!("act cancel");
                }
            }
            // idempotent: ohne laufenden Aufruf (schon fertig, ersetzt, abgelaufen) gibt es nichts abzubrechen
            Reply::data(json!({}))
        }
        Request::ActResult { id } => match parse_act_id(&id).ok_or(ResultError::Unknown).and_then(|id| acts.result(id)) {
            Ok(r) => Reply::data(json!({"text": r.text.as_str(), "output": r.output.as_str(), "truncated": r.truncated, "action": r.action})),
            Err(ResultError::Unknown) => Reply::err(UNKNOWN_SELECTION),
            Err(ResultError::NotReady) => Reply::err("No result yet"),
        },
        Request::ActClear => {
            acts.clear();
            Reply::data(json!({}))
        }
        Request::PalAsk { input, context, env } => pal_ask(input, context, env, core, palette, peer),
        Request::PalResolve { .. } | Request::PalChosen { .. } | Request::PalSourcesRefresh | Request::PalFrecencyClear => pal_request(req, palette),
        Request::DiagPlan { .. }
        | Request::DiagUnits
        | Request::DiagCollect { .. }
        | Request::DiagPreview { .. }
        | Request::DiagCheck { .. }
        | Request::DiagSend { .. }
        | Request::DiagCopy { .. }
        | Request::DiagCancel { .. }
        | Request::DiagClear
        | Request::DiagBootHint => diag_request(req, core, diag, lines, peer),
        Request::Subscribe => Reply::err("already subscribed"),
        req => {
            let (rtx, rrx) = mpsc::channel();
            if core.send(Msg::Req(req, owner, rtx)).is_err() {
                return Reply::err("daemon is shutting down");
            }
            rrx.recv().unwrap_or_else(|_| Reply::err("daemon is shutting down"))
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Befehlspalette (`pal_*`): laufen auf dem Verbindungs-Thread; Eingabe und Kontext nie ins Log, auch nicht mit
// `DICTATE_DEBUG` (kein `debug!` und kein `{:?}` mit diesen Werten)

fn pal_error(e: PaletteError) -> Reply {
    Reply::err(e.to_string())
}

fn pal_request(req: Request, palette: &Palette) -> Reply {
    match req {
        Request::PalResolve { input, context, env } => match palette.resolve(input.as_str(), context.as_ref().map(|c| c.as_str()), &env) {
            Ok(r) => {
                let mut v = serde_json::to_value(&r.resolution).unwrap_or_else(|_| json!({}));
                r.prefs.add_to(&mut v);
                Reply::data(v)
            }
            Err(e) => pal_error(e),
        },
        Request::PalChosen { intent, target_id } => match palette.chosen(&intent, &target_id) {
            Ok(recorded) => Reply::data(json!({"recorded": recorded})),
            Err(e) => pal_error(e),
        },
        Request::PalSourcesRefresh => palette.refresh().map_or_else(pal_error, |()| Reply::data(json!({}))),
        Request::PalFrecencyClear => palette.clear_history().map_or_else(pal_error, |()| Reply::data(json!({}))),
        _ => Reply::err("internal: wrong handler"),
    }
}

/// `pal_ask`: KI-Pfad der Palette. Erst Kontext und Umgebung prüfen (kein Modellaufruf für eine ungültige Anfrage),
/// dann die Eingabe (allein, ohne Kontext) über `palette::ai` an das Modell, die geprüfte Absicht lokal auflösen und die
/// Kandidaten anmelden (für `pal_chosen`). Läuft auf dem Verbindungs-Thread; der Kern liefert nur die Einstellungen.
/// Legt der Client auf (Panel geschlossen, CLI beendet), bricht [`HangupWatch`] den Aufruf ab. Im Log stehen nur
/// Modell, Name der Absicht, Zahlen und Dauer.
fn pal_ask(
    input: SecretText,
    context: Option<SecretText>,
    env: protocol::SessionEnv,
    core: &Sender<Msg>,
    palette: &Palette,
    peer: &dyn Fn() -> Option<UnixStream>,
) -> Reply {
    let prepared = match palette.prepare(context.as_ref().map(|c| c.as_str()), &env) {
        Ok(p) => p,
        Err(e) => return pal_error(e),
    };
    drop(context);
    let cfg = match act_cfg(core) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    let _watch = peer().and_then(|s| HangupWatch::start(s, cancel.clone()));
    let model = cfg.palette.model_for(&cfg.ai).to_string();
    let started = Instant::now();
    let asked = crate::palette::ai::ask_intent(&cfg.ai, &cfg.palette, input.as_str(), &cancel);
    let ms = started.elapsed().as_millis();
    use crate::palette::ai::AskError;
    // die Eingabe bleibt bis nach dem Auflösen: eine Adresse des Modells braucht ihren Host
    let resolved = asked.map(|intent| {
        let r = palette.resolve_intent(&prepared, &intent, input.as_str());
        (intent, r)
    });
    drop(input);
    match resolved {
        Ok((intent, resolved)) => match resolved {
            Ok(candidates) => {
                info!("pal ask with {model}: {} -> {} candidates in {ms} ms", intent.name(), candidates.len());
                let mut v = json!({"intent": intent, "candidates": candidates});
                prepared.prefs.add_to(&mut v);
                Reply::data(v)
            }
            Err(e) => {
                info!("pal ask with {model}: {} without target in {ms} ms", intent.name());
                Reply::err(e.to_string())
            }
        },
        Err(AskError::Ai(crate::pipeline::ai::AiError::Cancelled)) => {
            info!("pal ask: cancelled after {ms} ms");
            cancelled()
        }
        Err(AskError::Blocked(kinds)) => {
            info!("pal ask: blocked, secrets {kinds:?}");
            Reply::err(AskError::Blocked(kinds.clone()).to_string()).with("secret", json!(kinds))
        }
        Err(e) => {
            let kind = match &e {
                AskError::Ai(_) => "AI error",
                AskError::Invalid(_) => "invalid intent",
                AskError::NoMatch => "no match",
                AskError::Empty | AskError::TooLong(_) => "bad input",
                AskError::Blocked(_) => "blocked",
            };
            info!("pal ask with {model}: {kind} after {ms} ms");
            Reply::err(e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Diagnose (`diag_*`): laufen auf dem Verbindungs-Thread; Berichte, Ausgaben und Parameter nie ins Log (nur Zahlen,
// siehe `diagnose::service`)

/// Führt eine Anfrage der Diagnose aus. Sammeln und Prüfen schreiben Fortschritt über `lines` vor der Antwort und
/// brechen ab, wenn der Client auflegt ([`HangupWatch`]).
fn diag_request(req: Request, core: &Sender<Msg>, diag: &Diagnose, lines: &dyn Fn() -> Option<LineOut>, peer: &dyn Fn() -> Option<UnixStream>) -> Reply {
    match req {
        Request::DiagCancel { id } => return diag.cancel(&id),
        Request::DiagClear => return diag.clear(),
        _ => {}
    }
    let cfg = match act_cfg(core) {
        Ok(c) => c,
        Err(r) => return r,
    };
    // Abbruch: Auflegen des Clients, `diag_cancel`, eine neue Sammlung oder das Ende der Sitzung
    let watched = || {
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let watch = peer().and_then(|s| HangupWatch::start(s, cancel.clone()));
        (cancel, watch)
    };
    let mut out: LineOut = lines().unwrap_or_else(|| Box::new(|_: &str| Ok(())));
    match req {
        Request::DiagPlan { profile, unit, container } => diag.plan(&cfg, profile.as_deref(), unit.as_deref(), container.as_deref()),
        Request::DiagUnits => {
            let (cancel, _watch) = watched();
            diag.units(&cfg, &cancel)
        }
        Request::DiagCollect { profile, unit, container, skip } => {
            let (cancel, _watch) = watched();
            diag.collect(&cfg, &CollectRequest { profile, unit, container, skip }, cancel, &mut out)
        }
        Request::DiagPreview { id, deselected, symptom } => diag.preview(&cfg, &id, deselected.as_deref(), symptom.as_ref().map(SecretText::as_str)),
        Request::DiagCheck { id, checks, network } => {
            let (cancel, _watch) = watched();
            diag.check(&cfg, &id, &checks, network, cancel, &mut out)
        }
        Request::DiagSend { id, preview } => {
            let (cancel, _watch) = watched();
            diag.send(&cfg, &id, preview, cancel)
        }
        Request::DiagCopy { id } => diag.copy(&cfg, &id),
        Request::DiagBootHint => {
            let (cancel, _watch) = watched();
            diag.boot_hint(&cfg, &cancel)
        }
        _ => Reply::err("internal: wrong handler"),
    }
}

/// Bemerkt, dass der Client einer laufenden Anfrage auflegt (Verbindung ganz geschlossen), und setzt dann das
/// Abbruch-Flag. Ein eigener Thread fragt die Verbindung alle 50 ms ab (`poll`, nichts wird gelesen); er endet mit dem
/// Freigeben (`Drop`) oder beim Auflegen. Weitere Daten des Clients (die nächste Anfrage, ein Byte) und ein halbes
/// Schließen (der Client hat das Schreiben beendet und liest noch) beenden ihn nicht: danach läuft er weiter und
/// bemerkt das spätere Auflegen.
struct HangupWatch {
    done: Arc<AtomicBool>,
}

impl HangupWatch {
    fn start(stream: UnixStream, cancel: CancelFlag) -> Option<HangupWatch> {
        let done = Arc::new(AtomicBool::new(false));
        let d = done.clone();
        std::thread::Builder::new().name("dictate-hangup".into()).spawn(move || watch_hangup(&stream, &cancel, &d)).ok()?;
        Some(HangupWatch { done })
    }
}

impl Drop for HangupWatch {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
    }
}

fn watch_hangup(stream: &UnixStream, cancel: &CancelFlag, done: &AtomicBool) {
    use std::os::fd::AsRawFd;
    while !done.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
        // Ohne `events` meldet `poll` nur POLLHUP, POLLERR und POLLNVAL (die immer): lesbare Daten (POLLIN) und das halbe
        // Schließen des Clients (POLLRDHUP, bei AF_UNIX auch POLLIN) wecken `poll` nicht, es gibt keine Warteschleife
        // ohne Pause. Ganz geschlossen setzt AF_UNIX POLLHUP, auch wenn noch ungelesene Daten warten.
        let mut p = libc::pollfd { fd: stream.as_raw_fd(), events: 0, revents: 0 };
        // SAFETY: ein gültiger pollfd, die Verbindung lebt so lange wie dieser Thread
        let n = unsafe { libc::poll(&mut p, 1, 50) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if n == 0 {
            continue;
        }
        if p.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            cancel.store(true, Ordering::Relaxed);
        }
        return;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Textaktionen (`act_*`): laufen auf dem Verbindungs-Thread; Texte nie ins Log, nur Zahlen und Arten

const ACTIONS_OFF: &str = "Text actions are off (Settings › Actions)";
const UNKNOWN_SELECTION: &str = "Unknown or expired selection, select again";

fn act_cfg(core: &Sender<Msg>) -> Result<Box<Config>, Reply> {
    let (tx, rx) = mpsc::channel();
    if core.send(Msg::ActCfg(tx)).is_err() {
        return Err(Reply::err("daemon is shutting down"));
    }
    rx.recv().map_err(|_| Reply::err("daemon is shutting down"))
}

/// Ids sind Dezimalzahlen als String (`"7"`); alles andere ist unbekannt.
fn parse_act_id(id: &str) -> Option<u64> {
    id.trim().parse().ok()
}

/// Ziel der KI wie aufgelöst, mit Host für die Vorschau; Fehler als Antwort.
fn act_target(ai: &crate::config::AiCfg) -> Result<(TargetKey, String), Reply> {
    crate::pipeline::ai::resolve(ai).map_err(|e| Reply::err(format!("AI error: {e}")))?;
    let (protocol, base) = ai.resolved();
    let host = crate::config::parse_endpoint(&base).map(|e| e.host).unwrap_or_default();
    Ok((TargetKey { protocol, base, cloud: ai.is_cloud() }, host))
}

/// Hinweis, wenn die Antwort einer wachsenden Aktion (`chars` Zeichen bei `input` Zeichen Markierung) länger ist als
/// ihr Ziel [`crate::config::refine_target_chars`]; sonst `None`.
fn long_note(input: usize, chars: usize) -> Option<String> {
    (chars > crate::config::refine_target_chars(input)).then(|| {
        let factor = chars as f64 / input.max(1) as f64;
        format!("Longer than aimed for: {factor:.1} times the selection (target at most {} times). Check it before replacing.", crate::config::REFINE_GROWTH)
    })
}

fn too_long(source: act_store::Source, chars: usize, max: usize) -> Reply {
    let what = match source {
        act_store::Source::Selection => "Selection",
        act_store::Source::Clipboard => "Clipboard text",
        act_store::Source::Question => "Question",
    };
    Reply::field_err(format!("{what} too long: {chars} characters, at most {max} (Settings › Actions)"), "actions.max_chars")
}

/// Liste der Arten für Meldungen: „Private key, GitHub token“
fn describe_secrets(kinds: &[&str]) -> String {
    kinds.iter().map(|k| crate::actions::secrets::describe(k).unwrap_or(k)).collect::<Vec<_>>().join(", ")
}

/// Schalter der Herkunft: die Markierung gehört den Textaktionen, Zwischenablage und Frage der Befehlspalette.
fn source_off(cfg: &Config, source: act_store::Source) -> Option<Reply> {
    if source.is_palette() {
        (!cfg.palette.enabled).then(|| Reply::err(crate::palette::service::DISABLED))
    } else {
        (!cfg.features.actions).then(|| Reply::err(ACTIONS_OFF))
    }
}

/// Aktionen einer Sitzung: der Katalog für die Markierung, sonst die eine Aktion der Palette.
fn source_actions(cfg: &Config, source: act_store::Source) -> Vec<crate::actions::Action> {
    match crate::actions::source_action(cfg, source) {
        Some(a) => vec![a],
        None => crate::actions::catalog(cfg),
    }
}

/// Modell einer Aktion: bei der Markierung `actions.model` (KI-Modi ihr eigenes), bei der Palette `palette.model`
/// (leer = Vorgabe des Servers, dieselbe Kette wie beim KI-Pfad der Palette).
fn source_model<'a>(cfg: &'a Config, source: act_store::Source, mode: Option<&str>) -> &'a str {
    if source.is_palette() {
        cfg.palette.model_for(&cfg.ai)
    } else {
        cfg.actions.model_for(mode, &cfg.ai)
    }
}

/// `actions.secrets = "block"` gilt nur für Markierungen; die Palette sperrt bei einem Geheimnis die Cloud und erlaubt
/// lokale Server (wie `pal_ask`, PLAN-palette.md „Geheimnisse“).
fn block_all_secrets(cfg: &Config, source: act_store::Source) -> bool {
    !source.is_palette() && cfg.actions.secrets == "block"
}

/// `act_grab`: Text prüfen und als neue Sitzung speichern (die vorige wird in jedem Fall gelöscht). Für die
/// Zwischenablage ist das die Vorschau: gesendet wird erst mit `act_run`, also nach der Bestätigung im Panel.
fn act_grab(text: SecretText, app: Option<String>, source: act_store::Source, core: &Sender<Msg>, acts: &ActStore) -> Reply {
    acts.clear();
    let cfg = match act_cfg(core) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Some(r) = source_off(&cfg, source) {
        return r;
    }
    if text.as_str().trim().is_empty() {
        return Reply::err(match source {
            act_store::Source::Selection => "Nothing selected",
            act_store::Source::Clipboard => "The clipboard is empty",
            act_store::Source::Question => "Nothing to ask",
        });
    }
    let chars = text.as_str().chars().count();
    if chars > cfg.actions.max_chars {
        return too_long(source, chars, cfg.actions.max_chars);
    }
    let (target, host) = match act_target(&cfg.ai) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let lines = text.as_str().lines().count();
    let secret = crate::actions::secrets::scan(text.as_str());
    let blocked = !secret.is_empty() && (target.cloud || block_all_secrets(&cfg, source));
    let replace_ok = !source.is_palette() && app.as_deref().is_some_and(|a| !cfg.actions.is_terminal(a));
    let cloud = target.cloud;
    // gesperrt: der Text wird gar nicht gespeichert (`text` überschreibt sich beim Freigeben)
    let id = acts.grab(NewSelection { text: (!blocked).then_some(text), chars, target, secret: secret.clone(), source });
    let actions: Vec<Value> = source_actions(&cfg, source)
        .iter()
        .map(|a| {
            json!({
                "key": a.key.to_string(),
                "name": a.name,
                "label": a.label,
                "output": a.output.as_str(),
                "allowed": !blocked,
                "model": source_model(&cfg, source, a.mode.as_deref()),
            })
        })
        .collect();
    info!("act grab ({}): {chars} chars, {lines} lines, cloud={cloud}, secrets {secret:?}, blocked={blocked}, replace_ok={replace_ok}", source.as_str());
    Reply::data(json!({
        "id": id.to_string(),
        "source": source.as_str(),
        "chars": chars,
        "lines": lines,
        "target": {"host": host, "model": source_model(&cfg, source, None), "cloud": cloud},
        "secret": secret,
        "blocked": blocked,
        "replace_ok": replace_ok,
        "actions": actions,
    }))
}

/// Schreibt eine Zeile (ohne `\n`) auf die Verbindung einer Anfrage, vor deren Antwort
type LineOut = Box<dyn FnMut(&str) -> std::io::Result<()> + Send>;

/// Höchstens eine `delta`-Zeile je Intervall (das erste Stück geht sofort)
const DELTA_INTERVAL: Duration = Duration::from_millis(50);
/// Ab so viel gesammeltem Text geht die Zeile auch vor dem Intervall raus
const DELTA_MAX_BYTES: usize = 4096;

/// Was der Schreiber der Zwischenzeilen geschafft hat (fürs Log)
#[derive(Default)]
struct DeltaStats {
    lines: usize,
    first_ms: Option<u128>,
}

/// Schreiber der Zwischenzeilen von `act_run` auf eigenem Thread: bündelt die Stücke von `ai::ask`, damit das Panel
/// nicht mit Tausenden Zeilen geflutet wird. Das erste Stück und jedes nach einer Pause ab [`DELTA_INTERVAL`] geht
/// sofort raus (Zeit bis zum ersten Text), sonst höchstens eine Zeile je Intervall, früher ab [`DELTA_MAX_BYTES`].
/// Endet, wenn der Sender fällt, und schreibt dann den Rest, also immer vor der Antwortzeile. Scheitert das Schreiben
/// (Client weg), wird der KI-Aufruf über `cancel` abgebrochen und der Rest nur noch verworfen.
fn send_deltas(rx: Receiver<String>, mut out: LineOut, cancel: CancelFlag, started: Instant) -> DeltaStats {
    let mut stats = DeltaStats::default();
    let mut pending = String::new();
    let mut last_sent: Option<Instant> = None;
    let mut broken = false;
    let mut flush = |pending: &mut String, stats: &mut DeltaStats, broken: &mut bool| {
        if !*broken {
            let mut line = protocol::delta_line(pending);
            if out(&line).is_ok() {
                stats.lines += 1;
                stats.first_ms.get_or_insert(started.elapsed().as_millis());
            } else {
                *broken = true;
                cancel.store(true, Ordering::Relaxed);
            }
            act_store::wipe(&mut line);
        }
        act_store::wipe(pending);
    };
    loop {
        let next = match last_sent {
            Some(t) if !pending.is_empty() => match rx.recv_timeout((t + DELTA_INTERVAL).saturating_duration_since(Instant::now())) {
                Ok(s) => Some(s),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            _ => match rx.recv() {
                Ok(s) => Some(s),
                Err(_) => break,
            },
        };
        if let Some(mut s) = next {
            pending.push_str(&s);
            act_store::wipe(&mut s);
        }
        let due = last_sent.is_none_or(|t| t.elapsed() >= DELTA_INTERVAL) || pending.len() >= DELTA_MAX_BYTES;
        if due && !pending.is_empty() {
            flush(&mut pending, &mut stats, &mut broken);
            last_sent = Some(Instant::now());
        }
    }
    if !pending.is_empty() {
        flush(&mut pending, &mut stats, &mut broken);
    }
    stats
}

/// Freie Anfrage einer Aktion mit gestreamter Antwort: Stücke gehen über [`send_deltas`] als Zwischenzeilen an den
/// Client. Ohne Schreiber (Verbindung nicht klonbar) oder Thread ohne Streaming.
fn ask_streamed(
    ai: &crate::config::AiCfg,
    model: &str,
    prompt: &str,
    text: &str,
    opts: crate::pipeline::ai::AskOpts,
    cancel: &CancelFlag,
    lines: &dyn Fn() -> Option<LineOut>,
) -> (Result<crate::pipeline::ai::Answer, crate::pipeline::ai::AiError>, DeltaStats) {
    let plain = |opts| (crate::pipeline::ai::ask(ai, Some(model), prompt, text, &opts, Some(cancel)), DeltaStats::default());
    let Some(out) = lines() else { return plain(opts) };
    let (tx, rx) = mpsc::channel::<String>();
    let (c, started) = (cancel.clone(), Instant::now());
    let Ok(writer) = std::thread::Builder::new().name("dictate-act-delta".into()).spawn(move || send_deltas(rx, out, c, started)) else {
        return plain(opts);
    };
    let r = {
        let send = |d: &str| {
            let _ = tx.send(d.to_string());
        };
        crate::pipeline::ai::ask(ai, Some(model), prompt, text, &opts.on_delta(&send), Some(cancel))
    };
    drop(tx);
    (r, writer.join().unwrap_or_default())
}

/// `act_run`: Aktion auf der gespeicherten Markierung. Prüft Schalter, Ziel und Sperre gegen den jetzigen Stand der
/// Einstellungen, ruft die KI (Modi über `rewrite_as`, sonst `ask`) und speichert das Ergebnis für `act_result`.
///
/// Freie Aktionen (`ask`) streamen: vor der Antwort kommen `{"delta":"…"}`-Zeilen über `lines`. KI-Modi nicht: ihre
/// Antwort ist ein Ersatztext, der erst als Ganzes geprüft und bereinigt wird (abgeschnitten = Fehler, Länge gegen die
/// Eingabe, Präambel und umschließende Anführungszeichen erst am Ende erkennbar, Zeilenumbrüche je nach Gesamtzahl
/// eingeebnet). Gestreamte Stücke wären also nicht der Endtext oder würden nachträglich verworfen; das Panel zeigt
/// dort bis zum geprüften Ergebnis die Wartezeit.
fn act_run(id: &str, action: &str, core: &Sender<Msg>, acts: &ActStore, lines: &dyn Fn() -> Option<LineOut>) -> Reply {
    let Some(id) = parse_act_id(id) else {
        return Reply::err(UNKNOWN_SELECTION);
    };
    let cfg = match act_cfg(core) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Some(source) = acts.source(id) else {
        return Reply::err(UNKNOWN_SELECTION);
    };
    if let Some(r) = source_off(&cfg, source) {
        acts.clear();
        return r;
    }
    let Some(act) = source_actions(&cfg, source).into_iter().find(|a| a.name == action) else {
        return Reply::err(format!("Unknown action \"{action}\""));
    };
    let (target, _) = match act_target(&cfg.ai) {
        Ok(t) => t,
        Err(r) => return r,
    };
    // Geheimnisfilter gegen den jetzt gültigen Server: hat sich das Ziel seit dem Greifen geändert, gilt die Prüfung
    // nicht mehr (`TargetChanged`), sonst sperrt ein Geheimnis die Cloud wie beim Greifen
    let ticket = match acts.begin_run(id, &target, block_all_secrets(&cfg, source)) {
        Ok(t) => t,
        Err(RunError::Unknown) => return Reply::err(UNKNOWN_SELECTION),
        Err(RunError::TargetChanged) => return Reply::err("AI server changed, select again"),
        Err(RunError::Blocked(kinds)) => {
            let what = match source {
                act_store::Source::Selection => "the selection contains",
                act_store::Source::Clipboard => "the clipboard contains",
                act_store::Source::Question => "the question contains",
            };
            return Reply::err(format!("Blocked: {what} secrets ({})", describe_secrets(&kinds))).with("secret", json!(kinds));
        }
    };
    if ticket.chars > cfg.actions.max_chars {
        acts.finish_run(id, ticket.seq, None);
        return too_long(source, ticket.chars, cfg.actions.max_chars);
    }
    let model = source_model(&cfg, source, act.mode.as_deref());
    let timeout_ms = cfg.actions.timeout_ms;
    let started = Instant::now();
    let mut stats = DeltaStats::default();
    let r = match &act.mode {
        // KI-Modus: derselbe Prompt und dieselbe Prüfung wie beim Diktat, Zeitgrenze der Textaktionen
        Some(mode) => {
            let mut ai = cfg.ai.clone();
            ai.timeout_ms = timeout_ms;
            crate::pipeline::ai::rewrite_as(&ai, model, mode, ticket.text.as_str(), Some(&ticket.cancel)).map(|t| (t, false))
        }
        None => {
            let opts = crate::pipeline::ai::AskOpts::new(timeout_ms, crate::actions::max_tokens(act.output, act.grows, ticket.chars));
            // wachsende Aktion (Refine prompt): mit dem Längenlimit dieser Markierung am Ende des System-Prompts
            let prompt = act.prompt_for(ticket.chars);
            let (r, s) = ask_streamed(&cfg.ai, model, &prompt, ticket.text.as_str(), opts, &ticket.cancel, lines);
            stats = s;
            r.map(|a| (a.text, a.truncated))
        }
    };
    let ms = started.elapsed().as_millis();
    let streamed = match stats.first_ms {
        Some(first) => format!(", {} delta lines, first after {first} ms", stats.lines),
        None => String::new(),
    };
    match r {
        Ok((text, truncated)) => {
            // Steuerzeichen außer `\n`/`\t` raus: das Ergebnis kann getippt werden
            let text = SecretText::new(crate::pipeline::ai::strip_controls(&text));
            let chars = text.as_str().chars().count();
            let result = ActResult { action: act.name.clone(), output: act.output, text: text.clone(), truncated };
            if !acts.finish_run(id, ticket.seq, Some(result)) {
                // währenddessen abgebrochen, ersetzt oder gelöscht
                info!("act run {}: cancelled after {ms} ms", act.name);
                return cancelled();
            }
            let mut reply = json!({"text": text.as_str(), "output": act.output.as_str(), "truncated": truncated});
            // Wachsende Aktion (Refine prompt) über dem Ziel, aber unter der harten Grenze (`max_tokens`): zeigen und
            // sagen, statt einen brauchbaren, etwas zu langen Prompt als Fehler wegzuwerfen; der User entscheidet
            if let Some(n) = act.grows.then(|| long_note(ticket.chars, chars)).flatten().filter(|_| !truncated) {
                reply["note"] = json!(n);
            }
            // Änderungen zum Markieren im Panel; die Stücke werden direkt nach dem Übertragen ins JSON überschrieben
            let mut marked = String::new();
            if act.output == crate::actions::Output::Replace && act.marks && !truncated {
                if let Some(mut d) = crate::actions::diff::words(ticket.text.as_str(), text.as_str()) {
                    reply["diff"] = d.iter().map(|(op, s)| json!([op.as_str(), s])).collect();
                    marked = format!(", diff with {} pieces", d.len());
                    for (_, s) in &mut d {
                        crate::actions::store::wipe(s);
                    }
                }
            }
            info!(
                "act run {} with {model}: {} -> {chars} chars{} in {ms} ms{streamed}{marked}",
                act.name,
                ticket.chars,
                if truncated { " (truncated)" } else { "" }
            );
            Reply::data(reply)
        }
        Err(crate::pipeline::ai::AiError::Cancelled) => {
            acts.finish_run(id, ticket.seq, None);
            info!("act run {}: cancelled after {ms} ms", act.name);
            cancelled()
        }
        Err(e) => {
            acts.finish_run(id, ticket.seq, None);
            info!("act run {} with {model}: failed after {ms} ms", act.name);
            Reply::err(match e {
                crate::pipeline::ai::AiError::Timeout(t) => format!("AI timeout after {} s", t.div_ceil(1000)),
                // nie Servertext ans Panel (wie `pal_ask`): ein fremder Endpunkt schriebe sonst beliebigen Text hinein
                e => format!("AI error: {}", e.client_text()),
            })
        }
    }
}

fn cancelled() -> Reply {
    Reply::err("Cancelled").with("cancelled", json!(true))
}

fn ai_cfg(core: &Sender<Msg>) -> Result<crate::config::AiCfg, Reply> {
    let (tx, rx) = mpsc::channel();
    if core.send(Msg::AiCfg(tx)).is_err() {
        return Err(Reply::err("daemon is shutting down"));
    }
    rx.recv().map_err(|_| Reply::err("daemon is shutting down"))
}

fn handle_conn(stream: UnixStream, core: Sender<Msg>, ids: Arc<AtomicU64>, audio: Arc<dyn AudioSource>, acts: ActStore, palette: Palette, diag: Diagnose) {
    let Ok(mut out) = stream.try_clone() else { return };
    let _ = out.set_write_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = match read_limited_line(&mut reader) {
            Ok(Line::Text(l)) => l,
            Ok(Line::TooLarge) => {
                let _ = write_line(&mut out, &too_large());
                return;
            }
            _ => return,
        };
        if line.is_empty() {
            continue;
        }
        let parsed = Request::parse(&line);
        let fits = parsed.as_ref().map_or(true, |req| check_size(&line, req));
        // Zeilen können Markierungen enthalten (`act_grab`), Antworten Ergebnisse (`act_run`, `act_result`)
        act_store::wipe(&mut line);
        let reply = match parsed {
            Err(e) => Reply::err(e),
            Ok(_) if !fits => {
                let _ = write_line(&mut out, &too_large());
                return;
            }
            Ok(Request::Subscribe) => {
                subscribe(reader, out, core, ids.fetch_add(1, Ordering::Relaxed), audio, acts, palette, diag);
                return;
            }
            Ok(req) => {
                let lines = || -> Option<LineOut> {
                    let mut s = out.try_clone().ok()?;
                    Some(Box::new(move |l: &str| write_line(&mut s, l)))
                };
                let peer = || out.try_clone().ok();
                execute(req, None, &core, audio.as_ref(), &acts, &palette, &diag, &lines, &peer)
            }
        };
        let mut text = reply.to_line();
        let written = write_line(&mut out, &text);
        act_store::wipe(&mut text);
        if written.is_err() {
            return;
        }
    }
}

/// Subscriber-Verbindung: Events laufen über den Schreib-Thread, Anfragen auf derselben Verbindung werden
/// weiter beantwortet (die Antwortzeile steht zwischen den Events). `meter` gehört dieser Verbindung: schließt
/// sie, endet die Pegelmessung.
#[allow(clippy::too_many_arguments)]
fn subscribe(
    mut reader: BufReader<UnixStream>,
    out: UnixStream,
    core: Sender<Msg>,
    id: u64,
    audio: Arc<dyn AudioSource>,
    acts: ActStore,
    palette: Palette,
    diag: Diagnose,
) {
    let (stx, srx) = mpsc::sync_channel::<Arc<str>>(256);
    if core.send(Msg::Subscribe(id, stx)).is_err() {
        return;
    }
    let out = Arc::new(Mutex::new(out));
    // Schreib-Thread: endet, wenn der Kern den Sender fallen lässt (Shutdown, langsamer Client) oder das Schreiben scheitert
    let wout = out.clone();
    std::thread::Builder::new()
        .name("dictate-sub".into())
        .spawn(move || {
            while let Ok(line) = srx.recv() {
                let Ok(mut o) = wout.lock() else { break };
                if o.write_all(line.as_bytes()).and_then(|_| o.flush()).is_err() {
                    break;
                }
            }
            if let Ok(o) = wout.lock() {
                let _ = o.shutdown(std::net::Shutdown::Both);
            }
        })
        .expect("Subscriber-Thread starten");
    // Lesen bis EOF/Fehler (das Schließen durch den Schreib-Thread weckt uns ebenfalls)
    loop {
        let mut line = match read_limited_line(&mut reader) {
            Ok(Line::Text(l)) => l,
            Ok(Line::TooLarge) => {
                if let Ok(mut o) = out.lock() {
                    let _ = write_line(&mut o, &too_large());
                }
                break;
            }
            _ => break,
        };
        if line.is_empty() {
            continue;
        }
        let parsed = Request::parse(&line);
        let fits = parsed.as_ref().map_or(true, |req| check_size(&line, req));
        act_store::wipe(&mut line);
        let reply = match parsed {
            Err(e) => Reply::err(e),
            Ok(_) if !fits => {
                if let Ok(mut o) = out.lock() {
                    let _ = write_line(&mut o, &too_large());
                }
                break;
            }
            Ok(req) => {
                // über denselben Schreiber wie Events und Antworten, zeilenweise unter der Sperre
                let lines = || -> Option<LineOut> {
                    let o = out.clone();
                    Some(Box::new(move |l: &str| {
                        let mut g = o.lock().map_err(|_| std::io::Error::other("connection writer poisoned"))?;
                        write_line(&mut g, l)
                    }))
                };
                let peer = || out.lock().ok()?.try_clone().ok();
                execute(req, Some(id), &core, audio.as_ref(), &acts, &palette, &diag, &lines, &peer)
            }
        };
        let Ok(mut o) = out.lock() else { break };
        let mut text = reply.to_line();
        let written = write_line(&mut o, &text);
        act_store::wipe(&mut text);
        if written.is_err() {
            break;
        }
    }
    let _ = core.send(Msg::Unsub(id));
}

/// Daemon mit frei wählbarem Socket und Abhängigkeiten (Tests). Kehrt nach `shutdown` bzw. Signal zurück.
pub fn run_at(cfg: Config, sock: &Path, deps: Deps, handle_signals: bool) -> Result<()> {
    run_at_path(cfg, None, sock, deps, handle_signals)
}

/// Wie `run_at`, mit der Konfigurationsdatei für `set_config` (schreibt dorthin) und `reload` (liest von dort).
/// Ohne Pfad gilt `set_config` nur im Speicher.
pub fn run_at_path(cfg: Config, config_path: Option<PathBuf>, sock: &Path, deps: Deps, handle_signals: bool) -> Result<()> {
    ensure_dir(sock)?;
    // Eine Instanz: Sperrdatei neben dem Socket, lebt so lange wie der Prozess
    let lock_path: PathBuf = sock.with_extension("lock");
    let lock =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path).with_context(|| format!("opening {}", lock_path.display()))?;
    if lock.try_lock().is_err() {
        bail!("another dictate daemon is already running ({})", sock.display());
    }
    // Wir halten die Sperre, ein vorhandener Socket ist also veraltet
    if sock.exists() {
        info!("removing stale socket {}", sock.display());
        std::fs::remove_file(sock).with_context(|| format!("removing stale socket {}", sock.display()))?;
    }
    let listener = UnixListener::bind(sock).with_context(|| format!("binding {}", sock.display()))?;
    std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600))?;

    let (tx, rx) = mpsc::channel::<Msg>();
    let (wtx, wrx) = mpsc::channel::<Work>();
    let last_typed: LastTyped = Arc::new(Mutex::new(None));
    let ai_cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    let worker = {
        let (core_tx, backend, cfg, lt, ac) = (tx.clone(), deps.backend.clone(), cfg.clone(), last_typed.clone(), ai_cancel.clone());
        std::thread::Builder::new().name("dictate-engine".into()).spawn(move || worker_main(wrx, core_tx, backend, cfg, lt, ac))?
    };
    // Modell im Hintergrund vorladen
    let _ = wtx.send(Work::Load);

    if handle_signals {
        let mut sigs = signal_hook::iterator::Signals::new([signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT])?;
        let qtx = tx.clone();
        std::thread::Builder::new().name("dictate-signals".into()).spawn(move || {
            if sigs.forever().next().is_some() {
                let _ = qtx.send(Msg::Quit);
            }
        })?;
    }

    // Textaktionen: Speicher der Markierung, geteilt von den Verbindungs-Threads; der Ablauf-Thread endet mit `sweeper`
    let (acts, sweeper) = ActStore::start(deps.act_ttl)?;
    // Befehlspalette: Quellen und Häufigkeit entstehen erst beim ersten Zugriff; `frecency.json` liegt neben dem Verlauf.
    // Vorher holt der Start eine Häufigkeit ab, die noch unter dem alten Namen `palette.json` liegt (dem Namen der
    // Farbdatei von Noctalia, `PLAN-wayland.md` 6h, V1), bevor Noctalia sie überschreibt.
    if let Some(dir) = deps.history.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Err(e) = frecency::adopt_legacy(dir) {
            tracing::warn!("palette: old frecency file not moved: {e:#}");
        }
    }
    let palette = Palette::new(cfg.palette.clone(), deps.history.with_file_name(frecency::FILE_NAME), deps.palette_env.clone());
    palette.set_diagnose_enabled(cfg.diagnose.enabled);
    // Diagnose: Sitzung im Speicher, der Ablauf-Thread endet mit `diag_sweeper`
    // Der Hinweis nach einem unsauberen Boot-Ende merkt sich den Boot in `diagnose.json` neben dem Verlauf (State-Ordner)
    let mut diag_deps = deps.diagnose.clone();
    if diag_deps.state.is_none() {
        diag_deps.state = Some(deps.history.with_file_name("diagnose.json"));
    }
    let (diag, diag_sweeper) = Diagnose::start(diag_deps)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let acceptor = {
        let (stopping, core_tx) = (stopping.clone(), tx.clone());
        let ids = Arc::new(AtomicU64::new(1));
        let audio = deps.audio.clone();
        let palette = palette.clone();
        std::thread::Builder::new().name("dictate-accept".into()).spawn(move || {
            for conn in listener.incoming() {
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(conn) = conn else {
                    // Fehler wie EMFILE nicht in einer Dauerschleife wiederholen
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                let (core_tx, ids, audio, acts, palette, diag) = (core_tx.clone(), ids.clone(), audio.clone(), acts.clone(), palette.clone(), diag.clone());
                let _ = std::thread::Builder::new().name("dictate-conn".into()).spawn(move || handle_conn(conn, core_tx, ids, audio, acts, palette, diag));
            }
        })?
    };

    let mut core = Core {
        overlay: cfg.overlay.clone(),
        terminals: cfg.actions.terminal_classes.clone(),
        cfg,
        pending: None,
        config_path,
        rx,
        tx,
        work: wtx,
        worker: Some(worker),
        audio: deps.audio,
        subs: Vec::new(),
        model: Model::Loading,
        loads_inflight: 1,
        recording: None,
        rec_gen: 0,
        finalizing: false,
        cancel_req: false,
        pending_start: false,
        pending_app: None,
        rec_window: true,
        pending_window: true,
        next_window: true,
        error: None,
        last_state: State::Loading,
        idle_since: Instant::now(),
        started: None,
        meter: None,
        meter_gen: 0,
        history: History::new(deps.history),
        last_typed,
        ai_cancel,
        palette,
    };
    info!("daemon ready on {}", sock.display());
    core.run();

    // Aufräumen: Accept-Thread wecken, Socket entfernen
    stopping.store(true, Ordering::Relaxed);
    let _ = UnixStream::connect(sock);
    let _ = acceptor.join();
    // Markierung löschen, Ablauf-Thread beenden
    drop(sweeper);
    drop(diag_sweeper);
    let _ = std::fs::remove_file(sock);
    let _ = std::fs::remove_file(&lock_path);
    drop(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::long_note;

    #[test]
    fn long_note_only_above_the_refine_target() {
        // Ziel: dreifache Eingabe, ohne Untergrenze in Zeichen
        assert_eq!(long_note(40, 120), None);
        assert!(long_note(40, 121).is_some());
        assert_eq!(long_note(200, 600), None);
        let n = long_note(200, 840).unwrap();
        assert!(n.contains("4.2 times") && n.contains("at most 3 times"), "{n}");
    }
}
