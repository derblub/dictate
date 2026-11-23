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

//! Adressen der Befehlspalette (`open_url`, `web_search`), Port von `palette_url.lua` (Stand `30cad69`, Vorlage in
//! `c552237:docs/w5-vorlagen/`; PLAN-wayland.md 6g, Sicherheitsregel 3).
//!
//! Die Palette gibt eine Adresse nie an eine Shell, sondern als **ein** argv-Element an `xdg-open`. Trotzdem prüft
//! `dictate-ui` jede Adresse noch einmal selbst (der Daemon hat sie schon geprüft, aber die Ausführung vertraut nie
//! darauf, Doppelprüfung wie unter Awesome): nur http und https, ein Host mit einem Buchstaben oder einer Ziffer,
//! keine Steuer- und Leerzeichen, begrenzte Länge. Modelltext erreicht argv nur auf diesem Weg (als geprüfte Adresse
//! bzw. als prozentkodierter Suchbegriff).

/// Längste Adresse (Bytes)
pub const MAX_URL: usize = 2048;
/// Längster Suchbegriff (Bytes)
pub const MAX_QUERY: usize = 2000;

/// Prozentkodierung nach RFC 3986 (Abschnitt 2.1 und 2.3): nur die ungeschützten Zeichen (ASCII-Buchstaben, Ziffern,
/// `-` `.` `_` `~`) bleiben, jedes andere **Byte** der UTF-8-Form wird `%XX` (ü = `%C3%BC`). Leerzeichen wird `%20`
/// (nicht `+`); `&`, `=`, `#`, `+`, `/`, `?` und `%` selbst werden kodiert, sodass sich der Begriff nie als weiterer
/// Parameter, Pfad oder Fragment lesen lässt.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Schema (klein) und Bereich des Host-Teils (`user@host:port`, bis vor `/`, `?` oder `#`) als Byte-Bereich, oder
/// `None` ohne `http://`/`https://` am Anfang (Groß/klein egal).
fn authority(url: &str) -> Option<(&'static str, std::ops::Range<usize>)> {
    let lower = |n: usize| url.get(..n).map(str::to_ascii_lowercase);
    let (scheme, start) = if lower(8).as_deref() == Some("https://") {
        ("https", 8)
    } else if lower(7).as_deref() == Some("http://") {
        ("http", 7)
    } else {
        return None;
    };
    let end = url[start..].find(['/', '?', '#']).map_or(url.len(), |i| start + i);
    Some((scheme, start..end))
}

/// Richtungssteuerzeichen von Unicode („Trojan Source“): drehen den angezeigten Text um. Dieselbe Liste wie
/// `is_forbidden` im Katalog des Daemons (Doppelprüfung, W5 Durchsicht H3).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Darf `url` an `xdg-open`? Fehler als kurzer englischer Text für das Overlay (ohne die Adresse).
pub fn valid_web_url(url: &str) -> Result<(), &'static str> {
    if url.is_empty() || url.len() > MAX_URL {
        return Err("The address is empty or too long");
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace() || is_bidi_control(c)) {
        return Err("The address contains spaces or control characters");
    }
    let Some((_, host)) = authority(url) else { return Err("Only http and https addresses can be opened") };
    if !url[host].bytes().any(|b| b.is_ascii_alphanumeric()) {
        return Err("The address has no host");
    }
    Ok(())
}

