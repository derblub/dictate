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

//! Häufigkeit und Aktualität (Frecency) gewählter Ziele der Palette.
//!
//! **Was gespeichert wird:** je gewähltem Ziel genau eine Zeile `<Absicht>:<Ziel-ID>` (z. B.
//! `launch_app:firefox.desktop`, `open_project:/home/git/blub/dictate`) mit Zähler und der letzten Nutzung als
//! Unix-Zeit. **Nie Eingabetext:** weder was getippt noch was gesprochen wurde, und die Ziel-ID kommt aus den Quellen
//! (App-ID, Projektpfad, `app_id` eines Fensters, Name oder Nummer einer Arbeitsfläche aus dem Kontext), nicht aus der
//! Eingabe. Die Absicht muss aus dem Katalog stammen.
//!
//! **Datei:** `$XDG_STATE_HOME/dictate/frecency.json` (Vorgabe `~/.local/state/dictate/frecency.json`, [`FILE_NAME`]
//! neben `history.jsonl`), Ordner 0700, Datei 0600, JSON `{"version":1,"entries":{"<Schlüssel>":{"count":N,"last":T}}}`.
//! Geschrieben wird atomar: Temp-Datei im selben Ordner (0600), `fsync`, `rename`; ein Absturz hinterlässt die alte oder
//! die neue Datei, nie eine halbe. Eine kaputte oder unlesbare Datei ergibt eine leere Liste und eine Warnung (ohne
//! Inhalt) im Log; der nächste Schreibvorgang ersetzt sie. [`Frecency::clear`] löscht die Datei und alle Einträge.
//!
//! **Bis W6 hieß die Datei `palette.json`** ([`LEGACY_FILE_NAME`]); unter diesem Namen schreibt Noctalia die Farben für
//! `dictate-ui` (`crate::ui::palette`, Vorlage `noctalia/dictate.json.tmpl`), und beide ersetzten einander per
//! `rename` (`PLAN-wayland.md` 6h, Vorlage V1). [`adopt_legacy`] holt beim Start des Daemons eine alte Häufigkeit unter
//! dem neuen Namen ab, nur nach Inhalt erkannt; eine Farbdatei wird dabei nie verschoben oder gelöscht. Liegen beide
//! vor (dann zieht nichts um), leert [`Frecency::clear`] die alte mit, nach derselben Erkennung (Entscheidung 21 in
//! `PLAN-wayland.md` Abschnitt 7): sonst holte der nächste Start die gelöschte Häufigkeit zurück. Die alte Datei wird
//! in beiden Fällen nie per Namen entfernt, sondern über ihren Deskriptor geleert (Lesart L54 in 6h: ein `unlink` könnte
//! eine Farbdatei treffen, die Noctalia gerade per `rename` hinlegt); `palette.json` bleibt dann als leere Datei.
//!
//! **Bewertung** ([`Frecency::score`]): `Zähler × Gewicht(Alter der letzten Nutzung)`. Gewicht: jünger als 4 Stunden ×4,
//! jünger als 1 Tag ×2, jünger als 1 Woche ×1, älter ×0,5 (wie `zoxide`, nur milder im Alter). Der Zähler wächst mit
//! jeder Wahl um 1, höchstens bis [`MAX_COUNT`]: eine Gewohnheit sättigt, damit ein Ziel, das vor Monaten hundertmal
//! gewählt wurde (Höchstwert 100 × 0,5 = 50), ein heute dreizehnmal gewähltes (13 × 4 = 52) nicht für immer verdrängt.
//! Mehr als [`FrecencySettings::max_entries`] (Vorgabe 500) Einträge: die mit der ältesten letzten Nutzung fliegen raus
//! (bei Gleichstand der mit dem kleineren Zähler).
//!
//! **Namen aus Awesome** (Vorlage E2 e in `PLAN-wayland.md` 6g): beim Laden werden Einträge von vor W5 auf die Namen
//! unter niri abgebildet ([`RENAMED`], z. B. `view_tag:3` → `view_workspace:3`; liegt beides vor, werden Zähler addiert
//! und die jüngere Zeit genommen), Einträge ohne Gegenstück verworfen ([`RETIRED`] und jede Absicht, die der Katalog
//! nicht mehr kennt). Das Format bleibt Version 1, damit eine ältere Fassung die Datei weiter liest; geschrieben wird
//! die abgebildete Liste beim nächsten [`Frecency::record`].
//!
//! **Aus** ([`FrecencySettings::enabled`] `= false`): nichts wird gelesen, gezählt oder geschrieben, [`Frecency::score`]
//! ist 0. Vorhandene Daten bleiben dabei unberührt (löschen nur über [`Frecency::clear`]).

use super::unix_now;
use crate::palette::catalog;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Vorgabe für die Obergrenze der Einträge.
pub const DEFAULT_MAX_ENTRIES: usize = 500;
/// Höchster Zählerstand je Ziel.
pub const MAX_COUNT: u32 = 100;
/// Längste Ziel-ID (Bytes); ein Pfad hat höchstens 4096.
pub const MAX_TARGET_BYTES: usize = 4096;
/// Format der Datei.
const FILE_VERSION: u32 = 1;
/// Größte gelesene Datei (500 Einträge mit langen Pfaden bleiben weit darunter).
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;

/// Einstellungen der Häufigkeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrecencySettings {
    /// Aus = nichts lesen, zählen oder schreiben.
    pub enabled: bool,
    /// Obergrenze der Einträge (mindestens 1).
    pub max_entries: usize,
}

impl Default for FrecencySettings {
    fn default() -> FrecencySettings {
        FrecencySettings { enabled: true, max_entries: DEFAULT_MAX_ENTRIES }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    count: u32,
    /// Unix-Zeit der letzten Nutzung (Sekunden).
    last: i64,
}

#[derive(Serialize, Deserialize)]
struct Doc {
    version: u32,
    entries: BTreeMap<String, Entry>,
}

/// Name der Datei im Zustandsordner (neben `history.jsonl`).
pub const FILE_NAME: &str = "frecency.json";
/// Name bis W6; seit jeher auch der Name der Farbdatei von Noctalia (`crate::ui::palette::path`).
pub const LEGACY_FILE_NAME: &str = "palette.json";

/// Standardpfad nach XDG: `$XDG_STATE_HOME/dictate/frecency.json`, sonst `~/.local/state/dictate/frecency.json`.
pub fn default_path() -> PathBuf {
    crate::pipeline::history::default_path().with_file_name(FILE_NAME)
}

/// Der Schlüssel in der Datei: `<Absicht>:<Ziel-ID>`.
pub fn key(intent: &str, target: &str) -> String {
    format!("{intent}:{target}")
}

/// Gewicht nach Alter der letzten Nutzung in Sekunden (negativ = Zukunft zählt als jetzt).
pub fn weight(age_secs: i64) -> f64 {
    match age_secs {
        a if a < 4 * HOUR => 4.0,
        a if a < DAY => 2.0,
        a if a < WEEK => 1.0,
        _ => 0.5,
    }
}

/// Häufigkeit gewählter Ziele, aus mehreren Threads nutzbar.
pub struct Frecency {
    path: PathBuf,
    settings: FrecencySettings,
    entries: Mutex<HashMap<String, Entry>>,
    /// Ordnet die Schreibvorgänge (eine Wahl nach der anderen, der Stand der Datei ist immer der neueste), ohne die
    /// Abfragen der Bewertung (`entries`) während des `fsync` warten zu lassen.
    io: Mutex<()>,
}

impl Frecency {
    /// Öffnet die Datei unter `path` (liest sie, wenn `settings.enabled`). Fehlt sie, beginnt die Liste leer; ist sie
    /// kaputt, ebenfalls, mit Warnung.
    pub fn open(path: PathBuf, mut settings: FrecencySettings) -> Frecency {
        settings.max_entries = settings.max_entries.max(1);
        let entries = if settings.enabled { load(&path, settings.max_entries) } else { HashMap::new() };
        Frecency { path, settings, entries: Mutex::new(entries), io: Mutex::new(()) }
    }

