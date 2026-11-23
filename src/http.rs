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

//! Eigener HTTP/1.1-Client für alle Netzaufrufe von dictate: KI gestreamt und am Stück ([`crate::pipeline::ai`]:
//! Diktat-Modi, Textaktionen, Modellliste, Verbindungstest) und Modell-Downloads (`gui/downloads.rs`). Blockierend,
//! ohne Async-Runtime und ohne Thread je Anfrage, aber jedes Warten ist abbrechbar.
//!
//! Warum nicht ureq 2: ein blockiertes Lesen lässt sich dort nicht von außen beenden, und eine Leerlauf-Grenze je
//! Lesen gibt es nur ohne Gesamtfrist. Belege aus `ureq-2.12.1/src`: `stream.rs` `connect_socket` setzt
//! `timeout_read` nur, wenn keine Frist (`timeout`) besteht, und `DeadlineStream::fill_buf` stellt vor jedem Lesen die
//! Restzeit der Frist ein; der Socket (`Stream::socket`) ist `pub(crate)`, an ihn kommt man nur über einen eigenen
//! `TlsConnector`, also nie bei `http://` (lokale Server). Kurze Lese-Fristen mit Wiederholung gehen auch nicht:
//! `chunked/decoder.rs` `read_chunk_size` verliert bei einem Fehler die schon gelesenen Ziffern der Chunk-Größe.
//! ureq 3 hat Leerlauf und Gesamtfrist, aber keinen Abbruch (nur über das ausdrücklich unversionierte
//! Transport-Modul) und liest per Vorgabe Proxy-Variablen; reqwest/hyper brächten tokio, curl eine C-Abhängigkeit.
//!
//! Modell: der Socket gehört uns, Lese- und Schreibfrist sind kurz ([`POLL`]), und nach jedem Systemaufruf, der
//! nichts oder nur einen Teil liefert, werden Abbruch, Gesamtfrist und Leerlauf geprüft ([`Limits`]); alle Zerleger
//! sind fortsetzbar (Kopf, Chunks, TLS über `rustls::ClientConnection`). Auch der Verbindungsaufbau wartet nur in
//! Schritten: Namensauflösung in einem Hilfsthread, dessen Ergebnis verworfen werden darf (er hält nur den Hostnamen),
//! TCP nicht blockierend mit `poll(2)` über alle Adressen nach Happy Eyeballs (RFC 8305), sodass eine tote IPv6-Route
//! IPv4 nicht aufhält. Beim Verlassen schließt `Drop` die Verbindung, der Server hört auf zu erzeugen.
//!
//! Bewusst schmal: HTTP/1.1, `Connection: close`, keine Kompression (kein `Accept-Encoding`), kein Proxy, Weiterleitungen
//! nur ausdrücklich über [`get_following_https`] (Downloads). Kopfzeilen zerlegt `httparse`, die Rahmung folgt RFC 9112
//! §6.3 streng. TLS: rustls mit *ring*, TLS 1.2/1.3, Mozilla-Wurzeln aus `webpki-roots` (wie vorher ureq 2), Prüfung
//! von Kette und Hostname durch den Standard-Verifier; kein „Zertifikat ignorieren“.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

/// Lese-/Schreibfrist am Socket = Takt für Abbruch und Fristen
pub const POLL: Duration = Duration::from_millis(50);
/// Abstand zwischen zwei Verbindungsversuchen an verschiedene Adressen (RFC 8305 §5, „Connection Attempt Delay“)
const ATTEMPT_DELAY: Duration = Duration::from_millis(250);
/// Größter Antwortkopf (alle 1xx-Zwischenantworten zusammen mit dem endgültigen Kopf)
const MAX_HEAD: usize = 64 * 1024;
/// Höchstzahl der Kopfzeilen einer Antwort
const MAX_HEADERS: usize = 64;
/// So viel Fehlertext lesen wir höchstens
const MAX_ERROR_BODY: usize = 16 * 1024;
/// Längste Chunk-Größenzeile (mit Erweiterungen)
const MAX_CHUNK_LINE: usize = 1024;
/// Alle Trailer-Zeilen zusammen
const MAX_TRAILER: usize = 16 * 1024;
/// Rahmungs-Bytes (Größenzeilen, Zeilenenden, Trailer), die ohne Nutzdaten erlaubt sind; darüber höchstens
/// [`OVERHEAD_FACTOR`] je Nutzbyte. So begrenzen die Nutzdaten-Grenzen der Aufrufer auch die gelesenen Rohbytes.
const OVERHEAD_FREE: u64 = 64 * 1024;
const OVERHEAD_FACTOR: u64 = 8;
/// Höchstzahl verfolgter Weiterleitungen ([`get_following_https`])
pub const MAX_REDIRECTS: usize = 5;
/// Lesepuffer je Antwort
const READ_BUF: usize = 64 * 1024;

/// Fehler eines Aufrufs. Texte enthalten nie die URL (nur den Host), nie Kopfzeilenwerte und keinen Körper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Abbruch-Flag gesetzt
    Cancelled,
    /// Gesamtfrist ([`Limits::deadline`]) abgelaufen; Wert = [`Limits::timeout_ms`]
    Timeout(u64),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Cancelled => f.write_str("cancelled"),
            Error::Timeout(ms) => write!(f, "timed out after {ms} ms"),
            Error::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

