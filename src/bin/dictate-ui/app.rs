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

//! Hauptthread von `dictate-ui run`: GTK, Overlay, Haltelogik (`ui::hold`), Warteschlange der Textausgabe
//! (`ui::output::Queue`, ausgeführt im Ausgabe-Thread `output`) und die Nachrichten der Threads aus `link`. Abfrage
//! des Tastenzustands im Takt `hold::POLL` nur, solange gehalten wird; die Geräte werden dafür bei jedem Halten neu
//! geöffnet und danach geschlossen.
//!
//! Drei Panels teilen sich ein Fenster (`panel.rs`), offen ist höchstens eins: Textaktionen (`ui::panel`, Mod+I),
//! Diagnose (`ui::diag_panel`, Mod+Shift+I) und Befehlspalette (`ui::cmdpal`, Mod+Alt+Space). Das Kürzel eines anderen
//! Panels schließt das offene nur; die Eingaben des Fensters gehören der offenen Quelle (Reihenfolge Diagnose, Palette,
//! Textaktionen). Sperre, Fokusverlust und Leerlauf schließen jedes.
//!
//! Mod+E bei offener Palette nimmt für ihre Eingabezeile auf (`ui::voice_owner`, L23): Zustand, Mitschrift, Pegel und
//! `final` dieser Sitzung gehen an den Automaten statt an Overlay und Ausgabe, ausgeführt wird nie.

use crate::act::{self, Act, ActMsg};
use crate::actions::store::{wipe, SecretText, Source as TextSource};
use crate::diag::{self, Diag, DiagMsg};
use crate::link::{self, Commander, CtlServer, Msg};
use crate::lock::Logind;
use crate::output::{Done, Output};
use crate::overlay::Overlay;
use crate::pal::{Origin, PalIo, PalMsg};
use crate::panel::{PanelUi, UiEv};
use crate::protocol::{self, Event, State};
use crate::ui::boot_hint::{self, Asker, Next, Timing};
use crate::ui::cmdpal::{self, CmdPal, Lane, VoicePhase};
use crate::ui::ctl::{self, UiCmd};
use crate::ui::diag_panel::{self, DiagPanel};
use crate::ui::fake_keys::FileKeys;
use crate::ui::hold::{self, DaemonCmd, Hold, Out};
use crate::ui::keys::{Evdev, KeySource};
use crate::ui::output::{OutCfg, Queue};
use crate::ui::pal_exec::{ExecCtx, Handoff, Outcome, Programs};
use crate::ui::palette::{self, Palette, Source};
use crate::ui::panel::{Effect, Panel, Phase};
use crate::ui::view::{self, NoticeKind};
use crate::ui::voice_owner::{Route, VoiceOwner};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Dauer von Hinweisen (`notice_timeout` in init.lua)
const NOTICE: Duration = Duration::from_secs(3);
/// Hinweis, wenn das Halten nicht erkannt wurde und die Aufnahme als Schalter weiterläuft (`hold::Out::Latched`)
const LATCHED_TEXT: &str = "Key release not detected: press the dictate key again to stop";

/// Ein Kürzel kam, der Fokus wird gerade im Hilfsthread gemerkt (bei der Palette auch der Kontext)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opening {
    Actions,
    Palette,
}

struct Notice {
    text: String,
    kind: NoticeKind,
    timer: glib::SourceId,
}

/// Boot-Hinweis (`ui::boot_hint`): Fragen an den Daemon, ein wartendes „ja“, der stehende Hinweis
struct Boot {
    asker: Asker,
    /// „ja“ kam, das Overlay war noch nicht frei
    pending: bool,
    /// Der Hinweis steht (Zeitgeber bis zum Ende); er zeigt sich, solange das Overlay frei ist
    until: Option<glib::SourceId>,
    /// Zuletzt gezeichnet: Hinweis sichtbar (und anklickbar)
    shown: bool,
}

impl Boot {
    /// Hinweis beenden (Diagnose geöffnet): auch ein wartendes „ja“ entfällt
    fn end(&mut self) -> bool {
        let was = self.pending || self.until.is_some();
        self.pending = false;
        if let Some(t) = self.until.take() {
            // läuft noch (ein abgelaufener Zeitgeber hat `until` selbst geleert)
            t.remove();
        }
        was
    }
}

