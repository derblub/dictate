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

//! JSON aus Programmausgaben lesen, auch wenn sie an der Grenze abgeschnitten wurde.
//!
//! Die meisten Listen der Proben (`journalctl --list-boots`, `systemctl list-units`, `coredumpctl list`, `docker ps`) sind
//! Arrays aus *flachen* Objekten (oder Objekte Zeile für Zeile). Wird so eine Ausgabe an der Byte-Grenze gekürzt, ist
//! das Dokument als Ganzes ungültig, aber jedes vollständige Objekt davor bzw. danach noch brauchbar: [`objects`] liefert
//! genau diese. Für verschachtelte Dokumente (`ip -j addr`, `lsblk -J`, `tailscale status --json`) gibt es das nicht:
//! dort muss das ganze Dokument lesbar sein ([`document`]), sonst fällt die Probe auf den Rohtext zurück.

use serde_json::Value;

/// Das ganze Dokument, wenn es gültiges JSON ist.
pub fn document(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim()).ok()
}

/// Die vollständigen Objekte auf oberster Ebene eines Arrays (oder hintereinander, wie bei NDJSON) und ob etwas
/// fehlt (ein angeschnittenes Objekt am Ende, ein fehlendes `]`).
///
/// Beginnt der Text mitten in einem Objekt (die Ausgabe wurde vorn gekürzt), wird bis zur ersten Grenze `},{` übersprungen.
/// Ein Objekt, das sich nicht lesen lässt, fehlt in der Liste, ohne die übrigen zu stören.
pub fn objects(text: &str) -> (Vec<Value>, bool) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut incomplete = false;
    let mut in_array = false;
    match bytes.get(i) {
        Some(b'[') => {
            in_array = true;
            i += 1;
        }
        Some(b'{') => {}
        Some(_) => {
            // Angeschnitten: ab dem ersten `{` nach einem `},`
            incomplete = true;
            match find_boundary(text) {
                Some(start) => i = start,
                None => return (Vec::new(), true),
            }
        }
        None => return (Vec::new(), false),
    }
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut closed = !in_array;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    if depth == 0 && b == b'{' {
                        start = i;
                    }
                    depth += 1;
                }
                b'}' | b']' => {
                    if depth == 0 {
                        if b == b']' {
                            closed = true;
                        }
                    } else {
                        depth -= 1;
                        if depth == 0 && b == b'}' {
                            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=i]) {
                                out.push(v);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    (out, incomplete || depth > 0 || !closed)
}

/// Position des `{` hinter der ersten Grenze `}` `,` `{` (mit Leerraum dazwischen).
fn find_boundary(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'}' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b',') {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'{') {
                    return Some(j);
                }
            }
        }
        i += 1;
    }
    None
}

/// Text eines Feldes.
pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Ganzzahl eines Feldes, auch als Text („3“) oder als ganze Gleitkommazahl.
pub fn int_of(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_arrays_and_ndjson() {
        let (v, incomplete) = objects(r#"[{"a":1,"s":"x}"},{"a":2}]"#);
        assert_eq!(v.len(), 2);
        assert!(!incomplete);
        assert_eq!(str_of(&v[0], "s"), Some("x}"));
        let (v, incomplete) = objects("{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
        assert_eq!((v.len(), incomplete), (3, false));
        assert_eq!(objects("[]"), (vec![], false));
        assert_eq!(objects(""), (vec![], false));
        assert_eq!(objects("  \n"), (vec![], false));
    }

    #[test]
    fn a_cut_at_the_end_keeps_the_complete_objects() {
        let (v, incomplete) = objects(r#"[{"a":1},{"a":2},{"a":3,"b":"cu"#);
        assert_eq!(v.len(), 2);
        assert!(incomplete);
        let (v, incomplete) = objects(r#"[{"a":1},{"a":2}"#);
        assert_eq!(v.len(), 2);
        assert!(incomplete, "the closing bracket is missing");
    }

    #[test]
    fn a_cut_at_the_start_skips_to_the_first_boundary() {
        let (v, incomplete) = objects(r#"a":1,"b":"x"},{"a":2},{"a":3}]"#);
        assert_eq!(v.len(), 2);
        assert!(incomplete);
        assert_eq!(int_of(&v[0], "a"), Some(2));
        let (v, incomplete) = objects("no json at all");
        assert!(v.is_empty() && incomplete);
        // Mit Leerraum an der Grenze
        let (v, _) = objects("x\":1}  ,\n  {\"a\":5}]");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn broken_objects_are_skipped_without_disturbing_the_rest() {
        let (v, _) = objects(r#"[{"a":1},{"a":},{"a":3}]"#);
        assert_eq!(v.iter().filter_map(|o| int_of(o, "a")).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn escapes_and_nesting_do_not_confuse_the_scanner() {
        let (v, incomplete) = objects(r#"[{"a":"quote \" and \\ and }","n":{"x":[1,2,{"y":3}]}},{"a":"z"}]"#);
        assert_eq!(v.len(), 2);
        assert!(!incomplete);
        assert_eq!(str_of(&v[0], "a"), Some("quote \" and \\ and }"));
    }

    #[test]
    fn field_helpers() {
        let v: Value = serde_json::from_str(r#"{"s":"x","i":5,"t":"7","f":3.0,"g":3.5,"b":true}"#).unwrap();
        assert_eq!(str_of(&v, "s"), Some("x"));
        assert_eq!(str_of(&v, "i"), None);
        assert_eq!(int_of(&v, "i"), Some(5));
        assert_eq!(int_of(&v, "t"), Some(7));
        assert_eq!(int_of(&v, "f"), Some(3));
        assert_eq!(int_of(&v, "g"), None);
        assert_eq!(int_of(&v, "b"), None);
        assert_eq!(int_of(&v, "none"), None);
        assert!(document(" {\"a\":1} ").is_some());
        assert!(document("{\"a\":").is_none());
    }
}
