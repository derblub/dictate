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

//! Belegung der eigenen virtuellen Tastatur (T2, `zwp_virtual_keyboard_v1`; PLAN-wayland.md 3.4, Spike S2) und die
//! Tastenfolgen, die `dictate-ui` darüber schickt. Ohne Wayland: das Gerät selbst steht in `src/bin/dictate-ui/vkbd.rs`.
//!
//! **Belegung.** Wie wtype trägt jede Taste genau ein Keysym auf Ebene 1, Zeichen brauchen also nie einen
//! Modifikator. Zeichen liegen **nur** auf den evdev-Codes [`CHAR_CODES`] (Buchstaben-, Ziffern- und Satzzeichentasten
//! des Hauptblocks): Chromium deutet manche Codes nach ihrer physischen Bedeutung (gemessen: Code 14 = Rücktaste löschte
//! das vorige Zeichen, wenn dort `:` lag); Esc, Tab, Return, Rücktaste, Ziffernblock, Navigation und Sondertasten
//! bekommen nie ein Zeichen. Leerzeichen, Return, Rücktaste, Shift und Ctrl liegen auf ihren echten Codes
//! ([`FIXED`]); für `Ctrl+V` liegt `v` auf dem echten Code von V ([`Keymap::paste`]). Hat ein Text mehr verschiedene
//! Zeichen als Plätze, wird er in Abschnitte mit je eigener Belegung geteilt ([`segments`]).
//!
//! **Anschläge.** Jede Taste wird in derselben Nachrichtenfolge gedrückt und losgelassen ([`Stroke`]), Modifikatoren
//! eines Akkords umschließen ihn ganz. Zwischen zwei `flush` hält das Gerät also nie eine Taste: niri lässt beim
//! Zerstören des Geräts und beim Prozessende nicht los (S2), das Zielprogramm wiederholte sonst die Taste.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// evdev-Codes, auf denen Zeichen liegen dürfen: 1–0 ß ´ (2–13), Q–Ü + (16–27), A–Ä ^ (30–41), # Y–- (43–53), <> (86)
pub const CHAR_CODES: [u32; 48] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, //
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, //
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, //
    43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 86,
];

pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_ENTER: u32 = 28;
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_V: u32 = 47;
pub const KEY_SPACE: u32 = 57;

/// Tasten auf ihren echten Codes, in jeder Belegung
pub const FIXED: [(u32, &str); 5] =
    [(KEY_BACKSPACE, "BackSpace"), (KEY_ENTER, "Return"), (KEY_LEFTCTRL, "Control_L"), (KEY_LEFTSHIFT, "Shift_L"), (KEY_SPACE, "space")];

/// Bits der Modifikatoren in `zwp_virtual_keyboard_v1.modifiers` (Standard-Maske: Shift, Lock, Control, …)
pub const MOD_SHIFT: u32 = 1;
pub const MOD_CTRL: u32 = 4;

/// Keysym eines Zeichens: Buchstaben und Ziffern mit Namen, Latin-1 mit dem klassischen Wert (wie wtype; mit
/// `U`-Keysyms verlor Chromium Zeichen, S2), alles andere als `U+XXXX`-Keysym
pub fn keysym(c: char) -> String {
    match c {
        ' ' => "space".into(),
        c if c.is_ascii_alphanumeric() => c.to_string(),
        c if matches!(c as u32, 0x21..=0x7e | 0xa0..=0xff) => format!("0x{:x}", c as u32),
        c => format!("U{:04X}", c as u32),
    }
}

/// Eine Belegung: evdev-Code → Keysym
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: BTreeMap<u32, String>,
    chars: BTreeMap<char, u32>,
}

impl Keymap {
    fn base() -> Keymap {
        Keymap { keys: FIXED.iter().map(|&(c, s)| (c, s.to_string())).collect(), chars: BTreeMap::from([(' ', KEY_SPACE)]) }
    }

    /// Belegung für höchstens [`CHAR_CODES`]`.len()` verschiedene Zeichen (ohne Leerzeichen); `None`, wenn es mehr sind
    /// oder ein Steuerzeichen dabei ist (die Ausgabe entfernt sie vorher, `output::sanitize`)
    pub fn for_chars(chars: impl IntoIterator<Item = char>) -> Option<Keymap> {
        let mut m = Keymap::base();
        let mut free = CHAR_CODES.iter();
        for c in chars {
            if c.is_control() {
                return None;
            }
            if m.chars.contains_key(&c) {
                continue;
            }
            let code = *free.next()?;
            m.keys.insert(code, keysym(c));
            m.chars.insert(c, code);
        }
        Some(m)
    }