struct App {
    hold: Hold,
    state: State,
    stable: String,
    tail: String,
    notice: Option<Notice>,
    overlay: Overlay,
    keys: Option<Box<dyn KeySource>>,
    /// Öffnet die Tastaturen; läuft im Hilfsthread, das Ergebnis kommt als `Msg::Keys`
    open_keys: Arc<dyn Fn() -> Box<dyn KeySource> + Send + Sync>,
    /// Nummer des laufenden Haltens (ein spät geöffnetes Gerät eines früheren wird sofort geschlossen)
    poll_gen: u64,
    /// Letzte Abfrage (für das Log: Zeitpunkt, zu dem E losgelassen gesehen wurde)
    last_e: Option<bool>,
    poll: Option<glib::SourceId>,
    tx: async_channel::Sender<Msg>,
    /// Gerade im Rückruf des Abfrage-Timers (der Timer endet dann über seinen Rückgabewert, nicht über `remove`)
    in_tick: bool,
    cmd: Commander,
    /// Nummer des letzten `start` (siehe `Msg::Failed`)
    start_seq: u64,
    /// Beobachtet `palette.json` (Noctalia schreibt sie bei jedem Wechsel von Schema oder Modus neu)
    _palette_watch: Option<gio::FileMonitor>,
    palette_reload: Option<glib::SourceId>,
    /// Warteschlange der Textausgabe (Hauptthread) und der Ausgabe-Thread
    queue: Queue,
    out: Option<Output>,
    out_cfg: OutCfg,
    /// Textaktionen: Automat, Fenster (beim ersten Zeigen angelegt), Daemon-Seite
    panel: Panel,
    panel_ui: Option<PanelUi>,
    act: Act,
    /// Mod+I oder Mod+Alt+Space kam, der Fokus wird gerade gemerkt; `open_cancelled`: ein weiteres Kürzel (oder die
    /// Sperre) kam vorher
    opening: Option<Opening>,
    open_cancelled: bool,
    /// Zeitgeber für `Panel::next_deadline`
    panel_timer: Option<glib::SourceId>,
    /// Laufende Replace-Aufträge der Ausgabe: Nummer → Zeichen
    replaces: HashMap<u64, u64>,
    /// Sperr-Erkennung (logind); `None` nur ganz am Anfang
    logind: Option<Logind>,
    /// Diagnose: Automat, Daemon-Seite (`diag.rs`), Zeitgeber für `DiagPanel::next_deadline`
    diag: DiagPanel,
    diag_io: Diag,
    diag_timer: Option<glib::SourceId>,
    boot: Boot,
    /// Befehlspalette: Automat, Daemon-Seite und Ausführen (`pal.rs`), Zeitgeber für `CmdPal::next_deadline`
    cmdpal: CmdPal,
    pal_io: PalIo,
    pal_timer: Option<glib::SourceId>,
    /// Kontext der offenen Palette (Fenstertitel; beim Schließen überschrieben) und das Fenster, das vor dem Öffnen
    /// fokussiert war (nie das Panel, `pal_host::NiriHost::opened`)
    pal_ctx: Option<SecretText>,
    pal_focus: Option<u64>,
    /// `app_id` des Fensters `pal_focus` (für eine Übergabe an die Textaktionen: Replace)
    pal_app: Option<String>,
    /// Übergabe aus der Palette an die Textaktionen bzw. die Diagnose: Absicht und Ziel-ID für `pal_chosen`, sobald das
    /// Panel Erfolg meldet (`Effect::Chosen`, Lesart L34); vergessen, sobald das Panel zu ist. Nie loggen
    act_origin: Option<(&'static str, String)>,
    diag_origin: Option<(&'static str, String)>,
    /// Wem die laufende bzw. nächste Aufnahme gehört (Fenster oder Palette)
    voice: VoiceOwner,
}

type Shared = Rc<RefCell<App>>;

pub fn run(fake_keys: Option<PathBuf>, boot_timing: Timing, programs: Programs) -> Result<(), String> {
    let start = Instant::now();
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err("WAYLAND_DISPLAY is not set: dictate-ui runs only in a Wayland session".into());
    }
    // Steuer-Socket zuerst: eine zweite Instanz endet hier, bevor sie GTK und das Display anfasst
    let (tx, rx) = async_channel::unbounded::<Msg>();
    let tx_quit = tx.clone();
    let _ctl = CtlServer::bind(ctl::socket_path(), tx.clone())?;
    // `app_id` der GTK-Fenster (GTK nimmt den Programmnamen): nur das Panel ist ein Fenster, das Overlay eine
    // Layer-Fläche mit eigenem Namensraum
    glib::set_prgname(Some(crate::panel::APP_ID));
    gtk::init().map_err(|e| format!("cannot initialise GTK: {e}"))?;
    let overlay = Overlay::new(start)?;
    overlay.prepare();
    info!("GTK ready after {} ms", start.elapsed().as_millis());

    let sock = protocol::socket_path();
    let cmd = Commander::spawn(sock.clone(), tx.clone());
    link::spawn_listener(sock, tx.clone());

    let open_keys: Arc<dyn Fn() -> Box<dyn KeySource> + Send + Sync> = match fake_keys {
        Some(p) => {
            info!("key state from {} (test mode)", p.display());
            Arc::new(move || Box::new(FileKeys::new(p.clone())))
        }
        None => Arc::new(|| {
            let t = Instant::now();
            let e = Evdev::open();
            info!("keys: {} keyboard(s) with E opened in {:.1} ms", e.len(), t.elapsed().as_secs_f64() * 1000.0);
            Box::new(e)
        }),
    };
    let out = Output::spawn(tx.clone(), open_keys.clone())?;
    let act = Act::spawn(protocol::socket_path(), out.handle(), tx.clone());
    let back = tx.clone();
    let diag_io = Diag::spawn(protocol::socket_path(), out.handle(), move |m| back.send_blocking(Msg::Diag(m)).is_ok());
    let pal_io = PalIo::spawn(protocol::socket_path(), programs, tx.clone());
    let app: Shared = Rc::new(RefCell::new(App {
        hold: Hold::new(),
        state: State::Idle,
        stable: String::new(),
        tail: String::new(),
        notice: None,
        overlay,
        keys: None,
        open_keys,
        poll_gen: 0,
        last_e: None,
        tx,
        poll: None,
        in_tick: false,
        cmd,
        start_seq: 0,
        _palette_watch: None,
        palette_reload: None,
        queue: Queue::new(),
        out: Some(out),
        out_cfg: OutCfg::default(),
        panel: Panel::new(),
        panel_ui: None,
        act,
        opening: None,
        open_cancelled: false,
        panel_timer: None,
        replaces: HashMap::new(),
        logind: None,
        diag: DiagPanel::new(),
        diag_io,
        diag_timer: None,
        boot: Boot { asker: Asker::new(boot_timing), pending: false, until: None, shown: false },
        cmdpal: CmdPal::new(),
        pal_io,
        pal_timer: None,
        pal_ctx: None,
        pal_focus: None,
        pal_app: None,
        act_origin: None,
        diag_origin: None,
        voice: VoiceOwner::new(),
    }));
    // Klick auf den Boot-Hinweis (nur solange er steht, sonst nimmt das Overlay keine Klicks an)
    {
        let tx = app.borrow().tx.clone();
        app.borrow().overlay.on_click(move || {
            let _ = tx.send_blocking(Msg::BootClick);
        });
    }
    if boot_timing != Timing::default() {
        info!("boot hint: test timing {boot_timing:?}");
    }
    boot_ask_later(&app, boot_timing.first);
    watch_palette(&app);
    // Sperre: vor dem ersten Kürzel abonnieren (asynchron, blockiert nie)
    let logind = Logind::start(app.borrow().tx.clone());
    app.borrow_mut().logind = Some(logind);

    let main_loop = glib::MainLoop::new(None, false);
    // Signale in einem eigenen Thread (signal-hook), Ende über den Kanal: so endet der Hauptthread geordnet
    let mut signals = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT, libc::SIGHUP]).map_err(|e| format!("signals: {e}"))?;
    let quit = tx_quit;
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            if let Some(sig) = signals.forever().next() {
                info!("signal {sig}");
                let _ = quit.send_blocking(Msg::Quit);
            }
        })
        .map_err(|e| e.to_string())?;
    let a = app.clone();
    let ml = main_loop.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            if matches!(msg, Msg::Quit) {
                break;
            }
            handle(&a, msg);
        }
        ml.quit();
    });
    main_loop.run();

    // Beenden: eine laufende Aufnahme abbrechen (sonst nähme der Daemon bis `max_listen_s` weiter auf)
    let recording = {
        let a = app.borrow();
        a.hold.polling() || a.hold.latched() || matches!(a.state, State::Loading | State::Listening)
    };
    if recording {
        info!("exiting during a recording: cancelling it");
        link::cancel_on_exit();
    }
    // Offenes Panel: Sitzung im Daemon löschen (Markierung und Antwort nicht liegen lassen)
    if app.borrow().panel.is_open() {
        act::clear_now(&protocol::socket_path());
    }
    // Offene Diagnose: ebenso (Bericht und Antwort nicht liegen lassen)
    if app.borrow().diag.is_open() {
        diag::clear_now(&protocol::socket_path());
    }
    // Ausgabe beenden: laufenden Auftrag abbrechen, Tasten loslassen, Eingabemethode zerstören
    if let Some(out) = app.borrow_mut().out.take() {
        out.shutdown();
    }
    info!("exiting");
    Ok(())
}

fn handle(this: &Shared, msg: Msg) {
    handle_msg(this, msg);
    // ein wartender oder stehender Boot-Hinweis zeigt sich, sobald das Overlay frei ist (Panel zu, Aufnahme vorbei,
    // entsperrt), und verschwindet, solange es das nicht ist
    let redraw = {
        let a = this.borrow();
        let free = overlay_free(&a);
        (free && a.boot.pending) || a.boot.shown != (free && a.boot.until.is_some())
    };
    if redraw {
        render(this);
    }
}

