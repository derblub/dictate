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

//! Spike S3 (W0, PLAN-wayland.md Abschnitt 5): Einfügen über Data-Control (`ext_data_control_v1`) unter niri.
//!
//! `clip types`            MIME-Typen der aktuellen Auswahl (CLIPBOARD), eine Zeile
//! `clip get MIME`         Inhalt eines Typs nach stdout
//! `clip save DIR`         alle Typen sichern: DIR/types.txt (ein Typ je Zeile), DIR/<n>.bin
//! `clip offer DIR`        Auswahl mit den Typen aus DIR anbieten (wie `save` geschrieben); meldet jedes `send`
//!                         und `cancelled`, endet bei `cancelled` oder EOF auf stdin
//! `clip watch greedy|respect`  Stellvertreter eines Verlaufs: meldet jede neue Auswahl mit ihren Typen und holt den
//!                         Text sofort ab (`greedy`) bzw. nur ohne `x-kde-passwordManagerHint: secret` (`respect`)
//!
//! Ausgabe: eine Zeile je Beobachtung mit Millisekunden seit Start. Inhalte werden nie ausgegeben (nur Längen).
//! Isolation: startet nur, wenn XDG_RUNTIME_DIR unter /home/data/stt-test/wl-run/ liegt (Harness).

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const HINT: &str = "x-kde-passwordManagerHint";
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn say(line: &str) {
    let ms = START.get_or_init(Instant::now).elapsed().as_millis();
    let mut out = std::io::stderr();
    let _ = writeln!(out, "{ms:>7} {line}").and_then(|()| out.flush());
}

fn guard() {
    const BASE: &str = "/home/data/stt-test/wl-run/";
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let wl = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !rt.starts_with(BASE) || (wl.starts_with('/') && !wl.starts_with(BASE)) {
        eprintln!("clip: refusing to run outside the test runtime dir {BASE}");
        std::process::exit(70);
    }
}

#[derive(Default)]
struct State {
    /// Typen je Angebot (Angebot kommt vor `selection`).
    pending: Vec<(ExtDataControlOfferV1, Vec<String>)>,
    /// Aktuelle Auswahl: Angebot und Typen; `None` = leer. `Some` erst nach dem ersten `selection`.
    selection: Option<Option<(ExtDataControlOfferV1, Vec<String>)>>,
    selections_seen: u32,
    /// Für `offer`: Inhalte je Typ und Ende.
    content: Arc<Vec<(String, Vec<u8>)>>,
    cancelled: bool,
    sends: Arc<Mutex<u32>>,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ExtDataControlManagerV1);

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(state: &mut Self, _: &ExtDataControlDeviceV1, event: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => state.pending.push((id, Vec::new())),
            Event::Selection { id } => {
                state.selections_seen += 1;
                let entry = id.and_then(|o| {
                    let i = state.pending.iter().position(|(p, _)| *p == o)?;
                    Some(state.pending.remove(i))
                });
                state.selection = Some(entry);
            }
            Event::PrimarySelection { .. } => {}
            Event::Finished => say("device finished"),
            _ => {}
        }
    }
    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(state: &mut Self, offer: &ExtDataControlOfferV1, event: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Some((_, types)) = state.pending.iter_mut().find(|(p, _)| p == offer)
        {
            types.push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(state: &mut Self, _: &ExtDataControlSourceV1, event: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use ext_data_control_source_v1::Event;
        match event {
            Event::Send { mime_type, fd } => {
                let n = {
                    let mut s = state.sends.lock().unwrap();
                    *s += 1;
                    *s
                };
                let data = state.content.iter().find(|(m, _)| *m == mime_type).map(|(_, d)| d.clone());
                say(&format!("send #{n} {mime_type} ({} bytes)", data.as_ref().map_or(0, Vec::len)));
                // Schreiben im eigenen Thread: große Inhalte blockieren sonst die Ereignisschleife.
                std::thread::spawn(move || {
                    let mut f = std::fs::File::from(fd);
                    if let Some(d) = data {
                        let _ = f.write_all(&d);
                    }
                });
            }
            Event::Cancelled => {
                say("cancelled");
                state.cancelled = true;
            }
            _ => {}
        }
    }
}

struct Ctx {
    conn: Connection,
    queue: wayland_client::EventQueue<State>,
    qh: QueueHandle<State>,
    mgr: ExtDataControlManagerV1,
    device: ExtDataControlDeviceV1,
    state: State,
}

fn connect() -> Result<Ctx, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
    let mgr: ExtDataControlManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("ext_data_control: {e}"))?;
    let device = mgr.get_data_device(&seat, &qh, ());
    Ok(Ctx { conn, queue, qh, mgr, device, state: State::default() })
}

impl Ctx {
    fn wait_selection(&mut self) -> Result<(), String> {
        while self.state.selection.is_none() {
            self.queue.blocking_dispatch(&mut self.state).map_err(|e| format!("dispatch: {e}"))?;
        }
        Ok(())
    }
    fn receive(&mut self, offer: &ExtDataControlOfferV1, mime: &str) -> Result<Vec<u8>, String> {
        let (mut r, w) = std::io::pipe().map_err(|e| format!("pipe: {e}"))?;
        offer.receive(mime.to_string(), w.as_fd());
        self.conn.flush().map_err(|e| format!("flush: {e}"))?;
        drop(w);
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).map_err(|e| format!("read: {e}"))?;
        Ok(buf)
    }
    fn current(&mut self) -> Result<Option<(ExtDataControlOfferV1, Vec<String>)>, String> {
        self.wait_selection()?;
        Ok(self.state.selection.clone().flatten())
    }
}

