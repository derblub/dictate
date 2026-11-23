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

//! Automat des Diagnose-Panels (Port der Logik des früheren Awesome-Moduls `diagnose.lua`, PLAN-wayland.md 6e Punkt 1;
//! Zeilennummern der Vorlage unter „Stand W4“ dort): rein, ohne GTK und ohne Uhr. Jede Eingabe trägt `now`, jede Folge
//! ist ein [`Effect`], den der Hauptthread von `dictate-ui` ausführt (Fenster zeigen, `diag_*` an den Daemon, Auflegen).
//!
//! Ansichten `Loading → Profiles → Items → Progress → Preview → Send → Answer` und `Error` wie in Lua. `gen` steigt bei
//! jedem Öffnen und Schließen, jeder Aufruf trägt dazu eine eigene Nummer (`seq`): eine Antwort mit älterem `gen` oder
//! ohne wartenden Aufruf dieser Nummer wird verworfen (Befund 10). Die Aufrufe laufen im Binary der Reihe nach (ein
//! `diag_clear` trifft nie eine neuere Diagnose); ein laufendes Sammeln, Prüfen oder Senden bricht [`Effect::Hangup`] ab
//! (die Verbindung wird aufgelegt, der Daemon bricht dann ab, wie unter Awesome beim Beenden des CLI).
//!
//! Totzeit und alte Tasten wie beim Panel der Textaktionen ([`crate::ui::gate`]); Leerlauf schließt außer beim Sammeln
//! und Senden.
//!
//! Symptom des Users (Entscheidung Daniel, 21.11.): Feld „What is wrong?“ in der Vorschau. Tab oder Klick führt hinein,
//! dann tippt der User in ein echtes Eingabefeld (GTK); Enter oder Verlassen mit Tab übernimmt es (`diag_preview` mit
//! `symptom`, die neue Vorschau zeigt den gefilterten Text), Esc verwirft die Änderung. Nach einem neuen Sammeln wird ein
//! übernommenes Symptom selbst wieder gesetzt (die neue Sitzung im Daemon kennt es nicht).
//!
//! Inhalte (Bericht, Fortschritt mit Parametern, Antwort, Klartext der Platzhalter, Symptom) leben nur hier im Speicher,
//! werden beim Schließen überschrieben und nie geloggt; Kennungen der Bedienelemente nennen nur Indizes (`unit:3`,
//! `item:4`, `check:2`, `ev:1:2`), Abschnitte ihre Kennung `S<n>`.

use crate::actions::store::{wipe, SecretText};
use crate::protocol::Request;
use crate::ui::gate::Gate;
use crate::ui::panel::{plural, Item, ItemKind, Mods, Scroll, DAEMON_DOWN, SHOW_DELAY};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

/// Arten der Platzhalter (`pseudo::Category::prefix`)
pub const PREFIXES: [&str; 8] = ["user", "host", "ip", "mac", "ssid", "path", "email", "id"];
/// Fristen je Aufruf (`TIMEOUTS` in diagnose.lua): Sammeln, Prüfen und die Units laufen unter der Sperre der Sammlungen
/// (Budget 15 s plus Warten), Senden bis zur größten Zeitgrenze der Diagnose plus Reserve
pub const COLLECT_TIMEOUT: Duration = Duration::from_secs(40);
pub const SEND_TIMEOUT: Duration = Duration::from_secs(330);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Höchstlänge des Symptoms, falls die Vorschau keine nennt (`report::MAX_SYMPTOM_CHARS` im Daemon)
pub const MAX_SYMPTOM_CHARS: usize = 300;
/// Lebensdauer der Sitzung, wenn der Daemon `expires_in_s` nicht nennt (wie `diagnose::service::SESSION_TTL`)
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Closed,
    /// Profile werden geladen (erst nach [`SHOW_DELAY`] sichtbar)
    Loading,
    Profiles,
    Items,
    Progress,
    Preview,
    Send,
    Answer,
    Error,
}

/// Ein Aufruf an den Daemon
#[derive(Clone, PartialEq)]
pub enum Call {
    Plan {
        profile: Option<String>,
        unit: Option<String>,
        container: Option<String>,
    },
    Units,
    Collect {
        profile: String,
        unit: Option<String>,
        container: Option<String>,
        skip: Vec<usize>,
    },
    Preview {
        id: String,
        deselected: Option<Vec<String>>,
        symptom: Option<SecretText>,
    },
    Check {
        id: String,
        checks: Vec<Value>,
        network: bool,
    },
    Send {
        id: String,
        preview: u64,
    },
    /// Bericht holen und in CLIPBOARD legen (macht das Binary; zurück kommt nur `{"chars":n}`)
    Copy {
        id: String,
    },
    /// `Cancel` und `Clear` führt das Binary immer aus, auch wenn ihr `gen` schon veraltet ist (sie kommen beim Schließen)
    Cancel {
        id: String,
    },
    Clear,
}

impl Call {
    /// Name fürs Log (keine Parameter)
    pub fn name(&self) -> &'static str {
        match self {
            Call::Plan { profile: None, .. } => "plan",
            Call::Plan { .. } => "plan_profile",
            Call::Units => "units",
            Call::Collect { .. } => "collect",
            Call::Preview { .. } => "preview",
            Call::Check { .. } => "check",
            Call::Send { .. } => "send",
            Call::Copy { .. } => "copy",
            Call::Cancel { .. } => "cancel",
            Call::Clear => "clear",
        }
    }

    pub fn timeout(&self) -> Duration {
        match self {
            Call::Units | Call::Collect { .. } | Call::Check { .. } => COLLECT_TIMEOUT,
            Call::Send { .. } => SEND_TIMEOUT,
            _ => CALL_TIMEOUT,
        }
    }

    /// Die Anfrage an den Daemon (`Copy` wird `diag_copy`; die Zwischenablage setzt das Binary)
    pub fn request(&self) -> Request {
        match self.clone() {
            Call::Plan { profile, unit, container } => Request::DiagPlan { profile, unit, container },
            Call::Units => Request::DiagUnits,
            Call::Collect { profile, unit, container, skip } => Request::DiagCollect { profile, unit, container, skip },
            Call::Preview { id, deselected, symptom } => Request::DiagPreview { id, deselected, symptom },
            Call::Check { id, checks, network } => Request::DiagCheck { id, checks, network },
            Call::Send { id, preview } => Request::DiagSend { id, preview },
            Call::Copy { id } => Request::DiagCopy { id },
            Call::Cancel { id } => Request::DiagCancel { id },
            Call::Clear => Request::DiagClear,
        }
    }
}

/// Fehler eines Aufrufs (`Debug` nur die Art, die Meldung kann Namen tragen)
#[derive(Clone, PartialEq, Eq)]
pub enum CallErr {
    DaemonDown,
    Timeout,
    /// Der Daemon hat abgebrochen (`cancelled`)
    Cancelled,
    /// Meldung des Daemons oder der Verbindung (nie loggen: kann Namen tragen)
    Failed(String),
}

impl CallErr {
    /// Der Daemon kennt die Sitzung nicht (mehr)
    pub fn unknown_session(&self) -> bool {
        matches!(self, CallErr::Failed(m) if m.to_lowercase().contains(UNKNOWN_SESSION_MARK))
    }

    /// Aus einer Antwortzeile mit `ok: false`
    pub fn from_reply(v: &Value) -> CallErr {
        if v["cancelled"] == true {
            return CallErr::Cancelled;
        }
        CallErr::Failed(v["error"].as_str().unwrap_or("command failed").to_string())
    }

    fn message(&self) -> String {
        match self {
            CallErr::DaemonDown => DAEMON_DOWN.to_string(),
            CallErr::Timeout => "dictate did not answer in time".to_string(),
            CallErr::Cancelled => "Cancelled".to_string(),
            CallErr::Failed(m) => m.clone(),
        }
    }

    /// Art fürs Log (ohne Text)
    pub fn kind(&self) -> &'static str {
        match self {
            CallErr::DaemonDown => "daemon down",
            CallErr::Timeout => "timeout",
            CallErr::Cancelled => "cancelled",
            CallErr::Failed(_) => "refused",
        }
    }
}

/// Folgen für den Hauptthread
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Call {
        gen: u64,
        seq: u64,
        call: Call,
    },
    /// Laufenden Aufruf `seq` auflegen (der Daemon bricht ab)
    Hangup {
        seq: u64,
    },
    Show,
    Hide,
    Render,
    /// Nur die Hervorhebung neu setzen (Hover baut nichts neu auf)
    Highlight,
    Scroll(Scroll),
    /// Eingabefeld „What is wrong?“ fokussieren (`true`) bzw. verlassen
    Field(bool),
    /// Hinweis im Overlay (nur Zahlen)
    Notice(String),
    /// Aus der Palette geöffnet: die Diagnose antwortet und ist an (`pal_chosen`, einmal je Öffnung; wie `on_success`
    /// in Lua nach der Liste der Profile, Lesart L34)
    Chosen,
}

/// Wohin die Vorschau nach dem Zeichnen springt (Abschnitt, optional die Zeile mit dieser Nummer)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollTo {
    pub section: String,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub name: String,
    pub label: String,
    pub description: String,
    pub needs_unit: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UnitInfo {
    pub unit: String,
    pub user: bool,
    /// Suchbegriff aus der Palette, der keine fehlgeschlagene Unit trifft (der Daemon löst ihn auf)
    pub searched: bool,
}

