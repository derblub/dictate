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

//! Lokale Auflösung der Parameter gegen die echten Listen. Nichts aus einem Parameter gelangt als Text in ein argv: ein
//! Suchbegriff gilt erst, wenn er auf genau einen Eintrag einer echten Liste passt, und eingesetzt wird der Name bzw. die
//! volle ID aus dieser Liste, nie der Begriff.
//!
//! - **Unit** (`UnitRef`): gegen `systemctl list-units --all` des System-Managers, dann des User-Managers. Reihenfolge:
//!   voller Name; Name plus `.service`; sonst genau ein Eintrag, dessen Name den Begriff enthält. Mehrere Treffer sind
//!   ein Fehler mit der Liste der Kandidaten (das Modell fragt dann mit einem vollen Namen noch einmal). Eine Unit
//!   in beiden Managern gilt als die des Systems. Eingesetzt wird nur ein Name der Form `<Name>.<Typ>` aus der Liste
//!   (`Term::check_resolved`): eine reine Zahl käme bei `systemctl status` als PID an.
//! - **Container** (`ContainerRef`): gegen `docker ps -a --no-trunc`. Reihenfolge: Name; Anfang der ID (nur Hex, mindestens
//!   vier Zeichen); genau ein Eintrag, dessen Name den Begriff enthält. Eingesetzt wird die volle 64-stellige ID.
//! - **Boot** (`BootOffset`): nur, wenn `journalctl --list-boots` ihn kennt.
//! - **Ping-Ziel** (`PingTarget`): nur ein Name (oder eine Adresse) der Config-Liste, nie der Begriff selbst.
//! - **Alter des Boots** für `since: boot`: Sekunden aus `/proc/uptime`.
//!
//! Die Liste der Boots liest mit `Keep::Tail` (bei einer riesigen Liste zählen die jüngsten), die übrigen mit `Keep::Head`.
//!
//! Die Listen holt der Zwischenspeicher ([`Cache`]) je Sammlung einmal; Fehler (Programm fehlt, Docker nicht erreichbar)
//! werden mit gespeichert, damit nicht jede Probe auf dieselbe Frist wartet. Abbruch und Frist zählen nicht als
//! Fehler der Liste und werden nicht gespeichert.

use super::collect::{exec, ProbeContext, Run};
use super::fs::Fs;
use super::json;
use super::output::{ProbeOutput, Status};
use super::time::Clock;
use crate::diagnose::argv::{resolution, Command, ResolvedUnit};
use crate::diagnose::catalog::Term;
use crate::diagnose::probe::{BootOffset, ContainerRef, PingTarget, UnitRef};
use crate::diagnose::run::Keep;

/// Wie viel eine Liste der Auflösung höchstens liefern darf (Bytes).
pub const LIST_LIMIT: usize = 512 * 1024;
/// Wie viele Kandidaten eine Meldung nennt.
const CANDIDATES_SHOWN: usize = 8;

/// Warum ein Parameter nicht aufgelöst werden konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Gibt es nicht (oder nicht eindeutig): die Meldung ist für das Modell gedacht.
    Unavailable(String),
    /// Die Liste ließ sich nicht holen (Programm scheitert, JSON unlesbar).
    Failed(String),
    TimedOut,
    Cancelled,
}

impl From<ResolveError> for Status {
    fn from(e: ResolveError) -> Status {
        match e {
            ResolveError::Unavailable(m) => Status::Unavailable(m),
            ResolveError::Failed(m) => Status::Failed(m),
            ResolveError::TimedOut => Status::TimedOut,
            ResolveError::Cancelled => Status::Cancelled,
        }
    }
}

/// Eine Unit aus `list-units`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitEntry {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
}

/// Ein Container aus `docker ps -a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerEntry {
    /// Volle ID, 64 Zeichen.
    pub id: String,
    pub names: Vec<String>,
    pub state: String,
    pub image: String,
}

/// Ein Boot aus `journalctl --list-boots`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootEntry {
    pub index: i64,
    pub boot_id: String,
    /// Erster und letzter Eintrag, Sekunden seit der Epoche.
    pub first: i64,
    pub last: i64,
}

