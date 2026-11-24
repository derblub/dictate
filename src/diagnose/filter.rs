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

//! Filter für den Diagnosebericht (Aufgabe 4, `PLAN-diagnose.md` „Geheimnisse: schwärzen statt sperren“): jede
//! [`ProbeOutput`] läuft durch die probenspezifischen Filter ihrer Teile ([`Filter`]), dann durch den allgemeinen
//! Geheimnisfilter [`secrets::redact`] (derselbe Kern wie der Filter der Textaktionen, strengerer Modus) und zuletzt durch das
//! Pseudonymisieren ([`Pseudonyms`]). Ergebnis ist der Text des Abschnitts und die Zählung je Art für die Vorschau.
//!
//! Gefiltert wird der ganze Text des Abschnitts ([`ProbeOutput::text`]: Teile, Hinweise, Grund bei fehlenden Daten), damit
//! auch Fehlermeldungen der Programme und Auflösungen durch den Filter gehen. `resolved` steht nicht im Text; der Bericht
//! (Aufgabe 5) schickt den Kopf durch [`filter_line`], das dieselben allgemeinen Filter anwendet.
//!
//! Reihenfolge je Abschnitt: probenspezifische Filter der Teile ([`Filter`]), dann in jeder Zeile die Kernel-Kommandozeile
//! ([`cmdline_lines`]), die Kommandozeilen von Prozessen ([`command_lines`]), der Geheimnisfilter im strengen Modus für
//! Logs ([`secrets::redact`], [`secrets::Strictness::Log`]) und zuletzt das Pseudonymisieren ([`Pseudonyms`]).
//!
//! **Kernel-Kommandozeile** (streng; `Filter::KernelCmdline` für den ganzen Teil von `kernel_info`, [`cmdline_lines`]
//! für jede Zeile mit `Kernel command line:` bzw. `Command line:` in jedem Abschnitt: Journal von `boot_end`,
//! pstore). Geschwärzt wird der ganze Wert eines Parameters (`[REDACTED:kernel_param]`), wenn sein Name oder ein Name
//! im Wert (`systemd.setenv=DB_PASSWORD=…`) nach einem Geheimnis aussieht (`pass`, `key`, `secret`, `token`, `cred`,
//! `psk`, `pwd`, `auth`; nicht `keymap`, `passthrough`, `authorized`), oder wenn ein Stück des Wertes zwischen `=` und
//! `,` wie ein markiertes Passwort aussieht (Regel 8 von `scan`, [`secrets::password_like`]) oder ein Hochentropie-Token
//! ist ([`secrets::random_token`]). Pfade, Zahlen, UUIDs, Hex und Wörter wie `quiet`, `amd_pstate=active`,
//! `console=ttyS0,115200n8` bleiben. In Kauf genommen: Videomodi mit `@` (`video=DP-1:1920x1080@60` sieht wie ein
//! Passwort aus), Locale-Werte wie `de_AT.UTF-8`.
//!
//! **Kommandozeilen von Prozessen** (Security-Review H2): sudo `COMMAND=`, pkexec `[COMMAND=…]`, `…CMDLINE=`, audit
//! `proctitle=` und `EXECVE`, cron `CMD (…)`, Beschreibungen transienter Units von `systemd-run` werden auf den
//! Programmnamen gekürzt (`[REDACTED:command_arguments]`), wie `unit_status` und `top_procs` es für ihre Prozesse tun.
//!
//! **Fehlalarmquote** (Messschritt am 05.11.2026, wiederholt nach dem Security-Review und nach Aufgabe 10c am selben Tag; Test
//! `false_alarm_corpus`, Zahlen in [`CORPUS_COUNTS`]). Korpus: 2400 synthetische Zeilen ohne Geheimnis und ohne
//! persönliche Daten (`tests/diagnose/corpus/`, erzeugt von `generate.py` mit fester Saat): Kernel-Meldungen (Xid, AER,
//! MCE, NVMe, Oops-Register, X.509-Fingerabdrücke, `audit proctitle=<hex>`, Base64 in TPM-, IMA- und Firmware-Meldungen),
//! NetworkManager, resolved, tailscaled, Xorg; Docker (volle und kurze Container-IDs, `sha256:`-Digests, Logs mit
//! Trace-IDs, UUIDs, Go-Panics, Base64-Cursor und -Etags); systemd (Scopes und Mounts mit 64 Hex-Zeichen, `\x2d`-Units
//! mit UUID, Coredump mit Build-ID, Journal-Pfade mit Maschinen-ID, InvocationID, pacman-Versionen); `logmode.txt` für
//! die strengeren Regeln (URLs mit Pfaden, IDs und Digests, `sig=11` und `key=(null)` im Audit-Log, `'key_mgmt' value`,
//! `session=`/`sid=` mit Zahlen, `auth: none`, `token: expired`, `password: incorrect`, escaptes JSON ohne Geheimnis,
//! Units mit `@`, Kernel-Kommandozeilen ohne Geheimnis); `structured.txt` (Messschritt der Diagnose, Aufgabe 10b) für
//! strukturierte Zeilen, die als Ganzes wie ein Token oder Passwort aussehen: Zeilen von `systemctl show`
//! (`ExecMainStartTimestamp=Thu …`, `FragmentPath=/etc/systemd/system/…`, `WantedBy=…`, `DropInPaths=…`) und Parameter
//! aus dem Kopf eines Abschnitts als einzelnes Wort (`container=dataroom-worker-1`). Jeder Treffer ist falsch; gekürzte
//! Kommandozeilen sind gewollt und eigene Spalte.
//!
//! | Datei | Zeilen | Fund von `scan` (Textaktionen) | Fund des Filters (Modus `Log`, Kernel) | Kommandozeile gekürzt | Pseudonym |
//! |---|---|---|---|---|---|
//! | `journal.txt` | 600 | 46 (7,7 %) | 46 (7,7 %) | 13 | 0 |
//! | `docker.txt` | 500 | 49 (9,8 %) | 64 (12,8 %) | 0 | 0 |
//! | `systemd.txt` | 500 | 0 | 0 | 0 | 0 |
//! | `logmode.txt` | 400 | 0 | 0 | 0 | 0 |
//! | `structured.txt` | 400 | 249 (62 %) | 0 | 0 | 0 |
//!
//! `structured.txt`: vor der Korrektur (Aufgabe 10c) schwärzte der Filter mindestens dieselben 249 Zeilen (der Modus `Log`
//! prüfte zuletzt mit den Regeln von `scan`), darunter die Funde des Messschritts (`docker_state · container=…` als
//! `[REDACTED:password]`, `ExecMainStartTimestamp`/`FragmentPath` als `high_entropy_token`). Ursache: Regel 8 von `scan`
//! („die Markierung ist ein einzelnes Wort“) traf den Kopf mit nur einem Parameter, den [`filter_line`] allein prüft, und
//! jede Zeile `Name=Wert` von `systemctl show`; Regel 7 sah `Name=Wert` als einen Base64-Lauf. Seitdem gelten im Modus
//! `Log` zwei enge Ausnahmen für Namen aus Wörtern ([`secrets::Strictness::Log`]); `scan` bleibt unverändert und meldet
//! sie weiter, darum prüft die Diagnose vor dem Senden mit [`secrets::residue`] statt mit `scan`. Passwörter als Wert,
//! Tokens als Wert und zufällige Namen bleiben Funde (Test `structured_identifiers_are_no_secret` in `secrets.rs`).
//!
//! `scan` ist unverändert. Alle Fehlalarme von `scan` sind `high_entropy_token` auf Base64-Blöcken ≥ 24 Zeichen
//! (Digests, Signaturen, Cursor, Etags); sie sind von zufälligen Tokens nicht zu unterscheiden und bleiben (die Schwärzung
//! kostet die Diagnose nur den Blockinhalt). Der Modus `Log` schwärzt zusätzlich 15 Zeilen in `docker.txt`:
//! `cache: stored object key=thumb/ab/<hex40>` (der Review verlangt `key=` als Geheimnis; der Wert ist hier ein
//! Speicherpfad, aber Schlüssel in Base64 enthalten ebenfalls `/`, darum in Kauf genommen). Vor der Messung waren es im
//! Modus `Log` 47 Zeilen mehr: `Kernel signing key: <hex40>` (32 in `journal.txt`) und `session: rotated id=…` (15 in
//! `docker.txt`); seitdem zählen die allgemeinen Wörter `key`, `sig`, `signature`, `auth`, `session`, `sid`, `cookie`
//! nur mit `=` oder als CLI-Option. Die 13 gekürzten Zeilen sind `audit proctitle=<hex>` mit zufälligem Hex, das keinen
//! Text ergibt (wird ganz entfernt). Aus der ersten Messung: `InvocationID=<hex>` und `ViewPortOut=2211x1963+0+0` (35
//! Zeilen) sind seit damals frei (Hex bzw. Maße hinter einem Bezeichner, Test `hex_behind_an_identifier_is_like_hex_alone`
//! in `secrets.rs`), `user-1000.journal` bleibt stehen (Platzhalter-Formen im Rohtext, siehe [`crate::diagnose::pseudo`]).

