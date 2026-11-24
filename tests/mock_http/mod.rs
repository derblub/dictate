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

//! Minimaler HTTP-Mock für die KI-Tests: ein Thread mit `TcpListener`, der Anfragen aufzeichnet und per Closure beantwortet.
//! Gestreamte Antworten ([`Resp::stream`]) gehen als `Transfer-Encoding: chunked`, jedes Stück einzeln mit Pause.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    /// Namen klein geschrieben
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

pub struct Resp {
    pub status: u16,
    pub body: String,
    /// Wartezeit vor der Antwort
    pub delay_ms: u64,
    pub content_type: String,
    /// Gestreamt: (Pause davor in ms, Bytes) je HTTP-Chunk; `body` bleibt dann leer
    pub chunks: Option<Vec<(u64, Vec<u8>)>>,
    /// Gestreamt: nach den Stücken nicht enden, sondern hängen, bis der Client schließt (höchstens 10 s)
    pub hang: bool,
}

impl Resp {
    pub fn ok(body: serde_json::Value) -> Resp {
        Resp::status(200, body)
    }
    pub fn status(status: u16, body: serde_json::Value) -> Resp {
        Resp { status, body: body.to_string(), delay_ms: 0, content_type: "application/json".into(), chunks: None, hang: false }
    }
    /// Gestreamte Antwort mit Status 200: jedes Stück ein eigener Chunk, geschrieben nach seiner Pause
    pub fn stream(content_type: &str, chunks: Vec<(u64, Vec<u8>)>) -> Resp {
        Resp { status: 200, body: String::new(), delay_ms: 0, content_type: content_type.into(), chunks: Some(chunks), hang: false }
    }
    /// Nach den Stücken hängen statt enden (siehe [`Mock::closed`])
    pub fn hang(mut self) -> Resp {
        self.hang = true;
        self
    }
    pub fn delayed(mut self, ms: u64) -> Resp {
        self.delay_ms = ms;
        self
    }
}

pub struct Mock {
    /// `http://127.0.0.1:PORT`
    pub url: String,
    pub reqs: Arc<Mutex<Vec<Req>>>,
    /// Gestreamte Antworten, bei denen der Client die Verbindung vor dem Ende schloss (Schreiben scheiterte)
    pub aborted: Arc<AtomicUsize>,
    /// Gestreamt: Zeitpunkte, zu denen der Mock beim Warten (Pause vor dem Kopf oder hängend) das Schließen durch den
    /// Client bemerkte
    pub closed: Arc<Mutex<Vec<Instant>>>,
}