fn other(m: impl Into<String>) -> Error {
    Error::Other(m.into())
}

/// Grenzen eines Aufrufs. Geprüft wird nach jedem Systemaufruf am Socket, gewartet höchstens [`POLL`] am Stück.
#[derive(Clone, Copy, Debug)]
pub struct Limits<'a> {
    /// Gesamtfrist ab Aufruf (`None` = keine, z. B. für Downloads)
    pub deadline: Option<Instant>,
    /// Für [`Error::Timeout`]
    pub timeout_ms: u64,
    /// Höchstdauer für Namensauflösung, TCP und TLS-Handshake zusammen (`None` = nur die Gesamtfrist)
    pub connect: Option<Duration>,
    /// So lange darf ab dem Antwortkopf kein Byte Klartext kommen (TLS-Teilstücke zählen nicht als Lebenszeichen)
    pub idle: Duration,
    /// Leerlauf schon vor dem Kopf messen (ab dem Senden der Anfrage). Für die KI aus: dort darf das erste Byte so
    /// lange dauern, wie das Modell braucht, es gilt nur die Gesamtfrist.
    pub idle_before_head: bool,
    pub cancel: Option<&'a AtomicBool>,
}

impl<'a> Limits<'a> {
    /// Gesamtfrist `timeout_ms` ab jetzt, Leerlauf `idle` ab dem Antwortkopf (KI-Aufrufe)
    pub fn total(timeout_ms: u64, idle: Duration, cancel: Option<&'a AtomicBool>) -> Limits<'a> {
        Limits { deadline: Some(Instant::now() + Duration::from_millis(timeout_ms)), timeout_ms, connect: None, idle, idle_before_head: false, cancel }
    }

    /// Ohne Gesamtfrist (Downloads beliebiger Größe): Verbindungsaufbau höchstens `connect`, danach höchstens `idle`
    /// ohne ein Byte, auch beim Warten auf den Kopf
    pub fn transfer(connect: Duration, idle: Duration, cancel: Option<&'a AtomicBool>) -> Limits<'a> {
        Limits { deadline: None, timeout_ms: 0, connect: Some(connect), idle, idle_before_head: true, cancel }
    }

    /// Darf weiter gewartet werden? `since` = letztes Klartext-Byte bzw. Senden der Anfrage (`None`: kein Leerlauf)
    fn check(&self, since: Option<Instant>) -> Result<(), Error> {
        if self.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        let now = Instant::now();
        if self.deadline.is_some_and(|d| now >= d) {
            return Err(Error::Timeout(self.timeout_ms));
        }
        if let Some(t) = since {
            if now.saturating_duration_since(t) >= self.idle {
                return Err(other(format!("the server sent nothing for {:.1} s", self.idle.as_secs_f64())));
            }
        }
        Ok(())
    }

    /// Wie [`Limits::check`] während des Verbindungsaufbaus (ab `started`), zusätzlich mit [`Limits::connect`]
    fn check_connect(&self, started: Instant) -> Result<(), Error> {
        self.check(None)?;
        match self.connect {
            Some(c) if started.elapsed() >= c => Err(other(format!("connection timed out after {:.1} s", c.as_secs_f64()))),
            _ => Ok(()),
        }
    }
}

fn would_block(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted)
}

fn lost(e: std::io::Error) -> Error {
    other(format!("connection lost: {e}"))
}

fn tls_err(e: rustls::Error) -> Error {
    other(format!("TLS error: {e}"))
}

// ---------------------------------------------------------------------------------------------------------------
// TLS-Konfiguration

fn client_config(roots: rustls::RootCertStore) -> Arc<rustls::ClientConfig> {
    let cfg = rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
        .expect("ring supports TLS 1.2 and 1.3")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

/// Produktion: immer die Mozilla-Wurzeln aus `webpki-roots`. Nur in den Tests dieses Moduls kann ein Thread eine
/// eigene Wurzel setzen ([`test_tls`]); außerhalb von `cfg(test)` gibt es diesen Weg nicht.
fn tls_config() -> Arc<rustls::ClientConfig> {
    #[cfg(test)]
    {
        if let Some(c) = test_tls::config() {
            return c;
        }
    }
    static C: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    C.get_or_init(|| client_config(rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() })).clone()
}

#[cfg(test)]
mod test_tls {
    use std::cell::RefCell;
    use std::sync::Arc;

    thread_local! {
        static CONFIG: RefCell<Option<Arc<rustls::ClientConfig>>> = const { RefCell::new(None) };
    }

    /// Ab jetzt vertraut dieser Thread nur `ca` (statt der Mozilla-Wurzeln); `None` stellt zurück
    pub fn set_root(ca: Option<rustls::pki_types::CertificateDer<'static>>) {
        let cfg = ca.map(|ca| {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(ca).expect("test CA");
            super::client_config(roots)
        });
        CONFIG.with(|c| *c.borrow_mut() = cfg);
    }

