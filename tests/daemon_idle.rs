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

//! Leerlauf ohne CPU-Last: kein Polling in Kern, Engine-, Accept- und Signal-Threads.
mod daemon_common;

use daemon_common::*;
use std::time::Duration;

fn cpu_ticks() -> u64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    // Felder nach dem letzten ')' : state ppid ... utime (Index 11) stime (12) relativ dazu
    let rest = &s[s.rfind(')').unwrap() + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}

#[test]
fn idle_daemon_uses_no_cpu() {
    let h = Harness::start(quick_cfg());
    h.wait_state("idle");
    std::thread::sleep(Duration::from_millis(300));
    let before = cpu_ticks();
    std::thread::sleep(Duration::from_secs(5));
    let used = cpu_ticks() - before;
    assert!(used <= 1, "idle used {used} ticks in 5 s");
}