    /// Wie [`Frecency::open`] mit dem Standardpfad ([`default_path`]).
    pub fn open_default(settings: FrecencySettings) -> Frecency {
        Frecency::open(default_path(), settings)
    }

    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Anzahl der Einträge.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bewertung eines Ziels zum Zeitpunkt `now` (Unix-Sekunden); 0, wenn unbekannt oder die Häufigkeit aus ist.
    pub fn score(&self, intent: &str, target: &str, now: i64) -> f64 {
        if !self.settings.enabled {
            return 0.0;
        }
        match self.lock().get(&key(intent, target)) {
            Some(e) => f64::from(e.count) * weight(now.saturating_sub(e.last)),
            None => 0.0,
        }
    }

    /// Bewertung zum Zeitpunkt jetzt.
    pub fn score_now(&self, intent: &str, target: &str) -> f64 {
        self.score(intent, target, unix_now())
    }

    /// Merkt sich, dass der User dieses Ziel gewählt hat, und schreibt die Datei. Aus = nichts. Ein Fehler beim Schreiben
    /// lässt den Zähler im Speicher stehen (die Reihenfolge stimmt bis zum Neustart) und kommt als `Err` zurück.
    /// Fehler bei der Prüfung (`intent` nicht im Katalog, leere oder zu lange oder mit Steuerzeichen versehene Ziel-ID)
    /// ändern nichts.
    pub fn record(&self, intent: &str, target: &str, now: i64) -> Result<()> {
        if !self.settings.enabled {
            return Ok(());
        }
        if catalog::spec(intent).is_none() {
            bail!("unknown intent {intent:?}");
        }
        if target.is_empty() || target.len() > MAX_TARGET_BYTES || target.chars().any(char::is_control) {
            bail!("invalid target id for {intent}");
        }
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = {
            let mut entries = self.lock();
            let e = entries.entry(key(intent, target)).or_insert(Entry { count: 0, last: now });
            e.count = e.count.saturating_add(1).min(MAX_COUNT);
            e.last = now;
            evict(&mut entries, self.settings.max_entries);
            entries.clone()
        };
        save(&self.path, &snapshot)
    }

    /// Löscht alle Einträge im Speicher und die Datei; liegt daneben eine alte Häufigkeit unter `palette.json`, wird sie
    /// geleert ([`remove_legacy`]), damit der nächste Start sie nicht zurückholt.
    pub fn clear(&self) -> Result<()> {
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        self.lock().clear();
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", self.path.display())),
        }
        // nur neben der Datei unter ihrem eigenen Namen: die alte Häufigkeit lag immer im selben Ordner
        match self.path.parent().filter(|_| self.path.file_name() == Some(std::ffi::OsStr::new(FILE_NAME))) {
            Some(dir) if !dir.as_os_str().is_empty() => remove_legacy(dir).map(|_| ()),
            _ => Ok(()),
        }
    }
}

/// Wirft die ältesten Einträge hinaus, bis höchstens `max` bleiben.
fn evict(entries: &mut HashMap<String, Entry>, max: usize) {
    if entries.len() <= max {
        return;
    }
    let mut order: Vec<(i64, u32, String)> = entries.iter().map(|(k, e)| (e.last, e.count, k.clone())).collect();
    order.sort();
    for (_, _, k) in order.into_iter().take(entries.len() - max) {
        entries.remove(&k);
    }
}

/// Absichten von vor W5 (Awesome) und ihr Name unter niri.
pub const RENAMED: &[(&str, &str)] =
    &[("view_tag", "view_workspace"), ("move_window_to_tag", "move_window_to_workspace"), ("previous_tag", "previous_workspace")];
/// Ziel-IDs, die die Kennung eines Schlüsselworts sind und mit der Absicht umbenannt wurden: (neue Absicht, alt, neu).
const RENAMED_TARGETS: &[(&str, &str, &str)] = &[("previous_workspace", "previous_tag", "previous_workspace")];
/// Einträge ohne Gegenstück unter niri (Vorlagen E3, E4, E7), deren Absicht es noch gibt.
pub const RETIRED: &[&str] = &["toggle_window:sticky", "toggle_window:ontop", "show_widget:claude_usage"];

/// Bildet einen geladenen Schlüssel auf den Namen unter niri ab; `None` = verwerfen.
fn migrate(k: &str) -> Option<String> {
    let (intent, target) = k.split_once(':')?;
    let intent = RENAMED.iter().find(|(old, _)| *old == intent).map_or(intent, |(_, new)| new);
    let target = RENAMED_TARGETS.iter().find(|(i, old, _)| *i == intent && *old == target).map_or(target, |(_, _, new)| new);
    let k = key(intent, target);
    (catalog::spec(intent).is_some() && !RETIRED.contains(&k.as_str())).then_some(k)
}

/// Liest die Datei; fehlt sie → leer, kaputt → leer mit Warnung.
fn load(path: &Path, max: usize) -> HashMap<String, Entry> {
    let text = match read_limited(path) {
        Ok(Some(t)) => t,
        Ok(None) => return HashMap::new(),
        Err(e) => {
            tracing::warn!("palette: cannot read {}: {e}; starting with an empty history", path.display());
            return HashMap::new();
        }
    };
    match serde_json::from_str::<Doc>(&text) {
        Ok(doc) if doc.version == FILE_VERSION => {
            let mut entries: HashMap<String, Entry> = HashMap::with_capacity(doc.entries.len());
            for (k, e) in doc.entries.into_iter().filter(|(k, _)| valid_key(k)) {
                let Some(k) = migrate(&k) else { continue };
                let e = Entry { count: e.count.clamp(1, MAX_COUNT), last: e.last.max(0) };
                entries
                    .entry(k)
                    .and_modify(|old| {
                        old.count = old.count.saturating_add(e.count).min(MAX_COUNT);
                        old.last = old.last.max(e.last);
                    })
                    .or_insert(e);
            }
            evict(&mut entries, max);
            entries
        }
        Ok(doc) => {
            tracing::warn!("palette: {} has format version {} (expected {FILE_VERSION}); starting with an empty history", path.display(), doc.version);
            HashMap::new()
        }
        Err(e) => {
            // nur Art und Stelle: die Meldung von serde zitiert bei falschem Typ den Wert (eine Ziel-ID)
            tracing::warn!(
                "palette: {} is not valid ({:?} error at line {} column {}); starting with an empty history",
                path.display(),
                e.classify(),
                e.line(),
                e.column()
            );
            HashMap::new()
        }
    }
}

/// `<Absicht>:<Ziel-ID>` mit nicht leerer Ziel-ID in den Grenzen.
fn valid_key(k: &str) -> bool {
    match k.split_once(':') {
        Some((intent, target)) => !intent.is_empty() && !target.is_empty() && target.len() <= MAX_TARGET_BYTES && !k.chars().any(char::is_control),
        None => false,
    }
}

/// Liest höchstens [`MAX_FILE_BYTES`]; `Ok(None)`, wenn die Datei fehlt.
fn read_limited(path: &Path) -> Result<Option<String>> {
    use std::io::Read;
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("file is larger than {MAX_FILE_BYTES} bytes");
    }
    Ok(Some(String::from_utf8(bytes)?))
}

fn save(path: &Path, entries: &HashMap<String, Entry>) -> Result<()> {
    let doc = Doc { version: FILE_VERSION, entries: entries.iter().map(|(k, e)| (k.clone(), *e)).collect() };
    let mut json = serde_json::to_string_pretty(&doc)?;
    json.push('\n');
    write_atomic(path, json.as_bytes()).with_context(|| format!("writing {}", path.display()))
}