fn handle_msg(this: &Shared, msg: Msg) {
    match msg {
        Msg::Ctl(UiCmd::Actions) => {
            info!("shortcut actions");
            actions_key(this);
        }
        Msg::Ctl(UiCmd::Diagnose) => {
            info!("shortcut diagnose");
            diagnose_key(this);
        }
        Msg::Ctl(UiCmd::Palette) => {
            info!("shortcut palette");
            palette_key(this);
        }
        Msg::Pal(m) => pal_msg(this, m),
        Msg::PalAskTimeout(ms) => {
            debug!("palette: AI timeout {ms} ms");
            this.borrow_mut().pal_io.set_ask_timeout_ms(ms);
        }
        Msg::BootHint(ans) => boot_answer(this, ans),
        Msg::BootClick => {
            // nur, wenn er gerade gezeigt wird (ein später Klick bei inzwischen offenem Panel zählt nicht)
            if this.borrow().boot.shown {
                info!("boot hint: clicked");
                diagnose_key(this);
            }
        }
        Msg::Act(m) => act_msg(this, m),
        Msg::Diag(m) => diag_msg(this, m),
        Msg::Panel(ev) => panel_ev(this, ev),
        Msg::Lock(ev) => {
            let reason = this.borrow_mut().logind.as_mut().and_then(|l| l.event(ev));
            if let Some(r) = reason {
                locked(this, r);
            }
            // Erkennung an- oder ausgegangen, während das Panel offen ist: Hinweis nachziehen
            let mut a = this.borrow_mut();
            let a = &mut *a;
            let off = a.logind.as_ref().is_some_and(Logind::off);
            if let Some(ui) = a.panel_ui.as_mut() {
                if ui.set_lock_off(off) {
                    if a.panel.is_open() {
                        ui.render(&a.panel);
                    } else if a.diag.is_open() {
                        ui.render_diag(&mut a.diag);
                    } else if a.cmdpal.is_open() {
                        ui.render_pal(&a.cmdpal, Instant::now());
                    }
                }
            }
        }
        Msg::Ctl(c) => {
            info!("shortcut {}", c.name());
            let outs = {
                let mut a = this.borrow_mut();
                match c {
                    UiCmd::Hold => a.hold.press(Instant::now()),
                    UiCmd::Toggle => a.hold.toggle(),
                    UiCmd::Cancel => {
                        a.clear_notice();
                        a.cancel_output();
                        // eine Aufnahme der Palette: ihr `final` verwerfen (`cancel` schickt `hold.cancel` ohnehin)
                        a.voice.cancel();
                        a.hold.cancel()
                    }
                    UiCmd::Actions | UiCmd::Diagnose | UiCmd::Palette => unreachable!("handled above"),
                }
            };
            apply(this, outs);
            if c == UiCmd::Cancel {
                // die Aufnahme-Ansicht der Palette weg (die Liste kehrt zurück)
                let effects = this.borrow_mut().cmdpal.voice_state(None);
                apply_pal(this, effects);
            }
            render(this);
        }
        Msg::Event(ev) => event(this, ev),
        Msg::Connected | Msg::Quit => {}
        Msg::Keys { gen, keys } => {
            {
                let mut a = this.borrow_mut();
                if a.poll.is_none() || gen != a.poll_gen {
                    // Halten schon vorbei: Geräte sofort schließen
                    return;
                }
                a.keys = Some(keys);
            }
            // erste Abfrage sofort (ein sehr kurzer Druck wäre sonst schon vorbei)
            let outs = sample(this);
            apply(this, outs);
        }
        Msg::Disconnected => {
            let (outs, effects) = {
                let mut a = this.borrow_mut();
                a.state = State::Idle;
                a.stable.clear();
                a.tail.clear();
                a.voice.reset();
                (a.hold.reset(), a.cmdpal.voice_state(None))
            };
            apply(this, outs);
            apply_pal(this, effects);
            render(this);
        }
        Msg::Limit(n) => {
            let mut a = this.borrow_mut();
            a.hold.set_daemon_limit(n);
            debug!("daemon max_listen_s {n}, hold cap {} s", a.hold.cap().as_secs());
        }
        Msg::OutCfg(c) => {
            debug!("output: paste {}, delay {} ms, {} terminal classes, {} keyboard-only apps", c.paste, c.delay_ms, c.terminals.len(), c.keyboard_only.len());
            this.borrow_mut().out_cfg = c;
        }
        Msg::OutputDone { id, done } => {
            let replaced = this.borrow_mut().replaces.remove(&id);
            let note = if let Some(chars) = replaced {
                Some(match &done {
                    Done::Written => (format!("Replaced {}", crate::ui::panel::plural(chars, "character", "characters")), NoticeKind::Info),
                    Done::Stopped => ("Replace stopped".to_string(), NoticeKind::Info),
                    Done::Unconfirmed => ("The window did not take the text. Copy it instead.".to_string(), NoticeKind::Info),
                    Done::Failed(e) => (format!("Replace: {e}"), NoticeKind::Error),
                })
            } else {
                match &done {
                    Done::Written | Done::Stopped => None,
                    Done::Unconfirmed => Some(("Dictation: the window did not take the pasted text".to_string(), NoticeKind::Info)),
                    Done::Failed(e) => Some((format!("Dictation: {e}"), NoticeKind::Error)),
                }
            };
            {
                let mut a = this.borrow_mut();
                a.queue.done(id);
                a.pump_output();
            }
            if let Some((text, kind)) = note {
                notice(this, text, kind);
            }
        }
        Msg::OutputNotice(text, kind) => notice(this, text, kind),
        Msg::Failed { cmd, seq, msg } => {
            // Start gescheitert (Daemon fehlt, Fehler): kein Halten ohne Aufnahme; nur das Halten, das dieses `start`
            // ausgelöst hat (eine späte Antwort setzt kein neueres zurück)
            let outs = {
                let mut a = this.borrow_mut();
                if matches!(cmd, DaemonCmd::Start | DaemonCmd::Toggle) {
                    // das `start`/`toggle` der Palette: es beginnt keine Sitzung für sie
                    a.voice.failed(seq);
                }
                if cmd == DaemonCmd::Start && seq == a.start_seq {
                    a.hold.reset()
                } else {
                    vec![]
                }
            };
            apply(this, outs);
            notice(this, msg, NoticeKind::Error);
        }
    }
}

fn event(this: &Shared, ev: Event) {
    let now = Instant::now();
    match ev {
        Event::State { state } => {
            debug!("daemon state {}", state.name());
            let (outs, effects) = {
                let mut a = this.borrow_mut();
                let a = &mut *a;
                let was_recording = matches!(a.state, State::Loading | State::Listening);
                a.state = state;
                let recording = matches!(state, State::Loading | State::Listening);
                let route = a.voice.state(recording, state == State::Finalizing, pal_takes_voice(a));
                if recording && !was_recording {
                    info!(
                        "recording for the {}",
                        match route {
                            Route::Window => "window",
                            Route::Palette => "command palette",
                            Route::Discard => "command palette (cancelled, discarded)",
                        }
                    );
                }
                let mut effects = vec![];
                match route {
                    Route::Window => {
                        if recording && !was_recording {
                            a.queue.recording_started();
                        }
                        if state == State::Finalizing {
                            // Eingabemethode schon jetzt anlegen: ihr `activate` liegt beim `final` vor
                            if let Some(o) = &a.out {
                                o.arm();
                            }
                        }
                    }
                    // nie `recording_started` und keine Eingabemethode: das `final` geht nicht in die Ausgabe
                    Route::Palette => {
                        effects = a.cmdpal.voice_state(match state {
                            State::Loading => Some(VoicePhase::Loading),
                            State::Listening => Some(VoicePhase::Listening),
                            State::Finalizing => Some(VoicePhase::Finalizing),
                            State::Idle | State::Error => None,
                        });
                    }
                    Route::Discard => {}
                }
                match state {
                    State::Loading | State::Idle => {
                        a.stable.clear();
                        a.tail.clear();
                    }
                    _ => {}
                }
                // Fehler von früher weg, sobald eine neue Aufnahme beginnt (Hinweise wie „press again“ bleiben)
                if recording && a.notice.as_ref().is_some_and(|n| n.kind == NoticeKind::Error) {
                    a.clear_notice();
                }
                (a.hold.daemon_state(recording), effects)
            };
            apply(this, outs);
            apply_pal(this, effects);
            render(this);
        }
        Event::Partial { stable, tail } => {
            let route = this.borrow().voice.route();
            match route {
                Route::Window => {
                    let show = {
                        let mut a = this.borrow_mut();
                        a.stable = stable;
                        a.tail = tail;
                        matches!(a.state, State::Listening | State::Finalizing)
                    };
                    if show {
                        render(this);
                    }
                }
                Route::Palette => {
                    let (mut stable, mut tail) = (stable, tail);
                    let effects = this.borrow_mut().cmdpal.voice_partial(&stable, &tail);
                    wipe(&mut stable);
                    wipe(&mut tail);
                    apply_pal(this, effects);
                }
                Route::Discard => {
                    let (mut stable, mut tail) = (stable, tail);
                    wipe(&mut stable);
                    wipe(&mut tail);
                }
            }
        }
        Event::Level { db, .. } => {
            // die Ausleihe nicht im `match` halten (darin wird `borrow_mut` gebraucht)
            let route = this.borrow().voice.route();
            match route {
                Route::Window => {
                    let a = this.borrow();
                    if a.state == State::Listening && a.notice.is_none() && a.overlay.visible() {
                        a.overlay.set_level(view::level_from_db(db));
                    }
                }
                Route::Palette => {
                    let effects = this.borrow_mut().cmdpal.voice_level(view::level_from_db(db) as f32);
                    apply_pal(this, effects);
                }
                Route::Discard => {}
            }
        }
        Event::Final { mut text, cancelled, keys, .. } => {
            // Ins Log nur Länge und Tasten, nie den Text
            let effects = {
                let mut a = this.borrow_mut();
                let a = &mut *a;
                let chars = text.chars().count();
                let effects = match a.voice.final_event() {
                    Route::Window => {
                        let queued = a.queue.on_final(&text, &keys, cancelled);
                        if cancelled {
                            info!("final: cancelled");
                        } else {
                            info!("final: {chars} chars, {} keys, {}", keys.len(), if queued { "queued for output" } else { "discarded" });
                        }
                        vec![]
                    }
                    // Tasten im `final` („undo that“) gehören nie zur Eingabezeile; ausgeführt wird nie (B5)
                    Route::Palette => {
                        info!("final: {chars} chars for the command palette{}", if cancelled { " (cancelled)" } else { "" });
                        a.cmdpal.voice_final(if cancelled { "" } else { &text }, now)
                    }
                    Route::Discard => {
                        info!("final: {chars} chars of a cancelled palette recording, discarded");
                        vec![]
                    }
                };
                wipe(&mut text);
                a.stable.clear();
                a.tail.clear();
                a.state = State::Idle;
                a.pump_output();
                effects
            };
            apply_pal(this, effects);
            render(this);
        }
        Event::Error { msg } => notice(this, format!("Dictation error: {msg}"), NoticeKind::Error),
        Event::Overlay { overlay } => {
            this.borrow_mut().overlay.apply_cfg(overlay);
            render(this);
        }
        // Terminals: dort Return statt Shift+Return und nie einfügen (die Config kommt danach ganz, `Msg::OutCfg`)
        Event::Terminals { classes } => this.borrow_mut().out_cfg.terminals = classes,
    }
}