/// Setzt den prozentkodierten Begriff in die Vorlage der Suchmaschine ein (`palette.web_search_url`, vom Daemon als
/// `web_search_url` mitgeschickt): nur https, genau ein `{query}`, nie im Host-Teil (sonst bestimmte der Begriff, wohin
/// die Anfrage geht). Das Ergebnis besteht [`valid_web_url`].
pub fn search_url(template: &str, query: &str) -> Result<String, &'static str> {
    const SLOT: &str = "{query}";
    if template.is_empty() {
        return Err("No search engine is configured");
    }
    if query.trim().is_empty() || query.len() > MAX_QUERY {
        return Err("There is nothing to search for");
    }
    let Some(("https", host)) = authority(template) else { return Err("The search address must start with https://") };
    let mut slots = template.match_indices(SLOT);
    let (Some((first, _)), None) = (slots.next(), slots.next()) else { return Err("The search address needs exactly one {query}") };
    if first < host.end {
        return Err("{query} must not be part of the host");
    }
    let url = format!("{}{}{}", &template[..first], encode(query), &template[first + SLOT.len()..]);
    valid_web_url(&url)?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Testvektoren aus `palette_url_test.lua` (bis Block E unter `docs/w5-vorlagen/`, `git show c552237:docs/w5-vorlagen/palette_url_test.lua`)
    #[test]
    fn encodes_every_byte_outside_the_unreserved_set() {
        assert_eq!(encode("grüße & co"), "gr%C3%BC%C3%9Fe%20%26%20co");
        assert_eq!(encode("AZaz09-._~"), "AZaz09-._~");
        assert_eq!(encode("a=b+c#d%e/f?g"), "a%3Db%2Bc%23d%25e%2Ff%3Fg");
        assert_eq!(encode(""), "");
        assert_eq!(encode("😀"), "%F0%9F%98%80");
        assert_eq!(encode("\"a\" (b)"), "%22a%22%20%28b%29");
        assert_eq!(encode("a\nb\tc"), "a%0Ab%09c");
        assert_eq!(encode("100%"), "100%25");
    }

    const DDG: &str = "https://duckduckgo.com/?q={query}";

    #[test]
    fn builds_search_addresses() {
        assert_eq!(search_url(DDG, "grüße & co").unwrap(), "https://duckduckgo.com/?q=gr%C3%BC%C3%9Fe%20%26%20co");
        assert_eq!(search_url("https://example.org/search/{query}/x", "a b").unwrap(), "https://example.org/search/a%20b/x");
        // der Begriff wird nie als Parameter oder Fragment gelesen
        assert_eq!(search_url(DDG, "x&q=evil#frag").unwrap(), "https://duckduckgo.com/?q=x%26q%3Devil%23frag");
        // {query} im Fragment hinter dem Host ist erlaubt
        assert_eq!(search_url("https://example.org#{query}", "a b").unwrap(), "https://example.org#a%20b");
        // Schema in Großbuchstaben zählt als https
        assert_eq!(search_url("HTTPS://example.org/?q={query}", "a").unwrap(), "HTTPS://example.org/?q=a");
    }

    #[test]
    fn rejects_bad_templates_and_queries() {
        let fails = |template: &str, query: &str, part: &str| {
            let e = search_url(template, query).unwrap_err();
            assert!(e.contains(part), "{template:?} {query:?}: {e}");
        };
        fails("", "x", "No search engine");
        fails("http://duckduckgo.com/?q={query}", "x", "https://");
        fails("javascript:alert({query})", "x", "https://");
        fails("https://duckduckgo.com/?q=", "x", "exactly one");
        fails("https://duckduckgo.com/?q={query}&r={query}", "x", "exactly one");
        fails("https://{query}.example.org/", "x", "host");
        fails("https://example.org:{query}/", "x", "host");
        fails("https://{query}@example.org/", "x", "host");
        fails(DDG, "", "nothing to search");
        fails(DDG, "   ", "nothing to search");
        fails(DDG, &"a".repeat(MAX_QUERY + 1), "nothing to search");
        // genau die Grenze geht (kodiert bleibt sie unter MAX_URL)
        assert!(search_url(DDG, &"a".repeat(MAX_QUERY)).is_ok());
        // kodiert zu lang: die Adresse wird abgelehnt, nicht gekürzt
        assert!(search_url(DDG, &"ü".repeat(MAX_QUERY / 2)).is_err());
    }

    #[test]
    fn only_http_and_https_with_a_host() {
        let ok = |u: &str| valid_web_url(u).is_ok();
        assert!(ok("https://example.org/a?b=1&c=2"));
        assert!(ok("http://example.org"));
        assert!(ok("HTTPS://Example.org"));
        assert!(ok("https://user@example.org:8443/x"));
        assert!(ok("https://bücher.example/x"));
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.org/",
            "example.org",
            "https://",
            "https:///etc",
            "https://example.org/a b",
            "https://example.org/\nx",
            "https://example.org/\u{1}",
            "https://example.org/\u{a0}x",
            "https://example.org/\u{85}x",
            "",
            "x-https://example.org",
            "-https://example.org",
            // Richtungssteuerzeichen drehen die Anzeige um (wie im Katalog des Daemons, W5 Durchsicht H3)
            "https://example.org/\u{202E}gro.live",
            "https://example.org/\u{2066}x",
            "https://example.org/\u{200F}x",
            "https://example.org/\u{061C}x",
        ] {
            assert!(!ok(bad), "{bad:?}");
        }
        assert!(!ok(&format!("https://example.org/{}", "a".repeat(MAX_URL))));
        // Fehlertexte nennen die Adresse nicht
        assert!(!valid_web_url("ftp://geheim.example/").unwrap_err().contains("geheim"));
    }
}