    pub fn config() -> Option<Arc<rustls::ClientConfig>> {
        CONFIG.with(|c| c.borrow().clone())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Verbindung

/// Verbindung mit kurzen Fristen; jede Methode wartet nur in [`POLL`]-Schritten und prüft dazwischen `Limits`.
struct Conn {
    sock: TcpStream,
    tls: Option<Box<rustls::ClientConnection>>,
    /// TLS-Verbindung ohne `close_notify` beendet: das Ende kann von einem Angreifer im Pfad stammen
    unclean_eof: bool,
}

impl Conn {
    /// Schreibt ausstehende TLS-Daten auf den Socket; `check` nach jedem Schreibversuch, auch nach Teilerfolg
    fn flush_tls(tls: &mut rustls::ClientConnection, sock: &mut TcpStream, check: &dyn Fn() -> Result<(), Error>) -> Result<(), Error> {
        while tls.wants_write() {
            match tls.write_tls(sock) {
                Ok(_) => {}
                Err(e) if would_block(&e) => {}
                Err(e) => return Err(lost(e)),
            }
            check()?;
        }
        Ok(())
    }

    fn handshake(&mut self, check: &dyn Fn() -> Result<(), Error>) -> Result<(), Error> {
        let Some(tls) = &mut self.tls else { return Ok(()) };
        while tls.is_handshaking() {
            Self::flush_tls(tls, &mut self.sock, check)?;
            if !tls.is_handshaking() {
                break;
            }
            match tls.read_tls(&mut self.sock) {
                Ok(0) => return Err(other("connection closed during the TLS handshake")),
                Ok(_) => {
                    if let Err(e) = tls.process_new_packets() {
                        // die Warnung (Alert) an den Server ist nur höflich; ihr Scheitern ändert nichts
                        let _ = tls.write_tls(&mut self.sock);
                        return Err(tls_err(e));
                    }
                }
                Err(e) if would_block(&e) => {}
                Err(e) => return Err(lost(e)),
            }
            check()?;
        }
        Self::flush_tls(tls, &mut self.sock, check)
    }

    fn send(&mut self, mut data: &[u8], lim: &Limits) -> Result<(), Error> {
        match &mut self.tls {
            None => {
                while !data.is_empty() {
                    lim.check(None)?;
                    match self.sock.write(data) {
                        Ok(0) => return Err(other("connection closed while sending")),
                        Ok(n) => data = &data[n..],
                        Err(e) if would_block(&e) => {}
                        Err(e) => return Err(lost(e)),
                    }
                }
                Ok(())
            }
            Some(tls) => {
                // Puffer ohne Grenze (siehe `open`): nimmt alles, `flush_tls` schreibt es in Schritten
                tls.writer().write_all(data).map_err(lost)?;
                Self::flush_tls(tls, &mut self.sock, &|| lim.check(None))
            }
        }
    }

    /// Liest Klartext; 0 = Verbindung zu Ende ([`Conn::unclean_eof`] sagt, ob sauber). Wartet in Schritten, `since`
    /// wie bei [`Limits::check`]. Geprüft wird vor dem ersten Lesen und nach jedem Socket-Lesen, auch wenn es Bytes
    /// lieferte: ein Server, der einen TLS-Record byteweise tröpfeln lässt, liefert lange keinen Klartext, darf aber
    /// Abbruch, Frist und Leerlauf nicht aufhalten.
    fn recv(&mut self, buf: &mut [u8], lim: &Limits, since: Option<Instant>) -> Result<usize, Error> {
        lim.check(since)?;
        let Some(tls) = &mut self.tls else {
            loop {
                match self.sock.read(buf) {
                    Ok(n) => return Ok(n),
                    Err(e) if would_block(&e) => lim.check(since)?,
                    Err(e) => return Err(lost(e)),
                }
            }
        };
        let mut eof = false;
        loop {
            match tls.reader().read(buf) {
                // 0 nur nach close_notify: sauberes Ende
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    self.unclean_eof = true;
                    return Ok(0);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if eof {
                        self.unclean_eof = true;
                        return Ok(0);
                    }
                }
                Err(e) => return Err(lost(e)),
            }
            Self::flush_tls(tls, &mut self.sock, &|| lim.check(since))?;
            match tls.read_tls(&mut self.sock) {
                Ok(0) => eof = true,
                Ok(_) => {}
                Err(e) if would_block(&e) => {}
                Err(e) => return Err(lost(e)),
            }
            lim.check(since)?;
            tls.process_new_packets().map_err(tls_err)?;
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // sofort beide Richtungen schließen; der Server sieht das Ende und hört auf zu erzeugen. Kein close_notify:
        // das müsste geschrieben werden und könnte blockieren.
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
    }
}

/// Löst `lookup` in einem Hilfsthread auf und wartet in [`POLL`]-Schritten mit Abbruch und Fristen. Bei Abbruch läuft
/// der Thread zu Ende (`getaddrinfo` lässt sich nicht unterbrechen), hält aber nur den Hostnamen; sein Ergebnis wird
/// verworfen.
fn resolve<F>(lookup: F, lim: &Limits, started: Instant) -> Result<std::io::Result<Vec<SocketAddr>>, Error>
where
    F: FnOnce() -> std::io::Result<Vec<SocketAddr>> + Send + 'static,
{
    lim.check_connect(started)?;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("dictate-dns".into())
        .spawn(move || {
            let _ = tx.send(lookup());
        })
        .map_err(|e| other(format!("cannot start the name lookup: {e}")))?;
    loop {
        match rx.recv_timeout(POLL) {
            Ok(r) => return Ok(r),
            Err(mpsc::RecvTimeoutError::Timeout) => lim.check_connect(started)?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Err(std::io::Error::other("name lookup failed"))),
        }
    }
}

/// Reihenfolge der Verbindungsversuche nach RFC 8305 §4: die Reihenfolge des Resolvers (RFC 6724) bleibt je
/// Adressfamilie erhalten, die Familien wechseln sich ab, beginnend mit der Familie der ersten Adresse.
fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first_v6) = addrs.first().map(SocketAddr::is_ipv6) else { return addrs };
    let mut seen = std::collections::HashSet::new();
    let addrs: Vec<SocketAddr> = addrs.into_iter().filter(|a| seen.insert(*a)).collect();
    let (a, b): (Vec<SocketAddr>, Vec<SocketAddr>) = addrs.into_iter().partition(|x| x.is_ipv6() == first_v6);
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut a, mut b) = (a.into_iter(), b.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return out,
            (x, y) => out.extend(x.into_iter().chain(y)),
        }
    }
}