#[derive(Clone, PartialEq)]
pub struct PlanItem {
    pub index: usize,
    pub label: String,
    pub probe: Option<Value>,
    pub selected: bool,
    pub network: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProgressRow {
    pub probe: String,
    pub params: String,
    /// `None` = läuft
    pub status: Option<String>,
    pub bytes: u64,
    pub ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressInfo {
    pub total: usize,
    pub rows: BTreeMap<usize, ProgressRow>,
    pub lines: u64,
    pub started: Instant,
}

impl ProgressInfo {
    pub fn done(&self) -> usize {
        self.rows.values().filter(|r| r.status.is_some()).count()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetInfo {
    pub host: String,
    pub model: String,
    pub cloud: bool,
    pub ready: bool,
    pub problem: Option<String>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SectionInfo {
    pub id: String,
    pub included: bool,
    pub header: String,
    pub round: u32,
    pub placeholders: HashMap<String, u64>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SymptomInfo {
    pub text: String,
    pub placeholders: HashMap<String, u64>,
}

/// Vorschau des Berichts (Antwort auf `diag_collect`, `diag_check`, `diag_preview`)
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PreviewInfo {
    pub id: String,
    pub preview: u64,
    pub rounds: u32,
    pub max_rounds: u32,
    pub text: String,
    pub bytes: u64,
    pub budget: u64,
    pub sections: Vec<SectionInfo>,
    pub redactions: u64,
    pub placeholders: u64,
    pub pseudonymize: bool,
    pub target: TargetInfo,
    pub symptom: Option<SymptomInfo>,
    pub max_symptom_chars: usize,
    /// Restzeit der Sitzung im Daemon (`expires_in_s`)
    pub expires_in: Option<Duration>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn take_str(v: &mut Value) -> String {
    match v.take() {
        Value::String(s) => s,
        _ => String::new(),
    }
}

fn counts(v: &Value) -> HashMap<String, u64> {
    v.as_object().map(|m| m.iter().filter_map(|(k, n)| Some((k.clone(), n.as_u64()?))).collect()).unwrap_or_default()
}

impl PreviewInfo {
    /// Aus der Antwort; Texte werden herausgenommen (der Aufrufer überschreibt den Rest). `None` ohne `text`
    pub fn from_json(v: &mut Value) -> Option<PreviewInfo> {
        if !v["text"].is_string() {
            return None;
        }
        let sections = v["sections"]
            .as_array_mut()
            .map(|a| {
                a.iter_mut()
                    .filter(|x| x["id"].is_string())
                    .map(|x| SectionInfo {
                        id: s(&x["id"]),
                        included: x["included"] != false,
                        header: take_str(&mut x["header"]),
                        round: round_of(x),
                        placeholders: counts(&x["placeholders"]),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let symptom =
            v["symptom"].is_object().then(|| SymptomInfo { placeholders: counts(&v["symptom"]["placeholders"]), text: take_str(&mut v["symptom"]["text"]) });
        let t = &v["target"];
        let target = TargetInfo {
            host: s(&t["host"]),
            model: s(&t["model"]),
            cloud: t["cloud"] == true,
            ready: t["ready"] != false,
            problem: t["problem"].as_str().map(str::to_string),
        };
        Some(PreviewInfo {
            id: match &v["id"] {
                Value::String(i) => i.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            },
            preview: v["preview"].as_u64().unwrap_or(0),
            rounds: v["rounds"].as_u64().unwrap_or(1) as u32,
            max_rounds: v["max_rounds"].as_u64().unwrap_or(3) as u32,
            bytes: v["bytes"].as_u64().unwrap_or(0),
            budget: v["budget"].as_u64().unwrap_or(0),
            redactions: v["redactions"].as_u64().unwrap_or(0),
            placeholders: v["placeholders"].as_u64().unwrap_or(0),
            pseudonymize: v["pseudonymize"] != false,
            max_symptom_chars: v["max_symptom_chars"].as_u64().map_or(MAX_SYMPTOM_CHARS, |n| n as usize),
            expires_in: v["expires_in_s"].as_u64().map(Duration::from_secs),
            target,
            symptom,
            sections,
            text: take_str(&mut v["text"]),
        })
    }

    fn wipe(&mut self) {
        wipe(&mut self.text);
        for s in &mut self.sections {
            wipe(&mut s.header);
        }
        if let Some(sy) = &mut self.symptom {
            wipe(&mut sy.text);
        }
    }

    pub fn included(&self) -> usize {
        self.sections.iter().filter(|s| s.included).count()
    }

    fn section(&self, id: &str) -> Option<&SectionInfo> {
        self.sections.iter().find(|s| s.id == id)
    }
}

/// Sammlung eines Abschnitts: `round`, sonst aus dem Kopf (` · round N`), sonst 1 (`round_of` 448)
fn round_of(v: &Value) -> u32 {
    if let Some(r) = v["round"].as_u64() {
        return r as u32;
    }
    let h = v["header"].as_str().unwrap_or("");
    h.find(" · round ")
        .and_then(|i| {
            let rest = &h[i + " · round ".len()..];
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            rest[..end].parse().ok()
        })
        .unwrap_or(1)
}

/// Eine Spanne der Rückübersetzung (Bytes `[start, end)` im Text, genau der Platzhalter, sein Klartext)
#[derive(Clone, PartialEq, Eq)]
pub struct PhSpan {
    pub start: usize,
    pub end: usize,
    pub placeholder: String,
    pub value: String,
}

/// Beleg eines Befunds (Zeilennummern wie im Bericht vor `| `)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub section: String,
    pub from: u64,
    pub to: u64,
}

impl Evidence {
    /// „S2 · lines 3–5“ (`evidence_text` 913)
    pub fn text(&self) -> String {
        if self.to != self.from {
            format!("{} · lines {}–{}", self.section, self.from, self.to)
        } else {
            format!("{} · line {}", self.section, self.from)
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Finding {
    pub cause: String,
    /// `high`, `medium` oder `low`
    pub confidence: String,
    pub evidence: Vec<Evidence>,
    pub spans: Vec<PhSpan>,
}

#[derive(Clone, PartialEq)]
pub struct CheckInfo {
    pub probe: Value,
    pub label: String,
    pub reason: String,
    pub network: bool,
}

#[derive(Clone, PartialEq)]
pub struct Answer {
    pub findings: Vec<Finding>,
    pub checks: Vec<CheckInfo>,
    pub notes: String,
    pub note_spans: Vec<PhSpan>,
}

impl Answer {
    pub fn has_values(&self) -> bool {
        !self.note_spans.is_empty() || self.findings.iter().any(|f| !f.spans.is_empty())
    }

    fn wipe(&mut self) {
        for f in &mut self.findings {
            wipe(&mut f.cause);
            for sp in &mut f.spans {
                wipe(&mut sp.value);
            }
        }
        for c in &mut self.checks {
            wipe(&mut c.label);
            wipe(&mut c.reason);
        }
        wipe(&mut self.notes);
        for sp in &mut self.note_spans {
            wipe(&mut sp.value);
        }
    }
}

/// Ist das ein Platzhalter (`^[a-z]+-[1-9][0-9]*$`)?
fn placeholder_like(p: &str) -> bool {
    let Some((kind, n)) = p.split_once('-') else { return false };
    !kind.is_empty()
        && kind.bytes().all(|b| b.is_ascii_lowercase())
        && n.starts_with(|c: char| ('1'..='9').contains(&c))
        && n.bytes().all(|b| b.is_ascii_digit())
}

/// Spannen zu einem Text prüfen (`valid_spans` 947): Bytes im Text, genau der genannte Platzhalter, aufsteigend, ohne
/// Überlappung, mit einem Wert; alles andere fällt weg (nie wird etwas erfunden). Die Werte werden aus `list`
/// herausgenommen.
pub fn valid_spans(text: &str, list: &mut Value) -> Vec<PhSpan> {
    let mut out = Vec::new();
    let mut last = 0usize;
    let Some(a) = list.as_array_mut() else { return out };
    for sp in a {
        let (Some(st), Some(en)) = (sp["start"].as_u64(), sp["end"].as_u64()) else { continue };
        let (st, en) = (st as usize, en as usize);
        let ph = s(&sp["placeholder"]);
        let ok = st >= last && en > st && en <= text.len() && placeholder_like(&ph) && text.get(st..en) == Some(ph.as_str());
        if !ok || sp["value"].as_str().is_none_or(str::is_empty) {
            continue;
        }
        out.push(PhSpan { start: st, end: en, placeholder: ph, value: take_str(&mut sp["value"]) });
        last = en;
    }
    out
}

/// Parameter einer Probe als „since=1h unit=docker.service“ (Schlüssel sortiert, ohne `probe`; `params_of` 235)
pub fn params_of(probe: &Value) -> String {
    let Some(m) = probe.as_object() else { return String::new() };
    let mut keys: Vec<&String> = m.keys().filter(|k| *k != "probe").collect();
    keys.sort();
    keys.into_iter()
        .filter_map(|k| match &m[k] {
            Value::String(v) => Some(format!("{k}={v}")),
            Value::Number(n) => Some(format!("{k}={n}")),
            Value::Bool(b) => Some(format!("{k}={b}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Antwort nach dem Schema prüfen (`parse_answer` 1735); unbrauchbare Teile fallen weg. `known`: Abschnitte dieses
/// Berichts. Texte und Klartext werden aus `a` und `restore` herausgenommen
pub fn parse_answer(a: &mut Value, restore: &mut Value, known: &HashSet<String>) -> Option<Answer> {
    if !a.is_object() {
        return None;
    }
    let mut none = Value::Null;
    let notes = take_str(&mut a["notes"]);
    let note_spans = valid_spans(&notes, restore.get_mut("notes").unwrap_or(&mut none));
    let mut findings = Vec::new();
    if let Some(list) = a["findings"].as_array_mut() {
        for (fi, f) in list.iter_mut().enumerate() {
            if !f["cause"].is_string() {
                continue;
            }
            let evidence = f["evidence"]
                .as_array()
                .map(|ev| {
                    ev.iter()
                        .filter(|e| e["section"].as_str().is_some_and(|sec| known.contains(sec)))
                        .map(|e| {
                            let from = e["from"].as_f64().unwrap_or(1.0).floor().max(1.0) as u64;
                            let to = (e["to"].as_f64().map_or(from, |t| t.floor() as u64)).max(from);
                            Evidence { section: s(&e["section"]), from, to }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let cause = take_str(&mut f["cause"]);
            let mut none = Value::Null;
            let spans = valid_spans(&cause, restore.get_mut("findings").and_then(|f| f.get_mut(fi)).unwrap_or(&mut none));
            let confidence = match f["confidence"].as_str() {
                Some(c @ ("high" | "medium" | "low")) => c.to_string(),
                _ => "low".to_string(),
            };
            findings.push(Finding { cause, confidence, evidence, spans });
        }
    }
    let mut checks = Vec::new();
    if let Some(list) = a["checks"].as_array_mut() {
        for c in list {
            let Some(name) = c["probe"]["probe"].as_str().map(str::to_string) else { continue };
            let probe = c["probe"].take();
            let label = match c["label"].as_str() {
                Some(l) => l.to_string(),
                None => {
                    let p = params_of(&probe);
                    if p.is_empty() {
                        name
                    } else {
                        format!("{name} · {p}")
                    }
                }
            };
            checks.push(CheckInfo { label, reason: take_str(&mut c["reason"]), network: c["network"] == true, probe });
        }
    }
    Some(Answer { findings, checks, notes, note_spans })
}

/// Bericht in Kopf und Abschnitte zerlegen (`split_report` 361). Eine Zeile `resolved:` gehört nur dann zum Kopf, wenn
/// der Kopf des Abschnitts laut Daemon (`sections[].header`) eine hat (eine Ausgabe, die mit `resolved: ` beginnt, ist
/// Inhalt)
#[derive(Clone, PartialEq, Eq)]
pub struct ReportSection {
    pub id: String,
    pub head: Vec<String>,
    pub body: Vec<String>,
}

pub fn split_report(text: &str, sections: &[SectionInfo]) -> (Vec<String>, Vec<ReportSection>) {
    let two: HashSet<&str> = sections.iter().filter(|v| v.header.contains("\nresolved: ")).map(|v| v.id.as_str()).collect();
    let mut pre = Vec::new();
    let mut secs: Vec<ReportSection> = Vec::new();
    for line in text.split('\n') {
        if let Some(id) = section_start(line) {
            secs.push(ReportSection { id: id.to_string(), head: vec![line.to_string()], body: vec![] });
        } else if let Some(cur) = secs.last_mut() {
            if two.contains(cur.id.as_str()) && cur.body.is_empty() && cur.head.len() == 1 && line.starts_with("resolved: ") {
                cur.head.push(line.to_string());
            } else {
                cur.body.push(line.to_string());
            }
        } else {
            pre.push(line.to_string());
        }
    }
    for s in &mut secs {
        while s.body.last().is_some_and(String::is_empty) {
            s.body.pop();
        }
    }
    while pre.last().is_some_and(String::is_empty) {
        pre.pop();
    }
    (pre, secs)
}

/// `=== S<n> · …` → `S<n>`
fn section_start(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("=== S")?;
    let n = rest.find(|c: char| !c.is_ascii_digit())?;
    (n > 0 && rest[n..].starts_with(" · ")).then(|| &line[4..4 + 1 + n])
}

/// Zeilennummer vor `| ` (von Rust): `12| text` → (12, "text")
pub fn line_number(line: &str) -> Option<(u64, &str)> {
    let bar = line.find('|')?;
    let n = line[..bar].parse().ok()?;
    Some((n, line[bar + 1..].strip_prefix(' ').unwrap_or(&line[bar + 1..])))
}

/// Art eines Stücks einer Berichtszeile
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Plain,
    /// `[REDACTED:art]`
    Redacted,
    /// `<art>-N` mit N höchstens der Zahl der Platzhalter dieser Art
    Placeholder,
}

fn is_alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// Erste Schwärzung `[REDACTED:art]` ab `from`: (Anfang, Ende)
fn find_redacted(line: &str, from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while let Some(p) = line[i..].find("[REDACTED:") {
        let st = i + p;
        let rest = &line.as_bytes()[st + 10..];
        let n = rest.iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
        if n > 0 && rest.get(n) == Some(&b']') {
            return Some((st, st + 10 + n + 1));
        }
        i = st + 1;
    }
    None
}

/// Erster Platzhalter `<präfix>-N` (Wortgrenzen wie `%f[%w]` in Lua) ab `from` mit N ≤ Grenze der Art
fn find_placeholder(line: &str, from: usize, limits: &HashMap<String, u64>) -> Option<(usize, usize)> {
    let b = line.as_bytes();
    let mut best: Option<(usize, usize)> = None;
    for p in PREFIXES {
        let Some(&lim) = limits.get(p).filter(|l| **l > 0) else { continue };
        let pat = format!("{p}-");
        let mut i = from;
        while let Some(off) = line[i..].find(&pat) {
            let st = i + off;
            let ds = st + pat.len();
            let de = ds + b[ds..].iter().take_while(|c| c.is_ascii_digit()).count();
            let left_ok = st == 0 || !is_alnum(b[st - 1]);
            let right_ok = de == b.len() || !is_alnum(b[de]);
            if de > ds && left_ok && right_ok && line[ds..de].parse::<u64>().is_ok_and(|n| n >= 1 && n <= lim) {
                if best.is_none_or(|(bs, _)| st < bs) {
                    best = Some((st, de));
                }
                break;
            }
            i = st + 1;
        }
    }
    best
}

/// Schwärzungen und Platzhalter einer Zeile markieren (`mark_line` 395). Platzhalter sind nur an der Form erkennbar;
/// Formen aus dem Rohtext (`user-1000.slice`) bleiben stehen, darum gilt `<art>-N` nur bis zur Zahl der Platzhalter dieser
/// Art im Bericht.
pub fn mark_line<'a>(line: &'a str, limits: &HashMap<String, u64>) -> Vec<(Mark, &'a str)> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let r = find_redacted(line, pos);
        let p = find_placeholder(line, pos, limits);
        let hit = match (r, p) {
            (Some(r), Some(p)) if p.0 < r.0 => Some((Mark::Placeholder, p)),
            (Some(r), _) => Some((Mark::Redacted, r)),
            (None, Some(p)) => Some((Mark::Placeholder, p)),
            (None, None) => None,
        };
        let Some((kind, (st, en))) = hit else {
            if pos < line.len() {
                out.push((Mark::Plain, &line[pos..]));
            }
            return out;
        };
        if st > pos {
            out.push((Mark::Plain, &line[pos..st]));
        }
        out.push((kind, &line[st..en]));
        pos = en;
    }
}

/// Grenzen je Art für [`mark_line`]: Summe über alle Abschnitte und das Symptom
pub fn limits(pv: &PreviewInfo) -> HashMap<String, u64> {
    let mut m: HashMap<String, u64> = HashMap::new();
    let all = pv.sections.iter().map(|s| &s.placeholders).chain(pv.symptom.iter().map(|s| &s.placeholders));
    for c in all {
        for (k, n) in c {
            *m.entry(k.clone()).or_default() += n;
        }
    }
    m
}

/// Passt der Suchbegriff auf Units der Liste (`unit_matches` 1407)? Wie der Daemon: voller Name, mit `.service`, sonst
/// Teil des Namens (ohne Groß/klein). Einer = eindeutig, mehrere = der User wählt, keiner = der Begriff geht unverändert
/// an den Daemon.
pub fn unit_matches(units: &[UnitInfo], term: &str) -> Vec<String> {
    let lower = term.to_lowercase();
    let has_suffix = lower.rsplit_once('.').is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic()));
    let with_service = if has_suffix { lower.clone() } else { format!("{lower}.service") };
    if let Some(u) = units.iter().find(|u| {
        let n = u.unit.to_lowercase();
        n == lower || n == with_service
    }) {
        return vec![u.unit.clone()];
    }
    units.iter().filter(|u| u.unit.to_lowercase().contains(&lower)).map(|u| u.unit.clone()).collect()
}

/// Vorbelegung aus der Befehlspalette (Absicht `diagnose`, W5): Profil und ein Suchbegriff für eine Unit oder einen
/// Container (`preset_of` 1913). Der Begriff wird nie ausgeführt, nur abgeglichen bzw. an den Daemon gegeben.
#[derive(Clone, PartialEq, Eq)]
pub struct Preset {
    pub profile: String,
    pub unit: Option<String>,
    pub container: Option<String>,
}

pub fn preset_of(profile: Option<&str>, unit: Option<&str>, container: Option<&str>) -> Option<Preset> {
    let term = |v: Option<&str>| v.filter(|t| t.chars().any(|c| !c.is_whitespace())).map(str::to_string);
    let (profile, mut unit, mut container) = (term(profile), term(unit), term(container));
    if unit.is_some() && container.is_some() {
        // nur einer von beiden; welcher gemeint ist, weiß nur der User
        (unit, container) = (None, None);
    }
    if unit.is_some() && profile.as_deref().is_some_and(|p| p != "unit") {
        unit = None;
    }
    if container.is_some() && profile.as_deref().is_some_and(|p| p != "docker") {
        container = None;
    }
    let profile = profile.or_else(|| unit.as_ref().map(|_| "unit".to_string())).or_else(|| container.as_ref().map(|_| "docker".to_string()))?;
    Some(Preset { profile, unit, container })
}

/// Fehler mit Anleitung (`show_error` 1292)
#[derive(Clone, PartialEq, Eq)]
pub struct DiagErr {
    pub title: String,
    pub body: Option<String>,
    pub neutral: bool,
    /// ⌫ führt in diese Ansicht
    pub back: Option<View>,
    /// Knopf „Try again“ bzw. „Start again“: zurück zu den Profilen
    pub retry: Option<&'static str>,
    /// Copy report möglich (Senden gescheitert)
    pub copy: bool,
    /// Der Daemon kennt die Sitzung nicht mehr (abgelaufen, Neustart des Daemons)
    pub expired: bool,
}

/// Kennzeichen der Meldung `diagnose::service::UNKNOWN_SESSION` („Unknown or expired diagnosis, collect again“)
const UNKNOWN_SESSION_MARK: &str = "unknown or expired";

pub fn diag_error(msg: &str, back: Option<View>, copy: bool) -> DiagErr {
    let mut e = DiagErr { title: msg.to_string(), body: None, neutral: false, back, retry: None, copy, expired: false };
    if msg == DAEMON_DOWN {
        e.body = Some("Start it with systemctl --user start dictate, then try again.".into());
        e.retry = Some("Try again");
    } else if msg.starts_with("Diagnosis is off") {
        e.body = Some("Turn it on in Settings › Diagnose, then press Mod+Shift+I again.".into());
        e.neutral = true;
        e.back = None;
    } else if msg.to_lowercase().contains(UNKNOWN_SESSION_MARK) {
        e.title = "The diagnosis expired".into();
        e.body = Some("A diagnosis lives for 30 minutes. Collect again.".into());
        e.retry = Some("Start again");
        e.back = None;
        e.copy = false;
        e.expired = true;
    } else if msg.contains("AI server changed") || msg.contains("changed since the preview") {
        e.body = Some("Check the report again before sending.".into());
        e.back = Some(View::Preview);
    }
    e
}

/// Wartender Aufruf: was mit seiner Antwort geschieht
#[derive(Clone, PartialEq, Eq)]
enum Pending {
    Profiles,
    Plan {
        profile: String,
    },
    Units {
        profile: String,
    },
    Collect,
    Check,
    Toggle,
    Symptom {
        text: String,
    },
    Copy,
    Send {
        preview: u64,
    },
    /// `diag_cancel`, `diag_clear`: Antwort egal
    Fire,
}

/// Eingabefeld „What is wrong?“ in der Vorschau
#[derive(Default)]
pub struct Field {
    /// Inhalt des Eingabefelds (wie getippt)
    pub text: String,
    /// Zuletzt übernommen (im Daemon gesetzt)
    pub applied: String,
    pub editing: bool,
}

pub struct DiagPanel {
    pub view: View,
    gen: u64,
    seq: u64,
    pending: HashMap<u64, Pending>,
    /// Laufendes Sammeln, Prüfen oder Senden (für Stop)
    job: Option<u64>,
    pub gate: Gate,
    pub visible: bool,
    opened_at: Option<Instant>,
    /// Ablauf der Sitzung im Daemon (ab der Vorschau, Restzeit aus `expires_in_s`): danach schließt das Panel und
    /// löscht Bericht, Antwort und Klartext auch hier, selbst wenn der User die ganze Zeit tippt (Durchsicht W4, F2)
    expires_at: Option<Instant>,
    /// Hervorgehobenes Element (Hover, ↑/↓, Tab)
    pub active: Option<String>,
    preset: Option<Preset>,
    /// Aus der Palette geöffnet und die Profile noch nicht geladen: [`Effect::Chosen`] steht aus
    chosen_pending: bool,
    pub profiles: Vec<ProfileInfo>,
    pub profile: Option<String>,
    /// `None` = wird gesucht (Profil Unit)
    pub units: Option<Vec<UnitInfo>>,
    pub unit: Option<String>,
    pub container: Option<String>,
    pub plan: Option<Vec<PlanItem>>,
    pub plan_loading: bool,
    items_active: Option<String>,
    /// Kennung der Diagnose im Daemon
    pub id: Option<String>,
    pub progress: Option<ProgressInfo>,
    pub check_round: bool,
    pub preview: Option<PreviewInfo>,
    preview_busy: bool,
    /// Gewählter Abschnitt der Vorschau
    pub sec: Option<String>,
    /// Neu aus der letzten Prüfrunde
    pub new_ids: Vec<String>,
    /// Beleg, dessen Zeilen die Vorschau markiert
    pub evidence: Option<Evidence>,
    pub scroll_to: Option<ScrollTo>,
    pub answer: Option<Answer>,
    answer_preview: Option<u64>,
    pub answer_model: Option<String>,
    /// Gewählte Prüfbefehle (Index ab 1)
    pub picked: BTreeSet<usize>,
    answer_active: Option<String>,
    /// Klartext statt Platzhalter in der Antwort
    pub plain: bool,
    pub send_started: Option<Instant>,
    pub err: Option<DiagErr>,
    /// Zeile im Panel (`(text, Fehler)`)
    pub notice: Option<(String, bool)>,
    copy_busy: bool,
    pub field: Field,
    /// Zähler für Tests und das Debug-Log (keine Inhalte)
    pub stats: Stats,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stats {
    pub collects: u64,
    pub checks: u64,
    pub sends: u64,
    pub copies: u64,
    pub hangups: u64,
    pub jumps: u64,
    pub previews: u64,
}

impl Default for DiagPanel {
    fn default() -> Self {
        DiagPanel::new()
    }
}

impl DiagPanel {
    pub fn new() -> DiagPanel {
        DiagPanel {
            view: View::Closed,
            gen: 0,
            seq: 0,
            pending: HashMap::new(),
            job: None,
            gate: Gate::new(),
            visible: false,
            opened_at: None,
            expires_at: None,
            active: None,
            preset: None,
            chosen_pending: false,
            profiles: vec![],
            profile: None,
            units: None,
            unit: None,
            container: None,
            plan: None,
            plan_loading: false,
            items_active: None,
            id: None,
            progress: None,
            check_round: false,
            preview: None,
            preview_busy: false,
            sec: None,
            new_ids: vec![],
            evidence: None,
            scroll_to: None,
            answer: None,
            answer_preview: None,
            answer_model: None,
            picked: BTreeSet::new(),
            answer_active: None,
            plain: false,
            send_started: None,
            err: None,
            notice: None,
            copy_busy: false,
            field: Field::default(),
            stats: Stats::default(),
        }
    }

    pub fn gen(&self) -> u64 {
        self.gen
    }

    pub fn is_open(&self) -> bool {
        self.view != View::Closed
    }

    /// Wartet ein Aufruf (Tests, Debug-Log)
    pub fn calls_pending(&self) -> usize {
        self.pending.values().filter(|p| **p != Pending::Fire).count()
    }

    // ---- Öffnen und Schließen ------------------------------------------------------------------------------------

    /// Mod+Shift+I: offen schließt, zu öffnet die Auswahl der Profile
    pub fn toggle(&mut self, now: Instant) -> Vec<Effect> {
        if self.is_open() {
            return self.close();
        }
        self.open(None, now)
    }

    /// Öffnen, optional mit Vorbelegung (Palette, W5)
    pub fn open(&mut self, preset: Option<Preset>, now: Instant) -> Vec<Effect> {
        let mut out = self.close();
        self.gen += 1;
        self.view = View::Loading;
        self.opened_at = Some(now);
        self.gate.open(now);
        self.preset = preset;
        out.extend(self.start_profiles());
        out
    }

    /// Aus der Befehlspalette öffnen (Absicht `diagnose`, W5 Block D): wie [`DiagPanel::open`], dazu einmal
    /// [`Effect::Chosen`], sobald die Profile da sind
    pub fn open_from_palette(&mut self, preset: Option<Preset>, now: Instant) -> Vec<Effect> {
        let out = self.open(preset, now);
        self.chosen_pending = true;
        out
    }

    /// Schließen (Esc, ✕, Fokusverlust, Kürzel, Sperre, Leerlauf): laufenden Aufruf auflegen, Diagnose im Daemon
    /// abbrechen und löschen, alle Inhalte überschreiben
    pub fn close(&mut self) -> Vec<Effect> {
        if !self.is_open() {
            return vec![];
        }
        // jeden laufenden Aufruf auflegen, nicht nur Sammeln, Prüfen und Senden: die Aufrufe laufen der Reihe nach, ein
        // laufendes `diag_units` (bis 40 s) hielte sonst `diag_cancel` und `diag_clear` auf (Block B Punkt 0)
        let mut out = self.hang_up_all();
        self.job = None;
        self.wipe_pending();
        if let Some(id) = self.id.clone() {
            out.push(self.call(Call::Cancel { id }, Pending::Fire));
        }
        out.push(self.call(Call::Clear, Pending::Fire));
        // Antworten dieser Öffnung zählen ab jetzt nicht mehr
        self.gen += 1;
        let was_visible = self.visible;
        self.wipe_all();
        self.chosen_pending = false;
        if was_visible {
            out.push(Effect::Hide);
        }
        out
    }

    /// Jeden wartenden Aufruf außer `diag_cancel` und `diag_clear` auflegen (aufsteigend nach `seq`)
    fn hang_up_all(&mut self) -> Vec<Effect> {
        let mut seqs: Vec<u64> = self.pending.iter().filter(|(_, p)| **p != Pending::Fire).map(|(s, _)| *s).collect();
        seqs.sort_unstable();
        self.stats.hangups += seqs.len() as u64;
        seqs.into_iter().map(|seq| Effect::Hangup { seq }).collect()
    }

    /// Wartende Aufrufe vergessen; ein wartendes Symptom überschreiben
    fn wipe_pending(&mut self) {
        for p in self.pending.values_mut() {
            if let Pending::Symptom { text } = p {
                wipe(text);
            }
        }
        self.pending.clear();
    }

    fn wipe_all(&mut self) {
        if let Some(p) = &mut self.preview {
            p.wipe();
        }
        if let Some(a) = &mut self.answer {
            a.wipe();
        }
        if let Some(pr) = &mut self.progress {
            for r in pr.rows.values_mut() {
                wipe(&mut r.params);
            }
        }
        for u in self.units.iter_mut().flatten() {
            wipe(&mut u.unit);
        }
        for it in self.plan.iter_mut().flatten() {
            wipe(&mut it.label);
            if let Some(p) = &mut it.probe {
                wipe_json(p);
            }
        }
        if let Some(e) = &mut self.err {
            wipe(&mut e.title);
            if let Some(b) = &mut e.body {
                wipe(b);
            }
        }
        if let Some((n, _)) = &mut self.notice {
            wipe(n);
        }
        if let Some(u) = &mut self.unit {
            wipe(u);
        }
        wipe(&mut self.field.text);
        wipe(&mut self.field.applied);
        let stats = std::mem::take(&mut self.stats);
        let (gen, seq) = (self.gen, self.seq);
        *self = DiagPanel::new();
        self.gen = gen;
        self.seq = seq;
        self.stats = stats;
    }

    /// Der Daemon kennt die Sitzung nicht mehr (`UNKNOWN_SESSION`: abgelaufen oder Daemon neu gestartet, etwa bei
    /// der Installation): Bericht, Antwort und Klartext auch hier löschen, nicht erst beim Schließen (Durchsicht W4, F2).
    /// Das übernommene Symptom bleibt (Eingabe des Users, kommt nach neuem Sammeln wieder)
    fn forget_session(&mut self) {
        if let Some(p) = &mut self.preview {
            p.wipe();
        }
        if let Some(a) = &mut self.answer {
            a.wipe();
        }
        self.preview = None;
        self.answer = None;
        self.answer_preview = None;
        self.evidence = None;
        self.id = None;
        self.expires_at = None;
        self.preview_busy = false;
    }

    /// Fokus des Panels verloren (anderes Fenster gewählt): schließen
    pub fn focus_lost(&mut self) -> Vec<Effect> {
        self.close()
    }

    fn call(&mut self, call: Call, pending: Pending) -> Effect {
        self.seq += 1;
        self.pending.insert(self.seq, pending);
        Effect::Call { gen: self.gen, seq: self.seq, call }
    }

    fn set_view(&mut self, v: View, now: Instant, out: &mut Vec<Effect>) {
        if self.view != v {
            self.view = v;
            self.gate.changed(now);
            if v != View::Preview && self.field.editing {
                self.field.editing = false;
                out.push(Effect::Field(false));
            }
            self.active = self.default_active();
        }
        if !self.visible {
            self.visible = true;
            out.push(Effect::Show);
        }
        out.push(Effect::Render);
    }

    fn render_only(&self) -> Vec<Effect> {
        if self.visible {
            vec![Effect::Render]
        } else {
            vec![]
        }
    }

    /// Nächster Zeitpunkt, zu dem [`DiagPanel::tick`] etwas tun kann
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.view {
            View::Closed => None,
            View::Loading if !self.visible => self.opened_at.map(|t| t + SHOW_DELAY),
            View::Loading | View::Progress | View::Send => None,
            _ => {
                let idle = self.gate.last_input().map(|t| t + crate::ui::gate::IDLE);
                match (idle, self.expires_at) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }
            }
        }
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        match self.view {
            View::Loading if !self.visible && self.opened_at.is_some_and(|t| now >= t + SHOW_DELAY) => {
                self.visible = true;
                vec![Effect::Show, Effect::Render]
            }
            View::Closed | View::Loading | View::Progress | View::Send => vec![],
            _ if self.gate.idle(now) || self.expires_at.is_some_and(|t| now >= t) => self.close(),
            _ => vec![],
        }
    }

    // ---- Abläufe -------------------------------------------------------------------------------------------------

    fn start_profiles(&mut self) -> Vec<Effect> {
        self.profile = None;
        self.plan = None;
        self.units = None;
        self.unit = None;
        self.container = None;
        vec![self.call(Call::Plan { profile: None, unit: None, container: None }, Pending::Profiles)]
    }

    fn profile_info(&self) -> Option<&ProfileInfo> {
        let name = self.profile.as_deref()?;
        self.profiles.iter().find(|p| p.name == name)
    }

    fn load_plan(&mut self) -> Effect {
        let profile = self.profile.clone().unwrap_or_default();
        self.plan_loading = true;
        self.plan = None;
        self.call(Call::Plan { profile: Some(profile.clone()), unit: self.unit.clone(), container: self.container.clone() }, Pending::Plan { profile })
    }

    fn pick_profile(&mut self, i: usize, now: Instant) -> Vec<Effect> {
        let Some(p) = self.profiles.get(i).cloned() else { return vec![] };
        let preset = self.preset.take().filter(|pr| pr.profile == p.name);
        self.profile = Some(p.name.clone());
        self.unit = None;
        self.units = None;
        self.plan = None;
        self.items_active = None;
        self.container = None;
        let mut out = vec![];
        if p.needs_unit {
            self.preset = preset;
            self.set_view(View::Items, now, &mut out);
            out.push(self.call(Call::Units, Pending::Units { profile: p.name }));
        } else {
            self.container = preset.and_then(|pr| pr.container);
            self.plan_loading = true;
            self.set_view(View::Items, now, &mut out);
            out.push(self.load_plan());
        }
        out
    }

    fn pick_unit(&mut self, i: usize) -> Vec<Effect> {
        let Some(u) = self.units.as_ref().and_then(|l| l.get(i)).map(|u| u.unit.clone()) else { return vec![] };
        self.items_active = Some(format!("unit:{}", i + 1));
        self.active = self.items_active.clone();
        if self.unit.as_deref() == Some(u.as_str()) && self.plan.is_some() {
            return vec![Effect::Render];
        }
        self.unit = Some(u);
        vec![self.load_plan(), Effect::Render]
    }

    fn toggle_item(&mut self, index: usize) -> Vec<Effect> {
        let Some(plan) = &mut self.plan else { return vec![] };
        if let Some(it) = plan.iter_mut().find(|it| it.index == index) {
            it.selected = !it.selected;
            self.items_active = Some(format!("item:{index}"));
            self.active = self.items_active.clone();
        }
        vec![Effect::Render]
    }

    pub fn selected_count(&self) -> usize {
        self.plan.as_ref().map_or(0, |p| p.iter().filter(|it| it.selected).count())
    }

    pub fn can_collect(&self) -> bool {
        if self.profile_info().is_some_and(|p| p.needs_unit) && self.unit.is_none() {
            return false;
        }
        self.plan.is_some() && self.selected_count() > 0
    }

    fn collect(&mut self, now: Instant) -> Vec<Effect> {
        if !self.can_collect() {
            return vec![];
        }
        let skip = self.plan.as_ref().map(|p| p.iter().filter(|it| !it.selected).map(|it| it.index).collect()).unwrap_or_default();
        // neue Sitzung: ihre Frist kommt mit der Vorschau
        self.expires_at = None;
        let call = Call::Collect { profile: self.profile.clone().unwrap_or_default(), unit: self.unit.clone(), container: self.container.clone(), skip };
        self.stats.collects += 1;
        if let Some(a) = &mut self.answer {
            a.wipe();
        }
        self.answer = None;
        self.answer_preview = None;
        self.sec = None;
        self.new_ids.clear();
        self.start_progress(call, false, now)
    }

    fn start_progress(&mut self, call: Call, check_round: bool, now: Instant) -> Vec<Effect> {
        self.progress = Some(ProgressInfo { total: 0, rows: BTreeMap::new(), lines: 0, started: now });
        self.check_round = check_round;
        let mut out = vec![];
        self.set_view(View::Progress, now, &mut out);
        let e = self.call(call, if check_round { Pending::Check } else { Pending::Collect });
        if let Effect::Call { seq, .. } = &e {
            self.job = Some(*seq);
        }
        out.push(e);
        out
    }

    /// Zwischenzeile `{"progress":{…}}` eines Sammelns oder Prüfens (`on_progress` 1512)
    pub fn progress_line(&mut self, gen: u64, seq: u64, v: &Value) -> Vec<Effect> {
        if gen != self.gen || self.view != View::Progress || self.job != Some(seq) {
            return vec![];
        }
        let p = &v["progress"];
        if !p.is_object() {
            return vec![];
        }
        if let Some(id) = p["id"].as_str() {
            self.id = Some(id.to_string());
        }
        let Some(pr) = &mut self.progress else { return vec![] };
        pr.total = pr.total.max(p["total"].as_u64().unwrap_or(0) as usize);
        let Some(idx) = p["index"].as_u64().map(|i| i as usize) else { return vec![] };
        match p["phase"].as_str() {
            Some("start") => {
                pr.rows.insert(idx, ProgressRow { probe: s(&p["probe"]), params: s(&p["params"]), status: None, bytes: 0, ms: 0 });
            }
            Some("end") => {
                let r = pr.rows.entry(idx).or_insert_with(|| ProgressRow { probe: s(&p["probe"]), params: String::new(), status: None, bytes: 0, ms: 0 });
                r.status = Some(p["status"].as_str().unwrap_or("ok").to_string());
                r.bytes = p["bytes"].as_u64().unwrap_or(0);
                r.ms = p["ms"].as_u64().unwrap_or(0);
            }
            _ => {}
        }
        pr.lines += 1;
        vec![Effect::Render]
    }

    fn take_preview(&mut self, mut pv: PreviewInfo, now: Instant) {
        self.expires_at = Some(now + pv.expires_in.unwrap_or(SESSION_TTL));
        if let Some(old) = &mut self.preview {
            old.wipe();
        }
        if !pv.id.is_empty() {
            self.id = Some(pv.id.clone());
        } else {
            pv.id = self.id.clone().unwrap_or_default();
        }
        self.notice = None;
        self.stats.previews += 1;
        if self.sec.as_ref().is_none_or(|s| pv.section(s).is_none()) {
            self.sec = pv.sections.first().map(|s| s.id.clone());
        }
        self.preview = Some(pv);
    }

    /// Antwort zu genau diesem Bericht schon da: Senden erst nach einer Änderung (`reviewing` 783)
    pub fn reviewing(&self) -> bool {
        self.answer.is_some() && self.answer_preview.is_some() && self.answer_preview == self.preview.as_ref().map(|p| p.preview)
    }

    pub fn can_send(&self) -> bool {
        let Some(pv) = &self.preview else { return false };
        !self.reviewing() && pv.target.ready && pv.included() > 0 && !self.preview_busy && !self.field.editing
    }

    pub fn rounds_left(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| p.rounds < p.max_rounds)
    }

    fn stop(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = vec![];
        if let Some(seq) = self.job.take() {
            self.pending.remove(&seq);
            self.stats.hangups += 1;
            out.push(Effect::Hangup { seq });
        }
        match self.view {
            View::Progress => {
                if let Some(id) = self.id.clone() {
                    out.push(self.call(Call::Cancel { id }, Pending::Fire));
                }
                let back = if !self.check_round {
                    View::Items
                } else if self.answer.is_some() {
                    View::Answer
                } else {
                    View::Preview
                };
                self.set_view(back, now, &mut out);
            }
            View::Send => self.set_view(View::Preview, now, &mut out),
            _ => {}
        }
        out
    }

    /// Abschnitt weglassen bzw. wieder aufnehmen: neue Vorschau mit der ganzen Abwahl (`toggle_section` 1654)
    fn toggle_section(&mut self, id: &str) -> Vec<Effect> {
        let Some(pv) = &self.preview else { return vec![] };
        if self.preview_busy || pv.section(id).is_none() {
            return vec![];
        }
        let des: Vec<String> = pv.sections.iter().filter(|s| (s.id == id) == s.included).map(|s| s.id.clone()).collect();
        let call = Call::Preview { id: pv.id.clone(), deselected: Some(des), symptom: None };
        self.sec = Some(id.to_string());
        self.preview_busy = true;
        vec![self.call(call, Pending::Toggle), Effect::Render]
    }

    fn move_section(&mut self, dir: i32) -> Vec<Effect> {
        let Some(pv) = &self.preview else { return vec![] };
        let ids: Vec<&String> = pv.sections.iter().map(|s| &s.id).collect();
        if ids.is_empty() {
            return vec![];
        }
        let idx = ids.iter().position(|i| Some(*i) == self.sec.as_ref());
        let next = idx.map_or(0, |i| (i as i32 + dir).clamp(0, ids.len() as i32 - 1) as usize);
        self.sec = Some(ids[next].clone());
        self.scroll_to = Some(ScrollTo { section: ids[next].clone(), line: None });
        vec![Effect::Render]
    }

    fn copy(&mut self) -> Vec<Effect> {
        let Some(id) = self.id.clone() else { return vec![] };
        if self.copy_busy {
            return vec![];
        }
        self.copy_busy = true;
        vec![self.call(Call::Copy { id }, Pending::Copy)]
    }

    fn send(&mut self, now: Instant) -> Vec<Effect> {
        if !self.can_send() {
            return vec![];
        }
        let pv = self.preview.as_ref().expect("can_send");
        let call = Call::Send { id: pv.id.clone(), preview: pv.preview };
        let preview = pv.preview;
        self.stats.sends += 1;
        self.send_started = Some(now);
        let mut out = vec![];
        self.set_view(View::Send, now, &mut out);
        let e = self.call(call, Pending::Send { preview });
        if let Effect::Call { seq, .. } = &e {
            self.job = Some(*seq);
        }
        out.push(e);
        out
    }

    pub fn selected_checks(&self) -> Vec<&CheckInfo> {
        let Some(a) = &self.answer else { return vec![] };
        a.checks.iter().enumerate().filter(|(i, _)| self.picked.contains(&(i + 1))).map(|(_, c)| c).collect()
    }

    fn toggle_check(&mut self, i: usize) -> Vec<Effect> {
        let n = self.answer.as_ref().map_or(0, |a| a.checks.len());
        if i == 0 || i > n || !self.rounds_left() {
            return vec![];
        }
        if !self.picked.remove(&i) {
            self.picked.insert(i);
        }
        self.answer_active = Some(format!("check:{i}"));
        self.active = self.answer_active.clone();
        vec![Effect::Render]
    }

    fn run_checks(&mut self, now: Instant) -> Vec<Effect> {
        let list: Vec<CheckInfo> = self.selected_checks().into_iter().cloned().collect();
        let Some(id) = self.id.clone() else { return vec![] };
        if list.is_empty() || !self.rounds_left() {
            return vec![];
        }
        // Netzverkehr gilt als bestätigt, wenn der User eine als „network“ markierte Prüfung gewählt hat
        let network = list.iter().any(|c| c.network);
        let call = Call::Check { id, checks: list.into_iter().map(|c| c.probe).collect(), network };
        self.stats.checks += 1;
        self.picked.clear();
        self.start_progress(call, true, now)
    }

    /// Beleg: Vorschau mit markierten Zeilen, ⌫ zurück zur Antwort (`jump` 1878)
    fn jump(&mut self, ev_id: &str, now: Instant) -> Vec<Effect> {
        let Some(e) = self.evidence_of(ev_id).cloned() else { return vec![] };
        self.stats.jumps += 1;
        self.answer_active = Some(ev_id.to_string());
        self.sec = Some(e.section.clone());
        self.scroll_to = Some(ScrollTo { section: e.section.clone(), line: Some(e.from) });
        self.evidence = Some(e);
        let mut out = vec![];
        self.set_view(View::Preview, now, &mut out);
        out
    }

    pub fn evidence_of(&self, id: &str) -> Option<&Evidence> {
        let rest = id.strip_prefix("ev:")?;
        let (f, e) = rest.split_once(':')?;
        let (f, e): (usize, usize) = (f.parse().ok()?, e.parse().ok()?);
        self.answer.as_ref()?.findings.get(f.checked_sub(1)?)?.evidence.get(e.checked_sub(1)?)
    }

    /// Kennungen der Belege in Reihenfolge (`ev:<befund>:<beleg>`, ab 1)
    pub fn evidence_ids(&self) -> Vec<String> {
        let Some(a) = &self.answer else { return vec![] };
        a.findings.iter().enumerate().flat_map(|(fi, f)| (0..f.evidence.len()).map(move |ei| format!("ev:{}:{}", fi + 1, ei + 1))).collect()
    }

    fn back(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = vec![];
        match self.view {
            View::Items => {
                // ein laufendes `diag_units` oder `diag_plan` dieser Ansicht auflegen (es hielte die Reihe auf)
                let mut seqs: Vec<u64> =
                    self.pending.iter().filter(|(_, p)| matches!(p, Pending::Units { .. } | Pending::Plan { .. })).map(|(s, _)| *s).collect();
                seqs.sort_unstable();
                for seq in seqs {
                    self.pending.remove(&seq);
                    self.stats.hangups += 1;
                    out.push(Effect::Hangup { seq });
                }
                self.plan_loading = false;
                self.plan = None;
                self.units = None;
                self.unit = None;
                self.container = None;
                self.set_view(View::Profiles, now, &mut out);
            }
            View::Preview if self.answer.is_some() => {
                self.evidence = None;
                self.set_view(View::Answer, now, &mut out);
                if let Some(a) = self.answer_active.clone() {
                    self.active = Some(a);
                }
            }
            View::Answer => {
                self.answer_active = self.active.clone();
                self.evidence = None;
                self.set_view(View::Preview, now, &mut out);
            }
            View::Error => {
                if let Some(b) = self.err.as_ref().and_then(|e| e.back) {
                    self.set_view(b, now, &mut out);
                }
            }
            _ => {}
        }
        out
    }

    fn show_error(&mut self, err: &CallErr, back: Option<View>, copy: bool, now: Instant) -> Vec<Effect> {
        self.notice = None;
        let e = diag_error(&err.message(), back, copy);
        if e.expired {
            self.forget_session();
        }
        self.err = Some(e);
        let mut out = vec![];
        self.set_view(View::Error, now, &mut out);
        out
    }

    // ---- Feld „What is wrong?“ -----------------------------------------------------------------------------------

    /// Inhalt des Eingabefelds hat sich geändert (GTK)
    pub fn field_text(&mut self, text: String, now: Instant) -> Vec<Effect> {
        if self.view != View::Preview {
            let mut t = text;
            wipe(&mut t);
            return vec![];
        }
        self.gate.touch(now);
        wipe(&mut self.field.text);
        self.field.text = text;
        self.field.editing = true;
        vec![]
    }

    /// Das Eingabefeld hat den Fokus bekommen (Klick hinein); verloren geht er nur über Enter, Tab und Esc
    pub fn field_focused(&mut self, now: Instant) -> Vec<Effect> {
        if self.view != View::Preview || self.field.editing {
            return vec![];
        }
        if self.preview_busy {
            // eine neue Vorschau ist unterwegs: erst danach
            return vec![Effect::Field(false)];
        }
        self.gate.touch(now);
        self.field.editing = true;
        self.active = Some("symptom".into());
        vec![Effect::Highlight]
    }

    fn enter_field(&mut self) -> Vec<Effect> {
        if self.view != View::Preview || self.preview_busy {
            return vec![];
        }
        self.field.editing = true;
        self.active = Some("symptom".into());
        vec![Effect::Field(true), Effect::Render]
    }

    /// Feld verlassen; `apply`: übernehmen (neue Vorschau), sonst zurück zum übernommenen Text
    fn leave_field(&mut self, apply: bool) -> Vec<Effect> {
        if !self.field.editing {
            return vec![];
        }
        self.field.editing = false;
        let mut out = vec![Effect::Field(false)];
        let changed = self.field.text.trim() != self.field.applied.trim();
        if apply && changed {
            if let Some(id) = self.preview.as_ref().map(|p| p.id.clone()) {
                let text = self.field.text.clone();
                let call = Call::Preview { id, deselected: None, symptom: Some(SecretText::new(text.clone())) };
                self.preview_busy = true;
                out.push(self.call(call, Pending::Symptom { text }));
            }
        } else if !apply {
            wipe(&mut self.field.text);
            self.field.text = self.field.applied.clone();
        }
        self.active = self.default_active();
        out.push(Effect::Render);
        out
    }

    // ---- Antworten des Daemons -----------------------------------------------------------------------------------

    /// Antwort auf den Aufruf `seq`; Texte werden aus `res` herausgenommen (der Aufrufer überschreibt den Rest)
    pub fn reply(&mut self, gen: u64, seq: u64, res: Result<&mut Value, CallErr>, now: Instant) -> Vec<Effect> {
        if gen != self.gen {
            return vec![];
        }
        let Some(p) = self.pending.remove(&seq) else { return vec![] };
        if self.job == Some(seq) {
            self.job = None;
        }
        // Sitzung im Daemon unbekannt (abgelaufen, Daemon neu gestartet): gleich bei jedem Aufruf, auch bei denen, die
        // sonst nur einen Hinweis zeigen (Copy, Abwahl, Symptom), Bericht und Antwort hier löschen (Durchsicht W4, F2)
        if let Err(e) = &res {
            if p != Pending::Fire && e.unknown_session() {
                if let Pending::Symptom { mut text } = p {
                    wipe(&mut text);
                }
                return self.show_error(e, None, false, now);
            }
        }
        match p {
            Pending::Fire => vec![],
            Pending::Profiles => match res {
                Ok(v) if v["profiles"].is_array() => {
                    self.profiles = v["profiles"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|p| p["name"].is_string())
                        .map(|p| ProfileInfo {
                            name: s(&p["name"]),
                            label: p["label"].as_str().map_or_else(|| s(&p["name"]), str::to_string),
                            description: s(&p["description"]),
                            needs_unit: p["needs_unit"] == true,
                        })
                        .collect();
                    let preset = self.preset.clone();
                    let mut out = match preset.and_then(|pr| self.profiles.iter().position(|p| p.name == pr.profile)) {
                        Some(i) => self.pick_profile(i, now),
                        None => {
                            self.preset = None;
                            let mut out = vec![];
                            self.set_view(View::Profiles, now, &mut out);
                            out
                        }
                    };
                    if std::mem::take(&mut self.chosen_pending) {
                        out.push(Effect::Chosen);
                    }
                    out
                }
                Ok(_) => self.show_error(&CallErr::Failed("Unexpected answer from dictate".into()), None, false, now),
                Err(e) => self.show_error(&e, None, false, now),
            },
            Pending::Units { profile } => {
                if self.profile.as_deref() != Some(profile.as_str()) || self.view != View::Items {
                    return vec![];
                }
                match res {
                    Ok(v) if v["units"].is_array() => {
                        let mut units: Vec<UnitInfo> = v["units"]
                            .as_array_mut()
                            .into_iter()
                            .flatten()
                            .filter(|u| u["unit"].is_string())
                            .map(|u| UnitInfo { user: u["user"] == true, unit: take_str(&mut u["unit"]), searched: false })
                            .collect();
                        let mut out = vec![];
                        match self.preset.take().and_then(|p| p.unit) {
                            Some(term) => {
                                let hits = unit_matches(&units, &term);
                                if hits.len() == 1 {
                                    self.unit = hits.into_iter().next();
                                    self.units = Some(units);
                                    out.push(self.load_plan());
                                } else if hits.len() > 1 {
                                    // mehrere passen: der User wählt, die erste ist vorgemerkt
                                    let i = units.iter().position(|u| u.unit == hits[0]).unwrap_or(0);
                                    self.items_active = Some(format!("unit:{}", i + 1));
                                    self.active = self.items_active.clone();
                                    self.units = Some(units);
                                } else {
                                    // keine fehlgeschlagene passt: der Begriff steht oben und wird beim Sammeln gegen alle
                                    // Units aufgelöst (oder abgelehnt)
                                    units.insert(0, UnitInfo { unit: term.clone(), user: false, searched: true });
                                    self.unit = Some(term);
                                    self.units = Some(units);
                                    out.push(self.load_plan());
                                }
                            }
                            None => {
                                if let Some(first) = units.first() {
                                    self.unit = Some(first.unit.clone());
                                    self.units = Some(units);
                                    out.push(self.load_plan());
                                } else {
                                    self.units = Some(units);
                                }
                            }
                        }
                        if self.active.is_none() {
                            self.active = self.default_active();
                        }
                        out.push(Effect::Render);
                        out
                    }
                    Ok(_) => self.show_error(&CallErr::Failed("Unexpected answer from dictate".into()), Some(View::Profiles), false, now),
                    Err(e) => self.show_error(&e, Some(View::Profiles), false, now),
                }
            }
            Pending::Plan { profile } => {
                if self.profile.as_deref() != Some(profile.as_str()) {
                    return vec![];
                }
                self.plan_loading = false;
                match res {
                    Ok(v) if v["items"].is_array() => {
                        let items: Vec<PlanItem> = v["items"]
                            .as_array_mut()
                            .into_iter()
                            .flatten()
                            .filter_map(|it| {
                                Some(PlanItem {
                                    index: it["index"].as_u64()? as usize,
                                    label: it["label"].as_str().unwrap_or("?").to_string(),
                                    probe: it["probe"].is_object().then(|| it["probe"].take()),
                                    selected: it["selected"] == true,
                                    network: it["network"] == true,
                                })
                            })
                            .collect();
                        self.plan = Some(items);
                        let mut out = vec![];
                        if self.view == View::Items {
                            if self.items_active.is_none() {
                                self.active = self.default_active();
                            }
                            out.push(Effect::Render);
                        } else {
                            self.set_view(View::Items, now, &mut out);
                        }
                        out
                    }
                    Ok(_) => self.show_error(&CallErr::Failed("Unexpected answer from dictate".into()), Some(View::Profiles), false, now),
                    Err(e) => self.show_error(&e, Some(View::Profiles), false, now),
                }
            }
            Pending::Collect | Pending::Check => {
                let check_round = p == Pending::Check;
                if self.view != View::Progress {
                    return vec![];
                }
                // ein Stop des Users entfernt den wartenden Aufruf; was hier ankommt, hat jemand anderes beendet
                let pv = match res {
                    Ok(v) => PreviewInfo::from_json(v),
                    Err(e) => {
                        let back = if !check_round {
                            View::Items
                        } else if self.answer.is_some() {
                            View::Answer
                        } else {
                            View::Preview
                        };
                        return self.show_error(&e, Some(back), false, now);
                    }
                };
                let Some(pv) = pv else {
                    return self.show_error(&CallErr::Failed("Unexpected answer from dictate".into()), Some(View::Items), false, now);
                };
                self.evidence = None;
                self.scroll_to = None;
                self.new_ids.clear();
                let round = pv.rounds;
                self.take_preview(pv, now);
                if check_round {
                    // die Vorschau steht auf dem ersten neuen Abschnitt, die neuen sind markiert (Review L3)
                    let fresh: Vec<String> =
                        self.preview.as_ref().map_or(vec![], |p| p.sections.iter().filter(|s| round > 1 && s.round == round).map(|s| s.id.clone()).collect());
                    if let Some(first) = fresh.first() {
                        self.sec = Some(first.clone());
                        self.scroll_to = Some(ScrollTo { section: first.clone(), line: None });
                    }
                    self.new_ids = fresh;
                }
                let mut out = vec![];
                // neues Sammeln: die neue Sitzung kennt das übernommene Symptom nicht, es wird wieder gesetzt
                if !check_round && !self.field.applied.trim().is_empty() {
                    if let Some(id) = self.preview.as_ref().map(|p| p.id.clone()) {
                        let text = self.field.applied.clone();
                        self.preview_busy = true;
                        out.push(self.call(Call::Preview { id, deselected: None, symptom: Some(SecretText::new(text.clone())) }, Pending::Symptom { text }));
                    }
                }
                self.set_view(View::Preview, now, &mut out);
                out
            }
            Pending::Toggle | Pending::Symptom { .. } => {
                self.preview_busy = false;
                let mut symptom = match p {
                    Pending::Symptom { text } => Some(text),
                    _ => None,
                };
                match res {
                    Ok(v) => match PreviewInfo::from_json(v) {
                        Some(pv) => {
                            self.take_preview(pv, now);
                            if let Some(t) = symptom {
                                wipe(&mut self.field.applied);
                                self.field.applied = t;
                            }
                            // der Bericht hat sich geändert: Enter sendet wieder (Vorgabe), außer der User steht im Feld
                            let stale =
                                self.active.as_deref().is_none_or(|a| !self.enabled(a)) || (!self.reviewing() && self.active.as_deref() == Some("back"));
                            if self.view == View::Preview && !self.field.editing && stale {
                                self.active = self.default_active();
                            }
                            self.render_only()
                        }
                        None => self.show_error(&CallErr::Failed("Unexpected answer from dictate".into()), Some(View::Preview), false, now),
                    },
                    Err(e) if symptom.is_some() => {
                        // das Symptom wurde nicht übernommen: Meldung in der Vorschau, das Feld zeigt wieder den alten Stand
                        if let Some(t) = &mut symptom {
                            wipe(t);
                        }
                        self.notice = Some((e.message(), true));
                        if !self.field.editing {
                            wipe(&mut self.field.text);
                            self.field.text = self.field.applied.clone();
                        }
                        self.render_only()
                    }
                    Err(e) => self.show_error(&e, Some(View::Preview), false, now),
                }
            }
            Pending::Copy => {
                self.copy_busy = false;
                match res {
                    Ok(v) => {
                        self.stats.copies += 1;
                        let n = v["chars"].as_u64().unwrap_or(0);
                        self.notice = Some((format!("Copied the report: {}.", plural(n, "character", "characters")), false));
                        let mut out = self.render_only();
                        out.push(Effect::Notice(format!("Copied the report ({})", plural(n, "character", "characters"))));
                        out
                    }
                    Err(e) => {
                        self.notice = Some((e.message(), true));
                        self.render_only()
                    }
                }
            }
            Pending::Send { preview } => {
                if self.view != View::Send {
                    return vec![];
                }
                match res {
                    Ok(v) => {
                        let known: HashSet<String> = self.preview.as_ref().map_or(HashSet::new(), |p| p.sections.iter().map(|s| s.id.clone()).collect());
                        let (mut a, mut r) = (v["answer"].take(), v["restore"].take());
                        let parsed = parse_answer(&mut a, &mut r, &known);
                        wipe_json(&mut a);
                        wipe_json(&mut r);
                        let Some(ans) = parsed else {
                            return self.show_error(&CallErr::Failed("The answer of the AI was not usable".into()), Some(View::Preview), true, now);
                        };
                        if let Some(old) = &mut self.answer {
                            old.wipe();
                        }
                        self.answer_model = v["model"].as_str().map(str::to_string);
                        if let (Some(n), Some(pv)) = (v["rounds"].as_u64(), self.preview.as_mut()) {
                            pv.rounds = n as u32;
                        }
                        self.answer = Some(ans);
                        self.answer_preview = Some(preview);
                        self.picked.clear();
                        self.answer_active = None;
                        // jede Antwort zeigt zuerst die Platzhalter, wie gesendet
                        self.plain = false;
                        let mut out = vec![];
                        self.set_view(View::Answer, now, &mut out);
                        out
                    }
                    Err(e) => self.show_error(&e, Some(View::Preview), true, now),
                }
            }
        }
    }

    // ---- Bedienelemente ------------------------------------------------------------------------------------------

    fn default_active(&self) -> Option<String> {
        match self.view {
            View::Profiles => {
                let i = self.profile.as_ref().and_then(|n| self.profiles.iter().position(|p| &p.name == n)).unwrap_or(0);
                (!self.profiles.is_empty()).then(|| format!("profile:{}", i + 1))
            }
            View::Items => {
                if let Some(a) = &self.items_active {
                    return Some(a.clone());
                }
                if self.units.as_ref().is_some_and(|u| !u.is_empty()) && self.unit.is_none() {
                    return Some("unit:1".into());
                }
                self.plan.as_ref().and_then(|p| p.first()).map(|it| format!("item:{}", it.index))
            }
            View::Preview => {
                if self.reviewing() {
                    Some(if self.answer.is_some() { "back".into() } else { "copy".into() })
                } else if self.preview.as_ref().is_some_and(|p| p.target.ready && p.included() > 0) {
                    Some("send".into())
                } else {
                    Some("copy".into())
                }
            }
            View::Answer => {
                if let Some(a) = self.answer_active.clone().filter(|a| self.enabled(a)) {
                    return Some(a);
                }
                if let Some(first) = self.evidence_ids().into_iter().next() {
                    return Some(first);
                }
                let can = self.rounds_left();
                if can && !self.picked.is_empty() {
                    Some("run".into())
                } else if can && self.answer.as_ref().is_some_and(|a| !a.checks.is_empty()) {
                    Some("check:1".into())
                } else {
                    Some("copy".into())
                }
            }
            View::Error => Some(match &self.err {
                Some(e) if e.retry.is_some() => "retry".into(),
                Some(e) if e.copy => "copy".into(),
                _ => "close_btn".into(),
            }),
            View::Closed | View::Loading | View::Progress | View::Send => None,
        }
    }

    /// Bedienbare Elemente der Ansicht in Tab-Reihenfolge (Zeilen und Belege zuerst, dann Knöpfe links, rechts)
    pub fn items(&self) -> Vec<Item> {
        let it = |id: &str, label: &str, key: Option<&str>, kind: ItemKind, left: bool| Item {
            id: id.into(),
            label: label.into(),
            key: key.map(str::to_string),
            kind,
            enabled: true,
            left,
        };
        let mut v = vec![];
        match self.view {
            View::Profiles => {
                for (i, p) in self.profiles.iter().enumerate() {
                    v.push(it(&format!("profile:{}", i + 1), &p.label, Some(&(i + 1).to_string()), ItemKind::Row, false));
                }
            }
            View::Items => {
                for (i, u) in self.units.iter().flatten().enumerate() {
                    v.push(it(&format!("unit:{}", i + 1), &u.unit, None, ItemKind::Row, false));
                }
                for p in self.plan.iter().flatten() {
                    v.push(it(&format!("item:{}", p.index), &p.label, None, ItemKind::Row, false));
                }
                v.push(it("back", "Back", Some("⌫"), ItemKind::Ghost, true));
                let mut c = it("collect", "Collect", Some("Enter"), ItemKind::Primary, false);
                c.enabled = self.can_collect();
                v.push(c);
            }
            View::Progress | View::Send => v.push(it("stop", "Stop", Some("Esc"), ItemKind::Secondary, false)),
            View::Preview => {
                let mut f = it("symptom", "What is wrong?", Some("Tab"), ItemKind::Row, false);
                f.enabled = !self.preview_busy || self.field.editing;
                v.push(f);
                if self.answer.is_some() {
                    v.push(it("back", "Answer", Some("⌫"), ItemKind::Ghost, true));
                }
                v.push(it("copy", "Copy report", Some("c"), ItemKind::Secondary, false));
                if !self.reviewing() {
                    let mut s = it("send", "Send", Some("Enter"), ItemKind::Primary, false);
                    s.enabled = self.can_send();
                    v.push(s);
                }
            }
            View::Answer => {
                let Some(a) = &self.answer else { return v };
                for id in self.evidence_ids() {
                    v.push(it(&id, "Evidence", None, ItemKind::Row, false));
                }
                let can = self.rounds_left();
                for (i, c) in a.checks.iter().enumerate() {
                    let mut r = it(&format!("check:{}", i + 1), &c.label, None, ItemKind::Row, false);
                    r.enabled = can;
                    v.push(r);
                }
                v.push(it("report", "Report", Some("⌫"), ItemKind::Ghost, true));
                if a.has_values() {
                    v.push(it("values", if self.plain { "Placeholders" } else { "Real values" }, Some("p"), ItemKind::Ghost, true));
                }
                v.push(it("copy", "Copy report", Some("c"), ItemKind::Secondary, false));
                if !a.checks.is_empty() {
                    let n = self.picked.len() as u64;
                    let label = if n > 0 { format!("Run {}", plural(n, "check", "checks")) } else { "Run checks".into() };
                    let mut r = it("run", &label, Some("Enter"), ItemKind::Primary, false);
                    r.enabled = can && n > 0;
                    v.push(r);
                }
            }
            View::Error => {
                let Some(e) = &self.err else { return v };
                if e.back.is_some() {
                    v.push(it("back", "Back", Some("⌫"), ItemKind::Ghost, true));
                }
                v.push(it("close_btn", "Close", Some("Esc"), ItemKind::Secondary, false));
                if e.copy {
                    let key = if e.retry.is_some() { "c" } else { "Enter" };
                    let kind = if e.retry.is_some() { ItemKind::Secondary } else { ItemKind::Primary };
                    v.push(it("copy", "Copy report", Some(key), kind, false));
                }
                if let Some(label) = e.retry {
                    v.push(it("retry", label, Some("Enter"), ItemKind::Primary, false));
                }
            }
            View::Closed | View::Loading => {}
        }
        v
    }

    /// Ist das Element `id` gerade bedienbar (auch `sec:S<n>` und `close`)?
    pub fn enabled(&self, id: &str) -> bool {
        if id == "close" {
            return self.is_open();
        }
        if let Some(sec) = id.strip_prefix("sec:") {
            return self.view == View::Preview && self.preview.as_ref().is_some_and(|p| p.section(sec).is_some()) && !self.preview_busy;
        }
        self.items().iter().any(|i| i.id == id && i.enabled)
    }

    /// Element auslösen per Klick (Prüfbefehle: wählen)
    pub fn activate(&mut self, id: &str, now: Instant) -> Vec<Effect> {
        self.trigger(id, false, now)
    }

    /// Element auslösen; `by_key`: Enter auf der Hervorhebung (Prüfbefehle: ausführen)
    fn trigger(&mut self, id: &str, by_key: bool, now: Instant) -> Vec<Effect> {
        if !self.is_open() || !self.gate.click(now) {
            return vec![];
        }
        // ein Klick neben das offene Feld übernimmt es zuerst; der Klick selbst zählt nicht (der User sieht erst die neue
        // Vorschau, dann sendet er sie)
        if self.field.editing && !matches!(id, "symptom" | "close") {
            self.gate.touch(now);
            return self.leave_field(true);
        }
        if !self.enabled(id) {
            return vec![];
        }
        self.gate.touch(now);
        let index = |p: &str| id.strip_prefix(p).and_then(|n| n.parse::<usize>().ok());
        if let Some(i) = index("profile:") {
            return self.pick_profile(i.saturating_sub(1), now);
        }
        if let Some(i) = index("unit:") {
            return self.pick_unit(i.saturating_sub(1));
        }
        if let Some(i) = index("item:") {
            return self.toggle_item(i);
        }
        if let Some(i) = index("check:") {
            return if by_key { self.run_checks(now) } else { self.toggle_check(i) };
        }
        if id.starts_with("ev:") {
            return self.jump(id, now);
        }
        if let Some(sec) = id.strip_prefix("sec:") {
            let sec = sec.to_string();
            return self.toggle_section(&sec);
        }
        match id {
            "collect" => self.collect(now),
            "back" | "report" => self.back(now),
            "stop" => self.stop(now),
            "copy" => self.copy(),
            "send" => self.send(now),
            "run" => self.run_checks(now),
            "values" => {
                self.plain = !self.plain;
                vec![Effect::Render]
            }
            "symptom" => self.enter_field(),
            "retry" => {
                let mut out = vec![];
                self.err = None;
                self.notice = None;
                self.set_view(View::Loading, now, &mut out);
                out.extend(self.start_profiles());
                out
            }
            "close_btn" | "close" => self.close(),
            _ => vec![],
        }
    }

    /// Hervorhebung setzen (Hover)
    pub fn hover(&mut self, id: Option<&str>) -> Vec<Effect> {
        if self.field.editing {
            return vec![];
        }
        let id = id.filter(|i| self.enabled(i)).map(str::to_string);
        if id.is_some() && id != self.active {
            self.active = id;
            return vec![Effect::Highlight];
        }
        vec![]
    }

    fn move_active(&mut self, dir: i32, rows_only: bool, wrap: bool) -> Vec<Effect> {
        let list: Vec<String> =
            self.items().into_iter().filter(|i| i.enabled && (!rows_only || i.kind == ItemKind::Row) && i.id != "symptom").map(|i| i.id).collect();
        if list.is_empty() {
            return vec![];
        }
        let n = list.len() as i32;
        let idx = list.iter().position(|i| Some(i) == self.active.as_ref()).map(|i| i as i32);
        let next = match idx {
            None if dir > 0 => 0,
            None => n - 1,
            Some(i) if wrap => (i + dir).rem_euclid(n),
            Some(i) => (i + dir).clamp(0, n - 1),
        };
        self.active = Some(list[next as usize].clone());
        if self.view == View::Items {
            self.items_active = self.active.clone();
        }
        if self.view == View::Answer {
            self.answer_active = self.active.clone();
        }
        vec![Effect::Render]
    }

    /// Tab: alle Elemente einschließlich des Feldes in der Vorschau
    fn tab(&mut self, dir: i32) -> Vec<Effect> {
        let list: Vec<String> = self.items().into_iter().filter(|i| i.enabled).map(|i| i.id).collect();
        if list.is_empty() {
            return vec![];
        }
        let n = list.len() as i32;
        let idx = list.iter().position(|i| Some(i) == self.active.as_ref()).map(|i| i as i32);
        let next = match idx {
            None if dir > 0 => 0,
            None => n - 1,
            Some(i) => (i + dir).rem_euclid(n),
        };
        let id = list[next as usize].clone();
        if id == "symptom" {
            return self.enter_field();
        }
        self.active = Some(id);
        vec![Effect::Render]
    }

    // ---- Tasten --------------------------------------------------------------------------------------------------

    pub fn key_press(&mut self, key: &str, mods: Mods, now: Instant) -> Vec<Effect> {
        self.gate.press(key, now);
        self.on_key(key, mods, now)
    }

    pub fn key_release(&mut self, key: &str, now: Instant) {
        self.gate.release(key, now);
    }

    fn fire(&mut self, key: &str, now: Instant) -> bool {
        self.gate.fire(key, now)
    }

    fn on_key(&mut self, key: &str, mods: Mods, now: Instant) -> Vec<Effect> {
        if !self.is_open() {
            return vec![];
        }
        self.gate.touch(now);
        let enter = key == "Return" || key == "KP_Enter";
        let tab = key == "Tab" || key == "ISO_Left_Tab";
        // im Feld: nur Esc, Enter und Tab kommen hierher (den Rest bekommt das Eingabefeld)
        if self.field.editing {
            return match key {
                "Escape" => self.leave_field(false),
                _ if enter && self.fire(key, now) => self.leave_field(true),
                _ if tab => {
                    let mut out = self.leave_field(true);
                    out.extend(self.tab(if mods.shift || key == "ISO_Left_Tab" { -1 } else { 1 }));
                    out
                }
                _ => vec![],
            };
        }
        if key == "Escape" {
            return if matches!(self.view, View::Progress | View::Send) { self.stop(now) } else { self.close() };
        }
        if !mods.plain() {
            return vec![];
        }
        if tab {
            return self.tab(if mods.shift || key == "ISO_Left_Tab" { -1 } else { 1 });
        }
        let up_down = match key {
            "Up" => Some(-1),
            "Down" => Some(1),
            _ => None,
        };
        match self.view {
            View::Profiles => {
                if let Some(n) = key.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= self.profiles.len()) {
                    if self.fire(key, now) {
                        return self.trigger(&format!("profile:{n}"), true, now);
                    }
                } else if let Some(d) = up_down {
                    return self.move_active(d, true, true);
                } else if enter {
                    if let Some(a) = self.active.clone() {
                        if self.fire(key, now) {
                            return self.trigger(&a, true, now);
                        }
                    }
                }
                vec![]
            }
            View::Items => {
                if let Some(d) = up_down {
                    return self.move_active(d, true, true);
                }
                let active = self.active.clone().unwrap_or_default();
                let on_row = active.starts_with("item:") || active.starts_with("unit:");
                if key == "space" && on_row && self.fire(key, now) {
                    return self.trigger(&active, true, now);
                }
                if enter && self.fire(key, now) {
                    if active.starts_with("unit:") && self.plan.is_none() {
                        return self.trigger(&active, true, now);
                    }
                    if !active.is_empty() && !on_row {
                        return self.trigger(&active, true, now);
                    }
                    return self.trigger("collect", true, now);
                }
                if key == "BackSpace" && self.fire(key, now) {
                    return self.trigger("back", true, now);
                }
                vec![]
            }
            View::Preview => {
                if let Some(d) = up_down {
                    return self.move_section(d);
                }
                match key {
                    "space" => match self.sec.clone() {
                        Some(sec) if self.fire(key, now) => self.toggle_section(&sec),
                        _ => vec![],
                    },
                    "c" if self.fire(key, now) => self.trigger("copy", true, now),
                    "BackSpace" if self.answer.is_some() && self.fire(key, now) => self.trigger("back", true, now),
                    _ if enter => match self.active.clone() {
                        Some(a) if self.fire(key, now) => self.trigger(&a, true, now),
                        _ => vec![],
                    },
                    _ => scroll_key(key).map(|s| vec![Effect::Scroll(s)]).unwrap_or_default(),
                }
            }
            View::Answer => {
                if let Some(d) = up_down {
                    return self.move_active(d, true, false);
                }
                let active = self.active.clone().unwrap_or_default();
                match key {
                    "space" if active.starts_with("check:") => {
                        if self.fire(key, now) {
                            self.trigger(&active, false, now)
                        } else {
                            vec![]
                        }
                    }
                    "c" if self.fire(key, now) => self.trigger("copy", true, now),
                    "p" if self.fire(key, now) => self.trigger("values", true, now),
                    "BackSpace" if self.fire(key, now) => self.trigger("report", true, now),
                    _ if enter => {
                        if !active.is_empty() && self.fire(key, now) {
                            self.trigger(&active, true, now)
                        } else {
                            vec![]
                        }
                    }
                    _ => scroll_key(key).map(|s| vec![Effect::Scroll(s)]).unwrap_or_default(),
                }
            }
            View::Error => {
                let e = self.err.clone();
                if enter {
                    if let Some(a) = self.active.clone() {
                        if self.fire(key, now) {
                            return self.trigger(&a, true, now);
                        }
                    }
                } else if key == "c" && e.as_ref().is_some_and(|e| e.copy) && self.fire(key, now) {
                    return self.trigger("copy", true, now);
                } else if key == "BackSpace" && e.as_ref().is_some_and(|e| e.back.is_some()) && self.fire(key, now) {
                    return self.trigger("back", true, now);
                }
                vec![]
            }
            View::Closed | View::Loading | View::Progress | View::Send => vec![],
        }
    }

    /// Nach dem Zeichnen: wohin die Vorschau springen soll (einmal)
    pub fn take_scroll(&mut self) -> Option<ScrollTo> {
        self.scroll_to.take()
    }
}

fn scroll_key(key: &str) -> Option<Scroll> {
    Some(match key {
        "k" => Scroll::LineUp,
        "j" => Scroll::LineDown,
        "Prior" | "Page_Up" => Scroll::PageUp,
        "Next" | "Page_Down" => Scroll::PageDown,
        "Home" => Scroll::Top,
        "End" => Scroll::Bottom,
        _ => return None,
    })
}

/// Überschreibt alle Texte in einem JSON-Wert (wie `sock_client::wipe_value`; `ui` hängt nicht von `sock_client` ab)
pub fn wipe_json(v: &mut Value) {
    match v {
        Value::String(s) => wipe(s),
        Value::Array(a) => a.iter_mut().for_each(wipe_json),
        Value::Object(m) => m.values_mut().for_each(wipe_json),
        _ => {}
    }
}

/// `Debug` ohne Inhalt: nur der Typname. Bericht, Parameter, Antwort, Klartext der Platzhalter und Symptom kommen so
/// nie in ein Log oder eine Panik-Meldung (wie die Typen der Sitzung im Daemon, die nur Zahlen zeigen)
macro_rules! opaque_debug {
    ($($t:ty),*) => {$(
        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($t), "(…)"))
            }
        }
    )*};
}

opaque_debug!(
    UnitInfo,
    PlanItem,
    ProgressRow,
    SectionInfo,
    SymptomInfo,
    PreviewInfo,
    PhSpan,
    Finding,
    CheckInfo,
    Answer,
    ReportSection,
    Preset,
    DiagErr,
    Field,
    Pending
);

impl std::fmt::Debug for CallErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CallErr({})", self.kind())
    }
}

impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Call::{}", self.name())
    }
}

impl std::fmt::Debug for DiagPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagPanel").field("view", &self.view).field("gen", &self.gen).field("pending", &self.pending.len()).field("stats", &self.stats).finish()
    }
}

#[cfg(test)]
#[path = "diag_panel_tests.rs"]
mod tests;