/// Der aufgelöste Container: volle ID und Name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContainer {
    pub id: String,
    pub name: String,
}

/// Zeitraum eines Boots für die Proben, die Dateien nach Zeit durchsuchen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootWindow {
    pub index: i64,
    /// Beginn des Boots (erster Journaleintrag).
    pub start: i64,
    /// Ende des Boots: letzter Journaleintrag, beim laufenden Boot jetzt.
    pub end: i64,
    /// Beginn des nächsten Boots, beim laufenden Boot jetzt. Zwischen `end` und `next_start` lag der Absturz, und
    /// was ein Absturz hinterlässt (pstore, die letzten Zeilen des temp-loggers), trägt Zeiten in diesem Fenster.
    pub next_start: i64,
}

/// Zwischenspeicher der Listen einer Sammlung.
#[derive(Debug, Default)]
pub struct Cache {
    units: [Option<Result<Vec<UnitEntry>, ResolveError>>; 2],
    containers: Option<Result<Vec<ContainerEntry>, ResolveError>>,
    boots: Option<Result<Vec<BootEntry>, ResolveError>>,
}

fn keep_in_cache<T>(r: &Result<T, ResolveError>) -> bool {
    !matches!(r, Err(ResolveError::TimedOut | ResolveError::Cancelled))
}

/// Holt die Ausgabe eines Befehls der Auflösung.
fn list(run: &mut Run, command: &Command, what: &str, keep: Keep) -> Result<String, ResolveError> {
    let argv = command.expand(run.probe, &Default::default()).expect("resolution commands have no placeholders");
    let mut scratch = ProbeOutput::new("");
    let cmd = exec(run.ctx, &argv, run.deadline, LIST_LIMIT, super::collect::STDERR_LIMIT, keep, run.cancel, &mut scratch);
    match cmd.problem() {
        Some(Status::TimedOut) => Err(ResolveError::TimedOut),
        Some(Status::Cancelled) => Err(ResolveError::Cancelled),
        Some(Status::Unavailable(m)) => Err(ResolveError::Failed(format!("cannot look up the {what}: {m}"))),
        Some(Status::Failed(m)) => Err(ResolveError::Failed(format!("cannot look up the {what}: {m}"))),
        Some(other) => Err(ResolveError::Failed(format!("cannot look up the {what}: {}", other.label()))),
        None => Ok(cmd.raw_text()),
    }
}

// ---- Units ---------------------------------------------------------------------------------------------------------

fn parse_units(text: &str) -> Vec<UnitEntry> {
    let (items, _incomplete) = json::objects(text);
    items
        .iter()
        .filter_map(|o| {
            let name = json::str_of(o, "unit")?;
            // Nur Namen der vollen Form, die in ein argv dürfen (nicht `-.mount`, nicht ohne Typ)
            Term::Unit.check_resolved(name).ok()?;
            Some(UnitEntry {
                name: name.to_string(),
                load: json::str_of(o, "load").unwrap_or("").to_string(),
                active: json::str_of(o, "active").unwrap_or("").to_string(),
                sub: json::str_of(o, "sub").unwrap_or("").to_string(),
            })
        })
        .collect()
}

fn units<'c>(run: &'c mut Run, user: bool) -> &'c Result<Vec<UnitEntry>, ResolveError> {
    let slot = usize::from(user);
    if run.cache.units[slot].as_ref().is_none_or(|r| !keep_in_cache(r)) {
        let command = if user { &resolution::LIST_USER_UNITS } else { &resolution::LIST_UNITS };
        let result = list(run, command, if user { "units of the user manager" } else { "units of the system manager" }, Keep::Head).map(|t| parse_units(&t));
        run.cache.units[slot] = Some(result);
    }
    run.cache.units[slot].as_ref().expect("just filled")
}

/// Hat der Begriff schon einen Unit-Typ als Endung (`docker.socket`)?
fn has_unit_type(term: &str) -> bool {
    term.rsplit_once('.').is_some_and(|(_, kind)| crate::diagnose::catalog::UNIT_TYPES.contains(&kind))
}

