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

//! Geheimnisfilter für Textaktionen: erkennt Zugangsdaten in der Markierung, bevor sie an eine KI geht.
//!
//! [`scan`] liefert nur die **Arten** der Funde (`"private_key"`, `"github_token"` …), nie Inhalte oder Positionen:
//! das Ergebnis darf in Protokoll, Panel und Log. Ohne regex-Crate; jeder Durchgang ist linear in der Textlänge
//! (Suchen nach festen Mustern, maximale Zeichenläufe, Werte höchstens [`MAX_VALUE`] Byte), und alle Muster sind ASCII:
//! Schnitte liegen nur an ASCII-Bytes und damit immer auf UTF-8-Zeichengrenzen.
//!
//! [`redact`] (für die Linux-Diagnose, `PLAN-diagnose.md` „schwärzen statt sperren“) nutzt denselben Kern
//! (`Scanner::run`, eine Regelliste): jede Fundstelle ([`findings`], Byte-Bereich und Art) wird zu
//! `[REDACTED:<art>]`, überlappende zu einem Platzhalter; dazu die Arten mit der Zahl ihrer Fundstellen. Danach ist
//! [`scan`] des Ergebnisses leer (Tests: für jeden Testfall von `scan` stimmen die Arten überein, der geschwärzte Text ist
//! frei, Platzhalter lösen nie einen Fund aus), bis auf zwei enge Ausnahmen für strukturierte Zeilen im Modus `Log`
//! (`Name=Wert` mit einem Namen aus Wörtern, siehe [`Strictness::Log`]); das Netz der Diagnose ist darum [`residue`].
//! `scan` selbst verhält sich wie vorher.
//!
//! Erkannt wird (spezifisch vor allgemein; ein Fund belegt seinen Bereich, damit dieselbe Stelle nicht doppelt zählt):
//! 1. PEM-Blöcke mit `PRIVATE KEY` (RSA, EC, OPENSSH, PGP, verschlüsselt), PuTTY-Schlüsseldateien, age-Schlüssel.
//!    Öffentliche Blöcke (Zertifikate, `PUBLIC KEY`, Signaturen) und ssh-Public-Keys sind keine Geheimnisse und von der
//!    Entropieprüfung ausgenommen.
//! 2. Tokens mit festem Präfix ([`PREFIXES`]): Anthropic, OpenAI, GitHub, GitLab, AWS, Slack, Google, Stripe, npm,
//!    Hugging Face, PyPI, DigitalOcean, Shopify.
//! 3. JWTs (`eyJ….eyJ….sig`).
//! 4. URLs mit Passwort (`scheme://user:pass@host`); URLs sind sonst von der Entropieprüfung ausgenommen.
//! 5. `Authorization:`-Kopfzeilen mit Wert und `Bearer <token>`.
//! 6. Zuweisungen an Schlüssel wie `password`, `secret`, `token`, `api_key`, `client_secret` (auch `DB_PASSWORD`,
//!    `apiKey`, `x-api-key`, `Passwort`) mit `=`, `:`, `:=`, `=>` und einem Wert, der kein Platzhalter, Typ,
//!    Variablenverweis oder Statuswort ist (`.env`, YAML, JSON, INI, Verbindungszeichenketten, URL-Parameter, CLI
//!    `--password x`). Schlüssel wie `password_file`, `token_url`, `max_tokens` zählen nicht.
//! 7. Hochentropie-Tokens: Lauf aus Base64/Base64url-Zeichen, mindestens 24 Zeichen, mindestens drei Zeichenklassen,
//!    Shannon-Entropie über 4,0 Bit je Zeichen. Ausgenommen: reines Hex (Git-SHA, Hashes, also auch Hex ≤ 64 ohne
//!    Schlüsselwort; mit Schlüsselwort greift 6), UUIDs, beides und Maße auch hinter einem schlichten Bezeichner
//!    (`InvocationID=<hex>`, `ViewPortOut=2211x1963+0+0`; Messschritt in `src/diagnose/filter.rs`), aus Wörtern
//!    zusammengesetzte Bezeichner, Pfade, URLs.
//! 8. Nur wenn sonst nichts gefunden wurde: die ganze Markierung ist ohne äußere Satzzeichen ein einzelnes Token
//!    (a, b: ohne Leerzeichen, 8 bis 128 Zeichen), das wie ein markiertes Passwort aussieht:
//!    - a) mindestens drei Zeichenklassen (klein, groß, Ziffer, Sonderzeichen), auch wenn es aus Wörtern mit Trennern
//!      zusammengesetzt ist (`Sommer-2024`, `Hund_Katze_99`, `Correct-Horse-Battery7`). Wortverbinder `- _ . / :`
//!      zählen dabei nicht als Sonderzeichen, darum bleiben `snake_case`, `kebab-case`, `SCREAMING_SNAKE` und
//!      Kleinbuchstaben-Bezeichner mit Ziffern (`x86_64-unknown-linux-gnu`) bei höchstens zwei Klassen. Ausgenommen
//!      sind nur Formen, die sich als Verweis erkennen lassen: URLs, Pfade, E-Mail-Adressen, Code (`::`, Klammern …),
//!      CLI-Optionen, Versionen, Datumsangaben, Dateinamen, Umgebungszuweisungen, Hex/UUID, gepunktete Bezeichner.
//!    - b) Passphrase aus einem Generator (Bitwarden/Vaultwarden „Passphrase“ mit „Include number“): mindestens drei
//!      Wörter aus 3 bis 9 ASCII-Buchstaben mit einem einheitlichen Trenner, genau ein Wort trägt eine angehängte
//!      Ziffer (`correct-horse7-battery`). Trenner `_ . / : \ @` gelten hier nicht: so sehen `ipv4_route_lookup`,
//!      Hostnamen (`web1.example.com`) und Pfade aus.
//!    - c) Passphrase aus Wörtern der EFF Large Wordlist ([`wordlist`], Quelle der Generatoren von Bitwarden und Vaultwarden):
//!      3 bis 20 Wörter mit einheitlichem Trenner, auch ohne Ziffer und kleingeschrieben (`correct-battery-staple`), mit
//!      Großschreibung aller Wörter und einer Ziffer an einem Wort, Trenner aus 1 bis 8 Zeichen ohne Leerraum oder ein
//!      einzelnes Leerzeichen, dazu die vier Listenwörter mit Bindestrich (`yo-yo`); Einzelheiten bei [`eff_passphrase`].
//!      Es zählen nur Wörter der Liste: `horse` fehlt, darum ist `correct-horse-battery-staple` (xkcd) keine solche
//!      Passphrase, sondern erst mit Ziffer (b) oder Großschreibung und Ziffer (a) ein Fund.
//!      Fehlalarme, gemessen am 23.10.2026 (`false_alarm_report`; Anteil der verschiedenen Kandidaten aus mindestens drei
//!      Teilen, die ganz aus Listenwörtern bestehen): kebab-case 0,3 bis 0,6 % (9000 Ketten aus 663 MB Crate-Quellen, 4900
//!      Arch-Paketnamen), `snake_case` 0,2 %, Rust-Pfade mit `::` 0,2 %, Pfade mit `/` 0,01 %, Punktketten 2,1 % (448 von
//!      543 Treffern beginnen mit `self.` und sind ausgenommen, danach 0,4 %), drei aufeinanderfolgende Wörter Fließtext
//!      0,16 bis 0,24 %, vier 0,02 %, fünf 0,005 %, ganze Zeilen 0,001 %. Von 34 HTTP-Kopfzeilen aus mindestens drei
//!      Wörtern trifft es `Strict-Transport-Security`, von 93 CSS-Klassen eine (`footer-widget-area`), von 74
//!      CLI-Optionen, 53 Paketnamen und 189 systemd-Units keine. Mindestwortzahl 3, nicht 4: der Generator beginnt bei 3
//!      (SDK-Vorgabe, 39 Bit Entropie, die schwächste Passphrase), und vier statt drei Wörter senkten die Fehlalarme um
//!      höchstens 0,5 Prozentpunkte, ließen aber jede Passphrase mit drei Wörtern durch.
//!    - Nicht erkannt: Passphrasen aus anderen Listen (diceware, xkcd), ohne Trenner (der Generator verlangt einen), mit
//!      Buchstaben oder Ziffern als Trenner, ganz in Großbuchstaben (KeePassXC „UPPER CASE“; im Code wären es `SCREAMING_SNAKE`-
//!      Konstanten, +0,4 Prozentpunkte) und mit Trennern wie `, ` oder ` - ` (Aufzählungen und Fließtext).
//!    - In Kauf genommen (Fehlalarm sperrt nur die Cloud): Modellnamen mit Groß-, Kleinbuchstaben und Ziffern
//!      (`Qwen2.5-7B-Instruct`), `SHA-256-Prüfung`, `claude-opus-4-5@20251101` (`@` zählt als Sonderzeichen wie in `p@ss`).
//!
//! Die Regeln sind Heuristiken: Ein Fehlalarm sperrt nur die Cloud (lokale KI bleibt erlaubt), ein übersehenes
//! Geheimnis geht an einen fremden Server. Im Zweifel melden wir also.

use super::wordlist;

/// Arten in fester Reihenfolge (so liefert [`scan`] sie auch).
pub const KINDS: [&str; 16] = [
    "private_key",
    "anthropic_key",
    "openai_key",
    "github_token",
    "gitlab_token",
    "aws_access_key",
    "slack_token",
    "google_api_key",
    "stripe_key",
    "api_token",
    "jwt",
    "auth_header",
    "url_credentials",
    "secret_assignment",
    "high_entropy_token",
    "password",
];

/// Englische Beschreibung einer Art für die Anzeige (Panel, Einstellungen).
pub fn describe(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "private_key" => "Private key",
        "anthropic_key" => "Anthropic API key",
        "openai_key" => "OpenAI API key",
        "github_token" => "GitHub token",
        "gitlab_token" => "GitLab token",
        "aws_access_key" => "AWS access key",
        "slack_token" => "Slack token",
        "google_api_key" => "Google API key",
        "stripe_key" => "Stripe key",
        "api_token" => "API token",
        "jwt" => "JSON Web Token",
        "auth_header" => "Authorization header",
        "url_credentials" => "Password in a URL",
        "secret_assignment" => "Password or secret value",
        "high_entropy_token" => "Random-looking token",
        "password" => "Possible password",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    PrivateKey,
    AnthropicKey,
    OpenaiKey,
    GithubToken,
    GitlabToken,
    AwsAccessKey,
    SlackToken,
    GoogleApiKey,
    StripeKey,
    ApiToken,
    Jwt,
    AuthHeader,
    UrlCredentials,
    SecretAssignment,
    HighEntropyToken,
    Password,
}

/// Höchstlänge eines Wertes hinter einem Schlüssel (hält die Zuweisungssuche linear)
pub const MAX_VALUE: usize = 256;

/// Erkannte Arten in der Reihenfolge von [`KINDS`], ohne Doppelte; leer = nichts gefunden.
pub fn scan(text: &str) -> Vec<&'static str> {
    let s = Scanner::run(text);
    KINDS.iter().zip(s.found).filter(|(_, f)| *f).map(|(k, _)| *k).collect()
}

/// Eine Fundstelle: Byte-Bereich `start..end` im Text (immer auf Zeichengrenzen) und Art aus [`KINDS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
}

/// Fundstellen in der Reihenfolge der Prüfungen (spezifisch vor allgemein). Dieselben Regeln wie [`scan`]: die Arten
/// der Fundstellen sind genau die von [`scan`]. Fundstellen verschiedener Regeln können sich überlappen (eine Regel
/// überspringt nicht jeden Bereich einer früheren); [`redact`] führt sie zusammen.
pub fn findings(text: &str) -> Vec<Finding> {
    Scanner::run(text).spans.into_iter().map(|(k, start, end)| Finding { kind: KINDS[k as usize], start, end }).collect()
}

/// Platzhalter an der Stelle eines Fundes: `[REDACTED:<art>]`. Er löst selbst keinen Fund aus (Klammern und `:` sind in
/// keinem Token-Lauf erlaubt, `redacted` ist ein Platzhalterwort, ein Wert mit `[` ein Verweis).
pub fn placeholder(kind: &str) -> String {
    format!("[REDACTED:{kind}]")
}

/// Ergebnis von [`redact`]: Text mit Platzhaltern und die Arten mit der Zahl ihrer Fundstellen (Reihenfolge von
/// [`KINDS`]). Die Zahl zählt Fundstellen, nicht Platzhalter: überlappende Funde werden zu einem Platzhalter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    pub text: String,
    pub kinds: Vec<(&'static str, usize)>,
}

impl Redacted {
    /// Nur die Arten, wie [`scan`] sie liefert.
    pub fn kind_names(&self) -> Vec<&'static str> {
        self.kinds.iter().map(|(k, _)| *k).collect()
    }

    /// Zahl aller Fundstellen.
    pub fn total(&self) -> usize {
        self.kinds.iter().map(|(_, n)| n).sum()
    }
}

/// Höchstzahl der Durchgänge von [`redact`] (siehe dort); danach wird der ganze Text ersetzt.
const MAX_REDACT_PASSES: usize = 4;

/// Wann ein Fund zählt. Die Regeln (der Kern `Scanner::run`) sind dieselben, der Modus ändert nur die Schwellen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Für die Textaktionen ([`scan`]): ein Fehlalarm sperrt die Cloud, also zählt ein Wert nur, wenn der Zusammenhang
    /// ihn deutlich als Geheimnis zeigt.
    Selection,
    /// Für Logs der Diagnose ([`redact`]): ein Fehlalarm kostet nur den Wert, ein übersehenes Geheimnis geht an einen
    /// fremden Server. Zusätzlich zu allem, was `Selection` findet:
    /// - jeder Wert hinter einem Schlüssel einer Passwort-, Geheimnis- oder Token-Familie, auch nur aus Buchstaben, kürzer
    ///   als 4 Zeichen, mitten in der Zeile oder hinter `--password` mit weiteren Optionen (außer Stoppwort, Platzhalter,
    ///   Verweis, Pfad, URL, reine Zahl bei Token-Familien);
    /// - Schlüssel in escaptem JSON (`{\"password\":\"x\"}`, beliebig tief) und `\/` in URLs: gesucht wird in einer Kopie
    ///   ohne die Backslashes vor `"` und `/`, die Fundstellen gelten für den Originaltext;
    /// - weitere Schlüssel: `sig`, `signature`, `key` (allein), `auth`, `authkey`/`auth_key`, `session`/`session_id`, `sid`,
    ///   `cookie`, `psk`, `credential(s)`, dazu zusammengeschriebene Namen, die auf ein Schlüsselwort enden
    ///   (`PGPASSWORD`, `MYSQLPWD`, `adminpassword`, `csrftoken`);
    /// - in URLs: Userinfo ohne `:` (`https://<token>@host`), zufällige Pfadsegmente (Entropieregel je Segment) und feste
    ///   Formen (Telegram `bot<id>:<token>`, Discord- und Slack-Webhooks);
    /// - Präfixe `tskey-` (Tailscale), `dckr_pat_` (Docker), `glsa_` (Grafana), `SG.` (SendGrid).
    ///
    /// Zwei Ausnahmen für strukturierte Zeilen (Messschritt der Diagnose, Aufgabe 10b), die `scan` weiter meldet:
    /// - Regel 7: ein Lauf `Name=Wert` mit einem Namen aus Wörtern und einem Wert aus Wörtern oder Zahlen zwischen `/`
    ///   ist kein Token (`ExecMainStartTimestamp=Thu`, `FragmentPath=/etc/systemd/system/backup` aus `systemctl show`;
    ///   ohne den Namen wären es Wort und Pfad);
    /// - Regel 8: ein einzelnes Wort `Name=Wert` mit einem Namen aus Wörtern und einem Wert (der Kopf eines Abschnitts mit
    ///   einem Parameter, `container=…`, eine Zeile `WantedBy=default.target`) wird nur am Wert gemessen; Passphrasen mit
    ///   `=` als Trenner bleiben Funde.
    ///
    /// Darum ist `scan` des Ergebnisses von [`redact`] nur ohne diese Formen leer; die Diagnose prüft vor dem Senden mit
    /// [`residue`].
    Log,
}

/// Schwärzt jede Fundstelle mit [`placeholder`], im strengen Modus für Logs ([`Strictness::Log`]; für die Diagnose:
/// Logs enthalten fast immer etwas, das [`scan`] meldet; Sperren wie bei den Textaktionen machte die Diagnose in der Cloud
/// unbrauchbar). Siehe [`redact_with`].
pub fn redact(text: &str) -> Redacted {
    redact_with(text, Strictness::Log)
}

/// Arten, die [`redact`] in `text` noch schwärzen würde; leer heißt, der Text ist so, wie `redact` ihn hinterlässt. Das
/// Netz unter dem Filter der Diagnose (vor jedem Aufruf des Modells): strenger als [`scan`], nur ohne dessen Fehlalarme
/// auf strukturierten Zeilen (Ausnahmen siehe [`Strictness::Log`]).
pub fn residue(text: &str) -> Vec<&'static str> {
    redact(text).kind_names()
}