    /// Belegung für `Ctrl+V`: `v` auf dem echten Code von V (Programme, die Kürzel nach dem Code statt nach dem Keysym
    /// auflösen, sehen dasselbe)
    pub fn paste() -> Keymap {
        let mut m = Keymap::base();
        m.keys.insert(KEY_V, "v".into());
        m.chars.insert('v', KEY_V);
        m
    }

    /// evdev-Code eines Zeichens dieser Belegung
    pub fn code(&self, c: char) -> Option<u32> {
        self.chars.get(&c).copied()
    }

    /// Alle belegten Codes (für Tests und das Loslassen)
    pub fn codes(&self) -> impl Iterator<Item = u32> + '_ {
        self.keys.keys().copied()
    }

    /// XKB-Text der Belegung (xkb-Keycode = evdev-Code + 8), wie ihn `zwp_virtual_keyboard_v1.keymap` erwartet
    pub fn to_xkb(&self) -> String {
        let mut s = String::from("xkb_keymap {\nxkb_keycodes \"dictate\" {\nminimum = 8;\nmaximum = 255;\n");
        for code in self.keys.keys() {
            let _ = writeln!(s, "<K{code}> = {};", code + 8);
        }
        s += "};\nxkb_types \"dictate\" { include \"complete\" };\nxkb_compat \"dictate\" { include \"complete\" };\n";
        s += "xkb_symbols \"dictate\" {\n";
        for (code, sym) in &self.keys {
            let _ = writeln!(s, "key <K{code}> {{[ {sym} ]}};");
        }
        let _ = writeln!(s, "modifier_map Shift {{ <K{KEY_LEFTSHIFT}> }};");
        let _ = writeln!(s, "modifier_map Control {{ <K{KEY_LEFTCTRL}> }};");
        s += "};\n};\n";
        s
    }
}

/// Text in Abschnitte, deren verschiedene Zeichen in eine Belegung passen (Reihenfolge bleibt). Steuerzeichen dürfen
/// nicht vorkommen (vorher `output::sanitize`); ein solcher Text ergibt `None`.
pub fn segments(text: &str) -> Option<Vec<(Keymap, String)>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut distinct: Vec<char> = Vec::new();
    for c in text.chars() {
        if c.is_control() {
            return None;
        }
        if c != ' ' && !distinct.contains(&c) {
            if distinct.len() == CHAR_CODES.len() {
                out.push((Keymap::for_chars(distinct.iter().copied())?, std::mem::take(&mut cur)));
                distinct.clear();
            }
            distinct.push(c);
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push((Keymap::for_chars(distinct)?, cur));
    }
    Some(out)
}

/// Ein Tastenereignis: Code und gedrückt/losgelassen, dazu die Modifikator-Maske, die danach gilt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ev {
    pub code: u32,
    pub down: bool,
    /// `Some(maske)`: danach `modifiers(maske, …)` senden
    pub mods: Option<u32>,
}

/// Ein ganzer Anschlag (Taste oder Akkord): wird ohne `flush` dazwischen gesendet und lässt alles wieder los
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stroke(pub Vec<Ev>);

impl Stroke {
    /// Eine Taste drücken und loslassen
    pub fn tap(code: u32) -> Stroke {
        Stroke(vec![Ev { code, down: true, mods: None }, Ev { code, down: false, mods: None }])
    }

    /// Modifikator halten (Code und Bit), Taste drücken und loslassen, Modifikator loslassen
    pub fn chord(modifier: u32, bit: u32, code: u32) -> Stroke {
        Stroke(vec![
            Ev { code: modifier, down: true, mods: Some(bit) },
            Ev { code, down: true, mods: None },
            Ev { code, down: false, mods: None },
            Ev { code: modifier, down: false, mods: Some(0) },
        ])
    }

    pub fn shift_return() -> Stroke {
        Stroke::chord(KEY_LEFTSHIFT, MOD_SHIFT, KEY_ENTER)
    }

