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

//! Tastenzustand aus einer Datei statt aus `/dev/input`: nur für den Prüfschritt `wayland` (`dictate-ui run
//! --fake-keys DATEI`). Dort kommen die Tasten über eine virtuelle Tastatur des kopflosen sway und erscheinen nie
//! unter `/dev/input`; uinput-Geräte sind in Tests verboten (das echte niri übernähme sie als Tastatur), und die
//! echten Tastaturen des Users dürfen einen Test nicht beeinflussen.
//!
//! Inhalt der Datei: `1` = E gedrückt, `0` = losgelassen, optional danach das Wort `mod` = ein Modifikator gehalten
//! (z. B. `0 mod`); fehlt die Datei oder steht vorn etwas anderes, gilt „kein Gerät lesbar“ (Rückfall Umschalten bzw.
//! Tippen statt Einfügen). Bewusst getrennt von [`super::keys`], dessen Test jedes Lesen verbietet.

use super::keys::KeySource;
use std::path::PathBuf;

pub struct FileKeys {
    path: PathBuf,
}

impl FileKeys {
    pub fn new(path: PathBuf) -> FileKeys {
        FileKeys { path }
    }
}

impl FileKeys {
    /// (E gedrückt, Modifikator gehalten) oder `None`, wenn die Datei fehlt oder vorn weder `1` noch `0` steht
    fn state(&self) -> Option<(bool, bool)> {
        // nur eine gewöhnliche Datei mit wenigen Bytes (nie ein Gerät oder FIFO, auch wenn der Pfad darauf zeigt)
        let bytes = super::fsutil::read_regular(&self.path, 16).ok()?;
        let text = std::str::from_utf8(&bytes).ok()?;
        let mut words = text.split_whitespace();
        let e = match words.next()? {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        Some((e, words.any(|w| w == "mod")))
    }
}

impl KeySource for FileKeys {
    fn e_down(&mut self) -> Option<bool> {
        self.state().map(|(e, _)| e)
    }

    fn mods_down(&mut self) -> Option<bool> {
        self.state().map(|(_, m)| m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_states() {
        let p = std::env::temp_dir().join(format!("dictate-fake-keys-{}", std::process::id()));
        let mut k = FileKeys::new(p.clone());
        assert_eq!(k.e_down(), None);
        std::fs::write(&p, "1\n").unwrap();
        assert_eq!(k.e_down(), Some(true));
        std::fs::write(&p, "0").unwrap();
        assert_eq!(k.e_down(), Some(false));
        assert_eq!(k.mods_down(), Some(false));
        std::fs::write(&p, "0 mod\n").unwrap();
        assert_eq!((k.e_down(), k.mods_down()), (Some(false), Some(true)));
        std::fs::write(&p, "x").unwrap();
        assert_eq!(k.e_down(), None);
        assert_eq!(k.mods_down(), None);
        assert_eq!(FileKeys::new("/dev/zero".into()).e_down(), None);
        std::fs::remove_file(&p).unwrap();
    }
}
