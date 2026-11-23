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

//! Text der Proben: bereinigen, zu lange Zeilen kürzen, wiederholte Zeilen zusammenfassen und auf eine Byte-Grenze
//! kürzen (Journale vorn, das Ende bleibt).
//!
//! Die Ausgabe der Programme ist fremder Text (Logs können Escape-Folgen, Steuerzeichen und riesige Zeilen enthalten).
//! [`clean`] macht daraus druckbaren UTF-8-Text; [`collapse_repeats`] fasst Serien gleicher Meldungen zusammen
//! („… repeated 76×“, verglichen nach Nachricht ohne Zeitstempel und PID, Zahlen und Adressen maskiert), damit eine
//! Segfault-Serie nicht das Budget frisst; [`cut_to_bytes`] kürzt auf die Grenze und sagt, wie viele Zeilen fehlen.

use super::time::parse_iso_prefix;
use crate::diagnose::run::Keep;

/// Längste Zeile (Bytes); längere Zeilen werden am Ende gekürzt (minifizierte JSON-Logs können 100 KB je Zeile haben).
pub const MAX_LINE: usize = 2000;
/// Ab so vielen gleichen Zeilen hintereinander wird zusammengefasst.
pub const MIN_RUN: usize = 3;
/// Kommt eine Meldung in so vielen getrennten Serien vor, bleiben nur die ersten zwei und die letzte.
pub const MIN_SCATTERED: usize = 6;

/// Gibt es das Zeichen im bereinigten Text nicht mehr? Steuerzeichen außer Zeilenumbruch und Tabulator, und die
/// Zeichen, die die Leserichtung umschalten oder verbergen (Anweisungen in Logs sollen nicht verborgen bleiben). Dasselbe
/// Prädikat gilt für die Texte des Modells, die das Panel zeigt (`diagnose::ai`, Review L1).
pub(crate) fn dropped(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t') || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// Macht Programmausgabe zu druckbarem Text: ungültiges UTF-8 wird zu `\u{fffd}`, ANSI-Escape-Folgen (Farben)
/// entfallen, `\r\n` und einzelnes `\r` werden zu Zeilenumbrüchen, übrige Steuerzeichen entfallen. Jede Zeile wird auf
/// [`MAX_LINE`] Bytes gekürzt.
pub fn clean(bytes: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(lossy.len());
    let mut chars = lossy.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ … Endbyte 0x40–0x7e; OSC: ESC ] … BEL oder ESC \; sonst das eine Zeichen nach ESC
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        for n in chars.by_ref() {
                            if ('\u{40}'..='\u{7e}').contains(&n) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        chars.next();
                        while let Some(n) = chars.next() {
                            if n == '\u{7}' {
                                break;
                            }
                            if n == '\u{1b}' {
                                chars.next();
                                break;
                            }
                        }
                    }
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push('\n');
                }
            }
            c if dropped(c) => {}
            c => out.push(c),
        }
    }
    cap_lines(&out)
}

/// Kürzt jede Zeile auf [`MAX_LINE`] Bytes (an einer Zeichengrenze) und hängt an, wie viele Bytes fehlen.
pub fn cap_lines(text: &str) -> String {
    if text.lines().all(|l| l.len() <= MAX_LINE) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.len() <= MAX_LINE {
            out.push_str(line);
        } else {
            let cut = safe_head(line, MAX_LINE);
            out.push_str(&line[..cut]);
            out.push_str(&format!("…[+{} bytes]", line.len() - cut));
        }
    }
    out
}

