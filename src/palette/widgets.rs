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

//! Eingebaute Anzeigen der Befehlspalette (`show_widget`, `PLAN-lizenz.md` O6): Verlauf der Zwischenablage und
//! Emoji-Wähler. Je Anzeige gilt der eingestellte Befehl (`palette.clipboard_command`, `palette.emoji_command`, argv),
//! sonst die automatische Wahl: Noctalia (`noctalia msg panel-toggle clipboard`) bzw. rofi mit dem Plugin rofi-emoji
//! (`rofi -modi emoji -show emoji`). Die Palette bietet eine Anzeige nur an, wenn ihr Programm gefunden wird
//! ([`available`]): weder ihre Schlüsselwörter noch die Liste bei leerer Eingabe, und eine Absicht des Modells dazu
//! endet mit [`super::NoTarget::WidgetMissing`]. Gesucht wird im `PATH` der Sitzung, den das Panel bei jeder Anfrage
//! mitschickt; es ist derselbe Suchpfad, mit dem `dictate-ui` die Programme startet (`ui::pal_exec::Programs`).
//! Ausgeführt wird nur in `dictate-ui` (`ui::pal_exec`), per argv, nie über eine Shell.
//!
//! rofi ohne rofi-emoji zeigt beim Aufruf von `-modi emoji` einen eigenen Fehler; darum sucht die automatische Wahl
//! auch das Plugin: in `ROFI_PLUGIN_PATH` (Einträge mit `:` getrennt, nur absolute) und unter dem Präfix des
//! gefundenen rofi (`<präfix>/lib/rofi` usw., Präfix = Ordner über `bin/` nach dem Auflösen von Links). Wer rofi
//! anders eingerichtet hat, trägt den Befehl selbst ein.

use super::intent::Widget;
use super::sources::desktop::find_program;
use crate::config::PaletteCfg;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Programm der automatischen Wahl für den Verlauf der Zwischenablage
pub const NOCTALIA: &str = "noctalia";
/// Programm der automatischen Wahl für Emoji
pub const ROFI: &str = "rofi";
/// Dateiname des Plugins rofi-emoji in einem Plugin-Ordner von rofi
pub const ROFI_EMOJI_PLUGIN: &str = "emoji.so";
/// Plugin-Ordner unter dem Präfix von rofi (Arch, Fedora/openSUSE, Debian/Ubuntu)
pub const ROFI_PLUGIN_SUBDIRS: [&str; 4] = ["lib/rofi", "lib64/rofi", "lib/x86_64-linux-gnu/rofi", "lib/aarch64-linux-gnu/rofi"];

/// Der eingestellte Befehl einer Anzeige (leer = automatisch)
pub fn command(cfg: &PaletteCfg, widget: Widget) -> &[String] {
    match widget {
        Widget::ClipboardHistory => &cfg.clipboard_command,
        Widget::Emoji => &cfg.emoji_command,
    }
}

/// Wird die Anzeige angeboten? `path`: Suchpfad der Sitzung (nur absolute Einträge); `rofi_plugin_path`: Wert von
/// `ROFI_PLUGIN_PATH` (nur für Emoji ohne eigenen Befehl).
pub fn available(widget: Widget, cfg: &PaletteCfg, path: &[PathBuf], rofi_plugin_path: Option<&OsStr>) -> bool {
    if let Some(first) = command(cfg, widget).first() {
        return find_program(first, path).is_some();
    }
    match widget {
        Widget::ClipboardHistory => find_program(NOCTALIA, path).is_some(),
        Widget::Emoji => find_program(ROFI, path).is_some_and(|rofi| rofi_emoji_found(&rofi, rofi_plugin_path)),
    }
}

/// Liegt das Plugin rofi-emoji in einem Plugin-Ordner des gefundenen rofi?
fn rofi_emoji_found(rofi: &Path, plugin_path: Option<&OsStr>) -> bool {
    plugin_dirs(rofi, plugin_path).iter().any(|d| is_plugin(&d.join(ROFI_EMOJI_PLUGIN)))
}

/// Plugin-Ordner in der Reihenfolge der Suche: `ROFI_PLUGIN_PATH`, dann unter dem Präfix von rofi.
fn plugin_dirs(rofi: &Path, plugin_path: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = plugin_path.map(|v| std::env::split_paths(v).filter(|p| p.is_absolute()).collect()).unwrap_or_default();
    let real = std::fs::canonicalize(rofi).unwrap_or_else(|_| rofi.to_path_buf());
    if let Some(prefix) = real.parent().and_then(Path::parent) {
        dirs.extend(ROFI_PLUGIN_SUBDIRS.iter().map(|s| prefix.join(s)));
    }
    dirs
}