/// Was [`adopt_legacy`] mit `palette.json` getan hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adoption {
    /// Keine `palette.json` im Ordner.
    Absent,
    /// `palette.json` ist keine Häufigkeit (die Farbdatei von Noctalia, etwas anderes, unlesbar, kein JSON, ein
    /// symbolischer Link, ein Ordner, zu groß): unberührt.
    NotFrecency,
    /// `palette.json` ist eine Häufigkeit, aber `frecency.json` gibt es schon: beide unberührt.
    BothPresent,
    /// Die Häufigkeit steht jetzt in `frecency.json`; `palette.json` ist geleert und bleibt als leere Datei stehen
    /// (Lesart L54: nie per Namen entfernen).
    Moved,
    /// Die Häufigkeit steht jetzt in `frecency.json`; `palette.json` wurde während der Übernahme ersetzt (Noctalia) und
    /// bleibt darum stehen.
    MovedButReplaced,
    /// Die Häufigkeit steht jetzt in `frecency.json`, aber die alte Datei ließ sich nicht leeren (zweiter Name, nicht
    /// schreibbar); sie bleibt, wie sie ist, und jeder weitere Start meldet `BothPresent`.
    MovedNotEmptied,
}

/// Übernahme beim Start des Daemons (Vorlage V1 in `PLAN-wayland.md` 6h): liegt in `dir` eine Häufigkeit unter dem alten
/// Namen `palette.json` und noch keine `frecency.json`, zieht sie um. Erkannt wird nur am Inhalt: ein JSON-Objekt mit
/// genau den Schlüsseln `version` (Ganzzahl) und `entries` (Objekt), so wie [`save`] schreibt; damit ist jede Datei mit
/// einer Farbrolle (oder sonst einem Schlüssel) ausgeschlossen.
///
/// Gegen ein gleichzeitiges `rename` von Noctalia: `palette.json` wird einmal geöffnet (ohne symbolischen Link zu folgen,
/// ohne zu blockieren, nur eine gewöhnliche Datei bis [`MAX_FILE_BYTES`]), und genau die gelesenen Bytes werden geprüft
/// und ohne Ersetzen nach `frecency.json` geschrieben (Temp-Datei 0600, `fsync`, `hard_link`, das bei einer vorhandenen
/// Datei scheitert), danach der Ordner `fsync`t. Dann wird die alte Datei nicht per Namen entfernt, sondern über ihren
/// eigenen Deskriptor geleert ([`Legacy::empty`], Lesart L54), und nur, wenn der Pfad noch auf sie zeigt (Gerät und
/// Inode) und sie seit dem Lesen nicht an Ort und Stelle geändert wurde (Länge und `mtime` am Deskriptor). Linux kennt
/// kein „Entfernen nur bei dieser Inode“: ein `unlink` nach der Prüfung hätte eine Farbdatei getroffen, die Noctalia genau
/// dazwischen per `rename` hinlegt. Das Leeren trifft dann nur die alte, schon verdrängte Inode. `palette.json` bleibt
/// als leere Datei stehen; für `dictate-ui` und das Einstellungsfenster gilt dann wie bei einer fehlenden Datei Tokyo
/// Night, bis Noctalia neu schreibt. Grenze: das gilt, solange andere Schreiber per `rename` ersetzen; wer genau zwischen
/// Prüfung und Leeren an Ort und Stelle in dieselbe Inode schreibt, würde mit geleert. Eine Farbdatei wird so nie
/// verschoben oder entfernt, und der Inhalt der Häufigkeit geht nie verloren.
///
/// Läuft unabhängig davon, ob Palette oder Häufigkeit an sind (Lesart L48 in 6h): sie rettet nur Daten des Daemons vor
/// dem nächsten Schreiben von Noctalia, gelesen wird nichts in den Speicher der Häufigkeit. Ins Log kommen nur Pfad und
/// Ergebnis, nie der Inhalt.
pub fn adopt_legacy(dir: &Path) -> Result<Adoption> {
    adopt_legacy_with(dir, || {}, || {})
}

/// [`adopt_legacy`] mit einem Haken direkt vor der Prüfung, ob `palette.json` noch dieselbe Datei ist (Tests ersetzen
/// sie dort wie Noctalia).
fn adopt_legacy_with(dir: &Path, before_check: impl FnOnce(), before_empty: impl FnOnce()) -> Result<Adoption> {
    let old = dir.join(LEGACY_FILE_NAME);
    let new = dir.join(FILE_NAME);
    let legacy = match read_legacy(&old)? {
        LegacyRead::Absent => return Ok(Adoption::Absent),
        LegacyRead::NotFrecency => return Ok(Adoption::NotFrecency),
        LegacyRead::Frecency(l) => l,
    };
    match std::fs::symlink_metadata(&new) {
        Ok(_) => {
            tracing::warn!("palette: {} looks like an old frecency file, but {} exists; both left as they are", old.display(), new.display());
            return Ok(Adoption::BothPresent);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("checking {}", new.display())),
    }
    match write_new(&new, &legacy.bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(Adoption::BothPresent),
        Err(e) => return Err(e).with_context(|| format!("writing {}", new.display())),
    }
    // erst den neuen Eintrag dauerhaft machen, dann die alte Datei leeren: ein Absturz dazwischen verliert nichts
    sync_dir(dir);
    before_check();
    if !legacy.unchanged_at(&old) {
        tracing::warn!("palette: moved the old frecency file to {}; {} was replaced meanwhile and stays", new.display(), old.display());
        return Ok(Adoption::MovedButReplaced);
    }
    before_empty();
    if let Err(e) = legacy.empty() {
        tracing::warn!("palette: moved the frecency file to {}, but could not empty {} ({e}); it stays", new.display(), old.display());
        return Ok(Adoption::MovedNotEmptied);
    }
    tracing::info!("palette: moved the frecency file from {} to {} (old file emptied)", old.display(), new.display());
    Ok(Adoption::Moved)
}

/// Eine alte Häufigkeit unter `palette.json`, einmal geöffnet und gelesen; der Deskriptor bleibt offen, bis die Datei
/// geleert ist (über ihn wird geleert, und solange er offen ist, wird die Inode-Nummer nicht neu vergeben)
struct Legacy {
    file: std::fs::File,
    meta: std::fs::Metadata,
    bytes: Vec<u8>,
}

enum LegacyRead {
    Absent,
    NotFrecency,
    /// geboxt: `Metadata` ist groß, die anderen Fälle leer
    Frecency(Box<Legacy>),
}

