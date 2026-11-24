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

//! Ausgabe-Thread von `dictate-ui` (PLAN-wayland.md 3.4, Block W2): führt die Aufträge der Warteschlange
//! (`ui::output::Queue`, Hauptthread) der Reihe nach aus. Weg je Auftrag nach `ui::output::route`: Eingabemethode
//! (T3), Einfügen über die Zwischenablage (`clip`) oder eigene virtuelle Tastatur (T2).
//!
//! Vor jedem Auftrag wird gewartet, bis Ctrl/Shift/Alt/Super losgelassen sind (Regel 2; Tastenzustand wie beim Halten
//! nur über `EVIOCGKEY`, die Geräte sind nur dafür kurz offen). Ist der Zustand nicht lesbar, wird nie eingefügt
//! (`Ctrl+V` braucht ihn), aber getippt: die virtuelle Tastatur hat ihren eigenen Modifikator-Zustand und löst keine
//! niri-Kürzel aus (S2, gemessen).
//!
//! Abbruch (Mod+Ctrl+E) über eine gemeinsame Nummer ([`Control`]): der laufende Auftrag hört nach dem aktuellen
//! Anschlag auf; ein Einfügen nach dem `Ctrl+V` legt erst nach dem Abholen zurück (Befund 2). Beim Beenden lässt die
//! virtuelle Tastatur alles los, bevor sie zerstört wird, und die Eingabemethode wird zerstört (Regeln 3 und 9).
//!
//! Ins Log nur Wege, Zahlen und Zeiten, nie Text.

use crate::actions::store::SecretText;
use crate::clip::{self, CtrlV, Outcome};
use crate::link::Msg;
use crate::niri;
use crate::ui::keymap::{self, Keymap, Stroke};
use crate::ui::keys::KeySource;
use crate::ui::output::{self, Job, OutCfg, Piece, Route, Target};
use crate::ui::view::NoticeKind;
use crate::wl::{poll_fds, Vkbd, Wl};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Höchstgröße eines `commit_string` in Bytes (eine Wayland-Nachricht hat höchstens 4096 Bytes samt Kopf)
const COMMIT_BYTES: usize = 2000;
/// Wie lange nach dem Anlegen der Eingabemethode auf ihr erstes `done` gewartet wird (meist schon beim Abschließen
/// angelegt, `Cmd::Arm`: dann ist die Zeit beim `final` schon um)
const IM_SETTLE: Duration = Duration::from_millis(500);
/// Nach so langer Wartezeit auf losgelassene Modifikatoren ein Hinweis im Overlay
const MODS_HINT_AFTER: Duration = Duration::from_millis(1500);
/// Danach wird der Auftrag aufgegeben (mit Hinweis), statt endlos zu warten
const MODS_TIMEOUT: Duration = Duration::from_secs(30);
/// Takt der Abfrage des Tastenzustands
const MODS_POLL: Duration = Duration::from_millis(20);
/// Eine angelegte Eingabemethode ohne Auftrag wird nach dieser Zeit zerstört (falls ein `Disarm` fehlt)
const IM_IDLE: Duration = Duration::from_secs(10);
/// Abstand der Versuche, nach einem Wayland-Fehler neu zu verbinden
const RECONNECT: Duration = Duration::from_secs(2);
/// Höchstens so lange wartet die Ausgabe, bis das Feld ein `commit` bzw. eine Taste gemeldet hat, bevor sie auf den
/// anderen Weg wechselt (`Wl::im_wait_before`)
const IM_SYNC: Duration = Duration::from_millis(500);
/// Replace: so lange wartet die Ausgabe, bis niri den Fokus nach dem Schließen des Panels zurückgegeben hat
const FOCUS_RETURN: Duration = Duration::from_millis(1500);
/// Meldung, wenn sich das Ziel während der Ausgabe ändert
const FOCUS_MOVED: &str = "the focused window changed, output stopped";
/// Meldung, wenn das Panel der Textaktionen offen ist (es hätte den Fokus)
const PANEL_OPEN: &str = "the text actions panel has the focus, text not written";

pub type OpenKeys = Arc<dyn Fn() -> Box<dyn KeySource> + Send + Sync>;

