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

//! Tests des HTTP-Clients: Zerleger (auch mit Zufallseingaben), Kopf-Randfälle, Verbindungsaufbau mit Abbruch und
//! Happy Eyeballs, TLS offline gegen eine selbst erzeugte Test-CA (`rcgen`). Alles lokal, ohne Netz.

use super::*;
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn lim(ms: u64) -> Limits<'static> {
    Limits::total(ms, Duration::from_secs(5), None)
}

/// Setzt `flag` nach `ms`
fn cancel_after(flag: &Arc<AtomicBool>, ms: u64) {
    let f = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        f.store(true, Ordering::Relaxed);
    });
}

/// Liest einen Anfragekopf (bis zur Leerzeile)
fn read_request(r: &mut impl Read) -> Vec<u8> {
    let mut req = Vec::new();
    let mut b = [0u8; 1];
    while !req.ends_with(b"\r\n\r\n") {
        match r.read(&mut b) {
            Ok(1) => req.push(b[0]),
            _ => break,
        }
    }
    req
}

/// Ein lokaler Klartext-Server für genau eine Verbindung: liest den Anfragekopf und gibt den Socket an `script`
fn serve(script: impl FnOnce(TcpStream) + Send + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            read_request(&mut s);
            script(s);
        }
    });
    format!("http://127.0.0.1:{port}/x")
}

/// Server, der `raw` sendet und schließt
fn serve_raw(raw: impl Into<Vec<u8>>) -> String {
    let raw = raw.into();
    serve(move |mut s| {
        let _ = s.write_all(&raw);
    })
}

fn get(url: &str, l: &Limits) -> Result<(u16, Vec<u8>), Error> {
    let mut r = request(Method::Get, url, &[], None, l)?;
    let body = r.read_to_end(l, 1 << 20)?;
    Ok((r.status, body))
}

fn err_text(r: Result<(u16, Vec<u8>), Error>) -> String {
    r.expect_err("must fail").to_string()
}

// ---------------------------------------------------------------------------------------------------------------
// Chunked

fn decode(wire: &[u8]) -> Result<(Vec<u8>, bool), Error> {
    let mut c = Chunked::new();
    let mut out = Vec::new();
    c.feed(wire, &mut out)?;
    Ok((out, c.is_done()))
}

#[test]
fn chunked_decoder_is_resumable_at_every_byte() {
    let wire = b"4\r\nWiki\r\n6;ext=1\r\npedia \r\nE\r\nin \r\n\r\nchunks.\r\n0\r\nX-Trailer: 1\r\n\r\n";
    for step in 1..wire.len() {
        let mut c = Chunked::new();
        let mut out = Vec::new();
        for part in wire.chunks(step) {
            c.feed(part, &mut out).unwrap();
        }
        assert!(c.is_done(), "step {step}");
        assert_eq!(out, b"Wikipedia in \r\n\r\nchunks.", "step {step}");
    }
}

