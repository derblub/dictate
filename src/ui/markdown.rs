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

//! Kleine Markdown-Teilmenge für Antworten im Panel der Textaktionen (Port des früheren Awesome-Moduls `markdown.lua`,
//! PLAN-wayland.md 6d Punkt 2). Liefert Blöcke mit Inline-Stücken; das Panel baut daraus Pango-Markup
//! ([`spans_markup`]) und maskiert dabei jedes Stück selbst. Nichts wird ausgeführt, Links sind nicht klickbar (Text
//! plus Ziel in grau).
//!
//! Blöcke: Absatz, Überschrift, Listenpunkt (Ebene 0..3, Marke `•` bzw. `3.`), Zitat, Codeblock (Inhalt ohne Fence,
//! Nummer ab 1 in Textreihenfolge), Abstand (Leerzeile zwischen Absätzen).
//!
//! Code-Fences exakt wie `src/actions/markdown.rs` (`actions::markdown::code_blocks` und das Kopieren je Block im
//! Panel nummerieren gleich; der Test `code_blocks_match_the_daemon` hält das fest): Zeilen an `\n`, ein `\r` am Zeilenende
//! fällt weg, ein `\n` am Textende beginnt keine leere Zeile. Öffnen: nach höchstens 3 Leerzeichen mindestens 3
//! Backticks (Info-String ohne Backtick) oder 3 Tilden. Schließen: nach höchstens 3 Leerzeichen nur dasselbe Zeichen in
//! mindestens der öffnenden Länge, danach nur Leerraum. Offen bis Textende. Zeilenumbrüche innerhalb eines Absatzes
//! bleiben Umbrüche.
//!
//! Gearbeitet wird auf Bytes (alle Marken sind ASCII, Schnitte liegen darum immer auf Zeichengrenzen), Leerraum wie
//! `%s` in Lua.

/// Ein Inline-Stück
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub code: bool,
    pub bold: bool,
    pub italic: bool,
    pub link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Para(Vec<Span>),
    Heading(Vec<Span>),
    Item { level: usize, marker: String, spans: Vec<Span> },
    Quote(Vec<Span>),
    Code { text: String, index: usize },
    Gap,
}

impl Block {
    pub fn kind(&self) -> &'static str {
        match self {
            Block::Para(_) => "para",
            Block::Heading(_) => "heading",
            Block::Item { .. } => "item",
            Block::Quote(_) => "quote",
            Block::Code { .. } => "code",
            Block::Gap => "gap",
        }
    }
}

/// Leerraum wie `%s` in Lua
fn ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn all_ws(s: &str) -> bool {
    s.bytes().all(ws)
}

/// Zeilen an `\n`, ohne `\r` am Ende; ein `\n` am Textende beginnt keine leere Zeile
pub fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// Führende Leerzeichen (nur ` `)
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Öffnende Fence: Zeichen und Länge
fn fence_open(line: &str) -> Option<(u8, usize)> {
    let ind = indent(line);
    if ind > 3 {
        return None;
    }
    let rest = &line.as_bytes()[ind..];
    let ch = *rest.first().filter(|c| **c == b'`' || **c == b'~')?;
    let n = rest.iter().take_while(|c| **c == ch).count();
    if n < 3 {
        return None;
    }
    if ch == b'`' && rest[n..].contains(&b'`') {
        return None;
    }
    Some((ch, n))
}

fn fence_close(line: &str, ch: u8, n: usize) -> bool {
    let ind = indent(line);
    if ind > 3 {
        return false;
    }
    let rest = &line.as_bytes()[ind..];
    let run = rest.iter().take_while(|c| **c == ch).count();
    run >= n && run > 0 && rest[run..].iter().all(|b| ws(*b))
}

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
}

/// Inline: `code`, **fett**/__fett__, *kursiv*, [Text](Ziel), `\` vor einer Marke. Unvollständige Marken bleiben
/// wörtlich.
pub fn inline(s: &str) -> Vec<Span> {
    let mut out = Vec::new();
    inline_into(s, Style::default(), &mut out, 0);
    out
}

