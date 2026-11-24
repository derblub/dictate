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

//! Bericht der Diagnose (Aufgabe 5, `PLAN-diagnose.md` „Vorschau vor dem Senden, immer“, „Größenbudget“, „Drossel gegen
//! Hitze“): sammelt die Proben eines Plans ([`super::profile::Plan`]) nacheinander, filtert sie ([`super::filter`]) und baut
//! daraus den Text, den die Vorschau zeigt und den das Modell bekommt.
//!
//! **Eine Sitzung** ([`Session`]) hält alles, was ein Bericht braucht: die Pseudonyme ([`Pseudonyms`], eine Zuordnung für
//! alle Runden), die Abschnitte mit ihrem *gefilterten* Text, die Abwahl und das Größenbudget. Rohdaten einer Probe
//! ([`ProbeOutput`]) leben nur während der Sammlung und kommen nie in die Sitzung. `Debug` zeigt nur Zahlen und Namen der
//! Proben, nie Inhalte; nichts davon wird geloggt.
//!
//! **Vorschau = gesendeter Text.** [`Session::report`] ist eine reine Funktion des Zustands (Abschnitte, Abwahl, Budget):
//! gleicher Zustand, gleicher Text, byte für byte. Die Vorschau zeigt `report().text`, gesendet wird [`Session::outgoing`],
//! und das ist dieselbe Funktion. Wer zwischen Vorschau und Senden etwas ändert (Abwahl, neue Runde), bekommt beim Senden
//! den geänderten Text, den die nächste Vorschau gezeigt hätte; ein Aufrufer, der „genau das Gezeigte“ braucht, vergleicht
//! den Text.
//!
//! # Sammeln
//!
//! Zwei Schritte, damit der Daemon die Sitzung nicht bis zu 15 s festhält: [`gather_plan`] bzw. [`gather_probes`] sammeln
//! ohne Sitzung (Sperre, Budget, Abbruch, Fortschritt) und liefern [`Gathered`] (die Rohdaten); [`Session::add`] filtert sie
//! und macht Abschnitte daraus. [`Session::collect_plan`] und [`Session::collect_probes`] tun beides.
//!
//! - **Nacheinander, mit Gesamtbudget** ([`COLLECT_BUDGET`], 15 s): eine Probe startet nur, wenn ihre Frist samt dem
//!   Überlauf des Ausführers ([`MAX_OVERRUN`]) ins Restbudget passt. Die Frist ist die des Katalogs, gekürzt auf
//!   `Restbudget − MAX_OVERRUN`; bleibt dafür weniger als [`MIN_PROBE_TIME`] (oder, bei einer kürzeren Frist, die Frist
//!   selbst), startet die Probe nicht und der Abschnitt heißt `skipped` („skipped: the collection time budget … is used up“,
//!   damit das Modell „keine Daten“ nicht als „kein Befund“ liest). So endet jede Sammlung nach höchstens
//!   `Gesamtbudget` plus der Zeit, die das Betriebssystem für das Beenden braucht.
//! - **Nie zwei Sammlungen zugleich** (Review I3: `run` sperrt nicht selbst): [`CollectLock`], ein Mutex für den Prozess.
//!   Eine zweite Sammlung wird **abgewiesen** ([`CollectError::Busy`]), sie wartet nicht. Gründe: Warten blockiert den
//!   Verbindungs-Thread bis zu 15 s ohne Rückmeldung; mehrere Wartende liefen nacheinander und erzeugten so über Minuten die
//!   Last, die die Drossel verhindern soll; und eine Sammlung, die erst nach einer anderen läuft, ist meist veraltet (der
//!   User hat zweimal gedrückt). Das Panel zeigt „a collection is already running“.
//! - **Abbruch** über [`CancelFlag`]: der Ausführer beendet die laufende Probe in unter 200 ms, danach startet keine mehr,
//!   und die Sammlung endet mit [`CollectError::Cancelled`]. Die Sitzung bleibt dann unverändert (kein halber Bericht).
//! - **Fortschritt** je Probe über einen Rückruf ([`Progress::Start`], [`Progress::End`] mit Status, Bytes und Dauer). Die
//!   Zahl der Schritte kann beim Docker-Profil wachsen, sobald die Liste der Container da ist.
//!
//! # Der Bericht
//!
//! Text: ein Kopf (Profil, Zeit, Abschnitte, Legende der Platzhalter), dann je Abschnitt eine Kopfzeile
//! `=== S2 · boot_end · boot=-1 lines=200 · 188 lines · 14.3 KiB · 3 redactions (api_key 2, …) · 5 placeholders (ip 3, …)`
//! (Probe, Parameter, Zeilen, Bytes, Schwärzungen; dazu `status:` wenn nicht ok, `cut:` wenn gekürzt, `round N` ab der
//! zweiten Runde), bei aufgelösten Parametern eine Zeile `resolved: …`, dann der Text.
//!
//! **Zeilennummern.** Jede Zeile des Textes trägt ihre Nummer, ab 1, gefolgt von `| ` ([`number_lines`]; `12| …`, eine leere
//! Zeile `13|`). Kopfzeile und `resolved:` gehören zum Kopf und haben keine Nummer; Zeile 1 ist die erste Zeile darunter,
//! eine Zeile „… left out to stay within the report budget“ zählt mit. Diese eine Zählung gilt für die Belege des Modells
//! (Prompt und Schema nennen die Nummer vor `| `), die Prüfung der Belege in Rust (`1 ≤ from ≤ to ≤ lines`) und den Sprung
//! zum Beleg im Panel (es liest die Nummer am Zeilenanfang; bis W7 `diagnose.lua`). Früher zählte das Modell selbst und lag im
//! Messschritt (Aufgabe 10b) teils um eine Zeile daneben (`resolved:` mitgezählt oder nicht). Kosten: im Mittel 4 bis 5
//! Bytes und etwa 2 Tokens je Zeile, bei Zeilen von 80 bis 150 Zeichen rund 5 % der Bytes und 8 bis 10 % der Tokens eines
//! Berichts (bei 48 KiB etwa 1 000 Tokens, mit Sonnet 5.5 etwa 0,2 ct). Die Nummern zählen zum Budget (Kürzen und
//! Verteilen rechnen mit ihnen), Vorschau und Gesendetes bleiben byte-gleich.
//!
//! Kopfzeile und `resolved` laufen durch denselben Filter wie der Text ([`filter_line`]), ihre Schwärzungen zählen im
//! Abschnitt mit.
//!
//! **Abgewählte Abschnitte** stehen als eine Zeile `=== S3 · pstore · left out by the user` im Bericht, ohne Parameter und
//! Zahlen: das Modell soll „der User hat es nicht geteilt“ nicht als „leer“ lesen, und was der User aus Gründen der
//! Vertraulichkeit abwählt, verrät die Zeile nicht.
//!
//! **Budget.** Der Bericht ist höchstens [`ReportBudget`] groß (Vorgabe 48 KiB, einstellbar 8 bis 192 KiB), *einschließlich*
//! Kopf und Kopfzeilen. Jede Probe hat ein Teilbudget (höchstens ihre Grenze im Katalog und höchstens die Hälfte des
//! Gesamtbudgets), und das Gesamtbudget wird fair verteilt (max-min: wer weniger braucht als sein Anteil, behält alles, der
//! Rest teilt sich den Rest). Gekürzt wird der *gefilterte* Text in ganzen Zeilen, damit der Filter immer den ganzen
//! Text der Probe sieht (ein mitten durch einen Schlüsselblock geschnittener Text würde nicht mehr erkannt, und Namen
//! aus gekürzten Zeilen lernt das Pseudonymisieren trotzdem). Journale und Zeitreihen ([`CutPolicy::Tail`]) verlieren die
//! ersten Zeilen (das Ende ist wichtiger), alle anderen die letzten; eine Zeile im Text und `cut:` im Kopf sagen es. Die
//! Zahlen der Schwärzungen im Kopf gelten für den Abschnitt, wie er gesammelt wurde (auch für Zeilen, die das Budget
//! gekürzt hat). Weil das Budget für das gilt, was *gesendet* wird, gewinnen die übrigen Abschnitte Platz zurück, wenn der
//! User einen abwählt. Würde ein Abschnitt weniger als [`MIN_USEFUL_BODY`] Bytes Text behalten (viele Abschnitte, kleines
//! Budget), werden die ältesten Abschnitte zu Einzeilern (`left out: the report budget … is used up`), bis die übrigen genug
//! bekommen; der Bericht bleibt in jedem Fall im Budget.
//!
//! Zeilen in fremdem Text, die wie eine Kopfzeile aussehen (`=== S9 · …`), erhalten ein Leerzeichen davor: ein Log kann
//! keinen Abschnitt vortäuschen.
//!
//! **Symptom des Users** (Entscheidung Daniel, 21.11., PLAN-wayland.md 6e Punkt 7): ein kurzer Text „What is wrong?“ aus
//! dem Panel steht als eigene, gekennzeichnete Zeile ([`SYMPTOM_LABEL`]) am Ende des Kopfs, vor den Abschnitten, und ist
//! damit Teil von Vorschau, Copy und Gesendetem (Vorschau = Senden). Er läuft durch denselben Filter wie die Kopfzeilen
//! ([`filter_line`]: Geheimnisse, Pseudonyme) und bleibt eine Zeile ohne Steuer-, Format- und private Zeichen
//! ([`clean_symptom`], höchstens [`MAX_SYMPTOM_CHARS`] Zeichen); er ist kein Abschnitt (Belege zählen nur in Abschnitten)
//! und kann keine Kopfzeile vortäuschen (die Zeile beginnt immer mit der Kennzeichnung). Den bereinigten Text behält die
//! Sitzung als [`SecretText`] und filtert ihn nach jeder Runde neu, damit Namen, die eine spätere Probe erst lehrt, auch
//! im Symptom zum Platzhalter werden. Nie loggen.

