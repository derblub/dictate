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

//! Grenzen für das Lesen der Markierung (PRIMARY) durch `dictate-ui` (`src/bin/dictate-ui/primary.rs`, über
//! Data-Control). Bis W7 las hier `Xclip` PRIMARY und CLIPBOARD unter X11; das entfiel mit AwesomeWM.
//!
//! Für die Textaktionen wird nur PRIMARY gelesen, nie CLIPBOARD (auch nicht als Rückfall): dort liegt oft etwas, das
//! der User gar nicht markiert hat, etwa ein kopiertes Passwort (CLAUDE.md, Fallen).

use crate::config::ACTIONS_MAX_CHARS;
use std::time::Duration;

/// Frist für das Lesen der Markierung
pub const TIMEOUT: Duration = Duration::from_secs(2);
/// Längste gelesene Markierung in Bytes: [`ACTIONS_MAX_CHARS`] Zeichen zu höchstens 4 Byte. Was länger ist, hat sicher
/// mehr Zeichen, als `actions.max_chars` je erlaubt; die genaue Grenze prüft der Daemon.
pub const MAX_BYTES: usize = ACTIONS_MAX_CHARS * 4;
