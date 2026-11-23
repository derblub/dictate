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

//! Echte Pulse-Aufnahme (ignoriert): `DICTATE_TEST_SOURCE=dictate_test.monitor cargo test --test daemon_audio_real -- --ignored --nocapture`
use dictate::audio::{AudioSource, Capture, PulseSource};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn open_latency_to_first_block() {
    let src = std::env::var("DICTATE_TEST_SOURCE").expect("DICTATE_TEST_SOURCE setzen (z. B. dictate_test.monitor)");
    let mut lat = Vec::new();
    for _ in 0..10 {
        let (tx, rx) = mpsc::channel();
        let t = Instant::now();
        let rec = PulseSource.open(
            &src,
            Box::new(move |c| {
                let _ = tx.send((Instant::now(), c));
            }),
        );
        let mut first_block = None;
        let mut blocks = 0;
        let end = Instant::now() + Duration::from_millis(500);
        while Instant::now() < end {
            if let Ok((at, c)) = rx.recv_timeout(Duration::from_millis(100)) {
                match c {
                    Capture::Block(b) => {
                        assert_eq!(b.len(), 320);
                        blocks += 1;
                        first_block.get_or_insert(at.duration_since(t));
                    }
                    Capture::Failed(e) => panic!("{e}"),
                    Capture::Opened(_) => {}
                }
            }
        }
        drop(rec);
        println!("first block after {:?}, {blocks} blocks in 500 ms", first_block.unwrap());
        lat.push(first_block.unwrap());
    }
    lat.sort();
    println!("latency min {:?} median {:?} max {:?}", lat[0], lat[5], lat[9]);
}