/// Höchste Schachtelung von Fett/Kursiv/Link; tiefer wird der Rest wörtlich übernommen (die Antwort der KI lässt sich
/// über die Markierung steuern: eine lange `*`-Folge gäbe sonst eine Rekursion je vier Zeichen, Durchsicht W3 N3)
const MAX_INLINE_DEPTH: u8 = 8;

/// Merkt sich je Muster die letzte Suche: kein Treffer in `[from, ans)` heißt, dass jede Suche ab einem Punkt darin
/// dasselbe liefert. So bleiben Ketten offener Marken (`**a **b …` ohne Ende) linear statt quadratisch.
#[derive(Default)]
struct Finder(std::collections::HashMap<(u8, usize), (usize, Option<usize>)>);

impl Finder {
    fn next(&mut self, key: (u8, usize), from: usize, search: impl FnOnce() -> Option<usize>) -> Option<usize> {
        if let Some(&(f, ans)) = self.0.get(&key) {
            if from >= f && ans.is_none_or(|a| from <= a) {
                return ans;
            }
        }
        let ans = search();
        self.0.insert(key, (from, ans));
        ans
    }
}

fn find(s: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if from > s.len() || pat.is_empty() {
        return None;
    }
    s[from..].windows(pat.len()).position(|w| w == pat).map(|p| p + from)
}

fn inline_into(s: &str, style: Style, out: &mut Vec<Span>, depth: u8) {
    let b = s.as_bytes();
    let n = b.len();
    if depth >= MAX_INLINE_DEPTH {
        if !s.is_empty() {
            out.push(Span { text: s.to_string(), bold: style.bold, italic: style.italic, ..Span::default() });
        }
        return;
    }
    let mut finder = Finder::default();
    let at = |i: usize| b.get(i).copied();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Span>| {
        if !buf.is_empty() {
            out.push(Span { text: std::mem::take(buf), bold: style.bold, italic: style.italic, ..Span::default() });
        }
    };
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == b'`' {
            let run = b[i..].iter().take_while(|x| **x == b'`').count();
            let pat = &b[i..i + run];
            let mut j = finder.next((b'`', run), i + run, || find(b, pat, i + run));
            // die schließende Folge muss genau so lang sein
            while let Some(jj) = j {
                if at(jj + run) == Some(b'`') {
                    j = finder.next((b'`', run), jj + run + 1, || find(b, pat, jj + run + 1));
                } else {
                    break;
                }
            }
            if let Some(j) = j {
                flush(&mut buf, out);
                let mut code = &s[i + run..j];
                if code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ') && !code.bytes().all(|x| x == b' ') {
                    code = &code[1..code.len() - 1];
                }
                out.push(Span { text: code.to_string(), code: true, ..Span::default() });
                i = j + run;
            } else {
                buf.push_str(&s[i..i + run]);
                i += run;
            }
        } else if (c == b'*' || c == b'_') && at(i + 1) == Some(c) {
            let mark = [c, c];
            let mut j = finder.next((c, 2), i + 2, || find(b, &mark, i + 2));
            // „***“ am Ende: die letzten beiden schließen fett, das erste gehört zum inneren Kursiv
            while let Some(jj) = j {
                if at(jj + 2) == Some(c) {
                    j = Some(jj + 1);
                } else {
                    break;
                }
            }
            match j {
                Some(j) if j > i + 2 && !at(i + 2).is_some_and(ws) => {
                    flush(&mut buf, out);
                    inline_into(&s[i + 2..j], Style { bold: true, italic: style.italic }, out, depth + 1);
                    i = j + 2;
                }
                _ => {
                    buf.push_str(&s[i..i + 2]);
                    i += 2;
                }
            }
        } else if c == b'*' && at(i + 1).is_some_and(|x| !ws(x)) {
            // erstes Nicht-Leerzeichen, auf das ein `*` folgt
            let j = finder.next((b'i', 1), i + 1, || (i + 1..n.saturating_sub(1)).find(|&k| !ws(b[k]) && b[k + 1] == b'*'));
            match j {
                Some(j) if at(j + 2) != Some(b'*') => {
                    flush(&mut buf, out);
                    inline_into(&s[i + 1..=j], Style { bold: style.bold, italic: true }, out, depth + 1);
                    i = j + 2;
                }
                _ => {
                    buf.push('*');
                    i += 1;
                }
            }
        } else if c == b'[' {
            let close = finder.next((b']', 1), i + 1, || (i + 1..n).find(|&k| b[k] == b']'));
            match close.and_then(|c| link_at(s, i, c)) {
                Some((label, target, end)) => {
                    flush(&mut buf, out);
                    inline_into(label, style, out, depth + 1);
                    out.push(Span { text: format!(" ({target})"), link: true, ..Span::default() });
                    i = end;
                }
                None => {
                    buf.push('[');
                    i += 1;
                }
            }
        } else if c == b'\\' && at(i + 1).is_some_and(|x| b"*_`[]\\#".contains(&x)) {
            buf.push(b[i + 1] as char);
            i += 2;
        } else {
            let j = (i + 1..n).find(|&k| b"`*_[\\".contains(&b[k])).unwrap_or(n);
            buf.push_str(&s[i..j]);
            i = j;
        }
    }
    flush(&mut buf, out);
}

