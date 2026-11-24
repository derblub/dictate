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

//! Codeblöcke in einer Antwort: `fenced_ranges` nutzt `diagnose::restore`; `code_blocks` diente bis W7
//! `dictate act copy --block N` und ist heute der Maßstab des Panels. Das Panel von `dictate-ui` nummeriert die Blöcke nach denselben
//! Regeln (`src/ui/markdown.rs`, Test `code_blocks_match_the_daemon`); beide müssen gleich bleiben.
//!
//! Regeln (umzäunte Blöcke wie in CommonMark, vereinfacht):
//! - Zeilen sind durch `\n` getrennt, ein `\r` am Zeilenende wird entfernt; ein `\n` am Textende beendet die letzte
//!   Zeile und beginnt keine leere neue.
//! - Eine Zeile **öffnet** einen Block, wenn sie nach höchstens 3 Leerzeichen mit mindestens 3 Backticks oder mindestens
//!   3 Tilden beginnt. Der Rest ist der Info-String (ignoriert); bei Backticks darf er keinen Backtick enthalten, sonst
//!   ist die Zeile keine Fence.
//! - Eine Zeile **schließt** den Block, wenn sie nach höchstens 3 Leerzeichen nur aus demselben Zeichen in mindestens
//!   der öffnenden Länge besteht, danach nur Leerraum.
//! - Ein nicht geschlossener Block reicht bis zum Textende. Inhalt: die Zeilen dazwischen, mit `\n` verbunden, ohne
//!   `\n` am Ende. Eingerückter Code ohne Fence zählt nicht.
//!
//! Std-only: `dictate-settings` bindet `actions/` per Pfad ein.

/// Fence am Zeilenanfang: Zeichen, Länge, Rest der Zeile
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = rest.len() - rest.trim_start_matches(c).len();
    (len >= 3).then_some((c, len, &rest[len..]))
}

/// Inhalte aller umzäunten Codeblöcke in Textreihenfolge (siehe Modulkopf).
pub fn code_blocks(text: &str) -> Vec<String> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut out = Vec::new();
    // offener Block: Zeichen, Länge der öffnenden Fence, Zeilen
    let mut open: Option<(char, usize, Vec<&str>)> = None;
    for raw in body.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match &mut open {
            None => {
                if let Some((c, len, info)) = fence(line) {
                    if !(c == '`' && info.contains('`')) {
                        open = Some((c, len, Vec::new()));
                    }
                }
            }
            Some((c, len, lines)) => match fence(line) {
                Some((c2, len2, rest)) if c2 == *c && len2 >= *len && rest.trim().is_empty() => {
                    out.push(lines.join("\n"));
                    open = None;
                }
                _ => lines.push(line),
            },
        }
    }
    if let Some((_, _, lines)) = open {
        out.push(lines.join("\n"));
    }
    out
}

/// Byte-Bereiche aller umzäunten Codeblöcke samt ihrer Fence-Zeilen (gleiche Regeln wie [`code_blocks`]): von der
/// öffnenden Fence bis zum Ende der schließenden Zeile, ein offener Block bis zum Textende. Für die Diagnose, die
/// Platzhalter nie in Codeblöcken zurückübersetzt (`diagnose::restore`).
pub fn fenced_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    // offener Block: Zeichen, Länge der öffnenden Fence, Anfang
    let mut open: Option<(char, usize, usize)> = None;
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let start = at;
        at += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        match open {
            None => {
                if let Some((c, len, info)) = fence(line) {
                    if !(c == '`' && info.contains('`')) {
                        open = Some((c, len, start));
                    }
                }
            }
            Some((c, len, from)) => {
                if let Some((c2, len2, rest)) = fence(line) {
                    if c2 == c && len2 >= len && rest.trim().is_empty() {
                        out.push((from, at));
                        open = None;
                    }
                }
            }
        }
    }
    if let Some((_, _, from)) = open {
        out.push((from, text.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_ranges_cover_the_same_blocks() {
        let t = "Text\n```sh\na\nb\n```\nmehr `x`\n~~~\nc\n~~~\nend\n```\nopen";
        let r = fenced_ranges(t);
        assert_eq!(r.iter().map(|&(a, b)| &t[a..b]).collect::<Vec<_>>(), ["```sh\na\nb\n```\n", "~~~\nc\n~~~\n", "```\nopen"]);
        assert_eq!(r.len(), code_blocks(t).len());
        assert!(fenced_ranges("kein Code, nur `inline`").is_empty());
        assert!(fenced_ranges("```a`b\nx").is_empty());
    }

    #[test]
    fn backticks_and_tildes() {
        assert_eq!(code_blocks("Text\n```\na\nb\n```\nmehr\n~~~\nc\n~~~\n"), ["a\nb", "c"]);
        assert!(code_blocks("kein Code, nur `inline` und ``zwei``").is_empty());
        assert!(code_blocks("").is_empty());
    }

    #[test]
    fn info_string_is_ignored_but_no_backtick_in_backtick_info() {
        assert_eq!(code_blocks("```rust title=\"x\"\nfn main() {}\n```"), ["fn main() {}"]);
        // Backtick im Info-String: keine Fence (Inline-Code über die ganze Zeile)
        assert!(code_blocks("```a`b\nx").is_empty());
        assert_eq!(code_blocks("```a`b\n```\nx\n```"), ["x"]);
        // bei Tilden ist er erlaubt
        assert_eq!(code_blocks("~~~ a`b\nx\n~~~"), ["x"]);
    }

    #[test]
    fn closing_fence_rules() {
        // längere schließende Fence, Leerraum danach
        assert_eq!(code_blocks("````\n```\ninnen\n`````  \t\nnach"), ["```\ninnen"]);
        // kürzere oder andere schließt nicht, Text dahinter auch nicht
        assert_eq!(code_blocks("````\na\n```\n~~~~\n```` x\nb"), ["a\n```\n~~~~\n```` x\nb"]);
        // offen bis zum Ende
        assert_eq!(code_blocks("```\nnie zu\n"), ["nie zu"]);
        assert_eq!(code_blocks("```"), [""]);
        assert_eq!(code_blocks("```\n```"), [""]);
        assert_eq!(code_blocks("```\n\n```"), [""]);
        assert_eq!(code_blocks("```\n\n\n```"), ["\n"]);
    }

    #[test]
    fn indentation() {
        // bis 3 Leerzeichen sind Fences (öffnen und schließen), der Inhalt bleibt unverändert
        assert_eq!(code_blocks("   ```\n  x\n   ```\n"), ["  x"]);
        assert_eq!(code_blocks(" ~~~\ny\n  ~~~"), ["y"]);
        // 4 Leerzeichen: keine Fence, eingerückter Code zählt nicht
        assert!(code_blocks("    ```\n    code\n    ```").is_empty());
        assert_eq!(code_blocks("```\na\n    ```\nb\n```"), ["a\n    ```\nb"]);
        // Tab ist kein Leerzeichen
        assert!(code_blocks("\t```\nx\n\t```").is_empty());
    }

    #[test]
    fn crlf_and_umlauts() {
        assert_eq!(code_blocks("Ä\r\n```sh\r\necho ü\r\nls\r\n```\r\n"), ["echo ü\nls"]);
        assert_eq!(code_blocks("```\r\na\r\n```\r\n```\r\nb\r\n```"), ["a", "b"]);
    }
}