/// `palette.json` öffnen (ohne symbolischem Link zu folgen, ohne zu blockieren, nur eine gewöhnliche Datei bis
/// [`MAX_FILE_BYTES`]) und am gelesenen Inhalt erkennen ([`is_frecency_doc`])
fn read_legacy(old: &Path) -> Result<LegacyRead> {
    use std::io::Read;
    let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC).open(old) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LegacyRead::Absent),
        // ELOOP: ein symbolischer Link; nie folgen, nie anfassen
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Ok(LegacyRead::NotFrecency),
        Err(e) => return Err(e).with_context(|| format!("opening {}", old.display())),
    };
    let meta = file.metadata().with_context(|| format!("reading {}", old.display()))?;
    if !meta.file_type().is_file() || meta.len() > MAX_FILE_BYTES {
        return Ok(LegacyRead::NotFrecency);
    }
    let mut bytes = Vec::new();
    (&file).take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).with_context(|| format!("reading {}", old.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES || !is_frecency_doc(&bytes) {
        return Ok(LegacyRead::NotFrecency);
    }
    Ok(LegacyRead::Frecency(Box::new(Legacy { file, meta, bytes })))
}

impl Legacy {
    /// Zeigt der Pfad noch auf dieselbe Datei (Gerät und Inode wie der offene Deskriptor), und ist sie seit dem Lesen
    /// nicht an Ort und Stelle geändert (Länge und `mtime` am Deskriptor)? Restfenster: ersetzt Noctalia die Datei genau
    /// zwischen dieser Prüfung und dem `unlink`, fehlt die Farbdatei bis zum nächsten Schreiben von Noctalia.
    fn unchanged_at(&self, old: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        let m = &self.meta;
        std::fs::symlink_metadata(old).is_ok_and(|n| n.dev() == m.dev() && n.ino() == m.ino())
            && self.file.metadata().is_ok_and(|n| n.len() == m.len() && n.mtime() == m.mtime() && n.mtime_nsec() == m.mtime_nsec())
    }
}

impl Legacy {
    /// Genau die gelesene Datei leeren (Lesart L54): schreibend wiedergeöffnet über den eigenen Deskriptor
    /// (`/proc/self/fd`, nie über den Namen, erst nach der Erkennung, damit eine Farbdatei nie zum Schreiben geöffnet
    /// wird), geprüft auf dieselbe Inode und genau einen Namen (eine zweite Verknüpfung würde mit geleert), dann
    /// Länge 0 und `fsync`. Scheitert das, bleibt die Datei unberührt und der Fehler geht an den Aufrufer.
    fn empty(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;
        let me = format!("/proc/self/fd/{}", self.file.as_raw_fd());
        let w = std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC).open(me)?;
        let m = w.metadata()?;
        if m.dev() != self.meta.dev() || m.ino() != self.meta.ino() {
            return Err(std::io::Error::other("reopened a different file"));
        }
        if m.nlink() > 1 {
            return Err(std::io::Error::other("the file has more than one name"));
        }
        w.set_len(0)?;
        w.sync_all()
    }
}

/// Was [`remove_legacy`] mit `palette.json` getan hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyRemoval {
    /// Keine alte Häufigkeit (keine Datei, die Farbdatei von Noctalia, ein Link, etwas anderes): unberührt.
    Nothing,
    /// Die alte Häufigkeit ist geleert (`palette.json` bleibt als leere Datei, Lesart L54).
    Emptied,
    /// Sie wurde während des Löschens ersetzt oder geändert (Noctalia) und bleibt.
    Replaced,
}

/// Beim Löschen der Häufigkeit ([`Frecency::clear`]): eine alte Häufigkeit unter `palette.json` in `dir` mit
/// leeren (über den Deskriptor wie bei [`adopt_legacy`], Lesart L54), erkannt wie bei [`adopt_legacy`] (genau `version` und `entries`, nie eine Farbdatei, nie ein Link), mit
/// derselben Vorsicht gegen ein gleichzeitiges `rename` von Noctalia. Ins Log nur Pfad und Ergebnis.
pub fn remove_legacy(dir: &Path) -> Result<LegacyRemoval> {
    remove_legacy_with(dir, || {}, || {})
}

/// [`remove_legacy`] mit einem Haken direkt vor der Prüfung, ob `palette.json` noch dieselbe Datei ist.
fn remove_legacy_with(dir: &Path, before_check: impl FnOnce(), before_empty: impl FnOnce()) -> Result<LegacyRemoval> {
    let old = dir.join(LEGACY_FILE_NAME);
    let LegacyRead::Frecency(legacy) = read_legacy(&old)? else { return Ok(LegacyRemoval::Nothing) };
    before_check();
    if !legacy.unchanged_at(&old) {
        tracing::warn!("palette: {} was replaced while clearing the frecency and stays", old.display());
        return Ok(LegacyRemoval::Replaced);
    }
    before_empty();
    // bewusst ein Fehler (nicht nur eine Warnung): bliebe die alte Häufigkeit stehen, holte der nächste Start sie nach
    // dem Löschen von `frecency.json` zurück; „Clear“ soll das melden
    legacy.empty().with_context(|| format!("emptying the old frecency file {}", old.display()))?;
    tracing::info!("palette: emptied the old frecency file {} with the frecency", old.display());
    Ok(LegacyRemoval::Emptied)
}

/// Genau die Form, die [`save`] schreibt: ein Objekt mit nur `version` (Ganzzahl) und `entries` (Objekt).
fn is_frecency_doc(bytes: &[u8]) -> bool {
    let Ok(serde_json::Value::Object(doc)) = serde_json::from_slice::<serde_json::Value>(bytes) else { return false };
    doc.len() == 2 && doc.get("version").is_some_and(serde_json::Value::is_u64) && doc.get("entries").is_some_and(serde_json::Value::is_object)
}

/// Legt `path` mit `bytes` an, ohne eine vorhandene Datei zu ersetzen (`AlreadyExists`): Temp-Datei im selben Ordner
/// (0600, exklusiv), `fsync`, `hard_link` auf den Zielnamen, Temp-Name entfernen.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = temp_name(path);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::hard_link(&tmp, path)
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Temp-Name neben `path` (`.<name>.tmp-<pid>-<n>`).
fn temp_name(path: &Path) -> PathBuf {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dir.join(format!(".{name}.tmp-{}-{}", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed)))
}