fn read_dir(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let types = std::fs::read_to_string(dir.join("types.txt")).map_err(|e| format!("types.txt: {e}"))?;
    types
        .lines()
        .enumerate()
        .map(|(i, t)| Ok((t.to_string(), std::fs::read(dir.join(format!("{i}.bin"))).map_err(|e| format!("{i}.bin: {e}"))?)))
        .collect()
}

fn main() {
    guard();
    START.get_or_init(Instant::now);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        say(&format!("error {e}"));
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut ctx = connect()?;
    match args.first().map(String::as_str) {
        Some("types") => {
            let cur = ctx.current()?;
            println!("{}", cur.map(|(_, t)| t.join(" ")).unwrap_or_else(|| "(empty)".into()));
        }
        Some("get") => {
            let mime = args.get(1).ok_or("get MIME")?;
            let (offer, _) = ctx.current()?.ok_or("selection empty")?;
            let data = ctx.receive(&offer, mime)?;
            std::io::stdout().write_all(&data).map_err(|e| e.to_string())?;
        }
        Some("save") => {
            let dir = PathBuf::from(args.get(1).ok_or("save DIR")?);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let t0 = Instant::now();
            let (offer, types) = ctx.current()?.ok_or("selection empty")?;
            let mut total = 0;
            for (i, t) in types.iter().enumerate() {
                let data = ctx.receive(&offer, t)?;
                total += data.len();
                std::fs::write(dir.join(format!("{i}.bin")), &data).map_err(|e| e.to_string())?;
            }
            std::fs::write(dir.join("types.txt"), types.join("\n") + "\n").map_err(|e| e.to_string())?;
            say(&format!("saved {} types, {total} bytes in {} ms", types.len(), t0.elapsed().as_millis()));
        }
        Some("offer") => {
            let dir = PathBuf::from(args.get(1).ok_or("offer DIR")?);
            let content = read_dir(&dir)?;
            let source = ctx.mgr.create_data_source(&ctx.qh, ());
            for (t, _) in &content {
                source.offer(t.clone());
            }
            ctx.state.content = Arc::new(content);
            ctx.device.set_selection(Some(&source));
            ctx.conn.flush().map_err(|e| e.to_string())?;
            say(&format!("offering {} types", ctx.state.content.len()));
            // stdin EOF beendet; Ereignisse im Hauptthread.
            let eof = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let e2 = eof.clone();
            std::thread::spawn(move || {
                let mut sink = Vec::new();
                let _ = std::io::stdin().read_to_end(&mut sink);
                e2.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            while !ctx.state.cancelled && !eof.load(std::sync::atomic::Ordering::SeqCst) {
                ctx.queue.dispatch_pending(&mut ctx.state).map_err(|e| e.to_string())?;
                ctx.conn.flush().map_err(|e| e.to_string())?;
                if let Some(g) = ctx.queue.prepare_read() {
                    let mut p = [libc_poll(g.connection_fd())];
                    if poll(&mut p, 20) > 0 {
                        let _ = g.read();
                    }
                }
            }
            say("offer ends");
        }
        Some("watch") => {
            let respect = args.get(1).map(String::as_str) == Some("respect");
            let mut seen = 0;
            loop {
                ctx.queue.blocking_dispatch(&mut ctx.state).map_err(|e| e.to_string())?;
                if ctx.state.selections_seen == seen {
                    continue;
                }
                seen = ctx.state.selections_seen;
                let Some((offer, types)) = ctx.state.selection.clone().flatten() else {
                    say("selection empty");
                    continue;
                };
                say(&format!("selection #{seen}: {}", types.join(" ")));
                let mut secret = false;
                if types.iter().any(|t| t == HINT) {
                    let v = ctx.receive(&offer, HINT)?;
                    secret = v == b"secret";
                    say(&format!("hint read: {:?}", String::from_utf8_lossy(&v)));
                }
                if respect && secret {
                    say("skipped (secret)");
                    continue;
                }
                if let Some(t) = types.iter().find(|t| t.starts_with("text/plain")) {
                    let d = ctx.receive(&offer, t)?;
                    say(&format!("stored {t} ({} bytes)", d.len()));
                }
            }
        }
        _ => return Err("usage: clip types|get MIME|save DIR|offer DIR|watch greedy|respect".into()),
    }
    Ok(())
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
fn libc_poll(fd: std::os::fd::BorrowedFd<'_>) -> PollFd {
    use std::os::fd::AsRawFd;
    PollFd { fd: fd.as_raw_fd(), events: 1, revents: 0 }
}
unsafe extern "C" {
    #[link_name = "poll"]
    fn c_poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
}
fn poll(fds: &mut [PollFd], timeout_ms: i32) -> i32 {
    // SAFETY: gültiges, veränderbares Feld von PollFd mit passender Länge.
    unsafe { c_poll(fds.as_mut_ptr(), fds.len() as u64, timeout_ms) }
}