use super::probes::boot::secret_param_name;
use super::probes::units::program_of;
use super::probes::{Filter, ProbeOutput, Redaction, KERNEL_CMDLINE_MARK};
use super::pseudo::{Category, Counts, Pseudonyms};
use crate::actions::secrets;

/// Zahlen der Messung je Datei (Datei, Zeilen, Zeilen mit Fund von `scan`, Zeilen mit Fund des Filters der Diagnose,
/// Zeilen mit gekürzter Kommandozeile, Zeilen mit Pseudonym); der Test `false_alarm_corpus` prüft sie, die Tabelle im
/// Modulkopf nennt sie.
pub const CORPUS_COUNTS: [(&str, usize, usize, usize, usize, usize); 5] = [
    ("journal", 600, 46, 46, 13, 0),
    ("docker", 500, 49, 64, 0, 0),
    ("systemd", 500, 0, 0, 0, 0),
    ("logmode", 400, 0, 0, 0, 0),
    ("structured", 400, 249, 0, 0, 0),
];

/// Was der Filter je Abschnitt getan hat, für die Vorschau („Art und Zahl je Abschnitt“).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterCounts {
    /// Schwärzungen der Probe, der strengen Filter und des Geheimnisfilters, je Art addiert (Reihenfolge: zuerst die der
    /// Probe, dann `kernel_param`, dann die Arten von [`secrets::KINDS`]).
    pub redactions: Vec<Redaction>,
    /// Ersetzte Vorkommen je Art der Platzhalter.
    pub pseudonyms: Counts,
}

impl FilterCounts {
    fn add_redaction(&mut self, kind: &'static str, count: usize) {
        if count == 0 {
            return;
        }
        match self.redactions.iter_mut().find(|r| r.kind == kind) {
            Some(r) => r.count += count,
            None => self.redactions.push(Redaction { kind, count }),
        }
    }