/// Größte Zeichengrenze höchstens bei `max`.
pub fn floor_boundary(s: &str, max: usize) -> usize {
    let mut i = max.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Welche Form die Zeilen haben (bestimmt, was beim Vergleichen ignoriert wird).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineFormat {
    /// `journalctl -o short-iso`: `2026-11-04T09:47:12+0100 host unit[pid]: Nachricht`.
    Journal,
    /// Zeitstempel als erstes Wort, dann die Nachricht (`docker logs --timestamps`).
    Timestamped,
    /// Kein Zeitstempel; die ganze Zeile zählt.
    Plain,
}

/// Der Zeitstempel am Zeilenanfang als Text, falls die Zeile mit einem beginnt.
fn timestamp_of(line: &str) -> Option<&str> {
    let end = line.find(' ')?;
    parse_iso_prefix(&line[..end]).map(|_| &line[..end])
}

/// Entfernt `[Zahl]` aus dem ersten Wort (`dockerd[1234]:` wird zu `dockerd:`).
fn drop_pid(word: &str) -> String {
    if let Some(open) = word.find('[') {
        if let Some(close) = word[open..].find(']') {
            let inner = &word[open + 1..open + close];
            if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                return format!("{}{}", &word[..open], &word[open + close + 1..]);
            }
        }
    }
    word.to_string()
}

/// Die Nachricht ohne Zeitstempel, Rechnername und PID (Schlüssel für gleiche Meldungen).
fn message_of(line: &str, format: LineFormat) -> String {
    match format {
        LineFormat::Plain => line.to_string(),
        LineFormat::Timestamped => match timestamp_of(line) {
            Some(ts) => line[ts.len()..].trim_start().to_string(),
            None => line.to_string(),
        },
        LineFormat::Journal => {
            let Some(ts) = timestamp_of(line) else { return line.to_string() };
            let rest = line[ts.len()..].trim_start();
            // Rechnername überspringen, dann `unit[pid]:` ohne PID
            let Some((_host, rest)) = rest.split_once(' ') else { return rest.to_string() };
            let rest = rest.trim_start();
            match rest.split_once(' ') {
                Some((ident, message)) => format!("{} {message}", drop_pid(ident)),
                None => drop_pid(rest),
            }
        }
    }
}

/// Ist das Wort eine Zahl, eine Adresse oder ein Hash (wird im Vergleich zu `#`)?
fn numeric_token(t: &str) -> bool {
    if t.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if let Some(hex) = t.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    // Hex ohne Ziffer ist erst ab acht Zeichen sicher eine Adresse oder ein Hash (`deadbeef`), kürzer ein Wort (`cafe`)
    t.bytes().all(|b| b.is_ascii_hexdigit()) && (t.len() >= 8 || (t.len() >= 3 && t.bytes().any(|b| b.is_ascii_digit())))
}

/// Die Nachricht mit allen Zahlen, Adressen und Hashes als `#`: Zeilen, die sich nur darin unterscheiden, gelten als
/// gleich (Segfault-Meldungen mit wechselnder Adresse und PID).
pub fn mask_numbers(s: &str) -> String {
    fn flush(tok: &mut String, out: &mut String) {
        if !tok.is_empty() {
            if numeric_token(tok) {
                out.push('#');
            } else {
                out.push_str(tok);
            }
            tok.clear();
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut tok = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            tok.push(c);
        } else {
            flush(&mut tok, &mut out);
            out.push(c);
        }
    }
    flush(&mut tok, &mut out);
    out
}

/// Eine Serie gleicher (nach Maskierung) Zeilen.
struct Run {
    first: usize,
    last: usize,
    count: usize,
    /// Alle Zeilen der Serie sind bis auf Zeitstempel, Rechnername und PID wörtlich gleich.
    exact: bool,
    key: String,
}

/// Ergebnis von [`collapse_repeats`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collapsed {
    pub text: String,
    /// Wie viele Zeilen weniger als vorher.
    pub saved_lines: usize,
}

fn run_marker(lines: &[&str], run: &Run) -> String {
    let last = timestamp_of(lines[run.last]).map(|ts| format!(", last at {ts}")).unwrap_or_default();
    let numbers = if run.exact { "" } else { ", numbers differ" };
    format!("… repeated {}× (this line first{last}{numbers})", run.count)
}