fn shown_list(names: &[&str]) -> String {
    let mut v: Vec<String> = names.iter().take(CANDIDATES_SHOWN).map(|n| n.chars().take(80).collect()).collect();
    if names.len() > CANDIDATES_SHOWN {
        v.push(format!("and {} more", names.len() - CANDIDATES_SHOWN));
    }
    v.join(", ")
}

/// Wählt aus Namen den passenden zu `term`: voller Name, mit `.service`, oder genau ein Teiltreffer.
fn pick_name<'a>(term: &str, names: &[&'a str], kind: &str) -> Result<&'a str, ResolveError> {
    if let Some(n) = names.iter().find(|n| **n == term) {
        return Ok(n);
    }
    if !has_unit_type(term) {
        let with_service = format!("{term}.service");
        if let Some(n) = names.iter().find(|n| **n == with_service) {
            return Ok(n);
        }
    }
    let lower = term.to_lowercase();
    let hits: Vec<&str> = names.iter().copied().filter(|n| n.to_lowercase().contains(&lower)).collect();
    match hits.as_slice() {
        [] => {
            Err(ResolveError::Unavailable(format!("no {kind} matches \"{term}\"; only existing {kind}s can be examined (failed_units lists the failed ones)")))
        }
        [one] => Ok(one),
        many => Err(ResolveError::Unavailable(format!("\"{term}\" matches several {kind}s: {}; ask again with one full name", shown_list(many)))),
    }
}

/// Löst einen Suchbegriff zu einer Unit der Listen auf.
pub fn unit(run: &mut Run, term: &UnitRef) -> Result<ResolvedUnit, ResolveError> {
    let term = term.as_str();
    // Ohne die Liste des Systems ist keine Aussage möglich; fehlt nur die des User-Managers (keine Sitzung), zählen die
    // Units des Systems
    let system = units(run, false).clone()?;
    let user = match units(run, true).clone() {
        Ok(list) => list,
        Err(e @ (ResolveError::TimedOut | ResolveError::Cancelled)) => return Err(e),
        Err(_) => Vec::new(),
    };
    let (system_list, user_list) = (system.as_slice(), user.as_slice());
    let system_names: Vec<&str> = system_list.iter().map(|u| u.name.as_str()).collect();
    let user_names: Vec<&str> = user_list.iter().map(|u| u.name.as_str()).collect();
    // Voller Name oder mit `.service` im System, dann im User-Manager; ein Teiltreffer erst, wenn beides nichts ergab
    for (names, is_user) in [(&system_names, false), (&user_names, true)] {
        let exact = names
            .iter()
            .find(|n| **n == term)
            .or_else(|| (!has_unit_type(term)).then(|| format!("{term}.service")).and_then(|s| names.iter().find(|n| **n == s)));
        if let Some(name) = exact {
            return Ok(ResolvedUnit { name: (*name).to_string(), user: is_user });
        }
    }
    let all: Vec<(&str, bool)> = system_names.iter().map(|n| (*n, false)).chain(user_names.iter().map(|n| (*n, true))).collect();
    let names: Vec<&str> = all.iter().map(|(n, _)| *n).collect();
    let name = pick_name(term, &names, "unit")?;
    let is_user = all.iter().find(|(n, _)| *n == name).map(|(_, u)| *u).unwrap_or(false);
    Ok(ResolvedUnit { name: name.to_string(), user: is_user })
}

/// Beschreibung der aufgelösten Unit für den Kopf des Abschnitts.
pub fn describe_unit(term: &UnitRef, unit: &ResolvedUnit) -> String {
    format!("unit {} → {} ({} manager)", term.as_str(), unit.name, if unit.user { "user" } else { "system" })
}

// ---- Container -----------------------------------------------------------------------------------------------------

fn parse_containers(text: &str) -> Vec<ContainerEntry> {
    let (items, _incomplete) = json::objects(text);
    items
        .iter()
        .filter_map(|o| {
            let id = json::str_of(o, "ID")?;
            if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                return None;
            }
            let names: Vec<String> =
                json::str_of(o, "Names")?.split(',').map(|n| n.trim().trim_start_matches('/').to_string()).filter(|n| !n.is_empty()).collect();
            if names.is_empty() {
                return None;
            }
            Some(ContainerEntry {
                id: id.to_string(),
                names,
                state: json::str_of(o, "State").unwrap_or("").to_string(),
                image: json::str_of(o, "Image").unwrap_or("").to_string(),
            })
        })
        .collect()
}