/// Palette laden und die Datei beobachten (auch Anlegen, Löschen und Ersetzen per Umbenennen); neu geladen wird
/// 50 ms nach der letzten Änderung (Noctalia schreibt mehrere Vorlagen kurz nacheinander)
fn watch_palette(this: &Shared) {
    let path = palette::path();
    reload_palette(this, &path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let monitor = match gio::File::for_path(&path).monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
        Ok(m) => m,
        Err(e) => {
            warn!("palette: cannot watch {}: {e}", path.display());
            return;
        }
    };
    let weak = Rc::downgrade(this);
    monitor.connect_changed(move |_, _, _, _| {
        let Some(this) = weak.upgrade() else { return };
        if let Some(id) = this.borrow_mut().palette_reload.take() {
            id.remove();
        }
        let (weak, path) = (Rc::downgrade(&this), path.clone());
        let id = glib::timeout_add_local_once(Duration::from_millis(50), move || {
            if let Some(this) = weak.upgrade() {
                this.borrow_mut().palette_reload = None;
                reload_palette(&this, &path);
            }
        });
        this.borrow_mut().palette_reload = Some(id);
    });
    this.borrow_mut()._palette_watch = Some(monitor);
}

fn reload_palette(this: &Shared, path: &std::path::Path) {
    let (p, src) = Palette::load(path);
    match src {
        Source::File => info!("palette from {} ({})", path.display(), if p.dark { "dark" } else { "light" }),
        Source::Fallback(why) => info!("palette: built-in Tokyo Night ({why})"),
    }
    if this.borrow().overlay.palette() != p {
        if let Some(ui) = this.borrow_mut().panel_ui.as_mut() {
            ui.update_palette(p.clone());
        }
        this.borrow_mut().overlay.set_palette(p);
        render(this);
    }
}

impl App {
    fn clear_notice(&mut self) {
        if let Some(n) = self.notice.take() {
            n.timer.remove();
        }
    }

    /// Läuft oder endet gerade eine Aufnahme (dann kommt noch ein `final`)?
    fn in_flight(&self) -> bool {
        self.hold.polling() || self.hold.latched() || matches!(self.state, State::Loading | State::Listening | State::Finalizing)
    }

    /// Nächsten Auftrag an den Ausgabe-Thread; ist nichts mehr zu tun, die Eingabemethode zerstören (nur für die
    /// Dauer einer Ausgabe, Regel 4.9)
    fn pump_output(&mut self) {
        let Some(out) = &self.out else { return };
        if let Some(job) = self.queue.take_next() {
            debug!("output: {}", job.describe());
            out.job(job, self.out_cfg.clone());
        } else if self.queue.idle() && self.state != State::Finalizing {
            out.disarm();
        }
    }

    /// Mod+Ctrl+E: Ausgabe leeren und abbrechen (Befund 2), ein spätes `final` verwerfen (Befund 3)
    fn cancel_output(&mut self) {
        let in_flight = self.in_flight();
        let c = self.queue.cancel(in_flight);
        if let (Some(id), Some(out)) = (c.running, &self.out) {
            out.cancel(id);
        }
        // verworfene Replace-Aufträge vergessen (der laufende meldet sich noch)
        self.replaces.retain(|id, _| Some(*id) == c.running);
        if c.running.is_some() || c.dropped > 0 {
            info!("output cancelled ({} running, {} waiting dropped)", c.running.map_or(0, |_| 1), c.dropped);
        }
        if !in_flight {
            self.pump_output();
        }
    }
}

fn notice(this: &Shared, text: String, kind: NoticeKind) {
    let weak = Rc::downgrade(this);
    let timer = glib::timeout_add_local_once(NOTICE, move || {
        if let Some(this) = weak.upgrade() {
            // Der Timer ist gerade abgelaufen: Hinweis nur vergessen (ein `remove` darauf wäre ein Fehler)
            this.borrow_mut().notice = None;
            render(&this);
        }
    });
    {
        let mut a = this.borrow_mut();
        a.clear_notice();
        a.notice = Some(Notice { text, kind, timer });
    }
    render(this);
}

fn render(this: &Shared) {
    let mut a = this.borrow_mut();
    let a = &mut *a;
    let free = overlay_free(a);
    if free && a.boot.pending {
        a.boot.pending = false;
        let weak = Rc::downgrade(this);
        a.boot.until = Some(glib::timeout_add_local_once(a.boot.asker.timing().show, move || {
            if let Some(this) = weak.upgrade() {
                // abgelaufen: nur vergessen (ein `remove` darauf wäre ein Fehler)
                this.borrow_mut().boot.until = None;
                info!("boot hint: ended");
                render(&this);
            }
        }));
        info!("boot hint: shown");
    }
    let hint = free && a.boot.until.is_some();
    a.boot.shown = hint;
    let notice = a.notice.as_ref().map(|n| (n.text.as_str(), n.kind)).or(hint.then_some((boot_hint::TEXT, NoticeKind::Info)));
    // eine Aufnahme der Palette zeigt sich nur im Panel; im Overlay stehen dann nur Hinweise und Fehler
    let state = if a.voice.hides_overlay() { State::Idle } else { a.state };
    let content = view::content(state, &a.stable, &a.tail, notice);
    a.overlay.show(&content, state == State::Listening);
    a.overlay.set_clickable(hint);
}

/// Boot-Hinweis nur, wenn das Overlay sonst nichts zeigt, kein Panel offen und die Sitzung nicht gesperrt ist
fn overlay_free(a: &App) -> bool {
    a.notice.is_none() && !a.in_flight() && !a.panel.is_open() && !a.diag.is_open() && !a.cmdpal.is_open() && a.opening.is_none() && !session_locked(a)
}

/// Nach `after` den Daemon fragen, ob der vorige Boot unsauber endete (`diag_boot_hint`), in einem Hilfsthread
fn boot_ask_later(this: &Shared, after: Duration) {
    let weak = Rc::downgrade(this);
    // einmalig: läuft ab und wird nie entfernt
    glib::timeout_add_local_once(after, move || {
        let Some(this) = weak.upgrade() else { return };
        let (tx, attempt) = {
            let a = this.borrow();
            (a.tx.clone(), a.boot.asker.attempt())
        };
        info!("boot hint: asking (attempt {attempt})");
        let sock = protocol::socket_path();
        let spawned = std::thread::Builder::new().name("boot-hint".into()).spawn(move || {
            let ans = diag::boot_hint(&sock);
            let _ = tx.send_blocking(Msg::BootHint(ans));
        });
        if let Err(e) = spawned {
            warn!("boot hint: cannot start the thread: {e}");
        }
    });
}

fn boot_answer(this: &Shared, ans: boot_hint::Answer) {
    let next = this.borrow_mut().boot.asker.answer(ans);
    info!("boot hint: answer {}", ans.name());
    match next {
        Next::Ask(d) => boot_ask_later(this, d),
        Next::Show => {
            this.borrow_mut().boot.pending = true;
            render(this);
        }
        Next::Done => {}
    }
}

/// Folgen der Haltelogik ausführen
fn apply(this: &Shared, outs: Vec<Out>) {
    for o in outs {
        match o {
            Out::Daemon(c) => {
                let mut a = this.borrow_mut();
                let a = &mut *a;
                // Mod+E bei offener Palette: die Aufnahme gehört ihrer Eingabezeile, wenn gerade keine läuft (L23, L32)
                let palette = matches!(c, DaemonCmd::Start | DaemonCmd::Toggle) && a.voice.claim(a.state == State::Idle, pal_takes_voice(a));
                let seq = if palette { a.cmd.send_palette(c == DaemonCmd::Toggle) } else { a.cmd.send(c) };
                if palette {
                    a.voice.sent(seq);
                }
                if c == DaemonCmd::Start {
                    a.start_seq = seq;
                }
            }
            Out::Keys(true) => start_polling(this),
            Out::Keys(false) => stop_polling(this),
            Out::Latched => {
                info!("hold: key release not detected, recording continues until the next press");
                notice(this, LATCHED_TEXT.into(), NoticeKind::Info);
            }
        }
    }
}