/// Fasst gleiche Meldungen zusammen: Serien von mindestens [`MIN_RUN`] aufeinanderfolgenden Zeilen (nach Maskierung
/// gleich) werden zu der ersten Zeile und `… repeated N× (…)`; eine Meldung, die in mindestens [`MIN_SCATTERED`] getrennten
/// Serien vorkommt, bleibt nur in den ersten zwei und der letzten, die übrigen ersetzt eine Zeile
/// `… N× more of the message above omitted`. Stünden mehrere solcher Zeilen untereinander (ein wiederholter Block aus
/// mehreren Zeilen wie ein Absturz mit Backtrace in `docker_logs`, abwechselnde Meldungen), werden sie eine Zeile
/// `… N× more of the K messages above omitted` (bzw. `… M to N× …`, wenn die Zahlen abweichen), wie eine Serie gleicher
/// Zeilen. Eine Zeile mit Meldung, die nur einmal vorkommt, bleibt unverändert.
pub fn collapse_repeats(text: &str, format: LineFormat) -> Collapsed {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < MIN_RUN {
        return Collapsed { text: text.to_string(), saved_lines: 0 };
    }
    let messages: Vec<String> = lines.iter().map(|l| message_of(l, format)).collect();
    let keys: Vec<String> = messages.iter().map(|m| mask_numbers(m)).collect();

    // Serien bilden
    let mut runs: Vec<Run> = Vec::new();
    for i in 0..lines.len() {
        match runs.last_mut() {
            Some(r) if r.key == keys[i] && !keys[i].trim().is_empty() => {
                r.last = i;
                r.count += 1;
                r.exact &= messages[i] == messages[r.first];
            }
            _ => runs.push(Run { first: i, last: i, count: 1, exact: true, key: keys[i].clone() }),
        }
    }

    // Serien, die zusammengefasst werden: lang genug. Verstreute Serien derselben Meldung (mindestens MIN_SCATTERED
    // Serien, von denen jede zählt): erste zwei und letzte bleiben
    let mut occurrences: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (ri, r) in runs.iter().enumerate() {
        if r.count >= MIN_RUN || r.count == 1 {
            occurrences.entry(r.key.as_str()).or_default().push(ri);
        }
    }
    let mut omitted: std::collections::HashMap<usize, ()> = std::collections::HashMap::new();
    let mut omitted_summary: std::collections::HashMap<usize, (usize, Option<String>)> = std::collections::HashMap::new();
    for ris in occurrences.values() {
        if ris.len() >= MIN_SCATTERED {
            let drop = &ris[2..ris.len() - 1];
            let total: usize = drop.iter().map(|&ri| runs[ri].count).sum();
            let last_ts = timestamp_of(lines[runs[*drop.last().expect("at least three")].last]).map(str::to_string);
            omitted_summary.insert(drop[0], (total, last_ts));
            for &ri in drop {
                omitted.insert(ri, ());
            }
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut kept = 0usize;
    let push = |line: &str, out: &mut String| {
        out.push_str(line);
        out.push('\n');
    };
    // Sammelzeilen, die untereinander stünden (ein wiederholter Block aus mehreren Zeilen, abwechselnde Meldungen),
    // werden eine Zeile, wie eine Serie gleicher Zeilen
    let mut pending: Vec<(usize, Option<String>)> = Vec::new();
    let flush = |pending: &mut Vec<(usize, Option<String>)>, out: &mut String, kept: &mut usize| {
        if pending.is_empty() {
            return;
        }
        let last = pending.iter().filter_map(|(_, t)| t.as_deref()).max().map(|t| format!(", last at {t}")).unwrap_or_default();
        let (lo, hi) = (pending.iter().map(|p| p.0).min().unwrap_or(0), pending.iter().map(|p| p.0).max().unwrap_or(0));
        let line = match pending.len() {
            1 => format!("… {hi}× more of the message above omitted (similar lines{last})"),
            k if lo == hi => format!("… {hi}× more of the {k} messages above omitted (similar lines{last})"),
            k => format!("… {lo} to {hi}× more of the {k} messages above omitted (similar lines{last})"),
        };
        push(&line, out);
        *kept += 1;
        pending.clear();
    };
    for (ri, r) in runs.iter().enumerate() {
        if let Some((total, ts)) = omitted_summary.get(&ri) {
            pending.push((*total, ts.clone()));
            continue;
        }
        if omitted.contains_key(&ri) {
            continue;
        }
        flush(&mut pending, &mut out, &mut kept);
        if r.count >= MIN_RUN {
            push(lines[r.first], &mut out);
            push(&run_marker(&lines, r), &mut out);
            kept += 2;
        } else {
            for line in &lines[r.first..=r.last] {
                push(line, &mut out);
                kept += 1;
            }
        }
    }
    flush(&mut pending, &mut out, &mut kept);
    if !text.ends_with('\n') {
        out.pop();
    }
    Collapsed { text: out, saved_lines: lines.len().saturating_sub(kept) }
}

/// Ergebnis von [`cut_to_bytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    pub text: String,
    /// Wie viele Zeilen fehlen (vorn bei [`Keep::Tail`], hinten bei [`Keep::Head`]).
    pub omitted_lines: usize,
}

/// Kürzt `text` auf höchstens `max` Bytes, in ganzen Zeilen: mit [`Keep::Tail`] fehlen die ersten Zeilen (das Ende ist
/// wichtiger), mit [`Keep::Head`] die letzten. Eine Zeile am Anfang bzw. Ende sagt, wie viele fehlen; sie zählt zur
/// Grenze.
pub fn cut_to_bytes(text: &str, max: usize, keep: Keep) -> Cut {
    if text.len() <= max {
        return Cut { text: text.to_string(), omitted_lines: 0 };
    }
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    // Platz für die Meldung („[… 123456 earlier lines omitted]\n“)
    let reserve = 48usize.min(max / 2);
    let budget = max - reserve;
    let mut used = 0usize;
    let mut taken: Vec<&str> = Vec::new();
    let iter: Box<dyn Iterator<Item = &&str>> = match keep {
        Keep::Tail => Box::new(lines.iter().rev()),
        Keep::Head => Box::new(lines.iter()),
    };
    for line in iter {
        if used + line.len() + 1 > budget {
            break;
        }
        used += line.len() + 1;
        taken.push(line);
    }
    if taken.is_empty() && !lines.is_empty() {
        // Selbst eine Zeile passt nicht: an der Zeichengrenze kürzen
        let line = if keep == Keep::Tail { lines[total - 1] } else { lines[0] };
        let piece = if keep == Keep::Tail { &line[safe_tail_start(line, budget)..] } else { &line[..safe_head(line, budget)] };
        taken.push(piece);
    }
    let omitted = total - taken.len();
    let mut out = String::with_capacity(max);
    match keep {
        Keep::Tail => {
            out.push_str(&format!("[… {omitted} earlier lines omitted]\n"));
            for line in taken.iter().rev() {
                out.push_str(line);
                out.push('\n');
            }
        }
        Keep::Head => {
            for line in &taken {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str(&format!("[… {omitted} later lines omitted]\n"));
        }
    }
    // Der Abschluss-Zeilenumbruch gehört nicht zum Inhalt
    Cut { text: out.trim_end_matches('\n').to_string(), omitted_lines: omitted }
}

/// Wie viele Bytes vom Ende einer Zeile passen in `max` (an einer Zeichengrenze).
/// Kann das Zeichen zu einem Token oder einer Zuweisung `schlüssel=wert` gehören? Dieselben Zeichen wie die Läufe des
/// Geheimnisfilters (`secrets`: Base64, Hex, `name=wert`, URL-Teile).
fn token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'~' | b'+' | b'/' | b'=' | b'-' | b':' | b'%')
}