#[test]
fn chunked_decoder_is_strict() {
    // gültig: Großbuchstaben, BWS vor Erweiterung, nackte LF (RFC 9112 §2.2), Rest nach dem Ende wird ignoriert
    assert_eq!(decode(b"A\r\n0123456789\r\n0\r\n\r\n").unwrap(), (b"0123456789".to_vec(), true));
    assert_eq!(decode(b"5 ;x=\"y\"\r\nhello\r\n0\r\n\r\n").unwrap(), (b"hello".to_vec(), true));
    assert_eq!(decode(b"5\nhello\n0\n\n").unwrap(), (b"hello".to_vec(), true));
    assert_eq!(decode(b"1\r\na\r\n0\r\n\r\ngarbage").unwrap(), (b"a".to_vec(), true));
    assert_eq!(decode(b"00000000000000001\r\n").map(|_| ()), Err(other("invalid chunked encoding in the response")), "17 hex digits");
    assert!(decode(b"ffffffffffffffff\r\n").is_ok(), "16 hex digits fit u64");
    for bad in [
        &b"zz\r\n"[..],
        b"+5\r\nhello\r\n",
        b"-5\r\n",
        b" 5\r\nhello\r\n",
        b"5 \r\nhello\r\n",
        b"\r\n",
        b"5\r\r\nhello\r\n",
        b"5\x00\r\nhello\r\n",
        b"5\r\nhello\r\r\n",
        b"5\r\nhelloX\r\n",
        b"1\r\nab",
        b"0\r\nX: a\rb\r\n\r\n",
    ] {
        assert!(decode(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
    }
    // Größenzeile über 1 KiB
    let mut long = b"1;".to_vec();
    long.extend(std::iter::repeat_n(b'x', MAX_CHUNK_LINE));
    long.extend_from_slice(b"\r\na\r\n");
    assert!(decode(&long).is_err());
    // Trailer zusammen über 16 KiB, auch als endlose Folge kurzer Zeilen
    let mut c = Chunked::new();
    let mut out = Vec::new();
    c.feed(b"1\r\na\r\n0\r\n", &mut out).unwrap();
    let mut failed = false;
    for _ in 0..10_000 {
        if let Err(e) = c.feed(b"X-T: 1\r\n", &mut out) {
            assert_eq!(e.to_string(), "response trailer too large");
            failed = true;
            break;
        }
    }
    assert!(failed, "endless trailers must fail");
    // Kommentar-Chunks mit wenig Nutzdaten: Rahmung wächst nicht unbegrenzt gegenüber den Nutzdaten
    let mut c = Chunked::new();
    let unit = format!("1;{}\r\na\r\n", "e".repeat(1000));
    let mut fed = 0;
    let r = loop {
        if let Err(e) = c.feed(unit.as_bytes(), &mut out) {
            break e;
        }
        fed += 1;
        assert!(fed < 10_000, "must stop");
    };
    assert_eq!(r.to_string(), "too much chunk framing in the response");
}

// ---------------------------------------------------------------------------------------------------------------
// Antwortkopf

/// Rahmung eines vollständigen Kopfs als Text
fn frame(raw: &str) -> Result<String, Error> {
    let end = head_end(raw.as_bytes(), 0).ok_or_else(|| other("no end"))?;
    let h = parse_head(&raw.as_bytes()[..end])?;
    Ok(match framing(&h)? {
        (Framing::Length(n), _) => format!("length {n}"),
        (Framing::Chunked(_), _) => "chunked".into(),
        (Framing::Close, _) => "close".into(),
    })
}

#[test]
fn head_end_is_found_with_any_line_ending() {
    assert_eq!(head_end(b"HTTP/1.1 200 OK\r\n\r\nbody", 0), Some(19));
    assert_eq!(head_end(b"HTTP/1.1 200 OK\n\nbody", 0), Some(17));
    assert_eq!(head_end(b"HTTP/1.1 200 OK\r\nA: b\n\r\nbody", 0), Some(24));
    // `\n\n`-Kopf, Körper mit `\r\n\r\n`: getrennt wird am ersten Kopfende, nicht am späteren im Körper
    let raw = b"HTTP/1.1 200 OK\nContent-Length: 6\n\nab\r\n\r\n";
    assert_eq!(head_end(raw, 0), Some(35));
    assert_eq!(head_end(b"HTTP/1.1 200 OK\r\n\r", 0), None);
    assert_eq!(head_end(b"HTTP/1.1 200 OK\r\nX: y\r\n", 0), None);
    // fortgesetzte Suche findet dasselbe wie die Suche am Stück
    let raw = b"HTTP/1.1 200 OK\r\nA: b\r\n\r\n";
    for cut in 0..raw.len() {
        let first = head_end(&raw[..cut], 0);
        let again = head_end(raw, if first.is_none() { cut } else { 0 });
        assert_eq!(first.or(again), Some(raw.len()), "cut {cut}");
    }
}

#[test]
fn framing_follows_rfc_9112() {
    let ok = |raw: &str, want: &str| assert_eq!(frame(raw).unwrap(), want, "{raw:?}");
    let bad = |raw: &str| assert!(frame(raw).is_err(), "{raw:?} must fail");
    ok("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n", "length 5");
    ok("HTTP/1.1 200 OK\nContent-Length: 2\n\n", "length 2");
    ok("HTTP/1.1 200 OK\r\nContent-Length: 5, 5\r\nContent-Length: 5\r\n\r\n", "length 5");
    ok("HTTP/1.1 200 OK\r\nContent-Length: 007\r\n\r\n", "length 7");
    ok("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n", "chunked");
    ok("HTTP/1.1 200 OK\r\ntransfer-encoding: Chunked\r\n\r\n", "chunked");
    ok("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n", "close");
    ok("HTTP/1.0 200 OK\r\n\r\n", "close");
    ok("HTTP/1.1 204 No Content\r\nContent-Length: 10\r\n\r\n", "length 0");
    ok("HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n", "length 0");
    ok("HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\nContent-Length: 1\r\n\r\n", "length 1");
    bad("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Length: 6\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: 5, 6\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: +5\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: abc\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: 5 5\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: \r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Length: 99999999999999999999999\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked, chunked\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: gzip\r\n\r\n");
    bad("HTTP/1.0 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 5\r\n\r\n");
    bad("HTTP/1.1 +200 OK\r\n\r\n");
    bad("HTTP/1.1 600 Weird\r\n\r\n");
    bad("HTTP/1.1 99 Short\r\n\r\n");
    bad("HTTP/2 200 OK\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\nBad Header: x\r\n\r\n");
    bad("HTTP/1.1 200 OK\r\n folded: x\r\n\r\n");
    let many: String = (0..=MAX_HEADERS).map(|i| format!("X-{i}: v\r\n")).collect();
    assert_eq!(frame(&format!("HTTP/1.1 200 OK\r\n{many}\r\n")).unwrap_err().to_string(), "too many header fields in the response");
}

#[test]
fn framing_errors_never_quote_values_from_the_head() {
    // die Meldung geht bis an Clients (Befehlspalette), ein Server darf darin nichts zitieren
    for (raw, want) in [
        ("HTTP/1.1 200 OK\r\nContent-Encoding: quoted-marker\r\n\r\n", "the server sent a compressed response, which is not supported"),
        ("HTTP/1.1 200 OK\r\nTransfer-Encoding: quoted-marker, chunked\r\n\r\n", "the server used a transfer coding that is not supported"),
    ] {
        assert_eq!(frame(raw).unwrap_err().to_string(), want);
    }
}

#[test]
fn interim_responses_are_skipped_and_101_is_refused() {
    let url = serve_raw("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    assert_eq!(get(&url, &lim(5000)).unwrap(), (200, b"ok".to_vec()));
    let url = serve_raw("HTTP/1.1 101 Switching Protocols\r\nUpgrade: x\r\n\r\n");
    assert!(err_text(get(&url, &lim(5000))).contains("101"));
}

#[test]
fn bodies_end_where_the_framing_says() {
    // Content-Length kürzer als die Daten: der Rest wird nicht gelesen
    let url = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef");
    assert_eq!(get(&url, &lim(5000)).unwrap(), (200, b"abc".to_vec()));
    // Ende durch Schließen (Klartext): normales Ende
    let url = serve_raw("HTTP/1.0 200 OK\r\n\r\nuntil close");
    assert_eq!(get(&url, &lim(5000)).unwrap(), (200, b"until close".to_vec()));
    // EOF mitten im Chunk bzw. vor Content-Length
    let url = serve_raw("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab");
    assert!(err_text(get(&url, &lim(5000))).contains("before the response was complete"));
    let url = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab");
    assert!(err_text(get(&url, &lim(5000))).contains("before the response was complete"));
    // Server schließt ohne Antwort
    let url = serve_raw("");
    assert!(err_text(get(&url, &lim(5000))).contains("without an answer"));
}

#[test]
fn oversized_head_is_refused() {
    let big = format!("HTTP/1.1 200 OK\r\nX-Big: {}\r\n\r\n", "a".repeat(MAX_HEAD));
    assert_eq!(err_text(get(&serve_raw(big), &lim(5000))), "response header too large");
    // auch über viele Zwischenantworten verteilt
    let interim: String = std::iter::repeat_n(format!("HTTP/1.1 103 Early Hints\r\nLink: {}\r\n\r\n", "b".repeat(1000)), 70).collect();
    assert_eq!(err_text(get(&serve_raw(interim + "HTTP/1.1 200 OK\r\n\r\n"), &lim(5000))), "response header too large");
}

#[test]
fn head_sent_byte_by_byte_is_read() {
    let url = serve(|mut s| {
        s.set_nodelay(true).unwrap();
        for b in b"HTTP/1.1 200 OK\nContent-Length: 4\n\nbody" {
            let _ = s.write_all(&[*b]);
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    assert_eq!(get(&url, &lim(5000)).unwrap(), (200, b"body".to_vec()));
}

#[test]
fn request_headers_are_validated_before_connecting() {
    // Port 9 auf einer reservierten Adresse: ginge die Prüfung nicht vorher, käme ein Verbindungsfehler
    for (k, v) in [
        ("x-api-key", "secret\r\nX-Evil: 1"),
        ("x-api-key", "secret\0"),
        ("x-api-key", "s\u{e9}cret"),
        ("x-api-key", " secret"),
        ("bad name", "v"),
        ("", "v"),
        ("a:b", "v"),
    ] {
        let e = request(Method::Get, "http://192.0.2.1:9/", &[(k, v.to_string())], None, &lim(300)).unwrap_err().to_string();
        assert!(e.starts_with("invalid"), "{k:?}: {e}");
        assert!(!e.contains("secret"), "value leaked: {e}");
    }
    // Zugangsdaten in der URL
    assert!(request(Method::Get, "http://u:p@127.0.0.1:9/", &[], None, &lim(300)).unwrap_err().to_string().contains("user name"));
}

#[test]
fn request_line_and_reserved_headers() {
    let (tx, rx) = mpsc::channel();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut req = read_request(&mut s);
        let mut body = [0u8; 3];
        s.read_exact(&mut body).unwrap();
        req.extend_from_slice(&body);
        tx.send(String::from_utf8(req).unwrap()).unwrap();
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    });
    let url = format!("http://127.0.0.1:{port}/v1/x?q=1#frag");
    let h = [("Content-Length", "99".to_string()), ("Host", "evil".to_string()), ("x-api-key", "k".to_string())];
    let r = request(Method::Post, &url, &h, Some(b"abc"), &lim(5000)).unwrap();
    assert_eq!(r.status, 200);
    let req = rx.recv().unwrap();
    assert!(req.starts_with("POST /v1/x?q=1 HTTP/1.1\r\n"), "{req}");
    assert!(req.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")) && !req.contains("evil"), "{req}");
    // nicht `!contains("99")`: der zufällige Port der Anfrage kann „99“ enthalten
    assert!(req.contains("\r\nContent-Length: 3\r\n") && !req.contains("Content-Length: 99"), "{req}");
    assert!(req.contains("\r\nConnection: close\r\n") && req.contains("\r\nx-api-key: k\r\n") && req.ends_with("\r\n\r\nabc"), "{req}");
}

#[test]
fn error_body_passes_cancel_and_timeout_on() {
    let hang = |s: TcpStream| {
        let mut s = s;
        let _ = s.write_all(b"HTTP/1.1 500 Oops\r\nContent-Length: 100\r\n\r\npartial");
        std::thread::sleep(Duration::from_secs(3));
    };
    let flag = Arc::new(AtomicBool::new(false));
    let l = Limits::total(10_000, Duration::from_secs(5), Some(&flag));
    let mut r = request(Method::Get, &serve(hang), &[], None, &l).unwrap();
    assert_eq!(r.status, 500);
    cancel_after(&flag, 200);
    let t = Instant::now();
    assert_eq!(r.error_body(&l), Err(Error::Cancelled));
    assert!(t.elapsed() < Duration::from_millis(600), "{:?}", t.elapsed());
    let l = lim(400);
    let mut r = request(Method::Get, &serve(hang), &[], None, &l).unwrap();
    assert_eq!(r.error_body(&l), Err(Error::Timeout(400)));
    // Verbindung weg: Text bis dahin
    let url = serve_raw("HTTP/1.1 500 Oops\r\nContent-Length: 100\r\n\r\npartial");
    let mut r = request(Method::Get, &url, &[], None, &lim(5000)).unwrap();
    assert_eq!(r.error_body(&lim(5000)).unwrap(), "partial");
}

#[test]
fn idle_limit_for_transfers_counts_before_the_head() {
    let url = serve(|_s| std::thread::sleep(Duration::from_secs(3)));
    let l = Limits::transfer(Duration::from_secs(5), Duration::from_millis(300), None);
    let t = Instant::now();
    let e = request(Method::Get, &url, &[], None, &l).unwrap_err();
    assert!(e.to_string().contains("sent nothing"), "{e}");
    assert!(t.elapsed() < Duration::from_millis(800), "{:?}", t.elapsed());
}

// ---------------------------------------------------------------------------------------------------------------
// Verbindungsaufbau

/// Ein Port, der SYNs verschluckt: Listener mit Rückstau 0, dessen Warteschlange voll ist (Linux verwirft dann
/// weitere SYNs). So hängt ein Verbindungsaufbau wie bei einer schwarzen Route, ganz ohne Netz.
fn blackhole() -> (socket2::Socket, Vec<TcpStream>, SocketAddr) {
    let l = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    l.bind(&SocketAddr::from(([127, 0, 0, 1], 0)).into()).unwrap();
    l.listen(0).unwrap();
    let addr = l.local_addr().unwrap().as_socket().unwrap();
    let mut fill = Vec::new();
    for _ in 0..16 {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
            Ok(s) => fill.push(s),
            Err(_) => return (l, fill, addr),
        }
    }
    panic!("could not fill the accept queue");
}

#[test]
fn connect_to_a_blackhole_is_cancelled_quickly() {
    let (_l, _fill, addr) = blackhole();
    let flag = Arc::new(AtomicBool::new(false));
    cancel_after(&flag, 200);
    let l = Limits::total(20_000, Duration::from_secs(5), Some(&flag));
    let t = Instant::now();
    let e = request(Method::Get, &format!("http://{addr}/"), &[], None, &l).unwrap_err();
    assert_eq!(e, Error::Cancelled);
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    // Gesamtfrist und Verbindungsfrist greifen ebenso
    let t = Instant::now();
    assert_eq!(request(Method::Get, &format!("http://{addr}/"), &[], None, &lim(300)).unwrap_err(), Error::Timeout(300));
    assert!(t.elapsed() < Duration::from_millis(600), "{:?}", t.elapsed());
    let l = Limits::transfer(Duration::from_millis(300), Duration::from_secs(5), None);
    let e = request(Method::Get, &format!("http://{addr}/"), &[], None, &l).unwrap_err().to_string();
    assert!(e.contains("connection timed out"), "{e}");
}

#[test]
fn happy_eyeballs_moves_on_from_a_dead_address() {
    let (_l, _fill, dead) = blackhole();
    let good = TcpListener::bind("127.0.0.1:0").unwrap();
    let good_addr = good.local_addr().unwrap();
    let t = Instant::now();
    let s = connect_any(&[dead, good_addr], &lim(10_000), Instant::now()).ok().expect("connects");
    assert_eq!(s.peer_addr().unwrap(), good_addr);
    let d = t.elapsed();
    assert!(d >= ATTEMPT_DELAY - Duration::from_millis(20) && d < Duration::from_millis(1000), "{d:?}");
    // abgewiesene Adressen: sofort die nächste, ohne die Wartezeit
    let refused = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let t = Instant::now();
    let s = connect_any(&[refused, refused, good_addr], &lim(10_000), Instant::now()).ok().expect("connects");
    assert_eq!(s.peer_addr().unwrap(), good_addr);
    assert!(t.elapsed() < Duration::from_millis(200), "{:?}", t.elapsed());
    match connect_any(&[refused], &lim(10_000), Instant::now()) {
        Err(ConnectFail::Unreachable(why)) => assert!(why.contains("refused"), "{why}"),
        _ => panic!("must fail"),
    }
    // Hostname mit IPv6 und IPv4 (::1 hört hier nicht): kommt über IPv4 an
    let url = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").replace("127.0.0.1", "localhost");
    assert_eq!(get(&url, &lim(5000)).unwrap(), (200, b"ok".to_vec()));
}

#[test]
fn address_families_alternate() {
    let a = |s: &str| s.parse::<SocketAddr>().unwrap();
    let got = interleave(vec![a("[::1]:1"), a("[::2]:1"), a("[::3]:1"), a("10.0.0.1:1"), a("10.0.0.2:1"), a("[::1]:1")]);
    assert_eq!(got, vec![a("[::1]:1"), a("10.0.0.1:1"), a("[::2]:1"), a("10.0.0.2:1"), a("[::3]:1")]);
    let got = interleave(vec![a("10.0.0.1:1"), a("[::1]:1")]);
    assert_eq!(got, vec![a("10.0.0.1:1"), a("[::1]:1")]);
    assert!(interleave(Vec::new()).is_empty());
}

#[test]
fn slow_name_lookup_is_cancelled_and_dropped() {
    let flag = Arc::new(AtomicBool::new(false));
    cancel_after(&flag, 150);
    let l = Limits::total(20_000, Duration::from_secs(5), Some(&flag));
    let t = Instant::now();
    let r = resolve(
        || {
            std::thread::sleep(Duration::from_secs(3));
            Ok(Vec::new())
        },
        &l,
        Instant::now(),
    );
    assert_eq!(r.err(), Some(Error::Cancelled));
    assert!(t.elapsed() < Duration::from_millis(400), "{:?}", t.elapsed());
    // Fehler der Auflösung kommen als Text mit dem Host
    let e = request(Method::Get, "http://nonexistent.invalid/", &[], None, &lim(5000)).unwrap_err().to_string();
    assert!(e.starts_with("cannot reach server nonexistent.invalid"), "{e}");
}

// ---------------------------------------------------------------------------------------------------------------
// TLS offline: eigene CA, rustls-Server im Test, Client vertraut nur der Test-CA (nur in diesem Thread)

struct Pki {
    ca: rustls::pki_types::CertificateDer<'static>,
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}

fn pki() -> Pki {
    let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    p.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign, rcgen::KeyUsagePurpose::CrlSign];
    p.distinguished_name.push(rcgen::DnType::CommonName, "dictate test CA");
    let key = rcgen::KeyPair::generate().unwrap();
    let ca = p.self_signed(&key).unwrap().der().clone();
    Pki { ca, issuer: rcgen::Issuer::new(p, key) }
}

type Leaf = (Vec<rustls::pki_types::CertificateDer<'static>>, rustls::pki_types::PrivateKeyDer<'static>);

fn leaf(pki: &Pki, names: &[&str], expired: bool) -> Leaf {
    let mut p = rcgen::CertificateParams::new(names.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
    p.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    if expired {
        p.not_before = rcgen::date_time_ymd(2020, 1, 1);
        p.not_after = rcgen::date_time_ymd(2021, 1, 1);
    }
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = p.signed_by(&key, &pki.issuer).unwrap();
    (vec![cert.der().clone()], rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()))
}

/// TLS-Server für eine Verbindung; `script` bekommt die Serververbindung und den Socket
fn tls_serve(leaf: Leaf, script: impl FnOnce(&mut rustls::ServerConnection, &mut TcpStream) + Send + 'static) -> u16 {
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(leaf.0, leaf.1)
        .unwrap();
    let cfg = Arc::new(cfg);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut c = rustls::ServerConnection::new(cfg).unwrap();
            script(&mut c, &mut s);
        }
    });
    port
}

