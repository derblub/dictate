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

//! Spike S1 (W0, PLAN-wayland.md 3.2 A): Loslassen von Mod+E über den Tastenzustand der Tastaturen.
//!
//! Liest **nur** mit `ioctl` (`EVIOCGNAME`, `EVIOCGBIT`, `EVIOCGKEY`), **nie** mit `read()`: der Ereignisstrom (jeder
//! Tastendruck) bleibt ungelesen. Nachweis im Live-Termin: `strace -f -e trace=read,ioctl` zeigt keine `read` auf
//! `/dev/input/event*`.
//!
//! `keystate list`
//!     Geräte unter /dev/input/event*, die KEY_E und KEY_LEFTMETA melden können (Name, Pfad).
//! `keystate watch [--interval-ms 20] [--seconds 60]`
//!     Fragt alle 20 ms den Zustand von E, linker und rechter Super-Taste auf allen solchen Geräten ab und meldet jeden
//!     Wechsel mit Zeit (ms seit Start) und Gerät; am Ende: Zahl der Abfragen, Dauer je ioctl (Median/Maximum in µs),
//!     tatsächlicher Abstand der Abfragen (Median/p99/Maximum in ms).
//!
//! Läuft nur auf dem echten Rechner und nur im Live-Termin mit Daniel (PLAN-wayland.md Abschnitt 5).

use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

const KEY_E: usize = 18;
const KEY_LEFTMETA: usize = 125;
const KEY_RIGHTMETA: usize = 126;
const KEY_MAX: usize = 0x2ff;
const KEY_BYTES: usize = KEY_MAX / 8 + 1;
const EV_KEY: u64 = 0x01;
const O_NONBLOCK: i32 = 0o4000;

unsafe extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

/// `_IOC(_IOC_READ, 'E', nr, size)`
const fn ioc_read(nr: u64, size: usize) -> u64 {
    (2u64 << 30) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr
}

fn ioctl_buf(f: &File, req: u64, buf: &mut [u8]) -> std::io::Result<i32> {
    // SAFETY: buf ist so groß wie die in `req` kodierte Länge; der Kernel schreibt höchstens so viele Bytes.
    let r = unsafe { ioctl(f.as_raw_fd(), req, buf.as_mut_ptr()) };
    if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(r) }
}

fn bit(buf: &[u8], n: usize) -> bool {
    buf.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

struct Dev {
    path: String,
    name: String,
    file: File,
}

fn keyboards() -> Vec<Dev> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/dev/input") else { return out };
    let mut paths: Vec<_> = dir
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
        .collect();
    paths.sort();
    for p in paths {
        // Nur lesend geöffnet und nicht blockierend; gelesen wird nie.
        let Ok(file) = std::fs::OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(&p) else { continue };
        let mut bits = [0u8; KEY_BYTES];
        if ioctl_buf(&file, ioc_read(0x20 + EV_KEY, KEY_BYTES), &mut bits).is_err() {
            continue;
        }
        if !(bit(&bits, KEY_E) && bit(&bits, KEY_LEFTMETA)) {
            continue;
        }
        let mut name = [0u8; 256];
        let _ = ioctl_buf(&file, ioc_read(0x06, name.len()), &mut name);
        let name = String::from_utf8_lossy(name.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
        out.push(Dev { path: p.display().to_string(), name, file });
    }
    out
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let devs = keyboards();
    match args.first().map(String::as_str) {
        Some("list") => {
            for d in &devs {
                println!("{} {}", d.path, d.name);
            }
            if devs.is_empty() {
                println!("(no keyboard with KEY_E and KEY_LEFTMETA readable)");
            }
        }
        Some("watch") => {
            let interval = Duration::from_millis(arg(&args, "--interval-ms", 20));
            let until = Duration::from_secs(arg(&args, "--seconds", 60));
            let start = Instant::now();
            let mut last: Vec<[bool; 3]> = vec![[false; 3]; devs.len()];
            let (mut cost, mut gaps) = (Vec::new(), Vec::new());
            let mut prev = Instant::now();
            println!("watching {} device(s), interval {} ms", devs.len(), interval.as_millis());
            while start.elapsed() < until {
                let now = Instant::now();
                gaps.push((now - prev).as_secs_f64() * 1000.0);
                prev = now;
                for (i, d) in devs.iter().enumerate() {
                    let mut keys = [0u8; KEY_BYTES];
                    let t = Instant::now();
                    if let Err(e) = ioctl_buf(&d.file, ioc_read(0x18, KEY_BYTES), &mut keys) {
                        println!("{:>7} {} error {e}", start.elapsed().as_millis(), d.path);
                        continue;
                    }
                    cost.push(t.elapsed().as_secs_f64() * 1e6);
                    let s = [bit(&keys, KEY_E), bit(&keys, KEY_LEFTMETA), bit(&keys, KEY_RIGHTMETA)];
                    if s != last[i] {
                        println!(
                            "{:>7} {} E={} LMETA={} RMETA={}",
                            start.elapsed().as_millis(),
                            d.path,
                            u8::from(s[0]),
                            u8::from(s[1]),
                            u8::from(s[2])
                        );
                        last[i] = s;
                    }
                }
                std::thread::sleep(interval.saturating_sub(now.elapsed()));
            }
            gaps.remove(0);
            println!(
                "polls={} ioctl_us median={:.1} max={:.1} gap_ms median={:.2} p99={:.2} max={:.2}",
                gaps.len() + 1,
                pct(&mut cost.clone(), 0.5),
                pct(&mut cost, 1.0),
                pct(&mut gaps.clone(), 0.5),
                pct(&mut gaps.clone(), 0.99),
                pct(&mut gaps, 1.0)
            );
        }
        _ => {
            eprintln!("usage: keystate list | watch [--interval-ms 20] [--seconds 60]");
            std::process::exit(2);
        }
    }
}