/// Schnittstelle für den Anfang von `s` höchstens bei `max`, aber nie mitten in einem Lauf aus [`token_char`]: dann vor
/// dem Lauf (Security-Review N1). Ein angeschnittenes Token fiele unter die Schwellen des Filters, ein abgeschnittener
/// Schlüssel verriete seinen Wert nicht mehr.
pub fn safe_head(s: &str, max: usize) -> usize {
    let b = s.as_bytes();
    let mut cut = floor_boundary(s, max);
    if cut < b.len() && token_char(b[cut]) {
        while cut > 0 && token_char(b[cut - 1]) {
            cut -= 1;
        }
    }
    cut
}

/// Anfang des Endes von `s` mit höchstens `max` Bytes, nie mitten in einem Lauf aus [`token_char`]: dann hinter dem Lauf.
pub fn safe_tail_start(s: &str, max: usize) -> usize {
    let b = s.as_bytes();
    let mut start = s.len() - floor_tail(s, max);
    if start > 0 && token_char(b[start - 1]) {
        while start < b.len() && token_char(b[start]) {
            start += 1;
        }
    }
    start
}

/// Ohne die angeschnittene erste Zeile, wenn vorn etwas fehlt (`truncated` bei einem Lesen vom Ende): ein Wert ohne
/// seinen Schlüssel ist für den Filter nicht mehr als Geheimnis zu erkennen. Ohne Zeilenende bleibt alles.
pub fn without_partial_first_line(bytes: &[u8], truncated: bool) -> &[u8] {
    match bytes.iter().position(|&c| c == b'\n') {
        Some(nl) if truncated => &bytes[nl + 1..],
        _ => bytes,
    }
}

