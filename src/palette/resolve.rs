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

//! Lokaler Resolver der Befehlspalette (Aufgabe 2 in `PLAN-palette.md`, Stufe 1): Eingabe + Kontext + Quellen →
//! sortierte Kandidaten, ohne Modell, bei jedem Tastendruck.
//!
//! ```text
//! resolve(eingabe, kontext, programme, projekte, häufigkeit, jetzt) -> Resolution { candidates, confident }
//! ```
//!
//! Jeder [`Candidate`] trägt eine fertig validierte [`Intent`], das konkrete Ziel ([`Target`]: Fenster-ID, ID der
//! Arbeitsfläche, Desktop-ID, Projektpfad; das Panel sucht nie neu nach einem Namen), eine englische Anzeigezeile, Klasse und die Ziel-ID für
//! die Häufigkeit. Jede Absicht besteht [`Intent::validate`]; was die Prüfung nicht besteht, wird verworfen, nie
//! ausgegeben.
//!
//! # Wie die Kandidaten entstehen
//!
//! 1. **Feste Regeln** (Güte 100, „direkt“): `?frage` → `ask` (und nichts sonst), `g begriff` → `web_search`, eine
//!    Adresse → `open_url` (`http(s)://…` oder `www.…`/`domain.tld` mit bekannter Endung, dann `https`), eine Zahl →
//!    `view_workspace` (Arbeitsfläche mit diesem `idx` auf dem fokussierten Monitor, nur wenn es sie gibt), `volume 40` →
//!    `volume set`, `verschiebe nach 3` → `move_window_to_workspace` für das fokussierte Fenster (Zahl auf dessen Monitor).
//!    Befehlspräfixe stehen als Daten in [`keywords::PREFIXES`]; „projekt dataroom“, „open dataroom“, „claude dataroom“,
//!    „starte firefox“, „close firefox“ suchen den Rest nur dort, wo das Präfix es sagt.
//! 2. **Schlüsselwörter** ([`keywords::KEYWORDS`]): Absichten mit deutschen und englischen Wendungen („sperren“,
//!    „lauter“, „nächster titel“, „neustart“ …), bewertet wie jeder Text.
//! 3. **Fuzzy-Suche** ([`fuzzy`]) in Programmen (Name lokalisiert und Vorgabe, GenericName, Keywords, Desktop-ID),
//!    Projekten (Name, Gruppe), Fenstern (`app_id`, Titel, Name des zugehörigen Programms) und Arbeitsflächen (Name;
//!    „workspace N“, „tag N“, „arbeitsfläche N“ nur auf dem fokussierten Monitor).
//!
//! **Arbeitsflächen** (Vorlage E2 in `PLAN-wayland.md` 6g): eine Zahl meint `idx` auf dem Monitor des Zielfensters (beim
//! Anzeigen das fokussierte, beim Verschieben das verschobene); Treffer nur für Arbeitsflächen im Kontext (niri führt
//! die leere am Ende mit), nie eine erfundene. Ein Name trifft auf jedem Monitor. Ziel ist immer die feste ID.
//!
//! Ein Fenster, dessen `app_id` zu einem Programm gehört, steht vor dem Neustart desselben Programms („Firefox — switch
//! to window“ vor „Firefox — launch“). **Leere Eingabe:** zuletzt und oft gewählte Ziele nach Häufigkeit, danach die
//! laufenden Fenster.
//!
//! # Reihenfolge und Häufigkeit
//!
//! `score = Güte + Aufschlag + Häufigkeitsbonus`. Die Güte (0 bis 100, siehe [`fuzzy`]) bestimmt die Reihenfolge; der
//! Häufigkeitsbonus ist auf [`FRECENCY_MAX`] begrenzt (logarithmisch, gesättigt ab Häufigkeit 50) und damit kleiner als
//! der Abstand zwischen den Stufen Präfix, Wortanfang und Fuzzy: er sortiert Gleichwertiges, überholt aber nie einen
//! klar besseren Texttreffer. Laufende Fenster bekommen [`RUNNING_BONUS`]; das fokussierte Fenster verliert
//! [`FOCUSED_PENALTY`] (dorthin zu wechseln ist sinnlos).
//!
//! # Wann ein Treffer „sicher“ ist (`confident`)
//!
//! Ist der erste Kandidat sicher, führt Return ihn ohne das Modell aus (Klasse `confirm` fragt trotzdem nach). Regel:
//!
//! - die Güte des ersten Kandidaten ist mindestens [`CONFIDENT_MIN_QUALITY`] (88: genau das Feld oder sein Anfang, nie
//!   ein Treffer in der Mitte oder ein Fuzzy-Treffer wie „ff“), und
//! - bei einer Eingabe aus weniger als [`CONFIDENT_MIN_CHARS`] Zeichen nur, wenn eine feste Regel ihn lieferte
//!   (eine Zahl „3“, nicht der Buchstabe „f“), und
//! - kein anderes Ziel ist fast so gut: ist der erste *exakt* (Güte 100), darf kein zweiter exakt sein; sonst muss der
//!   Abstand zur besten Güte aller übrigen mindestens [`CONFIDENT_GAP`] (12) betragen. Der Neustart desselben Programms
//!   zählt nicht als Konkurrent eines laufenden Fensters. Zwei Fenster desselben Programms sind einander Konkurrenz:
//!   welches gemeint ist, weiß nur der User.
//!
//! Begründung: „sperr“ (Präfix von „sperren“, nichts anderes nah dran) und „lock“ (exakt) sind sicher; „f“ (Firefox,
//! Files …), „clo“ (Close, Clock …), „ff“ (Fuzzy) und Unsinn sind es nicht, dann geht Return an das Modell
//! (Aufgabe 4). Häufigkeit zählt nie für die Sicherheit: eine Gewohnheit macht einen Treffer nicht richtiger.
//!
//! # Grenzen
//!
//! Eingabe höchstens [`MAX_INPUT_CHARS`] Zeichen (mehr wird abgeschnitten); Fuzzy-Suche nur bis acht Wörter und 64 Zeichen
//! (mehr ist ein Satz für das Modell); höchstens [`MAX_CANDIDATES`] Kandidaten. Laufzeit unter 10 ms für 500 Programme,
//! 200 Projekte und 100 Fenster bei 20 Zeichen Eingabe (Test `resolver_is_fast`).

mod context;
mod fuzzy;
mod keywords;

pub use context::{
    Context, ContextError, WindowInfo, WorkspaceInfo, MAX_APP_ID_CHARS, MAX_CONTEXT_BYTES, MAX_NAME_CHARS, MAX_TITLE_CHARS, MAX_WINDOWS, MAX_WORKSPACES,
};

use super::catalog::{self, Class, MAX_WORKSPACE_INDEX};
use super::intent::{DiagProfile, Intent, SettingsPage, Widget, WorkspaceRef};
use super::sources::{App, Frecency, Project};
use fuzzy::{fold_string, Matcher, Needle, Q_EXACT};
use keywords::{Action, KEYWORDS, PREFIXES};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::iter::once;
use std::path::PathBuf;
use url::Url;

/// Höchstzahl ausgegebener Kandidaten.
pub const MAX_CANDIDATES: usize = 12;
/// So viele werden zunächst gebaut (und geprüft), damit Doppelte und verworfene Absichten nicht die Liste lichten.
const MATERIALIZE: usize = MAX_CANDIDATES * 3;
/// Längste verarbeitete Eingabe in Zeichen: so lang darf eine Frage (`ask`) sein.
pub const MAX_INPUT_CHARS: usize = catalog::MAX_QUESTION;