impl Mock {
    pub fn start(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Mock {
        Mock::start_on("127.0.0.1", handler)
    }

    /// Wie [`Mock::start`] auf einer anderen Adresse (z. B. einer Nicht-Loopback-Adresse, die als Cloud gilt).
    pub fn start_on(ip: &str, handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Mock {
        let l = TcpListener::bind((ip, 0)).unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let reqs: Arc<Mutex<Vec<Req>>> = Arc::default();
        let aborted: Arc<AtomicUsize> = Arc::default();
        let closed: Arc<Mutex<Vec<Instant>>> = Arc::default();
        let (r2, a2, c2, h) = (reqs.clone(), aborted.clone(), closed.clone(), Arc::new(handler));
        std::thread::spawn(move || {
            for conn in l.incoming().flatten() {
                let (r3, a3, c3, h2) = (r2.clone(), a2.clone(), c2.clone(), h.clone());
                std::thread::spawn(move || serve(conn, r3, a3, c3, h2));
            }
        });
        Mock { url, reqs, aborted, closed }
    }

    pub fn requests(&self) -> Vec<Req> {
        self.reqs.lock().unwrap().clone()
    }
}

/// Wartet `ms`, bemerkt dabei aber, wenn der Client die Verbindung schließt (`true`)
fn wait_or_closed(c: &mut std::net::TcpStream, ms: u64) -> bool {
    let _ = c.set_read_timeout(Some(Duration::from_millis(10)));
    let t = Instant::now();
    let mut b = [0u8; 256];
    while t.elapsed() < Duration::from_millis(ms) {
        match c.read(&mut b) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return true,
        }
    }
    false
}

fn serve(
    mut c: std::net::TcpStream,
    reqs: Arc<Mutex<Vec<Req>>>,
    aborted: Arc<AtomicUsize>,
    closed: Arc<Mutex<Vec<Instant>>>,
    h: Arc<dyn Fn(&Req) -> Resp + Send + Sync>,
) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        match c.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        match c.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    let req = Req { method, path, headers, body: String::from_utf8_lossy(&body).to_string() };
    reqs.lock().unwrap().push(req.clone());
    let resp = h(&req);
    if let Some(chunks) = resp.chunks {
        if wait_or_closed(&mut c, resp.delay_ms) {
            closed.lock().unwrap().push(Instant::now());
            return;
        }
        let head = format!("HTTP/1.1 {} X\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n", resp.status, resp.content_type);
        let _ = c.set_nodelay(true);
        let mut ok = c.write_all(head.as_bytes()).and_then(|_| c.flush()).is_ok();
        for (delay, data) in chunks {
            if !ok {
                break;
            }
            std::thread::sleep(Duration::from_millis(delay));
            let mut frame = format!("{:x}\r\n", data.len()).into_bytes();
            frame.extend_from_slice(&data);
            frame.extend_from_slice(b"\r\n");
            ok = c.write_all(&frame).and_then(|_| c.flush()).is_ok();
        }
        if ok && resp.hang {
            if wait_or_closed(&mut c, 10_000) {
                closed.lock().unwrap().push(Instant::now());
            }
            return;
        }
        if !ok || c.write_all(b"0\r\n\r\n").is_err() {
            aborted.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    std::thread::sleep(Duration::from_millis(resp.delay_ms));
    let out = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        resp.status,
        resp.content_type,
        resp.body.len(),
        resp.body
    );
    let _ = c.write_all(out.as_bytes());
}

/// SSE-Stücke im Format der Claude API: `message_start`, ein Textblock mit `texts` als `text_delta`, Endgrund
pub fn anthropic_sse(texts: &[&str], stop_reason: &str) -> Vec<u8> {
    let ev = |name: &str, v: serde_json::Value| format!("event: {name}\ndata: {v}\n\n");
    let mut s =
        ev("message_start", serde_json::json!({"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "content": []}}));
    s += &ev("content_block_start", serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}));
    s += &ev("ping", serde_json::json!({"type": "ping"}));
    for t in texts {
        s += &ev("content_block_delta", serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": t}}));
    }
    s += &ev("content_block_stop", serde_json::json!({"type": "content_block_stop", "index": 0}));
    s += &ev("message_delta", serde_json::json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": {"output_tokens": 9}}));
    s += &ev("message_stop", serde_json::json!({"type": "message_stop"}));
    s.into_bytes()
}

/// SSE-Stücke eines OpenAI-kompatiblen Servers mit `finish_reason` und `[DONE]`
pub fn openai_sse(texts: &[&str], finish: &str) -> Vec<u8> {
    let mut s = String::new();
    for t in texts {
        s += &format!("data: {}\n\n", serde_json::json!({"choices": [{"index": 0, "delta": {"content": t}, "finish_reason": null}]}));
    }
    s += &format!("data: {}\n\ndata: [DONE]\n\n", serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}));
    s.into_bytes()
}

/// NDJSON-Zeilen von Ollama mit `done`/`done_reason`
pub fn ollama_ndjson(texts: &[&str], reason: &str) -> Vec<u8> {
    let mut s = String::new();
    for t in texts {
        s += &format!("{}\n", serde_json::json!({"model": "m", "message": {"role": "assistant", "content": t}, "done": false}));
    }
    s += &format!("{}\n", serde_json::json!({"model": "m", "message": {"role": "assistant", "content": ""}, "done": true, "done_reason": reason}));
    s.into_bytes()
}

/// Zerlegt `bytes` in Stücke der Größe `n` mit je `delay_ms` Pause (schneidet mitten in UTF-8 und Events)
pub fn split(bytes: &[u8], n: usize, delay_ms: u64) -> Vec<(u64, Vec<u8>)> {
    bytes.chunks(n).map(|c| (delay_ms, c.to_vec())).collect()
}
