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

//! Eigene Wayland-Verbindung des Ausgabe-Threads (`output.rs`): virtuelle Tastatur (T2, `zwp_virtual_keyboard_v1`),
//! Eingabemethode (T3, `zwp_input_method_v2`) und Zwischenablage (`ext_data_control_v1`). GTK hat seine eigene
//! Verbindung; diese hier gehört allein dem Ausgabe-Thread.
//!
//! **Immer verteilen.** `dictate-ui` bedient eine zurückgelegte Zwischenablage selbst weiter (S3: endet der Anbieter,
//! ist sie leer). Darum wartet der Thread nie blind (`sleep`, blockierendes `recv`), sondern immer in [`Wl::pump`]:
//! `poll` über den Wayland-Socket (und beim Lesen einer Auswahl über die Pipe), Ereignisse verteilen, Anfragen anderer
//! Programme an die Zwischenablage in eigenen Schreib-Threads beantworten.
//!
//! **Nie loggen:** Inhalte der Zwischenablage, der eingefügte und der getippte Text. Ins Log kommen nur Typen-Zahlen,
//! Längen und Zeiten.

use crate::ui::keymap::{Ev, Keymap, Stroke};
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use wayland_client::backend::{ObjectId, WaylandError};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{delegate_noop, event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::ContentPurpose;
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// Hinweis an Verläufe der Zwischenablage (KDE-Konvention, Noctalia beachtet ihn; S3)
pub const HINT: &str = "x-kde-passwordManagerHint";
/// Frist für das Schreiben eines Inhalts an ein anderes Programm (hängt der Leser, gibt der Schreib-Thread auf)
const SERVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Höchstens so viele Schreib-Threads gleichzeitig; darüber bekommt der Leser sofort das Ende (leerer Inhalt) statt
/// eines weiteren Threads (ein Programm, das in einer Schleife abholt, könnte sonst beliebig viele erzeugen)
const MAX_SERVING: usize = 16;
static SERVING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Inhalt, der nach dem letzten Verweis überschrieben wird (Zwischenablage, eingefügter Text)
pub struct Secret(pub Vec<u8>);

impl Drop for Secret {
    fn drop(&mut self) {
        crate::actions::store::wipe_bytes(&mut self.0);
    }
}

/// Was eine eigene Quelle anbietet: Typ → Inhalt, in der Reihenfolge des Anbietens
pub type Items = Arc<Vec<(String, Arc<Secret>)>>;

/// Eine Anfrage eines anderen Programms an eine eigene Quelle
#[derive(Debug, Clone)]
pub struct Send {
    pub source: ObjectId,
    pub mime: String,
    pub at: Instant,
}

/// Höchstzahl gemerkter Typen eines Angebots; mehr gilt als abgeschnitten (dann wird nie eingefügt)
const MAX_TYPES: usize = 256;

/// Ein Angebot einer Auswahl mit seinen Typen
#[derive(Debug, Clone)]
pub struct Offer {
    pub offer: ExtDataControlOfferV1,
    pub types: Vec<String>,
    /// Mehr als [`MAX_TYPES`] Typen: nicht alle gemerkt (der Hinweis [`HINT`] wird immer gemerkt)
    pub truncated: bool,
}

/// So viele Zeichen vor dem Cursor merkt sich die Ausgabe aus `surrounding_text`
const BEFORE_CHARS: usize = 64;

/// Die letzten [`BEFORE_CHARS`] Zeichen vor `cursor` (Byte-Versatz, auf eine Zeichengrenze gekürzt)
fn tail_before(text: &str, cursor: u32) -> String {
    let mut c = (cursor as usize).min(text.len());
    while !text.is_char_boundary(c) {
        c -= 1;
    }
    let before = &text[..c];
    let skip = before.chars().count().saturating_sub(BEFORE_CHARS);
    before.chars().skip(skip).collect()
}

/// Endet `before` mit dem Ende von `suffix` (höchstens [`BEFORE_CHARS`] Zeichen verglichen)?
pub fn ends_with_tail(before: &str, suffix: &str) -> bool {
    let n = suffix.chars().count();
    let tail: String = suffix.chars().skip(n.saturating_sub(BEFORE_CHARS)).collect();
    before.ends_with(&tail)
}

/// Zustand der Eingabemethode (doppelt gepuffert: gilt erst nach `done`)
#[derive(Debug, Default, Clone, Copy)]
struct ImState {
    active: bool,
    terminal: bool,
}

#[derive(Default)]
pub struct St {
    im_pending: ImState,
    im_now: ImState,
    /// Text vor dem Cursor laut `surrounding_text` (höchstens [`BEFORE_CHARS`] Zeichen; nur im Speicher, nie geloggt):
    /// daran sieht die Ausgabe, dass das Feld ein `commit` oder eine Taste verarbeitet hat
    im_before_pending: Option<String>,
    im_before: Option<String>,
    /// Zahl der `done` (Serial für `commit`)
    im_serial: u32,
    /// Mindestens ein `done` kam (die Eingabemethode ist am Sitz angenommen)
    im_seen_done: bool,
    im_unavailable: bool,
    /// Angebote (Typen sammeln sich vor `selection`)
    offers: Vec<Offer>,
    /// Aktuelle Auswahl (CLIPBOARD): `None` = leer; `selection_known` erst nach dem ersten `selection`
    pub selection: Option<Offer>,
    pub selection_known: bool,
    /// Zählt jedes `selection` (eine Auswahl, die während des Sicherns wechselt, wird nie übernommen)
    pub selection_seq: u64,
    /// Aktuelle Markierung (PRIMARY, nur für die Textaktionen, `primary.rs`): wie `selection`
    pub primary: Option<Offer>,
    pub primary_known: bool,
    pub primary_seq: u64,
    /// Anfragen an eigene Quellen, seit dem letzten Leeren
    pub sends: Vec<Send>,
    /// Eigene Quellen, die `cancelled` bekamen (eine andere Auswahl gilt jetzt)
    pub cancelled: Vec<ObjectId>,
    /// Zurückgelegte (Einfügen) oder kopierte (Copy der Textaktionen) Zwischenablage, die `dictate-ui` weiter bedient
    pub restored: Option<(ExtDataControlSourceV1, Items)>,
}

impl Dispatch<WlRegistry, GlobalListContents> for St {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(St: ignore WlSeat);
delegate_noop!(St: ZwpVirtualKeyboardManagerV1);
delegate_noop!(St: ZwpVirtualKeyboardV1);
delegate_noop!(St: ZwpInputMethodManagerV2);
delegate_noop!(St: ExtDataControlManagerV1);

impl Dispatch<ZwpInputMethodV2, ()> for St {
    fn event(st: &mut Self, _: &ZwpInputMethodV2, event: zwp_input_method_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use zwp_input_method_v2::Event;
        match event {
            Event::Activate => {
                st.im_pending = ImState { active: true, terminal: false };
                st.im_before_pending = None;
            }
            Event::Deactivate => {
                st.im_pending = ImState { active: false, terminal: false };
                st.im_before_pending = None;
            }
            Event::SurroundingText { text, cursor, .. } => st.im_before_pending = Some(tail_before(&text, cursor)),
            Event::ContentType { purpose, .. } => st.im_pending.terminal = matches!(purpose, WEnum::Value(ContentPurpose::Terminal)),
            Event::Done => {
                st.im_serial = st.im_serial.wrapping_add(1);
                st.im_now = st.im_pending;
                st.im_before = st.im_before_pending.clone();
                st.im_seen_done = true;
                debug!("input method done {}: active {}, terminal {}", st.im_serial, st.im_now.active, st.im_now.terminal);
            }
            Event::Unavailable => {
                debug!("input method unavailable (another one holds the seat)");
                st.im_unavailable = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for St {
    fn event(st: &mut Self, _: &ExtDataControlDeviceV1, event: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => st.offers.push(Offer { offer: id, types: Vec::new(), truncated: false }),
            Event::Selection { id } => {
                // Das vorige Angebot wird nicht mehr gebraucht
                if let Some(old) = st.selection.take() {
                    if id.as_ref() != Some(&old.offer) {
                        old.offer.destroy();
                    }
                }
                st.selection = id.and_then(|o| {
                    let i = st.offers.iter().position(|p| p.offer == o)?;
                    Some(st.offers.remove(i))
                });
                st.selection_known = true;
                st.selection_seq += 1;
            }
            // PRIMARY gehört zu den Textaktionen (`primary.rs`); gelesen wird nur auf Mod+I
            Event::PrimarySelection { id } => {
                if let Some(old) = st.primary.take() {
                    if id.as_ref() != Some(&old.offer) {
                        old.offer.destroy();
                    }
                }
                st.primary = id.and_then(|o| {
                    let i = st.offers.iter().position(|p| p.offer == o)?;
                    Some(st.offers.remove(i))
                });
                st.primary_known = true;
                st.primary_seq += 1;
            }
            Event::Finished => warn!("clipboard: data control device finished"),
            _ => {}
        }
    }
    event_created_child!(St, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for St {
    fn event(st: &mut Self, offer: &ExtDataControlOfferV1, event: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some(o) = st.offers.iter_mut().find(|p| p.offer == *offer) {
                if o.types.contains(&mime_type) {
                    // doppelt angeboten
                } else if o.types.len() < MAX_TYPES || mime_type == HINT {
                    o.types.push(mime_type);
                } else {
                    o.truncated = true;
                }
            }
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, Items> for St {
    fn event(st: &mut Self, src: &ExtDataControlSourceV1, event: ext_data_control_source_v1::Event, items: &Items, _: &Connection, _: &QueueHandle<Self>) {
        use ext_data_control_source_v1::Event;
        match event {
            Event::Send { mime_type, fd } => {
                st.sends.push(Send { source: src.id(), mime: mime_type.clone(), at: Instant::now() });
                let data = items.iter().find(|(m, _)| *m == mime_type).map(|(_, d)| d.clone());
                debug!("clipboard: send of one type requested ({} bytes)", data.as_ref().map_or(0, |d| d.0.len()));
                if let Some(d) = data {
                    use std::sync::atomic::Ordering::SeqCst;
                    if SERVING.fetch_add(1, SeqCst) >= MAX_SERVING {
                        SERVING.fetch_sub(1, SeqCst);
                        warn!("clipboard: {MAX_SERVING} transfers running, request answered empty");
                        return;
                    }
                    // Im eigenen Thread: ein langsamer Leser hält die Ausgabe nie auf
                    let spawned = std::thread::Builder::new().name("clip-serve".into()).spawn(move || {
                        serve(fd, &d.0);
                        SERVING.fetch_sub(1, SeqCst);
                    });
                    if let Err(e) = spawned {
                        SERVING.fetch_sub(1, SeqCst);
                        warn!("clipboard: cannot serve: {e}");
                    }
                }
            }
            Event::Cancelled => {
                st.cancelled.push(src.id());
                if st.restored.as_ref().is_some_and(|(s, _)| s == src) {
                    // Ein anderes Programm hat die Zwischenablage übernommen: Gesichertes freigeben (wird überschrieben)
                    debug!("clipboard: restored content replaced by another program");
                    st.restored = None;
                }
                src.destroy();
            }
            _ => {}
        }
    }
}

/// Inhalt in die Pipe eines anderen Programms schreiben, mit Frist; danach schließen
fn serve(fd: OwnedFd, data: &[u8]) {
    set_nonblocking(fd.as_raw_fd());
    let deadline = Instant::now() + SERVE_TIMEOUT;
    let mut off = 0;
    while off < data.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !poll_one(fd.as_fd(), libc::POLLOUT, left) {
            debug!("clipboard: reader too slow, gave up");
            return;
        }
        // SAFETY: gültiger Deskriptor, Puffer ab `off` mit passender Länge
        let n = unsafe { libc::write(fd.as_raw_fd(), data[off..].as_ptr().cast(), data.len() - off) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        off += n as usize;
    }
}

pub fn set_nonblocking(fd: RawFd) {
    // SAFETY: fcntl auf einem gültigen Deskriptor
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        if fl >= 0 {
            libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
    }
}

/// `poll` auf einem Deskriptor; `true`, wenn `events` eintraten (oder Fehler/Ende, die der Aufrufer beim Lesen sieht)
pub fn poll_one(fd: BorrowedFd<'_>, events: i16, timeout: Duration) -> bool {
    let mut p = [libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 }];
    poll_fds(&mut p, timeout) > 0
}

pub fn poll_fds(fds: &mut [libc::pollfd], timeout: Duration) -> i32 {
    let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: gültiges Feld von pollfd mit passender Länge
    unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) }
}

/// Die Verbindung des Ausgabe-Threads
pub struct Wl {
    pub conn: Connection,
    queue: EventQueue<St>,
    pub qh: QueueHandle<St>,
    seat: WlSeat,
    vk_mgr: Option<ZwpVirtualKeyboardManagerV1>,
    im_mgr: Option<ZwpInputMethodManagerV2>,
    pub dc_mgr: Option<ExtDataControlManagerV1>,
    pub device: Option<ExtDataControlDeviceV1>,
    pub st: St,
    im: Option<ZwpInputMethodV2>,
    /// Wann die Eingabemethode angelegt wurde
    im_created: Option<Instant>,
}

impl Wl {
    pub fn connect() -> Result<Wl, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("wayland: {e}"))?;
        let (globals, queue) = registry_queue_init::<St>(&conn).map_err(|e| format!("wayland registry: {e}"))?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
        let vk_mgr = globals.bind(&qh, 1..=1, ()).map_err(|e| warn!("no virtual keyboard: {e}")).ok();
        let im_mgr = globals.bind(&qh, 1..=1, ()).map_err(|e| warn!("no input method: {e}")).ok();
        let dc_mgr: Option<ExtDataControlManagerV1> = globals.bind(&qh, 1..=1, ()).map_err(|e| warn!("no data control: {e}")).ok();
        let device = dc_mgr.as_ref().map(|m| m.get_data_device(&seat, &qh, ()));
        let mut wl = Wl { conn, queue, qh, seat, vk_mgr, im_mgr, dc_mgr, device, st: St::default(), im: None, im_created: None };
        wl.roundtrip()?;
        Ok(wl)
    }

    pub fn fd(&self) -> RawFd {
        self.conn.backend().poll_fd().as_raw_fd()
    }

    /// Alles Gepufferte senden; ist der Socket voll, warten (höchstens 2 s)
    pub fn flush(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.conn.flush() {
                Ok(()) => return Ok(()),
                Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err("wayland: socket full".into());
                    }
                    poll_one(self.conn.backend().poll_fd(), libc::POLLOUT, left.min(Duration::from_millis(50)));
                }
                Err(e) => return Err(format!("wayland: {e}")),
            }
        }
    }

    pub fn roundtrip(&mut self) -> Result<(), String> {
        self.queue.roundtrip(&mut self.st).map(|_| ()).map_err(|e| format!("wayland: {e}"))
    }

    /// Senden, dann bis `until` Ereignisse lesen und verteilen (auch wenn keine kommen: wartet bis `until`).
    /// `extra`: ein weiterer Deskriptor (Pipe beim Lesen); `Ok(true)`, sobald er lesbar ist (früheres Ende)
    pub fn pump_with(&mut self, until: Instant, extra: Option<RawFd>) -> Result<bool, String> {
        loop {
            self.flush()?;
            self.queue.dispatch_pending(&mut self.st).map_err(|e| format!("wayland: {e}"))?;
            let left = until.saturating_duration_since(Instant::now());
            let Some(guard) = self.queue.prepare_read() else { continue };
            let mut fds = vec![libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 }];
            if let Some(fd) = extra {
                fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
            }
            let n = poll_fds(&mut fds, left);
            if n > 0 && fds[0].revents != 0 {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => return Err(format!("wayland: {e}")),
                }
            } else {
                drop(guard);
            }
            self.queue.dispatch_pending(&mut self.st).map_err(|e| format!("wayland: {e}"))?;
            if fds.get(1).is_some_and(|p| p.revents != 0) {
                return Ok(true);
            }
            if left.is_zero() || Instant::now() >= until {
                return Ok(false);
            }
        }
    }

    pub fn pump(&mut self, until: Instant) -> Result<(), String> {
        self.pump_with(until, None).map(|_| ())
    }

    // --- Eingabemethode (T3) -----------------------------------------------------------------------------------

    pub fn has_im(&self) -> bool {
        self.im.is_some()
    }

    /// Eingabemethode anlegen (nur für die Dauer einer Ausgabe, Regel 4.9). Ohne Wartezeit; `activate` kommt, wenn
    /// das fokussierte Feld text-input-v3 eingeschaltet hat
    pub fn im_arm(&mut self) {
        if self.im.is_some() {
            return;
        }
        let Some(mgr) = &self.im_mgr else { return };
        self.st.im_pending = ImState::default();
        self.st.im_now = ImState::default();
        self.st.im_before_pending = None;
        self.st.im_before = None;
        self.st.im_serial = 0;
        self.st.im_seen_done = false;
        self.st.im_unavailable = false;
        self.im = Some(mgr.get_input_method(&self.seat, &self.qh, ()));
        self.im_created = Some(Instant::now());
        debug!("input method created");
    }

    pub fn im_disarm(&mut self) {
        self.im_created = None;
        if let Some(im) = self.im.take() {
            im.destroy();
            let _ = self.flush();
            debug!("input method destroyed");
        }
        self.st.im_now = ImState::default();
    }

    /// (aktiv, Zweck Terminal) für das fokussierte Feld
    pub fn im_state(&self) -> (bool, bool) {
        if self.im.is_none() || self.st.im_unavailable {
            return (false, false);
        }
        (self.st.im_now.active, self.st.im_now.terminal)
    }

    /// Wartet höchstens `max` nach dem Anlegen auf das erste `done` (das `activate` eines schon fokussierten Felds
    /// kommt mit ihm; ohne Feld mit text-input-v3 kommt keins). Liefert die Zahl der Rundreisen (Messwert fürs Log)
    pub fn im_settle(&mut self, max: Duration) -> Result<u32, String> {
        let Some(created) = self.im_created else { return Ok(0) };
        let until = created + max;
        // Mindestens eine Rundreise: was niri bis jetzt geschickt hat, ist dann verteilt
        self.roundtrip()?;
        let mut trips = 1;
        while self.im.is_some() && !self.st.im_seen_done && !self.st.im_unavailable && Instant::now() < until {
            self.roundtrip()?;
            trips += 1;
            if !self.st.im_seen_done {
                self.pump(Instant::now() + Duration::from_millis(5).min(until.saturating_duration_since(Instant::now())))?;
            }
        }
        Ok(trips)
    }

    /// Wartet höchstens `max`, bis das Feld meldet, dass der Text vor dem Cursor mit `suffix` endet (es hat ein `commit`
    /// oder eine Taste verarbeitet). Eingabemethode und virtuelle Tastatur sind getrennte Wege ohne feste Reihenfolge:
    /// unter Last kam ein `commit` vor einem zuvor gesendeten Shift+Return an (gemessen). `false`: Frist abgelaufen
    /// oder das Feld meldet keinen Text (dann geht es ohne Bestätigung weiter)
    pub fn im_wait_before(&mut self, suffix: &str, max: Duration) -> Result<bool, String> {
        let until = Instant::now() + max;
        loop {
            if self.st.im_before.as_deref().is_some_and(|b| ends_with_tail(b, suffix)) {
                return Ok(true);
            }
            if Instant::now() >= until || self.im.is_none() {
                return Ok(false);
            }
            self.pump((Instant::now() + Duration::from_millis(10)).min(until))?;
        }
    }

    /// Zahl der `done` bisher (für [`Wl::im_wait_done_after`])
    pub fn im_serial(&self) -> u32 {
        self.st.im_serial
    }

    /// Wartet höchstens `max` auf ein `done` nach `serial`: das Feld hat nach einer Taste seinen Zustand neu gemeldet.
    /// Ein festes Ende des Texts taugt dafür nicht: GTK meldet nach Shift+Return neue `done`, aber keinen geänderten
    /// Text vor dem Cursor (gemessen)
    pub fn im_wait_done_after(&mut self, serial: u32, max: Duration) -> Result<bool, String> {
        let until = Instant::now() + max;
        loop {
            if self.st.im_serial != serial {
                return Ok(true);
            }
            if Instant::now() >= until || self.im.is_none() {
                return Ok(false);
            }
            self.pump((Instant::now() + Duration::from_millis(10)).min(until))?;
        }
    }

    /// Text über die Eingabemethode übergeben (ein Stück, schon passend groß)
    pub fn im_commit(&mut self, text: &str) -> Result<(), String> {
        let im = self.im.as_ref().ok_or("input method gone")?;
        im.commit_string(text.to_string());
        im.commit(self.st.im_serial);
        self.flush()
    }

    // --- Virtuelle Tastatur (T2) -------------------------------------------------------------------------------

    pub fn vkbd(&self) -> Result<Vkbd, String> {
        let mgr = self.vk_mgr.as_ref().ok_or("the compositor offers no virtual keyboard")?;
        Ok(Vkbd {
            vk: Some(mgr.create_virtual_keyboard(&self.seat, &self.qh, ())),
            conn: self.conn.clone(),
            start: Instant::now(),
            pressed: BTreeSet::new(),
            unflushed: BTreeSet::new(),
            mods: 0,
            map: None,
        })
    }
}

