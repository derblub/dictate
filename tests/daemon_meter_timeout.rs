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

//! Eigene Test-Binary, weil der Timeout von `meter` prozessweit verkürzt wird.
mod daemon_common;

use daemon_common::*;
use dictate::daemon::METER_TIMEOUT_MS;
use dictate::protocol::State;
use std::sync::atomic::Ordering;

#[test]
fn meter_stops_by_itself_and_renewing_extends_it() {
    METER_TIMEOUT_MS.store(400, Ordering::SeqCst);
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    let mut sub = h.subscribe();
    sub.until_state(State::Idle);
    h.req(r#"{"cmd":"meter","on":true}"#);
    h.wait_until("meter recording", || h.ctl.live_recordings.load(Ordering::SeqCst) == 1);
    // Erneuern verlängert
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        h.req(r#"{"cmd":"meter","on":true}"#);
        assert_eq!(h.ctl.live_recordings.load(Ordering::SeqCst), 1);
    }
    // Ohne Erneuern schließt der Daemon das Mikrofon selbst
    let t = std::time::Instant::now();
    h.wait_until("timeout", || h.ctl.live_recordings.load(Ordering::SeqCst) == 0);
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(h.state(), "idle");
    // und der Daemon bleibt bedienbar
    h.cmd("start");
    h.cmd("cancel");
}