/// `[Text](Ziel)` ab `i` mit dem nächsten `]` bei `close`: Text ohne `]` (mindestens ein Zeichen), Ziel ohne Leerraum
/// und `)` (mindestens eins)
fn link_at(s: &str, i: usize, close: usize) -> Option<(&str, &str, usize)> {
    let b = s.as_bytes();
    if close == i + 1 || b.get(close + 1) != Some(&b'(') {
        return None;
    }
    let t0 = close + 2;
    let t1 = (t0..b.len()).find(|&k| ws(b[k]) || b[k] == b')').unwrap_or(b.len());
    if t1 == t0 || b.get(t1) != Some(&b')') {
        return None;
    }
    Some((&s[i + 1..close], &s[t0..t1], t1 + 1))
}

/// Zeilenumbruch und weitere Stücke an einen offenen Block anhängen (Absatz mit harten Umbrüchen)
fn append_line(spans: &mut Vec<Span>, line: &str) {
    if !spans.is_empty() {
        spans.push(Span { text: "\n".into(), ..Span::default() });
    }
    inline_into(line, Style::default(), spans, 0);
}

/// Trennlinie: bis 3 Leerzeichen, dann mindestens drei von `-*_` (gemischt erlaubt) mit Leerzeichen dazwischen
fn is_rule(line: &str) -> bool {
    let ind = indent(line);
    if ind > 3 {
        return false;
    }
    let rest = &line.as_bytes()[ind..];
    let marks = rest.iter().filter(|c| b"-*_".contains(c)).count();
    // nach der dritten Marke dürfen nur noch Marken und Leerzeichen folgen; davor nur Leerzeichen zwischen den Marken
    rest.first().is_some_and(|c| b"-*_".contains(c)) && marks >= 3 && rest.iter().all(|c| b"-*_ ".contains(c))
}

/// Überschrift: bis 3 Leerzeichen, 1–6 `#`, Leerraum, Titel ohne schließende `#` und Leerraum am Ende
fn heading(line: &str) -> Option<(usize, &str)> {
    let ind = indent(line);
    if ind > 3 {
        return None;
    }
    let rest = &line[ind..];
    let hashes = rest.bytes().take_while(|c| *c == b'#').count();
    if hashes == 0 || !rest.as_bytes().get(hashes).copied().is_some_and(ws) {
        return None;
    }
    let title = rest[hashes..].trim_start_matches(|c: char| c.is_ascii() && ws(c as u8));
    let title = title.trim_end_matches(|c: char| c.is_ascii() && ws(c as u8));
    let title = title.trim_end_matches('#');
    let title = title.trim_end_matches(|c: char| c.is_ascii() && ws(c as u8));
    Some((hashes, title))
}

/// Listenpunkt mit `-`, `*` oder `+`: Einrückung und Rest
fn bullet(line: &str) -> Option<(usize, &str)> {
    let ind = indent(line);
    let b = line.as_bytes();
    if !b.get(ind).is_some_and(|c| b"-*+".contains(c)) || !b.get(ind + 1).copied().is_some_and(ws) {
        return None;
    }
    Some((ind, line[ind + 1..].trim_start_matches(|c: char| c.is_ascii() && ws(c as u8))))
}