/// Höchster Bonus der Häufigkeit in Güte-Punkten. Kleiner als der kleinste Stufenabstand (Präfix ab 88 gegen
/// Wortanfang bis 77: 11), damit Häufigkeit nie eine bessere Stufe überholt.
pub const FRECENCY_MAX: f32 = 10.0;
/// Ab dieser Häufigkeit ([`Frecency::score`]) ist der Bonus voll (13 Wahlen heute, ×4).
const FRECENCY_SATURATION: f64 = 50.0;
/// Größte mögliche Häufigkeit (`MAX_COUNT` 100 × Gewicht 4); Skala der leeren Eingabe.
const FRECENCY_TOP: f64 = 400.0;
// Die Stufen der Güte (Präfix ab 88, Wortanfang bis 77, Fuzzy bis 60) liegen weiter auseinander als der größte
// Häufigkeitsbonus; sonst könnte eine Gewohnheit einen klar besseren Texttreffer überholen.
const _: () = assert!(fuzzy::Q_PREFIX - (fuzzy::Q_WORD + fuzzy::Q_WORD_SPAN) > FRECENCY_MAX);
const _: () = assert!(fuzzy::Q_WORD - fuzzy::Q_FUZZY >= FRECENCY_MAX);
/// Aufschlag für laufende Fenster gegenüber gleich gut passenden Programmen.
pub const RUNNING_BONUS: f32 = 2.0;
/// Abschlag für das schon fokussierte Fenster.
pub const FOCUSED_PENALTY: f32 = 3.0;
/// Aufschlag für Treffer fester Regeln: sie stehen vor zufälligen exakten Treffern.
const DIRECT_BONUS: f32 = 3.0;
/// Mindestgüte des ersten Kandidaten für `confident` (siehe Modulkopf).
pub const CONFIDENT_MIN_QUALITY: f32 = 88.0;
/// Mindestabstand zur besten Güte der übrigen Ziele für `confident`.
pub const CONFIDENT_GAP: f32 = 12.0;
/// Mindestzahl Zeichen der Eingabe für `confident`, wenn keine feste Regel getroffen hat.
pub const CONFIDENT_MIN_CHARS: usize = 2;
/// Mindestgüte, damit ein Name als Parameter eines Präfixes („workspace mail“) zählt.
const MIN_ARG_QUALITY: f32 = 60.0;
/// Grundwert der leeren Eingabe: häufig gewählte Ziele (50 bis 100) vor laufenden Fenstern (40 bis 50).
const EMPTY_FRECENT_BASE: f32 = 50.0;
const EMPTY_WINDOW_BASE: f32 = 40.0;
/// So viele Wörter am Anfang der Eingabe werden auf Befehlspräfixe geprüft (längstes Präfix: vier Wörter).
const PREFIX_TOKENS: usize = 5;

// Gewichte der Felder eines Eintrags: Treffer in Nebenfeldern zählen weniger als im Namen.
const W_GENERIC: f32 = 0.75;
const W_KEYWORD: f32 = 0.75;
const W_ID: f32 = 0.6;
const W_GROUP: f32 = 0.5;
const W_TITLE: f32 = 0.6;

/// Was eine Absicht trifft. Das Panel (bis W7 Lua) führt mit genau diesem Ziel aus und sucht nie neu nach einem Namen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// Die Absicht braucht kein Ziel (System, Text, Suche …).
    None,
    /// Fenster nach niri-ID: bei `focus_window` das zu fokussierende, bei `close_window`, `toggle_window` und
    /// `column_display` das betroffene (ohne `query` das fokussierte).
    Window { id: u64 },
    /// Arbeitsfläche nach niri-ID (`view_workspace`).
    Workspace { id: u64 },
    /// Fenster `id` auf die Arbeitsfläche `workspace` verschieben (`move_window_to_workspace`), beide nach niri-ID.
    MoveWindow { id: u64, workspace: u64 },
    /// Programm aus einem Desktop-Eintrag: Desktop-File-ID (`firefox.desktop`) und alles, was das Panel zum Starten braucht.
    /// `argv` ist die vom Daemon nach der Desktop Entry Specification zerlegte `Exec`-Zeile (Feldcodes entfernt,
    /// `argv[0]` das Programm, nie leer): das Panel startet sie ohne Shell. `working_dir` ist `Path`, `terminal` ist
    /// `Terminal=true` (das Panel stellt dann das Terminal davor). Nur der Kandidat trägt das; die Häufigkeit kennt nur die ID.
    App {
        id: String,
        argv: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        working_dir: Option<PathBuf>,
        terminal: bool,
    },
    /// Absoluter Pfad eines Projekts.
    Project { path: PathBuf },
}

/// Ein Ergebnis des Resolvers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    /// Fertig validierte Absicht (Suchbegriffe sind der Name des Ziels; ausgeführt wird [`Candidate::target`]).
    pub intent: Intent,
    pub target: Target,
    /// Anzeigezeile, englisch (`Firefox — switch to window`).
    pub display: String,
    /// Zweite, schwächere Zeile (Fenstertitel, Pfad, Name der Arbeitsfläche).
    pub detail: Option<String>,
    /// Sortierwert, höher = besser (Güte + Aufschläge + Häufigkeitsbonus).
    pub score: f32,
    pub class: Class,
    /// Ziel-ID für die Häufigkeit (`Frecency::record(intent.name(), target_id, …)`): aus den Quellen, nie Eingabetext.
    pub target_id: String,
}

/// Ergebnis einer Anfrage.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Resolution {
    /// Nach `score` absteigend, höchstens [`MAX_CANDIDATES`].
    pub candidates: Vec<Candidate>,
    /// Ist der erste Kandidat sicher genug, um ihn mit Return ohne Modell auszuführen? (Regel im Modulkopf.)
    pub confident: bool,
}

/// Schalter, die bestimmen, welche Absichten der Resolver überhaupt anbietet. Die Vorgabe bietet alles an (Tests,
/// Werkzeuge); der Dienst stellt sie nach der Config ([`super::service::Palette::set_diagnose_enabled`]) und je Anfrage
/// nach den gefundenen Programmen ein ([`super::widgets::available`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// `[diagnose] enabled`: Aus = die Absicht `diagnose` wird nicht angeboten (weder ihre Schlüsselwörter noch
    /// „diagnose unit …“); eine Absicht des Modells dazu endet mit [`NoTarget::DiagnosisOff`].
    pub diagnose: bool,
    /// Verlauf der Zwischenablage gefunden (`palette.clipboard_command` oder Noctalia): sonst weder Schlüsselwort noch
    /// Eintrag bei leerer Eingabe; eine Absicht des Modells dazu endet mit [`NoTarget::WidgetMissing`].
    pub clipboard_history: bool,
    /// Emoji-Wähler gefunden (`palette.emoji_command` oder rofi mit rofi-emoji); sonst wie bei `clipboard_history`.
    pub emoji: bool,
}

impl Default for Features {
    fn default() -> Features {
        Features { diagnose: true, clipboard_history: true, emoji: true }
    }
}

impl Features {
    /// Wird die Anzeige angeboten?
    pub fn widget(&self, widget: Widget) -> bool {
        match widget {
            Widget::ClipboardHistory => self.clipboard_history,
            Widget::Emoji => self.emoji,
        }
    }

    /// Wird die Absicht angeboten? (Die Diagnose nur bei eingeschaltetem Schalter, eine Anzeige nur, wenn gefunden.)
    pub fn offers(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Diagnose { .. } => self.diagnose,
            Intent::ShowWidget { widget } => self.widget(*widget),
            _ => true,
        }
    }
}

/// Löst die Eingabe lokal auf. `now` sind Unix-Sekunden für die Häufigkeit (`sources::unix_now()`).
pub fn resolve(input: &str, ctx: &Context, apps: &[App], projects: &[Project], frecency: Option<&Frecency>, now: i64) -> Resolution {
    resolve_with(Features::default(), input, ctx, apps, projects, frecency, now)
}

/// Wie [`resolve`] mit eingestellten [`Features`].
pub fn resolve_with(features: Features, input: &str, ctx: &Context, apps: &[App], projects: &[Project], frecency: Option<&Frecency>, now: i64) -> Resolution {
    let text = normalize(input);
    let resolver = Resolver::new(ctx, apps, projects, frecency, now).features(features);
    if text.is_empty() {
        resolver.empty_input()
    } else {
        resolver.text_input(&text)
    }
}

/// Warum eine Absicht (vom Modell) kein Ziel hat. Die Meldung nennt nie den Suchbegriff: er stammt aus der Eingabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoTarget {
    Window,
    App,
    Project,
    Workspace,
    /// Die Adresse des Modells (`open_url`) nennt einen Host, den die Eingabe nicht enthält: erfunden, nicht angenommen.
    Url,
    /// Die Absicht wirkt auf das fokussierte Fenster, der Kontext nennt keins.
    FocusedWindow,
    /// Die Diagnose ist ausgeschaltet (`[diagnose] enabled = false`).
    DiagnosisOff,
    /// Für diese Anzeige wurde kein Programm gefunden ([`super::widgets::available`]).
    WidgetMissing(Widget),
}