/// Die eigene virtuelle Tastatur einer Ausgabe. Hält zwischen zwei `flush` nie eine Taste ([`Stroke`]); beim
/// Verwerfen (auch bei einem Fehler mitten im Auftrag) lässt sie alles los, bevor sie zerstört wird (S2: niri lässt
/// beim Zerstören nicht los).
pub struct Vkbd {
    vk: Option<ZwpVirtualKeyboardV1>,
    conn: Connection,
    start: Instant,
    pressed: BTreeSet<u32>,
    /// Tasten, deren Ereignisse seit dem letzten gelungenen `flush` gepuffert sind (scheiterte er, ist offen, ob das
    /// Loslassen ankam: beim Verwerfen noch einmal loslassen)
    unflushed: BTreeSet<u32>,
    mods: u32,
    map: Option<Keymap>,
}

impl Vkbd {
    /// Nach einem gelungenen `flush` der Verbindung: alles Gepufferte ist beim Compositor
    pub fn flushed(&mut self) {
        self.unflushed.clear();
    }

    fn time(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    /// Belegung hochladen (über ein memfd: der Inhalt verrät die Zeichen des Textes, darum keine Datei)
    pub fn set_keymap(&mut self, map: &Keymap) -> Result<(), String> {
        if self.map.as_ref() == Some(map) {
            return Ok(());
        }
        let vk = self.vk.as_ref().ok_or("virtual keyboard gone")?;
        let mut text = map.to_xkb().into_bytes();
        text.push(0);
        let fd = memfd(&text)?;
        vk.keymap(1, fd.as_fd(), text.len() as u32);
        self.map = Some(map.clone());
        Ok(())
    }

    pub fn map(&self) -> Option<&Keymap> {
        self.map.as_ref()
    }

    /// Einen Anschlag senden (noch nicht `flush`)
    pub fn stroke(&mut self, s: &Stroke) -> Result<(), String> {
        for e in &s.0 {
            self.event(*e)?;
        }
        Ok(())
    }

    fn event(&mut self, e: Ev) -> Result<(), String> {
        let t = self.time();
        let vk = self.vk.as_ref().ok_or("virtual keyboard gone")?;
        vk.key(t, e.code, u32::from(e.down));
        self.unflushed.insert(e.code);
        if e.down {
            self.pressed.insert(e.code);
        } else {
            self.pressed.remove(&e.code);
        }
        if let Some(m) = e.mods {
            self.mods = m;
            vk.modifiers(m, 0, 0, 0);
        }
        Ok(())
    }

    /// Alles Gedrückte loslassen und die Modifikatoren zurücksetzen (noch nicht `flush`); dazu jede Taste, deren
    /// Loslassen noch ungesendet sein könnte (ein überzähliges Loslassen ist harmlos, ein fehlendes nicht)
    pub fn release_all(&mut self) {
        let Some(vk) = self.vk.as_ref() else { return };
        let t = self.time();
        let unsure = !self.unflushed.is_empty();
        let mut codes = std::mem::take(&mut self.pressed);
        codes.append(&mut self.unflushed);
        for code in codes {
            vk.key(t, code, 0);
        }
        if self.mods != 0 || unsure {
            self.mods = 0;
            vk.modifiers(0, 0, 0, 0);
        }
    }
}

impl Drop for Vkbd {
    fn drop(&mut self) {
        self.release_all();
        if let Some(vk) = self.vk.take() {
            vk.destroy();
        }
        // Senden, auch wenn der Socket voll ist (ohne Loslassen wiederholte das Zielprogramm die Taste)
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.conn.flush() {
                Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                    poll_one(self.conn.backend().poll_fd(), libc::POLLOUT, Duration::from_millis(50));
                }
                _ => break,
            }
        }
    }
}