/// Nummerierter Punkt `3.` oder `3)`: Einrückung, Marke (immer mit `.`), Rest
fn numbered(line: &str) -> Option<(usize, String, &str)> {
    let ind = indent(line);
    let b = line.as_bytes();
    let digits = b[ind..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let p = ind + digits;
    if !b.get(p).is_some_and(|c| *c == b'.' || *c == b')') || !b.get(p + 1).copied().is_some_and(ws) {
        return None;
    }
    let rest = line[p + 1..].trim_start_matches(|c: char| c.is_ascii() && ws(c as u8));
    Some((ind, format!("{}.", &line[ind..p]), rest))
}

fn quote(line: &str) -> Option<&str> {
    let ind = indent(line);
    if ind > 3 || line.as_bytes().get(ind) != Some(&b'>') {
        return None;
    }
    let q = &line[ind + 1..];
    Some(q.strip_prefix(' ').unwrap_or(q))
}

/// Text in Blöcke zerlegen (siehe Modulkopf)
pub fn parse(text: &str) -> Vec<Block> {
    let lines = split_lines(text);
    let mut blocks: Vec<Block> = Vec::new();
    let mut code_index = 0;
    // offener Absatz, Zitat oder Listenpunkt (Fortsetzungszeilen): Index in `blocks`
    let mut cur: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some((ch, n)) = fence_open(line) {
            cur = None;
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !fence_close(lines[i], ch, n) {
                body.push(lines[i]);
                i += 1;
            }
            code_index += 1;
            blocks.push(Block::Code { text: body.join("\n"), index: code_index });
            i += 1; // schließende Fence (oder Ende)
            continue;
        }
        if all_ws(line) {
            cur = None;
            if blocks.last().is_some_and(|b| *b != Block::Gap) {
                blocks.push(Block::Gap);
            }
            i += 1;
            continue;
        }
        let bul = bullet(line);
        if is_rule(line) && bul.is_none() {
            // Trennlinie: als Abstand
            cur = None;
            if blocks.last().is_some_and(|b| *b != Block::Gap) {
                blocks.push(Block::Gap);
            }
        } else if let Some((_, title)) = heading(line).filter(|(h, _)| *h <= 6) {
            blocks.push(Block::Heading(inline(title)));
            cur = None;
        } else if let Some((ind, rest)) = bul {
            blocks.push(Block::Item { level: (ind / 2).min(3), marker: "•".into(), spans: inline(rest) });
            cur = Some(blocks.len() - 1);
        } else if let Some((ind, marker, rest)) = numbered(line) {
            blocks.push(Block::Item { level: (ind / 2).min(3), marker, spans: inline(rest) });
            cur = Some(blocks.len() - 1);
        } else if let Some(q) = quote(line) {
            match cur.map(|k| &mut blocks[k]) {
                Some(Block::Quote(spans)) => append_line(spans, q),
                _ => {
                    blocks.push(Block::Quote(inline(q)));
                    cur = Some(blocks.len() - 1);
                }
            }
        } else {
            let leading_ws = line.as_bytes().first().copied().is_some_and(ws);
            match cur.map(|k| &mut blocks[k]) {
                Some(Block::Para(spans)) => append_line(spans, line),
                Some(Block::Item { spans, .. }) if leading_ws => append_line(spans, line.trim_start_matches(|c: char| c.is_ascii() && ws(c as u8))),
                _ => {
                    blocks.push(Block::Para(inline(line)));
                    cur = Some(blocks.len() - 1);
                }
            }
        }
        i += 1;
    }
    if blocks.last() == Some(&Block::Gap) {
        blocks.pop();
    }
    blocks
}