use super::filter::{filter_line, filter_outputs, FilterCounts};
use super::probe::Probe;
use super::probes::text::human_bytes;
use super::probes::{CollectOptions, Collector, ListedBoots, ProbeContext, ProbeOutput, Redaction, Status};
use super::profile::{container_probes, failed_unit_choices, problem_containers, problem_containers_note, Plan, Profile, Step, MAX_PROBLEM_CONTAINERS};
use super::pseudo::{Category, Identity, Pseudonyms};
use super::run::MAX_OVERRUN;
use super::UnitRef;
use crate::actions::store::SecretText;
use crate::pipeline::ai::CancelFlag;
use serde_json::Value;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};

/// Gesamtbudget einer Sammlung (`PLAN-diagnose.md`: „Gesamtbudget je Sammlung 15 s“).
pub const COLLECT_BUDGET: Duration = Duration::from_secs(15);
/// Mindestens so lange darf eine Probe laufen, sonst startet sie nicht (sie würde nur in die Frist laufen). Gilt als
/// Obergrenze der Mindestzeit; eine Probe mit kürzerer Frist braucht nur diese.
pub const MIN_PROBE_TIME: Duration = Duration::from_secs(1);
/// So viele Abschnitte hält eine Sitzung höchstens (alle Runden zusammen). Mit dem kleinsten Budget (8 KiB) passen auch
/// lauter Einzeiler samt Kopf hinein.
pub const MAX_SECTIONS: usize = 96;
/// Weniger als so viele Bytes Text (oder ihr ganzer Text, wenn der kürzer ist) bekommt ein Abschnitt im Bericht nicht: reicht das
/// Budget dafür nicht, werden die ältesten Abschnitte zu Einzeilern, statt dass alle zu leeren Köpfen werden.
const MIN_USEFUL_BODY: usize = 256;
/// So viele Proben kann ein Schritt höchstens anhängen (Docker: zwei je Container).
const MAX_EXPANSION: usize = 2 * MAX_PROBLEM_CONTAINERS;

// ---- Größenbudget ------------------------------------------------------------------------------------------------------

/// Größe des Berichts an das Modell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportBudget {
    bytes: usize,
}

impl ReportBudget {
    /// Kleinste einstellbare Größe in KiB.
    pub const MIN_KIB: usize = 8;
    /// Größte einstellbare Größe in KiB.
    pub const MAX_KIB: usize = 192;
    /// Vorgabe in KiB.
    pub const DEFAULT_KIB: usize = 48;
    /// Die Vorgabe (48 KiB).
    pub const DEFAULT: ReportBudget = ReportBudget { bytes: Self::DEFAULT_KIB * 1024 };

    /// Das Budget zu einer Größe in KiB; außerhalb von 8 bis 192 ein Fehler (die Config meldet ihn beim Speichern).
    pub fn from_kib(kib: usize) -> Result<ReportBudget, String> {
        if (Self::MIN_KIB..=Self::MAX_KIB).contains(&kib) {
            Ok(ReportBudget { bytes: kib * 1024 })
        } else {
            Err(format!("the report size must be between {} and {} KiB, got {kib}", Self::MIN_KIB, Self::MAX_KIB))
        }
    }

    pub fn bytes(self) -> usize {
        self.bytes
    }

    /// Teilbudget einer Probe: höchstens ihre Grenze im Katalog und höchstens die Hälfte des Gesamtbudgets.
    pub fn part_cap(self, probe: &str) -> usize {
        let limit = super::catalog::spec(probe).map_or(usize::MAX, |s| s.limit_bytes);
        limit.min(self.bytes / 2)
    }
}

impl Default for ReportBudget {
    fn default() -> ReportBudget {
        ReportBudget::DEFAULT
    }
}

// ---- Sperre -----------------------------------------------------------------------------------------------------------

/// Sperre, die zwei Sammlungen zugleich verhindert (Review I3). Die Sperre des Prozesses gibt [`CollectLock::global`]; Tests
/// bauen eigene, damit sie sich nicht gegenseitig abweisen.
pub struct CollectLock {
    inner: Mutex<()>,
}

static GLOBAL_LOCK: CollectLock = CollectLock::new();

impl CollectLock {
    pub const fn new() -> CollectLock {
        CollectLock { inner: Mutex::new(()) }
    }

