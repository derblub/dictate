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

//! Programme aus Desktop-Einträgen (`.desktop`) nach der Desktop Entry Specification 1.5.
//!
//! **Suche:** `$XDG_DATA_HOME/applications` (Vorgabe `~/.local/share/applications`), dann je Eintrag aus
//! `$XDG_DATA_DIRS` (Vorgabe `/usr/local/share:/usr/share`) dessen `applications`, rekursiv. Die Desktop-File-ID ist der
//! Pfad relativ zu `applications` mit `/` → `-` (`kde/foo.desktop` → `kde-foo.desktop`); die *erste* Datei mit einer ID
//! gewinnt und verdeckt alle späteren, auch wenn sie `Hidden`, `NoDisplay` oder per `OnlyShowIn` unsichtbar ist (so
//! löscht oder versteckt ein Benutzer einen Systemeintrag). Symlinks auf Dateien und Ordner werden aufgelöst (Flatpak,
//! Nix); jeder Ordner wird nur einmal gelesen.
//!
//! **Aussortiert** wird (mit Grund in [`AppScan::rejected`]): `Type` ≠ `Application`, `Hidden=true`, `NoDisplay=true`,
//! fehlendes `Name` oder `Exec`, `OnlyShowIn`/`NotShowIn` gegen `$XDG_CURRENT_DESKTOP`, `TryExec`, das nicht ausführbar
//! gefunden wird (absolut oder über `PATH`), und jedes `Exec`, das nicht streng zerlegbar ist.
//!
//! **Texte:** `Name`, `GenericName` und `Keywords` nach der Locale aus `LC_ALL`, `LC_MESSAGES`, `LANG` mit den
//! Fallback-Regeln der Spec (`lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`, dann der unlokalisierte
//! Schlüssel); der unlokalisierte Wert bleibt zusätzlich erhalten (`name_default` usw.), weil die Suche beides braucht
//! („Dateien“ und „Files“).
//!
//! **Exec** ([`split_exec`]) wird zu einer argv-Liste zerlegt, nie zu einem Shell-String, in zwei Ebenen wie in der
//! Spec: erst die Escapes der Datei für Werte vom Typ `string` (`\s \n \t \r \\`; ein unbekanntes `\x` bleibt samt
//! Backslash stehen, damit `\"` die zweite Ebene erreicht), dann die Regeln für `Exec`. Ein wörtlicher Backslash in
//! einem Argument braucht deshalb in der Datei vier (`\\\\`), ein Dollarzeichen `\\$`. Die zweite Ebene:
//! - Argumente sind durch Leerzeichen getrennt; ein Argument ist *im Ganzen* in doppelte Anführungszeichen gesetzt oder
//!   gar nicht. In Anführungszeichen stehen `\"`, `` \` ``, `\$` und `\\` für das jeweilige Zeichen; jedes andere
//!   `\x` ist ein Fehler. Ein Anführungszeichen mitten im Argument, Text hinter dem schließenden, ein nicht
//!   geschlossenes, ein einfaches Anführungszeichen, ein Backtick oder ein Backslash außerhalb von Anführungszeichen
//!   und Tabulator/Zeilenumbruch außerhalb sind „kaputtes Quoting“: Eintrag verworfen.
//! - Alles übrige ist wörtlich, auch `$`, `;`, `&`, `?`, `*` und `~`: ein Shell kommt nie dazwischen, die einzig
//!   eindeutige Lesart ist die wörtliche (die Spec verlangt Quoting für diese Zeichen; reale Einträge wie
//!   `sh -c "optiva;${SHELL:-bash}"` halten sich nicht daran, und glib liest sie ebenso).
//! - Feldcodes: `%f %F %u %U` entfallen (die Palette übergibt nie Dateien oder URLs; steht ein Code allein, entfällt das
//!   ganze Argument, steht er mitten in einem Argument wie `--uri=%u`, wird er durch nichts ersetzt), ebenso die
//!   veralteten `%d %D %n %N %v %m`; `%i` wird zu `--icon <Icon>` (zwei Argumente, nichts, wenn kein Icon gesetzt ist)
//!   und muss ein eigenes Argument sein; `%c` ist der lokalisierte Name, `%k` der Pfad der Datei, `%%` ein `%`. Ein
//!   unbekannter Feldcode, ein einzelnes `%` am Ende und jeder Feldcode (außer `%%`) in Anführungszeichen (laut Spec
//!   undefiniert) verwerfen den Eintrag.

use super::Fingerprint;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fmt;
use std::io::Read;
use std::ops::Deref;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Größte gelesene Datei; echte Einträge haben wenige KiB.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// Tiefste Verschachtelung unter `applications`.
const MAX_DEPTH: usize = 8;

/// Umgebung, aus der Verzeichnisse, Desktop, Locale und `PATH` stammen. Als Struktur, damit Tests ohne globale
/// Umgebungsvariablen auskommen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopEnv {
    /// `$XDG_DATA_HOME` (Vorgabe `~/.local/share`); `None`, wenn nicht ermittelbar.
    pub data_home: Option<PathBuf>,
    /// `$XDG_DATA_DIRS` in Rangfolge (nur absolute Pfade).
    pub data_dirs: Vec<PathBuf>,
    /// `$XDG_CURRENT_DESKTOP` (durch `:` getrennt; leer = keiner).
    pub current_desktops: Vec<String>,
    /// Locale wie `de_AT.UTF-8`; leer oder `C`/`POSIX` = keine lokalisierten Schlüssel.
    pub locale: String,
    /// `$PATH` für `TryExec` (nur absolute Einträge).
    pub path: Vec<PathBuf>,
}

impl DesktopEnv {
    /// Aus den Umgebungsvariablen des Prozesses.
    pub fn from_process() -> DesktopEnv {
        DesktopEnv::from_lookup(|k| std::env::var(k).ok(), dirs::home_dir())
    }

    /// Aus einer beliebigen Quelle für Variablen (Tests).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>, home: Option<PathBuf>) -> DesktopEnv {
        let absolute = |s: &str| -> Vec<PathBuf> { s.split(':').filter(|p| !p.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute()).collect() };
        let data_home = get("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home.map(|h| h.join(".local/share")));
        let mut data_dirs = get("XDG_DATA_DIRS").map(|s| absolute(&s)).unwrap_or_default();
        if data_dirs.is_empty() {
            data_dirs = vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
        }
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        let locale = non_empty("LC_ALL").or_else(|| non_empty("LC_MESSAGES")).or_else(|| non_empty("LANG")).unwrap_or_default();
        let current_desktops = get("XDG_CURRENT_DESKTOP").map(|s| s.split(':').filter(|d| !d.is_empty()).map(str::to_string).collect()).unwrap_or_default();
        let path = get("PATH").map(|s| absolute(&s)).unwrap_or_default();
        DesktopEnv { data_home, data_dirs, current_desktops, locale, path }
    }

    /// Die `applications`-Verzeichnisse in Rangfolge (ohne Wiederholungen).
    pub fn application_dirs(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for base in self.data_home.iter().chain(self.data_dirs.iter()) {
            let dir = base.join("applications");
            if !out.contains(&dir) {
                out.push(dir);
            }
        }
        out
    }
}