/// Befehle an den Ausgabe-Thread
pub enum Cmd {
    Job(Job, OutCfg),
    /// Eingabemethode schon anlegen (Daemon schließt ab: `activate` liegt dann beim `final` vor)
    Arm,
    /// Eingabemethode zerstören (nichts mehr auszugeben)
    Disarm,
    /// Markierung (PRIMARY) lesen für die Textaktionen (`primary.rs`); zwischen zwei Aufträgen ausgeführt, nur wenn
    /// der Anspruch noch im Slot liegt
    ReadPrimary(Claim<()>, std::sync::mpsc::Sender<Result<SecretText, String>>),
    /// CLIPBOARD als Text lesen, nur für die Absicht `explain_clipboard` der Palette (`clip::read_clipboard_text`,
    /// Lesart L8); wie `ReadPrimary` zwischen zwei Aufträgen und nur mit Anspruch
    ReadClipboard(Claim<()>, std::sync::mpsc::Sender<Result<SecretText, String>>),
    /// Text in CLIPBOARD legen (Copy der Textaktionen, eigene Quelle, die `dictate-ui` weiter bedient); der Text liegt
    /// im Slot
    Copy(Claim<SecretText>, std::sync::mpsc::Sender<Result<(), String>>),
}

/// Anspruch auf einen Befehl mit Frist: wer den Inhalt zuerst aus dem Slot nimmt, entscheidet. Der Ausgabe-Thread führt
/// nur aus, was er dort noch findet; der Aufrufer gibt nach Ablauf der Frist nur auf, wenn er ihn selbst herausnimmt,
/// sonst wartet er auf das (dann schon laufende) Ergebnis. So stimmen Rückmeldung und Wirkung immer überein: ein als
/// gescheitert gemeldetes Copy setzt CLIPBOARD nie später doch noch (Durchsicht W3 M1)
pub type Claim<T> = Arc<std::sync::Mutex<Option<T>>>;

fn claim<T>(c: &Claim<T>) -> Option<T> {
    c.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Sendeseite zum Ausgabe-Thread; klonbar für andere Threads (Textaktionen)
#[derive(Clone)]
pub struct OutTx {
    tx: std::sync::mpsc::Sender<Cmd>,
    wake: Arc<OwnedFd>,
}

impl OutTx {
    fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
        let one: u64 = 1;
        // SAFETY: eventfd, 8 Bytes
        unsafe { libc::write(self.wake.as_raw_fd(), (&one as *const u64).cast(), 8) };
    }

    /// Befehl mit Anspruch (`Claim`) schicken und höchstens `max` auf den Beginn warten
    fn claimed<T, R>(&self, payload: T, make: impl FnOnce(Claim<T>, std::sync::mpsc::Sender<R>) -> Cmd, max: Duration, busy: R) -> R {
        let slot: Claim<T> = Arc::new(std::sync::Mutex::new(Some(payload)));
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(make(slot.clone(), tx));
        match rx.recv_timeout(max) {
            Ok(r) => r,
            Err(_) => match claim(&slot) {
                // noch nicht begonnen: aufgeben (der Inhalt wird hier verworfen, der Thread überspringt den Befehl)
                Some(_) => busy,
                // schon begonnen: das Ergebnis kommt gleich
                None => rx.recv().unwrap_or(busy),
            },
        }
    }

    /// Markierung lesen; wartet höchstens `max` (ein laufender Auftrag der Ausgabe geht vor)
    pub fn read_primary(&self, max: Duration) -> Result<SecretText, String> {
        self.claimed((), Cmd::ReadPrimary, max, Err("Cannot read the selection: the text output is busy. Try again.".into()))
    }

    /// CLIPBOARD lesen, nur für `explain_clipboard` (Aufrufer allein `act.rs`, Test `sources_stay_apart`); wartet
    /// höchstens `max` wie [`OutTx::read_primary`]
    pub fn read_clipboard(&self, max: Duration) -> Result<SecretText, String> {
        self.claimed((), Cmd::ReadClipboard, max, Err("Cannot read the clipboard: the text output is busy. Try again.".into()))
    }

    /// Text in CLIPBOARD legen (wartet höchstens `max`, ein laufender Auftrag geht vor)
    pub fn copy(&self, text: SecretText, max: Duration) -> Result<(), String> {
        self.claimed(text, Cmd::Copy, max, Err("the text output is busy".into()))
    }
}