/// Inhalte der Codeblöcke in Reihenfolge
pub fn code_blocks(text: &str) -> Vec<String> {
    parse(text)
        .into_iter()
        .filter_map(|b| match b {
            Block::Code { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// Pango-Markup maskieren (`&`, `<`, `>`, Anführungszeichen); Steuerzeichen außer Zeilenumbruch und Tab fallen weg
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Farben und Schriften für das Markup (Hex-Farben `#rrggbb`, Schriftnamen für `font_desc`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// Nebentext (Links, Nummern)
    pub tail: String,
    /// Hintergrund von Inline-Code
    pub code_bg: String,
    /// Schrift von Inline-Code und Codeblöcken
    pub mono: String,
    /// Gelöschtes im Wortdiff (Strich)
    pub error: String,
    /// Eingefügtes im Wortdiff (Hinterlegung)
    pub accent: String,
}

/// Markup einer Folge von Stücken (jedes Stück einzeln maskiert)
pub fn spans_markup(spans: &[Span], look: &Look) -> String {
    let mut out = String::new();
    for s in spans {
        let t = esc(&s.text);
        if s.code {
            out.push_str(&format!("<span font_family=\"{}\" background=\"{}\">\u{2009}{t}\u{2009}</span>", esc(&look.mono), look.code_bg));
        } else if s.link {
            out.push_str(&format!("<span foreground=\"{}\">{t}</span>", look.tail));
        } else {
            let mut t = t;
            if s.bold {
                t = format!("<b>{t}</b>");
            }
            if s.italic {
                t = format!("<i>{t}</i>");
            }
            out.push_str(&t);
        }
    }
    out
}

/// Ein Stück des Wortdiffs vom Daemon (`"diff":[["=","…"],…]`): `=` unverändert, `-` gelöscht, `+` eingefügt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPiece {
    pub op: char,
    pub text: String,
}

/// Markup der Ansicht „changes marked“: Gelöschtes durchgestrichen, Eingefügtes hinterlegt. Ein gelöschtes Stück ohne
/// Leerraum am Ende grenzt unmittelbar an das eingefügte („Testversuch“); nur die Anzeige setzt dazwischen ein
/// unmarkiertes Leerzeichen. Der saubere Text (Replace, Copy) bleibt unverändert.
pub fn marked_markup(diff: &[DiffPiece], look: &Look) -> String {
    let mut out = String::new();
    for (i, d) in diff.iter().enumerate() {
        let t = esc(&d.text);
        match d.op {
            '-' => out.push_str(&format!("<span strikethrough=\"true\" strikethrough_color=\"{}\" foreground=\"{}\">{t}</span>", look.error, look.tail)),
            '+' => {
                let prev = i.checked_sub(1).map(|k| &diff[k]);
                let sep = prev.is_some_and(|p| p.op == '-' && !p.text.ends_with(char::is_whitespace) && !d.text.starts_with(char::is_whitespace));
                if sep {
                    out.push(' ');
                }
                out.push_str(&format!("<span background=\"{}\" bgalpha=\"16%\">{t}</span>", look.accent));
            }
            _ => out.push_str(&t),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fälle aus dem früheren `markdown_test.lua` des Awesome-Moduls (gleiche Nummerierung wie `src/actions/markdown.rs`)
    const FENCE_CASES: &[(&str, &[&str])] = &[
        ("Text\n```\na\nb\n```\nmehr\n~~~\nc\n~~~\n", &["a\nb", "c"]),
        ("kein Code, nur `inline` und ``zwei``", &[]),
        ("", &[]),
        ("```rust title=\"x\"\nfn main() {}\n```", &["fn main() {}"]),
        ("```a`b\nx", &[]),
        ("```a`b\n```\nx\n```", &["x"]),
        ("~~~ a`b\nx\n~~~", &["x"]),
        ("````\n```\ninnen\n`````  \t\nnach", &["```\ninnen"]),
        ("````\na\n```\n~~~~\n```` x\nb", &["a\n```\n~~~~\n```` x\nb"]),
        ("```\nnie zu\n", &["nie zu"]),
        ("```", &[""]),
        ("```\n```", &[""]),
        ("```\n\n```", &[""]),
        ("```\n\n\n```", &["\n"]),
        ("   ```\n  x\n   ```\n", &["  x"]),
        (" ~~~\ny\n  ~~~", &["y"]),
        ("    ```\n    code\n    ```", &[]),
        ("```\na\n    ```\nb\n```", &["a\n    ```\nb"]),
        ("\t```\nx\n\t```", &[]),
        ("Ä\r\n```sh\r\necho ü\r\nls\r\n```\r\n", &["echo ü\nls"]),
        ("```\r\na\r\n```\r\n```\r\nb\r\n```", &["a", "b"]),
    ];

    #[test]
    fn code_fences_like_the_lua_tests() {
        for (text, want) in FENCE_CASES {
            assert_eq!(code_blocks(text), want.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{text:?}");
        }
    }

    /// Das Kopieren je Block (Panel) und `act copy --block N` (Daemon-Seite) müssen gleich nummerieren
    #[test]
    fn code_blocks_match_the_daemon() {
        let extra = [
            "Antwort:\n```sh\nls -l\n```\nund\n~~~\ncat x\n~~~\n```\noffen",
            "- a\n  ```\n  b\n  ```\n> ```\n> zitat\n",
            "``` \n```\n````\n`````\n",
            "x\n```\n\n```\n\n```py\n1\n```",
        ];
        for text in FENCE_CASES.iter().map(|(t, _)| *t).chain(extra) {
            assert_eq!(code_blocks(text), crate::actions::markdown::code_blocks(text), "{text:?}");
        }
    }

    fn kinds(t: &str) -> Vec<String> {
        parse(t)
            .iter()
            .map(|b| match b {
                Block::Item { marker, .. } => format!("item:{marker}"),
                b => b.kind().to_string(),
            })
            .collect()
    }

    fn spans(t: &str) -> Vec<String> {
        inline(t)
            .iter()
            .map(|s| {
                let p = if s.code {
                    "c:"
                } else if s.bold && s.italic {
                    "bi:"
                } else if s.bold {
                    "b:"
                } else if s.italic {
                    "i:"
                } else if s.link {
                    "l:"
                } else {
                    ""
                };
                format!("{p}{}", s.text)
            })
            .collect()
    }

    #[test]
    fn blocks_like_the_lua_tests() {
        assert_eq!(
            kinds("# Title\nText\nmore\n\n- a\n- b\n1. one\n\n> q\n```\nx\n```"),
            ["heading", "para", "gap", "item:•", "item:•", "item:1.", "gap", "quote", "code"]
        );
    }

    #[test]
    fn inline_like_the_lua_tests() {
        assert_eq!(spans("Use `xclip` now"), ["Use ", "c:xclip", " now"]);
        assert_eq!(spans("``a`b``"), ["c:a`b"]);
        assert_eq!(spans("a **b** c"), ["a ", "b:b", " c"]);
        assert_eq!(spans("a *b c* d"), ["a ", "i:b c", " d"]);
        assert_eq!(spans("**a _b_ *c***"), ["b:a _b_ ", "bi:c"]);
        assert_eq!(spans("2 * 3 and **x"), ["2 * 3 and **x"]);
        assert_eq!(spans("my_var_name"), ["my_var_name"]);
        assert_eq!(spans("see [docs](https://x.y)"), ["see ", "docs", "l: (https://x.y)"]);
        assert_eq!(spans("<b>&"), ["<b>&"]);
    }

    #[test]
    fn more_blocks() {
        // Absatz mit hartem Umbruch, Fortsetzung eines Listenpunkts nur eingerückt
        let b = parse("eins\nzwei\n- punkt\n  weiter\nneu");
        assert_eq!(b.len(), 3, "{b:?}");
        assert_eq!(spans_text(&b[0]), "eins\nzwei");
        assert_eq!(spans_text(&b[1]), "punkt\nweiter");
        assert_eq!(b[2], Block::Para(inline("neu")));
        // Ebenen, Nummern mit `)`, Überschrift mit schließenden `#`, Trennlinie als Abstand
        assert_eq!(kinds("  - b\n2) z\n## H ##\n---\nx"), ["item:•", "item:2.", "heading", "gap", "para"]);
        assert!(matches!(&parse("    - tief")[0], Block::Item { level: 2, .. }));
        assert!(matches!(&parse("        - sehr tief")[0], Block::Item { level: 3, .. }));
        assert_eq!(parse("## H ##")[0], Block::Heading(inline("H")));
        // `- - -` ist ein Listenpunkt (wie in Lua), `#hashtag` und `####### x` sind Absätze
        assert_eq!(kinds("- - -"), ["item:•"]);
        assert_eq!(kinds("#hashtag"), ["para"]);
        assert_eq!(kinds("####### x"), ["para"]);
        // Zitat über mehrere Zeilen, Abstand am Ende entfällt
        let q = parse("> a\n>b\n\n");
        assert_eq!(q.len(), 1);
        assert_eq!(spans_text(&q[0]), "a\nb");
        // Escape
        assert_eq!(spans("\\*nicht kursiv\\*"), ["*nicht kursiv*"]);
        // Umlaute an Marken
        assert_eq!(spans("**Grüße** *ä*"), ["b:Grüße", " ", "i:ä"]);
        assert_eq!(spans("[x](a b)"), ["[x](a b)"]);
        assert_eq!(spans("` a `"), ["c:a"]);
        assert_eq!(spans("`  `"), ["c:  "]);
        assert!(parse("").is_empty());
        assert!(parse("\n\n").is_empty());
    }

    fn spans_text(b: &Block) -> String {
        match b {
            Block::Para(s) | Block::Heading(s) | Block::Quote(s) | Block::Item { spans: s, .. } => s.iter().map(|x| x.text.as_str()).collect(),
            _ => String::new(),
        }
    }

    fn look() -> Look {
        Look { tail: "#999999".into(), code_bg: "#222222".into(), mono: "Mono".into(), error: "#ff0000".into(), accent: "#00ff00".into() }
    }

    #[test]
    fn markup_escapes_every_piece() {
        let m = spans_markup(&inline("<i>**&b**</i> `<x>` [<a>](u)"), &look());
        assert!(!m.contains("<i>**"), "{m}");
        assert!(m.starts_with("&lt;i&gt;<b>&amp;b</b>&lt;/i&gt; "), "{m}");
        assert!(m.contains("\u{2009}&lt;x&gt;\u{2009}</span>"), "{m}");
        assert!(m.contains("&lt;a&gt;<span foreground=\"#999999\"> (u)</span>"), "{m}");
        assert_eq!(esc("a\u{1b}[0m\"'\n"), "a[0m&quot;&#39;\n");
    }

    #[test]
    fn diff_markup() {
        let d = |op, t: &str| DiffPiece { op, text: t.into() };
        let m = marked_markup(&[d('=', "Ein "), d('-', "Test"), d('+', "Versuch"), d('=', " <ok>")], &look());
        assert!(m.starts_with("Ein <span strikethrough"), "{m}");
        assert!(m.contains(">Test</span> <span background"), "{m}");
        assert!(m.ends_with(" &lt;ok&gt;"), "{m}");
        // mit Leerraum am Rand kein zusätzliches Leerzeichen
        let m = marked_markup(&[d('-', "a "), d('+', "b")], &look());
        assert!(m.contains("a </span><span background"), "{m}");
    }

    /// Antworten der KI lassen sich über die Markierung steuern: lange Ketten von Marken dürfen weder den Stack
    /// sprengen noch quadratisch dauern (Durchsicht W3 N3)
    #[test]
    fn hostile_mark_runs_stay_flat_and_fast() {
        let inputs = [
            "*".repeat(100_000),
            "_".repeat(100_000),
            "`".repeat(50_000) + "x",
            "**a ".repeat(25_000),
            "*a ".repeat(30_000),
            "`a ".repeat(30_000),
            "[a ".repeat(30_000),
            "[".repeat(50_000) + "]",
            "**".repeat(20_000) + &"x".repeat(10) + &"**".repeat(20_000),
        ];
        for s in &inputs {
            let t = std::time::Instant::now();
            let spans = inline(s);
            let took = t.elapsed();
            assert!(took < std::time::Duration::from_millis(500), "{} bytes took {took:?}", s.len());
            // nichts geht verloren (Marken bleiben wörtlich oder werden zu Stil)
            let text: usize = spans.iter().map(|sp| sp.text.len()).sum();
            assert!(text > 0 && text <= s.len(), "{} -> {text}", s.len());
        }
        // an der Grenze: der Rest wörtlich, mit dem Stil außen
        let mut out = Vec::new();
        inline_into("**a** *b*", Style { bold: true, italic: false }, &mut out, MAX_INLINE_DEPTH);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].text.as_str(), out[0].bold, out[0].italic), ("**a** *b*", true, false));
        let mut out = Vec::new();
        inline_into("**a**", Style::default(), &mut out, MAX_INLINE_DEPTH - 1);
        assert_eq!((out[0].text.as_str(), out[0].bold), ("a", true));
    }
}