    /// Die Sperre des Prozesses: jede Sammlung der Diagnose nimmt sie.
    pub fn global() -> &'static CollectLock {
        &GLOBAL_LOCK
    }

    /// Nimmt die Sperre, falls sie frei ist; sonst `None` (nie warten). Eine vergiftete Sperre (eine Sammlung ist mit einer
    /// Panik geendet) gilt als frei: sie schützt keine Daten, nur die Reihenfolge.
    pub fn try_acquire(&self) -> Option<CollectGuard<'_>> {
        match self.inner.try_lock() {
            Ok(guard) => Some(CollectGuard { _guard: guard }),
            Err(TryLockError::Poisoned(p)) => Some(CollectGuard { _guard: p.into_inner() }),
            Err(TryLockError::WouldBlock) => None,
        }
    }
}

impl Default for CollectLock {
    fn default() -> CollectLock {
        CollectLock::new()
    }
}

/// Hält die Sperre, bis er verschwindet.
pub struct CollectGuard<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
}

// ---- Fortschritt, Fehler, Parameter ------------------------------------------------------------------------------------

/// Fortschritt der Sammlung, ein Ereignispaar je Probe. Enthält Parameter und Meldungen (für das Panel des Users); ein
/// Aufrufer loggt höchstens Probe, Status, Bytes und Dauer. Das `Debug` zeigt nur das.
#[derive(Clone, PartialEq, Eq)]
pub enum Progress {
    /// Die Probe `index` von `total` (ab 1) beginnt. `total` kann wachsen (Docker).
    Start { index: usize, total: usize, probe: &'static str, params: String },
    /// Die Probe ist zu Ende (auch übersprungen oder abgebrochen). `bytes` ist die Größe ihres Textes vor Filter und Budget.
    End { index: usize, total: usize, probe: &'static str, status: &'static str, message: Option<String>, bytes: usize, duration: Duration },
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Progress::Start { index, total, probe, .. } => f.debug_struct("Start").field("index", index).field("total", total).field("probe", probe).finish(),
            Progress::End { index, total, probe, status, bytes, duration, .. } => f
                .debug_struct("End")
                .field("index", index)
                .field("total", total)
                .field("probe", probe)
                .field("status", status)
                .field("bytes", bytes)
                .field("duration", duration)
                .finish(),
        }
    }
}

/// Warum keine Sammlung stattfand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectError {
    /// Eine andere Sammlung läuft (siehe Modulkopf: abgewiesen, nicht gewartet).
    Busy,
    /// Abgebrochen; die Sitzung ist unverändert.
    Cancelled,
    /// Die Sitzung hat schon zu viele Abschnitte ([`MAX_SECTIONS`]).
    TooManySections,
}

impl fmt::Display for CollectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollectError::Busy => write!(f, "a collection is already running"),
            CollectError::Cancelled => write!(f, "the collection was cancelled"),
            CollectError::TooManySections => write!(f, "the report already has {MAX_SECTIONS} sections"),
        }
    }
}

impl std::error::Error for CollectError {}

/// Warum sich die Abwahl nicht ändern ließ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// Es gibt keinen Abschnitt mit dieser Kennung.
    NoSuchSection(String),
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectError::NoSuchSection(id) => write!(f, "the report has no section {:?}", id.chars().take(16).collect::<String>()),
        }
    }
}

impl std::error::Error for SelectError {}

/// Parameter einer Sammlung.
#[derive(Debug, Clone, Copy)]
pub struct CollectParams {
    /// Gesamtbudget ([`COLLECT_BUDGET`]).
    pub total: Duration,
    /// Der User hat Netzverkehr bestätigt (nur `net_reach`, nie in einem Profil).
    pub network_confirmed: bool,
}

impl Default for CollectParams {
    fn default() -> CollectParams {
        CollectParams { total: COLLECT_BUDGET, network_confirmed: false }
    }
}

/// Ergebnis einer Sammlung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    /// Kennungen der neuen Abschnitte in der Reihenfolge des Berichts.
    pub ids: Vec<String>,
    /// Wie lange das Sammeln dauerte.
    pub elapsed: Duration,
    /// Wie viele Proben das Gesamtbudget übersprungen hat.
    pub skipped: usize,
}

// ---- Kürzen -----------------------------------------------------------------------------------------------------------

/// Was bei einem Überlauf des Teilbudgets bleibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutPolicy {
    /// Der Anfang bleibt.
    Head,
    /// Das Ende bleibt (Journale, Zeitreihen); die ersten `pin` Zeilen (Spaltenköpfe) bleiben auch.
    Tail { pin: usize },
}

/// Die Regel für eine Probe: Journale und Zeitreihen verlieren vorn, alles andere hinten.
pub fn cut_policy(probe: &str) -> CutPolicy {
    match probe {
        "boot_end" | "kernel_errors" | "oom" | "unit_journal" | "docker_logs" | "coredumps" | "pacman_log" => CutPolicy::Tail { pin: 0 },
        // erste Zeile: Spaltenkopf der CSV
        "temps_tail" => CutPolicy::Tail { pin: 1 },
        _ => CutPolicy::Head,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Cut {
    text: String,
    kept: usize,
    total: usize,
}

/// Trenner hinter der Zeilennummer im Bericht (`12| …`, siehe Modulkopf).
pub const LINE_MARK: &str = "| ";

/// Stellt jeder Zeile ihre Nummer voran, ab 1 (`1| erste`, `2|` für eine leere Zeile): so steht jede Zeile eines Abschnitts im
/// Bericht, und so zählen Belege, Panel und Prüfung.
pub fn number_lines(text: &str) -> String {
    let mut out = String::with_capacity(numbered_len(text));
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&(i + 1).to_string());
        if line.is_empty() {
            out.push_str(LINE_MARK.trim_end());
        } else {
            out.push_str(LINE_MARK);
            out.push_str(line);
        }
    }
    out
}

/// Länge von [`number_lines`]`(text)` ohne den Text zu bauen.
pub fn numbered_len(text: &str) -> usize {
    text.lines().enumerate().map(|(i, l)| digits(i + 1) + LINE_MARK.trim_end().len() + if l.is_empty() { 0 } else { 1 + l.len() }).sum::<usize>()
        + text.lines().count().saturating_sub(1)
}

fn digits(n: usize) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}

/// Kürzt `body` in ganzen Zeilen so, dass er **mit Zeilennummern** ([`number_lines`]) höchstens `cap` Bytes hat. Eine Zeile am
/// Rand des Behaltenen sagt, wie viele fehlen (sie zählt zur Grenze und hat eine Nummer); passt nicht einmal sie, bleibt
/// der Text leer (der Kopf des Abschnitts sagt es dann). `text` ist ohne Nummern.
fn cut_body(body: &str, cap: usize, policy: CutPolicy) -> Cut {
    let lines: Vec<&str> = body.lines().collect();
    let total = lines.len();
    if numbered_len(body) <= cap {
        return Cut { text: body.to_string(), kept: total, total };
    }
    // jede behaltene Zeile kostet höchstens so viel mehr wie die längste Nummer und der Trenner
    let number = digits(total.max(1)) + LINE_MARK.len();
    let (tail, pin) = match policy {
        CutPolicy::Head => (false, 0),
        CutPolicy::Tail { pin } => (true, pin.min(total)),
    };
    let marker = |omitted: usize| {
        if tail {
            format!("[… {omitted} earlier lines left out to stay within the report budget]")
        } else {
            format!("[… {omitted} later lines left out to stay within the report budget]")
        }
    };
    let reserve = marker(total).len() + 1 + number;
    if cap < reserve {
        return Cut { text: String::new(), kept: 0, total };
    }
    let mut budget = cap - reserve;
    // die angehefteten Zeilen nur, wenn alle hineinpassen
    let pinned_bytes: usize = lines[..pin].iter().map(|l| l.len() + 1 + number).sum();
    let pin = if pinned_bytes <= budget { pin } else { 0 };
    budget -= lines[..pin].iter().map(|l| l.len() + 1 + number).sum::<usize>();
    let rest = &lines[pin..];
    let mut taken: Vec<&str> = Vec::new();
    let mut used = 0usize;
    let ordered: Box<dyn Iterator<Item = &&str>> = if tail { Box::new(rest.iter().rev()) } else { Box::new(rest.iter()) };
    for line in ordered {
        if used + line.len() + 1 + number > budget {
            break;
        }
        used += line.len() + 1 + number;
        taken.push(line);
    }
    let omitted = total - pin - taken.len();
    let mut out = String::with_capacity(cap);
    for line in &lines[..pin] {
        out.push_str(line);
        out.push('\n');
    }
    if tail {
        out.push_str(&marker(omitted));
        out.push('\n');
        for line in taken.iter().rev() {
            out.push_str(line);
            out.push('\n');
        }
    } else {
        for line in &taken {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&marker(omitted));
        out.push('\n');
    }
    Cut { text: out.trim_end_matches('\n').to_string(), kept: pin + taken.len(), total }
}