/// Antwort auf einen Befehl, den der Ausgabe-Thread ohne Wayland-Verbindung nicht ausführen kann
fn refuse(c: Cmd, why: &str, back: &async_channel::Sender<Msg>) -> bool {
    match c {
        Cmd::Job(job, _) => {
            let done = Done::Failed(why.to_string());
            info!("output {}: {done:?}", job.describe());
            back.send_blocking(Msg::OutputDone { id: job.id, done }).is_ok()
        }
        Cmd::ReadPrimary(slot, reply) => {
            if claim(&slot).is_some() {
                let _ = reply.send(Err(format!("Cannot read the selection: {why}")));
            }
            true
        }
        Cmd::ReadClipboard(slot, reply) => {
            if claim(&slot).is_some() {
                let _ = reply.send(Err(format!("Cannot read the clipboard: {why}")));
            }
            true
        }
        Cmd::Copy(slot, reply) => {
            if claim(&slot).is_some() {
                let _ = reply.send(Err(why.to_string()));
            }
            true
        }
        Cmd::Arm | Cmd::Disarm => true,
    }
}

/// Ergebnis eines Auftrags (für Hinweise und die Warteschlange)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Written,
    Unconfirmed,
    Stopped,
    Failed(String),
}

/// Gemeinsamer Zustand zwischen Hauptthread und Ausgabe-Thread
#[derive(Default)]
pub struct Control {
    /// Aufträge mit einer Nummer bis hierhin sind abgebrochen
    cancel_upto: AtomicU64,
    quit: AtomicBool,
    /// Das Panel der Textaktionen ist sichtbar (setzt der Hauptthread bei Zeigen und Verbergen): dann wird nichts
    /// geschrieben, auch wenn niri die `app_id` nicht liefert; getippte Buchstaben wären dort Aktionstasten
    /// (Durchsicht W3 N1)
    panel_visible: AtomicBool,
}