/// Schlüssel-Suffixe in der Reihenfolge der Spec für eine Locale (`de_AT.UTF-8@euro` → `de_AT@euro`, `de_AT`,
/// `de@euro`, `de`). Die Kodierung zählt nicht. `C`, `POSIX` und leer ergeben nichts.
pub fn locale_candidates(locale: &str) -> Vec<String> {
    let (rest, modifier) = match locale.split_once('@') {
        Some((r, m)) => (r, Some(m)),
        None => (locale, None),
    };
    let rest = rest.split('.').next().unwrap_or("");
    let (lang, country) = match rest.split_once('_') {
        Some((l, c)) => (l, Some(c)),
        None => (rest, None),
    };
    if lang.is_empty() || lang == "C" || lang == "POSIX" {
        return Vec::new();
    }
    let modifier = modifier.filter(|m| !m.is_empty());
    let mut out = Vec::new();
    if let (Some(c), Some(m)) = (country.filter(|c| !c.is_empty()), modifier) {
        out.push(format!("{lang}_{c}@{m}"));
    }
    if let Some(c) = country.filter(|c| !c.is_empty()) {
        out.push(format!("{lang}_{c}"));
    }
    if let Some(m) = modifier {
        out.push(format!("{lang}@{m}"));
    }
    out.push(lang.to_string());
    out
}

/// Ein installiertes Programm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop-File-ID (`firefox.desktop`, `kde-foo.desktop`); zugleich die Ziel-ID der Häufigkeit.
    pub id: String,
    /// Pfad der Datei (aus `%k`).
    pub path: PathBuf,
    /// `Name` in der Sprache der Locale (ohne Übersetzung gleich `name_default`).
    pub name: String,
    /// `Name` ohne Locale.
    pub name_default: String,
    /// `GenericName` in der Sprache der Locale; leer, wenn es keinen gibt.
    pub generic_name: String,
    /// `GenericName` ohne Locale; leer, wenn es keinen gibt.
    pub generic_name_default: String,
    /// `Keywords` in der Sprache der Locale (ohne Übersetzung gleich `keywords_default`).
    pub keywords: Vec<String>,
    /// `Keywords` ohne Locale.
    pub keywords_default: Vec<String>,
    /// Programm und Argumente, Feldcodes bereits aufgelöst; nie leer, `exec_argv[0]` ist das Programm.
    pub exec_argv: Vec<String>,
    /// `Icon`: Name aus dem Icon-Theme oder absoluter Pfad.
    pub icon: Option<String>,
    /// `Terminal=true`: das Programm braucht ein Terminalfenster (der Aufrufer stellt das Terminal davor).
    pub terminal: bool,
    /// `Path`: Arbeitsverzeichnis, falls der Eintrag eines verlangt.
    pub working_dir: Option<PathBuf>,
    /// `StartupWMClass`: Fensterklasse, mit der sich Fenster dem Programm zuordnen lassen.
    pub startup_wm_class: Option<String>,
}

/// Alle gefundenen Programme, nach Name sortiert, mit Suche nach ID.
#[derive(Debug, Clone, Default)]
pub struct AppIndex {
    apps: Vec<App>,
    by_id: HashMap<String, usize>,
}

impl AppIndex {
    /// Behält die Reihenfolge; bei gleichen IDs gilt der erste Eintrag.
    pub fn new(apps: Vec<App>) -> AppIndex {
        let mut by_id = HashMap::with_capacity(apps.len());
        for (i, a) in apps.iter().enumerate() {
            by_id.entry(a.id.clone()).or_insert(i);
        }
        AppIndex { apps, by_id }
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.by_id.get(id).map(|&i| &self.apps[i])
    }
}

impl Deref for AppIndex {
    type Target = [App];

    fn deref(&self) -> &[App] {
        &self.apps
    }
}

/// Warum ein Eintrag nicht in der Liste steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    NotApplication,
    Hidden,
    NoDisplay,
    NoName,
    NoExec,
    BadExec(ExecError),
    /// `OnlyShowIn` nennt den aktuellen Desktop nicht.
    OnlyShowIn,
    /// `NotShowIn` nennt den aktuellen Desktop.
    NotShowIn,
    /// `TryExec` zeigt auf nichts Ausführbares.
    TryExec,
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reject::NotApplication => f.write_str("Type is not Application"),
            Reject::Hidden => f.write_str("Hidden=true"),
            Reject::NoDisplay => f.write_str("NoDisplay=true"),
            Reject::NoName => f.write_str("no Name"),
            Reject::NoExec => f.write_str("no Exec"),
            Reject::BadExec(e) => write!(f, "invalid Exec: {e}"),
            Reject::OnlyShowIn => f.write_str("OnlyShowIn excludes this desktop"),
            Reject::NotShowIn => f.write_str("NotShowIn names this desktop"),
            Reject::TryExec => f.write_str("TryExec not found or not executable"),
        }
    }
}

/// Fehler beim Zerlegen von `Exec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// Kein Programm (leer, oder das erste „Argument“ ist ein entfallender Feldcode).
    Empty,
    UnterminatedQuote,
    /// In Anführungszeichen ein Backslash vor etwas anderem als `"`, `` ` ``, `$`, `\`.
    BadEscape(char),
    /// Ein `"` mitten in einem nicht eingeschlossenen Argument.
    QuoteInsideArgument,
    /// Text direkt hinter dem schließenden Anführungszeichen.
    TextAfterQuote,
    /// `'`, `` ` `` oder `\` außerhalb von Anführungszeichen.
    UnquotedSyntax(char),
    /// Tabulator oder Zeilenumbruch außerhalb von Anführungszeichen.
    UnquotedWhitespace,
    /// Unbekannter Feldcode (`None`: ein einzelnes `%` am Ende).
    UnknownFieldCode(Option<char>),
    /// Feldcode (außer `%%`) in Anführungszeichen: laut Spec undefiniert.
    FieldCodeInQuotes(char),
    /// `%i` steht nicht allein als Argument.
    IconCodeNotAlone,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::Empty => f.write_str("no program"),
            ExecError::UnterminatedQuote => f.write_str("unterminated quote"),
            ExecError::BadEscape(c) => write!(f, "invalid escape \\{c} in quotes"),
            ExecError::QuoteInsideArgument => f.write_str("quote inside an unquoted argument"),
            ExecError::TextAfterQuote => f.write_str("text directly after a closing quote"),
            ExecError::UnquotedSyntax(c) => write!(f, "unquoted {c:?}"),
            ExecError::UnquotedWhitespace => f.write_str("unquoted tab or newline"),
            ExecError::UnknownFieldCode(Some(c)) => write!(f, "unknown field code %{c}"),
            ExecError::UnknownFieldCode(None) => f.write_str("lone % at the end"),
            ExecError::FieldCodeInQuotes(c) => write!(f, "field code %{c} inside quotes"),
            ExecError::IconCodeNotAlone => f.write_str("%i is not a separate argument"),
        }
    }
}

impl std::error::Error for ExecError {}

/// Werte für die Feldcodes `%i`, `%c` und `%k`.
#[derive(Debug, Clone, Copy)]
pub struct ExecContext<'a> {
    /// Lokalisierter Name (`%c`).
    pub name: &'a str,
    /// `Icon` (`%i`).
    pub icon: Option<&'a str>,
    /// Pfad der Desktop-Datei (`%k`).
    pub file: &'a Path,
}

/// Zerlegt den Wert von `Exec` *so, wie er in der Datei steht*: erst die Escapes der Datei (Typ `string`), dann
/// [`split_exec`]. Für Tests der Spec-Fälle und für den Eintrag-Leser.
pub fn parse_exec_value(raw: &str, cx: &ExecContext) -> Result<Vec<String>, ExecError> {
    split_exec(&unescape_string(raw), cx)
}

