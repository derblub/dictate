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

//! dictate – lokales Live-Diktat (whisper.cpp, CUDA).

pub mod actions;
pub mod audio;
pub mod config;
pub mod daemon;
pub mod diagnose;
pub mod engine;
pub mod filter;
pub mod gain;
pub mod http;
pub mod models;
pub mod palette;
pub mod pipeline;
pub mod protocol;
pub mod simulate;
pub mod stream;
pub mod ui;
pub mod wav;

/// Transkripttexte dürfen nur mit `DICTATE_DEBUG` ins Log.
pub fn debug_text() -> bool {
    std::env::var_os("DICTATE_DEBUG").is_some()
}