    pub fn ctrl_v() -> Stroke {
        Stroke::chord(KEY_LEFTCTRL, MOD_CTRL, KEY_V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codes, die nie ein Zeichen tragen dürfen (Sonderbedeutung in Programmen, S2)
    const SPECIAL: [u32; 16] = [1, 14, 15, 28, 29, 42, 54, 55, 56, 57, 58, 96, 97, 100, 125, 126];

    #[test]
    fn chars_only_on_allowed_codes() {
        let text = "Grüße, Maß: äöü ÄÖÜ ß € – „ok“ :,-.;!?/()=+*#'<>|@{}[]^°§$%&_~`\\\"";
        for (map, part) in segments(text).unwrap() {
            for c in part.chars().filter(|&c| c != ' ') {
                let code = map.code(c).unwrap();
                assert!(CHAR_CODES.contains(&code), "{c:?} on {code}");
                assert!(!SPECIAL.contains(&code), "{c:?} on special code {code}");
            }
            assert_eq!(map.code(' '), Some(KEY_SPACE));
            // nur zugelassene oder feste Codes in der Belegung
            for code in map.codes() {
                assert!(CHAR_CODES.contains(&code) || FIXED.iter().any(|&(f, _)| f == code), "{code}");
            }
        }
    }

    #[test]
    fn more_distinct_chars_than_slots_split_into_segments() {
        let text: String = (0..120).map(|i| char::from_u32(0x4e00 + i).unwrap()).collect();
        let segs = segments(&text).unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs.iter().map(|(_, t)| t.as_str()).collect::<String>(), text);
        for (map, part) in &segs {
            assert!(part.chars().all(|c| map.code(c).is_some()));
        }
        assert_eq!(segments("").unwrap(), vec![]);
        // genau 48 verschiedene passen in einen Abschnitt, Leerzeichen zählen nicht
        let t48: String = (0..48).map(|i| char::from_u32(0x4e00 + i).unwrap()).collect::<String>() + "   ";
        assert_eq!(segments(&t48).unwrap().len(), 1);
    }

    #[test]
    fn control_chars_are_refused() {
        assert!(segments("a\nb").is_none());
        assert!(segments("a\u{1b}b").is_none());
        assert!(segments("a\u{9b}b").is_none());
        assert!(Keymap::for_chars("\t".chars()).is_none());
    }

    #[test]
    fn keysyms() {
        assert_eq!(keysym('a'), "a");
        assert_eq!(keysym('Z'), "Z");
        assert_eq!(keysym('7'), "7");
        assert_eq!(keysym(':'), "0x3a");
        assert_eq!(keysym('ß'), "0xdf");
        assert_eq!(keysym('€'), "U20AC");
        assert_eq!(keysym('„'), "U201E");
        assert_eq!(keysym(' '), "space");
    }

    #[test]
    fn xkb_text_lists_every_key_once() {
        let m = Keymap::for_chars("aä€".chars()).unwrap();
        let x = m.to_xkb();
        assert!(x.contains("<K2> = 10;") && x.contains("key <K2> {[ a ]};"));
        assert!(x.contains("key <K3> {[ 0xe4 ]};") && x.contains("key <K4> {[ U20AC ]};"));
        assert!(x.contains("key <K14> {[ BackSpace ]};") && x.contains("key <K28> {[ Return ]};"));
        assert!(x.contains("modifier_map Shift { <K42> };") && x.contains("modifier_map Control { <K29> };"));
        assert_eq!(x.matches("key <K").count(), 3 + FIXED.len());
        let p = Keymap::paste();
        assert_eq!(p.code('v'), Some(KEY_V));
        assert!(p.to_xkb().contains("key <K47> {[ v ]};"));
    }

    /// Kein Anschlag lässt eine Taste oder einen Modifikator gedrückt
    #[test]
    fn strokes_release_everything() {
        for s in [Stroke::tap(30), Stroke::shift_return(), Stroke::ctrl_v(), Stroke::tap(KEY_BACKSPACE)] {
            let mut down = std::collections::BTreeSet::new();
            let mut mods = 0;
            for e in &s.0 {
                if e.down {
                    assert!(down.insert(e.code), "pressed twice: {s:?}");
                } else {
                    assert!(down.remove(&e.code), "released without press: {s:?}");
                }
                if let Some(m) = e.mods {
                    mods = m;
                }
            }
            assert!(down.is_empty() && mods == 0, "{s:?}");
        }
        assert_eq!(Stroke::ctrl_v().0[0], Ev { code: KEY_LEFTCTRL, down: true, mods: Some(MOD_CTRL) });
    }
}
