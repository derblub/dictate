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

//! Welche Eingabegeräte haben eine Taste E? Antwort aus sysfs (`/sys/class/input/eventN/device/capabilities/key`),
//! ohne ein Gerät zu öffnen. Live gemessen (20.11.): jedes `open` auf `/dev/input/event*` kostet 5 bis 12 ms, alle 26
//! Geräte auf blub zusammen 137 ms; danach hat nur eine Handvoll E. `ui::keys` öffnet darum nur noch diese. Gelesen
//! werden nur die Fähigkeiten der Geräte (gewöhnliche Dateien in sysfs, über `ui::fsutil`), nie ein Gerät.

use super::fsutil::read_regular;
use std::path::Path;

/// Bitfeld `capabilities/key`: Wörter (je `unsigned long`, 64 Bit) in Hex, durch Leerzeichen getrennt, das höchste
/// zuerst. Ist Bit `n` gesetzt?
pub fn has_bit(caps: &str, n: usize) -> bool {
    let words: Vec<&str> = caps.split_whitespace().collect();
    let Some(i) = words.len().checked_sub(1 + n / 64) else { return false };
    u64::from_str_radix(words[i], 16).is_ok_and(|w| w & (1u64 << (n % 64)) != 0)
}

/// Namen (`event5`, …) aller Geräte unter `class_input` (`/sys/class/input`), die Taste `key` melden, sortiert;
/// `None`, wenn sysfs nicht lesbar ist (dann prüft `ui::keys` jedes Gerät selbst)
pub fn devices_with_key(class_input: &Path, key: usize) -> Option<Vec<String>> {
    let mut out: Vec<String> = std::fs::read_dir(class_input)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("event"))
        .filter(|n| {
            read_regular(&class_input.join(n).join("device/capabilities/key"), 4096)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .is_some_and(|caps| has_bit(&caps, key))
        })
        .collect();
    out.sort_by_key(|n| n[5..].parse::<u32>().unwrap_or(u32::MAX));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_of_the_capability_mask() {
        // echte Tastatur: niedrigstes Wort fffffffffffffffe (Bits 1..63, also auch E = 18)
        let kbd = "1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe";
        assert!(has_bit(kbd, 18));
        assert!(!has_bit(kbd, 0));
        assert!(has_bit(kbd, 64 + 1));
        // Maus nur mit Knöpfen (Bits 272..), kein E
        assert!(!has_bit("1f0000 0 0 0 0", 18));
        assert!(has_bit("1f0000 0 0 0 0", 4 * 64 + 16));
        assert!(!has_bit("0", 18));
        assert!(!has_bit("", 18));
        assert!(!has_bit("zz", 18));
    }

    #[test]
    fn only_devices_with_e() {
        let root = std::env::temp_dir().join(format!("dictate-sysfs-test-{}", std::process::id()));
        for (n, caps) in [("event2", "0 fffffffffffffffe"), ("event10", "40000 0"), ("event1", "1f0000 0 0 0 0"), ("mouse0", "fffe")] {
            let d = root.join(n).join("device/capabilities");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("key"), format!("{caps}\n")).unwrap();
        }
        std::fs::create_dir_all(root.join("event3")).unwrap(); // ohne Fähigkeiten
                                                               // event10: Bit 18 im zweiten Wort von hinten = Taste 82, nicht E
        assert_eq!(devices_with_key(&root, 18), Some(vec!["event2".to_string()]));
        assert_eq!(devices_with_key(&root.join("missing"), 18), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