    fn add_pseudonyms(&mut self, counts: &Counts) {
        for (cat, n) in counts {
            match self.pseudonyms.iter_mut().find(|(c, _)| c == cat) {
                Some((_, m)) => *m += n,
                None => self.pseudonyms.push((*cat, *n)),
            }
        }
        self.pseudonyms.sort_by_key(|(c, _)| Category::ALL.iter().position(|x| x == c));
    }

    /// Zahl aller Schwärzungen (für Log und Kopf: nur Zahlen, nie Inhalte).
    pub fn redacted_total(&self) -> usize {
        self.redactions.iter().map(|r| r.count).sum()
    }
}

/// Ein gefilterter Abschnitt: der Text, wie er in Vorschau und Bericht steht, und die Zählung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filtered {
    pub probe: &'static str,
    pub text: String,
    pub counts: FilterCounts,
}

/// Filtert eine Ausgabe (lernt vorher ihre Namen, siehe [`Pseudonyms::learn`]).
pub fn filter_output(out: &ProbeOutput, pseudo: &mut Pseudonyms) -> Filtered {
    pseudo.learn_names(&out.names);
    pseudo.learn(out.probe, &out.text());
    filter_learned(out, pseudo)
}

/// Filtert die Ausgaben einer Runde: erst lernen alle Abschnitte ihre Namen, dann wird ersetzt, damit ein Name, den
/// erst ein späterer Abschnitt strukturell zeigt (Gegenstelle in `net_tailscale`), auch in früheren ersetzt wird.
pub fn filter_outputs(outs: &[ProbeOutput], pseudo: &mut Pseudonyms) -> Vec<Filtered> {
    for out in outs {
        pseudo.learn_names(&out.names);
        pseudo.learn(out.probe, &out.text());
    }
    outs.iter().map(|o| filter_learned(o, pseudo)).collect()
}

/// Eine Zeile außerhalb der Ausgaben (Kopf des Abschnitts mit aufgelösten Parametern): dieselben allgemeinen Filter wie
/// der Text der Abschnitte (Kernel-Kommandozeile, Kommandozeilen von Prozessen, Geheimnisfilter, Pseudonyme), Zählung
/// wie bei den Abschnitten.
pub fn filter_line(line: &str, pseudo: &mut Pseudonyms) -> (String, FilterCounts) {
    let mut counts = FilterCounts::default();
    let (text, n) = cmdline_lines(line);
    counts.add_redaction("kernel_param", n);
    let (text, n) = command_lines(&text);
    counts.add_redaction("command_arguments", n);
    let r = secrets::redact(&text);
    for (kind, n) in &r.kinds {
        counts.add_redaction(kind, *n);
    }
    let (text, p) = pseudo.apply(&r.text);
    counts.add_pseudonyms(&p);
    (text, counts)
}

fn filter_learned(out: &ProbeOutput, pseudo: &mut Pseudonyms) -> Filtered {
    let mut counts = FilterCounts::default();
    for r in &out.redactions {
        counts.add_redaction(r.kind, r.count);
    }
    let mut o = out.clone();
    for part in &mut o.parts {
        match part.filter {
            Filter::Standard => {}
            Filter::KernelCmdline => {
                let (text, n) = strict_cmdline(&part.text);
                part.text = text;
                counts.add_redaction("kernel_param", n);
            }
        }
    }
    let (text, more) = filter_line(&o.text(), pseudo);
    for r in more.redactions {
        counts.add_redaction(r.kind, r.count);
    }
    counts.add_pseudonyms(&more.pseudonyms);
    Filtered { probe: out.probe, text, counts }
}

/// Strenger Filter für die Kernel-Kommandozeile (siehe Modulkopf); Leerraum bleibt, wie er ist.
pub fn strict_cmdline(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let ws = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
        out.push_str(&rest[..ws]);
        rest = &rest[ws..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        rest = &rest[end..];
        match word.split_once('=') {
            Some((name, value)) if !value.contains("[REDACTED:") && secret_param(name, value) => {
                out.push_str(name);
                out.push('=');
                out.push_str(&secrets::placeholder("kernel_param"));
                count += 1;
            }
            _ => out.push_str(word),
        }
    }
    (out, count)
}

/// Ist der Wert des Parameters `name=value` geheim? Der Name oder ein Name im Wert (`systemd.setenv=DB_PASSWORD=x`,
/// `rd.luks.options=…`) sieht danach aus ([`secret_param_name`]), oder ein Stück des Wertes zwischen `=` und `,` sieht
/// wie ein Passwort oder ein Token aus.
fn secret_param(name: &str, value: &str) -> bool {
    let inner_names = value.rsplit_once('=').map_or("", |(names, _)| names);
    secret_param_name(name)
        || inner_names.split('=').any(secret_param_name)
        || value.split(['=', ',']).any(|piece| secrets::password_like(piece) || secrets::random_token(piece))
}

/// Die Marken, hinter denen eine Zeile die Kernel-Kommandozeile zitiert: Kernel (`Kernel command line:` im Journal und
/// in pstore, [`KERNEL_CMDLINE_MARK`]; auf x86 früh auch `Command line:`).
const CMDLINE_MARKS: [&str; 2] = [KERNEL_CMDLINE_MARK, "Command line:"];

