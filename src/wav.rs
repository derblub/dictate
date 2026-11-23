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

//! 16-kHz-Mono-WAV lesen.

use anyhow::{bail, Context, Result};
use std::path::Path;

/// Liest eine WAV-Datei als f32 in [-1, 1]. Bricht bei anderer Rate oder mehreren Kanälen ab.
pub fn read_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = r.spec();
    if spec.sample_rate != 16000 {
        bail!("{}: sample rate is {} Hz, expected 16000 Hz (convert with: ffmpeg -i in.wav -ar 16000 -ac 1 out.wav)", path.display(), spec.sample_rate);
    }
    if spec.channels != 1 {
        bail!("{}: {} channels, expected mono (convert with: ffmpeg -i in.wav -ar 16000 -ac 1 out.wav)", path.display(), spec.channels);
    }
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => r.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
            let scale = (1i64 << (bits - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<Vec<_>, _>>()?
        }
        (f, b) => bail!("{}: unsupported sample format {:?} / {} bit", path.display(), f, b),
    };
    Ok(samples)
}