enum Attempt {
    Connected(socket2::Socket),
    Pending(socket2::Socket),
}

/// Startet einen nicht blockierenden Verbindungsversuch
fn start_attempt(a: SocketAddr) -> std::io::Result<Attempt> {
    let s = socket2::Socket::new(socket2::Domain::for_address(a), socket2::Type::STREAM, Some(socket2::Protocol::TCP))?;
    s.set_nonblocking(true)?;
    match s.connect(&a.into()) {
        Ok(()) => Ok(Attempt::Connected(s)),
        Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) || e.kind() == ErrorKind::WouldBlock => Ok(Attempt::Pending(s)),
        Err(e) => Err(e),
    }
}

/// Warum keine Verbindung zustande kam
enum ConnectFail {
    /// Abbruch oder Frist
    Limit(Error),
    /// Alle Adressen gescheitert; Text des letzten Fehlers
    Unreachable(String),
}

/// Verbindet mit der ersten Adresse, die antwortet (Happy Eyeballs, RFC 8305 §5): ein Versuch nach dem anderen im
/// Abstand von [`ATTEMPT_DELAY`], laufende Versuche bleiben offen, scheitert einer, startet der nächste sofort. Gewartet
/// wird mit `poll(2)` in [`POLL`]-Schritten; der erste erfolgreiche Socket gewinnt, die übrigen werden geschlossen.
fn connect_any(addrs: &[SocketAddr], lim: &Limits, started: Instant) -> Result<TcpStream, ConnectFail> {
    let mut pending: Vec<socket2::Socket> = Vec::new();
    let mut next = 0;
    // Start des letzten Versuchs; `None` = der nächste darf sofort beginnen
    let mut last_start: Option<Instant> = None;
    let mut last_err: Option<std::io::Error> = None;
    loop {
        lim.check_connect(started).map_err(ConnectFail::Limit)?;
        let due = last_start.is_none_or(|t| t.elapsed() >= ATTEMPT_DELAY);
        if next < addrs.len() && (pending.is_empty() || due) {
            let a = addrs[next];
            next += 1;
            match start_attempt(a) {
                Ok(Attempt::Connected(s)) => return Ok(s.into()),
                Ok(Attempt::Pending(s)) => {
                    pending.push(s);
                    last_start = Some(Instant::now());
                }
                Err(e) => {
                    last_err = Some(e);
                    last_start = None;
                }
            }
            continue;
        }
        if pending.is_empty() {
            let why = last_err.map_or_else(|| "no address".to_string(), |e| e.to_string());
            return Err(ConnectFail::Unreachable(why));
        }
        let wait = match last_start {
            Some(t) if next < addrs.len() => ATTEMPT_DELAY.saturating_sub(t.elapsed()).min(POLL),
            _ => POLL,
        };
        let mut fds: Vec<libc::pollfd> = pending.iter().map(|s| libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLOUT, revents: 0 }).collect();
        let ms = wait.as_millis().clamp(1, POLL.as_millis()) as libc::c_int;
        // SAFETY: `fds` ist ein gültiges, beschreibbares Feld mit `fds.len()` Einträgen; die Deskriptoren gehören den
        // Sockets in `pending`, die während des Aufrufs leben.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(ConnectFail::Unreachable(e.to_string()));
        }
        for i in (0..fds.len()).rev() {
            if fds[i].revents == 0 {
                continue;
            }
            let s = pending.remove(i);
            // SO_ERROR sagt, ob der Aufbau gelang; `peer_addr` bestätigt die bestehende Verbindung
            let err = match s.take_error() {
                Ok(None) => match s.peer_addr() {
                    Ok(_) => return Ok(s.into()),
                    Err(e) => e,
                },
                Ok(Some(e)) | Err(e) => e,
            };
            last_err = Some(err);
            last_start = None;
        }
    }
}