/// Verteilt `avail` Bytes fair (max-min) auf Abschnitte mit diesem Bedarf: wer weniger braucht als sein Anteil, bekommt
/// alles, der Rest teilt sich den Rest. Die Summe ist höchstens `avail`.
fn fair_share(demands: &[usize], avail: usize) -> Vec<usize> {
    let n = demands.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (demands[i], i));
    let mut caps = vec![0usize; n];
    let mut left = avail;
    for (k, &i) in order.iter().enumerate() {
        let share = left / (n - k);
        let give = demands[i].min(share);
        caps[i] = give;
        left -= give;
    }
    caps
}

// ---- Abschnitte und Bericht ------------------------------------------------------------------------------------------

/// Ein Abschnitt der Sitzung: gefilterter Text und Zählung, nie Rohdaten.
struct Section {
    id: String,
    round: u32,
    probe: &'static str,
    /// Parameter als `name=wert`, gefiltert.
    params: String,
    /// Aufgelöste Parameter, gefiltert.
    resolved: String,
    status: &'static str,
    policy: CutPolicy,
    /// Der vollständige gefilterte Text (das Budget kürzt erst beim Bauen des Berichts).
    body: String,
    counts: FilterCounts,
    included: bool,
}

/// Ein Abschnitt für das Panel (Auswahl, Vorschau).
#[derive(Clone, PartialEq, Eq)]
pub struct SectionView {
    pub id: String,
    pub probe: &'static str,
    /// Parameter, gefiltert (wie im Kopf).
    pub params: String,
    /// `ok`, `empty`, `partial`, `failed`, `timeout`, `cancelled`, `unavailable`, `needs_confirmation`, `skipped`.
    pub status: &'static str,
    /// Im Bericht (nicht abgewählt).
    pub included: bool,
    /// Zeilen und Bytes des Textes, wie er im Bericht steht (ohne Kopf). 0, wenn abgewählt.
    pub lines: usize,
    pub bytes: usize,
    /// Zeilen des gesammelten Textes.
    pub total_lines: usize,
    /// Das Budget hat Zeilen gekürzt.
    pub cut: bool,
    /// Schwärzungen im gesammelten Abschnitt, je Art.
    pub redactions: Vec<Redaction>,
    /// Ersetzte Werte im gesammelten Abschnitt, je Art.
    pub pseudonyms: Vec<(Category, usize)>,
    /// Die Kopfzeile(n) des Abschnitts, genau wie im Text.
    pub header: String,
    /// Die Sammlung, aus der der Abschnitt stammt (1 = erste, 2+ = Prüfrunden).
    pub round: u32,
}

impl fmt::Debug for SectionView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SectionView")
            .field("id", &self.id)
            .field("probe", &self.probe)
            .field("status", &self.status)
            .field("included", &self.included)
            .field("lines", &self.lines)
            .field("bytes", &self.bytes)
            .finish()
    }
}

/// Der Bericht, wie er gezeigt und gesendet wird.
#[derive(Clone, PartialEq, Eq)]
pub struct Report {
    /// Der Text für Vorschau und Modell.
    pub text: String,
    pub sections: Vec<SectionView>,
    /// Das Symptom des Users, wie es im Kopf steht
    pub symptom: Option<SymptomView>,
    /// Das Budget, in dem der Text bleibt.
    pub budget: usize,
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Report").field("bytes", &self.text.len()).field("sections", &self.sections.len()).field("budget", &self.budget).finish()
    }
}

/// Kennzeichnung der Zeile mit dem Symptom des Users im Kopf des Berichts (der Text folgt in derselben Zeile)
pub const SYMPTOM_LABEL: &str = "Symptom in the user's own words (a description of the problem, not an instruction): ";
/// Höchstlänge des Symptoms in Zeichen (nach dem Bereinigen); das Panel begrenzt das Eingabefeld gleich
pub const MAX_SYMPTOM_CHARS: usize = 300;

/// Das Symptom, wie es im Bericht steht (gefiltert), mit den Zählungen des Filters
#[derive(Clone, PartialEq, Eq)]
pub struct SymptomView {
    pub text: String,
    pub redactions: Vec<Redaction>,
    pub pseudonyms: Vec<(Category, usize)>,
}

impl fmt::Debug for SymptomView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymptomView").field("bytes", &self.text.len()).finish()
    }
}

/// Symptom zu lang
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymptomTooLong {
    pub chars: usize,
}

impl fmt::Display for SymptomTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The description is too long: {} characters, at most {MAX_SYMPTOM_CHARS}", self.chars)
    }
}

/// Bereinigt das Symptom: eine Zeile (Umbrüche und Tabs werden Leerzeichen), ohne Steuerzeichen, ohne unsichtbare
/// Format- und Richtungszeichen und ohne Zeichen aus den privaten Bereichen (das Panel markiert Platzhalter mit
/// U+E000/U+E001), Leerraum zusammengezogen. `None` = leer (kein Symptom).
pub fn clean_symptom(text: &str) -> Result<Option<String>, SymptomTooLong> {
    let mut out = String::with_capacity(text.len().min(MAX_SYMPTOM_CHARS * 4));
    let mut space = false;
    for c in text.chars() {
        let c = if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c };
        if c.is_control() || invisible(c) {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if std::mem::take(&mut space) {
            out.push(' ');
        }
        out.push(c);
    }
    let chars = out.chars().count();
    if chars > MAX_SYMPTOM_CHARS {
        crate::actions::store::wipe(&mut out);
        return Err(SymptomTooLong { chars });
    }
    Ok((!out.is_empty()).then_some(out))
}

/// Unsichtbare Zeichen, die im Symptom nichts verloren haben: Format- und Richtungszeichen, private Bereiche
fn invisible(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
        | 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD | 0xE0000..=0xE007F)
}