/// Zerlegt einen `Exec`-Wert, dessen Datei-Escapes schon aufgelöst sind (siehe Modulkopf), in eine argv-Liste.
pub fn split_exec(exec: &str, cx: &ExecContext) -> Result<Vec<String>, ExecError> {
    let chars: Vec<char> = exec.chars().collect();
    let mut args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' {
            i += 1;
        } else if chars[i] == '"' {
            i += 1;
            args.push(quoted_argument(&chars, &mut i)?);
        } else {
            unquoted_argument(&chars, &mut i, cx, &mut args)?;
        }
    }
    match args.first() {
        None => Err(ExecError::Empty),
        Some(program) if program.is_empty() => Err(ExecError::Empty),
        Some(_) => Ok(args),
    }
}

/// Liest ein Argument in Anführungszeichen; `i` steht hinter dem öffnenden und danach hinter dem schließenden.
fn quoted_argument(chars: &[char], i: &mut usize) -> Result<String, ExecError> {
    let mut buf = String::new();
    loop {
        let Some(&c) = chars.get(*i) else { return Err(ExecError::UnterminatedQuote) };
        *i += 1;
        match c {
            '"' => break,
            '\\' => match chars.get(*i) {
                Some(&n @ ('"' | '`' | '$' | '\\')) => {
                    buf.push(n);
                    *i += 1;
                }
                Some(&n) => return Err(ExecError::BadEscape(n)),
                None => return Err(ExecError::UnterminatedQuote),
            },
            '%' => match chars.get(*i) {
                Some('%') => {
                    buf.push('%');
                    *i += 1;
                }
                Some(&n) => return Err(ExecError::FieldCodeInQuotes(n)),
                None => return Err(ExecError::UnknownFieldCode(None)),
            },
            c => buf.push(c),
        }
    }
    match chars.get(*i) {
        None | Some(' ') => Ok(buf),
        Some(_) => Err(ExecError::TextAfterQuote),
    }
}

/// Liest ein Argument ohne Anführungszeichen bis zum nächsten Leerzeichen und hängt es (oder nichts, oder bei `%i` zwei)
/// an `args`.
fn unquoted_argument(chars: &[char], i: &mut usize, cx: &ExecContext, args: &mut Vec<String>) -> Result<(), ExecError> {
    let mut buf = String::new();
    // Hat das Argument Text? Sonst besteht es nur aus entfallenden Feldcodes und entfällt ganz.
    let mut has_text = false;
    let mut dropped = false;
    let mut icon_codes = 0;
    while let Some(&c) = chars.get(*i) {
        if c == ' ' {
            break;
        }
        *i += 1;
        match c {
            '"' => return Err(ExecError::QuoteInsideArgument),
            '\'' | '`' | '\\' => return Err(ExecError::UnquotedSyntax(c)),
            '\t' | '\n' | '\r' => return Err(ExecError::UnquotedWhitespace),
            '%' => {
                let code = chars.get(*i).copied().ok_or(ExecError::UnknownFieldCode(None))?;
                *i += 1;
                match code {
                    '%' => {
                        buf.push('%');
                        has_text = true;
                    }
                    'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm' => dropped = true,
                    'c' => {
                        buf.push_str(cx.name);
                        has_text = true;
                    }
                    'k' => {
                        buf.push_str(&cx.file.to_string_lossy());
                        has_text = true;
                    }
                    'i' => icon_codes += 1,
                    other => return Err(ExecError::UnknownFieldCode(Some(other))),
                }
            }
            c => {
                buf.push(c);
                has_text = true;
            }
        }
    }
    if icon_codes > 0 {
        if icon_codes != 1 || has_text || dropped {
            return Err(ExecError::IconCodeNotAlone);
        }
        if let Some(icon) = cx.icon.filter(|s| !s.is_empty()) {
            args.push("--icon".to_string());
            args.push(icon.to_string());
        }
    } else if has_text {
        args.push(buf);
    } else if args.is_empty() {
        // das erste Argument ist das Programm; ein Feldcode kann es nicht sein
        return Err(ExecError::Empty);
    }
    Ok(())
}

/// Escapes der Datei für Werte vom Typ `string`: `\s \n \t \r \\`. Ein unbekanntes `\x` bleibt samt Backslash stehen.
fn unescape_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Wert vom Typ `strings`: durch `;` getrennt (ein abschließendes `;` ist erlaubt), `\;` ist ein Semikolon im Element,
/// sonst die Escapes von [`unescape_string`]. Leere Elemente entfallen.
fn split_list(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        match c {
            ';' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '\\' => match it.next() {
                Some(';') => cur.push(';'),
                Some('s') => cur.push(' '),
                Some('n') => cur.push('\n'),
                Some('t') => cur.push('\t'),
                Some('r') => cur.push('\r'),
                Some('\\') => cur.push('\\'),
                Some(other) => {
                    cur.push('\\');
                    cur.push(other);
                }
                None => cur.push('\\'),
            },
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Die Schlüssel und rohen Werte der Gruppe `[Desktop Entry]` (Schlüssel mit Locale wie `Name[de]` unverändert).
/// `None`, wenn die Datei keine ist: Steuerzeichen NUL, die erste Gruppe nicht `[Desktop Entry]`, Werte vor der ersten
/// Gruppe, keine Gruppe `[Desktop Entry]`. Gleiche Schlüssel: der letzte gilt (wie glib). Zeilen ohne `=` entfallen.
fn parse_entry(text: &str) -> Option<HashMap<String, String>> {
    if text.contains('\0') {
        return None;
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut map = HashMap::new();
    let mut seen_group = false;
    let mut in_entry = false;
    let mut found_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if !seen_group && name != "Desktop Entry" {
                return None;
            }
            seen_group = true;
            in_entry = name == "Desktop Entry";
            found_entry |= in_entry;
        } else if !seen_group {
            return None;
        } else if in_entry {
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                if !k.is_empty() {
                    map.insert(k.to_string(), v.trim().to_string());
                }
            }
        }
    }
    found_entry.then_some(map)
}

/// Werte eines Eintrags mit Zugriff auf lokalisierte Schlüssel.
struct Entry<'a> {
    map: &'a HashMap<String, String>,
    locales: &'a [String],
}

impl Entry<'_> {
    fn raw(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    fn string(&self, key: &str) -> Option<String> {
        self.raw(key).map(unescape_string).filter(|s| !s.is_empty())
    }

    fn flag(&self, key: &str) -> bool {
        self.raw(key) == Some("true")
    }

    fn list(&self, key: &str) -> Vec<String> {
        self.raw(key).map(split_list).unwrap_or_default()
    }

    /// Roher Wert des besten lokalisierten Schlüssels, sonst des unlokalisierten.
    fn localized_raw(&self, key: &str) -> Option<&str> {
        self.locales.iter().find_map(|l| self.raw(&format!("{key}[{l}]")).filter(|v| !v.is_empty())).or_else(|| self.raw(key))
    }

    fn localized_string(&self, key: &str) -> Option<String> {
        self.localized_raw(key).map(unescape_string).filter(|s| !s.is_empty())
    }

    fn localized_list(&self, key: &str) -> Vec<String> {
        let found = self.locales.iter().find_map(|l| self.raw(&format!("{key}[{l}]")).filter(|v| !v.is_empty()));
        found.or_else(|| self.raw(key)).map(split_list).unwrap_or_default()
    }
}