fn start_polling(this: &Shared) {
    stop_polling(this);
    let weak = Rc::downgrade(this);
    let id = glib::timeout_add_local(hold::POLL, move || {
        let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
        tick(&this)
    });
    let mut a = this.borrow_mut();
    a.poll_gen += 1;
    a.poll = Some(id);
    a.last_e = None;
    // Öffnen im Hilfsthread: der Hauptthread (Overlay, Kürzel) wartet nie auf `open`
    let (open, tx, gen) = (a.open_keys.clone(), a.tx.clone(), a.poll_gen);
    let spawned = std::thread::Builder::new().name("keys-open".into()).spawn(move || {
        let keys = open();
        let _ = tx.send_blocking(Msg::Keys { gen, keys });
    });
    if let Err(e) = spawned {
        warn!("hold: cannot open the keyboards: {e}");
    }
}

/// Eine Abfrage; bis die Geräte offen sind, keine (die Haltelogik zählt nur echte Ergebnisse)
fn sample(this: &Shared) -> Vec<Out> {
    let mut a = this.borrow_mut();
    let Some(k) = a.keys.as_mut() else { return vec![] };
    let e = k.e_down();
    if a.last_e == Some(true) && e == Some(false) {
        info!("hold: E released");
    }
    a.last_e = e;
    a.hold.sample(Instant::now(), e)
}

fn tick(this: &Shared) -> glib::ControlFlow {
    this.borrow_mut().in_tick = true;
    let outs = sample(this);
    apply(this, outs);
    let mut a = this.borrow_mut();
    a.in_tick = false;
    if a.poll.is_some() {
        glib::ControlFlow::Continue
    } else {
        glib::ControlFlow::Break
    }
}

fn stop_polling(this: &Shared) {
    let mut a = this.borrow_mut();
    // Geräte sofort schließen
    if a.keys.take().is_some() {
        debug!("hold: key state closed");
    }
    if let Some(id) = a.poll.take() {
        if !a.in_tick {
            id.remove();
        }
    }
}

// --- Textaktionen ---------------------------------------------------------------------------------------------

/// Mod+I: offen schließt; zu: erst den Fokus merken (Hilfsthread), dann öffnen. Eine offene Diagnose oder Palette
/// schließt nur (ein Panel zur Zeit)
fn actions_key(this: &Shared) {
    if this.borrow().diag.is_open() {
        info!("text actions: the diagnosis is open, closing it");
        let effects = this.borrow_mut().diag.close();
        apply_diag(this, effects);
        return;
    }
    if this.borrow().cmdpal.is_open() {
        info!("text actions: the command palette is open, closing it");
        let effects = this.borrow_mut().cmdpal.close();
        apply_pal(this, effects);
        return;
    }
    let mut refused = false;
    let effects = {
        let mut a = this.borrow_mut();
        if a.panel.is_open() {
            a.panel.close()
        } else if session_locked(&a) {
            info!("text actions: session is locked, not opening");
            refused = true;
            vec![]
        } else if a.opening.is_some() {
            // ein weiteres Kürzel, bevor das Panel offen ist: keins öffnen
            a.open_cancelled = true;
            vec![]
        } else {
            a.opening = Some(Opening::Actions);
            a.open_cancelled = false;
            act::capture_focus(a.tx.clone());
            vec![]
        }
    };
    apply_panel(this, effects);
    if refused {
        // sichtbar machen: hinge der Hinweis von logind, sähe der User sonst nur ein stummes Mod+I
        notice(this, "Text actions: the session is locked".into(), NoticeKind::Info);
    }
}

fn act_msg(this: &Shared, m: ActMsg) {
    let now = Instant::now();
    let effects = {
        let mut a = this.borrow_mut();
        match m {
            ActMsg::Opened(target) => {
                // nur das eigene Öffnen verbrauchen (ein Öffnen der Palette bleibt unberührt)
                let mine = a.opening == Some(Opening::Actions);
                if mine {
                    a.opening = None;
                }
                let go = mine && !std::mem::take(&mut a.open_cancelled) && !session_locked(&a) && !a.diag.is_open() && !a.cmdpal.is_open();
                if go && !a.panel.is_open() {
                    info!("text actions: opening (window {})", if target.window.is_some() { "known" } else { "unknown" });
                    a.panel.toggle(target, now)
                } else {
                    vec![]
                }
            }
            ActMsg::Grabbed { gen, res } => {
                if let Err(e) = &res {
                    info!("text actions: grab failed: {e}");
                }
                a.panel.grab_done(gen, res, now)
            }
            ActMsg::Run { gen, run_gen, ev } => a.panel.run_event(gen, run_gen, ev, now),
            ActMsg::Copied { gen, block, res } => {
                let ok_block = matches!((&res, block), (Ok(_), Some(_))) && gen == a.panel.gen();
                let out = a.panel.copied(gen, block, res);
                if let (true, Some(i)) = (ok_block, block) {
                    let App { panel, panel_ui, .. } = &mut *a;
                    if let Some(ui) = panel_ui.as_mut() {
                        ui.block_copied(i, panel);
                    }
                }
                out
            }
        }
    };
    apply_panel(this, effects);
}

fn panel_ev(this: &Shared, ev: UiEv) {
    // ein Panel zur Zeit: die Eingaben gehören der offenen Diagnose, sonst der offenen Palette, sonst den Textaktionen
    if this.borrow().diag.is_open() {
        return diag_ev(this, ev);
    }
    if this.borrow().cmdpal.is_open() {
        return pal_ev(this, ev);
    }
    let now = Instant::now();
    let effects = {
        let mut a = this.borrow_mut();
        match ev {
            UiEv::Key { name, mods, down: true } => a.panel.key_press(&name, mods, now),
            UiEv::Key { name, down: false, .. } => {
                a.panel.key_release(&name, now);
                vec![]
            }
            UiEv::Activate(id) => {
                debug!("panel: click {id}");
                a.panel.activate(&id, now)
            }
            UiEv::Hover(id) => {
                debug!("panel: hover {}", id.as_deref().unwrap_or("-"));
                a.panel.hover(id.as_deref())
            }
            UiEv::FocusLost => {
                if a.panel.is_open() {
                    info!("text actions: panel lost the focus, closing");
                }
                a.panel.focus_lost()
            }
            UiEv::CloseRequest => a.panel.close(),
            // nur Diagnose und Palette haben ein Eingabefeld (der Text wird beim Freigeben überschrieben)
            UiEv::FieldText(_) | UiEv::FieldFocus | UiEv::PalChanged | UiEv::Vanished(_) => vec![],
            UiEv::Tick => {
                let App { panel, panel_ui, .. } = &mut *a;
                if let Some(ui) = panel_ui.as_mut() {
                    ui.update_secs(panel);
                    ui.fit_pending();
                    if panel.phase == Phase::Result && !ui.copied_shown() {
                        ui.render(panel);
                    }
                }
                vec![]
            }
        }
    };
    apply_panel(this, effects);
}

/// Sitzung gesperrt (oder Schlaf): Panel schließen, Markierung, Antwort und Diagnose im Daemon löschen (Regel 7, wie
/// früher `on_lock` in actions.lua: gelöscht wird auch bei geschlossenem Panel), sofort und nicht hinter einem wartenden
/// Greifen oder Copy. Die Ausgabe wird abgebrochen wie mit Mod+Ctrl+E (ein Replace oder Diktat schriebe sonst unter dem
/// Sperrbildschirm weiter). Ein Mod+I, dessen Fokus gerade gemerkt wird, öffnet danach nicht mehr (sonst läse es
/// PRIMARY hinter dem Sperrbildschirm).
fn locked(this: &Shared, reason: crate::ui::lock::Reason) {
    let effects = {
        let mut a = this.borrow_mut();
        let open = a.panel.is_open();
        info!("session locked ({}): {}clearing the text actions and the diagnosis", reason.name(), if open { "closing the panel, " } else { "" });
        if a.opening.is_some() {
            a.open_cancelled = true;
        }
        a.cancel_output();
        a.act.clear_all_now();
        // `close` bricht eine laufende Aktion ab (`act_cancel`) und verbirgt das Fenster
        a.panel.close()
    };
    apply_panel(this, effects);
    // die Diagnose ebenso (legt einen laufenden Aufruf auf, `diag_clear` ging schon oben)
    let effects = this.borrow_mut().diag.close();
    apply_diag(this, effects);
    // die Palette: schließen legt `pal_resolve` und `pal_ask` auf und bricht eine Aufnahme ab (im Daemon ist nichts zu
    // löschen, `pal_*` hat keine Sitzung)
    let effects = this.borrow_mut().cmdpal.close();
    apply_pal(this, effects);
}

