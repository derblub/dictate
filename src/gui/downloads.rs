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

//! Modell-Downloads im Hintergrund-Thread: Fortschritt, SHA-256-Prüfung, erst in `.part`, dann umbenennen.

use super::net::Msg;
use dictate::http;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum DlState {
    Running,
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub id: String,
    pub done: u64,
    pub total: u64,
    pub state: DlState,
}

pub struct Job {
    pub cancel: Arc<AtomicBool>,
}

/// Zielverzeichnis für Modelle.
pub fn models_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("dictate/models")
}

/// Dateiname aus der URL (letztes Pfadsegment ohne Query und Fragment). Erlaubt ist nur `[A-Za-z0-9._-]+\.bin`, der
/// Name beginnt nicht mit einem Punkt; alles andere (Pfadtricks, Prozentkodierung, andere Endungen) ergibt `None`.
pub fn file_name_from_url(url: &str) -> Option<String> {
    let last = url.split(['?', '#']).next()?.rsplit('/').next()?;
    let ok = last.len() > ".bin".len()
        && last.ends_with(".bin")
        && !last.starts_with('.')
        && last.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    ok.then(|| last.to_string())
}

/// Genau 64 Hexzeichen.
fn sha256_valid(s: &str) -> bool {
    s.trim().len() == 64 && s.trim().bytes().all(|b| b.is_ascii_hexdigit())
}

/// Größtes zulässiges Volumen: das Größere aus erwarteter Größe und `Content-Length`, plus 1 %. Ist beides unbekannt,
/// gilt eine feste Obergrenze.
fn size_limit(expected: u64, content_length: u64) -> u64 {
    const UNKNOWN: u64 = 4 * 1024 * 1024 * 1024;
    let n = expected.max(content_length);
    if n == 0 {
        return UNKNOWN;
    }
    n.saturating_add(n / 100)
}

/// Verbindungsaufbau (Namensauflösung, TCP, TLS) höchstens so lange
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// So lange darf kein Byte kommen (auch vor dem Antwortkopf); eine Gesamtfrist gibt es nicht (große Modelle)
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// GET über den eigenen Client mit sicher verfolgten Weiterleitungen ([`http::get_following_https`]: nur zu
/// `https://`, höchstens 5, ohne Zugangsdaten). Abbruch über `cancel` wirkt nach spätestens ~50 ms, auch während
/// Namensauflösung, Verbindungsaufbau und Warten auf Daten. Nur Status 2xx gilt als Erfolg.
fn get_following_https(url: &str, lim: &http::Limits) -> Result<http::Response, DlError> {
    let resp = http::get_following_https(url, &[], lim)?;
    if !(200..300).contains(&resp.status) {
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        return Err(DlError::Failed(format!("HTTP {} from {host}", resp.status)));
    }
    Ok(resp)
}

pub fn start(id: String, url: String, sha256: String, expected: u64, dest_dir: PathBuf, tx: Sender<Msg>, ctx: egui::Context) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::Builder::new()
        .name(format!("download-{id}"))
        .spawn(move || {
            let send = |done: u64, total: u64, state: DlState| {
                if tx.send(Msg::Download(Progress { id: id.clone(), done, total, state })).is_ok() {
                    ctx.request_repaint();
                }
            };
            let mut last_report = std::time::Instant::now();
            let mut last = (0u64, expected);
            let res = download(&url, &sha256, expected, &dest_dir, &c2, |done, total| {
                last = (done, total);
                if last_report.elapsed().as_millis() >= 100 {
                    last_report = std::time::Instant::now();
                    send(done, total, DlState::Running);
                }
            });
            match res {
                Ok(()) => send(last.1.max(last.0), last.1.max(last.0), DlState::Done),
                Err(DlError::Cancelled) => send(last.0, last.1, DlState::Cancelled),
                Err(DlError::Failed(m)) => send(last.0, last.1, DlState::Failed(m)),
            }
        })
        .expect("thread");
    Job { cancel }
}

enum DlError {
    Cancelled,
    Failed(String),
}

impl From<http::Error> for DlError {
    fn from(e: http::Error) -> DlError {
        match e {
            http::Error::Cancelled => DlError::Cancelled,
            e => DlError::Failed(e.to_string()),
        }
    }
}

fn fail<E: std::fmt::Display>(e: E) -> DlError {
    DlError::Failed(e.to_string())
}