/// Baut ein [`App`] oder nennt den Grund, warum der Eintrag nicht in die Liste kommt.
fn build_app(id: &str, path: &Path, map: &HashMap<String, String>, env: &DesktopEnv, locales: &[String]) -> Result<App, Reject> {
    let e = Entry { map, locales };
    if e.raw("Type") != Some("Application") {
        return Err(Reject::NotApplication);
    }
    if e.flag("Hidden") {
        return Err(Reject::Hidden);
    }
    if e.flag("NoDisplay") {
        return Err(Reject::NoDisplay);
    }
    let name = e.localized_string("Name").ok_or(Reject::NoName)?;
    let only = e.list("OnlyShowIn");
    let not = e.list("NotShowIn");
    if not.iter().any(|d| env.current_desktops.contains(d)) {
        return Err(Reject::NotShowIn);
    }
    if !only.is_empty() && !only.iter().any(|d| env.current_desktops.contains(d)) {
        return Err(Reject::OnlyShowIn);
    }
    if let Some(try_exec) = e.string("TryExec") {
        if !try_exec_found(&try_exec, &env.path) {
            return Err(Reject::TryExec);
        }
    }
    let icon = e.string("Icon");
    let exec = e.raw("Exec").filter(|s| !s.is_empty()).ok_or(Reject::NoExec)?;
    let cx = ExecContext { name: &name, icon: icon.as_deref(), file: path };
    let exec_argv = parse_exec_value(exec, &cx).map_err(Reject::BadExec)?;
    let keywords_default = e.list("Keywords");
    let keywords = e.localized_list("Keywords");
    Ok(App {
        id: id.to_string(),
        path: path.to_path_buf(),
        name_default: e.string("Name").unwrap_or_else(|| name.clone()),
        name,
        generic_name: e.localized_string("GenericName").unwrap_or_default(),
        generic_name_default: e.string("GenericName").unwrap_or_default(),
        keywords,
        keywords_default,
        exec_argv,
        icon,
        terminal: e.flag("Terminal"),
        working_dir: e.string("Path").map(PathBuf::from),
        startup_wm_class: e.string("StartupWMClass"),
    })
}

/// `TryExec`: absoluter Pfad oder Name, der in `PATH` gesucht wird; muss eine ausführbare Datei sein.
fn try_exec_found(try_exec: &str, path: &[PathBuf]) -> bool {
    let p = Path::new(try_exec);
    if p.is_absolute() {
        return is_executable(p);
    }
    path.iter().any(|dir| is_executable(&dir.join(p)))
}

/// Programm wie `dictate-ui` es startet (`ui::pal_exec::Programs::find`): ein Name ohne `/` im Suchpfad `path`, sonst
/// nur ein absoluter Pfad; jeweils eine ausführbare Datei. Für die Anzeigen der Palette (`super::super::widgets`).
pub(crate) fn find_program(name: &str, path: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    if name.contains('/') {
        let p = Path::new(name);
        return (p.is_absolute() && is_executable(p)).then(|| p.to_path_buf());
    }
    path.iter().map(|d| d.join(name)).find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    if !std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false) {
        return false;
    }
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else { return false };
    // SAFETY: `c` ist ein gültiger, NUL-terminierter C-String und lebt über den Aufruf hinaus.
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// Desktop-File-ID: Pfad relativ zu `applications`, `/` → `-`.
fn desktop_id(base: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(base).unwrap_or(file);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("-")
}

/// Ergebnis eines Durchlaufs.
#[derive(Debug, Clone)]
pub struct AppScan {
    pub apps: AppIndex,
    /// Gefundene Einträge, die nicht in `apps` stehen, mit ID und Grund (nach Fundreihenfolge).
    pub rejected: Vec<(String, Reject)>,
    /// Was der Durchlauf angesehen hat (für den Cache).
    pub fingerprint: Fingerprint,
}

struct Walk<'a> {
    env: &'a DesktopEnv,
    locales: Vec<String>,
    claimed: HashSet<String>,
    visited: HashSet<PathBuf>,
    apps: Vec<App>,
    rejected: Vec<(String, Reject)>,
    fingerprint: Fingerprint,
}

impl Walk<'_> {
    fn dir(&mut self, base: &Path, dir: &Path, depth: usize) {
        // vor dem Lesen messen (siehe `Fingerprint`)
        self.fingerprint.watch_mtime(dir);
        let Ok(canonical) = dir.canonicalize() else { return };
        if !self.visited.insert(canonical) {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    self.dir(base, &path, depth + 1);
                }
            } else if meta.is_file() && path.extension().is_some_and(|e| e == "desktop") {
                self.file(base, &path);
            }
        }
    }

    fn file(&mut self, base: &Path, path: &Path) {
        let id = desktop_id(base, path);
        if self.claimed.contains(&id) {
            return;
        }
        self.fingerprint.watch_mtime(path);
        let Some(text) = read_text(path) else { return };
        let Some(map) = parse_entry(&text) else { return };
        self.claimed.insert(id.clone());
        match build_app(&id, path, &map, self.env, &self.locales) {
            Ok(app) => self.apps.push(app),
            Err(reason) => {
                if matches!(reason, Reject::BadExec(_) | Reject::NoExec | Reject::NoName) {
                    tracing::debug!("palette: skipping desktop entry {id}: {reason}");
                }
                self.rejected.push((id, reason));
            }
        }
    }
}