impl std::fmt::Display for NoTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NoTarget::Window => "No match for a window",
            NoTarget::App => "No match for an application",
            NoTarget::Project => "No match for a project",
            NoTarget::Workspace => "No match for a workspace",
            NoTarget::Url => "No match for a web address",
            NoTarget::FocusedWindow => "No focused window",
            NoTarget::DiagnosisOff => crate::diagnose::service::DISABLED,
            NoTarget::WidgetMissing(Widget::ClipboardHistory) => "No clipboard history found: install Noctalia or check the command in Settings › Palette",
            NoTarget::WidgetMissing(Widget::Emoji) => "No emoji picker found: install rofi with rofi-emoji or check the command in Settings › Palette",
        })
    }
}

impl std::error::Error for NoTarget {}

/// Löst eine schon validierte Absicht (vom Modell, KI-Pfad in `palette::ai`) lokal auf: ihre Suchbegriffe werden nur
/// gegen echte Ziele gesucht (Fenster, Arbeitsflächen, Programme, Projekte; Güte mindestens wie ein Präfix-Argument), nie
/// erfunden. Kein Treffer = [`NoTarget`], nie ein Ersatz aus einer anderen Quelle. Absichten ohne Ziel (System, Text,
/// Suche) ergeben genau einen Kandidaten. Eine Adresse (`open_url`) nur, wenn ihr Host in `input`, der Eingabe des
/// Users, vorkommt (siehe [`mentions_host`]); sonst [`NoTarget::Url`]. Kandidaten wie bei [`resolve`] (Anzeige, Klasse,
/// `target_id`), höchstens [`MAX_CANDIDATES`], nach Güte und Häufigkeit sortiert.
pub fn resolve_intent(
    intent: &Intent,
    input: &str,
    ctx: &Context,
    apps: &[App],
    projects: &[Project],
    frecency: Option<&Frecency>,
    now: i64,
) -> Result<Vec<Candidate>, NoTarget> {
    resolve_intent_with(Features::default(), intent, input, ctx, apps, projects, frecency, now)
}

/// Wie [`resolve_intent`] mit eingestellten [`Features`]: ist die Diagnose aus, ergibt `diagnose` [`NoTarget::DiagnosisOff`].
#[allow(clippy::too_many_arguments)]
pub fn resolve_intent_with(
    features: Features,
    intent: &Intent,
    input: &str,
    ctx: &Context,
    apps: &[App],
    projects: &[Project],
    frecency: Option<&Frecency>,
    now: i64,
) -> Result<Vec<Candidate>, NoTarget> {
    Resolver::new(ctx, apps, projects, frecency, now).features(features).intent_input(intent, input)
}

/// Kommt `host` (wie [`Url::host_str`] ihn liefert: klein, internationale Namen in Punycode) in `input` vor? Der Host
/// muss ein ganzes Wort der Eingabe sein (Wörter: Folgen aus Buchstaben, Ziffern, `-` und `.`, ein Punkt am Rand zählt
/// nicht dazu), Groß- und Kleinschreibung gilt nicht, ein führendes `www.` auf beiden Seiten nicht. `evil.com` steht
/// also nicht in „notevil.com“ oder „evil.com.example.org“, `example.com` nicht in „mail.example.com“. Der Host einer
/// vom Modell gelieferten Adresse darf nicht erfunden sein; Pfad und Parameter prüft das nicht. Nicht erkannt werden
/// IPv6-Adressen (`[::1]`, die Eingabe enthält sie nicht als Wort) und Kurzformen von IPv4 („127.1“): das Modell darf
/// sie nicht liefern, getippt werden sie lokal erkannt.
fn mentions_host(input: &str, host: &str) -> bool {
    let strip = |h: &str| h.strip_prefix("www.").unwrap_or(h).trim_end_matches('.').to_string();
    let want = strip(host);
    if want.is_empty() {
        return false;
    }
    input.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '.')).map(|w| w.trim_matches('.')).filter(|w| !w.is_empty()).any(|w| {
        // ASCII: nur Kleinschreibung; sonst (Umlaute) wie der Browser nach Punycode (IDNA), damit „münchen.de“ passt
        let key =
            if w.is_ascii() { Some(w.to_ascii_lowercase()) } else { Url::parse(&format!("https://{w}")).ok().and_then(|u| u.host_str().map(str::to_string)) };
        key.is_some_and(|k| strip(&k) == want)
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Hilfen

/// Zeichen, die in einem Parameter nichts verloren haben (wie in `catalog`): Steuerzeichen und die
/// Richtungssteuerzeichen von Unicode.
fn is_forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Text für Anzeige und Parameter: Leerraum und Steuerzeichen werden zu *einem* Leerzeichen, getrimmt, höchstens
/// `max` Zeichen.
fn clean(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max * 2));
    let mut count = 0;
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() || is_forbidden(c) {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max {
            break;
        }
        out.push(c);
        count += 1;
    }
    out.truncate(out.trim_end().len());
    out
}

/// Kürzt für Anzeigezeilen mit Auslassungszeichen.
fn short(text: &str, max: usize) -> String {
    let cleaned = clean(text, max + 1);
    if cleaned.chars().count() > max {
        let cut: String = cleaned.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        cleaned
    }
}

/// Eingabe: getrimmt, Steuerzeichen als Leerzeichen, höchstens [`MAX_INPUT_CHARS`] Zeichen.
fn normalize(input: &str) -> String {
    let cut: String = input.trim().chars().take(MAX_INPUT_CHARS).map(|c| if is_forbidden(c) { ' ' } else { c }).collect();
    cut.trim().to_string()
}

fn only_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Faltet `rest` zur Suche und ruft `f` damit auf; bei leerer Eingabe oder einem Satz (siehe [`Needle::new`]) nichts.
fn with_needle(rest: &str, f: impl FnOnce(&mut Matcher)) {
    if let Some(needle) = Needle::new(rest) {
        f(&mut Matcher::new(&needle));
    }
}

/// Nummer einer Arbeitsfläche (1 bis [`MAX_WORKSPACE_INDEX`]) aus reinen Ziffern.
fn parse_workspace_number(text: &str) -> Option<u8> {
    only_digits(text).then(|| text.parse::<u8>().ok()).flatten().filter(|n| (1..=MAX_WORKSPACE_INDEX).contains(n))
}

/// Anzeige einer Arbeitsfläche: ihr Name, sonst die Nummer.
fn workspace_label(w: &WorkspaceInfo) -> String {
    w.name().map(|n| clean(n, 60)).filter(|n| !n.is_empty()).unwrap_or_else(|| w.idx.to_string())
}

/// Ziel-ID einer Arbeitsfläche für die Häufigkeit: der Name (bleibt beim Umordnen), sonst die Nummer (wie früher die
/// Tags; abgebildete alte Einträge `view_tag:<n>` passen so weiter).
fn workspace_target_id(w: &WorkspaceInfo) -> String {
    w.name().map(|n| clean(n, catalog::MAX_NAME)).filter(|n| !n.is_empty()).unwrap_or_else(|| w.idx.to_string())
}

/// Die Arbeitsfläche als Parameter einer Absicht: benannt mit Namen, sonst mit Nummer.
fn workspace_ref(w: &WorkspaceInfo) -> WorkspaceRef {
    match w.name().map(|n| clean(n, catalog::MAX_NAME)).filter(|n| !n.is_empty()) {
        Some(name) => WorkspaceRef::Name(name),
        None => WorkspaceRef::Index(w.idx),
    }
}

/// Prozentzahl 0 bis 100, optional mit `%`.
fn parse_percent(text: &str) -> Option<u8> {
    let t = text.trim().trim_end_matches('%').trim_end();
    only_digits(t).then(|| t.parse::<u8>().ok()).flatten().filter(|n| *n <= 100)
}

/// Endungen, bei denen „name.tld“ als Adresse gilt. Bewusst ohne Endungen, die auch Dateiendungen sind (`.rs`, `.md`,
/// `.sh`, `.py`, `.pl`, `.cc`): „main.rs“ ist eine Datei, `https://x.rs` geht trotzdem.
const TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "io", "dev", "app", "ai", "co", "de", "at", "ch", "eu", "uk", "us", "fr", "nl", "it", "es", "se", "no", "dk", "fi",
    "cz", "be", "jp", "cn", "ru", "ca", "au", "me", "tv", "info", "biz", "xyz", "online", "site", "tech", "cloud", "page", "wiki", "news", "blog", "shop",
    "store", "gg", "fm", "li", "lu", "pt", "ie", "gr", "hu", "ro", "bg", "sk", "si", "hr", "ua", "br", "mx", "kr", "nz", "za",
];

