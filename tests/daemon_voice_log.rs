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

//! Die gesprochene Eingabe der Befehlspalette kommt nie ins Log, auch nicht mit `DICTATE_DEBUG` (W5, Durchsicht N1;
//! PLAN-wayland.md 6g Sicherheitsregel 4). Eigenes Test-Binary, weil der Test `DICTATE_DEBUG` setzt und das Log des
//! Prozesses mitschneidet. Gegenprobe: ein Diktat ins Fenster steht mit `DICTATE_DEBUG` im Log (sonst prüfte der Test
//! einen leeren Mitschnitt).

mod daemon_common;

use daemon_common::*;
use dictate::protocol::{Event, State};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct LogWriter;

impl Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let sub = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_ansi(false).with_writer(|| LogWriter).finish();
        tracing::subscriber::set_global_default(sub).expect("kein anderer Subscriber in diesem Test-Binary");
    });
}

fn log() -> String {
    String::from_utf8_lossy(&LOG.lock().unwrap()).to_string()
}

/// Eine Aufnahme mit `line` starten, einen Zwischenstand mit `partial` abwarten, beenden und das final abwarten.
fn speak(h: &Harness, s: &mut Sub, line: &str, partial: &str, fin: &str) {
    *h.ctl.partial_text.lock().unwrap() = Some(partial.to_string());
    *h.ctl.finish_text.lock().unwrap() = Some(fin.to_string());
    assert_eq!(h.req(line)["ok"], true, "{line}");
    s.until(|e| matches!(e, Event::Partial { stable, .. } if stable == partial));
    h.cmd("stop");
    let ev = s.until(|e| matches!(e, Event::Final { .. }));
    assert!(matches!(ev.last(), Some(Event::Final { cancelled: false, .. })), "{ev:?}");
    s.until_state(State::Idle);
}

#[test]
fn palette_voice_never_reaches_the_log_even_with_debug() {
    // nur dieser Test läuft in diesem Binary; vor dem Start des Daemons gesetzt
    std::env::set_var("DICTATE_DEBUG", "1");
    capture_logs();
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut s = h.subscribe();
    s.until_state(State::Idle);

    speak(&h, &mut s, r#"{"cmd":"start","app":"kitty"}"#, "windowpartialmarker", "windowfinalmarker eins");
    assert_eq!(*h.ctl.log_text.lock().unwrap(), Some(true), "a window dictation may log its text with DICTATE_DEBUG");
    speak(&h, &mut s, r#"{"cmd":"start","app":"kitty","target":"palette"}"#, "palpartialmarker", "palfinalmarker eins");
    assert_eq!(*h.ctl.log_text.lock().unwrap(), Some(false), "the streamer of a palette recording must not log text");
    speak(&h, &mut s, r#"{"cmd":"toggle","target":"palette"}"#, "palpartialmarker", "palfinalmarker zwei");
    assert_eq!(*h.ctl.log_text.lock().unwrap(), Some(false));
    speak(&h, &mut s, r#"{"cmd":"start","app":"kitty"}"#, "windowpartialmarker", "windowfinalmarker zwei");
    assert_eq!(*h.ctl.log_text.lock().unwrap(), Some(true), "the next window dictation logs again");

    h.wait_until("Diktat im Log", || log().contains("windowfinalmarker zwei"));
    let log = log();
    assert!(log.contains("windowpartialmarker"), "der Mitschnitt hat keine Zwischenstände: {log}");
    for marker in ["palpartialmarker", "palfinalmarker"] {
        assert!(!log.contains(marker), "{marker} steht im Log:\n{log}");
    }
}