/// Handshake und Anfragekopf lesen; `false`, wenn der Client den Handshake abbricht
fn tls_accept(c: &mut rustls::ServerConnection, s: &mut TcpStream) -> bool {
    while c.is_handshaking() {
        if c.complete_io(s).is_err() {
            return false;
        }
    }
    read_request(&mut rustls::Stream::new(c, s));
    true
}

/// Klartext verschlüsseln und die TLS-Bytes liefern (statt sie zu senden)
fn seal(c: &mut rustls::ServerConnection, plain: &[u8]) -> Vec<u8> {
    c.writer().write_all(plain).unwrap();
    let mut wire = Vec::new();
    while c.wants_write() {
        c.write_tls(&mut wire).unwrap();
    }
    wire
}

/// Antwort senden; mit `close_notify` am Ende oder ohne (nur TCP schließen)
fn tls_answer(raw: &'static [u8], close_notify: bool) -> impl FnOnce(&mut rustls::ServerConnection, &mut TcpStream) + Send + 'static {
    move |c, s| {
        if !tls_accept(c, s) {
            return;
        }
        let mut wire = seal(c, raw);
        if close_notify {
            c.send_close_notify();
            while c.wants_write() {
                c.write_tls(&mut wire).unwrap();
            }
        }
        let _ = s.write_all(&wire);
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut sink = [0u8; 1024];
        while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
    }
}

/// Mit Test-CA in diesem Thread (danach wieder Produktion)
fn with_test_ca<T>(ca: &rustls::pki_types::CertificateDer<'static>, f: impl FnOnce() -> T) -> T {
    test_tls::set_root(Some(ca.clone()));
    let r = f();
    test_tls::set_root(None);
    r
}

#[test]
fn tls_works_offline_and_checks_name_and_expiry() {
    let pki = pki();
    let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
    let port = tls_serve(leaf(&pki, &["localhost", "127.0.0.1"], false), tls_answer(ok, true));
    assert_eq!(with_test_ca(&pki.ca, || get(&format!("https://localhost:{port}/"), &lim(5000))).unwrap(), (200, b"hello".to_vec()));
    // IP-Adresse gegen die IP-SAN
    let port = tls_serve(leaf(&pki, &["localhost", "127.0.0.1"], false), tls_answer(ok, true));
    assert_eq!(with_test_ca(&pki.ca, || get(&format!("https://127.0.0.1:{port}/"), &lim(5000))).unwrap(), (200, b"hello".to_vec()));
    // Produktions-Wurzeln (webpki-roots) kennen die Test-CA nicht
    let port = tls_serve(leaf(&pki, &["localhost"], false), tls_answer(ok, true));
    let e = err_text(get(&format!("https://localhost:{port}/"), &lim(5000)));
    assert!(e.starts_with("TLS error") && e.contains("UnknownIssuer"), "{e}");
    // falscher Name
    let port = tls_serve(leaf(&pki, &["other.test"], false), tls_answer(ok, true));
    let e = with_test_ca(&pki.ca, || err_text(get(&format!("https://localhost:{port}/"), &lim(5000))));
    assert!(e.starts_with("TLS error") && e.contains("not valid for name"), "{e}");
    // abgelaufen
    let port = tls_serve(leaf(&pki, &["localhost"], true), tls_answer(ok, true));
    let e = with_test_ca(&pki.ca, || err_text(get(&format!("https://localhost:{port}/"), &lim(5000))));
    assert!(e.starts_with("TLS error") && e.to_lowercase().contains("expired"), "{e}");
}

#[test]
fn tls_end_without_close_notify_is_a_cut_off_body() {
    let pki = pki();
    let close_framed = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\npartial";
    let port = tls_serve(leaf(&pki, &["localhost"], false), tls_answer(close_framed, true));
    assert_eq!(with_test_ca(&pki.ca, || get(&format!("https://localhost:{port}/"), &lim(5000))).unwrap(), (200, b"partial".to_vec()));
    let port = tls_serve(leaf(&pki, &["localhost"], false), tls_answer(close_framed, false));
    let e = with_test_ca(&pki.ca, || err_text(get(&format!("https://localhost:{port}/"), &lim(5000))));
    assert!(e.contains("cut off"), "{e}");
    // mit eigener Länge ist das fehlende close_notify egal (die Länge sagt, ob alles da ist)
    let port = tls_serve(leaf(&pki, &["localhost"], false), tls_answer(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok", false));
    assert_eq!(with_test_ca(&pki.ca, || get(&format!("https://localhost:{port}/"), &lim(5000))).unwrap(), (200, b"ok".to_vec()));
}

/// TLS-Server für beliebig viele Verbindungen: `answer` bekommt den Anfragekopf und liefert die Antwort; die
/// Anfrageköpfe werden gesammelt
fn tls_serve_each(leaf: Leaf, answer: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(leaf.0, leaf.1)
        .unwrap();
    let cfg = Arc::new(cfg);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut c = rustls::ServerConnection::new(cfg.clone()).unwrap();
            let mut st = rustls::Stream::new(&mut c, &mut s);
            let req = String::from_utf8_lossy(&read_request(&mut st)).into_owned();
            seen2.lock().unwrap().push(req.clone());
            let _ = st.write_all(answer(&req).as_bytes());
            c.send_close_notify();
            let _ = c.complete_io(&mut s);
        }
    });
    (port, seen)
}