/// Sitzung gerade gesperrt (dann öffnet das Panel nicht, auch nicht über den Steuer-Socket)
fn session_locked(a: &App) -> bool {
    a.logind.as_ref().is_some_and(Logind::locked)
}

/// Folgen des Automaten ausführen, dann den Zeitgeber neu stellen
fn apply_panel(this: &Shared, effects: Vec<Effect>) {
    let mut notices = vec![];
    {
        let mut a = this.borrow_mut();
        let a = &mut *a;
        // vor dem ersten Auftrag: der Thread vergleicht ein Greifen mit dieser Nummer (sonst übersprang er ein neues)
        a.act.set_gen(a.panel.gen());
        for e in effects {
            match e {
                Effect::Grab { gen, source } => {
                    // die Frage nur für ihre Quelle (sonst leer); die Kopie überschreibt sich beim Freigeben
                    let question = if source == TextSource::Question { a.panel.question.clone() } else { String::new() };
                    a.act.grab(gen, a.panel.target.app_id.clone(), source, SecretText::new(question))
                }
                Effect::Chosen => {
                    if let Some((intent, target_id)) = a.act_origin.take() {
                        info!("palette: {intent} succeeded in the text actions panel");
                        a.pal_io.chosen_later(intent, target_id);
                    }
                }
                Effect::Run { gen, run_gen, id, action } => a.act.run(gen, run_gen, id, action),
                Effect::CancelRun { id } => a.act.cancel(id),
                Effect::Clear => a.act.clear(),
                Effect::Copy { gen, id, block } => a.act.copy(gen, id, block),
                Effect::Replace { text, window } => {
                    let chars = text.chars().count() as u64;
                    let id = a.queue.push_replace(text, window);
                    a.replaces.insert(id, chars);
                    info!("text actions: replace queued as output job {id} ({chars} chars)");
                    notices.push((format!("Replacing {}. Mod+Ctrl+E stops", crate::ui::panel::plural(chars, "character", "characters")), NoticeKind::Info));
                    a.pump_output();
                }
                Effect::Show => {
                    // vor dem Zeigen: der Ausgabe-Thread schreibt ab jetzt nichts mehr (Fokus beim Panel)
                    if let Some(o) = &a.out {
                        o.set_panel_visible(true);
                    }
                    if a.panel_ui.is_none() {
                        a.panel_ui = Some(PanelUi::new(a.tx.clone(), a.overlay.palette()));
                    }
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.show(crate::panel::Source::Actions);
                    }
                }
                Effect::Hide => {
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.hide();
                    }
                    // vor einem folgenden `Replace` (gleiche Liste): dessen Auftrag darf schreiben
                    if let Some(o) = &a.out {
                        o.set_panel_visible(false);
                    }
                }
                Effect::Highlight => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.highlight(&a.panel);
                    }
                }
                Effect::Render => {
                    let off = a.logind.as_ref().is_some_and(Logind::off);
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.set_lock_off(off);
                        ui.render(&a.panel);
                    }
                }
                Effect::Stream => {
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.stream(&a.panel);
                    }
                }
                Effect::Scroll(s) => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.scroll(s);
                    }
                }
                Effect::Notice(t) => notices.push((t, NoticeKind::Info)),
                Effect::Settings(page) => open_settings(page),
            }
        }
        a.act.set_gen(a.panel.gen());
        if !a.panel.is_open() {
            a.act_origin = None;
        }
    }
    schedule_panel(this);
    for (t, k) in notices {
        notice(this, t, k);
    }
}

/// Zeitgeber auf `Panel::next_deadline` (Wartezustand zeigen, Leerlauf)
fn schedule_panel(this: &Shared) {
    let mut a = this.borrow_mut();
    if let Some(id) = a.panel_timer.take() {
        id.remove();
    }
    let Some(at) = a.panel.next_deadline() else { return };
    let weak = Rc::downgrade(this);
    let wait = at.saturating_duration_since(Instant::now());
    a.panel_timer = Some(glib::timeout_add_local_once(wait, move || {
        let Some(this) = weak.upgrade() else { return };
        // abgelaufen: nur vergessen (ein `remove` darauf wäre ein Fehler)
        this.borrow_mut().panel_timer = None;
        let effects = this.borrow_mut().panel.tick(Instant::now());
        apply_panel(&this, effects);
    }));
}

/// Einstellungsfenster auf einer Seite (argv, keine Shell); der Prozess wird eingesammelt
fn open_settings(page: &'static str) {
    match std::process::Command::new("dictate-settings").args(["--page", page]).spawn() {
        Ok(mut child) => {
            let _ = std::thread::Builder::new().name("settings-wait".into()).spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => warn!("cannot start dictate-settings: {e}"),
    }
}

// --- Diagnose -------------------------------------------------------------------------------------------------

/// Zwischenzeile oder Antwort des Daemons an den Automaten; der gelesene Wert wird danach überschrieben (`Wiped`)
fn diag_msg(this: &Shared, m: DiagMsg) {
    let now = Instant::now();
    let effects = {
        let mut a = this.borrow_mut();
        match m {
            DiagMsg::Progress { gen, seq, mut line } => a.diag.progress_line(gen, seq, line.value_mut()),
            DiagMsg::Reply { gen, seq, res } => match res {
                Ok(mut v) => a.diag.reply(gen, seq, Ok(v.value_mut()), now),
                Err(e) => a.diag.reply(gen, seq, Err(e), now),
            },
        }
    };
    apply_diag(this, effects);
}

/// Folgen des Diagnose-Automaten ausführen, dann den Zeitgeber neu stellen
fn apply_diag(this: &Shared, effects: Vec<diag_panel::Effect>) {
    use diag_panel::Effect as E;
    let mut notices = vec![];
    {
        let mut a = this.borrow_mut();
        let a = &mut *a;
        // vor dem ersten Auftrag: der Thread vergleicht jeden Aufruf mit dieser Nummer
        a.diag_io.set_gen(a.diag.gen());
        for e in effects {
            match e {
                E::Call { gen, seq, call } => a.diag_io.call(gen, seq, call),
                E::Hangup { seq } => a.diag_io.hangup(seq),
                E::Notice(t) => notices.push((t, NoticeKind::Info)),
                E::Show => {
                    // vor dem Zeigen: der Ausgabe-Thread schreibt ab jetzt nichts mehr (Fokus beim Panel)
                    if let Some(o) = &a.out {
                        o.set_panel_visible(true);
                    }
                    if a.panel_ui.is_none() {
                        a.panel_ui = Some(PanelUi::new(a.tx.clone(), a.overlay.palette()));
                    }
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.show(crate::panel::Source::Diagnose);
                    }
                }
                E::Hide => {
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.hide();
                    }
                    if let Some(o) = &a.out {
                        o.set_panel_visible(false);
                    }
                }
                E::Highlight => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.highlight_active(a.diag.active.as_deref());
                    }
                }
                E::Scroll(s) => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.scroll(s);
                    }
                }
                E::Render => {
                    let off = a.logind.as_ref().is_some_and(Logind::off);
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.set_lock_off(off);
                        ui.render_diag(&mut a.diag);
                    }
                }
                E::Field(on) => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.field_focus(on);
                    }
                }
                E::Chosen => {
                    if let Some((intent, target_id)) = a.diag_origin.take() {
                        info!("palette: {intent} succeeded in the diagnosis panel");
                        a.pal_io.chosen_later(intent, target_id);
                    }
                }
            }
        }
        a.diag_io.set_gen(a.diag.gen());
        if !a.diag.is_open() {
            a.diag_origin = None;
        }
    }
    schedule_diag(this);
    for (t, k) in notices {
        notice(this, t, k);
    }
}