fn containers<'c>(run: &'c mut Run) -> &'c Result<Vec<ContainerEntry>, ResolveError> {
    if run.cache.containers.as_ref().is_none_or(|r| !keep_in_cache(r)) {
        let result = list(run, &resolution::LIST_CONTAINERS, "containers", Keep::Head).map(|t| parse_containers(&t));
        run.cache.containers = Some(result);
    }
    run.cache.containers.as_ref().expect("just filled")
}

/// Löst einen Suchbegriff zu einem Container auf; eingesetzt wird später die volle ID.
pub fn container(run: &mut Run, term: &ContainerRef) -> Result<ResolvedContainer, ResolveError> {
    let term = term.as_str();
    let all = containers(run).clone()?;
    let resolved = |c: &ContainerEntry| ResolvedContainer { id: c.id.clone(), name: c.names[0].clone() };
    // 1. Name
    if let Some(c) = all.iter().find(|c| c.names.iter().any(|n| n == term)) {
        return Ok(resolved(c));
    }
    // 2. Anfang der ID
    let lower = term.to_lowercase();
    if lower.len() >= 4 && lower.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        let hits: Vec<&ContainerEntry> = all.iter().filter(|c| c.id.starts_with(&lower)).collect();
        match hits.as_slice() {
            [] => {}
            [one] => return Ok(resolved(one)),
            many => {
                let names: Vec<&str> = many.iter().map(|c| c.names[0].as_str()).collect();
                return Err(ResolveError::Unavailable(format!(
                    "the ID start \"{term}\" matches several containers: {}; ask again with the name",
                    shown_list(&names)
                )));
            }
        }
    }
    // 3. Teil eines Namens
    let hits: Vec<&ContainerEntry> = all.iter().filter(|c| c.names.iter().any(|n| n.to_lowercase().contains(&lower))).collect();
    match hits.as_slice() {
        [] => Err(ResolveError::Unavailable(format!("no container matches \"{term}\" (docker_ps lists the existing ones)"))),
        [one] => Ok(resolved(one)),
        many => {
            let names: Vec<&str> = many.iter().map(|c| c.names[0].as_str()).collect();
            Err(ResolveError::Unavailable(format!("\"{term}\" matches several containers: {}; ask again with one full name", shown_list(&names))))
        }
    }
}

/// Beschreibung des aufgelösten Containers für den Kopf des Abschnitts.
pub fn describe_container(term: &ContainerRef, c: &ResolvedContainer) -> String {
    format!("container {} → {} ({})", term.as_str(), c.name, &c.id[..12])
}

// ---- Boots ---------------------------------------------------------------------------------------------------------

fn parse_boots(text: &str) -> Vec<BootEntry> {
    let (items, _incomplete) = json::objects(text);
    let mut boots: Vec<BootEntry> = items
        .iter()
        .filter_map(|o| {
            Some(BootEntry {
                index: json::int_of(o, "index")?,
                boot_id: json::str_of(o, "boot_id")?.to_string(),
                first: json::int_of(o, "first_entry")? / 1_000_000,
                last: json::int_of(o, "last_entry")? / 1_000_000,
            })
        })
        .collect();
    boots.sort_by_key(|b| b.index);
    boots
}

/// Alle Boots des Journals, alt nach neu.
pub fn boots(run: &mut Run) -> Result<Vec<BootEntry>, ResolveError> {
    if run.cache.boots.as_ref().is_none_or(|r| !keep_in_cache(r)) {
        let result = list(run, &resolution::LIST_BOOTS, "boots of the journal", Keep::Tail).map(|t| parse_boots(&t));
        run.cache.boots = Some(result);
    }
    run.cache.boots.clone().expect("just filled")
}

