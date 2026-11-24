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

//! Farben von `dictate-ui` und `dictate-settings` aus der Noctalia-Palette (Entscheidung Daniel, 20.11.2026: das
//! Pushing-Pixels-Design entfällt; dictate richtet sich nach der Palette, die Noctalia auch für niri, Alacritty und
//! niri-coach erzeugt).
//!
//! Noctalia schreibt die Palette über die Vorlage `noctalia/dictate.json.tmpl` (im Repo; eingetragen in
//! `~/.config/noctalia/config.toml` unter `[theme.templates.user.dictate]`) nach `~/.local/state/dictate/palette.json`,
//! bei jedem Wechsel von Schema oder Modus neu. `dictate-ui` und das Einstellungsfenster beobachten die Datei und färben
//! sofort um (das Einstellungsfenster bindet dieses Modul samt `fsutil.rs` per `#[path]` ein, es braucht kein GTK). Fehlt sie, ist
//! sie unlesbar oder fehlt eine der benötigten Rollen, gilt die eingebaute Palette (Tokyo Night dunkel, dieselbe
//! wie in niri-coach). Gleiche Rollennamen wie niri-coach (`crates/coach-core/src/palette.rs`), Material-3-Begriffe.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Größte Palettendatei (eine echte hat unter 2 KiB); größere, Geräte und FIFOs werden nie gelesen
pub const MAX_BYTES: u64 = 64 * 1024;

/// Eine 8-Bit-sRGB-Farbe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Kanäle in 0..1 (Cairo)
    pub fn unit(self) -> (f64, f64, f64) {
        (f64::from(self.0) / 255.0, f64::from(self.1) / 255.0, f64::from(self.2) / 255.0)
    }

    /// Mischung `self` → `other` mit Anteil `t` von `other`
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let ch = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(ch(self.0, other.0), ch(self.1, other.1), ch(self.2, other.2))
    }

    /// `rgba(r, g, b, a)` für CSS
    pub fn css(self, alpha: f64) -> String {
        format!("rgba({}, {}, {}, {:.3})", self.0, self.1, self.2, alpha.clamp(0.0, 1.0))
    }
}

/// Eckenradius der Fenster und Flächen wie Noctalias Panels (`background_radius`): Overlay, Panels, Popups des
/// Einstellungsfensters; dazu `layer-rule` bzw. `window-rule` in `niri/dictate-binds.kdl`
pub const RADIUS: i32 = 12;
/// Eckenradius von Feldern, Knöpfen und Zeilen (Panel in `dictate-ui`: `.dp-row`, `.dp-btn`, `entry.dp-field`)
pub const RADIUS_CONTROL: i32 = 8;
/// Eckenradius von Chips und Kästchen (`.dp-chip`, `.dp-box`)
pub const RADIUS_CHIP: i32 = 4;

/// Die Rollen, die dictate liest (`dictate-ui` und das Einstellungsfenster). Die Vorlage schreibt dieselben 28 Rollen
/// wie die von niri-coach, gelesen werden nur diese; jede ist Pflicht (fehlt eine, gilt [`FALLBACK`] ganz)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    /// Akzent: Mittelzeile der Welle, wartender Pixel, gewählte Chips, Hauptknopf
    pub primary: Rgb,
    /// Text auf `primary` (Hauptknopf des Einstellungsfensters)
    pub on_primary: Rgb,
    pub error: Rgb,
    /// Fläche des Overlays, der Panels und des Inhalts im Einstellungsfenster
    pub surface: Rgb,
    /// Seitenleiste des Einstellungsfensters
    pub surface_container_low: Rgb,
    /// Popups und Hinweisleiste des Einstellungsfensters
    pub surface_container: Rgb,
    /// Felder, Auswahllisten und Zahlenfelder des Einstellungsfensters
    pub surface_container_high: Rgb,
    /// Text
    pub on_surface: Rgb,
    /// unbestätigter Tail, Nebentext
    pub on_surface_variant: Rgb,
    /// Haarlinie am Rand
    pub outline_variant: Rgb,
}