/// Ist `host` (ohne Port) ein Rechnername mit bekannter Endung (oder `www.` mit beliebiger alphabetischer Endung)?
fn looks_like_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let label_ok = |l: &&str| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-');
    if !labels.iter().all(label_ok) {
        return false;
    }
    let tld = labels[labels.len() - 1].to_lowercase();
    TLDS.contains(&tld.as_str())
        || (labels[0].eq_ignore_ascii_case("www") && labels.len() >= 3 && tld.chars().all(char::is_alphabetic) && tld.chars().count() >= 2)
}

/// Adresse in der Eingabe: `http://`/`https://` wie getippt, sonst `www.…` oder `domain.tld` (mit Pfad, Query, Port) mit
/// `https://` davor. Nie mit Leerraum, nie mit Zugangsdaten.
fn parse_url(text: &str) -> Option<String> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let url = if lower.starts_with("http://") || lower.starts_with("https://") {
        text.to_string()
    } else {
        let host_end = text.find(['/', '?', '#']).unwrap_or(text.len());
        let (authority, _) = text.split_at(host_end);
        let host = match authority.rsplit_once(':') {
            Some((h, port)) if only_digits(port) => h,
            Some(_) => return None,
            None => authority,
        };
        if !looks_like_host(host) {
            return None;
        }
        format!("https://{text}")
    };
    let parsed = Url::parse(&url).ok()?;
    (parsed.host_str().is_some_and(|h| !h.is_empty()) && parsed.username().is_empty() && parsed.password().is_none()).then_some(url)
}

/// Gewichtet die Häufigkeit zu einem Bonus von 0 bis [`FRECENCY_MAX`] (logarithmisch, gesättigt).
fn frecency_bonus(f: f64) -> f32 {
    if f <= 0.0 {
        return 0.0;
    }
    FRECENCY_MAX * ((1.0 + f).ln() / (1.0 + FRECENCY_SATURATION).ln()).min(1.0) as f32
}

/// Sortierwert eines häufig gewählten Ziels bei leerer Eingabe: 50 bis 100.
fn empty_score(f: f64) -> f32 {
    EMPTY_FRECENT_BASE + (100.0 - EMPTY_FRECENT_BASE) * ((1.0 + f).ln() / (1.0 + FRECENCY_TOP).ln()).min(1.0) as f32
}

// ---------------------------------------------------------------------------------------------------------------
// Treffer

/// Wofür ein Fenster oder Programm in der Rangfolge steht (für die Sicherheit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Window,
    Launch,
    Other,
}

/// Ein gebauter Kandidat mit den Angaben, die nur die Rangfolge braucht.
struct Hit {
    cand: Candidate,
    /// Güte ohne Aufschläge und Häufigkeit.
    quality: f32,
    /// Von einer festen Regel geliefert (Zahl, `g …`, Adresse, `volume 40` …).
    direct: bool,
    role: Role,
    /// Index des zugehörigen Programms (Fenster und Neustart), für die Sicherheit.
    app: Option<usize>,
}

impl Hit {
    fn new(intent: Intent, target: Target, display: String, detail: Option<String>, target_id: String) -> Option<Hit> {
        if target_id.is_empty() || intent.validate().is_err() {
            return None;
        }
        let class = intent.class();
        Some(Hit {
            cand: Candidate { intent, target, display, detail, score: 0.0, class, target_id },
            quality: 0.0,
            direct: false,
            role: Role::Other,
            app: None,
        })
    }

    fn scored(mut self, quality: f32, score: f32) -> Hit {
        self.quality = quality;
        self.cand.score = score;
        self
    }

    fn role(mut self, role: Role, app: Option<usize>) -> Hit {
        self.role = role;
        self.app = app;
        self
    }

    /// Treffer einer festen Regel: Güte 100 plus Aufschlag (und ein etwaiger Häufigkeitsbonus).
    fn direct(mut self, frecency_bonus: f32) -> Hit {
        self.direct = true;
        self.quality = Q_EXACT;
        self.cand.score = Q_EXACT + DIRECT_BONUS + frecency_bonus;
        self
    }

    fn key(&self) -> String {
        format!("{:?}|{:?}", self.cand.intent, self.cand.target)
    }
}

/// Woher ein Kandidat stammt; gebaut wird er erst, wenn er unter die besten kommt.
enum Source {
    Ready(Box<Hit>),
    Window {
        window: usize,
        close: bool,
    },
    /// Index in `Context::workspaces`.
    Workspace(usize),
    Keyword(usize),
    Page(usize),
    Project {
        index: usize,
        folder: bool,
    },
    App(usize),
}

impl Source {
    /// Reihenfolge bei gleichem Sortierwert: Fenster, Arbeitsflächen, feste Absichten, Projekte, Programme.
    fn order(&self) -> u8 {
        match self {
            Source::Ready(_) => 0,
            Source::Window { .. } => 1,
            Source::Workspace(_) => 2,
            Source::Keyword(_) | Source::Page(_) => 3,
            Source::Project { .. } => 4,
            Source::App(_) => 5,
        }
    }
}

struct Pending {
    src: Source,
    quality: f32,
    score: f32,
}

// ---------------------------------------------------------------------------------------------------------------
// Resolver

struct Resolver<'a> {
    ctx: &'a Context,
    apps: &'a [App],
    projects: &'a [Project],
    frecency: Option<&'a Frecency>,
    now: i64,
    /// `app_id` (klein) → Index des Programms.
    links: HashMap<String, usize>,
    focus: Option<u64>,
    /// Monitor, auf den sich eine Zahl ohne bestimmtes Fenster bezieht ([`Context::focused_output`]).
    focused_output: Option<&'a str>,
    /// Mehr als ein Monitor im Kontext (dann zeigt die zweite Zeile einer Arbeitsfläche ihren Monitor).
    multi_output: bool,
    features: Features,
}