/// Der Eintrag im Ordner soll einen Absturz überleben; scheitert das, bleibt die Datei trotzdem gültig.
fn sync_dir(dir: &Path) {
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Schreibt `bytes` nach `path`: Ordner 0700 anlegen, Temp-Datei im selben Ordner (0600, exklusiv angelegt) schreiben,
/// `fsync`, über `path` umbenennen, Ordner `fsync`en.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let tmp = temp_name(path);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return result;
    }
    sync_dir(dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const NOW: i64 = 1_800_000_000;

    fn open(t: &TempDir) -> Frecency {
        Frecency::open(t.join("state/dictate/frecency.json"), FrecencySettings::default())
    }

    fn with_max(t: &TempDir, max: usize) -> Frecency {
        Frecency::open(t.join("state/dictate/frecency.json"), FrecencySettings { enabled: true, max_entries: max })
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn weights_follow_the_age_buckets() {
        assert_eq!(weight(-100), 4.0, "Zukunft zählt als jetzt");
        assert_eq!(weight(0), 4.0);
        assert_eq!(weight(4 * HOUR - 1), 4.0);
        assert_eq!(weight(4 * HOUR), 2.0);
        assert_eq!(weight(DAY - 1), 2.0);
        assert_eq!(weight(DAY), 1.0);
        assert_eq!(weight(WEEK - 1), 1.0);
        assert_eq!(weight(WEEK), 0.5);
        assert_eq!(weight(400 * DAY), 0.5);
    }

    #[test]
    fn score_is_count_times_age_weight() {
        let t = TempDir::new("frec-score");
        let f = open(&t);
        assert_eq!(f.score("launch_app", "firefox.desktop", NOW), 0.0);
        for i in 0..3 {
            f.record("launch_app", "firefox.desktop", NOW + i).unwrap();
        }
        let last = NOW + 2;
        assert_eq!(f.score("launch_app", "firefox.desktop", last), 12.0, "3 × 4");
        assert_eq!(f.score("launch_app", "firefox.desktop", last + 5 * HOUR), 6.0, "3 × 2");
        assert_eq!(f.score("launch_app", "firefox.desktop", last + 2 * DAY), 3.0, "3 × 1");
        assert_eq!(f.score("launch_app", "firefox.desktop", last + 30 * DAY), 1.5, "3 × 0,5");
        assert_eq!(f.score("launch_app", "other.desktop", last), 0.0);
        assert_eq!(f.score("open_project", "firefox.desktop", last), 0.0, "die Absicht gehört zum Schlüssel");
        assert_eq!(f.score("launch_app", "firefox.desktop", last - 1000), 12.0, "eine Uhr, die zurückspringt, schadet nicht");
        // jede Wahl setzt die letzte Nutzung neu
        f.record("launch_app", "firefox.desktop", last + 10 * DAY).unwrap();
        assert_eq!(f.score("launch_app", "firefox.desktop", last + 10 * DAY), 16.0);
        assert!(f.score_now("launch_app", "firefox.desktop") > 0.0);
    }

    #[test]
    fn recent_use_beats_old_habit_but_frequency_still_counts() {
        let t = TempDir::new("frec-rank");
        // ein altes, oft gewähltes Ziel: der Zähler steht schon am Höchstwert
        let path = t.write(
            "state/dictate/frecency.json",
            &serde_json::to_string(&Doc {
                version: 1,
                entries: BTreeMap::from([("launch_app:old.desktop".to_string(), Entry { count: MAX_COUNT, last: NOW - 90 * DAY })]),
            })
            .unwrap(),
        );
        let f = Frecency::open(path, FrecencySettings::default());
        f.record("launch_app", "old.desktop", NOW - 89 * DAY).unwrap();
        for _ in 0..13 {
            f.record("launch_app", "new.desktop", NOW - 60).unwrap();
        }
        f.record("launch_app", "once.desktop", NOW - 60).unwrap();
        let s = |id: &str| f.score("launch_app", id, NOW);
        assert_eq!(s("old.desktop"), 50.0, "der Zähler sättigt bei {MAX_COUNT}");
        assert!(s("new.desktop") > s("old.desktop"), "13 × 4 gegen 100 × 0,5");
        assert!(s("old.desktop") > s("once.desktop"));
    }

    #[test]
    fn persists_and_reloads() {
        let t = TempDir::new("frec-persist");
        {
            let f = open(&t);
            f.record("launch_app", "firefox.desktop", NOW).unwrap();
            f.record("launch_app", "firefox.desktop", NOW + 1).unwrap();
            f.record("open_project", "/home/git/blub/dictate", NOW + 2).unwrap();
        }
        let f = open(&t);
        assert_eq!(f.len(), 2);
        assert_eq!(f.score("launch_app", "firefox.desktop", NOW + 2), 8.0);
        assert_eq!(f.score("open_project", "/home/git/blub/dictate", NOW + 2), 4.0);
        let text = std::fs::read_to_string(f.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["entries"]["launch_app:firefox.desktop"], serde_json::json!({"count": 2, "last": NOW + 1}));
        assert_eq!(v["entries"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn file_holds_only_target_ids_counts_and_times() {
        let t = TempDir::new("frec-privacy");
        let f = open(&t);
        f.record("launch_app", "firefox.desktop", NOW).unwrap();
        f.record("focus_window", "kitty", NOW).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap();
        let mut top: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        top.sort();
        assert_eq!(top, ["entries", "version"]);
        for (_, e) in v["entries"].as_object().unwrap() {
            let mut fields: Vec<_> = e.as_object().unwrap().keys().cloned().collect();
            fields.sort();
            assert_eq!(fields, ["count", "last"]);
        }
    }

    #[test]
    fn rejects_unknown_intents_and_bad_targets() {
        let t = TempDir::new("frec-validate");
        let f = open(&t);
        assert!(f.record("rm_rf", "x", NOW).is_err());
        assert!(f.record("launch_app", "", NOW).is_err());
        assert!(f.record("launch_app", "a\nb", NOW).is_err());
        assert!(f.record("launch_app", &"x".repeat(MAX_TARGET_BYTES + 1), NOW).is_err());
        assert!(f.is_empty());
        assert!(!f.path().exists(), "nichts geschrieben");
        f.record("launch_app", &"x".repeat(MAX_TARGET_BYTES), NOW).unwrap();
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn oldest_entries_are_evicted_over_the_limit() {
        let t = TempDir::new("frec-evict");
        let f = with_max(&t, 3);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        f.record("launch_app", "b.desktop", NOW + 10).unwrap();
        f.record("launch_app", "c.desktop", NOW + 20).unwrap();
        f.record("launch_app", "d.desktop", NOW + 30).unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(f.score("launch_app", "a.desktop", NOW + 30), 0.0, "der älteste ist weg");
        assert!(f.score("launch_app", "b.desktop", NOW + 30) > 0.0);
        // `b` erneut gewählt: jetzt ist `c` der älteste
        f.record("launch_app", "b.desktop", NOW + 40).unwrap();
        f.record("launch_app", "e.desktop", NOW + 50).unwrap();
        assert_eq!(f.score("launch_app", "c.desktop", NOW + 50), 0.0);
        assert!(f.score("launch_app", "b.desktop", NOW + 50) > 0.0);
        assert_eq!(f.len(), 3);
        // bei Gleichstand der Zeit fliegt der mit dem kleineren Zähler raus
        let g = with_max(&TempDir::new("frec-evict-tie"), 2);
        g.record("launch_app", "x.desktop", NOW).unwrap();
        g.record("launch_app", "x.desktop", NOW).unwrap();
        g.record("launch_app", "y.desktop", NOW).unwrap();
        g.record("launch_app", "z.desktop", NOW).unwrap();
        assert!(g.score("launch_app", "x.desktop", NOW) > 0.0);
        assert_eq!(g.len(), 2);
        // die Obergrenze gilt auch beim Laden einer zu großen Datei
        let reopened = with_max(&t, 2);
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn default_limit_is_500() {
        let t = TempDir::new("frec-500");
        let f = open(&t);
        // viele Einträge im Speicher anlegen, ohne 600-mal zu schreiben: über die Datei laden
        let entries: BTreeMap<String, Entry> = (0..600).map(|i| (format!("launch_app:app{i}.desktop"), Entry { count: 1, last: NOW + i })).collect();
        std::fs::create_dir_all(f.path().parent().unwrap()).unwrap();
        std::fs::write(f.path(), serde_json::to_string(&Doc { version: 1, entries }).unwrap()).unwrap();
        let f = open(&t);
        assert_eq!(f.len(), DEFAULT_MAX_ENTRIES);
        assert_eq!(f.score("launch_app", "app0.desktop", NOW + 600), 0.0);
        assert!(f.score("launch_app", "app599.desktop", NOW + 600) > 0.0);
        f.record("launch_app", "fresh.desktop", NOW + 700).unwrap();
        assert_eq!(f.len(), DEFAULT_MAX_ENTRIES);
        assert_eq!(f.score("launch_app", "app100.desktop", NOW + 700), 0.0, "der älteste Rest fliegt beim nächsten Schreiben raus");
    }

    #[test]
    fn writes_with_owner_only_permissions() {
        let t = TempDir::new("frec-perms");
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        assert_eq!(mode(f.path()), 0o600);
        assert_eq!(mode(f.path().parent().unwrap()), 0o700);
        // eine vorhandene Datei mit weiteren Rechten wird beim Schreiben ersetzt und damit enger
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        f.record("launch_app", "a.desktop", NOW + 1).unwrap();
        assert_eq!(mode(f.path()), 0o600);
    }

    #[test]
    fn write_is_atomic_via_temp_file_and_rename() {
        let t = TempDir::new("frec-atomic");
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        let first = std::fs::metadata(f.path()).unwrap().ino();
        f.record("launch_app", "b.desktop", NOW + 1).unwrap();
        let second = std::fs::metadata(f.path()).unwrap().ino();
        assert_ne!(first, second, "eine neue Datei ersetzt die alte (rename), die alte wird nicht überschrieben");
        let names: Vec<_> = std::fs::read_dir(f.path().parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["frecency.json"], "keine Temp-Reste");
        // Ein Leser sieht zu jedem Zeitpunkt eine vollständige Datei
        let path = f.path().to_path_buf();
        let reader = std::thread::spawn(move || {
            let mut seen = 0;
            for _ in 0..400 {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    serde_json::from_str::<serde_json::Value>(&text).expect("nie eine halbe Datei");
                    seen += 1;
                }
            }
            seen
        });
        for i in 0..60 {
            f.record("launch_app", &format!("w{i}.desktop"), NOW + 10 + i).unwrap();
        }
        assert!(reader.join().unwrap() > 0);
    }

    #[test]
    fn failed_write_keeps_the_old_file_and_leaves_no_temp_file() {
        // als root greift 0500 nicht
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let t = TempDir::new("frec-fail");
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        let before = std::fs::read(f.path()).unwrap();
        let dir = f.path().parent().unwrap().to_path_buf();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let err = f.record("launch_app", "b.desktop", NOW + 1);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(err.is_err());
        assert_eq!(std::fs::read(f.path()).unwrap(), before);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(f.score("launch_app", "b.desktop", NOW + 1) > 0.0, "der Zähler im Speicher bleibt");
    }

    #[test]
    fn corrupt_files_start_empty_and_are_replaced_on_the_next_write() {
        let bad: &[&str] = &[
            "",
            "not json",
            "{",
            "[]",
            "{\"version\":1}",
            "{\"version\":1,\"entries\":[1,2]}",
            "{\"version\":1,\"entries\":{\"launch_app:a\":{\"count\":\"x\",\"last\":1}}}",
            "{\"version\":1,\"entries\":{\"launch_app:a\":{\"count\":-1,\"last\":1}}}",
            "{\"version\":2,\"entries\":{}}",
            "\u{0}\u{0}\u{0}",
        ];
        for (i, content) in bad.iter().enumerate() {
            let t = TempDir::new(&format!("frec-corrupt{i}"));
            let path = t.write("state/dictate/frecency.json", content);
            let f = Frecency::open(path.clone(), FrecencySettings::default());
            assert!(f.is_empty(), "{content:?}");
            f.record("launch_app", "a.desktop", NOW).unwrap();
            assert_eq!(Frecency::open(path, FrecencySettings::default()).len(), 1, "{content:?}");
        }
        // Binärmüll (kein UTF-8)
        let t = TempDir::new("frec-corrupt-bin");
        let path = t.join("frecency.json");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(Frecency::open(path, FrecencySettings::default()).is_empty());
        // ein Ordner statt einer Datei: unlesbar, kein Absturz
        let t = TempDir::new("frec-corrupt-dir");
        let path = t.mkdir("frecency.json");
        assert!(Frecency::open(path, FrecencySettings::default()).is_empty());
    }

    #[test]
    fn loading_drops_bad_entries_and_clamps_values() {
        let t = TempDir::new("frec-sanitize");
        let path = t.write(
            "frecency.json",
            r#"{"version":1,"entries":{
                "launch_app:ok.desktop":{"count":7,"last":100},
                "nocolon":{"count":1,"last":1},
                ":notarget":{"count":1,"last":1},
                "launch_app:":{"count":1,"last":1},
                "launch_app:huge":{"count":4000000000,"last":5},
                "launch_app:zero":{"count":0,"last":-5},
                "launch_app:ctl\u0007":{"count":1,"last":1}
            },"extra":true}"#,
        );
        let f = Frecency::open(path, FrecencySettings::default());
        assert_eq!(f.len(), 3);
        assert_eq!(f.score("launch_app", "ok.desktop", 100), 28.0);
        assert_eq!(f.score("launch_app", "huge", 5), f64::from(MAX_COUNT) * 4.0);
        assert_eq!(f.score("launch_app", "zero", 0), 4.0, "Zähler mindestens 1, Zeit nicht negativ");
    }

    /// Einträge von vor W5 heißen nach dem Laden wie unter niri; was es nicht mehr gibt, fällt weg (Vorlage E2 e).
    #[test]
    fn awesome_names_are_mapped_on_load() {
        let t = TempDir::new("frec-migrate");
        let path = t.write(
            "frecency.json",
            r#"{"version":1,"entries":{
                "view_tag:3":{"count":2,"last":100},
                "view_workspace:3":{"count":1,"last":200},
                "move_window_to_tag:2":{"count":4,"last":50},
                "previous_tag:previous_tag":{"count":3,"last":60},
                "set_layout:tile":{"count":9,"last":70},
                "toggle_window:sticky":{"count":1,"last":1},
                "toggle_window:ontop":{"count":1,"last":1},
                "toggle_window:floating":{"count":5,"last":80},
                "show_widget:claude_usage":{"count":1,"last":1},
                "show_widget:emoji":{"count":2,"last":90},
                "no_such_intent:x":{"count":1,"last":1},
                "view_tag:9":{"count":99,"last":10},
                "view_workspace:9":{"count":99,"last":5}
            }}"#,
        );
        let f = Frecency::open(path.clone(), FrecencySettings::default());
        assert_eq!(f.len(), 6, "{:?}", f.lock().keys().collect::<Vec<_>>());
        // zusammengeführt: Zähler addiert (gedeckelt), die jüngere Zeit
        assert_eq!(f.score("view_workspace", "3", 200), 3.0 * 4.0);
        assert_eq!(f.score("view_workspace", "9", 10), f64::from(MAX_COUNT) * 4.0);
        assert_eq!(f.score("move_window_to_workspace", "2", 50), 16.0);
        assert_eq!(f.score("previous_workspace", "previous_workspace", 60), 12.0);
        assert_eq!(f.score("toggle_window", "floating", 80), 20.0);
        assert_eq!(f.score("show_widget", "emoji", 90), 8.0);
        for (intent, target) in [("view_tag", "3"), ("set_layout", "tile"), ("toggle_window", "sticky"), ("show_widget", "claude_usage")] {
            assert_eq!(f.score(intent, target, 100), 0.0, "{intent}:{target}");
        }
        // der nächste Schreibvorgang legt nur die neuen Namen ab, Version 1 bleibt
        f.record("lock", "lock", 300).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["version"], 1, "{text}");
        for old in ["view_tag", "move_window_to_tag", "previous_tag", "set_layout", "sticky", "ontop", "claude_usage", "no_such_intent"] {
            assert!(!text.contains(old), "{old} in {text}");
        }
        assert!(text.contains("view_workspace:3") && text.contains("previous_workspace:previous_workspace"), "{text}");
        // die alten Namen sind keine gültigen Absichten mehr
        assert!(f.record("view_tag", "3", 300).is_err());
        for (old, new) in RENAMED {
            assert!(catalog::spec(old).is_none() && catalog::spec(new).is_some(), "{old} → {new}");
        }
    }

    #[test]
    fn clear_removes_entries_and_file() {
        let t = TempDir::new("frec-clear");
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        assert!(f.path().exists());
        f.clear().unwrap();
        assert!(f.is_empty());
        assert!(!f.path().exists());
        assert_eq!(f.score("launch_app", "a.desktop", NOW), 0.0);
        f.clear().unwrap();
        assert!(open(&t).is_empty());
        // danach geht es weiter
        f.record("launch_app", "b.desktop", NOW).unwrap();
        assert_eq!(open(&t).len(), 1);
    }

    #[test]
    fn disabled_reads_counts_and_writes_nothing() {
        let t = TempDir::new("frec-off");
        {
            let f = open(&t);
            f.record("launch_app", "a.desktop", NOW).unwrap();
        }
        let before = std::fs::read(t.join("state/dictate/frecency.json")).unwrap();
        let f = Frecency::open(t.join("state/dictate/frecency.json"), FrecencySettings { enabled: false, ..FrecencySettings::default() });
        assert!(!f.enabled());
        assert!(f.is_empty(), "nichts gelesen");
        assert_eq!(f.score("launch_app", "a.desktop", NOW), 0.0);
        f.record("launch_app", "b.desktop", NOW).unwrap();
        assert!(f.is_empty());
        assert_eq!(std::fs::read(t.join("state/dictate/frecency.json")).unwrap(), before, "die Datei bleibt unberührt");
        // löschen geht trotzdem ausdrücklich
        f.clear().unwrap();
        assert!(!t.join("state/dictate/frecency.json").exists());
    }

    #[test]
    fn concurrent_records_all_land() {
        let t = TempDir::new("frec-threads");
        let f = std::sync::Arc::new(open(&t));
        let handles: Vec<_> = (0..4)
            .map(|n| {
                let f = std::sync::Arc::clone(&f);
                std::thread::spawn(move || {
                    for i in 0..10 {
                        f.record("launch_app", &format!("t{n}-{i}.desktop"), NOW + i).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(f.len(), 40);
        assert_eq!(open(&t).len(), 40);
    }

    #[test]
    fn default_path_follows_xdg_state_home() {
        // nur die Form prüfen: die Umgebung des Tests wird nicht verändert
        let p = default_path();
        assert!(p.ends_with("dictate/frecency.json"), "{p:?}");
        assert_eq!(p.parent(), crate::pipeline::history::default_path().parent());
    }

    /// Farbdatei der Noctalia-Vorlage mit allen Rollen (dieselbe Fixture wie in `tests/wayland`).
    const COLORS: &str = include_str!("../../../tests/wayland/palette-tokyo.json");
    const OLD: &str =
        "{\n  \"version\": 1,\n  \"entries\": {\n    \"launch_app:firefox.desktop\": {\n      \"count\": 7,\n      \"last\": 1800000000\n    }\n  }\n}\n";

    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        n.sort();
        n
    }

    #[test]
    fn adopt_legacy_moves_an_old_frecency_file_byte_for_byte() {
        let t = TempDir::new("frec-adopt");
        let old = t.write("state/dictate/palette.json", OLD);
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o644)).unwrap();
        let dir = old.parent().unwrap();
        assert_eq!(adopt_legacy(dir).unwrap(), Adoption::Moved);
        assert_eq!(names(dir), ["frecency.json", "palette.json"], "keine Temp-Reste");
        assert_eq!(std::fs::metadata(&old).unwrap().len(), 0, "die alte Datei ist geleert, nicht entfernt (L54)");
        let new = dir.join(FILE_NAME);
        assert_eq!(std::fs::read_to_string(&new).unwrap(), OLD);
        assert_eq!(mode(&new), 0o600);
        let f = Frecency::open(new, FrecencySettings::default());
        assert_eq!(f.score("launch_app", "firefox.desktop", NOW), 28.0, "7 × 4: die Häufigkeit gilt weiter");
        // ein zweiter Start findet keine Häufigkeit mehr (die leere Datei ist keine)
        assert_eq!(adopt_legacy(dir).unwrap(), Adoption::NotFrecency);
        assert!(crate::ui::palette::Palette::parse("").is_err(), "eine leere palette.json ist für die Farben ein Rückfall");
    }

    #[test]
    fn adopt_legacy_never_touches_the_color_file() {
        let t = TempDir::new("frec-adopt-colors");
        let colors = t.write("state/dictate/palette.json", COLORS);
        let dir = colors.parent().unwrap();
        let ino = std::fs::metadata(&colors).unwrap().ino();
        assert_eq!(adopt_legacy(dir).unwrap(), Adoption::NotFrecency);
        assert_eq!(names(dir), ["palette.json"]);
        assert_eq!(std::fs::read_to_string(&colors).unwrap(), COLORS);
        assert_eq!(std::fs::metadata(&colors).unwrap().ino(), ino, "nicht neu geschrieben");
        assert!(crate::ui::palette::Palette::parse(COLORS).is_ok());
        // auch neben einer Häufigkeit unter dem neuen Namen
        t.write("state/dictate/frecency.json", OLD);
        assert_eq!(adopt_legacy(dir).unwrap(), Adoption::NotFrecency);
        assert_eq!(std::fs::read_to_string(&colors).unwrap(), COLORS);
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), OLD);
    }

    #[test]
    fn adopt_legacy_leaves_both_files_when_the_new_one_exists() {
        let t = TempDir::new("frec-adopt-both");
        let old = t.write("state/dictate/palette.json", OLD);
        let newer = r#"{"version":1,"entries":{"launch_app:a.desktop":{"count":1,"last":1}}}"#;
        let new = t.write("state/dictate/frecency.json", newer);
        assert_eq!(adopt_legacy(old.parent().unwrap()).unwrap(), Adoption::BothPresent);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), OLD);
        assert_eq!(std::fs::read_to_string(&new).unwrap(), newer);
        assert_eq!(names(old.parent().unwrap()), ["frecency.json", "palette.json"]);
        // ein Ordner oder ein Link unter dem neuen Namen zählt ebenso als vorhanden
        std::fs::remove_file(&new).unwrap();
        std::os::unix::fs::symlink("/nonexistent", &new).unwrap();
        assert_eq!(adopt_legacy(old.parent().unwrap()).unwrap(), Adoption::BothPresent);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), OLD);
    }

    #[test]
    fn adopt_legacy_recognizes_only_the_exact_frecency_shape() {
        let t = TempDir::new("frec-adopt-shape");
        let dir = t.mkdir("state/dictate");
        for content in [
            // gemischt: Häufigkeit mit einer Farbrolle oder dem Modus
            r##"{"version":1,"entries":{},"primary":"#7aa2f7"}"##,
            r#"{"version":1,"entries":{},"mode":"dark"}"#,
            // ein weiterer Schlüssel, falsche Typen, fehlende Teile
            r#"{"version":1,"entries":{},"note":"x"}"#,
            r#"{"version":"1","entries":{}}"#,
            r#"{"version":-1,"entries":{}}"#,
            r#"{"version":1.5,"entries":{}}"#,
            r#"{"version":1,"entries":[]}"#,
            r#"{"version":1}"#,
            r#"{"entries":{}}"#,
            r#"[{"version":1,"entries":{}}]"#,
            "not json",
            "",
            "\u{fffd}\u{0}",
        ] {
            let p = t.write("state/dictate/palette.json", content);
            assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency, "{content:?}");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), content);
            assert_eq!(names(&dir), ["palette.json"], "{content:?}");
        }
        // die leere Häufigkeit (nach dem ersten Schreiben ohne Einträge) ist eine
        t.write("state/dictate/palette.json", r#"{"version":1,"entries":{}}"#);
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::Moved);
    }

    #[test]
    fn adopt_legacy_keeps_a_color_file_that_replaces_the_old_one_meanwhile() {
        let t = TempDir::new("frec-adopt-race");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        // Noctalia schreibt genau während der Übernahme: Temp-Datei und `rename` über `palette.json`
        let replace = || {
            let tmp = t.write("state/dictate/.palette.json.noctalia", COLORS);
            std::fs::rename(tmp, dir.join(LEGACY_FILE_NAME)).unwrap();
        };
        assert_eq!(adopt_legacy_with(&dir, replace, || {}).unwrap(), Adoption::MovedButReplaced);
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS, "die Farbdatei bleibt");
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), OLD, "die Häufigkeit ist gerettet");
        assert_eq!(names(&dir), ["frecency.json", "palette.json"], "keine Temp-Reste");

        // an Ort und Stelle überschrieben (gleiche Inode, andere Länge): ebenfalls stehen lassen
        let t = TempDir::new("frec-adopt-inplace");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let overwrite = || std::fs::write(dir.join(LEGACY_FILE_NAME), COLORS).unwrap();
        assert_eq!(adopt_legacy_with(&dir, overwrite, || {}).unwrap(), Adoption::MovedButReplaced);
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS);
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), OLD);
    }

    #[test]
    fn adopt_legacy_ignores_links_directories_fifos_and_large_files() {
        let t = TempDir::new("frec-adopt-kinds");
        let dir = t.mkdir("state/dictate");
        let old = dir.join(LEGACY_FILE_NAME);
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::Absent);
        // ein Link auf eine Häufigkeit: nie folgen, nichts verschieben
        let target = t.write("elsewhere.json", OLD);
        std::os::unix::fs::symlink(&target, &old).unwrap();
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency);
        assert!(std::fs::symlink_metadata(&old).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), OLD);
        assert_eq!(names(&dir), ["palette.json"]);
        std::fs::remove_file(&old).unwrap();
        // ein Ordner
        std::fs::create_dir(&old).unwrap();
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency);
        assert!(old.is_dir());
        std::fs::remove_dir(&old).unwrap();
        // eine FIFO ohne Schreiber: kein Blockieren
        let c = std::ffi::CString::new(old.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: gültiger, nullterminierter Pfad
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency);
        std::fs::remove_file(&old).unwrap();
        // zu groß, obwohl der Anfang passt
        let big = format!("{{\"version\":1,\"entries\":{{}}}}{}", " ".repeat(MAX_FILE_BYTES as usize));
        t.write("state/dictate/palette.json", &big);
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency);
        assert_eq!(names(&dir), ["palette.json"]);
    }

    #[test]
    fn key_joins_intent_and_target() {
        assert_eq!(key("launch_app", "firefox.desktop"), "launch_app:firefox.desktop");
        assert_eq!(key("open_project", "/home/git/blub/dictate"), "open_project:/home/git/blub/dictate");
    }

    /// Entscheidung 21: liegen eine alte Häufigkeit unter `palette.json` und `frecency.json` vor (dann zieht nichts
    /// um), löscht `clear` beide; der nächste Start holt nichts zurück. Eine Farbdatei bleibt unberührt.
    #[test]
    fn clear_also_removes_an_old_frecency_under_the_legacy_name() {
        let t = TempDir::new("frec-clear-legacy");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::BothPresent);
        f.clear().unwrap();
        assert_eq!(names(&dir), ["palette.json"], "{:?}", names(&dir));
        assert_eq!(std::fs::metadata(&old).unwrap().len(), 0, "geleert, nicht entfernt (L54)");
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::NotFrecency, "der nächste Start findet nichts");
        assert!(open(&t).is_empty());
        // auch ohne frecency.json
        t.write("state/dictate/palette.json", OLD);
        open(&t).clear().unwrap();
        assert_eq!(names(&dir), ["palette.json"]);
        assert_eq!(std::fs::metadata(&old).unwrap().len(), 0);

        // die Farbdatei bleibt, wie sie ist (gleiche Inode, gleicher Inhalt)
        let colors = t.write("state/dictate/palette.json", COLORS);
        let ino = std::fs::metadata(&colors).unwrap().ino();
        let f = open(&t);
        f.record("launch_app", "a.desktop", NOW).unwrap();
        f.clear().unwrap();
        assert_eq!(names(&dir), ["palette.json"]);
        assert_eq!(std::fs::read_to_string(&colors).unwrap(), COLORS);
        assert_eq!(std::fs::metadata(&colors).unwrap().ino(), ino);
        assert_eq!(remove_legacy(&dir).unwrap(), LegacyRemoval::Nothing);

        // ein Link auf eine Häufigkeit: nie folgen, nichts entfernen
        std::fs::remove_file(&colors).unwrap();
        let target = t.write("elsewhere.json", OLD);
        std::os::unix::fs::symlink(&target, &colors).unwrap();
        open(&t).clear().unwrap();
        assert!(std::fs::symlink_metadata(&colors).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), OLD);

        // eine Häufigkeit unter anderem Namen (Tests, eigene Pfade) räumt keine palette.json daneben ab
        std::fs::remove_file(&colors).unwrap();
        t.write("state/dictate/palette.json", OLD);
        let other = Frecency::open(t.join("state/dictate/other.json"), FrecencySettings::default());
        other.clear().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), OLD);
    }

    #[test]
    fn clear_keeps_a_color_file_that_replaces_the_old_frecency_meanwhile() {
        let t = TempDir::new("frec-clear-race");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let replace = || {
            let tmp = t.write("state/dictate/.palette.json.noctalia", COLORS);
            std::fs::rename(tmp, dir.join(LEGACY_FILE_NAME)).unwrap();
        };
        assert_eq!(remove_legacy_with(&dir, replace, || {}).unwrap(), LegacyRemoval::Replaced);
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS);
        // an Ort und Stelle überschrieben
        t.write("state/dictate/palette.json", OLD);
        let overwrite = || std::fs::write(dir.join(LEGACY_FILE_NAME), COLORS).unwrap();
        assert_eq!(remove_legacy_with(&dir, overwrite, || {}).unwrap(), LegacyRemoval::Replaced);
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS);
        assert_eq!(names(&dir), ["palette.json"]);
    }

    /// Gegenlesen in Block E: hat die alte Datei einen zweiten Namen, würde das Leeren ihn mit treffen; dann bleibt sie,
    /// die Übernahme meldet das ehrlich, „Clear“ scheitert laut (sonst holte der nächste Start sie zurück)
    #[test]
    fn a_second_name_keeps_the_old_file_untouched() {
        let t = TempDir::new("frec-second-name");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let other = t.join("other-name.json");
        std::fs::hard_link(&old, &other).unwrap();
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::MovedNotEmptied);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), OLD);
        assert_eq!(std::fs::read_to_string(&other).unwrap(), OLD);
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), OLD, "die Häufigkeit ist trotzdem gerettet");
        assert_eq!(adopt_legacy(&dir).unwrap(), Adoption::BothPresent);
        assert!(remove_legacy(&dir).is_err());
        assert_eq!(std::fs::read_to_string(&other).unwrap(), OLD, "der zweite Name bleibt unberührt");
    }

    /// Befund der Durchsicht in Block E (Lesart L54): ersetzt Noctalia `palette.json` genau zwischen der Prüfung und dem
    /// zerstörenden Schritt, bleibt die Farbdatei trotzdem byte-gleich (nie per Namen entfernen); die Häufigkeit ist
    /// gerettet bzw. gelöscht
    #[test]
    fn a_color_file_renamed_in_after_the_check_survives() {
        let t = TempDir::new("frec-race-after-check");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let replace = || {
            let tmp = t.write("state/dictate/.palette.json.noctalia", COLORS);
            std::fs::rename(tmp, dir.join(LEGACY_FILE_NAME)).unwrap();
        };
        assert!(matches!(adopt_legacy_with(&dir, || {}, replace).unwrap(), Adoption::Moved | Adoption::MovedButReplaced));
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS, "die Farbdatei bleibt");
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), OLD, "die Häufigkeit ist gerettet");
        assert_eq!(names(&dir), ["frecency.json", "palette.json"]);

        let t = TempDir::new("frec-race-after-check-clear");
        let old = t.write("state/dictate/palette.json", OLD);
        let dir = old.parent().unwrap().to_path_buf();
        let replace = || {
            let tmp = t.write("state/dictate/.palette.json.noctalia", COLORS);
            std::fs::rename(tmp, dir.join(LEGACY_FILE_NAME)).unwrap();
        };
        assert!(matches!(remove_legacy_with(&dir, || {}, replace).unwrap(), LegacyRemoval::Emptied | LegacyRemoval::Replaced));
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_FILE_NAME)).unwrap(), COLORS, "die Farbdatei bleibt");
        assert_eq!(names(&dir), ["palette.json"]);
    }
}