/// Namen der gelesenen Rollen (für den Test gegen die Vorlage); `mode` ist keine Rolle
pub const ROLES: [&str; 10] = [
    "primary",
    "on_primary",
    "error",
    "surface",
    "surface_container_low",
    "surface_container",
    "surface_container_high",
    "on_surface",
    "on_surface_variant",
    "outline_variant",
];

/// Eingebaute Palette: Tokyo Night dunkel (Noctalia, wie das Fixture von niri-coach und
/// `tests/gui/fixtures/palette-dark.json`)
pub const FALLBACK: Palette = Palette {
    dark: true,
    primary: Rgb(0x7a, 0xa2, 0xf7),
    on_primary: Rgb(0x16, 0x16, 0x1e),
    error: Rgb(0xf7, 0x76, 0x8e),
    surface: Rgb(0x1a, 0x1b, 0x26),
    surface_container_low: Rgb(0x1f, 0x21, 0x30),
    surface_container: Rgb(0x24, 0x28, 0x3b),
    surface_container_high: Rgb(0x2c, 0x31, 0x48),
    on_surface: Rgb(0xc0, 0xca, 0xf5),
    on_surface_variant: Rgb(0x9a, 0xa5, 0xce),
    outline_variant: Rgb(0x35, 0x3d, 0x57),
};

/// Woher die Palette stammt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File,
    /// Eingebaute Palette, mit Grund (nur fürs Log)
    Fallback(String),
}

/// `$XDG_STATE_HOME/dictate/palette.json`, sonst `~/.local/state/dictate/palette.json`; `DICTATE_PALETTE` überschreibt
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("DICTATE_PALETTE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    state.join("dictate/palette.json")
}

impl Palette {
    /// Aus dem JSON der Vorlage; Fehler nennt die erste fehlende oder ungültige Rolle
    pub fn parse(text: &str) -> Result<Palette, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let role = |name: &str| -> Result<Rgb, String> {
            let s = v.get(name).and_then(Value::as_str).ok_or_else(|| format!("role {name} missing"))?;
            Rgb::parse(s).ok_or_else(|| format!("role {name}: {s:?} is not #rrggbb"))
        };
        let dark = match v.get("mode").and_then(Value::as_str) {
            Some("light") => false,
            Some("dark") | None => true,
            Some(other) => return Err(format!("mode {other:?} is neither dark nor light")),
        };
        Ok(Palette {
            dark,
            primary: role("primary")?,
            on_primary: role("on_primary")?,
            error: role("error")?,
            surface: role("surface")?,
            surface_container_low: role("surface_container_low")?,
            surface_container: role("surface_container")?,
            surface_container_high: role("surface_container_high")?,
            on_surface: role("on_surface")?,
            on_surface_variant: role("on_surface_variant")?,
            outline_variant: role("outline_variant")?,
        })
    }

    /// Datei lesen (nur eine gewöhnliche Datei bis [`MAX_BYTES`]); sonst die eingebaute Palette mit Grund
    pub fn load(path: &Path) -> (Palette, Source) {
        match read_small(path).and_then(|t| Palette::parse(&t)) {
            Ok(p) => (p, Source::File),
            Err(e) => (FALLBACK, Source::Fallback(e)),
        }
    }

    /// Zwischenton der Welle (Abstand 1 zur Mitte): halb Akzent, halb Text
    pub fn mix(&self) -> Rgb {
        self.primary.mix(self.on_surface, 0.5)
    }
}