/// Mod+Shift+I (`dictate-ui diagnose`): offene Diagnose schließt; ein offenes (oder sich öffnendes) Panel der
/// Textaktionen schließt nur (wie unter Awesome); zu: öffnen, außer die Sitzung ist gesperrt
fn diagnose_key(this: &Shared) {
    let now = Instant::now();
    let mut refused = false;
    let (pe, de) = {
        let mut a = this.borrow_mut();
        if a.diag.is_open() {
            (vec![], a.diag.close())
        } else if a.panel.is_open() {
            info!("diagnose: the text actions panel is open, closing it");
            (a.panel.close(), vec![])
        } else if a.cmdpal.is_open() {
            info!("diagnose: the command palette is open, closing it");
            let pe = a.cmdpal.close();
            drop(a);
            apply_pal(this, pe);
            return;
        } else if a.opening.is_some() {
            a.open_cancelled = true;
            (vec![], vec![])
        } else if session_locked(&a) {
            info!("diagnose: session is locked, not opening");
            refused = true;
            (vec![], vec![])
        } else {
            info!("diagnose: opening");
            if a.boot.end() {
                info!("boot hint: ended (diagnosis opened)");
            }
            (vec![], a.diag.toggle(now))
        }
    };
    apply_panel(this, pe);
    apply_diag(this, de);
    if refused {
        notice(this, "Diagnose: the session is locked".into(), NoticeKind::Info);
    }
}

/// Eingaben des Fensters, solange die Diagnose offen ist
fn diag_ev(this: &Shared, ev: UiEv) {
    let now = Instant::now();
    let effects = {
        let mut a = this.borrow_mut();
        let a = &mut *a;
        match ev {
            UiEv::Key { name, mods, down: true } => a.diag.key_press(&name, mods, now),
            UiEv::Key { name, down: false, .. } => {
                a.diag.key_release(&name, now);
                vec![]
            }
            // Kennungen nennen nur Indizes (`unit:3`, `ev:1:2`, `sec:S4`)
            UiEv::Activate(id) => {
                debug!("diagnose: click {id}");
                a.diag.activate(&id, now)
            }
            UiEv::Hover(id) => a.diag.hover(id.as_deref()),
            UiEv::FocusLost => {
                info!("diagnose: panel lost the focus");
                a.diag.focus_lost()
            }
            UiEv::CloseRequest => a.diag.close(),
            UiEv::FieldText(t) => {
                debug!("diagnose: field text ({} bytes)", t.as_str().len());
                a.diag.field_text(t.as_str().to_string(), now)
            }
            UiEv::FieldFocus => a.diag.field_focused(now),
            UiEv::PalChanged | UiEv::Vanished(_) => vec![],
            UiEv::Tick => {
                if let Some(ui) = a.panel_ui.as_mut() {
                    ui.update_diag_secs(&a.diag);
                    ui.fit_pending();
                }
                vec![]
            }
        }
    };
    apply_diag(this, effects);
}

/// Zeitgeber auf `DiagPanel::next_deadline` (Laden zeigen, Leerlauf)
fn schedule_diag(this: &Shared) {
    let mut a = this.borrow_mut();
    if let Some(id) = a.diag_timer.take() {
        id.remove();
    }
    let Some(at) = a.diag.next_deadline() else { return };
    let weak = Rc::downgrade(this);
    let wait = at.saturating_duration_since(Instant::now());
    a.diag_timer = Some(glib::timeout_add_local_once(wait, move || {
        let Some(this) = weak.upgrade() else { return };
        // abgelaufen: nur vergessen (ein `remove` darauf wäre ein Fehler)
        this.borrow_mut().diag_timer = None;
        let effects = this.borrow_mut().diag.tick(Instant::now());
        apply_diag(&this, effects);
    }));
}

// --- Befehlspalette -------------------------------------------------------------------------------------------

/// Mod+Alt+Space (`dictate-ui palette`): offene Palette schließt; ein anderes offenes (oder sich öffnendes) Panel
/// schließt nur (ein Panel zur Zeit); zu: Fokus und Kontext im Hilfsthread holen, dann öffnen, außer die Sitzung ist
/// gesperrt
fn palette_key(this: &Shared) {
    let mut refused = false;
    let (pe, de, ce) = {
        let mut a = this.borrow_mut();
        if a.cmdpal.is_open() {
            (vec![], vec![], a.cmdpal.close())
        } else if a.diag.is_open() {
            info!("palette: the diagnosis is open, closing it");
            (vec![], a.diag.close(), vec![])
        } else if a.panel.is_open() {
            info!("palette: the text actions panel is open, closing it");
            (a.panel.close(), vec![], vec![])
        } else if a.opening.is_some() {
            // ein weiteres Kürzel, bevor das Panel offen ist: keins öffnen
            a.open_cancelled = true;
            (vec![], vec![], vec![])
        } else if session_locked(&a) {
            info!("palette: session is locked, not opening");
            refused = true;
            (vec![], vec![], vec![])
        } else {
            a.opening = Some(Opening::Palette);
            a.open_cancelled = false;
            a.pal_io.open();
            (vec![], vec![], vec![])
        }
    };
    apply_panel(this, pe);
    apply_diag(this, de);
    apply_pal(this, ce);
    if refused {
        notice(this, "Command palette: the session is locked".into(), NoticeKind::Info);
    }
}

/// Nachrichten aus `pal.rs`: Öffnen, Antworten des Daemons (danach überschrieben, `Wiped`), Ergebnis der Ausführung
fn pal_msg(this: &Shared, m: PalMsg) {
    let now = Instant::now();
    let mut note = None;
    let mut handoff = None;
    let effects = {
        let mut a = this.borrow_mut();
        match m {
            PalMsg::Opened { prior_focus, context } => {
                // nur das eigene Öffnen verbrauchen; eine Sperre oder ein anderes Kürzel dazwischen: der Kontext fällt
                // ungenutzt (`SecretText` überschreibt ihn)
                let mine = a.opening == Some(Opening::Palette);
                if mine {
                    a.opening = None;
                }
                let go = mine && !std::mem::take(&mut a.open_cancelled) && !session_locked(&a) && !a.diag.is_open() && !a.panel.is_open();
                if go && !a.cmdpal.is_open() {
                    info!(
                        "palette: opening (window {}, context {})",
                        if prior_focus.is_some() { "known" } else { "unknown" },
                        if context.is_some() { "known" } else { "missing" }
                    );
                    a.pal_focus = prior_focus.as_ref().map(|f| f.0);
                    a.pal_app = prior_focus.and_then(|f| f.1);
                    a.pal_ctx = context;
                    a.cmdpal.open(now)
                } else {
                    vec![]
                }
            }
            PalMsg::Reply { lane, gen, seq, res } => match (lane, res) {
                (Lane::Resolve, Ok(mut v)) => a.cmdpal.resolve_reply(gen, seq, Ok(&*v.value_mut()), now),
                (Lane::Resolve, Err(e)) => a.cmdpal.resolve_reply(gen, seq, Err(e), now),
                (Lane::Ask, Ok(mut v)) => a.cmdpal.ask_reply(gen, seq, Ok(&*v.value_mut()), now),
                (Lane::Ask, Err(e)) => a.cmdpal.ask_reply(gen, seq, Err(e), now),
            },
            // niri nahm an: ein Abgang schließt jetzt (Entscheidung 17), sonst nichts
            PalMsg::Accepted { run } => a.cmdpal.ran(run),
            PalMsg::Executed { run, intent, outcome, origin } => {
                // ins Log nur Absicht und Art (die Meldung kann Namen tragen)
                info!("palette: {intent} {}", outcome.kind());
                note = match outcome {
                    Outcome::Done(None) => None,
                    Outcome::Done(Some(n)) => Some((n.to_string(), NoticeKind::Info)),
                    Outcome::Failed(m) => Some((format!("Command palette: {m}"), NoticeKind::Error)),
                    Outcome::Handoff(h) => {
                        handoff = origin.map(|o| (intent, h, o));
                        None
                    }
                };
                // ein Abgang, dessen Annahme nicht kam (Fehler): jetzt schließen
                a.cmdpal.ran(run)
            }
        }
    };
    apply_pal(this, effects);
    if let Some((intent, h, origin)) = handoff {
        pal_handoff(this, intent, h, origin);
    }
    if let Some((t, k)) = note {
        notice(this, t, k);
    }
}