/// Der Boot mit diesem Versatz (0 = laufend, -1 = der davor), falls das Journal ihn kennt.
pub fn boot(run: &mut Run, offset: BootOffset) -> Result<BootEntry, ResolveError> {
    let all = boots(run)?;
    let wanted = i64::from(offset.offset());
    match all.iter().find(|b| b.index == wanted) {
        Some(b) => Ok(b.clone()),
        None => {
            let oldest = all.iter().map(|b| b.index).min();
            Err(ResolveError::Unavailable(match oldest {
                Some(min) if min < 0 => format!("boot {wanted} does not exist: the journal has the boots {min} to 0"),
                Some(_) => format!("boot {wanted} does not exist: the journal has only the current boot"),
                None => format!("boot {wanted} does not exist: the journal lists no boots"),
            }))
        }
    }
}

/// Zeitraum des Boots (siehe [`BootWindow`]).
pub fn boot_window(run: &mut Run, offset: BootOffset) -> Result<BootWindow, ResolveError> {
    let all = boots(run)?;
    let now = run.ctx.clock.now();
    let wanted = i64::from(offset.offset());
    let this = all.iter().find(|b| b.index == wanted).ok_or_else(|| boot(run, offset).expect_err("not in the list"))?;
    let next_start = all.iter().filter(|b| b.index > wanted).min_by_key(|b| b.index).map(|b| b.first).unwrap_or(now);
    let end = if wanted == 0 { now } else { this.last };
    Ok(BootWindow { index: wanted, start: this.first, end, next_start })
}

/// Beschreibung des Boots für den Kopf des Abschnitts.
pub fn describe_boot(clock: &Clock, offset: i64, window: &BootWindow) -> String {
    format!("boot {offset} → {} … {}", clock.format(window.start), clock.format(window.end))
}

// ---- Ping-Ziele und Alter des Boots --------------------------------------------------------------------------------

/// Die Adresse zu einem Ziel der Config-Liste; nur ein Name der Liste (ohne Rücksicht auf Groß-/Kleinschreibung) oder
/// eine Adresse darin gilt.
pub fn ping_target(ctx: &ProbeContext, term: &PingTarget) -> Result<String, ResolveError> {
    let wanted = term.as_str();
    let entry = ctx.ping_targets.iter().find(|t| t.name.eq_ignore_ascii_case(wanted)).or_else(|| ctx.ping_targets.iter().find(|t| t.address == wanted));
    match entry {
        Some(t) => Term::Target
            .check_resolved(&t.address)
            .map(|()| t.address.clone())
            .map_err(|why| ResolveError::Failed(format!("the configured address of target \"{}\" is not usable: {why}", t.name))),
        None if ctx.ping_targets.is_empty() => Err(ResolveError::Unavailable("no ping targets are configured, so nothing can be pinged".into())),
        None => {
            let names: Vec<&str> = ctx.ping_targets.iter().map(|t| t.name.as_str()).collect();
            Err(ResolveError::Unavailable(format!("\"{wanted}\" is not a configured ping target (configured: {})", shown_list(&names))))
        }
    }
}

/// Sekunden seit dem Start des laufenden Boots aus `/proc/uptime`.
pub fn boot_age_secs(run: &Run) -> Result<u64, ResolveError> {
    let fs = Fs::for_resolution(&run.ctx.paths);
    let path = run.ctx.paths.under_root("/proc/uptime");
    let read = fs.read(&path, 256).map_err(|e| ResolveError::Failed(format!("cannot read the uptime: {e}")))?;
    parse_uptime(&String::from_utf8_lossy(&read.bytes)).ok_or_else(|| ResolveError::Failed("the uptime is not readable".into()))
}