/// Baut die Verbindung auf: Namensauflösung, TCP, bei `https` der TLS-Handshake. `host` nur für Meldungen.
fn open(u: &url::Url, host: &str, https: bool, lim: &Limits) -> Result<Conn, Error> {
    let started = Instant::now();
    let unreachable = |why: &str| other(format!("cannot reach server {host}: {why}"));
    let port = u.port_or_known_default().ok_or_else(|| unreachable("no port"))?;
    let (addrs, name) = match u.host() {
        Some(url::Host::Ipv4(ip)) => (vec![SocketAddr::new(ip.into(), port)], rustls::pki_types::ServerName::IpAddress(std::net::IpAddr::from(ip).into())),
        Some(url::Host::Ipv6(ip)) => (vec![SocketAddr::new(ip.into(), port)], rustls::pki_types::ServerName::IpAddress(std::net::IpAddr::from(ip).into())),
        Some(url::Host::Domain(d)) => {
            let name = rustls::pki_types::ServerName::try_from(d.to_string()).map_err(|e| other(format!("invalid server name: {e}")))?;
            let d = d.to_string();
            let addrs = resolve(move || (d.as_str(), port).to_socket_addrs().map(Iterator::collect), lim, started)?.map_err(|e| unreachable(&e.to_string()))?;
            (addrs, name)
        }
        None => return Err(unreachable("no host")),
    };
    let sock = match connect_any(&interleave(addrs), lim, started) {
        Ok(s) => s,
        Err(ConnectFail::Limit(e)) => return Err(e),
        Err(ConnectFail::Unreachable(why)) => return Err(unreachable(&why)),
    };
    let setup = |s: &TcpStream| -> std::io::Result<()> {
        s.set_nonblocking(false)?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(POLL))?;
        s.set_write_timeout(Some(POLL))
    };
    setup(&sock).map_err(lost)?;
    let tls = if https {
        let mut tls = rustls::ClientConnection::new(tls_config(), name).map_err(tls_err)?;
        tls.set_buffer_limit(None);
        Some(Box::new(tls))
    } else {
        None
    };
    let mut conn = Conn { sock, tls, unclean_eof: false };
    conn.handshake(&|| lim.check_connect(started))?;
    Ok(conn)
}

// ---------------------------------------------------------------------------------------------------------------
// Anfrage

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