/// Übergabe aus der Palette (`ask`, `text_action`, `explain_clipboard`, `diagnose`): öffnet im selben Fenster die
/// passende Quelle des Panels der Textaktionen bzw. die Diagnose mit Vorbelegung (PLAN-wayland.md 6g, „Weiter an die
/// anderen Panels“). Die Palette ist da schon zu; dieselben Sperren wie beim Kürzel (Sperre, ein anderes Öffnen oder
/// Panel dazwischen): dann verfällt die Übergabe ohne `pal_chosen`. Frage, Suchbegriffe und Ziel-ID nie loggen
fn pal_handoff(this: &Shared, intent: &'static str, h: Handoff, origin: Origin) {
    let now = Instant::now();
    let pe = {
        let mut a = this.borrow_mut();
        if session_locked(&a) || a.opening.is_some() || a.panel.is_open() || a.diag.is_open() || a.cmdpal.is_open() {
            info!("palette: {intent} not handed over (lock, shortcut or another panel came first)");
            return;
        }
        let Origin { window, app_id, target_id } = origin;
        let target = crate::ui::panel::Target { window, app_id };
        let (source, preselect, question) = match h {
            Handoff::Ask { question } => (TextSource::Question, None, Some(question)),
            Handoff::TextAction { action } => (TextSource::Selection, Some(action), None),
            Handoff::ExplainClipboard => (TextSource::Clipboard, None, None),
            Handoff::Diagnose { profile, unit, container } => {
                info!("palette: {intent} handed over to the diagnosis panel");
                if a.boot.end() {
                    info!("boot hint: ended (diagnosis opened)");
                }
                a.diag_origin = Some((intent, target_id));
                let preset = diag_panel::preset_of(profile, unit.as_deref(), container.as_deref());
                let de = a.diag.open_from_palette(preset, now);
                drop(a);
                apply_diag(this, de);
                return;
            }
        };
        info!("palette: {intent} handed over to the text actions panel ({})", source.as_str());
        a.act_origin = Some((intent, target_id));
        a.panel.open_with(source, preselect, question, target, now)
    };
    apply_panel(this, pe);
}

/// Nimmt die Palette eine Aufnahme an? Offen und nicht im Abgang (Entscheidung 17: das unsichtbare Panel nimmt keine)
fn pal_takes_voice(a: &App) -> bool {
    a.cmdpal.is_open() && !a.cmdpal.leaving()
}

/// Folgen des Automaten der Palette ausführen, dann den Zeitgeber neu stellen
fn apply_pal(this: &Shared, effects: Vec<cmdpal::Effect>) {
    use cmdpal::Effect as E;
    let mut cancel_voice = false;
    {
        let mut a = this.borrow_mut();
        let a = &mut *a;
        // vor dem ersten Auftrag: die Bahnen vergleichen jeden Auftrag mit dieser Nummer (`open` zählt sie hoch und
        // fragt in derselben Liste)
        a.pal_io.set_gen(a.cmdpal.gen());
        for e in effects {
            match e {
                E::Show => {
                    // vor dem Zeigen: der Ausgabe-Thread schreibt ab jetzt nichts mehr (Fokus beim Panel)
                    if let Some(o) = &a.out {
                        o.set_panel_visible(true);
                    }
                    if a.panel_ui.is_none() {
                        a.panel_ui = Some(PanelUi::new(a.tx.clone(), a.overlay.palette()));
                    }
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.show(crate::panel::Source::Palette);
                    }
                }
                E::Hide => {
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.hide();
                    }
                    if let Some(o) = &a.out {
                        o.set_panel_visible(false);
                    }
                    // Kontext (Fenstertitel) überschreiben; der Fokus bleibt für ein folgendes `Execute`
                    a.pal_ctx = None;
                    // auch eine beanspruchte, noch nicht begonnene Aufnahme (der Automat kennt sie noch nicht)
                    cancel_voice = true;
                }
                E::Render => {
                    let off = a.logind.as_ref().is_some_and(Logind::off);
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.set_lock_off(off);
                        ui.render_pal(&a.cmdpal, Instant::now());
                    }
                }
                E::Highlight => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.highlight_pal(&a.cmdpal);
                    }
                }
                E::VoiceText => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.pal_voice_text(&a.cmdpal);
                    }
                }
                E::SetInput(mut t) => {
                    if let Some(ui) = a.panel_ui.as_ref() {
                        ui.set_pal_text(&t);
                    }
                    wipe(&mut t);
                }
                E::Resolve { gen, seq, input } => a.pal_io.request(Lane::Resolve, gen, seq, input, a.pal_ctx.clone()),
                E::Ask { gen, seq, input } => a.pal_io.request(Lane::Ask, gen, seq, input, a.pal_ctx.clone()),
                E::Hangup { lane, seq } => a.pal_io.hangup(lane, seq),
                E::Execute { cand, confirmed, prefs, via, run, late } => {
                    info!(
                        "palette: run {} ({}{}{})",
                        cand.intent.name(),
                        if via == cmdpal::Via::Key { "key" } else { "mouse" },
                        if confirmed { ", confirmed" } else { "" },
                        if late { ", panel not repainted in time" } else { "" }
                    );
                    let ctx = ExecCtx { prior_focus: a.pal_focus, prefs, confirmed };
                    a.pal_io.execute(run, *cand, ctx, a.pal_app.clone());
                }
                // Abgang (Entscheidung 17): Panel durchsichtig, die Hülle meldet `UiEv::Vanished`
                E::Vanish { gen } => {
                    if let Some(ui) = a.panel_ui.as_mut() {
                        ui.vanish(gen);
                    }
                }
                // Esc während der Aufnahme oder Schließen: abbrechen wie Mod+Ctrl+E, aber ohne die Ausgabe zu leeren
                E::VoiceCancel => cancel_voice = true,
            }
        }
        a.pal_io.set_gen(a.cmdpal.gen());
        if !a.cmdpal.is_open() {
            a.pal_focus = None;
            a.pal_app = None;
        }
    }
    // erst nach dem Block (`apply` leiht selbst): gab es eine Aufnahme der Palette, `cancel` (nach dem `start` der
    // Reihe nach) und das Halten beenden; ihr `final` wird verworfen
    if cancel_voice {
        let outs = {
            let mut a = this.borrow_mut();
            if a.voice.cancel() {
                info!("palette: recording cancelled");
                a.hold.cancel()
            } else {
                vec![]
            }
        };
        apply(this, outs);
        render(this);
    }
    schedule_pal(this);
}

/// Eingaben des Fensters, solange die Palette offen ist
fn pal_ev(this: &Shared, ev: UiEv) {
    let now = Instant::now();
    let effects = {
        let mut a = this.borrow_mut();
        let a = &mut *a;
        match ev {
            UiEv::Key { name, mods, down: true } => a.cmdpal.key_press(&name, mods, now),
            UiEv::Key { name, down: false, .. } => {
                a.cmdpal.key_release(&name, now);
                vec![]
            }
            // Kennungen nennen nur Indizes (`row:3`)
            UiEv::Activate(id) => {
                debug!("palette: click {id}");
                a.cmdpal.activate(&id, now)
            }
            UiEv::Hover(id) => a.cmdpal.hover(id.as_deref()),
            UiEv::FocusLost => {
                info!("palette: panel lost the focus, closing");
                a.cmdpal.focus_lost()
            }
            UiEv::CloseRequest => a.cmdpal.close(),
            UiEv::Vanished(gen) => a.cmdpal.vanished(gen, now),
            UiEv::PalChanged => match a.panel_ui.as_ref() {
                // erst hier lesen: ein `SetInput` davor ist schon in der Zeile
                Some(ui) => {
                    let mut text = ui.pal_entry.text().to_string();
                    let out = a.cmdpal.text_changed(&text, now);
                    wipe(&mut text);
                    out
                }
                None => vec![],
            },
            UiEv::FieldText(_) | UiEv::FieldFocus => vec![],
            UiEv::Tick => {
                if let Some(ui) = a.panel_ui.as_mut() {
                    ui.update_pal_secs(&a.cmdpal, now);
                    ui.fit_pending();
                }
                vec![]
            }
        }
    };
    apply_pal(this, effects);
}

/// Zeitgeber auf `CmdPal::next_deadline` (Leerlauf)
fn schedule_pal(this: &Shared) {
    let mut a = this.borrow_mut();
    if let Some(id) = a.pal_timer.take() {
        id.remove();
    }
    let Some(at) = a.cmdpal.next_deadline() else { return };
    let weak = Rc::downgrade(this);
    let wait = at.saturating_duration_since(Instant::now());
    a.pal_timer = Some(glib::timeout_add_local_once(wait, move || {
        let Some(this) = weak.upgrade() else { return };
        // abgelaufen: nur vergessen (ein `remove` darauf wäre ein Fehler)
        this.borrow_mut().pal_timer = None;
        let effects = this.borrow_mut().cmdpal.tick(Instant::now());
        apply_pal(&this, effects);
    }));
}