/// [`strict_cmdline`] auf dem Rest jeder Zeile hinter einer der [`CMDLINE_MARKS`], in jedem Abschnitt (Security-Review
/// M1: im Profil „Last crash“ zeigen `boot_end` und `pstore` die ersten Kernelzeilen eines kurzen Boots). Alle anderen
/// Zeilen und das, was vor der Marke steht (Zeitstempel), bleiben unberührt.
pub fn cmdline_lines(text: &str) -> (String, usize) {
    if !CMDLINE_MARKS.iter().any(|m| text.contains(m)) {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for line in text.split_inclusive('\n') {
        match CMDLINE_MARKS.iter().find_map(|m| line.find(m).map(|i| i + m.len())) {
            Some(i) => {
                let (head, rest) = line.split_at(i);
                let (filtered, n) = strict_cmdline(rest);
                out.push_str(head);
                out.push_str(&filtered);
                count += n;
            }
            None => out.push_str(line),
        }
    }
    (out, count)
}

/// Platzhalter für entfernte Argumente
fn arguments() -> String {
    secrets::placeholder("command_arguments")
}

/// `prog` oder `prog [REDACTED:command_arguments]` für eine Kommandozeile; zählt, wenn Argumente wegfallen.
fn shorten(command: &str, count: &mut usize) -> String {
    let (name, had_args) = program_of(command);
    if had_args {
        *count += 1;
        format!("{name} {}", arguments())
    } else {
        name
    }
}

/// Kommandozeilen von Prozessen in Log-Zeilen auf den Programmnamen kürzen (Security-Review H2; gleiche Regel wie
/// `unit_status` und `top_procs`: „Kommandozeilen von Prozessen nur als Programmname“). Erkannt werden:
/// - sudo `COMMAND=/usr/bin/x args` (bis zum Zeilenende), pkexec/polkit `[COMMAND=/usr/bin/x args]`,
/// - `…CMDLINE=` (Felder wie `COREDUMP_CMDLINE=`, bis zum Zeilenende),
/// - audit `proctitle=<hex>` (NUL-getrennt, dekodiert) bzw. `proctitle="…"` und `type=EXECVE … a0="x" a1=… a2=…`
///   (`a0` bleibt als Programmname, `a1…` und `aN[i]=` fallen weg),
/// - cron `CMD (…)`,
/// - transiente Units von `systemd-run`, deren Beschreibung die Kommandozeile ist (`run-u42.service - /usr/bin/x args`).
///
/// Liefert den Text und die Zahl der gekürzten Kommandozeilen.
pub fn command_lines(text: &str) -> (String, usize) {
    const MARKS: [&str; 6] = ["COMMAND=", "CMDLINE=", "proctitle=", "EXECVE", "CMD (", " - /"];
    if !MARKS.iter().any(|m| text.contains(m)) {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for line in text.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        out.push_str(&command_line_in(body, &mut count));
        out.push_str(nl);
    }
    (out, count)
}

fn command_line_in(line: &str, count: &mut usize) -> String {
    // pkexec/polkit: `[COMMAND=/usr/bin/foo --token abc]` bis zur letzten `]`
    if let Some(i) = line.find("[COMMAND=") {
        let start = i + 9;
        let end = line.rfind(']').filter(|&e| e >= start).unwrap_or(line.len());
        return format!("{}{}{}", &line[..start], shorten(&line[start..end], count), &line[end..]);
    }
    // sudo `COMMAND=…`, `COREDUMP_CMDLINE=…`: bis zum Zeilenende
    for mark in ["COMMAND=", "CMDLINE="] {
        if let Some(i) = line.find(mark) {
            let start = i + mark.len();
            return format!("{}{}", &line[..start], shorten(&line[start..], count));
        }
    }
    if let Some(i) = line.find("proctitle=") {
        let start = i + 10;
        let rest = &line[start..];
        let (value, end) = match rest.strip_prefix('"') {
            Some(q) => match q.find('"') {
                Some(e) => (&q[..e], start + e + 2),
                None => (q, line.len()),
            },
            None => {
                let e = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (&rest[..e], start + e)
            }
        };
        let short = match decode_audit_hex(value) {
            AuditValue::Text(command) => shorten(&command, count),
            AuditValue::Plain => shorten(value, count),
            AuditValue::Binary => {
                *count += 1;
                arguments()
            }
        };
        return format!("{}{short}{}", &line[..start], &line[end..]);
    }
    if line.contains("type=EXECVE") {
        return execve_args(line, count);
    }
    // cron `(user) CMD (command)`
    if let Some(i) = line.find("CMD (") {
        let start = i + 5;
        let end = line.rfind(')').filter(|&e| e >= start).unwrap_or(line.len());
        return format!("{}{}{}", &line[..start], shorten(&line[start..end], count), &line[end..]);
    }
    // systemd-run: `Started run-u42.service - /usr/bin/rsync …` (Beschreibung = Kommandozeile)
    if let Some(i) = line.find(" - /") {
        let unit = line[..i].rsplit(' ').next().unwrap_or("");
        let unit_like = [".service", ".scope", ".timer", ".socket"].iter().any(|s| unit.trim_end_matches(':').ends_with(s));
        if unit_like {
            let start = i + 3;
            return format!("{}{}", &line[..start], shorten(&line[start..], count));
        }
    }
    line.to_string()
}

/// Wie audit einen Wert schreibt
enum AuditValue {
    /// Hex (`2F7573722F62696E2F6375726C002D48`, Werte mit Leerzeichen oder Sonderzeichen), dekodiert: Argumente
    /// NUL-getrennt, hier mit Leerzeichen verbunden
    Text(String),
    /// Hex, das keinen druckbaren Text ergibt: wird ganz entfernt (dekodiert gehörten Steuerzeichen in den Bericht)
    Binary,
    /// kein Hex (der Wert steht im Klartext)
    Plain,
}

fn decode_audit_hex(v: &str) -> AuditValue {
    if v.len() < 2 || !v.len().is_multiple_of(2) || !v.bytes().all(|c| c.is_ascii_hexdigit()) {
        return AuditValue::Plain;
    }
    let bytes: Vec<u8> = (0..v.len()).step_by(2).filter_map(|i| u8::from_str_radix(&v[i..i + 2], 16).ok()).collect();
    match String::from_utf8(bytes) {
        Ok(t) if !t.chars().any(|c| c.is_control() && c != '\0') => AuditValue::Text(t.replace('\0', " ").trim().to_string()),
        _ => AuditValue::Binary,
    }
}

/// `type=EXECVE argc=3 a0="curl" a1="-u" a2="bob:geheim"`: `a0` bleibt (als Programmname, Hex dekodiert), alle
/// weiteren Argumente (`a1=`, `a2[0]=`, `a2_len=`) werden ein Platzhalter.
fn execve_args(line: &str, count: &mut usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut removed = false;
    for word in line.split(' ') {
        let arg = word.strip_prefix('a').and_then(|r| {
            let digits = r.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0 && matches!(r.as_bytes().get(digits), Some(b'=' | b'[' | b'_'))).then(|| (&r[..digits], &r[digits..]))
        });
        match arg {
            Some(("0", rest)) if rest.starts_with('=') => {
                let v = rest[1..].trim_matches('"');
                match decode_audit_hex(v) {
                    AuditValue::Text(program) => out.push(format!("a0=\"{}\"", program_of(&program).0)),
                    AuditValue::Plain => out.push(format!("a0=\"{}\"", program_of(v).0)),
                    AuditValue::Binary => {
                        out.push(format!("a0={}", arguments()));
                        removed = true;
                    }
                }
            }
            Some(_) => {
                if !removed {
                    out.push(arguments());
                    removed = true;
                }
            }
            None => out.push(word.to_string()),
        }
    }
    *count += usize::from(removed);
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::probes::{Part, Status};
    use crate::diagnose::pseudo::Identity;

    fn session() -> Pseudonyms {
        Pseudonyms::new(Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), keep_dirs: vec![], ..Default::default() }, true)
    }

    /// Fixtures zusammengesetzt, damit im Repo nichts wie ein echter Schlüssel aussieht.
    fn token(n: usize, seed: u64) -> String {
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

    #[test]
    fn strict_cmdline_redacts_secret_looking_values_only() {
        let keep = "BOOT_IMAGE=/vmlinuz-linux root=UUID=550e8400-e29b-41d4-a716-446655440000 rw quiet loglevel=3 nvidia_drm.modeset=1 amd_pstate=active console=ttyS0,115200n8 rd.luks.name=550e8400-e29b-41d4-a716-446655440000=cryptroot resume=/dev/nvme0n1p3 mitigations=off";
        assert_eq!(strict_cmdline(keep), (keep.to_string(), 0));
        let t = format!("root=/dev/sda1 cryptdevice=/dev/sda2:root  opt=a,Sommer#2024x foo={} luks.key=[REDACTED:kernel_param]\n", token(32, 1));
        let (out, n) = strict_cmdline(&t);
        assert_eq!(
            out,
            "root=/dev/sda1 cryptdevice=/dev/sda2:root  opt=[REDACTED:kernel_param] foo=[REDACTED:kernel_param] luks.key=[REDACTED:kernel_param]\n"
        );
        assert_eq!(n, 2);
        // die Lücke im Modulkopf: Videomodus mit `@`
        assert_eq!(strict_cmdline("video=DP-1:1920x1080@60").1, 1);
    }

    #[test]
    fn cmdline_lines_filters_only_what_follows_the_mark() {
        // Gleiche Wörter vor der Marke und in anderen Zeilen bleiben, hinter der Marke gilt der strenge Filter.
        let text = "[     0.900] (==) Using config: video=DP-1:1920x1080@60\n\
                    [     1.001] Kernel command line: root=UUID=550e8400-e29b-41d4-a716-446655440000 rw quiet video=DP-1:1920x1080@60 opt=Sommer#2024x luks.key=[REDACTED:kernel_param]\n\
                    [     1.002] (II) Option \"video\" \"video=DP-1:1920x1080@60\"\n\
                    [     1.003] Kernel command line:  quiet loglevel=3  \n\
                    [     1.004] no trailing newline video=DP-1:1920x1080@60";
        let (out, n) = cmdline_lines(text);
        assert_eq!(n, 2);
        assert_eq!(
            out,
            "[     0.900] (==) Using config: video=DP-1:1920x1080@60\n\
             [     1.001] Kernel command line: root=UUID=550e8400-e29b-41d4-a716-446655440000 rw quiet video=[REDACTED:kernel_param] opt=[REDACTED:kernel_param] luks.key=[REDACTED:kernel_param]\n\
             [     1.002] (II) Option \"video\" \"video=DP-1:1920x1080@60\"\n\
             [     1.003] Kernel command line:  quiet loglevel=3  \n\
             [     1.004] no trailing newline video=DP-1:1920x1080@60"
        );
        // Ohne Marke und ohne Zeilenende am Schluss: unverändert.
        assert_eq!(cmdline_lines("a=b"), ("a=b".to_string(), 0));
        assert_eq!(cmdline_lines(""), (String::new(), 0));
    }

    #[test]
    fn quoted_command_line_goes_through_the_strict_filter_with_the_probe_count() {
        // Eine Probe, die die Kommandozeile zitiert und `luks.key` schon geschwärzt hat (1; bis W7 so `xorg_log`), der
        // strenge Filter fängt `opt=…` (2), der allgemeine Filter das Token in der Fehlzeile (3).
        let mut out = ProbeOutput::new("pstore");
        out.push(
            Part::new(
                "",
                format!(
                    "[     1.001] Kernel command line: root=/dev/nvme0n1p2 rw opt=Sommer#2024x luks.key=[REDACTED:kernel_param]\n[     1.002] (EE) failed for anna, token={}",
                    token(30, 5)
                ),
            ),
        );
        out.redacted("kernel_param", 1);
        let f = filter_output(&out, &mut session());
        assert_eq!(
            f.text,
            "[     1.001] Kernel command line: root=/dev/nvme0n1p2 rw opt=[REDACTED:kernel_param] luks.key=[REDACTED:kernel_param]\n[     1.002] (EE) failed for user-1, token=[REDACTED:secret_assignment]"
        );
        assert_eq!(f.counts.redactions, [Redaction { kind: "kernel_param", count: 2 }, Redaction { kind: "secret_assignment", count: 1 }]);
        assert!(secrets::scan(&f.text).is_empty());
    }

    #[test]
    fn output_goes_through_probe_filter_redact_and_pseudonyms() {
        let mut out = ProbeOutput::new("kernel_info");
        out.push(Part::new("version", "Linux version 6.11.6-arch1-1 (linux@archlinux) #1 SMP PREEMPT_DYNAMIC"));
        out.push(
            Part::new("cmdline", "root=/dev/nvme0n1p2 rw quiet secretparam=Sommer#2024x luks.key=[REDACTED:kernel_param]").with_filter(Filter::KernelCmdline),
        );
        out.redacted("kernel_param", 1);
        out.note(format!("read by anna on werkbank, token={}", token(30, 2)));
        let mut p = session();
        let f = filter_output(&out, &mut p);
        assert_eq!(
            f.text,
            "## version\nLinux version 6.11.6-arch1-1 (linux@archlinux) #1 SMP PREEMPT_DYNAMIC\n\n## cmdline\nroot=/dev/nvme0n1p2 rw quiet secretparam=[REDACTED:kernel_param] luks.key=[REDACTED:kernel_param]\n[read by user-1 on host-1, token=[REDACTED:secret_assignment]]"
        );
        assert_eq!(f.counts.redactions, [Redaction { kind: "kernel_param", count: 2 }, Redaction { kind: "secret_assignment", count: 1 }]);
        assert_eq!(f.counts.pseudonyms, [(Category::User, 1), (Category::Host, 1)]);
        assert_eq!(f.counts.redacted_total(), 3);
        assert!(secrets::scan(&f.text).is_empty());
    }

    #[test]
    fn status_messages_are_filtered_too() {
        let out = ProbeOutput::without_data("docker_state", Status::Failed("cannot reach 192.168.7.7 as anna".into()));
        let f = filter_output(&out, &mut session());
        assert_eq!(f.text, "(failed: cannot reach ip-1 as user-1)");
    }

    #[test]
    fn a_round_learns_before_it_replaces_and_numbers_stay_stable() {
        let mut journal = ProbeOutput::new("unit_journal");
        journal.push(Part::new("", "2026-11-04T09:00:00+0100 werkbank smbd[9]: client lager-7 (100.64.1.2) connected"));
        let mut ts = ProbeOutput::new("net_tailscale");
        ts.push(Part::new("", "self: werkbank (werkbank.tail4f2a.ts.net) online=yes relay=fra os=linux\npeers: 1 (1 online)\npeer online\nlager-7 yes\n"));
        ts.names = vec![(Category::Host, "werkbank".into()), (Category::Host, "werkbank.tail4f2a.ts.net".into()), (Category::Host, "lager-7".into())];
        let mut p = session();
        let round1 = filter_outputs(&[journal.clone(), ts], &mut p);
        assert_eq!(round1[0].text, "2026-11-04T09:00:00+0100 host-1 smbd[9]: client host-2 (ip-1) connected");
        assert!(round1[1].text.contains("\nhost-2 yes"), "{}", round1[1].text);
        assert!(round1[1].text.starts_with("self: host-1 (host-3)"), "{}", round1[1].text);
        // zweite Runde: dieselben Platzhalter
        let round2 = filter_outputs(&[journal], &mut p);
        assert_eq!(round2[0].text, round1[0].text);
        assert_eq!(p.restore(&round2[0].text), "2026-11-04T09:00:00+0100 werkbank smbd[9]: client lager-7 (100.64.1.2) connected");
    }

    #[test]
    fn disabled_pseudonyms_still_redact() {
        let mut out = ProbeOutput::new("docker_logs");
        out.push(Part::new("", format!("2026-11-04T09:00:00Z anna@werkbank Authorization: Bearer {}", token(40, 3))));
        let mut p = Pseudonyms::new(
            Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), keep_dirs: vec![], ..Default::default() },
            false,
        );
        let f = filter_output(&out, &mut p);
        assert_eq!(f.text, "2026-11-04T09:00:00Z anna@werkbank Authorization: Bearer [REDACTED:auth_header]");
        assert_eq!(f.counts.pseudonyms, vec![]);
        assert_eq!(f.counts.redactions, [Redaction { kind: "auth_header", count: 1 }]);
    }

    /// Messschritt Diagnose (Aufgabe 10b): Kopf mit nur einem Parameter und Zeilen von `systemctl show` sind keine
    /// Geheimnisse; ein Passwort als Parameter oder ein Token als Wert bleibt einer.
    #[test]
    fn heads_and_unit_show_lines_keep_their_values() {
        let mut p = session();
        assert_eq!(filter_line("container=dataroom-worker-1", &mut p), ("container=dataroom-worker-1".to_string(), FilterCounts::default()));
        assert_eq!(filter_line("unit=backup", &mut p).0, "unit=backup");
        assert_eq!(filter_line("container=Sommer#2024x", &mut p).0, "container=[REDACTED:password]");
        let mut out = ProbeOutput::new("unit_show");
        let tok = token(28, 7);
        out.push(Part::new(
            "",
            format!(
                "Id=backup.service\nExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET\nFragmentPath=/etc/systemd/system/backup.service\nEnvironment=X={tok}"
            ),
        ));
        let f = filter_output(&out, &mut p);
        assert_eq!(
            f.text,
            "Id=backup.service\nExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET\nFragmentPath=/etc/systemd/system/backup.service\n[REDACTED:high_entropy_token]"
        );
        assert_eq!(f.counts.redactions, [Redaction { kind: "high_entropy_token", count: 1 }]);
        assert!(secrets::residue(&f.text).is_empty());
    }

    /// Messschritt Fehlalarmquote (Ergebnis im Modulkopf und in [`CORPUS_COUNTS`]): jede Zeile des synthetischen Korpus
    /// ist frei von Geheimnissen und persönlichen Daten, jeder Treffer ist also falsch. Gemessen wird `scan` (Filter der
    /// Textaktionen, unverändert), der Filter der Diagnose (Kernel-Kommandozeile, Geheimnisfilter im Modus `Log`), die
    /// gekürzten Kommandozeilen (gewollt, kein Fehlalarm, eigene Spalte) und die Pseudonyme.
    /// `cargo test --lib false_alarm_corpus -- --nocapture` zeigt Zahlen und Beispiele, mit `DICTATE_CORPUS_ALL=1` alle Funde.
    #[test]
    fn false_alarm_corpus() {
        let files = [
            ("journal", include_str!("../../tests/diagnose/corpus/journal.txt")),
            ("docker", include_str!("../../tests/diagnose/corpus/docker.txt")),
            ("systemd", include_str!("../../tests/diagnose/corpus/systemd.txt")),
            ("logmode", include_str!("../../tests/diagnose/corpus/logmode.txt")),
            ("structured", include_str!("../../tests/diagnose/corpus/structured.txt")),
        ];
        let all = std::env::var_os("DICTATE_CORPUS_ALL").is_some();
        let mut got = Vec::new();
        for (name, text) in files {
            let (mut lines, mut scan_lines, mut filter_lines, mut command_lines_n, mut pseudo_lines) = (0usize, 0usize, 0usize, 0usize, 0usize);
            let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
            let mut examples = Vec::new();
            let mut p = session();
            for l in text.lines().filter(|l| !l.is_empty()) {
                lines += 1;
                if !secrets::scan(l).is_empty() {
                    scan_lines += 1;
                }
                let (c, n_cmd) = command_lines(l);
                if n_cmd > 0 || c != l {
                    command_lines_n += 1;
                }
                let (k, n_kernel) = cmdline_lines(&c);
                let r = secrets::redact(&k);
                if n_kernel > 0 || r.total() > 0 {
                    filter_lines += 1;
                    if n_kernel > 0 {
                        *kinds.entry("kernel_param").or_default() += 1;
                    }
                    for (kind, _) in &r.kinds {
                        *kinds.entry(kind).or_default() += 1;
                    }
                    if all || examples.len() < 8 {
                        examples.push(format!("{l}  ->  {}", r.text));
                    }
                }
                let (out, _) = p.apply(l);
                if out != l {
                    pseudo_lines += 1;
                    if all || examples.len() < 12 {
                        examples.push(format!("pseudo: {out}"));
                    }
                }
            }
            println!("{name}: {lines} lines, scan {scan_lines}, filter {filter_lines} ({kinds:?}), commands {command_lines_n}, pseudonyms {pseudo_lines}");
            for e in examples {
                println!("    {e}");
            }
            got.push((name, lines, scan_lines, filter_lines, command_lines_n, pseudo_lines));
        }
        assert_eq!(got, CORPUS_COUNTS);
    }

    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02X}")).collect()
    }

    /// Security-Review H2: Kommandozeilen in Journal-Zeilen bleiben nur als Programmname.
    #[test]
    fn command_lines_keep_only_the_program() {
        let a = "[REDACTED:command_arguments]";
        let title = hex("/usr/bin/curl\0-u\0bob:geheim");
        let cases: Vec<(String, String, usize)> = vec![
            (
                "2026-11-05T10:00:00+0100 werkbank sudo[42]:     anna : TTY=pts/3 ; PWD=/home/anna ; USER=root ; COMMAND=/usr/bin/mysql -u root --password geheim -h db".into(),
                format!("2026-11-05T10:00:00+0100 werkbank sudo[42]:     anna : TTY=pts/3 ; PWD=/home/anna ; USER=root ; COMMAND=mysql {a}"),
                1,
            ),
            (
                "pkexec[7]: anna: Executing command [USER=root] [TTY=unknown] [CWD=/home/anna] [COMMAND=/usr/bin/foo --token abc]".into(),
                format!("pkexec[7]: anna: Executing command [USER=root] [TTY=unknown] [CWD=/home/anna] [COMMAND=foo {a}]"),
                1,
            ),
            (format!("kernel: audit: type=1327 audit(1730800000.123:99): proctitle={title}"), format!("kernel: audit: type=1327 audit(1730800000.123:99): proctitle=curl {a}"), 1),
            ("audit: type=1327 proctitle=\"/usr/bin/sleep\" x=1".into(), "audit: type=1327 proctitle=sleep x=1".into(), 0),
            (
                format!("audit: type=EXECVE msg=audit(1730800000.123:99): argc=4 a0=\"curl\" a1=\"-u\" a2=\"bob:geheim\" a3_len=9 a3[0]={}", hex("x y")),
                format!("audit: type=EXECVE msg=audit(1730800000.123:99): argc=4 a0=\"curl\" {a}"),
                1,
            ),
            (
                format!("audit: type=EXECVE argc=2 a0={} a1=\"--x\"", hex("/usr/bin/my tool")),
                format!("audit: type=EXECVE argc=2 a0=\"my\" {a}"),
                1,
            ),
            (
                "systemd[1]: Started run-u42.service - /usr/bin/rsync --password-file=/x anna@nas:/y.".into(),
                format!("systemd[1]: Started run-u42.service - rsync {a}"),
                1,
            ),
            ("CROND[99]: (anna) CMD (/usr/bin/backup --key Sommer)".into(), format!("CROND[99]: (anna) CMD (backup {a})"), 1),
            ("COREDUMP_CMDLINE=/usr/bin/app --secret x".into(), format!("COREDUMP_CMDLINE=app {a}"), 1),
            ("sudo: anna : COMMAND=/usr/bin/true".into(), "sudo: anna : COMMAND=true".into(), 0),
        ];
        for (input, want, n) in &cases {
            assert_eq!(command_lines(input), (want.clone(), *n), "{input}");
        }
        // mehrere Zeilen, Zeilenenden bleiben
        let two = format!("{}\n{}\n", cases[0].0, cases[7].0);
        assert_eq!(command_lines(&two), (format!("{}\n{}\n", cases[0].1, cases[7].1), 2));
        // bleiben: Beschreibungen, die keine Kommandozeile sind, Gedankenstriche vor Pfaden ohne Unit
        for keep in [
            "systemd[1]: Started docker.service - Docker Application Container Engine.",
            "kernel: nvme0n1: p1 p2 - /dev/nvme0n1p3",
            "audit: type=1300 arch=c000003e syscall=59 success=yes exit=0 a0=55d a1=55e items=2",
        ] {
            assert_eq!(command_lines(keep), (keep.to_string(), 0), "{keep}");
        }
    }

    #[test]
    fn journal_command_lines_go_through_the_whole_filter() {
        let mut out = ProbeOutput::new("unit_journal");
        out.push(Part::new(
            "",
            "2026-11-05T10:00:00+0100 werkbank sudo[42]:     anna : TTY=pts/3 ; PWD=/home/anna ; USER=root ; COMMAND=/usr/bin/mysql -u root --password geheim -h db",
        ));
        let f = filter_output(&out, &mut session());
        assert_eq!(
            f.text,
            "2026-11-05T10:00:00+0100 host-1 sudo[42]:     user-1 : TTY=pts/3 ; PWD=/home/user-1 ; USER=root ; COMMAND=mysql [REDACTED:command_arguments]"
        );
        assert_eq!(f.counts.redactions, [Redaction { kind: "command_arguments", count: 1 }]);
    }

    /// Security-Review M1: die Kernel-Kommandozeile in Journal und pstore bekommt den strengen Filter, auch Namen im Wert.
    #[test]
    fn kernel_command_line_is_strict_in_every_section() {
        let line = "BOOT_IMAGE=/vmlinuz-linux root=/dev/nvme0n1p2 rw luks.key=abc systemd.setenv=DB_PASSWORD=geheim opt=Sommer#2024x iommu.passthrough=1 quiet";
        let want = "BOOT_IMAGE=/vmlinuz-linux root=/dev/nvme0n1p2 rw luks.key=[REDACTED:kernel_param] systemd.setenv=[REDACTED:kernel_param] opt=[REDACTED:kernel_param] iommu.passthrough=1 quiet";
        let mut boot_end = ProbeOutput::new("boot_end");
        boot_end.push(Part::new(
            "",
            format!("2026-11-04T08:12:01+0100 werkbank kernel: Command line: {line}\n2026-11-04T08:12:01+0100 werkbank kernel: Kernel command line: {line}"),
        ));
        let f = filter_output(&boot_end, &mut session());
        assert_eq!(
            f.text,
            format!("2026-11-04T08:12:01+0100 host-1 kernel: Command line: {want}\n2026-11-04T08:12:01+0100 host-1 kernel: Kernel command line: {want}")
        );
        assert_eq!(f.counts.redactions, [Redaction { kind: "kernel_param", count: 6 }]);
        let mut pstore = ProbeOutput::new("pstore");
        pstore.push(Part::new("dmesg", format!("<6>[    0.000000] Command line: {line}\n<4>[   12.3] Oops: 0000 [#1] PREEMPT SMP")));
        let f = filter_output(&pstore, &mut session());
        assert!(f.text.contains(want) && !f.text.contains("geheim") && !f.text.contains("=abc"), "{}", f.text);
        assert!(f.text.contains("Oops: 0000 [#1] PREEMPT SMP"), "{}", f.text);
        // ein Name im Wert, der nichts verrät, bleibt
        assert_eq!(strict_cmdline("systemd.setenv=SYSTEMD_LOG_LEVEL=debug rd.luks.name=550e8400-e29b-41d4-a716-446655440000=root").1, 0);
    }
}