fn download(url: &str, sha256: &str, expected: u64, dir: &Path, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> Result<(), DlError> {
    // Ohne gültige Prüfsumme gibt es keinen Download
    if !sha256_valid(sha256) {
        return Err(DlError::Failed("no valid SHA-256 for this file, refusing to download".into()));
    }
    let sha256 = sha256.trim();
    let name = file_name_from_url(url).ok_or_else(|| DlError::Failed("bad file name in URL (only letters, digits, . _ - and the extension .bin)".into()))?;
    std::fs::create_dir_all(dir).map_err(fail)?;
    let final_path = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    let lim = http::Limits::transfer(CONNECT_TIMEOUT, IDLE_TIMEOUT, Some(cancel));
    let mut resp = get_following_https(url, &lim)?;
    let content_length = resp.content_length().unwrap_or(0);
    let limit = size_limit(expected, content_length);
    let total = if content_length > 0 { content_length } else { expected };
    let result = (|| {
        let mut f = std::fs::File::create(&part).map_err(fail)?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        while let Some(buf) = resp.next(&lim)? {
            hasher.update(&buf);
            f.write_all(&buf).map_err(fail)?;
            done += buf.len() as u64;
            if done > limit {
                return Err(DlError::Failed(format!("download is larger than expected (more than {limit} bytes), aborted")));
            }
            progress(done, total);
        }
        f.sync_all().map_err(fail)?;
        let got = hex(&hasher.finalize());
        if !got.eq_ignore_ascii_case(sha256) {
            return Err(DlError::Failed(format!("SHA-256 mismatch (expected {}…, got {}…)", &sha256[..8], &got[..8])));
        }
        std::fs::rename(&part, &final_path).map_err(fail)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::Ordering;

    #[test]
    fn name_from_url() {
        assert_eq!(file_name_from_url("https://h/x/ggml-a.bin?download=true").as_deref(), Some("ggml-a.bin"));
        assert_eq!(file_name_from_url("https://h/").as_deref(), None);
        assert_eq!(file_name_from_url("https://h/x/ggml-silero-v5.1.2.bin#frag").as_deref(), Some("ggml-silero-v5.1.2.bin"));
        // nur [A-Za-z0-9._-]+\.bin, nicht mit Punkt am Anfang
        for bad in [
            "https://h/.hidden.bin",
            "https://h/..bin",
            "https://h/a.txt",
            "https://h/a.bin.sh",
            "https://h/%2e%2e%2fx.bin",
            "https://h/a b.bin",
            "https://h/a%20b.bin",
            "https://h/ä.bin",
            "https://h/.bin",
            "https://h/a:b.bin",
            "https://h/a\\b.bin",
        ] {
            assert_eq!(file_name_from_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn sha_and_limit_rules() {
        assert!(sha256_valid(&"a".repeat(64)) && sha256_valid(&"AB12".repeat(16)));
        for bad in ["", "abc", &"a".repeat(63), &"a".repeat(65), &"g".repeat(64)] {
            assert!(!sha256_valid(bad), "{bad}");
        }
        assert_eq!(size_limit(1000, 0), 1010);
        assert_eq!(size_limit(1000, 2000), 2020);
        assert_eq!(size_limit(2000, 1000), 2020);
        assert_eq!(size_limit(0, 0), 4 * 1024 * 1024 * 1024);
    }

    /// Lokaler HTTP-Server mit einer Testdatei: Download, SHA-256-Prüfung, Umbenennen.
    fn serve(body: Vec<u8>) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf);
                let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(&body);
            }
        });
        port
    }

    fn sha(b: &[u8]) -> String {
        hex(&Sha256::digest(b))
    }

    #[test]
    fn download_verifies_and_renames() {
        let body = vec![7u8; 600_000];
        let port = serve(body.clone());
        let dir = std::env::temp_dir().join(format!("dictate-dl-ok-{}", std::process::id()));
        let url = format!("http://127.0.0.1:{port}/m.bin");
        let cancel = AtomicBool::new(false);
        let mut last = 0;
        assert!(download(&url, &sha(&body), 0, &dir, &cancel, |d, _| last = d).is_ok());
        assert_eq!(last, 600_000);
        assert_eq!(std::fs::read(dir.join("m.bin")).unwrap().len(), 600_000);
        assert!(!dir.join("m.bin.part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_with_wrong_sum_leaves_nothing() {
        let port = serve(vec![1u8; 1000]);
        let dir = std::env::temp_dir().join(format!("dictate-dl-bad-{}", std::process::id()));
        let url = format!("http://127.0.0.1:{port}/m.bin");
        let cancel = AtomicBool::new(false);
        let r = download(&url, &"0".repeat(64), 0, &dir, &cancel, |_, _| {});
        assert!(matches!(r, Err(DlError::Failed(m)) if m.contains("SHA-256")));
        assert!(!dir.join("m.bin").exists());
        assert!(!dir.join("m.bin.part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Server mit festen Kopfzeilen, zählt die Verbindungen.
    fn serve_raw(head: &'static str, body: Vec<u8>) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h2 = hits.clone();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                h2.fetch_add(1, Ordering::Relaxed);
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf);
                let _ = c.write_all(head.as_bytes());
                let _ = c.write_all(&body);
            }
        });
        (port, hits)
    }

    fn tmpdir(n: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dictate-dl-{n}-{}", std::process::id()))
    }

    #[test]
    fn no_download_without_a_valid_checksum() {
        let (port, hits) = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n", b"data".to_vec());
        let dir = tmpdir("nosum");
        let url = format!("http://127.0.0.1:{port}/m.bin");
        let cancel = AtomicBool::new(false);
        for bad in ["", "abc", &"z".repeat(64), &"a".repeat(63)] {
            let r = download(&url, bad, 4, &dir, &cancel, |_, _| {});
            assert!(matches!(r, Err(DlError::Failed(ref m)) if m.contains("SHA-256")), "{bad}");
        }
        assert_eq!(hits.load(Ordering::Relaxed), 0, "no request without a checksum");
        assert!(!dir.join("m.bin").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_download_is_aborted() {
        // kein Content-Length, 5000 Bytes gegen erwartete 1000 (Grenze 1010)
        let (port, _) = serve_raw("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", vec![9u8; 5000]);
        let dir = tmpdir("big");
        let url = format!("http://127.0.0.1:{port}/m.bin");
        let cancel = AtomicBool::new(false);
        let mut seen = 0;
        let r = download(&url, &sha(&[9u8; 5000]), 1000, &dir, &cancel, |d, _| seen = d);
        assert!(
            matches!(r, Err(DlError::Failed(ref m)) if m.contains("larger than expected")),
            "{:?}",
            r.err().map(|e| match e {
                DlError::Failed(m) => m,
                DlError::Cancelled => "cancelled".into(),
            })
        );
        assert!(seen <= 1010, "{seen}");
        assert!(!dir.join("m.bin").exists() && !dir.join("m.bin.part").exists());
        // genau erwartete Größe plus 1 % geht noch
        let body = vec![5u8; 1010];
        let (port, _) = serve_raw("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", body.clone());
        let url = format!("http://127.0.0.1:{port}/m.bin");
        assert!(download(&url, &sha(&body), 1000, &dir, &cancel, |_, _| {}).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn redirects_to_http_are_refused() {
        let (target, thits) = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n", b"data".to_vec());
        let head: &'static str = Box::leak(
            format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target}/m.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_boxed_str(),
        );
        let (port, _) = serve_raw(head, Vec::new());
        let dir = tmpdir("redir");
        let cancel = AtomicBool::new(false);
        let r = download(&format!("http://127.0.0.1:{port}/m.bin"), &sha(b"data"), 4, &dir, &cancel, |_, _| {});
        assert!(matches!(r, Err(DlError::Failed(ref m)) if m.contains("refusing redirect")));
        assert_eq!(thits.load(Ordering::Relaxed), 0, "the http target must not be contacted");
        // relative Weiterleitung bleibt http und wird ebenfalls abgelehnt
        let (port, _) = serve_raw("HTTP/1.1 301 Moved\r\nLocation: /other.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", Vec::new());
        assert!(download(&format!("http://127.0.0.1:{port}/m.bin"), &sha(b"data"), 4, &dir, &cancel, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancel_ends_a_stalled_download_at_once() {
        // Kopf und ein Teil der Daten, dann Stille: vorher hielt erst die 30-s-Lesefrist von ureq den Download an
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut c, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf);
                let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npart");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
        let dir = tmpdir("cancel");
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let mut seen = 0;
        let t = std::time::Instant::now();
        let r = download(&format!("http://127.0.0.1:{port}/m.bin"), &sha(b"x"), 1000, &dir, &cancel, |d, _| {
            seen = d;
            c2.store(true, Ordering::Relaxed);
        });
        assert!(matches!(r, Err(DlError::Cancelled)));
        assert_eq!(seen, 4);
        assert!(t.elapsed() < std::time::Duration::from_millis(500), "{:?}", t.elapsed());
        assert!(!dir.join("m.bin").exists() && !dir.join("m.bin.part").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn error_status_is_a_failure() {
        let (port, _) = serve_raw("HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found", Vec::new());
        let dir = tmpdir("404");
        let cancel = AtomicBool::new(false);
        let r = download(&format!("http://127.0.0.1:{port}/m.bin"), &sha(b"not found"), 9, &dir, &cancel, |_, _| {});
        assert!(matches!(r, Err(DlError::Failed(ref m)) if m == "HTTP 404 from 127.0.0.1"));
        assert!(!dir.join("m.bin").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bad_file_names_never_touch_the_disk() {
        let dir = tmpdir("name");
        let cancel = AtomicBool::new(false);
        let r = download("http://127.0.0.1:9/..%2f..%2fevil.bin", &sha(b"x"), 1, &dir, &cancel, |_, _| {});
        assert!(matches!(r, Err(DlError::Failed(ref m)) if m.contains("file name")));
        assert!(!dir.exists(), "the directory is not even created");
    }

    #[test]
    fn hex_encodes() {
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }
}