#[test]
fn redirects_follow_only_https_and_drop_credentials_across_origins() {
    let pki = pki();
    let (tport, seen) = tls_serve_each(leaf(&pki, &["localhost"], false), |req| {
        if req.starts_with("GET /loop ") {
            "HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n".into()
        } else {
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndata".into()
        }
    });
    let h = [("authorization", "Bearer k".to_string()), ("accept", "x/y".to_string())];
    // http → https auf anderem Ursprung: Zugangsdaten fallen weg, andere Kopfzeilen bleiben
    let start = serve_raw(format!("HTTP/1.1 302 Found\r\nLocation: https://localhost:{tport}/file\r\nContent-Length: 0\r\n\r\n"));
    let mut r = with_test_ca(&pki.ca, || get_following_https(&start, &h, &lim(5000))).unwrap();
    assert_eq!((r.status, r.read_to_end(&lim(5000), 100).unwrap()), (200, b"data".to_vec()));
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("GET /file HTTP/1.1\r\n") && req.contains("\r\naccept: x/y\r\n"), "{req}");
    assert!(!req.to_lowercase().contains("authorization"), "{req}");
    // gleicher Ursprung: Zugangsdaten bleiben; höchstens MAX_REDIRECTS Weiterleitungen
    let e = with_test_ca(&pki.ca, || get_following_https(&format!("https://localhost:{tport}/loop"), &h, &lim(5000))).unwrap_err();
    assert_eq!(e.to_string(), "too many redirects");
    let loops: Vec<String> = seen.lock().unwrap().iter().filter(|r| r.starts_with("GET /loop ")).cloned().collect();
    assert_eq!(loops.len(), MAX_REDIRECTS + 1);
    assert!(loops.iter().all(|r| r.contains("\r\nauthorization: Bearer k\r\n")));
    // Ziele mit http:// oder Zugangsdaten in der URL werden nicht verfolgt
    let to_http = serve_raw("HTTP/1.1 301 Moved\r\nLocation: http://127.0.0.1:9/x\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(get_following_https(&to_http, &[], &lim(5000)).unwrap_err().to_string(), "refusing redirect to http://127.0.0.1");
    let to_user = serve_raw("HTTP/1.1 307 Temporary\r\nLocation: https://u:p@127.0.0.1:9/x\r\nContent-Length: 0\r\n\r\n");
    assert!(get_following_https(&to_user, &[], &lim(5000)).unwrap_err().to_string().contains("user name"));
    // andere 3xx kommen als Antwort zurück, ohne Weiterleitung
    let other = serve_raw("HTTP/1.1 300 Multiple Choices\r\nLocation: https://localhost:1/\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(get_following_https(&other, &[], &lim(5000)).unwrap().status, 300);
}

/// Sendet `wire` byteweise mit Pause, bis der Client schließt
fn trickle(s: &mut TcpStream, wire: &[u8]) {
    s.set_nodelay(true).unwrap();
    for b in wire {
        if s.write_all(&[*b]).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn trickled_tls_record_does_not_delay_cancel_or_idle() {
    let pki = pki();
    let script = |c: &mut rustls::ServerConnection, s: &mut TcpStream| {
        if !tls_accept(c, s) {
            return;
        }
        let head = seal(c, b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n");
        s.write_all(&head).unwrap();
        // ein großer Record, Byte für Byte: liefert lange keinen Klartext
        let body = seal(c, &[b'x'; 8000]);
        trickle(s, &body);
    };
    let flag = Arc::new(AtomicBool::new(false));
    let l = Limits::total(20_000, Duration::from_secs(10), Some(&flag));
    let port = tls_serve(leaf(&pki, &["localhost"], false), script);
    let mut r = with_test_ca(&pki.ca, || request(Method::Get, &format!("https://localhost:{port}/"), &[], None, &l)).unwrap();
    cancel_after(&flag, 300);
    let t = Instant::now();
    assert_eq!(r.next(&l).unwrap_err(), Error::Cancelled);
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    // Leerlauf: Teilstücke eines Records zählen nicht als Lebenszeichen
    let l = Limits::total(20_000, Duration::from_millis(400), None);
    let port = tls_serve(leaf(&pki, &["localhost"], false), script);
    let mut r = with_test_ca(&pki.ca, || request(Method::Get, &format!("https://localhost:{port}/"), &[], None, &l)).unwrap();
    let t = Instant::now();
    assert!(r.next(&l).unwrap_err().to_string().contains("sent nothing"));
    assert!(t.elapsed() < Duration::from_millis(700), "{:?}", t.elapsed());
}

#[test]
fn trickled_handshake_does_not_delay_cancel() {
    let pki = pki();
    let port = tls_serve(leaf(&pki, &["localhost"], false), |c, s| {
        // ClientHello lesen, dann die Antwort des Servers tröpfeln lassen
        while c.wants_read() && !c.wants_write() {
            if matches!(c.read_tls(s), Ok(0) | Err(_)) || c.process_new_packets().is_err() {
                return;
            }
        }
        let mut wire = Vec::new();
        while c.wants_write() {
            c.write_tls(&mut wire).unwrap();
        }
        trickle(s, &wire);
    });
    let flag = Arc::new(AtomicBool::new(false));
    cancel_after(&flag, 300);
    let l = Limits::total(20_000, Duration::from_secs(10), Some(&flag));
    let t = Instant::now();
    let e = with_test_ca(&pki.ca, || request(Method::Get, &format!("https://localhost:{port}/"), &[], None, &l)).unwrap_err();
    assert_eq!(e, Error::Cancelled);
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
}

// ---------------------------------------------------------------------------------------------------------------
// Zufallseingaben (Fuzz-artig, als normaler Test mit begrenzter Dauer). Kein proptest: steht nicht im Lockfile und
// brächte rand, regex-syntax u. a. mit; ein kleiner deterministischer Generator reicht für reine, fortsetzbare
// Zerleger und macht jeden Fehlschlag über den ausgegebenen Seed wiederholbar.

/// xorshift64*
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
    /// Zerlegt `data` an zufälligen Stellen
    fn split<'a>(&mut self, data: &'a [u8]) -> Vec<&'a [u8]> {
        let mut parts = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            let cap = if self.chance(50) { 4 } else { 300 };
            let n = 1 + self.below(rest.len().min(cap));
            parts.push(&rest[..n]);
            rest = &rest[n..];
        }
        parts
    }
}