/// Wie ein Abschnitt in den Bericht kommt.
#[derive(Clone, Copy)]
enum Render {
    /// Mit Text, höchstens so viele Bytes.
    Body(usize),
    /// Der User hat ihn abgewählt.
    LeftOutByUser,
    /// Das Budget reicht nicht einmal für den Kopf.
    LeftOutBudget,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn counts_total(c: &FilterCounts) -> (usize, usize) {
    (c.redactions.iter().map(|r| r.count).sum(), c.pseudonyms.iter().map(|(_, n)| *n).sum())
}

fn merge_counts(into: &mut FilterCounts, from: &FilterCounts) {
    for r in &from.redactions {
        match into.redactions.iter_mut().find(|x| x.kind == r.kind) {
            Some(x) => x.count += r.count,
            None => into.redactions.push(r.clone()),
        }
    }
    for (cat, n) in &from.pseudonyms {
        match into.pseudonyms.iter_mut().find(|(c, _)| c == cat) {
            Some((_, m)) => *m += n,
            None => into.pseudonyms.push((*cat, *n)),
        }
    }
    into.pseudonyms.sort_by_key(|(c, _)| Category::ALL.iter().position(|x| x == c));
}

/// `name=wert` je Parameter in der Reihenfolge des Katalogs; leer ohne Parameter (der Kopf lässt das Feld dann weg).
pub(crate) fn describe_params(probe: &Probe) -> String {
    let value = serde_json::to_value(probe).unwrap_or(Value::Null);
    let parts: Vec<String> = probe
        .spec()
        .params
        .iter()
        .filter_map(|p| {
            let v = value.get(p.name)?;
            Some(match v {
                Value::String(s) => format!("{}={s}", p.name),
                other => format!("{}={other}", p.name),
            })
        })
        .collect();
    parts.join(" ")
}

/// Anfänge von Zeilen, die nur Rust in den Bericht schreibt: der Kopf eines Abschnitts und die Zeile `resolved:` darunter.
const HEADER_STARTS: [&str; 2] = ["=== ", "resolved: "];

/// Zeilen, die wie eine Kopfzeile des Berichts aussehen (Kopf eines Abschnitts oder `resolved:`), bekommen ein
/// Leerzeichen davor: fremder Text kann keinen Abschnitt und keine aufgelösten Parameter vortäuschen (Review L2; das Panel
/// und das Modell lesen eine Zeile `resolved: …` direkt unter dem Kopf als Teil des Kopfes, und die Zählung der Zeilen
/// für die Belege beginnt dahinter).
fn neutralize_headers(text: &str) -> String {
    if !HEADER_STARTS.iter().any(|h| text.contains(h)) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if HEADER_STARTS.iter().any(|h| line.starts_with(h)) {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// Die Sitzung einer Diagnose: Abschnitte, Pseudonyme, Abwahl, Budget. Lebt im Speicher des Daemons (höchstens 30 min, bis
/// `diag_clear`), wird nie geschrieben, nie geloggt.
pub struct Session {
    pseudo: Pseudonyms,
    budget: ReportBudget,
    profile: Option<Profile>,
    collected_at: Option<String>,
    sections: Vec<Section>,
    next_id: usize,
    rounds: u32,
    /// Symptom des Users: bereinigt (vor dem Filter) und gefiltert, wie es im Bericht steht
    symptom: Option<(SecretText, String, FilterCounts)>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // nie Inhalte: nur Zahlen und die Namen der Proben
        f.debug_struct("Session")
            .field("pseudonyms", &self.pseudo)
            .field("budget", &self.budget)
            .field("rounds", &self.rounds)
            .field("symptom", &self.symptom.as_ref().map(|(raw, _, _)| raw.as_str().len()))
            .field("sections", &self.sections.iter().map(|s| format!("{} {} {}", s.id, s.probe, s.status)).collect::<Vec<_>>())
            .finish()
    }
}

impl Session {
    /// Eine leere Sitzung. `pseudonymize` ist der Schalter `diagnose.pseudonymize` (Vorgabe an).
    pub fn new(identity: Identity, pseudonymize: bool, budget: ReportBudget) -> Session {
        Session {
            pseudo: Pseudonyms::new(identity, pseudonymize),
            budget,
            profile: None,
            collected_at: None,
            sections: Vec::new(),
            next_id: 1,
            rounds: 0,
            symptom: None,
        }
    }

    /// Das Profil der ersten Sammlung, falls sie eines hatte.
    pub fn profile(&self) -> Option<Profile> {
        self.profile
    }

    pub fn budget(&self) -> ReportBudget {
        self.budget
    }

    /// Wie viele Sammlungen die Sitzung hatte.
    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Übersetzt Platzhalter in einem Text zurück (nur für die Anzeige im Panel, nie für Befehle oder Links, nie zum Modell).
    pub fn restore(&self, text: &str) -> String {
        self.pseudo.restore(text)
    }

    /// Der Schalter `diagnose.pseudonymize`, mit dem diese Sitzung gesammelt hat (gilt für alle ihre Runden; eine spätere
    /// Änderung der Config ändert den Bericht nicht, Review L4).
    pub fn pseudonymize(&self) -> bool {
        self.pseudo.enabled()
    }

    /// Spannen zur Rückübersetzung in einem Text des Modells (Vorgabe I1, [`super::restore`]): nur Platzhalter dieser
    /// Sitzung außerhalb von Code, Links und URLs. `markdown` = der Text ist Markdown (Notizen; Codeblöcke zählen).
    pub fn restore_spans(&self, text: &str, markdown: bool) -> Vec<super::restore::RestoreSpan> {
        super::restore::spans(text, markdown, self.pseudo.known_placeholders(text))
    }

    // ---- Sammeln

    /// Wie viele Abschnitte noch Platz haben ([`MAX_SECTIONS`]); der Daemon prüft es vor dem Sammeln.
    pub fn room(&self) -> usize {
        MAX_SECTIONS.saturating_sub(self.sections.len())
    }

    fn check_room(&self, steps: usize) -> Result<(), CollectError> {
        if steps + MAX_EXPANSION > self.room() {
            Err(CollectError::TooManySections)
        } else {
            Ok(())
        }
    }

    /// Sammelt die angewählten Schritte eines Plans und hängt die Abschnitte an ([`gather_plan`] und [`Session::add`] in
    /// einem; wer die Sitzung während des Sammelns nicht festhalten will, ruft beides getrennt). Siehe Modulkopf für
    /// Budget, Sperre und Abbruch.
    pub fn collect_plan(
        &mut self,
        ctx: &ProbeContext,
        cancel: &CancelFlag,
        plan: &Plan,
        params: &CollectParams,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Collected, CollectError> {
        self.collect_steps(CollectLock::global(), ctx, cancel, Some(plan.profile()), plan.selected_steps(), params, progress)
    }

    /// Sammelt einzelne Proben (die bestätigten Prüfbefehle der KI in Runde 2 und 3) und hängt die Abschnitte an.
    pub fn collect_probes(
        &mut self,
        ctx: &ProbeContext,
        cancel: &CancelFlag,
        probes: &[Probe],
        params: &CollectParams,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Collected, CollectError> {
        self.collect_steps(CollectLock::global(), ctx, cancel, None, probes.iter().cloned().map(Step::Probe).collect(), params, progress)
    }

    /// Wie [`Session::collect_plan`] mit einer eigenen Sperre (Tests).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn collect_steps(
        &mut self,
        lock: &CollectLock,
        ctx: &ProbeContext,
        cancel: &CancelFlag,
        profile: Option<Profile>,
        steps: Vec<Step>,
        params: &CollectParams,
        progress: &mut dyn FnMut(&Progress),
    ) -> Result<Collected, CollectError> {
        // Zuerst die Sperre (wer abgewiesen wird, bekommt immer `Busy`), dann der Platz, dann erst läuft etwas
        let guard = lock.try_acquire().ok_or(CollectError::Busy)?;
        self.check_room(steps.len())?;
        let gathered = gather_locked(&guard, ctx, cancel, profile, steps, params, progress)?;
        self.add(gathered)
    }

    /// Filtert die Ausgaben einer Sammlung und hängt sie als Abschnitte an. Ab hier gibt es keine Rohdaten mehr: was in der
    /// Sitzung landet, ist gefiltert. Hat die Sitzung inzwischen keinen Platz mehr, bleibt sie unverändert.
    pub fn add(&mut self, gathered: Gathered) -> Result<Collected, CollectError> {
        if gathered.done.len() > self.room() {
            return Err(CollectError::TooManySections);
        }
        Ok(self.absorb(gathered))
    }

    /// Filtert die Ausgaben der Runde und macht Abschnitte daraus. Ab hier gibt es keine Rohdaten mehr.
    fn absorb(&mut self, gathered: Gathered) -> Collected {
        let Gathered { profile, done, elapsed, at } = gathered;
        self.rounds += 1;
        if self.profile.is_none() {
            self.profile = profile;
        }
        if self.collected_at.is_none() {
            self.collected_at = Some(at);
        }
        // Ab hier gibt es die Rohdaten nur noch hier; was in `self.sections` landet, ist gefiltert
        let (probes, outputs): (Vec<Probe>, Vec<ProbeOutput>) = done.into_iter().unzip();
        let filtered = filter_outputs(&outputs, &mut self.pseudo);
        let mut ids = Vec::new();
        let mut skipped = 0;
        for ((probe, output), filtered) in probes.iter().zip(&outputs).zip(filtered) {
            let mut counts = filtered.counts;
            // Kopfzeile und aufgelöste Parameter durch denselben Filter (Parameter sind Suchbegriffe des Users oder des Modells,
            // `resolved` nennt echte Namen aus den Listen des Systems)
            let (params, params_counts) = filter_line(&describe_params(probe), &mut self.pseudo);
            merge_counts(&mut counts, &params_counts);
            let resolved = if output.resolved.is_empty() {
                String::new()
            } else {
                // Namen aus den Listen des Systems sind fremder Text: nie ein Zeilenumbruch, der die Kopfzeile verlängert
                let joined = super::probes::text::clean(output.resolved.join("; ").as_bytes()).replace(['\n', '\r'], " ");
                let (text, resolved_counts) = filter_line(&joined, &mut self.pseudo);
                merge_counts(&mut counts, &resolved_counts);
                text
            };
            if matches!(output.status, Status::Skipped(_)) {
                skipped += 1;
            }
            let id = format!("S{}", self.next_id);
            self.next_id += 1;
            ids.push(id.clone());
            self.sections.push(Section {
                id,
                round: self.rounds,
                probe: output.probe,
                params,
                resolved,
                status: output.status.label(),
                policy: cut_policy(output.probe),
                body: neutralize_headers(&filtered.text),
                counts,
                included: true,
            });
        }
        // Namen, die diese Runde gelehrt hat, gelten auch im Symptom
        self.refilter_symptom();
        Collected { ids, elapsed, skipped }
    }

    // ---- Symptom des Users

    /// Setzt das Symptom (`None` oder leer: keins). `Ok(true)`, wenn sich der Bericht dadurch ändert.
    pub fn set_symptom(&mut self, text: Option<&str>) -> Result<bool, SymptomTooLong> {
        let before = self.symptom.as_ref().map(|(_, f, _)| f.clone());
        let cleaned = match text {
            Some(t) => clean_symptom(t)?,
            None => None,
        };
        // das alte überschreibt `SecretText` beim Freigeben
        self.symptom = cleaned.map(|c| (SecretText::new(c), String::new(), FilterCounts::default()));
        self.refilter_symptom();
        let after = self.symptom.as_ref().map(|(_, f, _)| f.as_str());
        let changed = before.as_deref() != after;
        if let Some(mut b) = before {
            crate::actions::store::wipe(&mut b);
        }
        Ok(changed)
    }

    /// Ob ein Symptom im Bericht steht (der System-Prompt nennt die Zeile dann)
    pub fn has_symptom(&self) -> bool {
        self.symptom.is_some()
    }

    fn refilter_symptom(&mut self) {
        if let Some((raw, filtered, counts)) = &mut self.symptom {
            let (text, c) = filter_line(raw.as_str(), &mut self.pseudo);
            crate::actions::store::wipe(filtered);
            *filtered = text;
            *counts = c;
        }
    }

    // ---- Abwahl

    /// Nimmt einen Abschnitt in den Bericht auf oder lässt ihn weg.
    pub fn set_included(&mut self, id: &str, included: bool) -> Result<(), SelectError> {
        let section = self.sections.iter_mut().find(|s| s.id == id).ok_or_else(|| SelectError::NoSuchSection(id.to_string()))?;
        section.included = included;
        Ok(())
    }

    /// Setzt die Abwahl als Ganzes: genau die genannten Abschnitte fehlen, alle anderen stehen im Bericht. Eine unbekannte
    /// Kennung ist ein Fehler und ändert nichts. So ist der Bericht eine Funktion von Sitzung und Abwahl.
    pub fn set_deselected(&mut self, ids: &[&str]) -> Result<(), SelectError> {
        if let Some(unknown) = ids.iter().find(|id| !self.sections.iter().any(|s| s.id == **id)) {
            return Err(SelectError::NoSuchSection((*unknown).to_string()));
        }
        for s in &mut self.sections {
            s.included = !ids.contains(&s.id.as_str());
        }
        Ok(())
    }

    // ---- Bericht

    /// Der Text, der an das Modell geht: genau der Text der Vorschau ([`Session::report`]).
    pub fn outgoing(&self) -> String {
        self.report().text
    }

    /// Der Bericht samt Abschnitten für das Panel (Vorschau): eine reine Funktion des Zustands.
    pub fn report(&self) -> Report {
        let budget = self.budget.bytes();
        let included: Vec<usize> = (0..self.sections.len()).filter(|&i| self.sections[i].included).collect();
        // Die ersten `dropped` Abschnitte fallen auf einen Einzeiler zurück, wenn das Budget selbst für die Köpfe nicht reicht
        for dropped in 0..=included.len() {
            let active = &included[dropped..];
            let mut modes: Vec<Render> =
                (0..self.sections.len()).map(|i| if self.sections[i].included { Render::Body(0) } else { Render::LeftOutByUser }).collect();
            for &i in &included[..dropped] {
                modes[i] = Render::LeftOutBudget;
            }
            // Der Platz für Köpfe: der Text ohne Inhalte, dazu ein Rest für längere Zahlen im Kopf
            let overhead = self.assemble(&modes).text.len() + 16 * active.len();
            let mut avail = budget.saturating_sub(overhead);
            for _ in 0..8 {
                let demands: Vec<usize> =
                    active.iter().map(|&i| numbered_len(&self.sections[i].body).min(self.budget.part_cap(self.sections[i].probe))).collect();
                let caps = fair_share(&demands, avail);
                // Ein Abschnitt, der kaum etwas behielte, nützt nicht: lieber der nächste Älteste auf einen Einzeiler
                if caps.iter().zip(&demands).any(|(cap, demand)| *cap < (*demand).min(MIN_USEFUL_BODY)) {
                    break;
                }
                for (&i, cap) in active.iter().zip(caps) {
                    modes[i] = Render::Body(cap);
                }
                let report = self.assemble(&modes);
                if report.text.len() <= budget {
                    return report;
                }
                avail = avail.saturating_sub(report.text.len() - budget);
                if avail == 0 {
                    break;
                }
            }
        }
        let modes: Vec<Render> =
            (0..self.sections.len()).map(|i| if self.sections[i].included { Render::LeftOutBudget } else { Render::LeftOutByUser }).collect();
        self.assemble(&modes)
    }

    fn preamble(&self) -> String {
        let mut head = String::from("Diagnostic report");
        if let Some(p) = self.profile {
            head.push_str(&format!(" · profile: {}", p.label()));
        }
        head.push('\n');
        if let Some(at) = &self.collected_at {
            head.push_str(&format!("Collected at {at}.\n"));
        }
        let included = self.sections.iter().filter(|s| s.included).count();
        match (self.sections.first(), self.sections.last()) {
            (Some(first), Some(last)) if first.id == last.id => head.push_str(&format!("Section {}: {} of 1 included.\n", first.id, included)),
            (Some(first), Some(last)) => head.push_str(&format!("Sections {}-{}: {} of {} included.\n", first.id, last.id, included, self.sections.len())),
            _ => head.push_str("No sections.\n"),
        }
        if self.pseudo.enabled() {
            let prefixes: Vec<String> = Category::ALL.iter().map(|c| format!("{}-N", c.prefix())).collect();
            head.push_str(&format!("Personal values are replaced by stable placeholders ({}) and secrets by [REDACTED:kind].\n", prefixes.join(", ")));
        } else {
            head.push_str("Secrets are replaced by [REDACTED:kind].\n");
        }
        if let Some((_, text, _)) = &self.symptom {
            head.push_str(SYMPTOM_LABEL);
            head.push_str(text);
            head.push('\n');
        }
        head
    }

    fn header(&self, s: &Section, lines: usize, bytes: usize, cut: Option<(usize, usize)>) -> String {
        let (redactions, placeholders) = counts_total(&s.counts);
        let mut h = format!("=== {} · {}", s.id, s.probe);
        if !s.params.is_empty() {
            h.push_str(&format!(" · {}", s.params));
        }
        h.push_str(&format!(" · {} · {}", plural(lines, "line", "lines"), human_bytes(bytes as u64)));
        if redactions == 0 {
            h.push_str(" · no redactions");
        } else {
            let kinds: Vec<String> = s.counts.redactions.iter().map(|r| format!("{} {}", r.kind, r.count)).collect();
            h.push_str(&format!(" · {} ({})", plural(redactions, "redaction", "redactions"), kinds.join(", ")));
        }
        if placeholders > 0 {
            let kinds: Vec<String> = s.counts.pseudonyms.iter().map(|(c, n)| format!("{} {}", c.prefix(), n)).collect();
            h.push_str(&format!(" · {} ({})", plural(placeholders, "placeholder", "placeholders"), kinds.join(", ")));
        }
        if s.status != "ok" {
            h.push_str(&format!(" · status: {}", s.status));
        }
        if let Some((kept, total)) = cut {
            h.push_str(&format!(" · cut: kept {kept} of {total} lines"));
        }
        if s.round > 1 {
            h.push_str(&format!(" · round {}", s.round));
        }
        if !s.resolved.is_empty() {
            h.push_str(&format!("\nresolved: {}", s.resolved));
        }
        h
    }

    fn assemble(&self, modes: &[Render]) -> Report {
        let mut text = self.preamble();
        let mut views = Vec::with_capacity(self.sections.len());
        for (s, mode) in self.sections.iter().zip(modes) {
            text.push('\n');
            let total_lines = s.body.lines().count();
            let (header, shown, cut) = match mode {
                Render::Body(cap) => {
                    let cut = cut_body(&s.body, *cap, s.policy);
                    let cut_note = (cut.kept < cut.total).then_some((cut.kept, cut.total));
                    let shown = number_lines(&cut.text);
                    let lines = cut.text.lines().count();
                    let header = self.header(s, lines, shown.len(), cut_note);
                    text.push_str(&header);
                    text.push('\n');
                    if !shown.is_empty() {
                        text.push_str(&shown);
                        text.push('\n');
                    }
                    (header, Some((lines, shown.len())), cut_note.is_some())
                }
                Render::LeftOutByUser => {
                    let header = format!("=== {} · {} · left out by the user", s.id, s.probe);
                    text.push_str(&header);
                    text.push('\n');
                    (header, None, false)
                }
                Render::LeftOutBudget => {
                    let header = format!("=== {} · {} · left out: the report budget of {} is used up", s.id, s.probe, human_bytes(self.budget.bytes() as u64));
                    text.push_str(&header);
                    text.push('\n');
                    (header, None, false)
                }
            };
            views.push(SectionView {
                id: s.id.clone(),
                probe: s.probe,
                params: s.params.clone(),
                status: s.status,
                included: s.included,
                lines: shown.map_or(0, |(l, _)| l),
                bytes: shown.map_or(0, |(_, b)| b),
                total_lines,
                cut,
                redactions: s.counts.redactions.clone(),
                pseudonyms: s.counts.pseudonyms.clone(),
                header,
                round: s.round,
            });
        }
        let symptom =
            self.symptom.as_ref().map(|(_, text, c)| SymptomView { text: text.clone(), redactions: c.redactions.clone(), pseudonyms: c.pseudonyms.clone() });
        Report { text, sections: views, symptom, budget: self.budget.bytes() }
    }
}

/// Die Rohdaten einer Sammlung: Ausgaben der Proben, bevor der Filter sie gesehen hat. Leben nur zwischen [`gather_plan`]
/// und [`Session::add`] (im Daemon: im Speicher des Verbindungs-Threads) und kommen nie in einen Bericht, nie in ein Log;
/// `Debug` zeigt nur Namen und Zahlen. Wer sammelt, braucht dafür die Sitzung nicht: der Daemon kann sie während der bis
/// zu 15 s frei lassen und sie erst für [`Session::add`] kurz sperren.
pub struct Gathered {
    profile: Option<Profile>,
    done: Vec<(Probe, ProbeOutput)>,
    elapsed: Duration,
    at: String,
}

impl Gathered {
    /// Wie viele Proben (und übersprungene) die Sammlung enthält.
    pub fn len(&self) -> usize {
        self.done.len()
    }

    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl fmt::Debug for Gathered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gathered")
            .field("probes", &self.done.iter().map(|(_, o)| format!("{} {}", o.probe, o.status.label())).collect::<Vec<_>>())
            .field("elapsed", &self.elapsed)
            .finish()
    }
}

/// Sammelt die angewählten Schritte eines Plans unter der Sperre des Prozesses (siehe Modulkopf). `room` ist
/// [`Session::room`]: ist es zu klein, passiert nichts.
pub fn gather_plan(
    ctx: &ProbeContext,
    cancel: &CancelFlag,
    plan: &Plan,
    params: &CollectParams,
    room: usize,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Gathered, CollectError> {
    gather_steps(CollectLock::global(), ctx, cancel, Some(plan.profile()), plan.selected_steps(), params, room, progress)
}

/// Sammelt einzelne Proben (Prüfbefehle der KI) unter der Sperre des Prozesses.
pub fn gather_probes(
    ctx: &ProbeContext,
    cancel: &CancelFlag,
    probes: &[Probe],
    params: &CollectParams,
    room: usize,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Gathered, CollectError> {
    gather_steps(CollectLock::global(), ctx, cancel, None, probes.iter().cloned().map(Step::Probe).collect(), params, room, progress)
}

#[allow(clippy::too_many_arguments)]
pub fn gather_steps(
    lock: &CollectLock,
    ctx: &ProbeContext,
    cancel: &CancelFlag,
    profile: Option<Profile>,
    steps: Vec<Step>,
    params: &CollectParams,
    room: usize,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Gathered, CollectError> {
    let guard = lock.try_acquire().ok_or(CollectError::Busy)?;
    if steps.len() + MAX_EXPANSION > room {
        return Err(CollectError::TooManySections);
    }
    gather_locked(&guard, ctx, cancel, profile, steps, params, progress)
}

/// Die Sammlung selbst; die Sperre hält der Aufrufer (der Beleg dafür ist der Guard).
fn gather_locked(
    _guard: &CollectGuard<'_>,
    ctx: &ProbeContext,
    cancel: &CancelFlag,
    profile: Option<Profile>,
    steps: Vec<Step>,
    params: &CollectParams,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Gathered, CollectError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CollectError::Cancelled);
    }
    let started = Instant::now();
    let mut collector = Collector::new(ctx, cancel);
    let mut total = steps.iter().filter(|s| matches!(s, Step::Probe(_))).count();
    let mut queue: VecDeque<Step> = steps.into();
    let mut done: Vec<(Probe, ProbeOutput)> = Vec::new();
    // Die letzte Liste von docker_ps dieser Sammlung: Quelle des Schritts ProblemContainers
    let mut listing: Option<usize> = None;
    while let Some(step) = queue.pop_front() {
        let probe = match step {
            Step::Probe(p) => p,
            Step::ProblemContainers => {
                // Ohne Liste (docker_ps lief nicht oder scheiterte) gibt es nichts zu holen
                if let Some(i) = listing.filter(|i| done[*i].1.status.has_data()) {
                    let (chosen, left_out) = problem_containers(&done[i].1.listed.containers);
                    if let Some(note) = problem_containers_note(chosen.len(), left_out) {
                        done[i].1.note(note);
                    }
                    let probes = container_probes(&chosen);
                    total += probes.len();
                    for p in probes.into_iter().rev() {
                        queue.push_front(Step::Probe(p));
                    }
                }
                continue;
            }
        };
        if cancel.load(Ordering::SeqCst) {
            return Err(CollectError::Cancelled);
        }
        let index = done.len() + 1;
        progress(&Progress::Start { index, total, probe: probe.name(), params: describe_params(&probe) });
        let output = run_probe(&mut collector, &probe, started, params);
        progress(&Progress::End {
            index,
            total,
            probe: output.probe,
            status: output.status.label(),
            message: output.status.message().map(str::to_string),
            bytes: output.byte_count(),
            duration: output.duration,
        });
        if cancel.load(Ordering::SeqCst) {
            return Err(CollectError::Cancelled);
        }
        if matches!(probe, Probe::DockerPs {}) {
            listing = Some(done.len());
        }
        done.push((probe, output));
    }
    Ok(Gathered { profile, done, elapsed: started.elapsed(), at: ctx.clock.format(ctx.clock.now()) })
}

/// Führt eine Probe aus, wenn ihre Frist samt Überlauf ins Restbudget passt; sonst `skipped`.
fn run_probe(collector: &mut Collector, probe: &Probe, started: Instant, params: &CollectParams) -> ProbeOutput {
    let spec = probe.spec();
    let opts = CollectOptions { budget: Duration::ZERO, network_confirmed: params.network_confirmed };
    // Netzverkehr ohne Bestätigung startet nichts und braucht kein Budget
    if spec.network_traffic && !params.network_confirmed {
        return collector.collect(probe, opts);
    }
    let remaining = params.total.saturating_sub(started.elapsed());
    let usable = remaining.saturating_sub(MAX_OVERRUN);
    if usable < MIN_PROBE_TIME.min(spec.deadline) {
        return ProbeOutput::without_data(
            spec.name,
            Status::Skipped(format!("the collection time budget of {} s is used up", params.total.as_secs_f64().round() as u64)),
        );
    }
    collector.collect(probe, CollectOptions { budget: usable, ..opts })
}

/// Die fehlgeschlagenen Units zur Auswahl für das Profil Unit: `failed_units` einmal, unter der Sperre der Sammlungen und im
/// Budget. Liefert den Suchbegriff und ob die Unit zum User-Manager gehört. Ein leeres Ergebnis heißt „keine“ oder „nicht
/// lesbar“; die Fehlermeldung zeigt der Aufrufer nicht, die Auswahl zeigt nur Units.
pub fn list_failed_units(ctx: &ProbeContext, cancel: &CancelFlag, total: Duration) -> Result<Vec<(UnitRef, bool)>, CollectError> {
    list_failed_units_with(CollectLock::global(), ctx, cancel, total)
}

pub fn list_failed_units_with(lock: &CollectLock, ctx: &ProbeContext, cancel: &CancelFlag, total: Duration) -> Result<Vec<(UnitRef, bool)>, CollectError> {
    let _guard = lock.try_acquire().ok_or(CollectError::Busy)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(CollectError::Cancelled);
    }
    let mut collector = Collector::new(ctx, cancel);
    let params = CollectParams { total, network_confirmed: false };
    let output = run_probe(&mut collector, &Probe::FailedUnits {}, Instant::now(), &params);
    if matches!(output.status, Status::Cancelled) {
        return Err(CollectError::Cancelled);
    }
    Ok(failed_unit_choices(&output))
}

/// Der laufende Boot und das Urteil über den davor, aus der Probe `boots` (Aufgabe 9: Hinweis nach einem unsauberen Ende).
/// Wie [`list_failed_units_with`] unter der Sperre der Sammlungen und im Budget; nichts davon kommt in einen Bericht. `None`,
/// wenn die Probe nichts liefern konnte (Programm fehlt, Frist, Journal nicht lesbar).
pub fn check_boots_with(lock: &CollectLock, ctx: &ProbeContext, cancel: &CancelFlag, total: Duration) -> Result<Option<ListedBoots>, CollectError> {
    let _guard = lock.try_acquire().ok_or(CollectError::Busy)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(CollectError::Cancelled);
    }
    let mut collector = Collector::new(ctx, cancel);
    let params = CollectParams { total, network_confirmed: false };
    let output = run_probe(&mut collector, &Probe::Boots {}, Instant::now(), &params);
    if matches!(output.status, Status::Cancelled) {
        return Err(CollectError::Cancelled);
    }
    Ok(output.listed.boots)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
