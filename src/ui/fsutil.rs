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

//! Kleine Dateien sicher lesen (Palette, Tastenzustand im Test): öffnen ohne zu blockieren, dann am geöffneten
//! Deskriptor prüfen, dass es eine gewöhnliche Datei ist (`fstat`, kein Wettlauf zwischen Prüfen und Öffnen), und
//! höchstens `max` Bytes lesen. Geräte (`/dev/input/event*`), FIFOs und Ordner werden nie gelesen: der Hauptthread
//! von `dictate-ui` blockiert nie, und kein Eingabegerät wird über diesen Weg gelesen (Sicherheitsregel 5).

use std::ffi::CString;
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| format!("{}: bad path", path.display()))?;
    // SAFETY: gültiger, nullterminierter Pfad; Ergebnis ist ein neuer Deskriptor oder -1
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY) };
    if fd < 0 {
        return Err(format!("{}: {}", path.display(), std::io::Error::last_os_error()));
    }
    // SAFETY: frisch geöffnet, gehört niemandem sonst
    let file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    let meta = file.metadata().map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.len() > max {
        return Err(format!("{} is larger than {max} bytes", path.display()));
    }
    let mut out = Vec::new();
    file.take(max).read_to_end(&mut out).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_small_regular_files() {
        let dir = std::env::temp_dir().join(format!("dictate-fsutil-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("f");
        std::fs::write(&f, b"0123456789").unwrap();
        assert_eq!(read_regular(&f, 10).unwrap(), b"0123456789");
        assert!(read_regular(&f, 9).unwrap_err().contains("larger"));
        assert!(read_regular(&dir, 10).unwrap_err().contains("regular"));
        assert!(read_regular(&dir.join("missing"), 10).is_err());
        // FIFO ohne Schreiber: Öffnen blockiert nicht, gelesen wird nicht
        let fifo = dir.join("fifo");
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: gültiger Pfad
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(read_regular(&fifo, 10).unwrap_err().contains("regular"));
        // Zeichengerät
        assert!(read_regular(Path::new("/dev/null"), 10).unwrap_err().contains("regular"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