/// Läuft höchstens `budget` lang bzw. `max` Runden, je mit eigenem Seed
fn fuzz(name: &str, max: u64, budget: Duration, mut round: impl FnMut(&mut Rng)) {
    let started = Instant::now();
    for i in 0..max {
        if started.elapsed() > budget {
            break;
        }
        let seed = 0x9e37_79b9_7f4a_7c15u64.wrapping_mul(i + 1);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| round(&mut Rng(seed))));
        if let Err(e) = r {
            panic!("{name}: round {i} (seed {seed:#x}) failed: {e:?}");
        }
    }
}

/// Gültige Chunked-Kodierung von `payload` mit zufälligen Größen, Erweiterungen, Zeilenenden und Trailern
fn encode_chunked(rng: &mut Rng, payload: &[u8]) -> Vec<u8> {
    let mut w = Vec::new();
    let eol = |rng: &mut Rng| if rng.chance(80) { &b"\r\n"[..] } else { &b"\n"[..] };
    let mut rest = payload;
    while !rest.is_empty() {
        let n = 1 + rng.below(rest.len().min(500));
        let hex = if rng.chance(50) { format!("{n:x}") } else { format!("{n:X}") };
        w.extend_from_slice(hex.as_bytes());
        if rng.chance(20) {
            w.extend_from_slice(if rng.chance(50) { b" ;a=b" } else { b";ext" });
        }
        w.extend_from_slice(eol(rng));
        w.extend_from_slice(&rest[..n]);
        w.extend_from_slice(eol(rng));
        rest = &rest[n..];
    }
    w.extend_from_slice(b"0");
    w.extend_from_slice(eol(rng));
    for _ in 0..rng.below(3) {
        w.extend_from_slice(b"X-T: v");
        w.extend_from_slice(eol(rng));
    }
    w.extend_from_slice(eol(rng));
    w
}