fn parse_uptime(text: &str) -> Option<u64> {
    let first = text.split_whitespace().next()?;
    let secs: f64 = first.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_lists_keep_only_full_names_that_may_enter_an_argv() {
        let text = r#"[{"unit":"docker.service","load":"loaded","active":"active","sub":"running","description":"d"},
            {"unit":"-.mount","load":"loaded","active":"active","sub":"mounted","description":"root"},
            {"unit":"1234","load":"loaded","active":"active","sub":"x","description":"a pid"},
            {"unit":"nosuffix","load":"loaded","active":"active","sub":"x","description":"x"},
            {"unit":"getty@tty1.service","load":"loaded","active":"inactive","sub":"dead","description":"g"}]"#;
        let units = parse_units(text);
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["docker.service", "getty@tty1.service"]);
        assert_eq!(units[1].active, "inactive");
    }

    #[test]
    fn container_lists_need_a_full_id_and_a_name() {
        let id = "a".repeat(64);
        let text = format!(
            "{{\"ID\":\"{id}\",\"Names\":\"web,old-name\",\"State\":\"running\",\"Image\":\"nginx\"}}\n{{\"ID\":\"abc\",\"Names\":\"short\"}}\n{{\"ID\":\"{id}\",\"Names\":\"\"}}\n{{\"ID\":\"{}\",\"Names\":\"/db\"}}\n",
            "B".repeat(64)
        );
        let list = parse_containers(&text);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].names, ["web", "old-name"]);
    }

    #[test]
    fn boot_lists_convert_microseconds_and_sort() {
        let text = r#"[{"index":0,"boot_id":"b0","first_entry":1793800000000000,"last_entry":1793809999000000},{"index":-1,"boot_id":"b1","first_entry":1793700000000000,"last_entry":1793780000500000},{"index":"-2","boot_id":"b2","first_entry":5000000,"last_entry":6000000},{"boot_id":"x"}]"#;
        let boots = parse_boots(text);
        assert_eq!(boots.iter().map(|b| b.index).collect::<Vec<_>>(), [-2, -1, 0]);
        assert_eq!(boots[1].last, 1_793_780_000);
        assert_eq!(boots[0].first, 5);
    }

    #[test]
    fn names_are_picked_exactly_then_with_service_then_by_a_single_part() {
        let names = ["docker.service", "docker.socket", "containerd.service", "pipewire.service", "pipewire-pulse.service", "getty@tty1.service"];
        assert_eq!(pick_name("docker.service", &names, "unit").unwrap(), "docker.service");
        assert_eq!(pick_name("docker", &names, "unit").unwrap(), "docker.service");
        assert_eq!(pick_name("docker.socket", &names, "unit").unwrap(), "docker.socket");
        assert_eq!(pick_name("getty", &names, "unit").unwrap(), "getty@tty1.service");
        assert_eq!(pick_name("CONTAINERD", &names, "unit").unwrap(), "containerd.service");
        // Mehrere Treffer: Fehler mit Kandidaten
        let err = pick_name("pipe", &names, "unit").unwrap_err();
        assert!(
            matches!(&err, ResolveError::Unavailable(m) if m.contains("pipewire.service") && m.contains("pipewire-pulse.service") && m.contains("several")),
            "{err:?}"
        );
        // Erfunden
        let err = pick_name("nginx", &names, "unit").unwrap_err();
        assert!(matches!(&err, ResolveError::Unavailable(m) if m.contains("no unit matches")), "{err:?}");
        // Die Antwort ist immer ein Name aus der Liste, nie der Begriff
        for term in ["docker", "getty", "docker.service", "1234", "x"] {
            if let Ok(n) = pick_name(term, &names, "unit") {
                assert!(names.contains(&n));
            }
        }
    }

    #[test]
    fn uptime_parses() {
        assert_eq!(parse_uptime("12345.67 99999.00\n"), Some(12_345));
        assert_eq!(parse_uptime("0.5 1"), Some(0));
        assert_eq!(parse_uptime(""), None);
        assert_eq!(parse_uptime("-5 1"), None);
        assert_eq!(parse_uptime("abc"), None);
    }

    #[test]
    fn unit_types_are_recognised() {
        assert!(has_unit_type("a.service"));
        assert!(has_unit_type("a.b.timer"));
        assert!(!has_unit_type("a.bak"));
        assert!(!has_unit_type("service"));
        assert!(!has_unit_type("docker"));
    }

    #[test]
    fn cache_keeps_failures_but_not_timeouts() {
        assert!(keep_in_cache::<()>(&Ok(())));
        assert!(keep_in_cache::<()>(&Err(ResolveError::Failed("x".into()))));
        assert!(keep_in_cache::<()>(&Err(ResolveError::Unavailable("x".into()))));
        assert!(!keep_in_cache::<()>(&Err(ResolveError::TimedOut)));
        assert!(!keep_in_cache::<()>(&Err(ResolveError::Cancelled)));
    }
}