fn read_small(path: &Path) -> Result<String, String> {
    let bytes = super::fsutil::read_regular(path, MAX_BYTES)?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die eingebaute Palette entspricht der Vorlage, wie Noctalia sie für Tokyo Night dunkel schreibt (Fixture der
    /// GUI-Tests, alle 28 Rollen in der Reihenfolge der Vorlage)
    const TOKYO: &str = include_str!("../../tests/gui/fixtures/palette-dark.json");
    const TOKYO_LIGHT: &str = include_str!("../../tests/gui/fixtures/palette-light.json");
    const TEMPLATE: &str = include_str!("../../noctalia/dictate.json.tmpl");

    fn keys(json: &str) -> Vec<String> {
        // serde_json ohne `preserve_order` sortiert Schlüssel; die Reihenfolge prüft der Text selbst
        json.lines().filter_map(|l| l.trim().strip_prefix('"')?.split_once('"').map(|(k, _)| k.to_string())).collect()
    }

    #[test]
    fn parse_template_output() {
        assert_eq!(Palette::parse(TOKYO).unwrap(), FALLBACK);
        let light = Palette::parse(TOKYO_LIGHT).unwrap();
        assert!(!light.dark);
        assert_eq!(light.surface, Rgb(0xe1, 0xe2, 0xe7));
        assert!(!Palette::parse(&TOKYO.replace("\"dark\"", "\"light\"")).unwrap().dark);
        assert!(Palette::parse(&TOKYO.replace("\"#7aa2f7\"", "\"blue\"")).unwrap_err().contains("primary"));
        assert!(Palette::parse(&TOKYO.replace("\"error\"", "\"err\"")).unwrap_err().contains("error missing"));
        assert!(Palette::parse(&TOKYO.replace("\"dark\"", "\"dim\"")).is_err());
        assert!(Palette::parse("{").is_err());
    }

    /// Jede gelesene Rolle steht in der Vorlage (Regel 1 in PLAN-wayland.md 6h: nur die 28 Rollen der installierten
    /// Vorlage) und ist Pflicht; die Fixtures tragen genau die Schlüssel der Vorlage in ihrer Reihenfolge
    #[test]
    fn roles_come_from_the_template() {
        let template = keys(TEMPLATE);
        assert_eq!(template.len(), 29, "mode und 28 Rollen");
        assert_eq!(keys(TOKYO), template);
        assert_eq!(keys(TOKYO_LIGHT), template);
        for role in ROLES {
            assert!(template.iter().any(|k| k == role), "{role} fehlt in der Vorlage");
            let without: String = TOKYO.lines().filter(|l| !l.trim().starts_with(&format!("\"{role}\""))).collect();
            assert!(Palette::parse(&without).unwrap_err().contains(&format!("role {role} missing")), "{role}");
        }
    }

    #[test]
    fn colours() {
        assert_eq!(Rgb::parse("#7AA2F7"), Some(Rgb(0x7a, 0xa2, 0xf7)));
        assert_eq!(Rgb::parse("7aa2f7"), None);
        assert_eq!(Rgb::parse("#7aa2f"), None);
        assert_eq!(Rgb(0x7a, 0xa2, 0xf7).hex(), "#7aa2f7");
        assert_eq!(Rgb(0, 0, 0).mix(Rgb(255, 255, 255), 0.5), Rgb(128, 128, 128));
        assert_eq!(Rgb(1, 2, 3).css(0.88), "rgba(1, 2, 3, 0.880)");
        assert_eq!(Rgb(1, 2, 3).css(7.0), "rgba(1, 2, 3, 1.000)");
    }

    #[test]
    fn load_falls_back() {
        let dir = std::env::temp_dir().join(format!("dictate-palette-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("palette.json");
        assert!(matches!(Palette::load(&p), (FALLBACK, Source::Fallback(_))));
        std::fs::write(&p, TOKYO.replace("#7aa2f7", "#ff0000")).unwrap();
        let (pal, src) = Palette::load(&p);
        assert_eq!((pal.primary, src), (Rgb(255, 0, 0), Source::File));
        std::fs::write(&p, "x".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert!(matches!(Palette::load(&p), (FALLBACK, Source::Fallback(e)) if e.contains("larger")));
        // ein Ordner (oder Gerät, FIFO) wird nie gelesen
        assert!(matches!(Palette::load(&dir), (FALLBACK, Source::Fallback(e)) if e.contains("regular")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