impl<'a> Resolver<'a> {
    fn new(ctx: &'a Context, apps: &'a [App], projects: &'a [Project], frecency: Option<&'a Frecency>, now: i64) -> Resolver<'a> {
        let links = if ctx.windows.is_empty() { HashMap::new() } else { app_links(apps) };
        let first_output = ctx.workspaces.first().map(WorkspaceInfo::output);
        Resolver {
            ctx,
            apps,
            projects,
            frecency,
            now,
            links,
            focus: ctx.focused_window().map(|c| c.id),
            focused_output: ctx.focused_output(),
            multi_output: ctx.workspaces.iter().any(|w| Some(w.output()) != first_output),
            features: Features::default(),
        }
    }

    fn features(mut self, features: Features) -> Resolver<'a> {
        self.features = features;
        self
    }

    /// Wird dieses Schlüsselwort angeboten? ([`Features::offers`])
    fn offers(&self, k: &keywords::Keyword) -> bool {
        self.features.offers(&(k.intent)())
    }

    fn frec(&self, intent: &str, target: &str) -> f64 {
        self.frecency.map_or(0.0, |f| f.score(intent, target, self.now))
    }

    fn windows(&self) -> impl Iterator<Item = (usize, &'a WindowInfo)> {
        self.ctx.windows.iter().enumerate().take(MAX_WINDOWS)
    }

    /// Programm zu einem Fenster: die `app_id` entspricht einem Programm.
    fn link(&self, c: &WindowInfo) -> Option<usize> {
        if self.links.is_empty() || c.app_id().is_empty() {
            return None;
        }
        self.links.get(&c.app_id().to_lowercase()).copied()
    }

    /// Anzeigename eines Fensters: das Programm, sonst `app_id`, Titel.
    fn window_label(&self, c: &WindowInfo, app: Option<usize>) -> String {
        if let Some(a) = app {
            let name = clean(&self.apps[a].name, 80);
            if !name.is_empty() {
                return name;
            }
        }
        [c.app_id(), c.title()].into_iter().map(|s| clean(s, 60)).find(|s| !s.is_empty()).unwrap_or_default()
    }

    /// Ziel-ID eines Fensters für die Häufigkeit: die `app_id` (aus dem Kontext, nicht aus der Eingabe), nie der Titel.
    fn window_target_id(c: &WindowInfo) -> String {
        Some(clean(&c.app_id().to_lowercase(), 128)).filter(|s| !s.is_empty()).unwrap_or_else(|| "window".to_string())
    }

    fn window_detail(&self, c: &WindowInfo) -> Option<String> {
        let title = short(c.title(), 100);
        let ws = self.ctx.workspace_of(c).map(|w| format!("Workspace {}", workspace_label(w)));
        match (title.is_empty(), ws) {
            (false, Some(t)) => Some(format!("{title} · {t}")),
            (false, None) => Some(title),
            (true, Some(t)) => Some(t),
            (true, None) => None,
        }
    }

    // -- Kandidaten bauen -------------------------------------------------------------------------------------

    fn window_hit(&self, window: usize, close: bool, quality: f32, score: f32) -> Option<Hit> {
        let c = self.ctx.windows.get(window)?;
        let app = self.link(c);
        let label = self.window_label(c, app);
        if label.is_empty() {
            return None;
        }
        let (intent, verb) = if close {
            (Intent::CloseWindow { query: Some(label.clone()) }, "Close window")
        } else {
            (Intent::FocusWindow { query: label.clone() }, "Switch to window")
        };
        let hit = Hit::new(intent, Target::Window { id: c.id }, format!("{label} · {verb}"), self.window_detail(c), Self::window_target_id(c))?;
        Some(hit.scored(quality, score).role(Role::Window, app))
    }

    fn app_hit(&self, index: usize, quality: f32, score: f32) -> Option<Hit> {
        let app = self.apps.get(index)?;
        let name = clean(&app.name, 100);
        let generic = clean(&app.generic_name, 100);
        let hit = Hit::new(
            Intent::LaunchApp { query: name.clone() },
            Target::App { id: app.id.clone(), argv: app.exec_argv.clone(), working_dir: app.working_dir.clone(), terminal: app.terminal },
            format!("{name} · Launch"),
            (!generic.is_empty()).then_some(generic),
            app.id.clone(),
        )?;
        Some(hit.scored(quality, score).role(Role::Launch, Some(index)))
    }

    fn project_hit(&self, index: usize, folder: bool, quality: f32, score: f32) -> Option<Hit> {
        let p = self.projects.get(index)?;
        let name = clean(&p.name, 100);
        let path = p.target_id();
        if path.is_empty() {
            return None;
        }
        let (intent, display) = if folder {
            (Intent::OpenFolder { query: name.clone() }, format!("Open folder {name}"))
        } else {
            (Intent::OpenProject { query: name.clone() }, format!("Open project {name} in a terminal"))
        };
        Hit::new(intent, Target::Project { path: p.path.clone() }, display, Some(short(path, 120)), path.to_string()).map(|h| h.scored(quality, score))
    }

    /// Zweite Zeile einer Arbeitsfläche: Name (falls benannt) und, bei mehreren Monitoren, der Monitor.
    fn workspace_detail(&self, w: &WorkspaceInfo) -> Option<String> {
        let parts: Vec<String> =
            [w.name().map(|n| clean(n, 60)), self.multi_output.then(|| clean(w.output(), 40))].into_iter().flatten().filter(|s| !s.is_empty()).collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    fn workspace_hit(&self, index: usize, quality: f32, score: f32) -> Option<Hit> {
        let w = self.ctx.workspaces.get(index)?;
        Hit::new(
            Intent::ViewWorkspace { workspace: workspace_ref(w) },
            Target::Workspace { id: w.id },
            format!("Workspace {}", w.idx),
            self.workspace_detail(w),
            workspace_target_id(w),
        )
        .map(|h| h.scored(quality, score))
    }

    fn keyword_hit(&self, index: usize, quality: f32, score: f32) -> Option<Hit> {
        let k = KEYWORDS.get(index)?;
        let intent = (k.intent)();
        // Absichten auf dem fokussierten Fenster: ohne ein solches gibt es nichts zu tun
        let (target, detail) = if on_focused_window(&intent) {
            let c = self.ctx.focused_window()?;
            (Target::Window { id: c.id }, Some(self.window_label(c, self.link(c))).filter(|s| !s.is_empty()))
        } else {
            (Target::None, None)
        };
        Hit::new(intent, target, k.display.to_string(), detail, k.id.to_string()).map(|h| h.scored(quality, score))
    }

    fn page_hit(&self, index: usize, quality: f32, score: f32) -> Option<Hit> {
        let page = *SettingsPage::ALL.get(index)?;
        let name = page.name();
        let mut label = name.to_string();
        label[..1].make_ascii_uppercase();
        Hit::new(Intent::DictateSettings { page: Some(page) }, Target::None, format!("Dictate settings › {label}"), None, name.to_string())
            .map(|h| h.scored(quality, score))
    }

    fn materialize(&self, p: Pending) -> Option<Hit> {
        let (q, s) = (p.quality, p.score);
        match p.src {
            Source::Ready(hit) => Some(*hit),
            Source::Window { window, close } => self.window_hit(window, close, q, s),
            Source::Workspace(i) => self.workspace_hit(i, q, s),
            Source::Keyword(i) => self.keyword_hit(i, q, s),
            Source::Page(i) => self.page_hit(i, q, s),
            Source::Project { index, folder } => self.project_hit(index, folder, q, s),
            Source::App(i) => self.app_hit(i, q, s),
        }
    }

    // -- Feste Regeln -----------------------------------------------------------------------------------------

    fn ask_hit(question: &str) -> Option<Hit> {
        Hit::new(Intent::Ask { question: question.to_string() }, Target::None, format!("Ask AI: {}", short(question, 70)), None, "ask".into())
            .map(|h| h.direct(0.0))
    }

    fn search_hit(query: &str) -> Option<Hit> {
        Hit::new(
            Intent::WebSearch { query: query.to_string() },
            Target::None,
            format!("Search the web for \"{}\"", short(query, 60)),
            None,
            "web_search".into(),
        )
        .map(|h| h.direct(0.0))
    }

    fn url_hit(url: &str) -> Option<Hit> {
        let shown = Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default();
        Hit::new(
            Intent::OpenUrl { url: url.to_string() },
            Target::None,
            format!("Open {} in browser", short(&shown, 70)),
            Some(short(url, 120)),
            "open_url".into(),
        )
        .map(|h| h.direct(0.0))
    }

    /// `open_url` vom Modell: nur, wenn der Host der Adresse in der Eingabe vorkommt ([`mentions_host`]), sonst keiner.
    fn url_from_model(url: &str, input: &str) -> Option<Hit> {
        let parsed = Url::parse(url).ok()?;
        mentions_host(input, parsed.host_str()?).then(|| Self::url_hit(url)).flatten()
    }

    fn volume_set_hit(percent: u8) -> Option<Hit> {
        Hit::new(Intent::Volume { set: Some(percent), step: None, mute: None }, Target::None, format!("Set volume to {percent}%"), None, "set".into())
            .map(|h| h.direct(0.0))
    }

    /// `move_window_to_workspace` für das Fenster `c` auf die Arbeitsfläche mit Index `ws` (in `Context::workspaces`).
    fn move_hit_for(&self, c: &WindowInfo, ws: usize, quality: f32, direct: bool) -> Option<Hit> {
        let w = self.ctx.workspaces.get(ws)?;
        let label = self.window_label(c, self.link(c));
        let target_id = workspace_target_id(w);
        let hit = Hit::new(
            Intent::MoveWindowToWorkspace { workspace: workspace_ref(w), query: None },
            Target::MoveWindow { id: c.id, workspace: w.id },
            format!("Move window to workspace {}", workspace_label(w)),
            Some(label).filter(|s| !s.is_empty()),
            target_id.clone(),
        )?;
        let bonus = self.frec_bonus("move_window_to_workspace", &target_id);
        Some(if direct { hit.direct(bonus) } else { hit.scored(quality, quality + bonus) })
    }

    /// Verschieben des fokussierten Fensters; die Zahl gilt auf dessen Monitor. Ohne Fokus gibt es nichts zu verschieben.
    fn move_focused(&self, rest: &str) -> Vec<Hit> {
        let Some(c) = self.ctx.focused_window() else { return Vec::new() };
        self.workspace_args(rest, self.ctx.output_for(c)).into_iter().filter_map(|(ws, q, direct)| self.move_hit_for(c, ws, q, direct)).collect()
    }

    fn view_workspace_pending(&self, ws: usize, quality: f32, direct: bool) -> Option<Pending> {
        let bonus = self.frec_bonus("view_workspace", &workspace_target_id(self.ctx.workspaces.get(ws)?));
        let mut hit = self.workspace_hit(ws, quality, quality + bonus)?;
        if direct {
            hit = hit.direct(bonus);
        }
        Some(Pending { quality: hit.quality, score: hit.cand.score, src: Source::Ready(Box::new(hit)) })
    }

    fn frec_bonus(&self, intent: &str, target: &str) -> f32 {
        frecency_bonus(self.frec(intent, target))
    }

    /// Arbeitsflächen zu einem Parameter (Index in `Context::workspaces`, Güte, direkt): eine Zahl meint `idx` auf
    /// `output` (direkt, sicher; ohne bestimmbaren Monitor oder ohne diese Arbeitsfläche kein Treffer), ein Name eine
    /// benannte Arbeitsfläche auf jedem Monitor (Fuzzy).
    fn workspace_args(&self, rest: &str, output: Option<&str>) -> Vec<(usize, f32, bool)> {
        if let Some(n) = parse_workspace_number(rest) {
            return output.and_then(|o| self.ctx.workspace_at(o, n)).map(|i| vec![(i, Q_EXACT, true)]).unwrap_or_default();
        }
        let Some(needle) = Needle::new(rest) else { return Vec::new() };
        let mut m = Matcher::new(&needle);
        let mut found: Vec<(usize, f32, bool)> =
            self.ctx.workspaces.iter().enumerate().filter_map(|(i, w)| m.score(w.name()?).filter(|q| *q >= MIN_ARG_QUALITY).map(|q| (i, q, false))).collect();
        found.sort_by(|a, b| b.1.total_cmp(&a.1));
        found
    }

    /// Wendet ein Befehlspräfix auf den Rest der Eingabe an.
    fn apply_prefix(&self, action: Action, rest: &str, out: &mut Vec<Pending>, win_floor: &mut HashMap<usize, f32>) {
        let ready = |hit: Option<Hit>, out: &mut Vec<Pending>| {
            if let Some(h) = hit {
                let (quality, score) = (h.quality, h.cand.score);
                out.push(Pending { src: Source::Ready(Box::new(h)), quality, score });
            }
        };
        match action {
            Action::WebSearch => ready(Self::search_hit(rest), out),
            Action::VolumeSet => ready(parse_percent(rest).and_then(Self::volume_set_hit), out),
            Action::MoveToWorkspace => {
                for hit in self.move_focused(rest) {
                    ready(Some(hit), out);
                }
            }
            Action::ViewWorkspace => out.extend(self.view_workspaces(rest)),
            Action::Project => with_needle(rest, |m| self.scan_projects(m, false, out)),
            Action::Folder => with_needle(rest, |m| self.scan_projects(m, true, out)),
            Action::App => with_needle(rest, |m| self.scan_apps(m, out, &HashMap::new())),
            Action::OpenAny => {
                if let Some(url) = parse_url(rest) {
                    ready(Self::url_hit(&url), out);
                }
                with_needle(rest, |m| {
                    self.scan_projects(m, false, out);
                    self.scan_apps(m, out, &HashMap::new());
                });
            }
            Action::Switch => {
                out.extend(self.view_workspaces(rest));
                with_needle(rest, |m| self.scan_windows(m, false, out, win_floor));
            }
            Action::Close => with_needle(rest, |m| self.scan_windows(m, true, out, win_floor)),
            Action::DiagnoseUnit => ready(self.diagnose_term_hit(rest, true), out),
            Action::DiagnoseContainer => ready(self.diagnose_term_hit(rest, false), out),
        }
    }

    /// Arbeitsflächen anzeigen; eine Zahl gilt auf dem fokussierten Monitor.
    fn view_workspaces(&self, rest: &str) -> Vec<Pending> {
        self.workspace_args(rest, self.focused_output).into_iter().filter_map(|(ws, q, direct)| self.view_workspace_pending(ws, q, direct)).collect()
    }

    /// „diagnose unit cups“ / „diagnose container dataroom“: die Diagnose mit einem Suchbegriff. Der Begriff wird hier
    /// nicht aufgelöst (die Palette kennt weder Units noch Container und führt keine Probe aus); das Panel löst ihn gegen
    /// die echten Listen auf. Ein Begriff, der kein Name sein kann (Leerzeichen, Sonderzeichen), ergibt keinen Kandidaten.
    fn diagnose_term_hit(&self, term: &str, unit: bool) -> Option<Hit> {
        if !self.features.diagnose {
            return None;
        }
        let (intent, label, id) = if unit {
            (Intent::Diagnose { profile: Some(DiagProfile::Unit), unit: Some(term.to_string()), container: None }, "unit", "unit")
        } else {
            (Intent::Diagnose { profile: Some(DiagProfile::Docker), unit: None, container: Some(term.to_string()) }, "container", "container")
        };
        Hit::new(intent, Target::None, format!("Diagnose {label} {}", short(term, 60)), None, id.to_string()).map(|h| h.direct(self.frec_bonus("diagnose", id)))
    }

    /// Befehlspräfixe am Anfang der Eingabe mit dem Rest dahinter (Rest nicht leer).
    fn prefixes<'t>(&self, text: &'t str) -> Vec<(Action, &'t str)> {
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut start = None;
        for (i, c) in text.char_indices().chain(once((text.len(), ' '))) {
            match (c.is_whitespace(), start) {
                (false, None) => start = Some(i),
                (true, Some(s)) => {
                    spans.push((s, i));
                    start = None;
                    if spans.len() == PREFIX_TOKENS {
                        break;
                    }
                }
                _ => {}
            }
        }
        let folded: Vec<[String; 2]> = spans.iter().map(|&(s, e)| [fold_string(&text[s..e], false), fold_string(&text[s..e], true)]).collect();
        let mut found = Vec::new();
        for prefix in PREFIXES {
            let n = prefix.words.len();
            if n >= spans.len() {
                continue;
            }
            let all = prefix.words.iter().enumerate().all(|(k, w)| fold_string(w, false) == folded[k][0] || fold_string(w, true) == folded[k][1]);
            if all {
                let rest = text[spans[n - 1].1..].trim();
                if !rest.is_empty() {
                    found.push((prefix.action, rest));
                }
            }
        }
        found
    }

    // -- Durchsuchen ------------------------------------------------------------------------------------------

    /// Fenster. `win_floor` sammelt je Programm die niedrigste Wertung seiner laufenden Fenster (der Neustart steht hinter
    /// allen).
    fn scan_windows(&self, m: &mut Matcher, close: bool, out: &mut Vec<Pending>, win_floor: &mut HashMap<usize, f32>) {
        let intent = if close { "close_window" } else { "focus_window" };
        for (i, c) in self.windows() {
            let app = self.link(c);
            let label = self.window_label(c, app);
            let fields = [(label.as_str(), 1.0), (c.app_id(), 1.0), (c.title(), W_TITLE)];
            let Some(q) = m.item(fields) else { continue };
            let mut f = self.frec(intent, &Self::window_target_id(c));
            let mut score = q;
            if !close {
                if let Some(a) = app {
                    f = f.max(self.frec("launch_app", &self.apps[a].id));
                }
                score += RUNNING_BONUS;
                if Some(c.id) == self.focus {
                    score -= FOCUSED_PENALTY;
                }
            }
            score += frecency_bonus(f);
            if let (false, Some(a)) = (close, app) {
                let floor = win_floor.entry(a).or_insert(score);
                *floor = floor.min(score);
            }
            out.push(Pending { src: Source::Window { window: i, close }, quality: q, score });
        }
    }

    fn scan_apps(&self, m: &mut Matcher, out: &mut Vec<Pending>, win_floor: &HashMap<usize, f32>) {
        for (i, app) in self.apps.iter().enumerate() {
            let stem = app.id.strip_suffix(".desktop").unwrap_or(&app.id);
            let fields = once((app.name.as_str(), 1.0))
                .chain((app.name_default != app.name).then_some((app.name_default.as_str(), 1.0)))
                .chain((!app.generic_name.is_empty()).then_some((app.generic_name.as_str(), W_GENERIC)))
                .chain(
                    (!app.generic_name_default.is_empty() && app.generic_name_default != app.generic_name)
                        .then_some((app.generic_name_default.as_str(), W_GENERIC)),
                )
                .chain(app.keywords.iter().map(|k| (k.as_str(), W_KEYWORD)))
                .chain(app.keywords_default.iter().filter(|k| !app.keywords.contains(k)).map(|k| (k.as_str(), W_KEYWORD)))
                .chain(once((stem, W_ID)));
            let Some(q) = m.item(fields) else { continue };
            let mut score = q + self.frec_bonus("launch_app", &app.id);
            if let Some(&floor) = win_floor.get(&i) {
                // laufende Fenster stehen vor dem Neustart desselben Programms
                score = score.min(floor - 0.5);
            }
            out.push(Pending { src: Source::App(i), quality: q, score });
        }
    }

    fn scan_projects(&self, m: &mut Matcher, folder: bool, out: &mut Vec<Pending>) {
        let intent = if folder { "open_folder" } else { "open_project" };
        for (i, p) in self.projects.iter().enumerate() {
            let fields = once((p.name.as_str(), 1.0)).chain(p.group.as_deref().map(|g| (g, W_GROUP)));
            let Some(q) = m.item(fields) else { continue };
            out.push(Pending { src: Source::Project { index: i, folder }, quality: q, score: q + self.frec_bonus(intent, p.target_id()) });
        }
    }

    /// Wird diese Arbeitsfläche ohne Präfix angeboten? Die auf dem fokussierten Monitor (über ihre Nummer) und jede
    /// benannte (über den Namen); eine unbenannte auf einem anderen Monitor ist nur über ihre Fenster erreichbar.
    fn numbered(&self, w: &WorkspaceInfo) -> bool {
        self.focused_output == Some(w.output())
    }

    fn scan_workspaces(&self, m: &mut Matcher, out: &mut Vec<Pending>) {
        for (i, w) in self.ctx.workspaces.iter().enumerate() {
            let numbered = self.numbered(w);
            let labels: Vec<String> =
                if numbered { ["workspace", "tag", "arbeitsfläche"].iter().map(|p| format!("{p} {}", w.idx)).collect() } else { Vec::new() };
            let fields = labels.iter().map(|l| (l.as_str(), 1.0)).chain(w.name().map(|n| (n, 1.0)));
            let Some(q) = m.item(fields) else { continue };
            out.push(Pending { src: Source::Workspace(i), quality: q, score: q + self.frec_bonus("view_workspace", &workspace_target_id(w)) });
        }
    }

    fn scan_keywords(&self, m: &mut Matcher, out: &mut Vec<Pending>) {
        for (i, k) in KEYWORDS.iter().enumerate().filter(|(_, k)| self.offers(k)) {
            let Some(q) = m.item(k.phrases.iter().map(|p| (*p, 1.0))) else { continue };
            let score = q + k.rank + self.frec_bonus((k.intent)().name(), k.id);
            out.push(Pending { src: Source::Keyword(i), quality: q, score });
        }
        for (i, page) in SettingsPage::ALL.iter().enumerate() {
            let (en, de) = (format!("settings {}", page.name()), format!("einstellungen {}", page.name()));
            let Some(q) = m.item([(en.as_str(), 1.0), (de.as_str(), 1.0)]) else { continue };
            out.push(Pending { src: Source::Page(i), quality: q, score: q + self.frec_bonus("dictate_settings", page.name()) });
        }
    }

    // -- Ablauf -----------------------------------------------------------------------------------------------

    /// Arbeitsfläche aus einer Absicht: Nummer auf `output` (nur, wenn es sie gibt) oder Name (wie ein Präfix-Argument,
    /// „workspace mail“; ein Name aus Ziffern zählt wie die Nummer).
    fn workspace_targets(&self, r: &WorkspaceRef, output: Option<&str>) -> Vec<(usize, f32, bool)> {
        match r {
            WorkspaceRef::Index(n) => output.and_then(|o| self.ctx.workspace_at(o, *n)).map(|i| vec![(i, Q_EXACT, true)]).unwrap_or_default(),
            WorkspaceRef::Name(name) => self.workspace_args(name, output),
        }
    }

    /// Kandidat für eine Absicht ohne eigenes Ziel: die Zeile des passenden Schlüsselworts (gleiche Absicht), sonst
    /// eine Zeile aus dem Katalog.
    fn fixed_hit(&self, intent: &Intent) -> Option<Hit> {
        if let Some(i) = KEYWORDS.iter().position(|k| (k.intent)() == *intent) {
            return self.keyword_hit(i, Q_EXACT, Q_EXACT);
        }
        let (display, target_id) = match intent {
            Intent::Volume { step: Some(n), .. } if *n > 0 => (format!("Volume up by {n} points"), "up".to_string()),
            Intent::Volume { step: Some(n), .. } => (format!("Volume down by {} points", n.unsigned_abs()), "down".to_string()),
            Intent::TextAction { action } => {
                let label = crate::actions::BUILTIN.iter().find(|b| b.name == action).map_or(action.as_str(), |b| b.label);
                (format!("{label} · Text action"), action.clone())
            }
            Intent::Diagnose { unit, container, .. } => {
                // Profil und Begriff zeigen, ohne den Begriff in die Ziel-ID zu nehmen (die kommt nie aus der Eingabe)
                let profile = intent.diagnose_profile();
                let label = match profile {
                    None => "Diagnose this computer".to_string(),
                    Some(DiagProfile::LastCrash) => "Diagnose · Last crash".to_string(),
                    Some(DiagProfile::Now) => "Diagnose · Right now".to_string(),
                    Some(DiagProfile::Unit) => "Diagnose · Failed unit".to_string(),
                    Some(DiagProfile::Docker) => "Diagnose · Docker".to_string(),
                    Some(DiagProfile::Network) => "Diagnose · Network".to_string(),
                    Some(DiagProfile::Heat) => "Diagnose · Heat".to_string(),
                };
                match (unit, container) {
                    (Some(u), _) => (format!("Diagnose unit {}", short(u, 60)), "unit".to_string()),
                    (_, Some(c)) => (format!("Diagnose container {}", short(c, 60)), "container".to_string()),
                    _ => (label, profile.map_or("diagnose", DiagProfile::name).to_string()),
                }
            }
            other => (other.spec().description.trim_end_matches('.').to_string(), other.name().to_string()),
        };
        Hit::new(intent.clone(), Target::None, display, None, target_id).map(|h| h.scored(Q_EXACT, Q_EXACT))
    }

    fn intent_input(&self, intent: &Intent, input: &str) -> Result<Vec<Candidate>, NoTarget> {
        let ready = |hit: Option<Hit>| -> Vec<Pending> {
            hit.map(|h| Pending { quality: h.quality, score: h.cand.score, src: Source::Ready(Box::new(h)) }).into_iter().collect()
        };
        let search = |query: &str, scan: &dyn Fn(&mut Matcher, &mut Vec<Pending>)| -> Vec<Pending> {
            let mut out = Vec::new();
            with_needle(query, |m| scan(m, &mut out));
            out.retain(|p| p.quality >= MIN_ARG_QUALITY);
            out
        };
        let windows = |query: &str, close: bool| search(query, &|m, out| self.scan_windows(m, close, out, &mut HashMap::new()));
        let (pending, missing) = match intent {
            Intent::ViewWorkspace { workspace } => (
                self.workspace_targets(workspace, self.focused_output)
                    .into_iter()
                    .filter_map(|(ws, q, direct)| self.view_workspace_pending(ws, q, direct))
                    .collect(),
                NoTarget::Workspace,
            ),
            Intent::FocusWindow { query } => (windows(query, false), NoTarget::Window),
            Intent::CloseWindow { query: Some(query) } => (windows(query, true), NoTarget::Window),
            Intent::MoveWindowToWorkspace { workspace, query } => {
                // die Zahl gilt je Fenster auf dessen Monitor: zwei gefundene Fenster auf zwei Monitoren wandern
                // auf verschiedene Arbeitsflächen
                let target_of = |c: &WindowInfo| self.workspace_targets(workspace, self.ctx.output_for(c)).first().map(|t| t.0);
                match query {
                    None => {
                        let Some(c) = self.ctx.focused_window() else { return Err(NoTarget::FocusedWindow) };
                        let Some(ws) = target_of(c) else { return Err(NoTarget::Workspace) };
                        (ready(self.move_hit_for(c, ws, Q_EXACT, true)), NoTarget::Workspace)
                    }
                    Some(query) => {
                        let windows = windows(query, false);
                        if windows.is_empty() {
                            return Err(NoTarget::Window);
                        }
                        let found: Vec<Pending> = windows
                            .into_iter()
                            .filter_map(|p| {
                                let Source::Window { window, .. } = p.src else { return None };
                                let c = self.ctx.windows.get(window)?;
                                let hit = self.move_hit_for(c, target_of(c)?, p.quality, false)?.scored(p.quality, p.score);
                                Some(Pending { quality: p.quality, score: p.score, src: Source::Ready(Box::new(hit)) })
                            })
                            .collect();
                        (found, NoTarget::Workspace)
                    }
                }
            }
            Intent::LaunchApp { query } => (search(query, &|m, out| self.scan_apps(m, out, &HashMap::new())), NoTarget::App),
            Intent::OpenProject { query } => (search(query, &|m, out| self.scan_projects(m, false, out)), NoTarget::Project),
            Intent::OpenFolder { query } => (search(query, &|m, out| self.scan_projects(m, true, out)), NoTarget::Project),
            Intent::OpenUrl { url } => (ready(Self::url_from_model(url, input)), NoTarget::Url),
            Intent::WebSearch { query } => (ready(Self::search_hit(query)), NoTarget::Window),
            Intent::Ask { question } => (ready(Self::ask_hit(question)), NoTarget::Window),
            Intent::Diagnose { .. } if !self.features.diagnose => return Err(NoTarget::DiagnosisOff),
            Intent::ShowWidget { widget } if !self.features.widget(*widget) => return Err(NoTarget::WidgetMissing(*widget)),
            Intent::Volume { set: Some(n), .. } => (ready(Self::volume_set_hit(*n)), NoTarget::Window),
            Intent::DictateSettings { page: Some(page) } => {
                let i = SettingsPage::ALL.iter().position(|p| p == page).unwrap_or(0);
                (ready(self.page_hit(i, Q_EXACT, Q_EXACT)), NoTarget::Window)
            }
            // fokussiertes Fenster (Umschalten, Schließen ohne Begriff) und alles ohne Ziel
            other => (ready(self.fixed_hit(other)), if on_focused_window(other) { NoTarget::FocusedWindow } else { NoTarget::Window }),
        };
        if pending.is_empty() {
            return Err(missing);
        }
        let candidates = self.finish(pending, 0).candidates;
        if candidates.is_empty() {
            return Err(missing);
        }
        Ok(candidates)
    }

    fn text_input(&self, text: &str) -> Resolution {
        let typed = text.chars().filter(|c| !c.is_whitespace()).count();
        let mut out: Vec<Pending> = Vec::new();

        // „?frage“ und „? frage“: ausdrücklich eine Frage, sonst nichts
        if let Some(question) = text.strip_prefix('?') {
            let question = question.trim();
            return match Self::ask_hit(question) {
                Some(hit) => self.finish(vec![Pending { quality: hit.quality, score: hit.cand.score, src: Source::Ready(Box::new(hit)) }], typed),
                None => Resolution::default(),
            };
        }
        let mut win_floor = HashMap::new();
        if let Some(url) = parse_url(text) {
            if let Some(h) = Self::url_hit(&url) {
                out.push(Pending { quality: h.quality, score: h.cand.score, src: Source::Ready(Box::new(h)) });
            }
        }
        if let Some(n) = parse_workspace_number(text) {
            if let Some(ws) = self.focused_output.and_then(|o| self.ctx.workspace_at(o, n)) {
                out.extend(self.view_workspace_pending(ws, Q_EXACT, true));
            }
        }
        for (action, rest) in self.prefixes(text) {
            self.apply_prefix(action, rest, &mut out, &mut win_floor);
        }
        if let Some(needle) = Needle::new(text) {
            let mut m = Matcher::new(&needle);
            self.scan_windows(&mut m, false, &mut out, &mut win_floor);
            self.scan_apps(&mut m, &mut out, &win_floor);
            self.scan_projects(&mut m, false, &mut out);
            self.scan_workspaces(&mut m, &mut out);
            self.scan_keywords(&mut m, &mut out);
        }
        self.finish(out, typed)
    }

    /// Leere Eingabe: häufig gewählte Ziele nach Häufigkeit, danach die laufenden Fenster.
    fn empty_input(&self) -> Resolution {
        let mut out: Vec<Pending> = Vec::new();
        let mut running: HashSet<usize> = HashSet::new();
        for (i, c) in self.windows() {
            let app = self.link(c);
            running.extend(app);
            let mut f = self.frec("focus_window", &Self::window_target_id(c));
            if let Some(a) = app {
                f = f.max(self.frec("launch_app", &self.apps[a].id));
            }
            let mut score = EMPTY_WINDOW_BASE + frecency_bonus(f);
            if Some(c.id) == self.focus {
                score -= FOCUSED_PENALTY;
            }
            out.push(Pending { src: Source::Window { window: i, close: false }, quality: 0.0, score });
        }
        if self.frecency.is_some_and(Frecency::enabled) {
            let mut push = |src: Source, f: f64| {
                if f > 0.0 {
                    out.push(Pending { src, quality: 0.0, score: empty_score(f) });
                }
            };
            for (i, app) in self.apps.iter().enumerate() {
                // läuft das Programm, steht sein Fenster schon in der Liste
                if !running.contains(&i) {
                    push(Source::App(i), self.frec("launch_app", &app.id));
                }
            }
            for (i, p) in self.projects.iter().enumerate() {
                push(Source::Project { index: i, folder: false }, self.frec("open_project", p.target_id()));
                push(Source::Project { index: i, folder: true }, self.frec("open_folder", p.target_id()));
            }
            for (i, w) in self.ctx.workspaces.iter().enumerate().filter(|(_, w)| self.numbered(w) || w.name().is_some()) {
                push(Source::Workspace(i), self.frec("view_workspace", &workspace_target_id(w)));
            }
            for (i, k) in KEYWORDS.iter().enumerate().filter(|(_, k)| self.offers(k)) {
                push(Source::Keyword(i), self.frec((k.intent)().name(), k.id));
            }
            for (i, page) in SettingsPage::ALL.iter().enumerate() {
                push(Source::Page(i), self.frec("dictate_settings", page.name()));
            }
        }
        self.finish(out, 0)
    }

    /// Sortiert, baut die besten, entfernt Doppelte, bestimmt `confident` und kürzt auf [`MAX_CANDIDATES`].
    fn finish(&self, mut pending: Vec<Pending>, typed: usize) -> Resolution {
        pending.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.src.order().cmp(&b.src.order())));
        let mut hits: Vec<Hit> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for p in pending {
            if hits.len() >= MATERIALIZE {
                break;
            }
            if let Some(hit) = self.materialize(p) {
                if seen.insert(hit.key()) {
                    hits.push(hit);
                }
            }
        }
        let confident = is_confident(&hits, typed);
        hits.truncate(MAX_CANDIDATES);
        Resolution { candidates: hits.into_iter().map(|h| h.cand).collect(), confident }
    }
}