/// Ein Plugin ist eine gewöhnliche Datei (Bibliotheken müssen nicht ausführbar sein)
fn is_plugin(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

#[cfg(test)]
mod tests {
    use super::super::sources::testutil::TempDir;
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn exe(t: &TempDir, rel: &str) -> PathBuf {
        let p = t.write(rel, "#!/bin/sh\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn cfg(clip: &[&str], emoji: &[&str]) -> PaletteCfg {
        PaletteCfg {
            clipboard_command: clip.iter().map(|s| s.to_string()).collect(),
            emoji_command: emoji.iter().map(|s| s.to_string()).collect(),
            ..PaletteCfg::default()
        }
    }

    #[test]
    fn automatic_clipboard_needs_noctalia_in_the_path() {
        let t = TempDir::new("widgets-clip");
        let path = vec![t.join("bin")];
        assert!(!available(Widget::ClipboardHistory, &cfg(&[], &[]), &path, None));
        // nicht ausführbar zählt nicht
        t.write("bin/noctalia", "");
        assert!(!available(Widget::ClipboardHistory, &cfg(&[], &[]), &path, None));
        exe(&t, "bin/noctalia");
        assert!(available(Widget::ClipboardHistory, &cfg(&[], &[]), &path, None));
        // ein Ordner, der nicht im Suchpfad steht, zählt nicht
        assert!(!available(Widget::ClipboardHistory, &cfg(&[], &[]), &[t.join("other")], None));
    }

    #[test]
    fn automatic_emoji_needs_rofi_and_the_plugin() {
        let t = TempDir::new("widgets-emoji");
        let path = vec![t.join("usr/bin")];
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None));
        exe(&t, "usr/bin/rofi");
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None), "rofi without rofi-emoji");
        // ein anderes Plugin genügt nicht
        t.write("usr/lib/rofi/calc.so", "");
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None));
        for sub in ROFI_PLUGIN_SUBDIRS {
            let plugin = t.write(&format!("usr/{sub}/emoji.so"), "");
            assert!(available(Widget::Emoji, &cfg(&[], &[]), &path, None), "{sub}");
            std::fs::remove_file(plugin).unwrap();
        }
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None));
    }

    #[test]
    fn emoji_plugin_from_rofi_plugin_path_and_through_a_link() {
        let t = TempDir::new("widgets-plugin-path");
        exe(&t, "opt/rofi/bin/rofi");
        t.write("plugins/emoji.so", "");
        std::fs::create_dir_all(t.join("bin")).unwrap();
        std::os::unix::fs::symlink(t.join("opt/rofi/bin/rofi"), t.join("bin/rofi")).unwrap();
        let path = vec![t.join("bin")];
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None));
        // nur absolute Einträge, mit `:` getrennt
        let rel = OsStr::new("plugins");
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, Some(rel)));
        let env = std::env::join_paths([t.join("nothing"), t.join("plugins")]).unwrap();
        assert!(available(Widget::Emoji, &cfg(&[], &[]), &path, Some(&env)));
        // der Link führt zum Präfix des echten rofi (`opt/rofi`), nicht zu dem des Links
        t.write("opt/rofi/lib/rofi/emoji.so", "");
        assert!(available(Widget::Emoji, &cfg(&[], &[]), &path, None));
        t.write("lib/rofi/emoji.so", "");
        std::fs::remove_file(t.join("opt/rofi/lib/rofi/emoji.so")).unwrap();
        assert!(!available(Widget::Emoji, &cfg(&[], &[]), &path, None));
    }

    #[test]
    fn a_set_command_replaces_the_automatic_choice() {
        let t = TempDir::new("widgets-command");
        let path = vec![t.join("bin")];
        // Noctalia und rofi samt Plugin da, aber ein eigener Befehl fehlt: nicht angeboten
        exe(&t, "bin/noctalia");
        exe(&t, "bin/rofi");
        t.write("lib/rofi/emoji.so", "");
        let set = cfg(&["copyq", "toggle"], &["bemoji"]);
        assert!(!available(Widget::ClipboardHistory, &set, &path, None));
        assert!(!available(Widget::Emoji, &set, &path, None));
        exe(&t, "bin/copyq");
        exe(&t, "bin/bemoji");
        assert!(available(Widget::ClipboardHistory, &set, &path, None));
        assert!(available(Widget::Emoji, &set, &path, None));
        // ein absoluter Pfad gilt ohne Suchpfad
        let abs = exe(&t, "elsewhere/picker").display().to_string();
        assert!(available(Widget::Emoji, &cfg(&[], &[&abs]), &[], None));
        assert!(!available(Widget::Emoji, &cfg(&[], &["/nonexistent/picker"]), &path, None));
        assert_eq!(command(&set, Widget::ClipboardHistory), ["copyq", "toggle"]);
        assert_eq!(command(&set, Widget::Emoji), ["bemoji"]);
    }
}