/// Griff des Hauptthreads
pub struct Output {
    tx: OutTx,
    ctl: Arc<Control>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Output {
    pub fn spawn(back: async_channel::Sender<Msg>, open_keys: OpenKeys) -> Result<Output, String> {
        // SAFETY: eventfd ohne Vorbedingungen
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(format!("eventfd: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: frisch angelegt
        let wake = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
        let (tx, rx) = std::sync::mpsc::channel();
        let ctl = Arc::new(Control::default());
        let (w2, c2) = (wake.clone(), ctl.clone());
        let thread = std::thread::Builder::new()
            .name("output".into())
            .spawn(move || match Wl::connect() {
                Ok(wl) => Worker { wl, rx, wake: w2, ctl: c2, back, open_keys, im_since: None }.run(),
                Err(e) => {
                    warn!("text output unavailable: {e}");
                    // Aufträge trotzdem beantworten, damit Warteschlange und Panel nicht stehen
                    let why = format!("text output unavailable: {e}");
                    for c in rx {
                        if !refuse(c, &why, &back) {
                            break;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Output { tx: OutTx { tx, wake }, ctl, thread: Some(thread) })
    }

    fn send(&self, c: Cmd) {
        self.tx.send(c);
    }

    /// Sendeseite für andere Threads
    pub fn handle(&self) -> OutTx {
        self.tx.clone()
    }

    pub fn job(&self, job: Job, cfg: OutCfg) {
        self.send(Cmd::Job(job, cfg));
    }

    pub fn arm(&self) {
        self.send(Cmd::Arm);
    }

    pub fn disarm(&self) {
        self.send(Cmd::Disarm);
    }

    /// Panel der Textaktionen sichtbar bzw. verborgen
    pub fn set_panel_visible(&self, on: bool) {
        self.ctl.panel_visible.store(on, Ordering::SeqCst);
    }

    /// Aufträge bis Nummer `id` abbrechen
    pub fn cancel(&self, id: u64) {
        self.ctl.cancel_upto.fetch_max(id, Ordering::SeqCst);
    }

    /// Beenden: laufenden Auftrag abbrechen, Tasten loslassen, Eingabemethode zerstören; wartet höchstens 3 s
    pub fn shutdown(mut self) {
        self.ctl.quit.store(true, Ordering::SeqCst);
        self.send(Cmd::Disarm);
        if let Some(t) = self.thread.take() {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            } else {
                warn!("output thread did not end in time");
            }
        }
    }
}

struct Worker {
    wl: Wl,
    rx: std::sync::mpsc::Receiver<Cmd>,
    wake: Arc<OwnedFd>,
    ctl: Arc<Control>,
    back: async_channel::Sender<Msg>,
    open_keys: OpenKeys,
    /// Seit wann die Eingabemethode ohne Auftrag besteht
    im_since: Option<Instant>,
}

impl Worker {
    fn quitting(&self) -> bool {
        self.ctl.quit.load(Ordering::SeqCst)
    }

    fn run(mut self) {
        info!("text output ready");
        loop {
            if self.quitting() {
                break;
            }
            // Befehle abholen
            let mut eventfd_buf = [0u8; 8];
            // SAFETY: eventfd, 8 Bytes, nicht blockierend
            unsafe { libc::read(self.wake.as_raw_fd(), eventfd_buf.as_mut_ptr().cast(), 8) };
            loop {
                match self.rx.try_recv() {
                    Ok(Cmd::Arm) => {
                        self.wl.im_arm();
                        self.im_since = Some(Instant::now());
                    }
                    Ok(Cmd::Disarm) => {
                        self.wl.im_disarm();
                        self.im_since = None;
                    }
                    Ok(Cmd::ReadPrimary(slot, reply)) => {
                        if claim(&slot).is_none() {
                            debug!("primary: request given up before it started, not read");
                            continue;
                        }
                        let t = Instant::now();
                        let r = crate::primary::read_primary(&mut self.wl);
                        debug!("primary read in {} ms ({})", t.elapsed().as_millis(), if r.is_ok() { "ok" } else { "failed" });
                        let _ = reply.send(r);
                    }
                    Ok(Cmd::ReadClipboard(slot, reply)) => {
                        if claim(&slot).is_none() {
                            debug!("clipboard: request given up before it started, not read");
                            continue;
                        }
                        let t = Instant::now();
                        let r = crate::clip::read_clipboard_text(&mut self.wl);
                        debug!("clipboard read in {} ms ({})", t.elapsed().as_millis(), if r.is_ok() { "ok" } else { "failed" });
                        let _ = reply.send(r);
                    }
                    Ok(Cmd::Copy(slot, reply)) => {
                        let Some(text) = claim(&slot) else {
                            debug!("copy: given up before it started, clipboard untouched");
                            continue;
                        };
                        let _ = reply.send(clip::copy(&mut self.wl, text));
                    }
                    Ok(Cmd::Job(job, cfg)) => {
                        let done = self.job(&job, &cfg);
                        info!("output {}: {done:?}", job.describe());
                        if self.back.send_blocking(Msg::OutputDone { id: job.id, done }).is_err() {
                            return self.finish();
                        }
                        if self.wl.has_im() {
                            self.im_since = Some(Instant::now());
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return self.finish(),
                }
                if self.quitting() {
                    return self.finish();
                }
            }
            if self.im_since.is_some_and(|t| t.elapsed() >= IM_IDLE) {
                debug!("input method idle: destroyed");
                self.wl.im_disarm();
                self.im_since = None;
            }
            // Meldungen an die eigenen Quellen brauchen nur die laufende Ausgabe
            self.wl.st.sends.clear();
            self.wl.st.cancelled.clear();
            // Warten auf Wayland (Zwischenablage bedienen) oder neue Befehle
            if let Err(e) = self.idle_wait(Duration::from_secs(1)) {
                warn!("text output: {e}; reconnecting");
                if !self.reconnect() {
                    return;
                }
            }
        }
        self.finish();
    }

    /// Nach einem Wayland-Fehler: Aufträge sofort als gescheitert beantworten (die Warteschlange steht nie), alle
    /// [`RECONNECT`] neu verbinden. `false`: beim Beenden aufgegeben. Eine zurückgelegte Zwischenablage ist mit der
    /// alten Verbindung verloren.
    fn reconnect(&mut self) -> bool {
        let _ = self.back.send_blocking(Msg::OutputNotice("Dictation: text output lost the Wayland connection".into(), NoticeKind::Error));
        loop {
            let deadline = Instant::now() + RECONNECT;
            while Instant::now() < deadline {
                if self.quitting() {
                    return false;
                }
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(c) => {
                        if !refuse(c, "text output has no Wayland connection", &self.back) {
                            return false;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return false,
                }
            }
            match Wl::connect() {
                Ok(wl) => {
                    self.wl = wl;
                    self.im_since = None;
                    info!("text output reconnected");
                    return true;
                }
                Err(e) => debug!("text output: reconnect failed: {e}"),
            }
        }
    }

    fn finish(mut self) {
        self.wl.im_disarm();
        let _ = self.wl.flush();
        info!("text output ended");
    }

    /// Bis zu `max` auf Ereignisse oder das Wecken warten, dabei verteilen
    fn idle_wait(&mut self, max: Duration) -> Result<(), String> {
        self.wl.flush()?;
        let mut fds =
            [libc::pollfd { fd: self.wl.fd(), events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: self.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if poll_fds(&mut fds, max) > 0 && fds[0].revents != 0 {
            self.wl.pump(Instant::now())?;
        }
        Ok(())
    }

    fn stopped(&self, id: u64) -> bool {
        self.quitting() || self.ctl.cancel_upto.load(Ordering::SeqCst) >= id
    }

    fn panel_open(&self) -> bool {
        self.ctl.panel_visible.load(Ordering::SeqCst)
    }

    /// Warten, bis kein Modifikator gehalten ist. `Ok(true)`: frei; `Ok(false)`: Zustand nicht lesbar
    fn wait_mods(&mut self, id: u64) -> Result<bool, Done> {
        let ctl = self.ctl.clone();
        let stopped = move || ctl.quit.load(Ordering::SeqCst) || ctl.cancel_upto.load(Ordering::SeqCst) >= id;
        wait_mods_with(&mut self.wl, &self.open_keys, &stopped, &self.back)
    }

    fn job(&mut self, job: &Job, cfg: &OutCfg) -> Done {
        let t0 = Instant::now();
        if self.stopped(job.id) {
            return Done::Stopped;
        }
        let mods_readable = match self.wait_mods(job.id) {
            Ok(r) => r,
            Err(d) => return d,
        };
        // Ziel: Fenster über niri, Feld über die Eingabemethode. Replace: nur im beim Öffnen gemerkten Fenster; niri
        // gibt den Fokus nach dem Schließen des Panels asynchron zurück (Befund 6), darum begrenzt darauf warten
        let t_mods = t0.elapsed();
        let focus = match job.replace {
            Some(want) => match self.wait_focus(job.id, want) {
                Ok(f) => Some(f),
                Err(d) => return d,
            },
            None => niri::focused(),
        };
        let app_id = focus.as_ref().and_then(|f| f.app_id.clone());
        let t_niri = t0.elapsed();
        if !self.wl.has_im() {
            self.wl.im_arm();
        }
        let trips = match self.wl.im_settle(IM_SETTLE) {
            Ok(n) => n,
            Err(e) => return Done::Failed(e),
        };
        let (im_active, im_terminal) = self.wl.im_state();
        let target = Target { app_id, im_active, im_terminal };
        let mut route = output::route(&target, cfg);
        if route == Route::Paste && !mods_readable {
            route = Route::Keyboard;
        }
        // Nie ins eigene Panel schreiben: getippte Buchstaben wären dort Aktionstasten (`e` sendet an die KI). Erkannt
        // an der `app_id` und, unabhängig von niri, am Zustand des Hauptthreads
        if target.app_id.as_deref() == Some(crate::panel::APP_ID) || self.panel_open() {
            return Done::Failed(PANEL_OPEN.into());
        }
        let terminal = output::is_terminal(&target, cfg);
        if job.replace.is_some() && terminal {
            // `replace_ok` des Daemons kennt nur die Liste; der Zweck des Felds (Eingabemethode) erkennt mehr Terminals
            return Done::Failed("Replace is off in terminals, nothing was written".into());
        }
        info!(
            "output job {}: route {}, app_id {}, terminal {terminal}, input method active {im_active} (settled after {trips} round trips), {} ms to decide (modifiers {} ms, niri {} ms)",
            job.id,
            route.name(),
            if target.app_id.is_some() { "known" } else { "unknown" },
            t0.elapsed().as_millis(),
            t_mods.as_millis(),
            (t_niri - t_mods).as_millis()
        );
        debug!("output job {}: app_id {:?}", job.id, target.app_id);
        let mut kbd = match self.wl.vkbd() {
            Ok(k) => k,
            Err(e) => return Done::Failed(e),
        };
        let res = self.write(job, cfg, route, terminal, focus.map(|f| f.id), &mut kbd);
        // Vor dem Zerstören alles loslassen (auch nach einem Fehler; `Drop` von Vkbd)
        drop(kbd);
        let _ = self.wl.flush();
        match res {
            Ok(d) => d,
            Err(e) => Done::Failed(e),
        }
    }

    /// Replace: warten, bis das Fenster `want` fokussiert ist (höchstens [`FOCUS_RETURN`]); sonst aufgeben
    fn wait_focus(&mut self, id: u64, want: u64) -> Result<niri::Focus, Done> {
        let until = Instant::now() + FOCUS_RETURN;
        loop {
            if self.stopped(id) {
                return Err(Done::Stopped);
            }
            let now = niri::focused();
            if let Some(f) = now.filter(|f| f.id == want) {
                return Ok(f);
            }
            if Instant::now() >= until {
                info!("output job {id}: window {want} did not get the focus back");
                return Err(Done::Failed("the window changed, so the text was not replaced. Copy it instead".into()));
            }
            self.wl.pump(Instant::now() + Duration::from_millis(50)).map_err(Done::Failed)?;
        }
    }

    /// Text und Tasten auf `route` schreiben. `focus`: niri-ID des Zielfensters beim Start; vor jedem Zeilenumbruch,
    /// jeder Rücktaste und dem `Ctrl+V` wird geprüft, dass es noch fokussiert ist (sonst führte ein Return den Text in
    /// einem anderen Fenster aus), bei der Eingabemethode vor jedem Stück, dass das Feld noch aktiv ist
    fn write(&mut self, job: &Job, cfg: &OutCfg, route: Route, terminal: bool, focus: Option<u64>, kbd: &mut Vkbd) -> Result<Done, String> {
        let pieces = output::Pieces(output::pieces(&job.text, &job.keys));
        let mut result = Done::Written;
        let mut rest: &[Piece] = &pieces;
        let n_text = pieces.iter().take_while(|p| !matches!(p, Piece::BackSpace)).count();
        if route == Route::InputMethod && terminal {
            // Terminal mit Eingabemethode: Text samt Zeilenumbrüchen in einem Weg und am Stück ("\n" kommt als Return
            // an, S2); nur die Rücktasten danach über die Tastatur
            let text = SecretText::new(output::sanitize(&job.text));
            for part in output::split_bytes(text.as_str(), COMMIT_BYTES) {
                if self.stopped(job.id) {
                    return Ok(Done::Stopped);
                }
                self.wl.pump(Instant::now())?;
                if !self.wl.im_state().0 {
                    return Ok(Done::Failed(FOCUS_MOVED.into()));
                }
                self.wl.im_commit(part)?;
            }
            if n_text < pieces.len() && !text.is_empty() {
                self.sync(job.id, text.as_str())?;
            }
            rest = &pieces[n_text..];
        }
        if route == Route::Paste {
            // Text ganz (mit Zeilenumbrüchen) einfügen, danach nur noch die Tasten
            let text = SecretText::new(output::sanitize(&job.text));
            if !text.is_empty() {
                let id = job.id;
                let ctl = self.ctl.clone();
                let ctl_v = self.ctl.clone();
                let stopped = move || ctl.quit.load(Ordering::SeqCst) || ctl.cancel_upto.load(Ordering::SeqCst) >= id;
                let stopped2 = stopped.clone();
                let open_keys = self.open_keys.clone();
                let back = self.back.clone();
                let mut ctrl_v = move |wl: &mut Wl, kbd: &mut Vkbd| -> CtrlV {
                    if stopped2() {
                        return CtrlV::Stopped;
                    }
                    // Unmittelbar vor dem Ctrl+V noch einmal: kein Modifikator gehalten (Pflicht, Regel 2)
                    match wait_mods_with(wl, &open_keys, &stopped2, &back) {
                        Ok(true) => {}
                        Ok(false) => return CtrlV::Unreadable,
                        Err(Done::Stopped) => return CtrlV::Stopped,
                        Err(Done::Failed(e)) => return CtrlV::Failed(e),
                        Err(d) => return CtrlV::Failed(format!("{d:?}")),
                    }
                    if focus_moved(focus) {
                        return CtrlV::Failed(FOCUS_MOVED.into());
                    }
                    if ctl_v.panel_visible.load(Ordering::SeqCst) {
                        return CtrlV::Failed(PANEL_OPEN.into());
                    }
                    if let Err(e) = kbd.set_keymap(&Keymap::paste()).and_then(|()| kbd.stroke(&Stroke::ctrl_v())).and_then(|()| wl.flush()) {
                        return CtrlV::Failed(e);
                    }
                    kbd.flushed();
                    CtrlV::Sent
                };
                match clip::paste(&mut self.wl, text.as_str(), kbd, &stopped, &mut ctrl_v) {
                    Outcome::Pasted => {}
                    Outcome::Unconfirmed => result = Done::Unconfirmed,
                    Outcome::Stopped => return Ok(Done::Stopped),
                    Outcome::Failed(e) => return Ok(Done::Failed(e)),
                    Outcome::Fallback(why) => {
                        info!("output job {}: not pasting ({why}), typing instead", job.id);
                        return self.write(job, cfg, Route::Keyboard, terminal, focus, kbd);
                    }
                }
            }
            rest = &pieces[n_text..];
        }
        // Bei der Eingabemethode: was zuletzt über welchen Weg ging (vor einem Wechsel des Wegs auf die Bestätigung
        // des Felds warten, `IM_SYNC`)
        let mut after_commit: Option<&str> = None;
        // Nach einer Taste: Zahl der `done` davor (danach muss ein neues kommen)
        let mut after_key: Option<u32> = None;
        for p in rest {
            if self.stopped(job.id) {
                return Ok(Done::Stopped);
            }
            // auch Return und Rücktaste wären im Panel Bedientasten
            if self.panel_open() {
                return Ok(Done::Failed(PANEL_OPEN.into()));
            }
            if route == Route::InputMethod {
                let commit_next = matches!(p, Piece::Text(_));
                if commit_next {
                    if let Some(prev) = after_key.take() {
                        let t = Instant::now();
                        if !self.wl.im_wait_done_after(prev, IM_SYNC)? {
                            debug!("output job {}: no confirmation of the key from the field after {} ms, continuing", job.id, t.elapsed().as_millis());
                        }
                    }
                } else if let Some(want) = after_commit.take() {
                    self.sync(job.id, want)?;
                }
            }
            match p {
                Piece::Text(t) if route == Route::InputMethod => {
                    for part in output::split_bytes(t, COMMIT_BYTES) {
                        if self.stopped(job.id) {
                            return Ok(Done::Stopped);
                        }
                        // Feld noch aktiv? (ein Fokuswechsel deaktiviert die Eingabemethode)
                        self.wl.pump(Instant::now())?;
                        if !self.wl.im_state().0 {
                            return Ok(Done::Failed(FOCUS_MOVED.into()));
                        }
                        self.wl.im_commit(part)?;
                    }
                    after_commit = Some(t.as_str());
                }
                Piece::Text(t) => {
                    let segs = keymap::segments(t).ok_or("text has control characters")?;
                    for (map, part) in segs {
                        kbd.set_keymap(&map)?;
                        for c in part.chars() {
                            if self.stopped(job.id) {
                                return Ok(Done::Stopped);
                            }
                            // Panel mitten im Tippen geöffnet: kein Zeichen mehr (Aktionstasten)
                            if self.panel_open() {
                                return Ok(Done::Failed(PANEL_OPEN.into()));
                            }
                            let code = map.code(c).ok_or("character not in keymap")?;
                            self.tap(kbd, &Stroke::tap(code), cfg)?;
                        }
                    }
                }
                Piece::Newlines(n) => {
                    if focus_moved(focus) {
                        return Ok(Done::Failed(FOCUS_MOVED.into()));
                    }
                    let serial_keys = self.wl.im_serial();
                    let stroke = if terminal { Stroke::tap(keymap::KEY_ENTER) } else { Stroke::shift_return() };
                    if kbd.map().is_none() {
                        kbd.set_keymap(&Keymap::for_chars([]).expect("empty keymap"))?;
                    }
                    for _ in 0..*n {
                        if self.stopped(job.id) {
                            return Ok(Done::Stopped);
                        }
                        self.tap(kbd, &stroke, cfg)?;
                    }
                    after_key = Some(serial_keys);
                }
                Piece::BackSpace => {
                    if focus_moved(focus) {
                        return Ok(Done::Failed(FOCUS_MOVED.into()));
                    }
                    if kbd.map().is_none() {
                        kbd.set_keymap(&Keymap::for_chars([]).expect("empty keymap"))?;
                    }
                    self.tap(kbd, &Stroke::tap(keymap::KEY_BACKSPACE), cfg)?;
                }
            }
        }
        Ok(result)
    }

    /// Warten, bis das Feld meldet, dass der Text vor dem Cursor mit `want` endet (höchstens [`IM_SYNC`])
    fn sync(&mut self, id: u64, want: &str) -> Result<(), String> {
        let t = Instant::now();
        if !self.wl.im_wait_before(want, IM_SYNC)? {
            debug!("output job {id}: no confirmation from the field after {} ms, continuing", t.elapsed().as_millis());
        }
        Ok(())
    }

    /// Ein Anschlag: senden, `flush`, dann die Pause (`typing.delay_ms`) mit Verteilen
    fn tap(&mut self, kbd: &mut Vkbd, s: &Stroke, cfg: &OutCfg) -> Result<(), String> {
        kbd.stroke(s)?;
        self.wl.flush()?;
        kbd.flushed();
        self.wl.pump(Instant::now() + Duration::from_millis(cfg.delay_ms))
    }
}

/// Ist ein anderes Fenster fokussiert als beim Start (`focus`)? Unbekannt beim Start: nicht prüfbar (`false`); jetzt
/// unbekannt (niri antwortet nicht, kein Fenster fokussiert): ja
fn focus_moved(focus: Option<u64>) -> bool {
    let Some(start) = focus else { return false };
    let now = niri::focused().map(|f| f.id);
    if now != Some(start) {
        info!("output: focus moved from window {start} to {now:?}");
        return true;
    }
    false
}

/// Warten, bis kein Modifikator gehalten ist (auch für das `Ctrl+V` im Ablauf von `clip::paste`). `Ok(true)`: frei;
/// `Ok(false)`: Zustand nicht lesbar; `Err`: abgebrochen, Frist abgelaufen oder Wayland-Fehler
fn wait_mods_with(wl: &mut Wl, open_keys: &OpenKeys, stopped: &dyn Fn() -> bool, back: &async_channel::Sender<Msg>) -> Result<bool, Done> {
    let mut keys = open_keys();
    let start = Instant::now();
    let mut hinted = false;
    loop {
        match keys.mods_down() {
            Some(false) => return Ok(true),
            None => return Ok(false),
            Some(true) => {}
        }
        if stopped() {
            return Err(Done::Stopped);
        }
        let waited = start.elapsed();
        if waited >= MODS_TIMEOUT {
            return Err(Done::Failed("modifier keys held for 30 s, text not inserted".into()));
        }
        if waited >= MODS_HINT_AFTER && !hinted {
            hinted = true;
            info!("output: waiting for the modifier keys to be released");
            let _ = back.send_blocking(Msg::OutputNotice("Release the modifier keys to insert the text".into(), NoticeKind::Info));
        }
        wl.pump(Instant::now() + MODS_POLL).map_err(Done::Failed)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_tx() -> (OutTx, std::sync::mpsc::Receiver<Cmd>) {
        // SAFETY: eventfd ohne Vorbedingungen, frisch angelegt
        let wake = Arc::new(unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)) });
        let (tx, rx) = std::sync::mpsc::channel();
        (OutTx { tx, wake }, rx)
    }

    /// Durchsicht W3 M1: ein als gescheitert gemeldetes Copy setzt CLIPBOARD nie später doch noch
    #[test]
    fn copy_given_up_is_never_executed_later() {
        let (out, rx) = out_tx();
        let r = out.copy(SecretText::new("Antwort".into()), Duration::from_millis(30));
        assert_eq!(r, Err("the text output is busy".to_string()));
        // erst jetzt kommt der Ausgabe-Thread dran: der Slot ist leer, er tut nichts
        match rx.try_recv() {
            Ok(Cmd::Copy(slot, _)) => assert!(claim(&slot).is_none()),
            _ => panic!("no copy queued"),
        }
    }

    #[test]
    fn copy_started_before_the_deadline_reports_its_real_result() {
        let (out, rx) = out_tx();
        let worker = std::thread::spawn(move || {
            let Ok(Cmd::Copy(slot, reply)) = rx.recv() else { panic!("no copy") };
            let text = claim(&slot).expect("claimed in time");
            // länger als die Frist des Aufrufers
            std::thread::sleep(Duration::from_millis(120));
            assert_eq!(text.as_str(), "Antwort");
            reply.send(Ok(())).unwrap();
        });
        assert_eq!(out.copy(SecretText::new("Antwort".into()), Duration::from_millis(30)), Ok(()));
        worker.join().unwrap();
    }

    #[test]
    fn primary_read_given_up_is_skipped() {
        let (out, rx) = out_tx();
        assert!(out.read_primary(Duration::from_millis(30)).is_err());
        match rx.try_recv() {
            Ok(Cmd::ReadPrimary(slot, _)) => assert!(claim(&slot).is_none()),
            _ => panic!("no read queued"),
        }
    }

    #[test]
    fn clipboard_read_given_up_is_skipped() {
        let (out, rx) = out_tx();
        assert!(out.read_clipboard(Duration::from_millis(30)).is_err());
        match rx.try_recv() {
            Ok(Cmd::ReadClipboard(slot, _)) => assert!(claim(&slot).is_none()),
            _ => panic!("no read queued"),
        }
    }
}