/// Wirkt die Absicht auf das fokussierte Fenster (das vor dem Öffnen des Panels)?
fn on_focused_window(intent: &Intent) -> bool {
    matches!(intent, Intent::ToggleWindow { .. } | Intent::CloseWindow { query: None } | Intent::ColumnDisplay { .. })
}

/// `app_id` (klein) → Index des Programms. Reihenfolge der Schlüssel nach Verlässlichkeit: `StartupWMClass`, ID ohne
/// `.desktop`, letzter Teil einer ID mit Punkten (`org.mozilla.firefox`), Programmdatei, Name; der erste Eintrag gewinnt.
fn app_links(apps: &[App]) -> HashMap<String, usize> {
    let mut links: HashMap<String, usize> = HashMap::with_capacity(apps.len() * 3);
    type KeyOf = fn(&App) -> Option<String>;
    let passes: [KeyOf; 5] = [
        |a| a.startup_wm_class.as_ref().map(|c| c.to_lowercase()),
        |a| Some(a.id.strip_suffix(".desktop").unwrap_or(&a.id).to_lowercase()),
        |a| a.id.strip_suffix(".desktop").unwrap_or(&a.id).rsplit_once('.').map(|(_, last)| last.to_lowercase()),
        |a| a.exec_argv.first().and_then(|p| p.rsplit('/').next()).map(str::to_lowercase),
        |a| Some(a.name_default.to_lowercase()),
    ];
    for key_of in passes {
        for (i, app) in apps.iter().enumerate() {
            if let Some(key) = key_of(app).filter(|k| !k.is_empty()) {
                links.entry(key).or_insert(i);
            }
        }
    }
    links
}

/// Ist der erste Kandidat sicher genug für Return ohne Modell? Regel und Begründung im Modulkopf.
fn is_confident(hits: &[Hit], typed: usize) -> bool {
    let Some(top) = hits.first() else { return false };
    if top.quality < CONFIDENT_MIN_QUALITY || (!top.direct && typed < CONFIDENT_MIN_CHARS) {
        return false;
    }
    let rival = hits[1..]
        .iter()
        // der Neustart desselben Programms ist kein Konkurrent seines laufenden Fensters
        .filter(|h| !(top.role == Role::Window && h.role == Role::Launch && h.app.is_some() && h.app == top.app))
        .map(|h| h.quality)
        .fold(0.0, f32::max);
    let exact = |q: f32| q >= Q_EXACT - 0.01;
    if exact(top.quality) {
        !exact(rival)
    } else {
        top.quality - rival >= CONFIDENT_GAP
    }
}

#[cfg(test)]
mod tests;