/// Ergebnis von `feed` über die gegebenen Stücke: Ausgabe und Fehler (falls einer kam)
fn feed_parts(parts: &[&[u8]]) -> (Vec<u8>, Option<Error>, bool) {
    let mut c = Chunked::new();
    let mut out = Vec::new();
    for p in parts {
        if let Err(e) = c.feed(p, &mut out) {
            return (out, Some(e), false);
        }
    }
    let done = c.is_done();
    (out, None, done)
}

#[test]
fn fuzz_chunked_decoder() {
    fuzz("chunked", 20_000, Duration::from_secs(2), |rng| {
        let len = rng.below(3000);
        let payload = rng.bytes(len);
        let wire = encode_chunked(rng, &payload);
        // gültig: jede Zerlegung ergibt die Nutzdaten
        let (out, err, done) = feed_parts(&rng.split(&wire));
        assert_eq!((err, done), (None, true));
        assert_eq!(out, payload);
        // verändert oder reiner Zufall: kein Panik, nie mehr Ausgabe als Eingabe, und das Ergebnis hängt nicht von der
        // Zerlegung ab
        let mut bad = if rng.chance(20) {
            let len = rng.below(2000);
            rng.bytes(len)
        } else {
            wire
        };
        for _ in 0..1 + rng.below(4) {
            if bad.is_empty() {
                break;
            }
            let at = rng.below(bad.len());
            match rng.below(3) {
                0 => bad[at] = rng.next() as u8,
                1 => {
                    bad.remove(at);
                }
                _ => bad.insert(at, *[b'\r', b'\n', b';', b'0', b'f', b' '].get(rng.below(6)).unwrap()),
            }
        }
        let whole = feed_parts(&[&bad]);
        let split = feed_parts(&rng.split(&bad));
        assert!(whole.0.len() <= bad.len());
        assert_eq!(whole.0, split.0);
        assert_eq!(whole.1.is_some(), split.1.is_some());
        assert_eq!(whole.2, split.2);
    });
}