/// Schwärzt jede Fundstelle der Regeln im Modus `strictness`.
///
/// Überlappende Fundstellen werden zu einem Bereich zusammengeführt, der die Art der zuerst gefundenen trägt
/// (spezifischere Regeln laufen zuerst); gezählt wird jede Fundstelle. Danach prüft `redact_with` den geschwärzten Text
/// noch einmal und schwärzt, was erst jetzt auffällt (z. B. `Authorization: Negotiate <token>`: ohne das Token gilt
/// `Negotiate` als Wert), bis nichts mehr gefunden wird; im Modus `Log` zuletzt auch mit den Regeln von [`scan`]. So ist
/// `scan(&redact_with(t, _).text)` leer, im Modus `Log` bis auf die Ausnahmen für strukturierte Zeilen (siehe dort). Was ein späterer Durchgang findet, zählt mit; im Modus `Selection` sind die
/// Arten für alle Testfälle genau die von [`scan`]. Nach [`MAX_REDACT_PASSES`] Durchgängen (kommt in den Tests nicht vor)
/// wird der ganze Text ein Platzhalter.
pub fn redact_with(text: &str, strictness: Strictness) -> Redacted {
    let mut counts = [0usize; KINDS.len()];
    let mut current = text.to_string();
    for _ in 0..MAX_REDACT_PASSES {
        let mut found = spans_in(&current, strictness);
        if found.is_empty() && strictness == Strictness::Log {
            found = Scanner::run_structured(&current).spans;
        }
        if found.is_empty() {
            return Redacted { text: current, kinds: kind_counts(&counts) };
        }
        for (k, _, _) in &found {
            counts[*k as usize] += 1;
        }
        current = replace_spans(&current, merge_spans(&current, found));
    }
    if scan(&current).is_empty() && spans_in(&current, strictness).is_empty() {
        return Redacted { text: current, kinds: kind_counts(&counts) };
    }
    let first = KINDS.iter().zip(counts).find(|(_, n)| *n > 0).map_or("password", |(k, _)| *k);
    Redacted { text: placeholder(first), kinds: kind_counts(&counts) }
}

/// Fundstellen im Modus `strictness`; im Modus `Log` in der Kopie ohne JSON-Escapes, zurückgerechnet auf `text`.
fn spans_in(text: &str, strictness: Strictness) -> Vec<(Kind, usize, usize)> {
    if strictness == Strictness::Selection {
        return Scanner::run(text).spans;
    }
    let (copy, map) = unescape_json(text);
    match map {
        None => Scanner::run_with(text, true).spans,
        Some(map) => {
            Scanner::run_with(&copy, true).spans.into_iter().filter(|(_, s, e)| s < e).map(|(k, s, e)| (k, map[s], map[e.min(map.len()) - 1] + 1)).collect()
        }
    }
}

/// Entfernt jeden Backslash-Lauf direkt vor `"` oder `/` (JSON in JSON in beliebiger Tiefe: `\"`, `\\\"`, `\/`).
/// Liefert die Kopie und für jedes ihrer Bytes den Versatz im Original; `None`, wenn nichts entfernt wurde.
fn unescape_json(text: &str) -> (String, Option<Vec<usize>>) {
    let b = text.as_bytes();
    if !b.windows(2).any(|w| w[0] == b'\\' && matches!(w[1], b'"' | b'/')) {
        return (String::new(), None);
    }
    let mut out = Vec::with_capacity(b.len());
    let mut map = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let end = run_end(b, i, |c| c == b'\\');
            if matches!(b.get(end), Some(b'"' | b'/')) {
                i = end;
                continue;
            }
            for (j, &c) in b.iter().enumerate().take(end).skip(i) {
                out.push(c);
                map.push(j);
            }
            i = end;
            continue;
        }
        out.push(b[i]);
        map.push(i);
        i += 1;
    }
    // nur ASCII-Backslashes entfernt: die Kopie bleibt gültiges UTF-8
    (String::from_utf8(out).unwrap_or_default(), Some(map))
}

fn kind_counts(counts: &[usize; KINDS.len()]) -> Vec<(&'static str, usize)> {
    KINDS.iter().zip(counts).filter(|(_, n)| **n > 0).map(|(k, n)| (*k, *n)).collect()
}

/// Bereiche nach Anfang sortiert und überlappende zusammengeführt (Art des zuerst gefundenen); Grenzen auf
/// Zeichengrenzen erweitert (bei ASCII-Mustern sind sie das schon).
fn merge_spans(text: &str, spans: Vec<(Kind, usize, usize)>) -> Vec<(Kind, usize, usize)> {
    let mut v: Vec<(usize, Kind, usize, usize)> = spans
        .into_iter()
        .enumerate()
        .filter(|(_, (_, s, e))| s < e)
        .map(|(order, (k, mut s, mut e))| {
            e = e.min(text.len());
            while s > 0 && !text.is_char_boundary(s) {
                s -= 1;
            }
            while e < text.len() && !text.is_char_boundary(e) {
                e += 1;
            }
            (order, k, s, e)
        })
        .collect();
    v.sort_by_key(|&(order, _, s, _)| (s, order));
    let mut out: Vec<(usize, Kind, usize, usize)> = Vec::new();
    for item in v {
        match out.last_mut() {
            Some(last) if item.2 < last.3 => {
                last.3 = last.3.max(item.3);
                if item.0 < last.0 {
                    last.0 = item.0;
                    last.1 = item.1;
                }
            }
            _ => out.push(item),
        }
    }
    out.into_iter().map(|(_, k, s, e)| (k, s, e)).collect()
}