/// Liest eine Datei als UTF-8, höchstens [`MAX_FILE_BYTES`].
fn read_text(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Liest alle Desktop-Einträge (siehe Modulkopf). Fehlende oder unlesbare Verzeichnisse und Dateien werden
/// übergangen.
pub fn scan_apps(env: &DesktopEnv) -> AppScan {
    let mut walk = Walk {
        env,
        locales: locale_candidates(&env.locale),
        claimed: HashSet::new(),
        visited: HashSet::new(),
        apps: Vec::new(),
        rejected: Vec::new(),
        fingerprint: Fingerprint::default(),
    };
    for base in env.application_dirs() {
        walk.dir(&base, &base, 0);
    }
    let mut apps = walk.apps;
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    tracing::debug!("palette: {} desktop entries, {} skipped", apps.len(), walk.rejected.len());
    AppScan { apps: AppIndex::new(apps), rejected: walk.rejected, fingerprint: walk.fingerprint }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn cx<'a>(icon: Option<&'a str>, file: &'a Path) -> ExecContext<'a> {
        ExecContext { name: "My App", icon, file }
    }

    /// Wert so, wie er in der Datei steht.
    fn file_value(raw: &str) -> Result<Vec<String>, ExecError> {
        parse_exec_value(raw, &cx(Some("my-icon"), Path::new("/usr/share/applications/my.desktop")))
    }

    fn argv(raw: &str) -> Vec<String> {
        file_value(raw).unwrap_or_else(|e| panic!("{raw:?} sollte zerlegt werden: {e}"))
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_arguments_split_at_spaces() {
        assert_eq!(argv("firefox"), strs(&["firefox"]));
        assert_eq!(argv("env FOO=bar  app --flag   x"), strs(&["env", "FOO=bar", "app", "--flag", "x"]));
        assert_eq!(argv("  app  "), strs(&["app"]));
    }

    #[test]
    fn quoted_arguments_keep_spaces() {
        assert_eq!(argv(r#""/opt/My App/bin/app" --flag"#), strs(&["/opt/My App/bin/app", "--flag"]));
        assert_eq!(argv(r#"app "--profile-directory=Profile 1" --x"#), strs(&["app", "--profile-directory=Profile 1", "--x"]));
        assert_eq!(argv(r#"app """#), strs(&["app", ""]), "ein ausdrücklich leeres Argument bleibt");
        assert_eq!(argv(r#"app "a  b""#), strs(&["app", "a  b"]));
        assert_eq!(argv(r#"claude-desktop "claude://code/new?source=desktop_action""#), strs(&["claude-desktop", "claude://code/new?source=desktop_action"]));
    }

    #[test]
    fn escapes_inside_quotes() {
        // in der Datei steht `\"`: die Dateiebene kennt es nicht und lässt es stehen, die Exec-Ebene löst es auf
        assert_eq!(argv(r#"app "say \"hi\"""#), strs(&["app", r#"say "hi""#]));
        // `\`` und `\$` genauso
        assert_eq!(argv(r#"app "a\`b" "c\$d""#), strs(&["app", "a`b", "c$d"]));
    }

    #[test]
    fn file_level_escapes_come_first() {
        // Spec: ein wörtlicher Backslash im Argument braucht in der Datei vier Backslashes ...
        assert_eq!(argv(r#"app "back\\\\slash""#), strs(&["app", r"back\slash"]));
        // ... ein Dollarzeichen `\\$` (Beispiel aus einem echten Eintrag)
        assert_eq!(argv(r#"/bin/sh -c "\\$SHELL -i -c scrcpy""#), strs(&["/bin/sh", "-c", "$SHELL -i -c scrcpy"]));
        assert_eq!(argv(r#"app "tick\\`""#), strs(&["app", "tick`"]));
        // zwei Backslashes in der Datei sind nur *ein* Backslash auf der Exec-Ebene: dort ist er außerhalb von
        // Anführungszeichen unzulässig, in ihnen ein unvollständiges Escape
        assert_eq!(file_value(r#"app a\\b"#), Err(ExecError::UnquotedSyntax('\\')));
        assert_eq!(file_value(r#"app "a\\b""#), Err(ExecError::BadEscape('b')));
        // `\s` der Dateiebene ist ein Leerzeichen und trennt Argumente (erst Escapes, dann Quoting)
        assert_eq!(argv(r"app\sone"), strs(&["app", "one"]));
        assert_eq!(argv(r#"app "a\sb""#), strs(&["app", "a b"]));
        // `\n` und `\t` in Anführungszeichen sind wörtliche Zeichen
        assert_eq!(argv(r#"app "a\nb\tc""#), strs(&["app", "a\nb\tc"]));
    }

    #[test]
    fn reserved_characters_are_literal_without_a_shell() {
        assert_eq!(argv(r#"sh -c "optiva;${SHELL:-bash}""#), strs(&["sh", "-c", "optiva;${SHELL:-bash}"]));
        assert_eq!(argv("app http://x.y/?a=1&b=2;c|d>e<f*g~h#i(j)"), strs(&["app", "http://x.y/?a=1&b=2;c|d>e<f*g~h#i(j)"]));
        assert_eq!(argv(r#"app "<LAT>&lon=<LON>""#), strs(&["app", "<LAT>&lon=<LON>"]));
    }

    #[test]
    fn file_and_url_codes_are_removed() {
        assert_eq!(argv("app %f"), strs(&["app"]));
        assert_eq!(argv("app %F %u %U"), strs(&["app"]));
        assert_eq!(argv("app --new-window %U"), strs(&["app", "--new-window"]));
        assert_eq!(argv("app %u --after"), strs(&["app", "--after"]));
        assert_eq!(argv("app %f%u"), strs(&["app"]), "mehrere entfallende Codes in einem Argument");
        // mitten in einem Argument: durch nichts ersetzt, der Rest bleibt
        assert_eq!(argv("app --uri=%u"), strs(&["app", "--uri="]));
        assert_eq!(argv("vlc bluray://%f"), strs(&["vlc", "bluray://"]));
    }

    #[test]
    fn deprecated_codes_are_removed() {
        assert_eq!(argv("app %d %D %n %N %v %m x"), strs(&["app", "x"]));
        assert_eq!(argv("app --x=%d"), strs(&["app", "--x="]));
    }

    #[test]
    fn icon_name_and_location_codes() {
        assert_eq!(argv("app %i"), strs(&["app", "--icon", "my-icon"]));
        assert_eq!(argv("app %i --x"), strs(&["app", "--icon", "my-icon", "--x"]));
        assert_eq!(argv("app --title=%c"), strs(&["app", "--title=My App"]), "%c bleibt ein Argument, auch mit Leerzeichen");
        assert_eq!(argv("app %c"), strs(&["app", "My App"]));
        assert_eq!(argv("app %k"), strs(&["app", "/usr/share/applications/my.desktop"]));
        assert_eq!(argv("app 100%%"), strs(&["app", "100%"]));
        assert_eq!(argv("app %%f"), strs(&["app", "%f"]), "%% ist ein Prozentzeichen, danach kein Feldcode");
        assert_eq!(argv(r#"app "100%%""#), strs(&["app", "100%"]), "%% ist auch in Anführungszeichen ein Prozentzeichen");
        // ohne Icon entfällt %i
        let no_icon = parse_exec_value("app %i x", &cx(None, Path::new("/x.desktop"))).unwrap();
        assert_eq!(no_icon, strs(&["app", "x"]));
        let empty_icon = parse_exec_value("app %i x", &cx(Some(""), Path::new("/x.desktop"))).unwrap();
        assert_eq!(empty_icon, strs(&["app", "x"]));
    }

    #[test]
    fn broken_exec_is_rejected() {
        let bad: &[(&str, ExecError)] = &[
            ("", ExecError::Empty),
            ("   ", ExecError::Empty),
            ("%U", ExecError::Empty),
            ("%f app", ExecError::Empty),
            (r#""""#, ExecError::Empty),
            (r#"app "unterminated"#, ExecError::UnterminatedQuote),
            (r#"app "ends with escape\"#, ExecError::UnterminatedQuote),
            (r#"app "bad \x escape""#, ExecError::BadEscape('x')),
            (r#"app "a\ b""#, ExecError::BadEscape(' ')),
            (r#"app a"b c""#, ExecError::QuoteInsideArgument),
            (r#"app WINEPREFIX="/home/u/.wine" wine"#, ExecError::QuoteInsideArgument),
            (r#"app "a"b"#, ExecError::TextAfterQuote),
            (r#"app "a""b""#, ExecError::TextAfterQuote),
            ("sh -c 'echo hi'", ExecError::UnquotedSyntax('\'')),
            ("app a`date`", ExecError::UnquotedSyntax('`')),
            (r"app a\ b", ExecError::UnquotedSyntax('\\')),
            ("app a\tb", ExecError::UnquotedWhitespace),
            (r"app a\nb", ExecError::UnquotedWhitespace),
            ("app %x", ExecError::UnknownFieldCode(Some('x'))),
            ("app %", ExecError::UnknownFieldCode(None)),
            ("app 50%", ExecError::UnknownFieldCode(None)),
            ("app %I", ExecError::UnknownFieldCode(Some('I'))),
            (r#"app "%f""#, ExecError::FieldCodeInQuotes('f')),
            (r#"app "--title=%c""#, ExecError::FieldCodeInQuotes('c')),
            ("app --icon=%i", ExecError::IconCodeNotAlone),
            ("app %i%i", ExecError::IconCodeNotAlone),
            ("app %i%f", ExecError::IconCodeNotAlone),
        ];
        for (raw, want) in bad {
            assert_eq!(file_value(raw).as_ref().err(), Some(want), "{raw:?}");
        }
    }

    #[test]
    fn locale_candidates_follow_the_spec_order() {
        assert_eq!(locale_candidates("de_AT.UTF-8@euro"), ["de_AT@euro", "de_AT", "de@euro", "de"]);
        assert_eq!(locale_candidates("de_AT.UTF-8"), ["de_AT", "de"]);
        assert_eq!(locale_candidates("sr_RS@latin"), ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]);
        assert_eq!(locale_candidates("en_US.UTF-8"), ["en_US", "en"]);
        assert_eq!(locale_candidates("de"), ["de"]);
        assert_eq!(locale_candidates("de@euro"), ["de@euro", "de"]);
        assert!(locale_candidates("C").is_empty());
        assert!(locale_candidates("C.UTF-8").is_empty());
        assert!(locale_candidates("POSIX").is_empty());
        assert!(locale_candidates("").is_empty());
    }

    #[test]
    fn env_lookup_applies_defaults_and_ignores_relative_paths() {
        let vars: HashMap<&str, &str> = [
            ("XDG_DATA_DIRS", "rel/x:/opt/share:/usr/share"),
            ("LANG", "de_DE.UTF-8"),
            ("LC_MESSAGES", "fr_FR.UTF-8"),
            ("XDG_CURRENT_DESKTOP", "KDE:awesome"),
            ("PATH", "/usr/bin::bin:/opt/bin"),
        ]
        .into_iter()
        .collect();
        let env = DesktopEnv::from_lookup(|k| vars.get(k).map(|v| v.to_string()), Some(PathBuf::from("/home/u")));
        assert_eq!(env.data_home, Some(PathBuf::from("/home/u/.local/share")));
        assert_eq!(env.data_dirs, [PathBuf::from("/opt/share"), PathBuf::from("/usr/share")]);
        assert_eq!(env.locale, "fr_FR.UTF-8", "LC_MESSAGES vor LANG");
        assert_eq!(env.current_desktops, ["KDE", "awesome"]);
        assert_eq!(env.path, [PathBuf::from("/usr/bin"), PathBuf::from("/opt/bin")]);
        assert_eq!(
            env.application_dirs(),
            [PathBuf::from("/home/u/.local/share/applications"), PathBuf::from("/opt/share/applications"), PathBuf::from("/usr/share/applications")]
        );

        let none = DesktopEnv::from_lookup(|_| None, None);
        assert_eq!(none.data_home, None);
        assert_eq!(none.data_dirs, [PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
        assert_eq!(none.locale, "");
        assert!(none.current_desktops.is_empty());

        let all = DesktopEnv::from_lookup(|k| (k == "LC_ALL").then(|| "es_ES.UTF-8".to_string()).or_else(|| (k == "LANG").then(|| "de_DE".to_string())), None);
        assert_eq!(all.locale, "es_ES.UTF-8", "LC_ALL vor allem anderen");
        let abs_home = DesktopEnv::from_lookup(|k| (k == "XDG_DATA_HOME").then(|| "/data/h".to_string()), Some(PathBuf::from("/home/u")));
        assert_eq!(abs_home.data_home, Some(PathBuf::from("/data/h")));
        let rel_home = DesktopEnv::from_lookup(|k| (k == "XDG_DATA_HOME").then(|| "data/h".to_string()), Some(PathBuf::from("/home/u")));
        assert_eq!(rel_home.data_home, Some(PathBuf::from("/home/u/.local/share")), "relativer XDG_DATA_HOME ist ungültig");
    }

    // ---- Dateien ----

    fn app_file(extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName=Prog\nExec=prog %U\n{extra}")
    }

    fn env_for(t: &TempDir) -> DesktopEnv {
        DesktopEnv {
            data_home: Some(t.join("home")),
            data_dirs: vec![t.join("sys1"), t.join("sys2")],
            current_desktops: vec!["awesome".into()],
            locale: "en_US.UTF-8".into(),
            path: vec![t.join("bin")],
        }
    }

    fn ids(scan: &AppScan) -> Vec<String> {
        scan.apps.iter().map(|a| a.id.clone()).collect()
    }

    fn rejected(scan: &AppScan, id: &str) -> Option<Reject> {
        scan.rejected.iter().find(|(i, _)| i == id).map(|(_, r)| r.clone())
    }

    #[test]
    fn reads_a_complete_entry() {
        let t = TempDir::new("desktop-complete");
        let path = t.write(
            "sys1/applications/org.example.Prog.desktop",
            "# Kommentar\n[Desktop Entry]\nType=Application\nName = Prog \nName[de]=Programm\nGenericName=Editor\nGenericName[de]=Texteditor\n\
             Keywords=text;edit;\nKeywords[de]=Text;Bearbeiten;\nExec=prog --x %U\nIcon=prog-icon\nTerminal=true\nPath=/tmp\nStartupWMClass=Prog\n\
             \n[Desktop Action new]\nName=New\nExec=prog --new\n",
        );
        let mut env = env_for(&t);
        env.locale = "de_DE.UTF-8".into();
        let scan = scan_apps(&env);
        let app = scan.apps.get("org.example.Prog.desktop").unwrap();
        assert_eq!(app.name, "Programm");
        assert_eq!(app.name_default, "Prog");
        assert_eq!(app.generic_name, "Texteditor");
        assert_eq!(app.generic_name_default, "Editor");
        assert_eq!(app.keywords, ["Text", "Bearbeiten"]);
        assert_eq!(app.keywords_default, ["text", "edit"]);
        assert_eq!(app.exec_argv, ["prog", "--x"]);
        assert_eq!(app.icon.as_deref(), Some("prog-icon"));
        assert!(app.terminal);
        assert_eq!(app.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(app.startup_wm_class.as_deref(), Some("Prog"));
        assert_eq!(app.path, path);
        assert!(scan.rejected.is_empty(), "Gruppen außer [Desktop Entry] zählen nicht als Eintrag: {:?}", scan.rejected);
    }

    #[test]
    fn minimal_entry_has_sane_defaults() {
        let t = TempDir::new("desktop-minimal");
        t.write("home/applications/p.desktop", &app_file(""));
        let scan = scan_apps(&env_for(&t));
        let app = &scan.apps[0];
        assert_eq!((app.name.as_str(), app.name_default.as_str()), ("Prog", "Prog"));
        assert_eq!(app.generic_name, "");
        assert!(app.keywords.is_empty() && app.keywords_default.is_empty());
        assert_eq!(app.icon, None);
        assert!(!app.terminal);
        assert_eq!(app.working_dir, None);
        assert_eq!(app.startup_wm_class, None);
    }

    #[test]
    fn earlier_directories_shadow_later_ones() {
        let t = TempDir::new("desktop-shadow");
        t.write("home/applications/foo.desktop", &app_file("").replace("Prog", "FromHome"));
        t.write("sys1/applications/foo.desktop", &app_file("").replace("Prog", "FromSys1"));
        t.write("sys2/applications/foo.desktop", &app_file("").replace("Prog", "FromSys2"));
        t.write("sys2/applications/bar.desktop", &app_file("").replace("Prog", "OnlySys2"));
        t.write("sys1/applications/baz.desktop", &app_file("").replace("Prog", "Sys1Baz"));
        t.write("sys2/applications/baz.desktop", &app_file("").replace("Prog", "Sys2Baz"));
        let scan = scan_apps(&env_for(&t));
        assert_eq!(scan.apps.get("foo.desktop").unwrap().name, "FromHome");
        assert_eq!(scan.apps.get("bar.desktop").unwrap().name, "OnlySys2");
        assert_eq!(scan.apps.get("baz.desktop").unwrap().name, "Sys1Baz");
        assert_eq!(scan.apps.len(), 3);
    }

    #[test]
    fn hidden_and_no_display_entries_mask_later_ones() {
        let t = TempDir::new("desktop-mask");
        // der Benutzer „löscht“ einen Systemeintrag
        t.write("home/applications/del.desktop", &app_file("Hidden=true\n"));
        t.write("sys1/applications/del.desktop", &app_file(""));
        t.write("home/applications/nd.desktop", &app_file("NoDisplay=true\n"));
        t.write("sys1/applications/nd.desktop", &app_file(""));
        t.write("sys1/applications/shown.desktop", &app_file(""));
        let scan = scan_apps(&env_for(&t));
        assert_eq!(ids(&scan), ["shown.desktop"]);
        assert_eq!(rejected(&scan, "del.desktop"), Some(Reject::Hidden));
        assert_eq!(rejected(&scan, "nd.desktop"), Some(Reject::NoDisplay));
        assert_eq!(scan.rejected.len(), 2, "die verdeckten Systemdateien tauchen nicht auf");
    }

    #[test]
    fn subdirectories_become_part_of_the_id() {
        let t = TempDir::new("desktop-subdir");
        t.write("sys1/applications/kde/foo.desktop", &app_file(""));
        t.write("sys1/applications/a/b/c.desktop", &app_file(""));
        t.write("sys1/applications/kde-foo.desktop", &app_file("").replace("Prog", "Flat"));
        t.write("sys1/applications/not-a-desktop-file.txt", "x");
        let scan = scan_apps(&env_for(&t));
        assert!(scan.apps.get("a-b-c.desktop").is_some());
        assert!(scan.apps.get("kde-foo.desktop").is_some(), "kde/foo.desktop und kde-foo.desktop haben dieselbe ID: eine gilt");
        assert_eq!(scan.apps.len(), 2);
    }

    #[test]
    fn only_applications_are_listed() {
        let t = TempDir::new("desktop-type");
        t.write("home/applications/link.desktop", "[Desktop Entry]\nType=Link\nName=L\nURL=http://x\n");
        t.write("home/applications/dir.desktop", "[Desktop Entry]\nType=Directory\nName=D\n");
        t.write("home/applications/notype.desktop", "[Desktop Entry]\nName=N\nExec=n\n");
        t.write("home/applications/noname.desktop", "[Desktop Entry]\nType=Application\nExec=n\n");
        t.write("home/applications/noexec.desktop", "[Desktop Entry]\nType=Application\nName=X\n");
        t.write("home/applications/badexec.desktop", "[Desktop Entry]\nType=Application\nName=X\nExec=x \"oops\n");
        t.write("home/applications/ok.desktop", &app_file(""));
        let scan = scan_apps(&env_for(&t));
        assert_eq!(ids(&scan), ["ok.desktop"]);
        assert_eq!(rejected(&scan, "link.desktop"), Some(Reject::NotApplication));
        assert_eq!(rejected(&scan, "dir.desktop"), Some(Reject::NotApplication));
        assert_eq!(rejected(&scan, "notype.desktop"), Some(Reject::NotApplication));
        assert_eq!(rejected(&scan, "noname.desktop"), Some(Reject::NoName));
        assert_eq!(rejected(&scan, "noexec.desktop"), Some(Reject::NoExec));
        assert_eq!(rejected(&scan, "badexec.desktop"), Some(Reject::BadExec(ExecError::UnterminatedQuote)));
    }

    #[test]
    fn invalid_files_are_ignored_and_do_not_mask() {
        let t = TempDir::new("desktop-invalid");
        t.write("home/applications/x.desktop", "Name=no group\nExec=x\n");
        t.write("sys1/applications/x.desktop", &app_file(""));
        t.write("home/applications/y.desktop", "[Other]\nName=wrong first group\n");
        t.write("home/applications/z.desktop", "[Desktop Entry]\nType=Application\nName=Z\0\nExec=z\n");
        std::fs::write(t.join("home/applications/utf.desktop"), b"[Desktop Entry]\nType=Application\nName=\xff\nExec=u\n").unwrap();
        std::fs::write(t.join("home/applications/big.desktop"), vec![b'#'; 300 * 1024]).unwrap();
        std::os::unix::fs::symlink(t.join("nowhere"), t.join("home/applications/dangling.desktop")).unwrap();
        let scan = scan_apps(&env_for(&t));
        assert_eq!(ids(&scan), ["x.desktop"], "eine unlesbare Datei verdeckt die spätere nicht");
        assert!(scan.rejected.is_empty());
    }

    #[test]
    fn show_in_lists_are_checked_against_the_current_desktop() {
        let t = TempDir::new("desktop-showin");
        t.write("home/applications/only-gnome.desktop", &app_file("OnlyShowIn=GNOME;KDE;\n"));
        t.write("home/applications/only-awesome.desktop", &app_file("OnlyShowIn=GNOME;awesome;\n"));
        t.write("home/applications/not-awesome.desktop", &app_file("NotShowIn=awesome;\n"));
        t.write("home/applications/not-gnome.desktop", &app_file("NotShowIn=GNOME;\n"));
        t.write("home/applications/both.desktop", &app_file("OnlyShowIn=awesome;\nNotShowIn=awesome;\n"));
        t.write("home/applications/case.desktop", &app_file("OnlyShowIn=AWESOME;\n"));
        t.write("home/applications/plain.desktop", &app_file(""));
        let scan = scan_apps(&env_for(&t));
        assert_eq!(ids(&scan), ["not-gnome.desktop", "only-awesome.desktop", "plain.desktop"]);
        assert_eq!(rejected(&scan, "only-gnome.desktop"), Some(Reject::OnlyShowIn));
        assert_eq!(rejected(&scan, "not-awesome.desktop"), Some(Reject::NotShowIn));
        assert_eq!(rejected(&scan, "both.desktop"), Some(Reject::NotShowIn));
        assert_eq!(rejected(&scan, "case.desktop"), Some(Reject::OnlyShowIn), "die Namen sind Zeichenketten, Groß/klein zählt");

        // ohne Desktop (XDG_CURRENT_DESKTOP leer): nur OnlyShowIn blendet aus
        let mut env = env_for(&t);
        env.current_desktops.clear();
        let scan = scan_apps(&env);
        assert_eq!(ids(&scan), ["not-awesome.desktop", "not-gnome.desktop", "plain.desktop"]);

        // mehrere Desktops (`KDE:awesome`): einer genügt
        env.current_desktops = vec!["KDE".into(), "awesome".into()];
        let scan = scan_apps(&env);
        assert_eq!(ids(&scan), ["not-gnome.desktop", "only-awesome.desktop", "only-gnome.desktop", "plain.desktop"]);
    }

    #[test]
    fn try_exec_is_looked_up_in_path_and_must_be_executable() {
        let t = TempDir::new("desktop-tryexec");
        let tool = t.write("bin/mytool", "#!/bin/sh\n");
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = t.write("bin/notexec", "data");
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        t.mkdir("bin/adir");
        t.write("home/applications/ok-path.desktop", &app_file("TryExec=mytool\n"));
        t.write("home/applications/ok-abs.desktop", &app_file(&format!("TryExec={}\n", tool.display())));
        t.write("home/applications/missing.desktop", &app_file("TryExec=nosuchtool\n"));
        t.write("home/applications/notexec.desktop", &app_file("TryExec=notexec\n"));
        t.write("home/applications/dir.desktop", &app_file("TryExec=adir\n"));
        t.write("home/applications/abs-missing.desktop", &app_file("TryExec=/nonexistent/tool\n"));
        t.write("home/applications/none.desktop", &app_file(""));
        let scan = scan_apps(&env_for(&t));
        assert_eq!(ids(&scan), ["none.desktop", "ok-abs.desktop", "ok-path.desktop"]);
        for id in ["missing.desktop", "notexec.desktop", "dir.desktop", "abs-missing.desktop"] {
            assert_eq!(rejected(&scan, id), Some(Reject::TryExec), "{id}");
        }
        // ohne PATH wird ein Name nirgends gefunden
        let mut env = env_for(&t);
        env.path.clear();
        assert_eq!(ids(&scan_apps(&env)), ["none.desktop", "ok-abs.desktop"]);
    }

    #[test]
    fn localized_names_follow_the_fallback_rules() {
        let t = TempDir::new("desktop-locale");
        t.write(
            "home/applications/l.desktop",
            &app_file("Name[de]=Deutsch\nName[de_AT]=Österreichisch\nName[de@euro]=Euro\nName[sr@latin]=Latin\nName[C]=C-Name\nGenericName=Plain\nGenericName[de]=Allgemein\n")
                .replace("Name=Prog", "Name=Default"),
        );
        let name_for = |locale: &str| {
            let mut env = env_for(&t);
            env.locale = locale.into();
            let scan = scan_apps(&env);
            let app = scan.apps[0].clone();
            (app.name, app.name_default, app.generic_name, app.generic_name_default)
        };
        let s = |a: &str, b: &str, c: &str, d: &str| (a.to_string(), b.to_string(), c.to_string(), d.to_string());
        assert_eq!(name_for("de_AT.UTF-8"), s("Österreichisch", "Default", "Allgemein", "Plain"));
        assert_eq!(name_for("de_DE.UTF-8"), s("Deutsch", "Default", "Allgemein", "Plain"));
        assert_eq!(name_for("de@euro"), s("Euro", "Default", "Allgemein", "Plain"));
        assert_eq!(name_for("de_CH@other"), s("Deutsch", "Default", "Allgemein", "Plain"));
        assert_eq!(name_for("sr_RS@latin"), s("Latin", "Default", "Plain", "Plain"));
        assert_eq!(name_for("fr_FR.UTF-8"), s("Default", "Default", "Plain", "Plain"));
        assert_eq!(name_for("C"), s("Default", "Default", "Plain", "Plain"), "Name[C] ist kein Schlüssel für die C-Locale");
        assert_eq!(name_for(""), s("Default", "Default", "Plain", "Plain"));
    }

    #[test]
    fn exec_in_a_file_uses_the_localized_name_and_the_file_path() {
        let t = TempDir::new("desktop-exec-codes");
        let path = t.write("home/applications/c.desktop", &app_file("Name[de]=Namen\nIcon=ico\nExec=prog --title=%c %i %k %U\n"));
        let mut env = env_for(&t);
        env.locale = "de_DE".into();
        let scan = scan_apps(&env);
        let want = ["prog".to_string(), "--title=Namen".to_string(), "--icon".to_string(), "ico".to_string(), path.display().to_string()];
        assert_eq!(scan.apps[0].exec_argv, want);
    }

    #[test]
    fn exec_escape_levels_in_a_file() {
        let t = TempDir::new("desktop-exec-escapes");
        t.write("home/applications/e.desktop", "[Desktop Entry]\nType=Application\nName=E\nExec=/bin/sh -c \"\\\\$SHELL -i -c 'scrcpy'\" \"a\\\\\\\\b\"\n");
        let scan = scan_apps(&env_for(&t));
        assert_eq!(scan.apps[0].exec_argv, ["/bin/sh", "-c", "$SHELL -i -c 'scrcpy'", r"a\b"]);
    }

    #[test]
    fn keys_may_repeat_and_the_last_one_wins() {
        let t = TempDir::new("desktop-dup");
        t.write("home/applications/d.desktop", &app_file("Name=Second\n"));
        assert_eq!(scan_apps(&env_for(&t)).apps[0].name, "Second");
    }

    #[test]
    fn crlf_bom_and_comments_are_tolerated() {
        let t = TempDir::new("desktop-crlf");
        t.write("home/applications/w.desktop", "\u{feff}# c\r\n[Desktop Entry]\r\nType=Application\r\nName=Win\r\nExec=win\r\n");
        let scan = scan_apps(&env_for(&t));
        assert_eq!(scan.apps[0].name, "Win");
        assert_eq!(scan.apps[0].exec_argv, ["win"]);
    }

    #[test]
    fn list_values_honor_escaped_semicolons() {
        assert_eq!(split_list(r"a;b\;c;;d;"), ["a", "b;c", "d"]);
        assert_eq!(split_list(""), Vec::<String>::new());
        assert_eq!(split_list(r"x\sy;z"), ["x y", "z"]);
    }

    #[test]
    fn symlinks_are_followed_and_loops_end() {
        let t = TempDir::new("desktop-links");
        let real = t.write("elsewhere/real.desktop", &app_file(""));
        t.mkdir("home/applications");
        std::os::unix::fs::symlink(&real, t.join("home/applications/linked.desktop")).unwrap();
        std::os::unix::fs::symlink(t.join("elsewhere"), t.join("home/applications/dirlink")).unwrap();
        std::os::unix::fs::symlink(t.join("home/applications"), t.join("home/applications/loop")).unwrap();
        let scan = scan_apps(&env_for(&t));
        assert!(scan.apps.get("linked.desktop").is_some());
        assert!(scan.apps.get("dirlink-real.desktop").is_some());
        assert_eq!(scan.apps.len(), 2, "die Schleife erzeugt nichts doppelt");
    }

    #[test]
    fn apps_are_sorted_by_name() {
        let t = TempDir::new("desktop-sort");
        t.write("home/applications/b.desktop", &app_file("").replace("Prog", "beta"));
        t.write("home/applications/a.desktop", &app_file("").replace("Prog", "Gamma"));
        t.write("home/applications/c.desktop", &app_file("").replace("Prog", "Alpha"));
        assert_eq!(ids(&scan_apps(&env_for(&t))), ["c.desktop", "b.desktop", "a.desktop"]);
    }

    #[test]
    fn missing_directories_are_fine_and_observed() {
        let t = TempDir::new("desktop-missing");
        let scan = scan_apps(&env_for(&t));
        assert!(scan.apps.is_empty());
        assert_eq!(scan.fingerprint.len(), 3, "home, sys1 und sys2 werden beobachtet, auch wenn es sie noch nicht gibt");
        assert!(scan.fingerprint.is_current());
        t.mkdir("home/applications");
        assert!(!scan.fingerprint.is_current());
    }
}