/// Anonyme Datei im Speicher mit `data`, Lesezeiger am Anfang
fn memfd(data: &[u8]) -> Result<OwnedFd, String> {
    // SAFETY: gültiger, nullterminierter Name; Ergebnis ist ein neuer Deskriptor oder -1
    let fd = unsafe { libc::memfd_create(c"dictate-keymap".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!("memfd: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: frisch angelegt, gehört niemandem sonst
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut off = 0;
    while off < data.len() {
        // SAFETY: gültiger Deskriptor, Puffer ab `off` mit passender Länge
        let n = unsafe { libc::write(fd.as_raw_fd(), data[off..].as_ptr().cast(), data.len() - off) };
        if n <= 0 {
            return Err(format!("memfd: {}", std::io::Error::last_os_error()));
        }
        off += n as usize;
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_before_the_cursor() {
        assert_eq!(tail_before("Grüße Welt", 7), "Grüße");
        // Cursor mitten in einem Zeichen: auf die Grenze davor
        assert_eq!(tail_before("äb", 1), "");
        assert_eq!(tail_before("abc", 99), "abc");
        let long = "x".repeat(100) + "Ende";
        assert_eq!(tail_before(&long, long.len() as u32).chars().count(), BEFORE_CHARS);
        assert!(ends_with_tail("Hallo Welt\n", "\n"));
        assert!(ends_with_tail(&tail_before(&long, long.len() as u32), &long));
        assert!(!ends_with_tail("Hallo", "Welt"));
    }
}