fn replace_spans(text: &str, spans: Vec<(Kind, usize, usize)>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (k, s, e) in spans {
        out.push_str(&text[at..s]);
        out.push_str(&placeholder(KINDS[k as usize]));
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

/// Sieht ein einzelnes Wort (ohne Leerraum, außen ohne Satzzeichen) wie ein Passwort aus? Regel 8 von [`scan`] für
/// ein Wort aus einem größeren Text (die Diagnose prüft so die Werte der Kernel-Kommandozeile streng).
pub fn password_like(word: &str) -> bool {
    if eff_passphrase(word) {
        return true;
    }
    let n = word.chars().count();
    (8..=128).contains(&n) && !word.chars().any(char::is_whitespace) && (generated_passphrase(word) || (char_classes(word) >= 3 && password_shaped(word)))
}

/// Ist das ASCII-Wort ein Hochentropie-Token nach Regel 7 von [`scan`]?
pub fn random_token(word: &str) -> bool {
    word.bytes().all(is_b64) && high_entropy(word.as_bytes())
}

/// Bereich ist ein Fund (andere Prüfungen überspringen ihn)
const CLAIMED: u8 = 1;
/// Bereich ist kein Fall für die Entropieprüfung (URL, öffentlicher Schlüssel, Zertifikat)
const NO_ENTROPY: u8 = 2;

struct Scanner<'a> {
    text: &'a str,
    b: &'a [u8],
    flags: Vec<u8>,
    found: [bool; KINDS.len()],
    /// Fundstellen in der Reihenfolge der Prüfungen
    spans: Vec<(Kind, usize, usize)>,
    /// Modus [`Strictness::Log`]
    log: bool,
    /// Ausnahmen für strukturierte Zeilen ([`word_assignment`]): im Modus `Log` und in seiner letzten Prüfung mit den
    /// Regeln von `scan`, nie in [`scan`] selbst
    structured: bool,
}

impl<'a> Scanner<'a> {
    /// Alle Prüfungen der Reihe nach im Modus [`Strictness::Selection`] ([`scan`], [`findings`])
    fn run(text: &'a str) -> Scanner<'a> {
        Scanner::run_with(text, false)
    }

    /// Alle Prüfungen der Reihe nach (der gemeinsame Kern von [`scan`], [`findings`] und [`redact`]); `log` ist der
    /// Modus [`Strictness::Log`].
    fn run_with(text: &'a str, log: bool) -> Scanner<'a> {
        Scanner::run_full(text, log, log)
    }

    /// Die Regeln von [`scan`] mit den Ausnahmen für strukturierte Zeilen: die letzte Prüfung von [`redact_with`] im
    /// Modus `Log`.
    fn run_structured(text: &'a str) -> Scanner<'a> {
        Scanner::run_full(text, false, true)
    }

    fn run_full(text: &'a str, log: bool, structured: bool) -> Scanner<'a> {
        let mut s = Scanner { text, b: text.as_bytes(), flags: vec![0; text.len()], found: [false; KINDS.len()], spans: Vec::new(), log, structured };
        s.pem_blocks();
        s.other_private_keys();
        s.ssh_public_keys();
        s.prefixed_tokens();
        s.jwts();
        s.urls();
        s.auth_headers();
        s.assignments();
        s.high_entropy();
        if !s.found.iter().any(|&f| f) {
            s.single_word();
        }
        s
    }
}

impl Scanner<'_> {
    fn hit(&mut self, k: Kind, from: usize, to: usize) {
        self.found[k as usize] = true;
        self.spans.push((k, from, to.min(self.b.len())));
        self.mark(from, to, CLAIMED);
    }

    fn mark(&mut self, from: usize, to: usize, flag: u8) {
        let to = to.min(self.flags.len());
        for f in &mut self.flags[from.min(to)..to] {
            *f |= flag;
        }
    }

    fn any_flag(&self, from: usize, to: usize, mask: u8) -> bool {
        self.flags[from..to.min(self.flags.len())].iter().any(|f| f & mask != 0)
    }

    /// Teilstück als `&str`; `None`, falls die Grenzen keine Zeichengrenzen sind (bei ASCII-Grenzen nie)
    fn str(&self, from: usize, to: usize) -> Option<&str> {
        self.text.get(from..to)
    }

    // -----------------------------------------------------------------------------------------------------------
    // 1. Private Schlüssel

    fn pem_blocks(&mut self) {
        let b = self.b;
        let mut from = 0;
        while let Some(p) = find(b, b"-----BEGIN ", from) {
            let label_start = p + 11;
            let mut i = label_start;
            while i < b.len() && i - label_start < 64 && (b[i].is_ascii_uppercase() || b[i].is_ascii_digit() || b[i] == b' ') {
                i += 1;
            }
            if !b[i..].starts_with(b"-----") || i == label_start {
                from = label_start;
                continue;
            }
            let private = find(&b[label_start..i], b"PRIVATE KEY", 0).is_some();
            // Ende: `-----END …-----`; fehlt es (Ausschnitt), gilt der Rest des Textes
            let end = match find(b, b"-----END ", i + 5) {
                Some(e) => {
                    let mut j = e + 9;
                    while j < b.len() && j - e < 80 && b[j] != b'\n' && !b[j..].starts_with(b"-----") {
                        j += 1;
                    }
                    if b[j..].starts_with(b"-----") {
                        j + 5
                    } else {
                        j
                    }
                }
                None => b.len(),
            };
            if private {
                self.hit(Kind::PrivateKey, p, end);
            } else {
                self.mark(p, end, NO_ENTROPY);
            }
            from = end.max(p + 1);
        }
    }

    /// PuTTY-Schlüsseldateien und age-Schlüssel
    fn other_private_keys(&mut self) {
        if let Some(p) = find(self.b, b"PuTTY-User-Key-File-", 0) {
            // bis zum Ende der Zeile `Private-MAC:` (oder des Textes)
            let end = find(self.b, b"Private-MAC:", p).map_or(self.b.len(), |m| line_end(self.b, m));
            self.hit(Kind::PrivateKey, p, end);
        }
        let mut from = 0;
        while let Some(p) = find(self.b, b"AGE-SECRET-KEY-1", from) {
            let end = run_end(self.b, p, |c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'-');
            if end - p >= 16 + 50 {
                self.hit(Kind::PrivateKey, p, end);
            }
            from = end.max(p + 1);
        }
    }

    /// Öffentliche ssh-Schlüssel (`ssh-ed25519 AAAA…`) sind kein Geheimnis: Base64 dahinter nicht prüfen
    fn ssh_public_keys(&mut self) {
        const TYPES: [&[u8]; 8] = [
            b"ssh-rsa ",
            b"ssh-dss ",
            b"ssh-ed25519 ",
            b"ecdsa-sha2-nistp256 ",
            b"ecdsa-sha2-nistp384 ",
            b"ecdsa-sha2-nistp521 ",
            b"sk-ssh-ed25519@openssh.com ",
            b"sk-ecdsa-sha2-nistp256@openssh.com ",
        ];
        for t in TYPES {
            let mut from = 0;
            while let Some(p) = find(self.b, t, from) {
                let start = p + t.len();
                let end = run_end(self.b, start, is_b64);
                if self.b[start..end].starts_with(b"AAAA") {
                    self.mark(start, end, NO_ENTROPY);
                }
                from = end.max(p + 1);
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // 2. Präfix-Tokens

    fn prefixed_tokens(&mut self) {
        let mut i = 0;
        while i < self.b.len() {
            if !is_tok(self.b[i]) {
                i += 1;
                continue;
            }
            let end = run_end(self.b, i, is_tok);
            let run = &self.b[i..end];
            if let Some(p) = PREFIXES.iter().find(|p| p.matches(run)) {
                self.hit(p.kind, i, end);
            } else if self.log {
                if let Some(p) = LOG_PREFIXES.iter().find(|p| p.matches(run)) {
                    self.hit(p.kind, i, end);
                } else if let Some(e) = self.sendgrid_key(i, end) {
                    self.hit(Kind::ApiToken, i, e);
                    i = e;
                    continue;
                }
            }
            i = end;
        }
    }

    /// SendGrid `SG.<22 Zeichen>.<43 Zeichen>` (der Lauf `run_start..run_end` ist `SG`); Ende des Schlüssels
    fn sendgrid_key(&self, run_start: usize, run_end_at: usize) -> Option<usize> {
        let b = self.b;
        if &b[run_start..run_end_at] != b"SG" || b.get(run_end_at) != Some(&b'.') || (run_start > 0 && is_key(b[run_start - 1])) {
            return None;
        }
        let a = run_end_at + 1;
        let a_end = run_end(b, a, is_tok);
        if a_end - a != 22 || b.get(a_end) != Some(&b'.') {
            return None;
        }
        let c_end = run_end(b, a_end + 1, is_tok);
        (c_end - a_end - 1 == 43).then_some(c_end)
    }

    // -----------------------------------------------------------------------------------------------------------
    // 3. JWT

    fn jwts(&mut self) {
        let is_jwt = |c: u8| is_tok(c) || c == b'.';
        let mut from = 0;
        while let Some(p) = find(self.b, b"eyJ", from) {
            if p > 0 && is_jwt(self.b[p - 1]) {
                from = p + 3;
                continue;
            }
            let end = run_end(self.b, p, is_jwt);
            let mut parts = self.b[p..end].split(|&c| c == b'.');
            let (h, c, sig) = (parts.next().unwrap_or(b""), parts.next().unwrap_or(b""), parts.next());
            if h.len() >= 10 && c.len() >= 10 && c.starts_with(b"eyJ") && sig.is_some() {
                self.hit(Kind::Jwt, p, end);
            }
            from = end.max(p + 3);
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // 4. URLs

    fn urls(&mut self) {
        let b = self.b;
        let mut from = 0;
        while let Some(p) = find(b, b"://", from) {
            let mut start = p;
            while start > 0 && (b[start - 1].is_ascii_alphanumeric() || matches!(b[start - 1], b'+' | b'.' | b'-')) {
                start -= 1;
            }
            if start == p || !b[start].is_ascii_alphabetic() {
                from = p + 3;
                continue;
            }
            let auth_start = p + 3;
            let auth_end = run_end(b, auth_start, |c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'?' | b'#' | b'"' | b'\'' | b'<' | b'>' | b'`'));
            let url_end = run_end(b, auth_end, |c| !c.is_ascii_whitespace() && !matches!(c, b'"' | b'\'' | b'<' | b'>' | b'`'));
            self.mark(start, url_end, NO_ENTROPY);
            let at = b[auth_start..auth_end].iter().rposition(|&c| c == b'@');
            if let Some(at) = at {
                let userinfo = &b[auth_start..auth_start + at];
                if let Some(colon) = userinfo.iter().position(|&c| c == b':') {
                    let (ps, pe) = (auth_start + colon + 1, auth_start + at);
                    let pass = self.str(ps, pe).unwrap_or("");
                    if !pass.is_empty() && !is_placeholder(pass) && !is_reference(pass) && !is_stopword(pass) {
                        self.hit(Kind::UrlCredentials, ps, pe);
                    }
                } else if self.log && url_token(userinfo) && !self.any_flag(auth_start, auth_start + at, CLAIMED) {
                    // `https://<token>@git.example.com/x.git`: ein Token als Benutzername
                    self.hit(Kind::UrlCredentials, auth_start, auth_start + at);
                }
            }
            if self.log {
                let host_start = at.map_or(auth_start, |a| auth_start + a + 1);
                self.url_path(host_start, auth_end, url_end);
            }
            from = url_end.max(p + 3);
        }
    }

    /// Modus `Log`: Geheimnisse im Pfad einer URL (`host_start..host_end` ist der Rechner mit Port, dahinter der Pfad bis
    /// `url_end`). Feste Formen der Webhooks, dann jedes Segment zwischen `/ ? & # ; = : ,` nach der Entropieregel.
    /// Parameter wie `?sig=…` prüft die Zuweisungsregel.
    fn url_path(&mut self, host_start: usize, host_end: usize, url_end: usize) {
        let b = self.b;
        let host = b[host_start..host_end].split(|&c| c == b':').next().unwrap_or(b"").to_ascii_lowercase();
        let path_end = host_end + b[host_end..url_end].iter().position(|&c| matches!(c, b'?' | b'#')).unwrap_or(url_end - host_end);
        let path = &b[host_end..path_end];
        // Discord `/api/webhooks/<id>/<token>`, Slack `hooks.slack.com/services/<T>/<B>/<token>`: alles dahinter
        let fixed: Option<usize> = if (host.ends_with(b"discord.com") || host.ends_with(b"discordapp.com")) && path.starts_with(b"/api/webhooks/") {
            let id_start = host_end + 14;
            let id_end = run_end(b, id_start, |c| c.is_ascii_digit());
            (id_end > id_start && b.get(id_end) == Some(&b'/')).then_some(id_end + 1)
        } else if host == b"hooks.slack.com" && path.starts_with(b"/services/") {
            Some(host_end + 10)
        } else {
            None
        };
        if let Some(s) = fixed.filter(|&s| s < path_end && !self.any_flag(s, path_end, CLAIMED)) {
            self.hit(Kind::ApiToken, s, path_end);
            return;
        }
        let mut i = host_end;
        while i < url_end {
            if matches!(b[i], b'/' | b'?' | b'&' | b'#' | b';' | b'=' | b',') {
                i += 1;
                continue;
            }
            let seg_end = run_end(b, i, |c| !matches!(c, b'/' | b'?' | b'&' | b'#' | b';' | b'=' | b',')).min(url_end);
            // Telegram `bot<id>:<token>` (das Segment enthält den Doppelpunkt)
            let seg = &b[i..seg_end];
            let telegram = seg.starts_with(b"bot")
                && seg.iter().position(|&c| c == b':').is_some_and(|c| {
                    let (id, tok) = (&seg[3..c], &seg[c + 1..]);
                    id.len() >= 6 && id.iter().all(u8::is_ascii_digit) && tok.len() >= 30 && tok.iter().all(|&x| is_tok(x))
                });
            if telegram && !self.any_flag(i, seg_end, CLAIMED) {
                self.hit(Kind::ApiToken, i, seg_end);
            } else {
                let mut j = i;
                for piece in seg.split(|&c| c == b':') {
                    if high_entropy(piece) && !self.any_flag(j, j + piece.len(), CLAIMED) {
                        self.hit(Kind::HighEntropyToken, j, j + piece.len());
                    }
                    j += piece.len() + 1;
                }
            }
            i = seg_end.max(i + 1);
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // 5. Authorization / Bearer

    fn auth_headers(&mut self) {
        let b = self.b;
        let mut from = 0;
        while let Some(p) = find_ci(b, b"authorization", from) {
            from = p + 13;
            if (p > 0 && b[p - 1].is_ascii_alphanumeric()) || b.get(from).is_some_and(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            let mut i = skip(b, from, |c| matches!(c, b'"' | b'\'' | b' ' | b'\t'));
            if !matches!(b.get(i), Some(b':' | b'=')) {
                continue;
            }
            i = skip(b, i + 1, |c| matches!(c, b'"' | b'\'' | b' ' | b'\t'));
            // „Bearer x“, „Basic x“, „token x“ oder gleich der Wert
            let word_end = run_end(b, i, |c| c.is_ascii_alphabetic());
            let (cs, ce) = if word_end > i && b.get(word_end) == Some(&b' ') && b.get(word_end + 1).is_some_and(|&c| is_cred(c)) {
                (word_end + 1, run_end(b, word_end + 1, is_cred))
            } else {
                (i, run_end(b, i, is_cred))
            };
            let cred = self.str(cs, ce).unwrap_or("");
            if cred.len() >= 8 && !is_placeholder(cred) && !is_reference(cred) && !is_stopword(cred) && !self.any_flag(cs, ce, CLAIMED) {
                self.hit(Kind::AuthHeader, cs, ce);
            }
            from = ce.max(from);
        }
        from = 0;
        while let Some(p) = find_ci(b, b"bearer", from) {
            from = p + 6;
            if p > 0 && b[p - 1].is_ascii_alphanumeric() {
                continue;
            }
            let cs = skip(b, from, |c| c == b' ' || c == b'\t');
            if cs == from {
                continue;
            }
            let ce = run_end(b, cs, is_cred);
            let cred = &b[cs..ce];
            let letters = cred.iter().any(u8::is_ascii_alphabetic);
            let other = cred.iter().any(|c| !c.is_ascii_alphabetic());
            let s = self.str(cs, ce).unwrap_or("");
            if cred.len() >= 16 && letters && other && !word_like(cred) && !is_placeholder(s) && !self.any_flag(cs, ce, CLAIMED) {
                self.hit(Kind::AuthHeader, cs, ce);
            }
            from = ce.max(from);
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // 6. Zuweisungen

    fn assignments(&mut self) {
        let b = self.b;
        let mut i = 0;
        while i < b.len() {
            if !is_key(b[i]) {
                i += 1;
                continue;
            }
            let (ks, ke) = (i, run_end(b, i, is_key));
            i = ke;
            let Some(family) = key_family(&b[ks..ke], self.log) else { continue };
            let Some(v) = self.value_after(ks, ke) else { continue };
            // `key`, `sig`, `session` … zählen nur mit `=` oder als CLI-Option (`?sig=…`, `--key x`), nicht mit `:`
            // (`Kernel signing key: <Fingerabdruck>`, `session: rotated`)
            if self.log && !v.equals && !v.flag && weak_log_key(&b[ks..ke]) {
                continue;
            }
            i = i.max(v.end);
            let s = self.str(v.start, v.end).unwrap_or("");
            if secret_value(s, &v, family, self.log) && !self.any_flag(v.start, v.end, CLAIMED) {
                self.hit(Kind::SecretAssignment, v.start, v.end);
            }
        }
    }

    /// Wert hinter dem Schlüssel `b[ks..ke]`: Trenner, dann ein Wert in Anführungszeichen oder ein Lauf bis zum
    /// nächsten Leerzeichen bzw. Trennzeichen
    fn value_after(&self, ks: usize, ke: usize) -> Option<Value> {
        let b = self.b;
        let quoted_key = ks > 0 && matches!(b[ks - 1], b'"' | b'\'') && b.get(ke) == Some(&b[ks - 1]);
        let mut i = if quoted_key { ke + 1 } else { ke };
        let ws = |c: u8| c == b' ' || c == b'\t';
        let after_key = skip(b, i, ws);
        let flag = b[ks..].starts_with(b"--");
        let equals = b.get(after_key) == Some(&b'=');
        match b.get(after_key) {
            Some(b':') if b.get(after_key + 1) != Some(&b':') => i = after_key + 1 + usize::from(b.get(after_key + 1) == Some(&b'=')),
            Some(b'=') => {
                i = after_key + 1;
                while matches!(b.get(i), Some(b'=' | b'>')) {
                    i += 1;
                }
            }
            // CLI: `--password geheim`
            Some(_) if flag && after_key == ke + 1 && b[ke] == b' ' => i = after_key,
            _ => return None,
        }
        i = skip(b, i, ws);
        let structured = self.structured_context(if quoted_key { ks - 1 } else { ks });
        match b.get(i) {
            Some(&q @ (b'"' | b'\'' | b'`')) => {
                let start = i + 1;
                let limit = (start + MAX_VALUE).min(b.len());
                let close = b[start..limit].iter().position(|&c| c == q || c == b'\n')?;
                if b[start + close] != q {
                    return None;
                }
                Some(Value { start, end: start + close, quoted: true, structured, complete: true, flag, equals })
            }
            Some(&c) if !c.is_ascii_whitespace() => {
                let limit = (i + MAX_VALUE).min(b.len());
                let end = i + b[i..limit]
                    .iter()
                    .position(|&c| c.is_ascii_whitespace() || matches!(c, b'"' | b'\'' | b'`' | b',' | b';' | b'&' | b')' | b']' | b'}' | b'>'))
                    .unwrap_or(limit - i);
                let rest = skip(b, end, ws);
                let complete = matches!(b.get(rest), None | Some(b'\n' | b'\r' | b';' | b'&' | b'#'));
                Some(Value { start: i, end, quoted: false, structured, complete, flag, equals })
            }
            _ => None,
        }
    }

    /// Steht der Schlüssel am Zeilenanfang (nach Einrückung, `export `, `set `, `- `) oder hinter `;`, `&`, `?`
    /// (Verbindungszeichenkette, URL-Parameter)? Dann ist es Konfiguration, kein Fließtext und kein Code.
    fn structured_context(&self, ks: usize) -> bool {
        let b = self.b;
        let mut j = ks;
        while j > 0 && matches!(b[j - 1], b' ' | b'\t') {
            j -= 1;
        }
        let word_start = {
            let mut w = j;
            while w > 0 && (b[w - 1].is_ascii_alphabetic() || b[w - 1] == b'-') {
                w -= 1;
            }
            w
        };
        if j < ks && matches!(&b[word_start..j], b"export" | b"set" | b"-") {
            j = word_start;
            while j > 0 && matches!(b[j - 1], b' ' | b'\t') {
                j -= 1;
            }
        }
        j == 0 || matches!(b[j - 1], b'\n' | b'\r' | b';' | b'&' | b'?')
    }

    // -----------------------------------------------------------------------------------------------------------
    // 7. Hochentropie

    fn high_entropy(&mut self) {
        let mut i = 0;
        while i < self.b.len() {
            if !is_b64(self.b[i]) {
                i += 1;
                continue;
            }
            let (s, e) = (i, run_end(self.b, i, is_b64));
            i = e;
            if e - s < 24 || self.any_flag(s, e, CLAIMED | NO_ENTROPY) || self.b[..s].ends_with(b"base64,") {
                continue;
            }
            let run = &self.b[s..e];
            if self.structured && word_assignment(run) {
                continue;
            }
            // absoluter Pfad oder `//`: jedes Segment für sich
            if run[0] == b'/' || find(run, b"//", 0).is_some() {
                let mut off = s;
                for seg in run.split(|&c| c == b'/') {
                    if high_entropy(seg) {
                        self.hit(Kind::HighEntropyToken, off, off + seg.len());
                    }
                    off += seg.len() + 1;
                }
            } else if high_entropy(run) {
                self.hit(Kind::HighEntropyToken, s, e);
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // 8. Ein einzelnes Wort

    fn single_word(&mut self) {
        // Satzzeichen und Anführungszeichen außen gehören nicht zum Wort (`PartialEq,`, `„Thinking`, `"claude-opus-5"`)
        let mut t = self.text.trim().trim_matches(|c: char| ".,;:!?\"'`„“”‘’«»‹›|*".contains(c));
        // Modus `Log`: `name=wert` mit einem Namen aus Wörtern ist kein markiertes Wort, sondern eine Zuweisung (Kopf
        // `container=dataroom-worker-1`); geprüft wird nur der Wert. Eine Passphrase mit `=` als Trenner
        // (`geologic=starship=perceive`) bleibt ein Fund.
        if self.structured && !eff_passphrase(t) && !generated_passphrase(t) {
            if let Some((key, value)) = t.split_once('=') {
                if !value.is_empty() && word_identifier(key.as_bytes()) {
                    t = value;
                }
            }
        }
        // Passphrase aus der EFF-Liste darf Leerzeichen haben und bis zu 20 Wörter lang sein (in `password_like`)
        if password_like(t) {
            // `t` ist ein Teilstück von `self.text`: der Abstand der Zeiger ist der Anfang
            let start = t.as_ptr() as usize - self.text.as_ptr() as usize;
            self.hit(Kind::Password, start, start + t.len());
        }
    }
}

/// Wert einer Zuweisung (Byte-Bereich) mit Kontext
struct Value {
    start: usize,
    end: usize,
    quoted: bool,
    /// Schlüssel steht am Zeilenanfang oder hinter `;`, `&`, `?`
    structured: bool,
    /// Hinter dem Wert endet die Zeile (oder folgt `;`, `&`, `#`)
    complete: bool,
    /// CLI-Option `--password …`
    flag: bool,
    /// Trenner `=` (nicht `:`)
    equals: bool,
}

// ---------------------------------------------------------------------------------------------------------------
// Präfixe

#[derive(Clone, Copy)]
enum Set {
    /// `[A-Za-z0-9_-]`
    Tok,
    /// `[A-Za-z0-9]`
    Alnum,
    /// `[A-Z0-9]`
    UpperDigit,
}

/// Token mit festem Präfix: Rest mit Länge `min..=max` aus `set`; `mixed` verlangt Buchstaben und Ziffern im Rest
/// (trennt `sk-…`-Schlüssel von Bindestrich-Wörtern).
struct Prefix {
    prefix: &'static str,
    min: usize,
    max: usize,
    set: Set,
    mixed: bool,
    kind: Kind,
}

impl Prefix {
    fn matches(&self, run: &[u8]) -> bool {
        let Some(rest) = run.strip_prefix(self.prefix.as_bytes()) else { return false };
        let ok_char = |c: &u8| match self.set {
            Set::Tok => is_tok(*c),
            Set::Alnum => c.is_ascii_alphanumeric(),
            Set::UpperDigit => c.is_ascii_uppercase() || c.is_ascii_digit(),
        };
        let mixed = rest.iter().any(u8::is_ascii_digit) && rest.iter().any(u8::is_ascii_alphabetic);
        (self.min..=self.max).contains(&rest.len()) && rest.iter().all(ok_char) && (!self.mixed || mixed) && !is_placeholder(&String::from_utf8_lossy(rest))
    }
}

const ANY: usize = usize::MAX;

const fn px(prefix: &'static str, min: usize, max: usize, set: Set, mixed: bool, kind: Kind) -> Prefix {
    Prefix { prefix, min, max, set, mixed, kind }
}

/// Bekannte Präfixe; das erste passende gewinnt (`sk-ant-` vor `sk-`).
const PREFIXES: [Prefix; 37] = [
    px("sk-ant-", 20, ANY, Set::Tok, true, Kind::AnthropicKey),
    px("sk-", 20, ANY, Set::Tok, true, Kind::OpenaiKey),
    px("ghp_", 30, ANY, Set::Alnum, false, Kind::GithubToken),
    px("gho_", 30, ANY, Set::Alnum, false, Kind::GithubToken),
    px("ghu_", 30, ANY, Set::Alnum, false, Kind::GithubToken),
    px("ghs_", 30, ANY, Set::Alnum, false, Kind::GithubToken),
    px("ghr_", 30, ANY, Set::Alnum, false, Kind::GithubToken),
    px("github_pat_", 22, ANY, Set::Tok, false, Kind::GithubToken),
    px("glpat-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("gldt-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glrt-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glptt-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glft-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glimt-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glagent-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("gloas-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glcbt-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("glsoat-", 20, ANY, Set::Tok, false, Kind::GitlabToken),
    px("AKIA", 16, 16, Set::UpperDigit, false, Kind::AwsAccessKey),
    px("ASIA", 16, 16, Set::UpperDigit, false, Kind::AwsAccessKey),
    px("xoxb-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("xoxa-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("xoxp-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("xoxr-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("xoxs-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("xapp-", 10, ANY, Set::Tok, true, Kind::SlackToken),
    px("AIza", 35, 35, Set::Tok, false, Kind::GoogleApiKey),
    px("sk_live_", 16, ANY, Set::Alnum, false, Kind::StripeKey),
    px("rk_live_", 16, ANY, Set::Alnum, false, Kind::StripeKey),
    px("sk_test_", 16, ANY, Set::Alnum, false, Kind::StripeKey),
    px("rk_test_", 16, ANY, Set::Alnum, false, Kind::StripeKey),
    px("npm_", 36, 36, Set::Alnum, false, Kind::ApiToken),
    px("hf_", 30, ANY, Set::Alnum, false, Kind::ApiToken),
    px("pypi-AgEIcHlwaS5vcmc", 50, ANY, Set::Tok, false, Kind::ApiToken),
    px("dop_v1_", 64, 64, Set::Alnum, false, Kind::ApiToken),
    px("doo_v1_", 64, 64, Set::Alnum, false, Kind::ApiToken),
    px("shpat_", 32, 32, Set::Alnum, false, Kind::ApiToken),
];

/// Präfixe nur im Modus [`Strictness::Log`] (der Filter der Textaktionen bleibt unverändert).
const LOG_PREFIXES: [Prefix; 3] = [
    px("tskey-", 20, ANY, Set::Tok, true, Kind::ApiToken),
    px("dckr_pat_", 20, ANY, Set::Tok, false, Kind::ApiToken),
    px("glsa_", 32, ANY, Set::Tok, false, Kind::ApiToken),
];

/// Modus `Log`: sieht der Benutzername einer URL wie ein Token aus (Entropieregel, oder lang mit Buchstaben und Ziffern,
/// auch reines Hex wie alte GitHub-Tokens)?
fn url_token(u: &[u8]) -> bool {
    high_entropy(u) || (u.len() >= 20 && u.iter().all(|&c| is_tok(c)) && u.iter().any(u8::is_ascii_digit) && u.iter().any(u8::is_ascii_alphabetic))
}

// ---------------------------------------------------------------------------------------------------------------
// Schlüsselwörter und Werte

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Password,
    Secret,
    Token,
}

/// Teile eines Schlüssels (`DB_PASSWORD` → db, password; `apiKey` → api, key; `x-api-key` → x, api, key), klein.
fn key_parts(key: &[u8]) -> Vec<String> {
    let mut parts = Vec::new();
    for seg in key.split(|&c| matches!(c, b'_' | b'-' | b'.')) {
        let mut cur = String::new();
        for (i, &c) in seg.iter().enumerate() {
            let prev = i.checked_sub(1).map(|j| seg[j]);
            let next = seg.get(i + 1);
            // camelCase: vor einem Großbuchstaben nach Klein/Ziffer, oder vor Groß+Klein nach Groß (`APIKey`)
            let boundary = c.is_ascii_uppercase()
                && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit() || (p.is_ascii_uppercase() && next.is_some_and(u8::is_ascii_lowercase)));
            if boundary && !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            cur.push(c.to_ascii_lowercase() as char);
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
    }
    parts
}

/// Teile, die keinen Geheimwert, sondern etwas darüber bezeichnen (`password_file`, `token_url`, `max_tokens`,
/// `password_hint`)
const NOT_A_VALUE: &[&str] = &[
    "file",
    "files",
    "path",
    "dir",
    "url",
    "uri",
    "endpoint",
    "host",
    "name",
    "type",
    "kind",
    "len",
    "length",
    "size",
    "count",
    "max",
    "min",
    "limit",
    "limits",
    "ttl",
    "timeout",
    "expiry",
    "expires",
    "expire",
    "expiration",
    "lifetime",
    "policy",
    "header",
    "prefix",
    "env",
    "var",
    "field",
    "ref",
    "source",
    "location",
    "provider",
    "mode",
    "format",
    "algorithm",
    "algo",
    "cmd",
    "command",
    "helper",
    "store",
    "manager",
    "budget",
    "total",
    "usage",
    "used",
    "num",
    "label",
    "hint",
    "placeholder",
    "message",
    "msg",
    "text",
    "title",
    "description",
    "desc",
    "help",
    "error",
    "prompt",
    "tooltip",
    "caption",
    "required",
    "enabled",
    "length",
    "rotation",
    "strength",
    "reset",
    "changed",
    "valid",
    "validation",
    "regex",
    "pattern",
    "rule",
    "rules",
    "id",
];

/// Schlüsselfamilie, wenn ein Teil ein Schlüsselwort ist und keiner nur etwas darüber bezeichnet. `log`: Modus
/// [`Strictness::Log`] mit weiteren Schlüsseln (siehe dort).
fn key_family(key: &[u8], log: bool) -> Option<Family> {
    // schneller Vorfilter ohne Zuteilung
    let lower_has = |w: &[u8]| key.windows(w.len()).any(|x| x.eq_ignore_ascii_case(w));
    let words: &[&[u8]] = if log {
        &[b"pass", b"pwd", b"secret", b"token", b"key", b"kennwort", b"sig", b"auth", b"session", b"sid", b"cookie", b"psk", b"cred"]
    } else {
        &[b"pass", b"pwd", b"secret", b"token", b"key", b"kennwort"]
    };
    if !words.iter().any(|w| lower_has(w)) {
        return None;
    }
    let parts = key_parts(key);
    if parts.iter().any(|p| NOT_A_VALUE.contains(&p.as_str())) {
        // `secret_id` (Vault AppRole) ist trotzdem ein Geheimnis, im Modus `Log` auch `session_id`
        let pair = |a: &str, b: &str| parts.len() == 2 && parts[0] == a && parts[1] == b;
        if !(pair("secret", "id") || (log && pair("session", "id"))) {
            return None;
        }
    }
    // Mehrzahl (`secrets:`, `tokens:`) bezeichnet eine Sammlung oder Anzahl, keinen Wert
    let mut fam = None;
    for (i, p) in parts.iter().enumerate() {
        let next = parts.get(i + 1).map(String::as_str);
        let mut f = match (p.as_str(), next) {
            ("password" | "passwd" | "pwd" | "pass" | "passphrase" | "passwort" | "kennwort", _) => Some(Family::Password),
            ("secret" | "clientsecret" | "apikey" | "privatekey" | "accesskey" | "secretkey", _) => Some(Family::Secret),
            ("api" | "private" | "access" | "signing" | "master" | "encryption", Some("key" | "keys")) => Some(Family::Secret),
            ("token" | "accesstoken" | "refreshtoken" | "authtoken" | "idtoken" | "sessiontoken" | "bearertoken", _) => Some(Family::Token),
            _ => None,
        };
        // nie für Namen von systemd-Units und Zählern (`session-3.scope:`, `sid-2`)
        let unit = parts.iter().any(|p| p.bytes().all(|c| c.is_ascii_digit()))
            || parts
                .last()
                .is_some_and(|l| matches!(l.as_str(), "scope" | "service" | "slice" | "socket" | "mount" | "timer" | "target" | "device" | "swap" | "path"));
        if log && f.is_none() && !unit {
            f = log_key_family(p, next, parts.len());
        }
        // Passwort schlägt Geheimnis schlägt Token (Token-Werte sind nie reine Zahlen, Passwörter schon)
        fam = match (fam, f) {
            (None, f) | (f, None) => f,
            (Some(Family::Password), _) | (_, Some(Family::Password)) => Some(Family::Password),
            (Some(Family::Secret), _) | (_, Some(Family::Secret)) => Some(Family::Secret),
            _ => Some(Family::Token),
        };
    }
    fam
}

/// Weitere Schlüssel im Modus [`Strictness::Log`] für einen Teil `p` (mit dem nächsten Teil `next`, `n` Teile insgesamt).
/// Token-Familie, wo der Wert auch eine Zahl sein kann, die nichts verrät (`sig=11` im Audit-Log, `session=3`).
fn log_key_family(p: &str, next: Option<&str>, n: usize) -> Option<Family> {
    match (p, next) {
        ("psk" | "credential" | "credentials", _) => return Some(Family::Password),
        ("sig" | "signature" | "authkey" | "session" | "sessionid" | "sid" | "cookie", _) => return Some(Family::Token),
        ("auth", Some("key" | "keys")) => return Some(Family::Secret),
        ("auth", None) if n == 1 => return Some(Family::Token),
        ("key", _) if n == 1 => return Some(Family::Token),
        _ => {}
    }
    // zusammengeschrieben: `pgpassword`, `mysqlpwd`, `adminpassword`, `clienttoken`, `appsecret`
    const ENDINGS: [(&str, Family); 7] = [
        ("password", Family::Password),
        ("passwd", Family::Password),
        ("passwort", Family::Password),
        ("passphrase", Family::Password),
        ("pwd", Family::Password),
        ("secret", Family::Secret),
        ("token", Family::Token),
    ];
    ENDINGS.iter().find(|(e, _)| p.len() > e.len() && p.ends_with(e)).map(|(_, f)| *f)
}

/// Ein Schlüssel, der nur im Modus [`Strictness::Log`] und nur mit `=` zählt: allgemeine Wörter wie `key`, `sig`,
/// `session`, die mit `:` meist eine Beschriftung sind.
fn weak_log_key(key: &[u8]) -> bool {
    key_family(key, false).is_none()
        && key_parts(key).iter().any(|p| matches!(p.as_str(), "key" | "sig" | "signature" | "auth" | "session" | "sessionid" | "sid" | "cookie"))
}

/// Füllwörter, die im Modus [`Strictness::Log`] nie ein Wert sind (`token: not found`, `password for anna`)
const LOG_STOPWORDS: &[&str] =
    &["a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "in", "is", "it", "not", "of", "or", "the", "to", "was", "will", "with"];

/// Stoppwörter, die im Modus [`Strictness::Log`] doch als Wert zählen (`password=geheim`, `password: secret`)
const LOG_VALUE_WORDS: &[&str] =
    &["secret", "secrets", "password", "passwort", "kennwort", "token", "pass", "pwd", "passwd", "geheim", "geheimnis", "test", "sample"];

/// Status- und Füllwörter, Typen und Schlüsselwörter selbst: kein Geheimwert
const STOPWORDS: &[&str] = &[
    "none",
    "null",
    "nil",
    "undefined",
    "true",
    "false",
    "yes",
    "no",
    "on",
    "off",
    "required",
    "optional",
    "incorrect",
    "invalid",
    "wrong",
    "missing",
    "expired",
    "empty",
    "unset",
    "set",
    "hidden",
    "redacted",
    "masked",
    "changeme",
    "change_me",
    "changed",
    "updated",
    "reset",
    "accepted",
    "rejected",
    "denied",
    "failed",
    "failure",
    "error",
    "ok",
    "success",
    "string",
    "str",
    "text",
    "bool",
    "boolean",
    "int",
    "integer",
    "number",
    "float",
    "double",
    "char",
    "any",
    "object",
    "secret",
    "secrets",
    "password",
    "passwort",
    "kennwort",
    "token",
    "tokens",
    "pass",
    "pwd",
    "passwd",
    "apikey",
    "api_key",
    "api-key",
    "value",
    "example",
    "sample",
    "test",
    "dummy",
    "placeholder",
    "todo",
    "tbd",
    "n/a",
    "na",
    "default",
    "auto",
    "env",
    "secure",
    "securestring",
    "secretstring",
    "option",
    "vec",
    "box",
    "arc",
    "rc",
    "cow",
    "result",
    "impl",
    "dyn",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "f32",
    "f64",
    "self",
    "this",
    "geheim",
    "geheimnis",
    "falsch",
    "ungültig",
    "erforderlich",
    "abgelaufen",
    "fehlt",
    "leer",
    "geändert",
    "vergessen",
    "gesetzt",
    "korrekt",
    "richtig",
    "verborgen",
    "ausgeblendet",
    "nein",
    "ja",
];

fn is_stopword(v: &str) -> bool {
    let l = v.trim().to_lowercase();
    STOPWORDS.contains(&l.as_str())
}

/// Platzhalter in Doku und Beispielen
fn is_placeholder(v: &str) -> bool {
    let l = v.to_lowercase();
    const MARKERS: [&str; 10] = ["your", "example", "placeholder", "changeme", "redacted", "xxxx", "****", "....", "<", "dummy"];
    if MARKERS.iter().any(|m| l.contains(m)) {
        return true;
    }
    // ein einziges, wiederholtes Zeichen (`aaaaaaaa`, `--------`)
    let mut cs = l.chars();
    cs.next().is_some_and(|c0| l.chars().count() >= 3 && cs.all(|c| c == c0))
}

/// Verweis statt Wert: Variable, Vorlage, Format-Platzhalter
fn is_reference(v: &str) -> bool {
    let v = v.trim();
    v.starts_with(['$', '%', '&', '*', '<', '{', '[', '(', '!', '@', '#'])
        || ["${", "{{", "%(", "{}", "{0}", "%s", "$(", "process.env", "os.environ", "getenv"].iter().any(|m| v.contains(m))
}

/// Ist der Wert einer Zuweisung ein Geheimnis? `log`: Modus [`Strictness::Log`].
fn secret_value(v: &str, ctx: &Value, family: Family, log: bool) -> bool {
    let t = v.trim();
    if t.is_empty() || is_placeholder(t) || is_reference(t) || (ctx.flag && t.starts_with('-')) {
        return false;
    }
    if log && LOG_STOPWORDS.contains(&t.to_lowercase().as_str()) {
        return false;
    }
    // Schlüsselwörter als Wert: in einer Beschriftung kein Geheimnis, in einem Log oft das (schwache) Passwort selbst
    if is_stopword(t) && !(log && LOG_VALUE_WORDS.contains(&t.to_lowercase().as_str())) {
        return false;
    }
    // Pfade und URLs sind Verweise (`password = "/run/secrets/db"`); Passwörter in URLs prüft `urls`
    if t.starts_with('/') || t.starts_with("~/") || t.starts_with("./") || t.starts_with("../") || t.contains("://") {
        return false;
    }
    let numeric = t.bytes().all(|c| c.is_ascii_digit());
    if numeric && family == Family::Token {
        return false;
    }
    if ctx.quoted {
        // Beschriftung (`"password": "Passwort eingeben"`): mehrere Wörter, eins davon ein Schlüsselwort
        let words: Vec<&str> = t.split_whitespace().collect();
        if words.len() >= 2 && words.iter().all(|w| w.chars().all(char::is_alphabetic)) && words.iter().any(|w| is_stopword(w)) {
            return false;
        }
        return true;
    }
    // In Logs zählt jeder Wert: Passwörter stehen mitten in der Zeile, sind kurz oder nur aus Buchstaben
    if log {
        return true;
    }
    if t.chars().count() < 4 || t.contains("::") || t.contains("->") || t.contains('(') {
        return false;
    }
    let config = ctx.structured && ctx.complete;
    // Code: `self.token`, `config.password`, `secret_value` (Bezeichner aus Buchstaben mit `.` oder `_`)
    let ident = t.bytes().all(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'.');
    if ident && (t.contains('.') || (t.contains('_') && !config)) {
        return false;
    }
    // ein Wort nur aus Buchstaben: in Konfiguration ein Passwort (`password: hunter`), im Fließtext ein Wort
    // („Passwort: muss …“) und in Code ein Bezeichner
    if t.chars().all(char::is_alphabetic) {
        return config || (ctx.flag && ctx.complete);
    }
    true
}

// ---------------------------------------------------------------------------------------------------------------
// Entropie und Wortform

fn high_entropy(run: &[u8]) -> bool {
    if run.len() < 24 || byte_classes(run) < 3 || word_like(run) {
        return false;
    }
    // reines Hex (Git-SHA, Hashes) und UUIDs
    if run.iter().all(|c| c.is_ascii_hexdigit() || *c == b'-') {
        return false;
    }
    // dasselbe hinter einem Bezeichner: `InvocationID=<hex>`, `MACHINE_ID=<uuid>`, dazu Maße und Zahlen
    // (`ViewPortOut=2211x1963+0+0` im Xorg-Log); `=` und `+` gehören zu Base64, darum ist es ein Lauf. Ohne den Bezeichner
    // wäre der Wert ausgenommen bzw. zu kurz; ein Bezeichner mit drei Zeichenklassen (`X5Rk9pQ2=…`) bleibt ein Fund.
    if let Some(eq) = run.iter().position(|&c| c == b'=') {
        let (key, value) = (&run[..eq], &run[eq + 1..]);
        let ident = key.first().is_some_and(u8::is_ascii_alphabetic) && key.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'));
        let hex = value.len() >= 8 && value.iter().all(|c| c.is_ascii_hexdigit() || *c == b'-');
        let measure = !value.is_empty() && value.iter().all(|c| c.is_ascii_digit() || matches!(c, b'x' | b'+' | b'-')) && value.iter().any(u8::is_ascii_digit);
        if ident && byte_classes(key) <= 2 && (hex || measure) {
            return false;
        }
    }
    entropy(run) > 4.0
}

/// Bezeichner aus Wörtern: Stücke zwischen `_`, `.`, `-` und an CamelCase-Grenzen (`CPUUsage` = `CPU` + `Usage`) aus
/// Buchstaben, mindestens eines lang (ein Akronym aus mindestens zwei Großbuchstaben oder ein Wort mit mindestens zwei
/// Kleinbuchstaben, groß- oder kleingeschrieben) und höchstens so viele kurze (ein, zwei Buchstaben: `By`, `In`, `U`) wie
/// lange: `ExecMainStartTimestamp`, `DropInPaths`, `TimeoutStartUSec`, `NRestarts`, `DB_HOST`; dazu Namen aus höchstens
/// zwei Zeichen (`Id`). Ziffern machen ihn ungültig, ebenso zufällige Buchstaben, die die Schreibung nach ein, zwei
/// Zeichen wechseln (`XkRpQmZw`: nur kurze Stücke).
fn word_identifier(key: &[u8]) -> bool {
    if key.is_empty() || !key[0].is_ascii_alphabetic() {
        return false;
    }
    let (mut long, mut short) = (0usize, 0usize);
    for part in key.split(|c| matches!(c, b'_' | b'.' | b'-')) {
        if part.is_empty() || !part.iter().all(u8::is_ascii_alphabetic) {
            return false;
        }
        let mut start = 0;
        let mut pieces = Vec::new();
        for i in 1..part.len() {
            let upper = part[i].is_ascii_uppercase();
            let after_lower = part[i - 1].is_ascii_lowercase();
            let acronym_end = part[i - 1].is_ascii_uppercase() && part.get(i + 1).is_some_and(u8::is_ascii_lowercase);
            if upper && (after_lower || acronym_end) {
                pieces.push(&part[start..i]);
                start = i;
            }
        }
        pieces.push(&part[start..]);
        for p in pieces {
            let acronym = p.len() >= 2 && p.iter().all(u8::is_ascii_uppercase);
            let rest = if p[0].is_ascii_uppercase() { &p[1..] } else { p };
            if acronym || rest.len() >= 2 {
                long += 1;
            } else {
                short += 1;
            }
        }
    }
    (long >= 1 && short <= long) || key.len() <= 2
}

/// Zuweisung einer Zeile von `systemctl show` und Ähnlichem als ein Base64-Lauf: Name aus Wörtern
/// ([`word_identifier`]), `=`, ein Wert aus Wörtern und Zahlen zwischen `/`, `-`, `_` (`ExecMainStartTimestamp=Thu`,
/// `FragmentPath=/etc/systemd/system/wg-quick`). Ein leerer Wert zählt nicht (Base64 mit `=` am Ende), ebenso ein Wert
/// mit gemischter Schreibung mitten im Wort oder mit Ziffern im Wort (`Name=Xk9pQ2…`); er bleibt ein Fall für die
/// Entropieprüfung.
fn word_assignment(run: &[u8]) -> bool {
    let Some(eq) = run.iter().position(|&c| c == b'=') else { return false };
    let (key, value) = (&run[..eq], &run[eq + 1..]);
    let word = |p: &[u8]| {
        let letters = p.iter().all(u8::is_ascii_alphabetic);
        let lower = |q: &[u8]| q.iter().all(u8::is_ascii_lowercase);
        p.iter().all(u8::is_ascii_digit) || (letters && (lower(p) || p.iter().all(u8::is_ascii_uppercase) || (p[0].is_ascii_uppercase() && lower(&p[1..]))))
    };
    let mut pieces = value.split(|&c| matches!(c, b'/' | b'-' | b'_')).filter(|p| !p.is_empty()).peekable();
    word_identifier(key) && pieces.peek().is_some() && pieces.all(word) && value.iter().any(u8::is_ascii_alphabetic)
}

/// Shannon-Entropie in Bit je Zeichen
fn entropy(s: &[u8]) -> f64 {
    let mut counts = [0u32; 256];
    for &c in s {
        counts[c as usize] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// Zeichenklassen (klein, groß, Ziffer, sonstiges) in ASCII
fn byte_classes(s: &[u8]) -> usize {
    let has = |f: fn(&u8) -> bool| usize::from(s.iter().any(f));
    has(u8::is_ascii_lowercase) + has(u8::is_ascii_uppercase) + has(u8::is_ascii_digit) + has(|c| !c.is_ascii_alphanumeric())
}

/// Zeichenklassen eines Wortes auch für Nicht-ASCII (`Größe2024!`). Wortverbinder `- _ / . :` zählen nicht als
/// Sonderzeichen: `OpenAI-kompatibel` oder `PipeWire-Default` haben zwei Klassen, nicht drei.
fn char_classes(s: &str) -> usize {
    let has = |f: fn(char) -> bool| usize::from(s.chars().any(f));
    has(char::is_lowercase) + has(char::is_uppercase) + has(|c| c.is_ascii_digit()) + has(|c| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '/' | '.' | ':'))
}

/// Ein Stück eines zusammengesetzten Wortes: nur Ziffern, klein (mit Ziffern: `v3`, `x86`, `größe`), groß (`GNU`)
/// oder großgeschrieben (`Mail`, `Prüfung`). Zufällige Base64-Stücke mischen Groß und Klein fast immer mittendrin.
fn wordish(piece: &str) -> bool {
    let lower = |s: &str| s.chars().all(|c| c.is_lowercase() || c.is_ascii_digit());
    let upper = piece.chars().all(|c| c.is_uppercase() || c.is_ascii_digit());
    let mut cs = piece.chars();
    upper || lower(piece) || (cs.next().is_some_and(char::is_uppercase) && lower(cs.as_str()))
}

/// Trenner in Bezeichnern und Base64-Läufen
fn run_sep(c: char) -> bool {
    matches!(c, '-' | '_' | '/' | '+' | '=' | '.')
}

/// Anzahl der Stücke zwischen Trennern, wenn alle [`wordish`] sind (`CARGO_PKG_VERSION`, `E-Mail-Adresse-2024`,
/// `src/pipeline/ai`, `ggml-large-v3-turbo-q5_0`, `Groß/klein`), sonst 0.
fn word_pieces(s: &str, sep: fn(char) -> bool) -> usize {
    let mut pieces = 0;
    for seg in s.split(sep).filter(|p| !p.is_empty()) {
        if !wordish(seg) {
            return 0;
        }
        pieces += 1;
    }
    pieces
}

/// Aus mindestens zwei Wörtern zusammengesetzt (siehe [`word_pieces`]); `run` ist ASCII
fn word_like(run: &[u8]) -> bool {
    std::str::from_utf8(run).is_ok_and(|s| word_pieces(s, run_sep) >= 2)
}

/// Passphrase aus einem Generator mit angehängter Ziffer (Bitwarden/Vaultwarden „Include number“ hängt genau eine
/// Ziffer an ein zufälliges Wort): mindestens drei Wörter aus 3 bis 9 ASCII-Buchstaben (Länge der Wörter in der
/// EFF-Liste der Generatoren), alle klein oder alle großgeschrieben, ein einheitlicher Trenner, genau eine Ziffer.
fn generated_passphrase(t: &str) -> bool {
    let Some(sep) = t.chars().find(|c| !c.is_ascii_alphanumeric()) else {
        return false;
    };
    // `_` (snake_case mit `ipv4`, `ext4`), `.` (Hostnamen), `/ \ :` (Pfade, Tags), `@` (E-Mail, Pakete)
    if !sep.is_ascii() || sep.is_ascii_whitespace() || matches!(sep, '_' | '.' | '/' | '\\' | ':' | '@') {
        return false;
    }
    let words: Vec<&str> = t.split(sep).collect();
    if words.len() < 3 {
        return false;
    }
    let mut digits = 0;
    let mut capitalized = 0;
    for w in &words {
        let letters = w.strip_suffix(|c: char| c.is_ascii_digit()).unwrap_or(w);
        digits += w.len() - letters.len();
        if !(3..=9).contains(&letters.len()) || !letters.bytes().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let rest = &letters[1..];
        if !rest.bytes().all(|c| c.is_ascii_lowercase()) {
            return false;
        }
        capitalized += usize::from(letters.as_bytes()[0].is_ascii_uppercase());
    }
    digits == 1 && (capitalized == 0 || capitalized == words.len())
}

/// Wortzahl, die der Generator erlaubt (Bitwarden/Vaultwarden: 3 bis 20)
const PASSPHRASE_WORDS: std::ops::RangeInclusive<usize> = 3..=20;
/// Längster Trenner in Zeichen. Die Bitwarden-Oberfläche nimmt eins, der Generator (SDK) jede Zeichenkette.
const MAX_SEPARATOR_CHARS: usize = 8;
/// Längste Passphrase in Byte: 20 Wörter mit je 9 Zeichen und einer Ziffer, 19 Trenner aus je 8 Zeichen zu 4 Byte
const MAX_PASSPHRASE_BYTES: usize = 20 * 10 + 19 * MAX_SEPARATOR_CHARS * 4;

/// Passphrase aus dem Generator von Bitwarden/Vaultwarden (und jedem anderen, der die EFF-Liste nutzt): 3 bis 20 Wörter
/// der EFF Large Wordlist ([`wordlist`]) mit einem einheitlichen Trenner. Der Generator zieht die Wörter zufällig,
/// schreibt auf Wunsch bei allen den ersten Buchstaben groß (alle oder keins) und hängt auf Wunsch genau eine Ziffer an
/// ein Wort (höchstens eine). Der Trenner ist beliebig, nur nicht leer (laut SDK-Dokumentation darf er das nicht sein)
/// und nicht aus Buchstaben oder Ziffern. Erkannt werden Trenner aus 1 bis 8 Zeichen (die
/// Oberfläche nimmt eins) ohne Leerraum, dazu das einzelne Leerzeichen (KeePassXC-Vorgabe, SDK-Vorgabe); Trenner wie `, `
/// und ` - ` sind Aufzählungen und Fließtext (`Clone, Copy, Debug` besteht aus Listenwörtern). Wörter, die alle gleich
/// sind, ergibt der Zufall nie (1 zu 60 Millionen bei drei Wörtern): `false, false, false` ist keine Passphrase.
///
/// Die Liste enthält nicht jedes geläufige Wort: `correct-battery-staple` ist eine Passphrase, `correct-horse-battery-staple`
/// (das Beispiel aus xkcd) nicht, denn `horse` steht nicht darin. `kebab-case-name` besteht aus Listenwörtern und gilt
/// darum als Passphrase (Fehlalarm, in Kauf genommen), `Access-Control-Allow-Origin`, `rust-analyzer` und
/// `--no-default-features` nicht: dort fehlt ein Wort.
fn eff_passphrase(t: &str) -> bool {
    if t.len() > MAX_PASSPHRASE_BYTES {
        return false;
    }
    // Kandidaten für den Trenner sind die verschiedenen Läufe aus Zeichen, die keine Buchstaben oder Ziffern sind. Mehr als
    // zwei kann eine Passphrase nicht haben: den Trenner und den Bindestrich in `t-shirt` bei einem anderen Trenner.
    let mut seps = [""; 2];
    let mut n = 0;
    for run in t.split(char::is_alphanumeric).filter(|r| !r.is_empty()) {
        if !seps[..n].contains(&run) {
            if n == seps.len() {
                return false;
            }
            seps[n] = run;
            n += 1;
        }
    }
    seps[..n].iter().any(|sep| split_passphrase(t, sep))
}

/// Ist `t` eine Passphrase mit diesem Trenner?
fn split_passphrase(t: &str, sep: &str) -> bool {
    // Leerraum trennt nur als einzelnes Leerzeichen: `, `, ` - ` und Zeilenumbrüche sind Listen und Fließtext
    let chars = sep.chars().count();
    if !(1..=MAX_SEPARATOR_CHARS).contains(&chars) || sep.chars().any(char::is_control) || (sep != " " && sep.chars().any(char::is_whitespace)) {
        return false;
    }
    // `self.name.clone`: der Empfänger in Rust und Python. `self` steht in der Liste, und 448 von 543 Punktketten aus
    // Listenwörtern in 25000 Ketten der Crate-Quellen beginnen so; eine Passphrase beginnt nur zu 1 von 7776 mit `self`.
    if sep == "." && t.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("self.")) {
        return false;
    }
    let hyphen = sep == "-";
    let pieces: Vec<&str> = t.split(sep).collect();
    // Stücke bestehen aus ASCII-Buchstaben und -Ziffern; der Bindestrich von `t-shirt` kommt nur vor, wenn er nicht trennt
    if pieces.iter().any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_alphanumeric() || (c == b'-' && !hyphen))) {
        return false;
    }
    // Mit `-` als Trenner zerfallen `drop-down`, `felt-tip`, `t-shirt` und `yo-yo` der Liste in zwei Stücke
    passphrase_words(&pieces, hyphen, false) || (hyphen && passphrase_words(&pieces, true, true))
}

/// Alle Stücke sind Wörter der Liste und bilden zusammen eine Passphrase. Mit `hyphen` darf ein Stück mit dem nächsten
/// ein Wort der Liste mit Bindestrich bilden: wenn es allein keins ist (`yo`), bei `always` auch wenn es allein eins ist
/// (`drop`, das Wort `drop-down` ist aber auch als `drop` und `down` erzeugbar, sodass beide Lesarten zu prüfen sind).
fn passphrase_words(pieces: &[&str], hyphen: bool, always: bool) -> bool {
    let (mut words, mut digits, mut upper) = (0, 0, 0);
    let mut first: Option<String> = None;
    let mut all_same = true;
    let mut i = 0;
    while i < pieces.len() {
        let mut shape = list_word(pieces[i]);
        let mut used = 1;
        if hyphen && i + 1 < pieces.len() && (shape.is_none() || always) {
            if let Some(s) = list_word(&format!("{}-{}", pieces[i], pieces[i + 1])) {
                shape = Some(s);
                used = 2;
            }
        }
        let Some(s) = shape else { return false };
        i += used;
        words += 1;
        digits += usize::from(s.digit);
        upper += usize::from(s.upper);
        all_same &= first.get_or_insert_with(|| s.word.clone()) == &s.word;
    }
    PASSPHRASE_WORDS.contains(&words) && digits <= 1 && (upper == 0 || upper == words) && !all_same
}

/// Wie ein Stück aussieht, das ein Wort der Liste ist
struct Shape {
    /// das Wort der Liste in Kleinbuchstaben, ohne Ziffer
    word: String,
    /// eine Ziffer angehängt (`battery7`)
    digit: bool,
    /// erster Buchstabe groß (`Battery`)
    upper: bool,
}

/// Wort der Liste, erster Buchstabe groß oder klein, der Rest klein, höchstens eine angehängte Ziffer
fn list_word(piece: &str) -> Option<Shape> {
    let (letters, digit) = match piece.strip_suffix(|c: char| c.is_ascii_digit()) {
        Some(l) => (l, true),
        None => (piece, false),
    };
    let (first, rest) = letters.as_bytes().split_first()?;
    if !first.is_ascii_alphabetic() || !rest.iter().all(|c| c.is_ascii_lowercase() || *c == b'-') {
        return None;
    }
    wordlist::contains(letters).then(|| Shape { word: letters.to_ascii_lowercase(), digit, upper: first.is_ascii_uppercase() })
}

/// Kann ein einzelnes markiertes Wort ein Passwort sein? Nein bei URL, Pfad, E-Mail, Code, CLI-Option, Version,
/// Datum/Zeit, Dateiname, Umgebungszuweisung, Bindestrich-Wörtern und gepunkteten Bezeichnern.
fn password_shaped(t: &str) -> bool {
    let b = t.as_bytes();
    if t.contains("://") || t.starts_with("www.") || t.starts_with(['/', '~', '\\']) || t.starts_with("./") || t.starts_with("../") || t.starts_with("--") {
        return false;
    }
    // Windows-Pfad `C:\…`
    if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        return false;
    }
    // Code
    if ["::", "()", "->", "=>", "</", "/>", "#[", "!("].iter().any(|m| t.contains(m)) || t.ends_with(';') {
        return false;
    }
    // Klammern aller Art: Code (`Result<Value`, `Ok(Target`, `Vec<u8>`)
    if t.contains(['(', ')', '[', ']', '{', '}', '<', '>', '`']) {
        return false;
    }
    // E-Mail: lokal@domain.tld
    if let Some((local, domain)) = t.split_once('@') {
        let tld = domain.rsplit('.').next().unwrap_or("");
        if !local.is_empty() && domain.contains('.') && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()) && !domain.contains('@') {
            return false;
        }
    }
    // Umgebungszuweisung `CMAKE_BUILD_TYPE=Release` (mit Schlüsselwort greift die Zuweisungsprüfung)
    if let Some((k, _)) = t.split_once('=') {
        if k.len() >= 2 && k.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') {
            return false;
        }
    }
    // Datum/Zeit: nur Ziffern und `-:.+TZ/`
    if t.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'-' | b':' | b'.' | b'+' | b'/' | b'T' | b'Z' | b'_')) {
        return false;
    }
    // Version `v1.2.3-beta.1`
    let v = t.strip_prefix(['v', 'V']).unwrap_or(t);
    if v.starts_with(|c: char| c.is_ascii_digit()) && v.contains('.') && v.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+')) {
        let core = v.split(['-', '+']).next().unwrap_or("");
        if core.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
            return false;
        }
    }
    // Aus Wörtern zusammengesetzt zu sein ist hier bewusst keine Ausnahme mehr: `Sommer-2024` ist so gebaut wie
    // `Sommer2024`. Bezeichner aus Kleinbuchstaben und Ziffern scheitern schon an den drei Zeichenklassen.
    // Verweise auf Variablen und Referenzen (`$XDG_RUNTIME_DIR/…`, `&Harness`)
    if t.starts_with(['$', '&']) {
        return false;
    }
    // UUID, Hex, Digest `sha256:…`
    if b.iter().all(|c| c.is_ascii_hexdigit() || *c == b'-') {
        return false;
    }
    if let Some((algo, hex)) = t.rsplit_once(':') {
        if hex.len() >= 16 && hex.bytes().all(|c| c.is_ascii_hexdigit()) && algo.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return false;
        }
    }
    if t.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')) {
        let dot_segs: Vec<&str> = t.split('.').collect();
        // Dateiname `Cargo.toml`, `report_v2.tar.gz`: Endung 1–5 Zeichen mit Buchstabe
        if dot_segs.len() >= 2 {
            let ext = dot_segs[dot_segs.len() - 1];
            if (1..=5).contains(&ext.len())
                && ext.bytes().any(|c| c.is_ascii_alphabetic())
                && ext.bytes().all(|c| c.is_ascii_alphanumeric())
                && !dot_segs[0].is_empty()
            {
                return false;
            }
            // gepunktete Bezeichner `std.io.Read`, `Max.Mustermann`
            let ident = |s: &&str| s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
            if dot_segs.iter().all(ident) {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------------------------
// Bytes

fn is_tok(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

fn is_key(c: u8) -> bool {
    is_tok(c) || c == b'.'
}

fn is_b64(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'=' | b'_' | b'-')
}

fn is_cred(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'~' | b'+' | b'/' | b'=' | b'-' | b':')
}

fn run_end(b: &[u8], start: usize, pred: impl Fn(u8) -> bool) -> usize {
    start + b[start.min(b.len())..].iter().position(|&c| !pred(c)).unwrap_or(b.len().saturating_sub(start))
}

fn skip(b: &[u8], start: usize, pred: impl Fn(u8) -> bool) -> usize {
    run_end(b, start, pred)
}

fn line_end(b: &[u8], from: usize) -> usize {
    from + b[from..].iter().position(|&c| c == b'\n').unwrap_or(b.len() - from)
}

/// Erstes Vorkommen von `needle` ab `from` (Muster sind kurz, also linear)
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    let first = needle[0];
    let mut i = from;
    while i + needle.len() <= hay.len() {
        match hay[i..hay.len() - needle.len() + 1].iter().position(|&c| c == first) {
            Some(p) => {
                i += p;
                if &hay[i..i + needle.len()] == needle {
                    return Some(i);
                }
                i += 1;
            }
            None => return None,
        }
    }
    None
}

/// Wie [`find`], ASCII ohne Groß/klein (`needle` klein)
fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures werden zusammengesetzt, damit im Repo keine Zeichenkette wie ein echter Schlüssel aussieht.
    fn rep(s: &str, n: usize) -> String {
        s.repeat(n)
    }

    /// Pseudozufällige Base62-Zeichenkette (fest, reproduzierbar)
    fn rnd(n: usize, seed: u64) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                A[(x % 62) as usize] as char
            })
            .collect()
    }

    /// Was für jeden Text gelten muss: die Arten von `redact` sind die von `scan`, der geschwärzte Text ist für `scan`
    /// leer, die Fundstellen liegen auf Zeichengrenzen und tragen Arten aus `KINDS`.
    /// Im Modus `Log` (der Vorgabe von `redact`) ist der geschwärzte Text für `scan` und für den Modus selbst leer.
    fn redact_agrees(text: &str) -> Result<(), String> {
        let r = redact_with(text, Strictness::Selection);
        let want = scan(text);
        if r.kind_names() != want {
            return Err(format!("{text:?}: redact kinds {:?}, scan {want:?}", r.kinds));
        }
        let again = scan(&r.text);
        if !again.is_empty() {
            return Err(format!("{text:?}: redacted {:?} still has {again:?}", r.text));
        }
        let log = redact(text);
        let again = scan(&log.text);
        if !again.is_empty() || !spans_in(&log.text, Strictness::Log).is_empty() {
            return Err(format!("{text:?}: log mode {:?} still has {again:?}", log.text));
        }
        if !want.is_empty() && log.total() == 0 {
            return Err(format!("{text:?}: log mode found nothing, scan {want:?}"));
        }
        for f in findings(text) {
            if !KINDS.contains(&f.kind) || f.start >= f.end || text.get(f.start..f.end).is_none() {
                return Err(format!("{text:?}: bad finding {f:?}"));
            }
        }
        if want.is_empty() && r.text != text {
            return Err(format!("{text:?}: changed without a finding"));
        }
        Ok(())
    }

    fn check(cases: &[(String, &[&str])]) {
        let mut bad = Vec::new();
        for (text, want) in cases {
            let got = scan(text);
            if got != *want {
                bad.push(format!("{text:?}\n   want {want:?}\n   got  {got:?}"));
            }
            if let Err(e) = redact_agrees(text) {
                bad.push(e);
            }
        }
        assert!(bad.is_empty(), "{} of {} cases wrong:\n{}", bad.len(), cases.len(), bad.join("\n"));
    }

    #[test]
    fn positives() {
        let pem_body = format!("{}\n{}\n", rnd(64, 1), rnd(64, 2));
        let cases: Vec<(String, &[&str])> = vec![
            (format!("-----BEGIN RSA PRIVATE KEY-----\n{pem_body}-----END RSA PRIVATE KEY-----"), &["private_key"]),
            (format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{pem_body}-----END OPENSSH PRIVATE KEY-----\n"), &["private_key"]),
            (format!("key:\n-----BEGIN PRIVATE KEY-----\n{pem_body}"), &["private_key"]),
            (format!("-----BEGIN ENCRYPTED PRIVATE KEY-----\n{pem_body}-----END ENCRYPTED PRIVATE KEY-----"), &["private_key"]),
            (format!("-----BEGIN EC PRIVATE KEY-----\n{pem_body}-----END EC PRIVATE KEY-----"), &["private_key"]),
            (format!("-----BEGIN PGP PRIVATE KEY BLOCK-----\n\n{pem_body}-----END PGP PRIVATE KEY BLOCK-----"), &["private_key"]),
            (
                format!("PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\nPrivate-Lines: 1\n{}\nPrivate-MAC: {}", rnd(44, 3), rep("ab12", 16)),
                &["private_key"],
            ),
            (format!("AGE-SECRET-KEY-1{}", rnd(58, 4).to_uppercase()), &["private_key"]),
            (format!("export ANTHROPIC_API_KEY=sk-ant-api03-{}", rnd(40, 5)), &["anthropic_key"]),
            (format!("der Schlüssel sk-ant-admin01-{}-{} ist neu", rnd(30, 6), rnd(10, 7)), &["anthropic_key"]),
            (format!("OPENAI_API_KEY=\"sk-proj-{}\"", rnd(48, 8)), &["openai_key"]),
            (format!("sk-{}", rnd(48, 9)), &["openai_key"]),
            (format!("git clone with ghp_{}", rnd(36, 10)), &["github_token"]),
            (format!("gho_{} and ghs_{}", rnd(36, 11), rnd(36, 12)), &["github_token"]),
            (format!("github_pat_{}_{}", rnd(22, 13), rnd(59, 14)), &["github_token"]),
            (format!("PRIVATE-TOKEN: glpat-{}", rnd(20, 15)), &["gitlab_token"]),
            (format!("aws_access_key_id = AKIA{}", rnd(16, 16).to_uppercase()), &["aws_access_key"]),
            (format!("ASIA{}", rnd(16, 17).to_uppercase()), &["aws_access_key"]),
            (format!("SLACK_BOT=xoxb-{}-{}-{}", "1234567890", "9876543210", rnd(24, 18)), &["slack_token"]),
            (format!("https://maps.googleapis.com/maps/api/js?key=AIza{}", rnd(35, 19)), &["google_api_key"]),
            (format!("Stripe::api_key = sk_live_{}", rnd(24, 20)), &["stripe_key"]),
            (format!("//registry.npmjs.org/:_authToken=npm_{}", rnd(36, 21)), &["api_token"]),
            (format!("HF_TOKEN hf_{}", rnd(34, 22)), &["api_token"]),
            (format!("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.{}", rnd(43, 23)), &["jwt"]),
            (format!("curl -H \"Authorization: Bearer {}\" https://api.example.org/v1", rnd(40, 24)), &["auth_header"]),
            (format!("authorization: Basic {}==", rnd(22, 25)), &["auth_header"]),
            (format!("headers = {{'Authorization': 'token {}'}}", rnd(30, 26)), &["auth_header"]),
            (format!("use bearer {} for that", rnd(20, 27)), &["auth_header"]),
            ("postgres://admin:S3cr3t!pw@db.internal:5432/app".into(), &["url_credentials"]),
            ("https://user:hunter2@git.example.org/repo.git".into(), &["url_credentials"]),
            ("redis://:Tr0ub4dor@cache:6379/0".into(), &["url_credentials"]),
            ("DB_PASSWORD=hunter2".into(), &["secret_assignment"]),
            ("password: correcthorse".into(), &["secret_assignment"]),
            ("  db_pass: Tr0ub4dor&3\n".into(), &["secret_assignment"]),
            ("{\"password\": \"hunter2\", \"user\": \"bob\"}".into(), &["secret_assignment"]),
            ("client_secret = \"abc def ghi\"".into(), &["secret_assignment"]),
            ("apiKey: 'k3y-value'".into(), &["secret_assignment"]),
            ("X-Api-Key: 9f8e7d6c".into(), &["secret_assignment"]),
            ("Server=db;User Id=sa;Password=Pa55w0rd;".into(), &["secret_assignment"]),
            ("Server=db;User Id=sa;Password=geheimnisvoll;".into(), &["secret_assignment"]),
            ("mysql --password Sommer2024 -u root".into(), &["secret_assignment"]),
            ("mysql --password=Sommer2024".into(), &["secret_assignment"]),
            ("https://example.org/cb?code=1&access_token=abc123def&state=x".into(), &["secret_assignment"]),
            ("Mein Passwort: Sommer2024! (bitte nicht weitergeben)".into(), &["secret_assignment"]),
            ("Kennwort=Sonnenblume".into(), &["secret_assignment"]),
            ("secret_id: 6a1c9b2e".into(), &["secret_assignment"]),
            ("- password: hunter2".into(), &["secret_assignment"]),
            ("PASSWORD=12345678".into(), &["secret_assignment"]),
            (format!("signature {}", rnd(40, 28)), &["high_entropy_token"]),
            (format!("value: {}+/{}==", rnd(30, 29), rnd(20, 30)), &["high_entropy_token"]),
            ("Tr0ub4dor&3".into(), &["password"]),
            ("  Sommer2024!\n".into(), &["password"]),
            ("Größe2024?".into(), &["password"]),
            ("p@ssW0rd".into(), &["password"]),
            (rnd(20, 31), &["password"]),
            // mehrere Arten
            (
                format!("ANTHROPIC_API_KEY=sk-ant-api03-{}\nDB_PASSWORD=hunter2\nhttps://u:pw1@h.example", rnd(40, 32)),
                &["anthropic_key", "url_credentials", "secret_assignment"],
            ),
        ];
        check(&cases);
    }

    #[test]
    fn negatives() {
        let rust = r#"
pub fn rewrite_as(cfg: &AiCfg, model: &str, mode: &str, text: &str, cancel: Option<&CancelFlag>) -> Result<String, AiError> {
    let system = cfg.mode_prompt(mode).ok_or_else(|| AiError::other(format!("unknown mode \"{mode}\"")))?;
    let key = key_for(p, cfg)?;
    let token: String = std::env::var("TOKEN").unwrap_or_default();
    let password = read_password()?;
    let secret = secret_value;
    struct Login { password: String, token: Option<Token>, api_key: &'static str }
    fn new(token: String, password: &str) -> Self { Self { token, password: password.into() } }
    if let Some(t) = self.token.as_ref() { headers.insert("authorization", format!("Bearer {t}")); }
    let max_tokens = 4096;
    let tokens: Vec<Token> = lexer.tokenize(&src);
    let api_key_file = "~/.config/anthropic/api-key";
    const MAX_GROWTH: usize = 3;
    HashMap::<String, Vec<u8>>::with_capacity(16);
}
"#;
        let german = "Sehr geehrte Damen und Herren,\n\nvielen Dank für Ihre Nachricht vom 22. Oktober 2026. Die Donaudampfschifffahrtsgesellschaft \
            hat die Änderungen übernommen; Größe, Übergröße und Maßeinheiten wurden angepasst. Ihr Passwort: muss mindestens zwölf Zeichen haben. \
            Das Kennwort ändern Sie unter Einstellungen › Sicherheit. Bei Fragen erreichen Sie uns per E-Mail an service@beispiel.de oder unter \
            https://www.beispiel.de/hilfe/faq?thema=konto#passwort.\n\nMit freundlichen Grüßen\nMax Mustermann";
        let cases: Vec<(String, &[&str])> = vec![
            ("".into(), &[]),
            ("   \n\t ".into(), &[]),
            (german.into(), &[]),
            ("Das ist ein ganz normaler Satz über Tokens, Passwörter und Geheimnisse.".into(), &[]),
            (rust.into(), &[]),
            // Git-SHA, Hashes, UUID
            ("commit 9fceb02d0ae598e95dc970b74767f19372d61af8".into(), &[]),
            ("9fceb02d0ae598e95dc970b74767f19372d61af8".into(), &[]),
            ("sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(), &[]),
            ("E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855".into(), &[]),
            ("550e8400-e29b-41d4-a716-446655440000".into(), &[]),
            ("id = \"550e8400-e29b-41d4-a716-446655440000\"".into(), &[]),
            // Logzeilen
            ("Oct 22 10:00:01 blub sshd[1234]: Failed password for invalid user admin from 192.0.2.7 port 52211 ssh2".into(), &[]),
            ("error: password incorrect".into(), &[]),
            ("Login failed: password incorrect (attempt 3 of 5)".into(), &[]),
            ("password: incorrect".into(), &[]),
            ("Password: required".into(), &[]),
            ("token: expired".into(), &[]),
            ("WARN auth: token expired at 2026-10-22T10:00:00Z, refreshing".into(), &[]),
            ("Authorization failed for request 42".into(), &[]),
            ("Authorization: required".into(), &[]),
            ("the bearer of this letter".into(), &[]),
            // Pfade und URLs ohne Zugangsdaten
            ("/home/blub/.config/anthropic/api-key".into(), &[]),
            ("/tmp/claude-1000/-home-git-blub-dictate/68fdfb7e-7c31-444f-bfbd-6fb1041a5f50/scratchpad/plan.md".into(), &[]),
            ("~/.local/share/dictate/models/ggml-large-v3-turbo-q5_0.bin".into(), &[]),
            ("https://docs.google.com/document/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms/edit".into(), &[]),
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG".into(), &[]),
            ("ssh://git@git.pushingpixels.at:22/daniel/dictate.git".into(), &[]),
            ("git@git.pushingpixels.at:daniel/dictate.git".into(), &[]),
            ("http://127.0.0.1:11434/api/chat".into(), &[]),
            ("postgres://user@localhost/db".into(), &[]),
            ("postgres://user:password@localhost/db".into(), &[]),
            ("postgres://user:${DB_PASSWORD}@localhost/db".into(), &[]),
            // Platzhalter, Verweise, Typen
            ("DB_PASSWORD=${DB_PASSWORD}".into(), &[]),
            ("password = \"\"".into(), &[]),
            ("password: \"<your password>\"".into(), &[]),
            ("API_KEY=your-api-key-here".into(), &[]),
            ("token: ${{ secrets.GITHUB_TOKEN }}".into(), &[]),
            ("password: !vault |".into(), &[]),
            ("password = os.environ[\"DB_PASSWORD\"]".into(), &[]),
            ("secret: ********".into(), &[]),
            ("\"password\": \"Passwort eingeben\"".into(), &[]),
            ("password_file = \"/run/secrets/db\"".into(), &[]),
            ("api_key_file = \"~/.config/anthropic/api-key\"".into(), &[]),
            ("max_tokens: 4096".into(), &[]),
            ("secrets: \"local_only\".into(),".into(), &[]),
            ("tokens: 1500 used".into(), &[]),
            ("token_url: https://auth.example.org/token".into(), &[]),
            ("password_min_length = 12".into(), &[]),
            ("AWS example AKIAIOSFODNN7EXAMPLE".into(), &[]),
            (format!("sk-{}", "this-is-a-long-hyphenated-phrase"), &[]),
            ("risk-assessment-and-mitigation-plan".into(), &[]),
            // öffentliche Schlüssel und Zertifikate
            (format!("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI{} daniel@blub", rnd(43, 40)), &[]),
            (format!("-----BEGIN CERTIFICATE-----\n{}\n{}\n-----END CERTIFICATE-----", rnd(64, 41), rnd(64, 42)), &[]),
            (format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", rnd(64, 43)), &[]),
            (format!("<img src=\"data:image/png;base64,iVBORw0KGgo{}\">", rnd(80, 44)), &[]),
            // Bezeichner und Wörter
            ("CARGO_PKG_VERSION_MAJOR".into(), &[]),
            ("OpenAI-kompatibel,".into(), &[]),
            ("ChatGPT-Recherche".into(), &[]),
            ("„PipeWire-Default“".into(), &[]),
            ("`gray-800`,".into(), &[]),
            ("GRAY_400".into(), &[]),
            ("Groß/klein-unabhängig".into(), &[]),
            ("`qwen2.5:7b-instruct`".into(), &[]),
            ("PartialEq,".into(), &[]),
            ("Result<Value,".into(), &[]),
            ("$XDG_RUNTIME_DIR/dictate/control.sock".into(), &[]),
            ("&Harness,".into(), &[]),
            ("e.sha256".into(), &[]),
            ("**Zwischenablage**".into(), &[]),
            ("x86_64-unknown-linux-gnu".into(), &[]),
            ("HashMap::new()".into(), &[]),
            ("Vec<String>".into(), &[]),
            ("std.io.Read".into(), &[]),
            ("Max.Mustermann@Firma.de".into(), &[]),
            ("Cargo.toml".into(), &[]),
            ("v1.2.3-beta.1".into(), &[]),
            ("2026-10-22T10:00:00Z".into(), &[]),
            ("--release".into(), &[]),
            ("CMAKE_BUILD_TYPE=Release".into(), &[]),
            ("C:\\Users\\Max\\Desktop".into(), &[]),
            ("Hallo".into(), &[]),
            ("Donaudampfschifffahrt".into(), &[]),
            ("Übergrößenträger".into(), &[]),
            ("password".into(), &[]),
        ];
        check(&cases);
    }

    /// Regel 8: eine ganze Markierung aus einem Token (Passwort-Manager, Vaultwarden-Generator) gegen Bezeichner.
    #[test]
    fn single_tokens_and_passphrases() {
        let cases: Vec<(String, &[&str])> = vec![
            // a) drei Zeichenklassen, auch aus Wörtern mit Trennern zusammengesetzt
            ("Sommer-2024".into(), &["password"]),
            ("Sommer_2024".into(), &["password"]),
            ("„Sommer-2024“".into(), &["password"]),
            ("Sommer.2024!".into(), &["password"]),
            ("Hund_Katze_99".into(), &["password"]),
            ("Winter#2025".into(), &["password"]),
            ("geheim!2024".into(), &["password"]),
            ("sommer@2024".into(), &["password"]),
            ("Mausi-1987-Bert".into(), &["password"]),
            // Vaultwarden-Passphrasen: großgeschrieben mit Ziffer (a), nur Ziffer (b), andere Trenner
            ("Correct-Horse-Battery7".into(), &["password"]),
            ("Correct-Horse7-Battery-Staple-Orbit-Lunar".into(), &["password"]),
            ("correct-horse7-battery".into(), &["password"]),
            ("correct-horse-battery-staple3".into(), &["password"]),
            ("correct!horse!battery7".into(), &["password"]),
            ("correct+horse4+battery".into(), &["password"]),
            ("Correct_Horse_Battery7".into(), &["password"]),
            // Generator-Passwort mit Sonderzeichen
            ("x7#Kq!m2Lp9@".into(), &["password"]),
            // in Kauf genommene Fehlalarme (sperren nur die Cloud)
            ("E-Mail-Adresse-Verwaltung-2024-Oktober".into(), &["password"]),
            ("SHA-256-Prüfung".into(), &["password"]),
            ("Qwen2.5-7B-Instruct".into(), &["password"]),
            ("claude-opus-4-5@20251101".into(), &["password"]),
            // Code-Bezeichner bleiben frei
            ("snake_case_name".into(), &[]),
            ("SCREAMING_SNAKE_CASE_2".into(), &[]),
            ("ipv4_route_lookup".into(), &[]),
            ("ext4_file_write_iter".into(), &[]),
            ("ggml-large-v3-turbo".into(), &[]),
            ("x86_64-unknown-linux-gnu".into(), &[]),
            ("python3-setuptools-scm".into(), &[]),
            ("web1.example.com".into(), &[]),
            ("lib32-glibc-devel".into(), &[]),
            ("camelCaseName".into(), &[]),
            ("PascalCaseName".into(), &[]),
            ("max_tokens_per_request".into(), &[]),
            ("--max-tokens=4096".into(), &[]),
            ("Access-Control-Allow-Origin".into(), &[]),
            ("Max-Planck-Institut".into(), &[]),
            // `horse` steht nicht in der EFF-Liste: kein Generator erzeugt diese Wörter, sie sind von kebab-case und
            // Kopfzeilen nicht zu unterscheiden (die Regel für die EFF-Liste erkennt nur deren Wörter)
            ("correct-horse-battery-staple".into(), &[]),
            ("Correct-Horse-Battery-Staple".into(), &[]),
            // Passphrasen-Form, aber keine: zwei Ziffern, Wort zu kurz/lang, gemischte Trenner, nur zwei Wörter
            ("correct-horse77-battery".into(), &[]),
            ("use-ab1-battery".into(), &[]),
            ("correct-horse-batteryzzz9".into(), &[]),
            ("correct-horse_battery7".into(), &[]),
            ("correct-horse7".into(), &[]),
            ("Correct-horse-Battery7".into(), &["password"]),
        ];
        check(&cases);
    }

    /// `n` verschiedene Wörter der Liste (die ersten der Datei)
    fn distinct(n: usize, sep: &str, capitalize: bool) -> String {
        let words: Vec<String> = wordlist::words().iter().take(n).map(|w| if capitalize { w[..1].to_uppercase() + &w[1..] } else { w.to_string() }).collect();
        words.join(sep)
    }

    /// Passphrasen des Generators (EFF-Liste) in allen Schreibweisen; Wörter müssen in der Liste stehen (`horse` nicht).
    #[test]
    fn eff_passphrases() {
        let pass: &[&str] = &["password"];
        let cases: Vec<(String, &[&str])> = vec![
            // Standard des Generators: klein, Trenner `-`, ohne Ziffer; 3 bis 20 Wörter
            ("correct-battery-staple".into(), pass),
            ("lunar-sample-case-name".into(), pass),
            (distinct(20, "-", false), pass),
            (distinct(20, " ", true), pass),
            // Großschreibung (alle Wörter), Ziffer an einem Wort, beides
            ("Correct-Battery-Staple".into(), pass),
            ("correct-battery5-staple".into(), pass),
            ("correct-battery-staple0".into(), pass),
            ("Correct-Battery-Staple7".into(), pass),
            ("Correct-Battery7-Staple-Lunar-Sample".into(), pass),
            // beliebiger Trenner, auch Leerzeichen (KeePassXC-Vorgabe) und mehrere Zeichen
            ("correct battery staple".into(), pass),
            ("Correct Battery Staple Lunar Sample Case Name".into(), pass),
            ("correct.battery.staple".into(), pass),
            ("self-battery-staple".into(), pass),
            ("self battery staple".into(), pass),
            ("self_battery_staple".into(), pass),
            ("correct_battery_staple".into(), pass),
            ("correct/battery/staple".into(), pass),
            ("correct:battery:staple".into(), pass),
            ("correct@battery@staple".into(), pass),
            ("correct\\battery\\staple".into(), pass),
            ("correct,battery,staple".into(), pass),
            ("correct;battery;staple".into(), pass),
            ("correct!battery!staple".into(), pass),
            ("correct+battery+staple".into(), pass),
            ("correct#battery#staple".into(), pass),
            ("correct|battery|staple".into(), pass),
            ("correct§battery§staple".into(), pass),
            ("correct–battery–staple".into(), pass),
            ("correct::battery::staple".into(), pass),
            ("correct😀battery😀staple".into(), pass),
            // Anführungszeichen und Satzzeichen außen gehören nicht dazu
            ("\"correct battery staple\"".into(), pass),
            ("„Correct-Battery-Staple“".into(), pass),
            ("correct-battery-staple.".into(), pass),
            ("  correct battery staple \n".into(), pass),
            // Wörter der Liste mit Bindestrich (alte Clients): als Stücke oder zusammen
            ("correct-yo-yo-staple".into(), pass),
            ("Correct-Yo-yo5-Staple".into(), pass),
            ("correct-t-shirt-staple".into(), pass),
            ("correct-drop-down-staple".into(), pass),
            ("Correct-Drop-down-Staple".into(), pass),
            ("Correct-Felt-tip-Staple".into(), pass),
            ("Correct_T-shirt_Staple".into(), pass),
            ("correct yo-yo5 staple".into(), pass),
            // keine Passphrase: zwei Wörter, mehr als 20, gemischte Trenner, gemischte Großschreibung, zwei Ziffern,
            // Ziffer vorn oder in der Mitte, ein Wort nicht in der Liste, Großbuchstaben, Umlaut
            ("correct-battery".into(), &[]),
            ("correct battery".into(), &[]),
            (distinct(21, "-", false), &[]),
            (distinct(21, " ", false), &[]),
            // dasselbe Wort immer wieder ergibt der Zufall nie
            ("lunar-lunar-lunar".into(), &[]),
            ("Lunar Lunar Lunar Lunar".into(), &[]),
            ("false, false, false".into(), &[]),
            ("Clone, Copy, Debug".into(), &[]),
            ("correct - battery - staple".into(), &[]),
            ("correct, battery, staple".into(), &[]),
            ("correct-battery_staple".into(), &[]),
            ("correct-battery staple".into(), &[]),
            ("Correct-battery-Staple".into(), &[]),
            ("correct-battery5-staple7".into(), &[]),
            ("correct-battery55-staple".into(), &[]),
            ("5correct-battery-staple".into(), &[]),
            ("correct-bat5tery-staple".into(), &[]),
            ("correct-battery-staple-xyzzy".into(), &[]),
            ("correct-battery-horse".into(), &[]),
            ("CORRECT-BATTERY-STAPLE".into(), &[]),
            ("correct-batterÿ-staple".into(), &[]),
            ("correct-battery-sta-ple".into(), &[]),
            // Zeilen, Tabellen und Listen sind keine Passphrase
            ("correct\nbattery\nstaple".into(), &[]),
            ("correct\tbattery\tstaple".into(), &[]),
            ("correct\r\nbattery\r\nstaple".into(), &[]),
            ("correct  battery  staple".into(), &[]),
            ("-correct-battery-staple".into(), &[]),
            ("correct-battery-staple-".into(), &[]),
            ("correct--battery-staple".into(), &[]),
            ("correct-battery-staple and lunar".into(), &[]),
            // zu langer Trenner
            ("correct~~~~~~~~~battery~~~~~~~~~staple".into(), &[]),
            // Wörter ohne Trenner sind kein Fall für die Liste (der Generator verlangt einen Trenner)
            ("correctbatterystaple".into(), &[]),
            // `self.` vorn: Empfänger in Rust und Python
            ("self.battery.staple".into(), &[]),
        ];
        check(&cases);
    }

    /// Kleiner Zufallsgenerator (xorshift64) für Passphrasen, ohne Abhängigkeit
    fn next(x: &mut u64) -> usize {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        (*x >> 11) as usize
    }

    /// So baut der Generator eine Passphrase: Wörter ziehen (mit Wiederholung), eine Ziffer an ein Wort, erster Buchstabe
    /// groß, Trenner dazwischen. `old_client` zieht auch die vier Wörter mit Bindestrich (das SDK filtert sie).
    fn generate(x: &mut u64, n: usize, sep: &str, capitalize: bool, number: bool, old_client: bool) -> String {
        let pool: Vec<&str> = wordlist::words().iter().copied().filter(|w| old_client || !w.contains('-')).collect();
        let mut words: Vec<String> = (0..n).map(|_| pool[next(x) % pool.len()].to_string()).collect();
        if old_client && next(x).is_multiple_of(2) {
            // mindestens ein Wort mit Bindestrich, sonst käme es fast nie vor (4 von 7776)
            let hyphenated = ["drop-down", "felt-tip", "t-shirt", "yo-yo"][next(x) % 4];
            let i = next(x) % n;
            words[i] = hyphenated.to_string();
        }
        if number {
            let i = next(x) % n;
            words[i].push(char::from(b'0' + (next(x) % 10) as u8));
        }
        if capitalize {
            for w in &mut words {
                w[..1].make_ascii_uppercase();
            }
        }
        words.join(sep)
    }

    /// Jede Passphrase, die der Generator erzeugen kann, wird erkannt: alle Wortzahlen, Schreibweisen, Trenner.
    #[test]
    fn every_generated_passphrase_is_detected() {
        let seps = [
            "-", " ", ".", "_", ",", ";", ":", "/", "\\", "@", "#", "!", "+", "=", "*", "~", "|", "$", "%", "^", "&", "?", "'", "\"", "(", ")", "[", "]", "{",
            "}", "<", ">", "§", "·", "→", "–", "…", "😀", "::", "--", "=>", "///",
        ];
        let mut x = 0x9e37_79b9_7f4a_7c15_u64;
        let mut count = 0;
        for old_client in [false, true] {
            for sep in seps {
                for n in [3, 4, 5, 6, 7, 10, 20] {
                    for (capitalize, number) in [(false, false), (true, false), (false, true), (true, true)] {
                        for _ in 0..6 {
                            let p = generate(&mut x, n, sep, capitalize, number, old_client);
                            assert!(eff_passphrase(&p), "{p:?} (n={n}, sep={sep:?}, caps={capitalize}, number={number}, old={old_client})");
                            // `scan` kennt zusätzlich die äußere Trimmung; eine andere Art darf zuerst melden, gemeldet wird immer
                            assert!(!scan(&p).is_empty(), "{p:?}");
                            redact_agrees(&p).unwrap();
                            // ein Wort außerhalb der Liste, und es ist keine Passphrase mehr
                            let mut pieces: Vec<&str> = p.split(sep).collect();
                            let i = next(&mut x) % pieces.len();
                            pieces[i] = if capitalize { "Qqqqq" } else { "qqqqq" };
                            let bad = pieces.join(sep);
                            assert!(!eff_passphrase(&bad), "{bad:?}");
                            count += 1;
                        }
                    }
                }
            }
        }
        assert!(count > 10_000);
    }

    /// Typische Bezeichner bleiben frei; dazu die bekannten Fehlalarme, die eine Passphrase gleichen (sperren nur die Cloud).
    #[test]
    fn code_identifiers_next_to_passphrases() {
        let free = [
            // CSS-Klassen
            "btn-primary-large",
            "text-center-block",
            "navbar-expand-lg",
            "card-header-title",
            "list-group-item",
            "col-md-6",
            "justify-content-between",
            "align-items-center",
            "font-weight-bold",
            "text-decoration-none",
            "sr-only-focusable",
            "visually-hidden",
            "min-h-screen",
            "space-y-4",
            // Optionen, Pakete, Bezeichner
            "--no-default-features",
            "--all-targets",
            "--force-with-lease",
            "--no-verify",
            "rust-analyzer",
            "python-setuptools",
            "xdg-desktop-portal",
            "network-manager-applet",
            "cargo-watch",
            "noto-fonts-emoji",
            "base-devel",
            "docker-compose",
            "serde_json",
            "tokio::sync::mpsc",
            // HTTP-Kopfzeilen (außer Strict-Transport-Security, siehe unten)
            "Access-Control-Allow-Origin",
            "Content-Security-Policy",
            "Cache-Control",
            "Content-Type",
            "Accept-Encoding",
            "X-Forwarded-For",
            "Cross-Origin-Resource-Policy",
            "Access-Control-Allow-Credentials",
            "If-None-Match",
            "WWW-Authenticate",
            // Empfänger `self` (Rust, Python): mit Abstand häufigste Punktkette aus Listenwörtern
            "self.net.request",
            "self.path.display",
            "Self.name.clone",
            // Pfade, Hostnamen, Versionen
            "/usr/share/applications",
            "usr/share/applications",
            "src/actions/secrets",
            "web1.example.com",
            "v1.2.3-beta.1",
            "x86_64-unknown-linux-gnu",
            // Prosa aus Wörtern außerhalb der Liste (Artikel, Pronomen, Hilfsverben fehlen darin)
            "Das ist ein Test",
            "This is a test",
            "the quick brown fox",
            "Ich habe keine Zeit",
            "Hello world again",
        ];
        for t in free {
            assert_eq!(scan(t), Vec::<&str>::new(), "{t:?}");
            redact_agrees(t).unwrap();
        }
        let alarm = [
            "kebab-case-name",
            "Strict-Transport-Security",
            "footer-widget-area",
            "clear_phrase_state",
            "Delete Last Word",
            "linked-hash-map",
            "target_pointer_width",
            "device.vendor.name",
            "crate::buffer::open",
        ];
        for t in alarm {
            assert_eq!(scan(t), ["password"], "{t:?}");
            redact_agrees(t).unwrap();
            assert_eq!(redact(t).text.trim(), "[REDACTED:password]", "{t:?}");
        }
    }

    /// Fehlalarm-Messung für die EFF-Regel (`cargo test --release --lib false_alarm_report -- --ignored --nocapture`):
    /// `DICTATE_SECRETS_CORPUS` nennt Dateien (getrennt durch `:`), jede Zeile gilt als ganze Markierung, am besten
    /// verschieden (`sort -u`). Je Datei: Zeilen, davon von `scan` gemeldet, davon von der EFF-Regel, dazu deren häufigste
    /// Treffer.
    #[test]
    #[ignore = "Messung, braucht DICTATE_SECRETS_CORPUS"]
    fn false_alarm_report() {
        let files = std::env::var("DICTATE_SECRETS_CORPUS").expect("DICTATE_SECRETS_CORPUS");
        for file in files.split(':').filter(|f| !f.is_empty()) {
            let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
            let (mut lines, mut reported, mut eff) = (0usize, 0usize, 0usize);
            let mut top: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for l in text.lines().filter(|l| !l.is_empty()) {
                lines += 1;
                reported += usize::from(!scan(l).is_empty());
                if eff_passphrase(l) {
                    eff += 1;
                    *top.entry(l).or_default() += 1;
                }
            }
            let mut top: Vec<_> = top.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let names: Vec<String> = top.iter().take(12).map(|(l, n)| format!("{l} ({n}x)")).collect();
            println!(
                "{file}: {lines} lines, scan reports {reported}, EFF rule {eff} ({:.3} %), e.g. {}",
                100.0 * eff as f64 / lines.max(1) as f64,
                names.join(", ")
            );
        }
    }

    #[test]
    fn kinds_are_described_and_ordered() {
        for k in KINDS {
            assert!(describe(k).is_some(), "{k}");
        }
        assert!(describe("nope").is_none());
        assert_eq!(KINDS[Kind::Password as usize], "password");
        assert_eq!(KINDS[Kind::SecretAssignment as usize], "secret_assignment");
        assert_eq!(KINDS[Kind::HighEntropyToken as usize], "high_entropy_token");
    }

    #[test]
    fn key_parts_split_snake_kebab_and_camel_case() {
        assert_eq!(key_parts(b"DB_PASSWORD"), ["db", "password"]);
        assert_eq!(key_parts(b"apiKey"), ["api", "key"]);
        assert_eq!(key_parts(b"APIKey"), ["api", "key"]);
        assert_eq!(key_parts(b"x-api-key"), ["x", "api", "key"]);
        assert_eq!(key_parts(b"spring.datasource.password"), ["spring", "datasource", "password"]);
        assert_eq!(key_parts(b"--password"), ["password"]);
        assert_eq!(key_family(b"tokenizer", false), None);
        assert_eq!(key_family(b"secretary", false), None);
        assert_eq!(key_family(b"max_tokens", false), None);
        assert_eq!(key_family(b"refreshToken", false), Some(Family::Token));
        assert_eq!(key_family(b"AWS_SECRET_ACCESS_KEY", false), Some(Family::Secret));
    }

    #[test]
    fn utf8_everywhere_without_panics() {
        // Mehrbyte-Zeichen direkt an allen Grenzen der Muster
        let samples = [
            "ä-----BEGIN ä",
            "-----BEGIN PRIVATE KEY-----ä",
            "password=ä",
            "password=äöü€𝄞",
            "password: \"ä",
            "Authorization: ä",
            "bearer äöü",
            "eyJä.eyJ.",
            "ä://ä:ä@ä",
            "AGE-SECRET-KEY-1ä",
            "sk-ant-ä",
            "ssh-rsa ä",
            "--password ä",
            "äöü😀",
            "€",
            "𝄞𝄞𝄞𝄞𝄞𝄞𝄞𝄞",
            "\u{202e}password=Sommer2024\u{202c}",
            "abacus😀abdomen😀abdominal",
            "äbacus-abdomen-abdominal",
            "abacus\u{301}-abdomen-abdominal",
            "abacus-abdomen-abdomin\u{e4}l",
            "abacus\u{a0}abdomen\u{a0}abdominal",
            "abacus-𝄞-abdomen-abdominal",
            "abacus𝄞𝄞𝄞𝄞𝄞𝄞𝄞𝄞𝄞abdomen𝄞𝄞𝄞𝄞𝄞𝄞𝄞𝄞𝄞abdominal",
            "-",
            "--",
            " ",
            "a-b-c",
            "yo-yo-yo",
        ];
        for s in samples {
            redact_agrees(s).unwrap();
            for cut in 0..=s.len() {
                if let Some(head) = s.get(..cut) {
                    redact_agrees(head).unwrap();
                }
                if let Some(tail) = s.get(cut..) {
                    redact_agrees(tail).unwrap();
                }
            }
        }
        assert_eq!(scan("Passwort=Grüße2024"), ["secret_assignment"]);
    }

    /// Lineare Laufzeit auch bei bösartigen Eingaben (30000 Zeichen und 256 KB).
    #[test]
    fn linear_time_on_adversarial_input() {
        let n = 30_000;
        let inputs = [
            rep("password=", n / 9),
            rep("password=\"", n / 10),
            rep("-----BEGIN ", n / 11),
            rep("-----BEGIN PRIVATE KEY-----", n / 27),
            rep("eyJ", n / 3),
            rep("eyJabcdefghij.", n / 14),
            rep("://", n / 3),
            rep("a://b:c@", n / 8),
            rep("authorization: ", n / 15),
            rep("bearer ", n / 7),
            rep("Aa1+", n / 4),
            rep("sk-", n / 3),
            rep("ä", n),
            rnd(n, 99),
            rep("--password x ", n / 13),
            rep("ssh-rsa AAAA", n / 12),
            rep("abacus-", n / 7),
            rep("abacus ", n / 7),
            rep("a-", n / 2),
            rep("yo-", 270),
            rep("yo-yo-", 135),
            rep("drop-down-", 80),
            rep("Abacus7-", 100),
            rep("a-b_c.d ", 100),
        ];
        let started = std::time::Instant::now();
        for s in &inputs {
            let t = std::time::Instant::now();
            let _ = scan(s);
            assert!(t.elapsed() < std::time::Duration::from_millis(300), "{:?}… took {:?}", &s[..20.min(s.len())], t.elapsed());
            let t = std::time::Instant::now();
            let _ = redact(s);
            assert!(t.elapsed() < std::time::Duration::from_millis(1200), "redact {:?}… took {:?}", &s[..20.min(s.len())], t.elapsed());
        }
        let big = rep(&format!("{} password=x eyJ ://a ", rnd(40, 7)), 256 * 1024 / 60);
        let t = std::time::Instant::now();
        let _ = scan(&big);
        assert!(t.elapsed() < std::time::Duration::from_millis(500), "256 KB took {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let r = redact(&big);
        assert!(t.elapsed() < std::time::Duration::from_millis(1500), "redact 256 KB took {:?}", t.elapsed());
        assert!(scan(&r.text).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(12));
    }

    #[test]
    fn redact_replaces_exactly_the_findings() {
        let jwt = format!("eyJ{}.eyJ{}.{}", rnd(20, 11), rnd(30, 12), rnd(20, 13));
        let r = redact(&format!("login ok token={jwt} for user"));
        assert_eq!(r.text, "login ok token=[REDACTED:jwt] for user");
        assert_eq!(r.kinds, [("jwt", 1)]);

        let r = redact(&format!("DB_PASSWORD={}\nurl=https://bob:{}@db.example.org/x", rnd(12, 3), rnd(10, 4)));
        assert_eq!(r.text, "DB_PASSWORD=[REDACTED:secret_assignment]\nurl=https://bob:[REDACTED:url_credentials]@db.example.org/x");
        assert_eq!(r.kinds, [("url_credentials", 1), ("secret_assignment", 1)]);

        // zwei Tokens derselben Art werden gezählt, Text dazwischen bleibt
        let a = format!("ghp_{}", rnd(36, 5));
        let b = format!("ghp_{}", rnd(36, 6));
        let r = redact(&format!("{a} and {b}"));
        assert_eq!(r.text, "[REDACTED:github_token] and [REDACTED:github_token]");
        assert_eq!(r.kinds, [("github_token", 2)]);
        assert_eq!(r.total(), 2);

        // ohne Fund bleibt der Text byte-gleich
        let plain = "Nov 05 kernel: usb 1-2: new high-speed USB device number 3 using xhci_hcd";
        assert_eq!(redact(plain), Redacted { text: plain.to_string(), kinds: vec![] });

        // Regel 8: der ganze Text ist ein Wort; Satzzeichen außen bleiben
        let r = redact("\"Sommer#2024\".");
        assert_eq!(r.text, "\"[REDACTED:password]\".");
        assert_eq!(r.kinds, [("password", 1)]);

        // Mehrbyte-Zeichen um den Fund
        let r = redact(&format!("ä password=„{}“ ü", rnd(10, 8)));
        assert!(r.text.starts_with("ä password=") && r.text.ends_with(" ü"), "{}", r.text);
        assert!(scan(&r.text).is_empty());
    }

    #[test]
    fn overlapping_findings_merge_into_one_placeholder() {
        // gebaute Überlappung: zwei Bereiche, der zweite ragt über das Ende des ersten
        let text = "abcdefghij";
        let merged = merge_spans(text, vec![(Kind::Jwt, 2, 6), (Kind::HighEntropyToken, 4, 9), (Kind::Password, 0, 1)]);
        assert_eq!(merged, [(Kind::Password, 0, 1), (Kind::Jwt, 2, 9)]);
        assert_eq!(replace_spans(text, merged), "[REDACTED:password]b[REDACTED:jwt]j");
        // ein später gefundener, umschließender Bereich übernimmt die Art des früheren
        let merged = merge_spans(text, vec![(Kind::AwsAccessKey, 3, 5), (Kind::PrivateKey, 1, 8)]);
        assert_eq!(merged, [(Kind::AwsAccessKey, 1, 8)]);
        // angrenzende Bereiche bleiben getrennt
        assert_eq!(merge_spans(text, vec![(Kind::Jwt, 0, 3), (Kind::Jwt, 3, 5)]).len(), 2);
        // Grenzen mitten in einem Zeichen werden auf Zeichengrenzen erweitert
        assert_eq!(merge_spans("aäb", vec![(Kind::Jwt, 2, 3)]), [(Kind::Jwt, 1, 3)]);

        // echte Überlappung: ein AWS-Schlüssel mitten in einem privaten PEM-Block zählt beide Arten, ein Platzhalter
        let pem = format!("-----BEGIN PRIVATE KEY-----\n{}\nAKIA{}\n{}\n-----END PRIVATE KEY-----", rnd(64, 1), "Z".repeat(16), rnd(64, 2));
        let r = redact(&format!("before\n{pem}\nafter"));
        assert_eq!(r.kind_names(), scan(&format!("before\n{pem}\nafter")));
        assert_eq!(r.text, "before\n[REDACTED:private_key]\nafter");
    }

    #[test]
    fn a_second_pass_catches_what_only_shows_after_redaction() {
        // `Negotiate` gilt erst ohne das Token dahinter als Wert der Kopfzeile (ab 8 Zeichen; `Bearer`, `Basic` bleiben)
        let t = format!("Authorization: Negotiate {}", rnd(40, 21));
        let r = redact(&t);
        assert!(scan(&r.text).is_empty(), "{}", r.text);
        assert_eq!(r.kind_names(), scan(&t));
        assert_eq!(r.text, "Authorization: [REDACTED:auth_header] [REDACTED:auth_header]");
        assert_eq!(r.kinds, [("auth_header", 2)]);
        let r = redact(&format!("Authorization: Bearer {}", rnd(40, 22)));
        assert_eq!(r.text, "Authorization: Bearer [REDACTED:auth_header]");
        assert_eq!(r.kinds, [("auth_header", 1)]);
    }

    #[test]
    fn placeholders_never_trigger_a_finding() {
        for kind in KINDS.iter().copied().chain(["kernel_param", "command_arguments", "docker_labels"]) {
            let p = placeholder(kind);
            let contexts = [
                p.clone(),
                format!("password={p}"),
                format!("password: \"{p}\""),
                format!("\"api_key\": \"{p}\""),
                format!("--password {p}"),
                format!("Authorization: Bearer {p}"),
                format!("Authorization: Basic {p}"),
                format!("https://user:{p}@host.example.org/"),
                format!("token={p}{p}{p}"),
                format!("{p}{p}"),
                format!("x {p} y {p}"),
                format!("export SECRET_KEY={p}"),
            ];
            for c in contexts {
                assert_eq!(scan(&c), Vec::<&str>::new(), "{c:?}");
                assert_eq!(redact(&c).text, c, "log mode");
                assert_eq!(redact_with(&c, Strictness::Selection).text, c);
            }
        }
    }

    /// Schärfung aus dem Messschritt der Diagnose (`src/diagnose/filter.rs`): Hex hinter einem Bezeichner ist so frei wie
    /// Hex allein; mit Schlüsselwort oder zufälligem Bezeichner bleibt es ein Fund.
    #[test]
    fn hex_behind_an_identifier_is_like_hex_alone() {
        // in einer Zeile (als ganze Markierung greift Regel 8, wie bei Hex allein mit Bezeichner davor)
        let hex = "8687aa8ef2d5dd5e5917da2f36dbfcbe";
        for t in [
            format!("InvocationID={hex}"),
            format!("MACHINE_ID={hex}"),
            format!("x.y-z={hex}"),
            "BootID=550e8400-e29b-41d4-a716-446655440000".into(),
            "ViewPortOut=2211x1963+0+0".into(),
        ] {
            let line = format!("unit: {t} ok");
            assert_eq!(scan(&line), Vec::<&str>::new(), "{line:?}");
        }
        assert_eq!(scan(&format!("unit: token={hex} ok")), ["secret_assignment"]);
        assert_eq!(scan(&format!("unit: X5Rk9pQ2={hex} ok")), ["high_entropy_token"]);
        assert_eq!(scan(&format!("unit: Id={} ok", rnd(30, 9))), ["high_entropy_token"]);
    }

    #[test]
    fn password_like_and_random_token_are_the_rules_of_scan() {
        assert!(password_like("Sommer-2024!"));
        assert!(password_like("correct-battery-staple"));
        assert!(!password_like("quiet"));
        assert!(!password_like("x86_64-unknown-linux-gnu"));
        assert!(!password_like("/dev/nvme0n1p2"));
        let tok = rnd(32, 4);
        assert!(random_token(&tok));
        assert_eq!(scan(&tok), ["high_entropy_token"]);
        assert!(!random_token("0123456789abcdef0123456789abcdef"));
        assert!(!random_token("550e8400-e29b-41d4-a716-446655440000"));
    }

    /// Security-Review Diagnose H1, N8: Log-Formen, die der Modus `Selection` übersieht und der Modus `Log` schwärzt.
    /// `scan` bleibt für jeden Fall, wie es war (der Modus `Selection` findet hier nichts oder dasselbe wie vorher).
    #[test]
    fn log_mode_catches_what_logs_contain() {
        let tg = format!("{}{}", rnd(3, 41), rnd(32, 42));
        let discord = rnd(68, 43);
        let slack = rnd(24, 44);
        let hex40 = "3f9a0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b";
        let ts = format!("tskey-auth-k{}-{}", rnd(10, 45), rnd(30, 46));
        let sg = format!("SG.{}.{}", rnd(22, 47), rnd(43, 48));
        // (Text, Geheimnis, das danach fehlen muss)
        let cases: Vec<(String, String)> = vec![
            // 1. escaptes JSON, auch doppelt
            (r#"2026-11-05T10:00:00Z {"msg":"login","body":"{\"user\":\"anna\",\"password\":\"Sommerwiese\"}"}"#.into(), "Sommerwiese".into()),
            (r#"{"a":"{\"b\":\"{\\\"secret\\\":\\\"Wolke\\\"}\"}"}"#.into(), "Wolke".into()),
            // 2. Wort aus Buchstaben mitten in der Zeile, kurz, hinter `--password` mit weiteren Optionen
            ("auth failed for user=bob password=geheimnis".into(), "geheimnis".into()),
            ("login pass=abc from 10.0.0.1".into(), "abc".into()),
            ("COMMAND=/usr/bin/mysql -u root --password geheim -h db".into(), "geheim".into()),
            // 3. Token im URL-Pfad und als Benutzername ohne `:`
            (format!("POST https://api.telegram.org/bot123456789:{tg}/sendMessage"), tg.clone()),
            (format!("webhook https://discord.com/api/webhooks/1234567/{discord} failed"), discord.clone()),
            (format!("post https://hooks.slack.com/services/T0001/B0002/{slack}"), slack.clone()),
            (format!("clone https://{hex40}@git.example.com/x.git"), hex40.into()),
            (format!(r#"{{"url":"https:\/\/hooks.slack.com\/services\/T0001\/B0002\/{slack}"}}"#), slack.clone()),
            // 4. `sig` und `key` als URL-Parameter
            (format!("GET /blob?sv=2022-11-02&sig={}&se=2026 200", rnd(20, 49)), rnd(20, 49)),
            (format!("GET https://maps.example.com/api?key={hex40}&q=x"), hex40.into()),
            // N8: zusammengeschriebene Namen, Tailscale, weitere Präfixe
            ("PGPASSWORD=Kater99 psql -h db".into(), "Kater99".into()),
            ("MYSQLPWD=hund".into(), "hund".into()),
            ("adminpassword: Kekse".into(), "Kekse".into()),
            (format!("TS_AUTHKEY={}", rnd(12, 50)), rnd(12, 50)),
            (format!("tailscale up --authkey={ts}"), ts.clone()),
            (format!("using {ts} for login"), ts.clone()),
            (format!("token {} ok", format_args!("dckr_pat_{}", rnd(27, 51))), format!("dckr_pat_{}", rnd(27, 51))),
            (format!("glsa_{}_{} used", rnd(32, 52), "0a1b2c3d"), rnd(32, 52)),
            (format!("mail key {sg} rejected"), sg.clone()),
            ("Cookie: session=c2VjcmV0; psk=Heimnetz123".into(), "Heimnetz123".into()),
            ("set-cookie sessionid=abcdef12".into(), "abcdef12".into()),
        ];
        let mut bad = Vec::new();
        for (text, secret) in &cases {
            let r = redact(text);
            if r.text.contains(secret.as_str()) || r.total() == 0 {
                bad.push(format!("{text:?}\n   -> {:?}", r.text));
            }
            if !scan(&r.text).is_empty() {
                bad.push(format!("{text:?}: scan after log mode {:?}", scan(&r.text)));
            }
            if let Err(e) = redact_agrees(text) {
                bad.push(e);
            }
        }
        assert!(bad.is_empty(), "{} of {} cases wrong:\n{}", bad.len(), cases.len(), bad.join("\n"));

        // Escaptes JSON: genau der Wert, die Escapes bleiben
        let r = redact(r#"{"body":"{\"password\":\"Sommerwiese\",\"user\":\"anna\"}"}"#);
        assert_eq!(r.text, r#"{"body":"{\"password\":\"[REDACTED:secret_assignment]\",\"user\":\"anna\"}"}"#);
        // Telegram: das ganze Segment, der Rest der URL bleibt
        let r = redact(&format!("https://api.telegram.org/bot123456789:{tg}/sendMessage"));
        assert_eq!(r.text, "https://api.telegram.org/[REDACTED:api_token]/sendMessage");
        // Der Modus `Selection` (Textaktionen) bleibt beim alten Verhalten
        assert_eq!(scan("auth failed for user=bob password=geheimnis"), Vec::<&str>::new());
        assert_eq!(scan("PGPASSWORD=Kater99 psql -h db"), Vec::<&str>::new());
    }

    /// Was der Modus `Log` stehen lässt: Füll- und Statuswörter, Verweise, Pfade, Zahlen bei Token-Familien, Schlüssel,
    /// die etwas über das Geheimnis sagen, gewöhnliche URLs.
    #[test]
    fn log_mode_keeps_what_is_no_secret() {
        for t in [
            "pam_unix(sudo:session): session opened for user root(uid=0) by blub(uid=1000)",
            "Accepted password for anna from 10.0.0.2 port 52344 ssh2",
            "password: incorrect",
            "token: not found",
            "token=12345 retries=3",
            "audit: type=1701 auid=1000 uid=1000 ses=2 pid=99 comm=\"foo\" sig=11 res=1",
            "key=(null)",
            "password_file=/run/secrets/db token_url=https://auth.example.org/token max_tokens=4096",
            "GET https://example.org/api/v1/items/42?page=2&sort=name 200",
            "Config: added 'key_mgmt' value 'WPA-PSK'",
            "keymap=de keyboard=pc105",
            "PWD=/home/blub OLDPWD=/tmp",
            "session-3.scope: Deactivated successfully.",
        ] {
            let r = redact(t);
            assert_eq!(r.text, t, "{t:?} -> {:?}", r.text);
        }
    }

    /// Messschritt Diagnose (Aufgabe 10b): Kopf `docker_state · container=…` (ein einzelnes Wort) und Zeilen von
    /// `systemctl show` mit CamelCase-Schlüssel galten als Passwort bzw. Hochentropie-Token.
    #[test]
    fn structured_identifiers_are_no_secret() {
        let mut bad = Vec::new();
        // (Text, was `scan` dazu meldet)
        for (t, scanned) in [
            ("container=dataroom-worker-1", &["password"][..]),
            ("unit=backup", &[]),
            ("WantedBy=default.target", &["password"]),
            ("Id=backup.service", &["password"]),
            ("DropInPaths=/etc/systemd/system/wg-quick@wg0.service.d/override.conf", &["high_entropy_token"]),
            ("ExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET", &["high_entropy_token"]),
            ("FragmentPath=/etc/systemd/system/backup.service", &["high_entropy_token"]),
            (
                "Id=backup.service\nExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET\nFragmentPath=/etc/systemd/system/backup.service\n",
                &["high_entropy_token"],
            ),
        ] {
            let r = redact(t);
            if r.text != t {
                bad.push(format!("{t:?} -> {:?}", r.text));
            }
            // `scan` (Textaktionen) bleibt, wie es war: dort sperrt ein solches einzelnes Wort weiter die Cloud
            if scan(t) != scanned {
                bad.push(format!("{t:?}: scan changed to {:?}", scan(t)));
            }
        }
        // Die Ausnahmen gelten nur für Wörter als Namen und Werte: Passwörter und Tokens bleiben Funde
        let tok = rnd(24, 61);
        for (t, secret) in [
            ("container=Sommer#2024x".to_string(), "Sommer#2024x".to_string()),
            ("geologic=starship=perceive".into(), "starship".into()),
            (format!("ExecMainStartTimestamp={tok}"), tok.clone()),
            (format!("FragmentPath=/etc/{tok}/backup"), tok.clone()),
            (format!("Xk9pQ2mZ=Thu{}", rnd(20, 62)), "Xk9pQ2mZ".into()),
            ("XkRpQmZwLtVb=Thursday".into(), "XkRpQmZwLtVb".into()),
            // Base64 aus Buchstaben mit `=` am Ende: kein Wert, also keine Zuweisung
            ("XkRpQmZwLtVbNcHsQwYt=".into(), "XkRpQmZwLtVbNcHsQwYt".into()),
        ] {
            let r = redact(&t);
            if r.text.contains(secret.as_str()) {
                bad.push(format!("{t:?} kept the secret: {:?}", r.text));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