/// Ohne die angeschnittene letzte Zeile, wenn hinten etwas fehlt (`truncated` bei einem Lesen vom Anfang).
pub fn without_partial_last_line(bytes: &[u8], truncated: bool) -> &[u8] {
    match bytes.iter().rposition(|&c| c == b'\n') {
        Some(nl) if truncated => &bytes[..nl + 1],
        _ => bytes,
    }
}

fn floor_tail(line: &str, max: usize) -> usize {
    let mut n = max.min(line.len());
    while !line.is_char_boundary(line.len() - n) {
        n -= 1;
    }
    n
}

/// Letzte `n` Zeilen eines Textes (die Zeilen bleiben unverändert).
pub fn last_lines(text: &str, n: usize) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// Bytes als `12 B`, `3.4 KiB`, `1.2 MiB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(n: u32) -> String {
        format!("2026-11-04T09:{:02}:{:02}+0100", n / 60, n % 60)
    }

    fn journal(n: u32, ident: &str, msg: &str) -> String {
        format!("{} blub {ident}: {msg}", ts(n))
    }

    #[test]
    fn clean_strips_escapes_controls_and_bidi_tricks() {
        let raw = "plain\r\nred \u{1b}[31mtext\u{1b}[0m\rnext\u{7}\0line\u{202e}evil \u{1b}]0;title\u{7}ok\u{1b}c\n".as_bytes();
        assert_eq!(clean(raw), "plain\nred text\nnextlineevil ok\n");
        assert_eq!(clean(b"bad \xff\xfe utf8"), "bad \u{fffd}\u{fffd} utf8");
        assert_eq!(clean(b""), "");
        assert_eq!(clean("tab\there".as_bytes()), "tab\there");
    }

    #[test]
    fn clean_cuts_huge_lines_at_a_char_boundary() {
        let big = format!("{}\nnext", "ä".repeat(5000));
        let out = clean(big.as_bytes());
        let first = out.lines().next().unwrap();
        assert!(first.len() < MAX_LINE + 40, "{}", first.len());
        assert!(first.contains("…[+"), "{first}");
        assert_eq!(out.lines().nth(1), Some("next"));
        // Gültiges UTF-8 trotz Schnitt in der Mitte eines Umlauts
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn masking_and_message_extraction() {
        assert_eq!(mask_numbers("segfault at 7f12ab ip 00007f9a1234 sp 7ffe in libc.so.6[7f12+28000]"), "segfault at # ip # sp # in libc.so.#[#+#]");
        assert_eq!(mask_numbers("eth0 link up 1000 Mbps"), "eth0 link up # Mbps");
        assert_eq!(mask_numbers("cafe deadbeef 0x1f abc12 face dead2 ab"), "cafe # # # face # ab");
        assert_eq!(drop_pid("dockerd[1234]:"), "dockerd:");
        assert_eq!(drop_pid("kernel:"), "kernel:");
        assert_eq!(drop_pid("a[b]:"), "a[b]:");
        let line = journal(5, "dockerd[1234]", "container exited");
        assert_eq!(message_of(&line, LineFormat::Journal), "dockerd: container exited");
        assert_eq!(message_of("2026-11-04T08:47:12.123456789Z hello world", LineFormat::Timestamped), "hello world");
        assert_eq!(message_of("no timestamp here", LineFormat::Timestamped), "no timestamp here");
        assert_eq!(message_of("x y", LineFormat::Plain), "x y");
    }

    #[test]
    fn a_series_of_identical_lines_becomes_one_line_and_a_count() {
        let mut text = String::new();
        text.push_str(&journal(1, "kernel", "start"));
        text.push('\n');
        for i in 0..76 {
            // PID und Zeit wechseln, die Meldung nicht
            text.push_str(&journal(10 + i, &format!("worker[{}]", 4000 + i), "out of cheese"));
            text.push('\n');
        }
        text.push_str(&journal(200, "kernel", "end"));
        let c = collapse_repeats(&text, LineFormat::Journal);
        let lines: Vec<&str> = c.text.lines().collect();
        assert_eq!(lines.len(), 4, "{}", c.text);
        assert!(lines[1].contains("worker[4000]: out of cheese"));
        assert!(lines[2].starts_with("… repeated 76×"), "{}", lines[2]);
        assert!(lines[2].contains("last at 2026-11-04T09:01:25+0100"), "{}", lines[2]);
        assert!(!lines[2].contains("numbers differ"));
        assert_eq!(c.saved_lines, 76 + 2 - 4);
        assert!(lines[3].ends_with("end"));
    }

    #[test]
    fn similar_lines_with_different_numbers_are_collapsed_and_say_so() {
        let mut text = String::new();
        for i in 0..40u32 {
            text.push_str(&journal(
                i,
                "kernel",
                &format!("node[{}]: segfault at {:x} ip 00007f{:06x} error 4 in libc.so.6", 1000 + i, 0x7f00 + i * 3, 0x10000 + i * 17),
            ));
            text.push('\n');
        }
        let c = collapse_repeats(&text, LineFormat::Journal);
        let lines: Vec<&str> = c.text.lines().collect();
        assert_eq!(lines.len(), 2, "{}", c.text);
        assert!(lines[1].starts_with("… repeated 40×"));
        assert!(lines[1].contains("numbers differ"));
    }

    #[test]
    fn short_runs_and_distinct_lines_are_untouched() {
        let mut text = String::new();
        for i in 0..10u32 {
            text.push_str(&journal(i, "svc", &format!("message number {}", ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"][i as usize])));
            text.push('\n');
        }
        // Zwei gleiche hintereinander bleiben
        text.push_str(&journal(20, "svc", "twice"));
        text.push('\n');
        text.push_str(&journal(21, "svc", "twice"));
        text.push('\n');
        let c = collapse_repeats(&text, LineFormat::Journal);
        assert_eq!(c.text, text);
        assert_eq!(c.saved_lines, 0);
        // Wenige Zeilen, leerer Text
        assert_eq!(collapse_repeats("", LineFormat::Plain).text, "");
        assert_eq!(collapse_repeats("a\nb", LineFormat::Plain).text, "a\nb");
        // Leere Zeilen werden nicht als Serie gezählt
        let blank = "x\n\n\n\n\ny";
        assert_eq!(collapse_repeats(blank, LineFormat::Plain).text, blank);
    }

    /// Messschritt Diagnose (Aufgabe 10b): ein Absturz mit Backtrace (Block aus neun Zeilen) wiederholt sich elfmal; statt
    /// neun gleicher Sammelzeilen untereinander steht eine.
    #[test]
    fn a_repeated_block_gets_one_summary_line() {
        let block = [
            "worker 2.4.0 starting (pid 1)",
            "loading config /etc/worker/worker.toml",
            "warning: unknown key 'storage.path' in /etc/worker/worker.toml (ignored)",
            "queue: connected to redis://queue:6379/0",
            "storage: opening backend",
            "*** SIGSEGV (address 0x0) received by PID 1",
            "#0  0x00007f3a1c2b8f3d in strlen () from /lib/x86_64-linux-gnu/libc.so.6",
            "#1  0x000055d0c1e4a2b1 in storage_open (cfg=0x55d0c3a0b2a0) at src/storage.c:88",
            "#2  0x000055d0c1e41a07 in main (argc=1, argv=0x7ffd2b6e1f58) at src/main.c:142",
        ];
        let mut text = String::new();
        for rep in 0..11u32 {
            for (i, l) in block.iter().enumerate() {
                text.push_str(&format!("2026-11-05T09:{:02}:{:02}.104118000Z {l}\n", 58 + rep / 6, (rep * 7 + i as u32 / 5) % 60));
            }
        }
        let c = collapse_repeats(&text, LineFormat::Timestamped);
        let lines: Vec<&str> = c.text.lines().collect();
        let summaries: Vec<&&str> = lines.iter().filter(|l| l.contains("× more of the")).collect();
        assert_eq!(summaries.len(), 1, "{}", c.text);
        assert!(summaries[0].starts_with("… 8× more of the 9 messages above omitted (similar lines, last at 2026-11-05T09:59:"), "{}", summaries[0]);
        // zwei Blöcke vorn, die Sammelzeile, der letzte Block
        assert_eq!(lines.len(), 9 * 3 + 1, "{}", c.text);
        assert_eq!(c.saved_lines, 99 - 28);
        // verschiedene Zahlen je Meldung: die erste Zeile kommt noch einmal allein
        let uneven = format!("{text}2026-11-05T10:00:00.000000000Z {}\n", block[0]);
        let c = collapse_repeats(&uneven, LineFormat::Timestamped);
        assert!(c.text.contains("… 8 to 9× more of the 9 messages above omitted (similar lines, last at 2026-11-05T09:59:"), "{}", c.text);
        assert_eq!(c.text.lines().filter(|l| l.contains("× more of the")).count(), 1, "{}", c.text);
    }

    #[test]
    fn interleaved_repeats_keep_the_first_two_and_the_last() {
        let mut text = String::new();
        for i in 0..20u32 {
            text.push_str(&journal(i * 2, "kernel", &format!("oom killed process {} total-vm", 100 + i)));
            text.push('\n');
            text.push_str(&journal(i * 2 + 1, "dockerd", &format!("container {i} restarted after crash {}", i * 7)));
            text.push('\n');
        }
        let c = collapse_repeats(&text, LineFormat::Journal);
        let lines: Vec<&str> = c.text.lines().collect();
        // Je Meldung: zwei vorn, die Sammelzeile, die letzte
        assert_eq!(lines.iter().filter(|l| l.contains("oom killed")).count(), 3, "{}", c.text);
        assert_eq!(lines.iter().filter(|l| l.contains("restarted after")).count(), 3, "{}", c.text);
        // die beiden Sammelzeilen stünden untereinander und sind eine
        assert_eq!(lines.iter().filter(|l| l.contains("× more of the")).count(), 1, "{}", c.text);
        assert!(c.text.contains("… 17× more of the 2 messages above omitted"), "{}", c.text);
        // Die letzte Zeile jeder Meldung ist erhalten
        assert!(c.text.contains("oom killed process 119"));
        assert!(c.text.contains("container 19 restarted"));
        // Die Reihenfolge der erhaltenen Zeilen bleibt die der Eingabe
        let times: Vec<&str> = lines.iter().filter_map(|l| timestamp_of(l)).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn timestamped_format_ignores_the_leading_time() {
        let text: String = (0..5).map(|i| format!("2026-11-04T08:47:0{i}.000000000Z GET /health 200\n")).collect();
        let c = collapse_repeats(&text, LineFormat::Timestamped);
        assert_eq!(c.text.lines().count(), 2);
        assert!(c.text.contains("repeated 5×"));
        assert!(c.text.contains("last at 2026-11-04T08:47:04.000000000Z"));
    }

    #[test]
    fn cut_keeps_the_end_or_the_start_in_whole_lines() {
        let text: String = (0..1000).map(|i| format!("line {i:04}\n")).collect();
        let tail = cut_to_bytes(&text, 500, Keep::Tail);
        assert!(tail.text.len() <= 500, "{}", tail.text.len());
        assert!(tail.text.starts_with("[… "));
        assert!(tail.text.contains("earlier lines omitted]"));
        assert!(tail.text.ends_with("line 0999"));
        assert!(tail.omitted_lines > 900 && tail.omitted_lines < 1000);
        assert_eq!(tail.text.lines().count() + tail.omitted_lines - 1, 1000);
        let head = cut_to_bytes(&text, 500, Keep::Head);
        assert!(head.text.len() <= 500);
        assert!(head.text.starts_with("line 0000"));
        assert!(head.text.ends_with("later lines omitted]"));
        // Passt alles, bleibt es
        assert_eq!(cut_to_bytes("a\nb", 100, Keep::Tail), Cut { text: "a\nb".into(), omitted_lines: 0 });
        // Eine einzige zu lange Zeile wird an der Zeichengrenze gekürzt
        let one = "ä".repeat(400);
        for keep in [Keep::Head, Keep::Tail] {
            let c = cut_to_bytes(&one, 100, keep);
            assert!(c.text.len() <= 100, "{}", c.text.len());
            assert!(std::str::from_utf8(c.text.as_bytes()).is_ok());
        }
    }

    #[test]
    fn small_helpers() {
        assert_eq!(last_lines("a\nb\nc\nd", 2), ["c", "d"]);
        assert_eq!(last_lines("a", 5), ["a"]);
        assert_eq!(human_bytes(12), "12 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    /// Security-Review N1: Schnitte trennen nie Schlüssel und Wert und lassen kein halbes Token stehen.
    #[test]
    fn cuts_never_split_a_token_or_an_assignment() {
        let line = format!("{} api_token=3f9a0c1d2e3f4a5b6c7d8e9f0a1b2c3d tail", "x ".repeat(10));
        // vorn: vor dem Lauf `api_token=…`, nicht mitten darin
        let head = safe_head(&line, 30);
        assert_eq!(&line[..head], "x x x x x x x x x x  ");
        // hinten: hinter dem Lauf
        let start = safe_tail_start(&line, 20);
        assert_eq!(&line[start..], " tail");
        assert_eq!(safe_head("abc def", 3), 3);
        assert_eq!(safe_head("abcdef", 3), 0);
        assert_eq!(safe_tail_start("abc def", 3), 4);
        // eine zu lange Zeile in `cap_lines` endet vor dem Lauf
        let long = format!("{} key={}", "w ".repeat(MAX_LINE / 2 - 1), "a".repeat(100));
        let capped = cap_lines(&long);
        assert!(!capped.contains("key=") && capped.contains("…[+"), "{}", &capped[capped.len() - 40..]);
        // angeschnittene Zeilen fallen weg, wenn etwas fehlt
        assert_eq!(without_partial_first_line(b"oken=abc\nnext\n", true), b"next\n");
        assert_eq!(without_partial_first_line(b"oken=abc\nnext\n", false), b"oken=abc\nnext\n");
        assert_eq!(without_partial_first_line(b"no newline", true), b"no newline");
        assert_eq!(without_partial_last_line(b"one\ntwo api_tok", true), b"one\n");
        assert_eq!(without_partial_last_line(b"one\ntwo", false), b"one\ntwo");
    }
}