/// Zufälliger gültiger Kopf mit gemischten Zeilenenden
fn random_head(rng: &mut Rng) -> Vec<u8> {
    let eol = |rng: &mut Rng| if rng.chance(70) { &b"\r\n"[..] } else { &b"\n"[..] };
    let mut h = format!("HTTP/1.{} {} {}", rng.below(2), 100 + rng.below(500), ["OK", "", "Some Reason"][rng.below(3)]).into_bytes();
    h.extend_from_slice(eol(rng));
    for i in 0..rng.below(10) {
        h.extend_from_slice(format!("X-{i}: {}", "v".repeat(rng.below(40))).as_bytes());
        h.extend_from_slice(eol(rng));
    }
    h.extend_from_slice(eol(rng));
    h
}

#[test]
fn fuzz_head_parser() {
    fuzz("head", 20_000, Duration::from_secs(2), |rng| {
        let head = random_head(rng);
        let mut wire = head.clone();
        wire.extend_from_slice(b"body\r\n\r\nmore\n\n");
        // fortgesetzte Suche über beliebige Stücke findet genau das Kopfende
        let mut buf = Vec::new();
        let mut scanned = 0;
        let mut found = None;
        for p in rng.split(&wire) {
            buf.extend_from_slice(p);
            if let Some(end) = head_end(&buf, scanned) {
                found = Some(end);
                break;
            }
            scanned = buf.len();
        }
        assert_eq!(found, Some(head.len()));
        let h = parse_head(&head).expect("valid head");
        let _ = framing(&h);
        // verändert oder Zufall: kein Panik
        let mut bad = if rng.chance(30) {
            let len = rng.below(400);
            rng.bytes(len)
        } else {
            head
        };
        for _ in 0..1 + rng.below(4) {
            if bad.is_empty() {
                break;
            }
            let at = rng.below(bad.len());
            bad[at] = rng.next() as u8;
        }
        if let Some(end) = head_end(&bad, 0) {
            if let Ok(h) = parse_head(&bad[..end]) {
                let _ = framing(&h);
            }
        }
    });
}