/// `token` nach RFC 9110 §5.6.2
fn valid_name(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Sichtbares ASCII, Leerzeichen und Tab; nicht mit Leerraum am Rand (RFC 9110 §5.5, ohne obs-text)
fn valid_value(v: &str) -> bool {
    v.bytes().all(|b| b == b'\t' || (0x20..0x7f).contains(&b)) && !v.starts_with([' ', '\t']) && !v.ends_with([' ', '\t'])
}

/// Diese Kopfzeilen setzt der Client selbst; vom Aufrufer kommende werden verworfen
fn reserved(k: &str) -> bool {
    ["host", "content-length", "connection", "transfer-encoding", "user-agent"].iter().any(|r| k.eq_ignore_ascii_case(r))
}

/// Verbindet, sendet `method url` mit `headers` (und bei `Some` dem Körper samt `Content-Length`) und liest den
/// Antwortkopf. Keine Weiterleitungen: ein 3xx kommt als Antwort zurück. Kopfzeilen müssen gültig sein
/// ([`valid_name`], [`valid_value`]), sonst Fehler ohne den Wert.
pub fn request(method: Method, url: &str, headers: &[(&str, String)], body: Option<&[u8]>, lim: &Limits) -> Result<Response, Error> {
    let u = url::Url::parse(url).map_err(|e| other(format!("invalid server address: {e}")))?;
    let https = match u.scheme() {
        "https" => true,
        "http" => false,
        s => return Err(other(format!("unsupported scheme {s}"))),
    };
    if !u.username().is_empty() || u.password().is_some() {
        return Err(other("server address must not contain a user name or password"));
    }
    if let Some((k, _)) = headers.iter().find(|(k, v)| !valid_name(k) || !valid_value(v)) {
        return Err(other(if valid_name(k) { format!("invalid value for request header {k}") } else { "invalid request header name".to_string() }));
    }
    lim.check(None)?;
    let host = u.host_str().unwrap_or("").to_string();
    let mut conn = open(&u, &host, https, lim)?;

    let host_header = &u[url::Position::BeforeHost..url::Position::AfterPort];
    let path = &u[url::Position::BeforePath..url::Position::AfterQuery];
    let path = if path.is_empty() { "/" } else { path };
    let mut head = format!("{} {path} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: dictate/{}\r\n", method.as_str(), env!("CARGO_PKG_VERSION"));
    if body.is_some() || method == Method::Post {
        head.push_str(&format!("Content-Length: {}\r\n", body.map_or(0, <[u8]>::len)));
    }
    head.push_str("Connection: close\r\n");
    for (k, v) in headers.iter().filter(|(k, _)| !reserved(k)) {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut req = head.into_bytes();
    req.extend_from_slice(body.unwrap_or_default());
    conn.send(&req, lim)?;
    read_head(conn, lim, Instant::now())
}

/// `GET url` und Weiterleitungen folgen (301, 302, 303, 307, 308; höchstens [`MAX_REDIRECTS`]), aber nur zu
/// `https://`-Zielen ohne Zugangsdaten in der URL. Wechselt der Ursprung (Schema, Host, Port), fallen Kopfzeilen mit
/// Zugangsdaten weg (`Authorization`, `Proxy-Authorization`, `Cookie`, `X-Api-Key`). Für Downloads; die KI folgt nie.
pub fn get_following_https(url: &str, headers: &[(&str, String)], lim: &Limits) -> Result<Response, Error> {
    let mut cur = url::Url::parse(url).map_err(|e| other(format!("invalid address: {e}")))?;
    let mut headers: Vec<(&str, String)> = headers.to_vec();
    for _ in 0..=MAX_REDIRECTS {
        let resp = request(Method::Get, cur.as_str(), &headers, None, lim)?;
        if !matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(resp);
        }
        let loc = resp.header("location").ok_or_else(|| other(format!("redirect without Location (HTTP {})", resp.status)))?;
        let next = cur.join(loc).map_err(|_| other("bad redirect target"))?;
        if next.scheme() != "https" {
            return Err(other(format!("refusing redirect to {}://{}", next.scheme(), next.host_str().unwrap_or(""))));
        }
        if !next.username().is_empty() || next.password().is_some() {
            return Err(other("refusing redirect to an address with a user name or password"));
        }
        if next.origin() != cur.origin() {
            headers.retain(|(k, _)| !["authorization", "proxy-authorization", "cookie", "x-api-key"].iter().any(|s| k.eq_ignore_ascii_case(s)));
        }
        cur = next;
    }
    Err(other("too many redirects"))
}

// ---------------------------------------------------------------------------------------------------------------
// Antwortkopf

/// Ende des Kopfes in `buf` (Index nach der Leerzeile, Zeilenenden `\r\n` oder `\n`, auch gemischt), gesucht ab
/// `from`: bis dorthin wurde schon ohne Treffer gesucht. Jedes Byte wird so nur einige Male angesehen, auch wenn der
/// Server byteweise sendet.
fn head_end(buf: &[u8], from: usize) -> Option<usize> {
    let mut i = from.saturating_sub(2).min(buf.len());
    while let Some(off) = buf[i..].iter().position(|&b| b == b'\n') {
        let p = i + off;
        match (buf.get(p + 1), buf.get(p + 2)) {
            (Some(b'\n'), _) => return Some(p + 2),
            (Some(b'\r'), Some(b'\n')) => return Some(p + 3),
            (None, _) | (Some(b'\r'), None) => return None,
            _ => i = p + 1,
        }
    }
    None
}

/// Zerlegter Antwortkopf
#[derive(Debug)]
struct Head {
    status: u16,
    /// 0 = HTTP/1.0, 1 = HTTP/1.1
    version: u8,
    /// Namen klein geschrieben, in Reihenfolge
    headers: Vec<(String, String)>,
}

impl Head {
    /// Alle Listenelemente aller Felder `name` (RFC 9110 §5.6.1), klein, ohne leere
    fn list(&self, name: &str) -> Vec<String> {
        self.headers
            .iter()
            .filter(|(k, _)| k == name)
            .flat_map(|(_, v)| v.split(','))
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Zerlegt genau einen Kopf (`raw` endet mit der Leerzeile) über `httparse`
fn parse_head(raw: &[u8]) -> Result<Head, Error> {
    let invalid = || other("invalid response from the server");
    let mut hs = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut r = httparse::Response::new(&mut hs);
    match r.parse(raw) {
        Ok(httparse::Status::Complete(n)) if n == raw.len() => {}
        Ok(_) => return Err(invalid()),
        Err(httparse::Error::TooManyHeaders) => return Err(other("too many header fields in the response")),
        Err(e) => return Err(other(format!("invalid response from the server ({e})"))),
    }
    let status = r.code.filter(|c| (100..=599).contains(c)).ok_or_else(invalid)?;
    let version = r.version.ok_or_else(invalid)?;
    let headers = r.headers.iter().map(|h| (h.name.to_ascii_lowercase(), String::from_utf8_lossy(h.value).into_owned())).collect();
    Ok(Head { status, version, headers })
}

/// Rahmung des Antwortkörpers
enum Framing {
    Length(u64),
    Chunked(Chunked),
    Close,
}

/// Rahmung nach RFC 9112 §6.3 (Antwort auf GET/POST): 1xx/204/304 ohne Körper; `Transfer-Encoding` geht vor
/// `Content-Length`, muss genau `chunked` sein (andere Codierungen können wir nicht entpacken, in HTTP/1.0 ist es
/// fehlerhafte Rahmung); `Content-Length` nur dezimal, mehrere Werte nur, wenn alle gleich; sonst bis zum Ende der
/// Verbindung. Komprimierte Körper (`Content-Encoding`) sind ein Fehler: wir bitten nie darum.
fn framing(h: &Head) -> Result<(Framing, Option<u64>), Error> {
    if (100..200).contains(&h.status) || matches!(h.status, 204 | 304) {
        return Ok((Framing::Length(0), Some(0)));
    }
    // Meldungen ohne Werte aus dem Kopf: ein Server könnte darin die Eingabe zitieren, die Meldung geht bis an Clients
    if h.list("content-encoding").iter().any(|c| c != "identity") {
        return Err(other("the server sent a compressed response, which is not supported"));
    }
    let te = h.list("transfer-encoding");
    if !te.is_empty() {
        if h.version == 0 {
            return Err(other("invalid response framing (Transfer-Encoding in HTTP/1.0)"));
        }
        if te != ["chunked"] {
            return Err(other("the server used a transfer coding that is not supported"));
        }
        return Ok((Framing::Chunked(Chunked::new()), None));
    }
    let mut len = None;
    for v in h.headers.iter().filter(|(k, _)| k == "content-length").flat_map(|(_, v)| v.split(',')) {
        let v = v.trim_matches([' ', '\t']);
        let n = (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).then(|| v.parse::<u64>().ok()).flatten();
        match (n, len) {
            (None, _) => return Err(other("invalid Content-Length in the response")),
            (Some(n), Some(m)) if n != m => return Err(other("conflicting Content-Length in the response")),
            (Some(n), _) => len = Some(n),
        }
    }
    Ok((len.map_or(Framing::Close, Framing::Length), len))
}

/// Liest den Antwortkopf (1xx-Zwischenantworten werden übersprungen, 101 ist ein Fehler); vor dem Kopf gilt der
/// Leerlauf nur mit [`Limits::idle_before_head`] (gemessen ab `sent`).
fn read_head(mut conn: Conn, lim: &Limits, sent: Instant) -> Result<Response, Error> {
    let mut buf = Vec::new();
    let mut scanned = 0;
    // Bytes schon verarbeiteter Zwischenantworten
    let mut used = 0;
    let mut chunk = vec![0u8; READ_BUF];
    let mut last = sent;
    loop {
        if let Some(end) = head_end(&buf, scanned) {
            if used + end > MAX_HEAD {
                return Err(other("response header too large"));
            }
            let head = parse_head(&buf[..end])?;
            let rest = buf.split_off(end);
            if (100..200).contains(&head.status) {
                if head.status == 101 {
                    return Err(other("the server switched protocols unexpectedly (HTTP 101)"));
                }
                used += end;
                buf = rest;
                scanned = 0;
                continue;
            }
            let (framing, length) = framing(&head)?;
            let done = matches!(framing, Framing::Length(0));
            return Ok(Response { conn, status: head.status, headers: head.headers, framing, length, pending: rest, last: Instant::now(), done, buf: chunk });
        }
        scanned = buf.len();
        if used + buf.len() > MAX_HEAD {
            return Err(other("response header too large"));
        }
        let n = conn.recv(&mut chunk, lim, lim.idle_before_head.then_some(last))?;
        if n == 0 {
            return Err(other("the server closed the connection without an answer"));
        }
        last = Instant::now();
        buf.extend_from_slice(&chunk[..n]);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Antwortkörper

#[derive(Debug)]
enum ChunkState {
    /// Größenzeile (in `line`)
    Size,
    Data(u64),
    /// Zeilenende nach den Daten; `true` = `\r` gesehen
    DataEnd(bool),
    /// Trailer-Zeile (in `line`)
    Trailer,
    Done,
}

/// Fortsetzbarer, strenger Zerleger für `Transfer-Encoding: chunked` (RFC 9112 §7.1): Größe 1–16 Hex-Ziffern, danach
/// höchstens Erweiterungen nach `;`; Zeilenende `\r\n` oder (RFC 9112 §2.2) ein nacktes `\n`, sonst kein `\r`.
/// Begrenzt: Größenzeile [`MAX_CHUNK_LINE`], Trailer zusammen [`MAX_TRAILER`], Rahmungs-Bytes gegenüber Nutzdaten
/// ([`OVERHEAD_FREE`], [`OVERHEAD_FACTOR`]).
#[derive(Debug)]
pub(crate) struct Chunked {
    state: ChunkState,
    line: Vec<u8>,
    trailer: usize,
    payload: u64,
    overhead: u64,
}

impl Chunked {
    pub(crate) fn new() -> Chunked {
        Chunked { state: ChunkState::Size, line: Vec::new(), trailer: 0, payload: 0, overhead: 0 }
    }

    pub(crate) fn is_done(&self) -> bool {
        matches!(self.state, ChunkState::Done)
    }

    /// Größe aus einer Größenzeile ohne Zeilenende
    fn size(line: &[u8]) -> Option<u64> {
        if line.iter().any(|&b| (b < 0x20 && b != b'\t') || b == 0x7f) {
            return None;
        }
        let hex = match line.iter().position(|&b| b == b';') {
            // BWS vor `;` (RFC 9112 §7.1.1); ohne Erweiterung kein Leerraum
            Some(i) => line[..i].trim_ascii_end(),
            None => line,
        };
        if hex.is_empty() || hex.len() > 16 || !hex.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        u64::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
    }

    /// Nimmt eine Zeile ohne Zeilenende entgegen; ein einzelnes `\r` am Ende gehört zum Zeilenende
    fn take_line(line: &mut Vec<u8>) -> Option<Vec<u8>> {
        let mut l = std::mem::take(line);
        if l.last() == Some(&b'\r') {
            l.pop();
        }
        (!l.contains(&b'\r')).then_some(l)
    }

    /// Ein Rahmungs-Byte mehr; zu viel Rahmung gegenüber den Nutzdaten ist ein Fehler. Je Byte geprüft, damit das
    /// Ergebnis nicht davon abhängt, wie die Eingabe in Stücke geteilt ankommt.
    fn framing_byte(&mut self) -> Result<(), Error> {
        self.overhead += 1;
        if self.overhead > OVERHEAD_FREE.saturating_add(self.payload.saturating_mul(OVERHEAD_FACTOR)) {
            return Err(other("too much chunk framing in the response"));
        }
        Ok(())
    }

    /// Verbraucht `input`, hängt Nutzdaten an `out`; Fehler bei kaputter oder übergroßer Rahmung. Nach dem Ende
    /// (`is_done`) wird der Rest ignoriert.
    pub(crate) fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        let bad = || other("invalid chunked encoding in the response");
        let mut i = 0;
        while i < input.len() {
            if let ChunkState::Data(left) = &mut self.state {
                let take = (*left).min((input.len() - i) as u64) as usize;
                out.extend_from_slice(&input[i..i + take]);
                i += take;
                *left -= take as u64;
                self.payload += take as u64;
                if *left == 0 {
                    self.state = ChunkState::DataEnd(false);
                }
                continue;
            }
            if self.is_done() {
                break;
            }
            let b = input[i];
            i += 1;
            self.framing_byte()?;
            match &mut self.state {
                ChunkState::Size => {
                    if b != b'\n' {
                        if self.line.len() >= MAX_CHUNK_LINE {
                            return Err(bad());
                        }
                        self.line.push(b);
                        continue;
                    }
                    let line = Self::take_line(&mut self.line).ok_or_else(bad)?;
                    let n = Self::size(&line).ok_or_else(bad)?;
                    self.state = if n == 0 { ChunkState::Trailer } else { ChunkState::Data(n) };
                }
                ChunkState::DataEnd(cr) => match b {
                    b'\r' if !*cr => *cr = true,
                    b'\n' => self.state = ChunkState::Size,
                    _ => return Err(bad()),
                },
                ChunkState::Trailer => {
                    self.trailer += 1;
                    if self.trailer > MAX_TRAILER {
                        return Err(other("response trailer too large"));
                    }
                    if b != b'\n' {
                        self.line.push(b);
                        continue;
                    }
                    let line = Self::take_line(&mut self.line).ok_or_else(bad)?;
                    if line.is_empty() {
                        self.state = ChunkState::Done;
                    }
                }
                ChunkState::Data(_) | ChunkState::Done => unreachable!("handled above"),
            }
        }
        Ok(())
    }
}

/// Laufende Antwort: Status und Kopfzeilen, dann der Körper stückweise über [`Response::next`]
pub struct Response {
    conn: Conn,
    pub status: u16,
    headers: Vec<(String, String)>,
    framing: Framing,
    length: Option<u64>,
    /// Schon gelesen, noch nicht zerlegt
    pending: Vec<u8>,
    /// Letztes Klartext-Byte (für den Leerlauf)
    last: Instant,
    done: bool,
    buf: Vec<u8>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response").field("status", &self.status).field("length", &self.length).finish_non_exhaustive()
    }
}

impl Response {
    /// Erster Wert der Kopfzeile `name` (Groß-/Kleinschreibung egal)
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// `Content-Type` ohne Parameter, klein
    pub fn media(&self) -> String {
        self.header("content-type").and_then(|v| v.split(';').next()).unwrap_or("").trim().to_ascii_lowercase()
    }

    /// Länge des Körpers laut `Content-Length` (`None` bei chunked oder Ende durch Schließen)
    pub fn content_length(&self) -> Option<u64> {
        self.length
    }

    /// Nächstes Stück des Körpers; `None` = Körper vollständig. Wartet in Schritten mit Abbruch, Gesamtfrist und
    /// Leerlauf seit dem letzten Byte. Endet eine TLS-Verbindung ohne `close_notify`, ist ein Körper ohne eigene
    /// Längenangabe abgeschnitten (Fehler), denn das Ende könnte von einem Angreifer im Pfad stammen.
    pub fn next(&mut self, lim: &Limits) -> Result<Option<Vec<u8>>, Error> {
        loop {
            if self.done {
                return Ok(None);
            }
            if !self.pending.is_empty() {
                let input = std::mem::take(&mut self.pending);
                let mut out = Vec::new();
                match &mut self.framing {
                    Framing::Chunked(c) => {
                        c.feed(&input, &mut out)?;
                        self.done = c.is_done();
                    }
                    Framing::Length(left) => {
                        let take = (*left).min(input.len() as u64) as usize;
                        out.extend_from_slice(&input[..take]);
                        *left -= take as u64;
                        self.done = *left == 0;
                    }
                    Framing::Close => out = input,
                }
                if !out.is_empty() {
                    return Ok(Some(out));
                }
                continue;
            }
            let n = self.conn.recv(&mut self.buf, lim, Some(self.last))?;
            if n == 0 {
                return match self.framing {
                    Framing::Close if self.conn.unclean_eof => Err(other("the response was cut off (TLS connection closed without close_notify)")),
                    Framing::Close => {
                        self.done = true;
                        Ok(None)
                    }
                    _ => Err(other("connection closed before the response was complete")),
                };
            }
            self.last = Instant::now();
            self.pending.extend_from_slice(&self.buf[..n]);
        }
    }

    /// Ganzer Körper, höchstens `max` Bytes (mehr ist ein Fehler)
    pub fn read_to_end(&mut self, lim: &Limits, max: u64) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        while let Some(b) = self.next(lim)? {
            if (out.len() + b.len()) as u64 > max {
                return Err(other("response too large"));
            }
            out.extend_from_slice(&b);
        }
        Ok(out)
    }

    /// Fehlertext einer Antwort mit Fehlerstatus (höchstens [`MAX_ERROR_BODY`]). Abbruch und Gesamtfrist kommen als
    /// Fehler zurück; andere Lesefehler (Verbindung weg, Leerlauf) beenden nur den Text.
    pub fn error_body(&mut self, lim: &Limits) -> Result<String, Error> {
        let mut body = Vec::new();
        while body.len() < MAX_ERROR_BODY {
            match self.next(lim) {
                Ok(Some(b)) => body.extend_from_slice(&b),
                Ok(None) => break,
                Err(e @ (Error::Cancelled | Error::Timeout(_))) => return Err(e),
                Err(_) => break,
            }
        }
        body.truncate(MAX_ERROR_BODY);
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}

#[cfg(test)]
mod tests;
