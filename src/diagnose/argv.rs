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

//! Feste argv-Vorlagen der Proben, als Daten.
//!
//! Eine Probe ist entweder eine Reihe von Befehlen ([`Exec::Commands`]: Programm mit absolutem Pfad plus Vorlage der
//! Argumente) oder liest Dateien selbst in Rust ([`Exec::Read`]: nur die Liste der Pfade, die sie öffnen darf). Die
//! Vorlage enthält feste Texte ([`Arg::Lit`]) und Platzhalter für die *aufgelösten* Parameter ([`Arg::Unit`],
//! [`Arg::ContainerId`], [`Arg::Lines`], …). Nie steht freier Text darin: ein Platzhalter bekommt entweder einen Wert
//! aus einer festen Aufzählung (Zeilen, Boot, Zeitfenster) oder einen Wert, den die lokale Auflösung gegen die echten
//! Listen geliefert hat ([`Bindings`]), und [`Command::expand`] prüft ihn noch einmal (kein Wert beginnt mit `-`, nur
//! die erlaubten Zeichen). Wo ein Programm es kennt, steht vor dem Platzhalter `--`, damit selbst ein falscher Wert
//! keine Option sein kann.
//!
//! Ausgeführt wird später nur in Rust per argv (Aufgabe 2, `std::process::Command`, nie `sh -c`). Dieses Modul führt
//! nichts aus; es beschreibt, was ausgeführt werden darf, und die Tests prüfen es: kein verbotenes Wort
//! (`--vacuum*`, `--rotate`, `--flush`, `--sync`, `restart`, `start`, `stop`, `kill`, `rm`, `exec`, `sudo`, …), je
//! Programm nur die erlaubten Unterbefehle und Optionen, nur Pfade unter `/proc`, `/sys` und den festen
//! Verzeichnissen. Ein Programm, das hier nicht als [`Program`] steht, kann keine Probe starten.

use super::catalog::Term;
use super::probe::Probe;
use std::fmt;

/// Deklariert die Programme, die eine Probe starten darf, jedes mit absolutem Pfad unter `/usr/bin`.
macro_rules! programs {
    ($($var:ident = $s:literal,)+) => {
        /// Ein Programm, das eine Probe starten darf (Arch Linux: alles unter `/usr/bin`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Program {
            $($var,)+
        }

        impl Program {
            /// Alle Programme.
            pub const ALL: &'static [Program] = &[$(Program::$var),+];

            /// Name der Datei, z. B. `journalctl`.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Program::$var => $s,)+
                }
            }

            /// Absoluter Pfad, z. B. `/usr/bin/journalctl`. Die Umgebung des Ausführers (`ProbeEnv`, Aufgabe 2) darf das
            /// Verzeichnis ersetzen (Tests mit Fakes), den Dateinamen nicht.
            pub const fn path(self) -> &'static str {
                match self {
                    $(Program::$var => concat!("/usr/bin/", $s),)+
                }
            }
        }
    };
}

programs! {
    Journalctl = "journalctl",
    Systemctl = "systemctl",
    Coredumpctl = "coredumpctl",
    Docker = "docker",
    Ip = "ip",
    Resolvectl = "resolvectl",
    Ss = "ss",
    Tailscale = "tailscale",
    Findmnt = "findmnt",
    Ping = "ping",
    Df = "df",
    Lsblk = "lsblk",
    Sensors = "sensors",
    NvidiaSmi = "nvidia-smi",
    Pacman = "pacman",
}

/// Ein Element der argv-Vorlage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// Fester Text, genau ein argv-Element.
    Lit(&'static str),
    /// Name der aufgelösten Unit ([`Bindings::unit`]), ein Element; steht hinter `--` oder hinter `-u`.
    Unit,
    /// `--user`, wenn die aufgelöste Unit zum User-Manager gehört ([`ResolvedUnit::user`]); sonst entfällt das Element.
    UserFlag,
    /// ID des aufgelösten Containers ([`Bindings::container_id`]), ein Element hinter `--`.
    ContainerId,
    /// Aufgelöstes Ping-Ziel ([`Bindings::ping_target`]), ein Element hinter `--`.
    PingTarget,
    /// Wert von `lines` als Dezimalzahl („200“), ein Element.
    Lines,
    /// `--boot=<boot>` („--boot=-1“), ein Element; `--boot=` mit Gleichheitszeichen, damit ein negativer Wert nie als
    /// eigene Option gelesen wird.
    Boot,
    /// Zeitfenster für `journalctl`, ein Element: `--since=-15m`, `--since=-1h`, …; für `boot` stattdessen `--boot=0`.
    JournalSince,
    /// Zeitfenster für `docker logs`, ein Element: `--since=15m`, …; für `boot` `--since=<Sekunden seit Boot>s`
    /// ([`Bindings::boot_age_secs`]).
    DockerSince,
    /// Zeitfenster für `coredumpctl`, ein Element: `--since=-15m`, …; für `boot` `--since=-<Sekunden seit Boot>s`.
    CoreSince,
}

impl Arg {
    /// Name des Parameters der Probe, den dieser Platzhalter braucht (`None` bei festem Text).
    pub const fn param(self) -> Option<&'static str> {
        match self {
            Arg::Lit(_) => None,
            Arg::Unit | Arg::UserFlag => Some("unit"),
            Arg::ContainerId => Some("container"),
            Arg::PingTarget => Some("target"),
            Arg::Lines => Some("lines"),
            Arg::Boot => Some("boot"),
            Arg::JournalSince | Arg::DockerSince | Arg::CoreSince => Some("since"),
        }
    }
}

/// Ein Befehl: Programm und argv-Vorlage (ohne das Programm selbst als erstes Element).
#[derive(Debug, Clone, Copy)]
pub struct Command {
    pub program: Program,
    pub args: &'static [Arg],
}

/// Wie eine Probe ausgeführt wird.
#[derive(Debug, Clone, Copy)]
pub enum Exec {
    /// Diese Befehle nacheinander (nie parallel); die Ausgaben stehen mit Überschrift im Bericht.
    Commands(&'static [Command]),
    /// Kein Prozess: Rust liest Dateien. Die Liste nennt alle Pfade, die die Probe öffnen darf (`*` steht für einen
    /// Ordner- oder Dateinamenteil, `~/` für das Home-Verzeichnis, `{temp_log_dir}` für `diagnose.temp_log_dir`).
    Read(&'static [&'static str]),
}

/// Befehle, die nur die lokale Auflösung startet (kein Eintrag im Katalog, nie ein Parameter darin): die echten Listen,
/// gegen die Suchbegriffe geprüft werden (`probes::resolve`). Dieselben Prüfungen wie für die Proben gelten für sie
/// (kein verbotenes Wort, je Programm nur erlaubte Elemente).
pub mod resolution {
    use super::{Arg::Lit as L, Command, Program};

    /// Units des System-Managers, auch inaktive (`--all`): hier muss eine Unit stehen, damit sie als Name zählt.
    pub const LIST_UNITS: Command = Command { program: Program::Systemctl, args: &[L("list-units"), L("--all"), L("--no-pager"), L("-o"), L("json")] };
    /// Units des User-Managers.
    pub const LIST_USER_UNITS: Command =
        Command { program: Program::Systemctl, args: &[L("--user"), L("list-units"), L("--all"), L("--no-pager"), L("-o"), L("json")] };
    /// Container mit voller 64-stelliger ID (`--no-trunc`): nur diese ID wird eingesetzt, nie der Suchbegriff.
    pub const LIST_CONTAINERS: Command = Command { program: Program::Docker, args: &[L("ps"), L("-a"), L("--no-trunc"), L("--format"), L("json")] };
    /// Boots des Journals mit Index, ID, erstem und letztem Eintrag.
    pub const LIST_BOOTS: Command = Command { program: Program::Journalctl, args: &[L("--list-boots"), L("--no-pager"), L("-o"), L("json")] };

    /// Alle Befehle der Auflösung (für die Tests der Vorlagen).
    pub const ALL: &[(&str, &Command)] =
        &[("list_units", &LIST_UNITS), ("list_user_units", &LIST_USER_UNITS), ("list_containers", &LIST_CONTAINERS), ("list_boots", &LIST_BOOTS)];
}

/// Eine aufgelöste Unit: ihr Name aus der Liste von `systemctl list-units --all` und ob sie zum User-Manager gehört.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUnit {
    pub name: String,
    pub user: bool,
}

/// Die Werte, die die lokale Auflösung (Aufgabe 3) für die Platzhalter liefert. Alles andere (Zeilen, Boot, Zeitfenster)
/// steht in der Probe selbst.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    /// Für `unit_*`.
    pub unit: Option<ResolvedUnit>,
    /// Volle ID (64 Zeichen, klein hexadezimal) für `docker_state` und `docker_logs`, aus `docker ps -a --no-trunc`. Eine
    /// kurze ID wäre bei `docker inspect` ohne Typ auch Präfix von Images oder Netzen.
    pub container_id: Option<String>,
    /// Ziel aus der Config-Liste für `net_reach`.
    pub ping_target: Option<String>,
    /// Sekunden seit dem Start des laufenden Boots (`/proc/uptime`), nur für `since: boot` bei `docker_logs` und
    /// `coredumps`.
    pub boot_age_secs: Option<u64>,
}

/// Ein ausführbarer Befehl: Programm und fertige argv (ohne das Programm als erstes Element).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argv {
    pub program: Program,
    pub args: Vec<String>,
}

/// Warum eine Vorlage nicht zu argv wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// Die Probe liest in Rust und hat keine Befehle.
    NoCommands,
    /// Die Vorlage braucht einen Parameter, den die Probe nicht hat (ein Fehler im Katalog; ein Test verhindert das).
    MissingParam(&'static str),
    /// Die Auflösung hat den Wert für den Platzhalter nicht geliefert.
    MissingBinding(&'static str),
    /// Der aufgelöste Wert ist nicht zulässig (leer, beginnt mit `-`, falsche Zeichen oder Länge).
    BadBinding { what: &'static str, reason: String },
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::NoCommands => write!(f, "this probe reads files itself and has no command"),
            ExpandError::MissingParam(p) => write!(f, "the command template needs the parameter \"{p}\", which the probe does not have"),
            ExpandError::MissingBinding(b) => write!(f, "the resolved value for \"{b}\" is missing"),
            ExpandError::BadBinding { what, reason } => write!(f, "the resolved {what} {reason}"),
        }
    }
}

impl std::error::Error for ExpandError {}

fn bad(what: &'static str, reason: impl Into<String>) -> ExpandError {
    ExpandError::BadBinding { what, reason: reason.into() }
}

fn unit_binding(b: &Bindings) -> Result<&ResolvedUnit, ExpandError> {
    b.unit.as_ref().ok_or(ExpandError::MissingBinding("unit"))
}

fn boot_age(b: &Bindings) -> Result<u64, ExpandError> {
    b.boot_age_secs.ok_or(ExpandError::MissingBinding("boot_age_secs"))
}

impl Arg {
    /// Der Text dieses Elements; `None`, wenn es entfällt ([`Arg::UserFlag`] bei einer Unit des Systems).
    fn render(self, probe: &Probe, b: &Bindings) -> Result<Option<String>, ExpandError> {
        let since = || probe.since().ok_or(ExpandError::MissingParam("since"));
        Ok(Some(match self {
            Arg::Lit(s) => s.to_string(),
            Arg::Unit => {
                let name = &unit_binding(b)?.name;
                Term::Unit.check_resolved(name).map_err(|r| bad("unit name", r))?;
                name.clone()
            }
            Arg::UserFlag => return Ok(unit_binding(b)?.user.then(|| "--user".to_string())),
            Arg::ContainerId => {
                let id = b.container_id.as_ref().ok_or(ExpandError::MissingBinding("container_id"))?;
                if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                    return Err(bad("container ID", "must be the full 64 lowercase hexadecimal characters"));
                }
                id.clone()
            }
            Arg::PingTarget => {
                let target = b.ping_target.as_ref().ok_or(ExpandError::MissingBinding("ping_target"))?;
                Term::Target.check_resolved(target).map_err(|r| bad("ping target", r))?;
                target.clone()
            }
            Arg::Lines => probe.lines().ok_or(ExpandError::MissingParam("lines"))?.count().to_string(),
            Arg::Boot => format!("--boot={}", probe.boot().ok_or(ExpandError::MissingParam("boot"))?.offset()),
            Arg::JournalSince => match since()?.duration() {
                Some(d) => format!("--since=-{d}"),
                None => "--boot=0".to_string(),
            },
            Arg::DockerSince => match since()?.duration() {
                Some(d) => format!("--since={d}"),
                None => format!("--since={}s", boot_age(b)?),
            },
            Arg::CoreSince => match since()?.duration() {
                Some(d) => format!("--since=-{d}"),
                None => format!("--since=-{}s", boot_age(b)?),
            },
        }))
    }
}

impl Command {
    /// Setzt die Werte der Probe und die aufgelösten Werte in die Vorlage ein.
    pub fn expand(&self, probe: &Probe, bindings: &Bindings) -> Result<Argv, ExpandError> {
        let mut args = Vec::with_capacity(self.args.len());
        for arg in self.args {
            if let Some(text) = arg.render(probe, bindings)? {
                args.push(text);
            }
        }
        Ok(Argv { program: self.program, args })
    }
}

/// Alle Befehle einer Probe als fertige argv, in der Reihenfolge des Katalogs. [`ExpandError::NoCommands`] bei Proben,
/// die in Rust lesen.
pub fn expand_probe(probe: &Probe, bindings: &Bindings) -> Result<Vec<Argv>, ExpandError> {
    match probe.spec().exec {
        Exec::Commands(commands) => commands.iter().map(|c| c.expand(probe, bindings)).collect(),
        Exec::Read(_) => Err(ExpandError::NoCommands),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::catalog::{self, DOCKER_STATE_FORMAT, GPU_QUERY, UNIT_SHOW_PROPERTIES};
    use crate::diagnose::probe::parse_validated;
    use crate::diagnose::test_support::full_probes;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn probe(json: &str) -> Probe {
        parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"))
    }

    /// Alle Befehle der Proben und der Auflösung.
    fn all_commands() -> Vec<(&'static str, &'static Command)> {
        catalog::all()
            .iter()
            .flat_map(|s| match &s.exec {
                Exec::Commands(commands) => commands.iter().map(|c| (s.name, c)).collect::<Vec<_>>(),
                Exec::Read(_) => Vec::new(),
            })
            .chain(resolution::ALL.iter().copied())
            .collect()
    }

    // ---- Verbotene Wörter -------------------------------------------------------------------------------------------

    /// Optionen, die ändern, aufräumen oder den Journalzustand anfassen: als Anfang eines Elements verboten.
    const FORBIDDEN_PREFIXES: &[&str] =
        &["--vacuum", "--rotate", "--flush", "--sync", "--relinquish-var", "--smart-relinquish-var", "--setup-keys", "--update-catalog"];
    /// Wörter, die als ganzes Element (oder als Teil einer Liste hinter `=` oder `,`) verboten sind: Verben, die etwas
    /// starten, beenden, löschen oder ausführen. `--since` ist erlaubt, `start` nicht; `docker stats` ist erlaubt.
    const FORBIDDEN_WORDS: &[&str] = &[
        "restart",
        "start",
        "stop",
        "kill",
        "rm",
        "exec",
        "sudo",
        "reload",
        "enable",
        "disable",
        "mask",
        "unmask",
        "prune",
        "run",
        "create",
        "delete",
        "remove",
        "pull",
        "push",
        "build",
        "cp",
        "pause",
        "unpause",
        "update",
        "rename",
        "commit",
        "reboot",
        "poweroff",
        "halt",
        "suspend",
        "set",
        "edit",
        "isolate",
        "reset-failed",
        "daemon-reload",
        "flush-caches",
        "del",
        "add",
        "-s",
        "-r",
        "-R",
        "-S",
        "-U",
        "-F",
        "-pl",
        "-pm",
    ];

    /// Das erste verbotene Wort in einem Element, falls eines vorkommt: als Anfang (Optionen), als ganzes Element oder als
    /// Teil einer Liste hinter `=`, `,` oder Leerraum. Wörter ohne `-` gelten ohne Rücksicht auf Groß-/Kleinschreibung,
    /// kurze Optionen genau.
    fn forbidden_in(element: &str) -> Option<&'static str> {
        let lower = element.to_lowercase();
        if let Some(p) = FORBIDDEN_PREFIXES.iter().find(|p| lower.starts_with(*p)) {
            return Some(p);
        }
        for token in element.split(|c: char| c == '=' || c == ',' || c.is_whitespace()) {
            if let Some(w) = FORBIDDEN_WORDS.iter().find(|w| token == **w || (!w.starts_with('-') && token.eq_ignore_ascii_case(w))) {
                return Some(w);
            }
        }
        None
    }

    #[test]
    fn the_checker_itself_flags_what_it_should() {
        for bad in [
            "--vacuum-size=1M",
            "--vacuum-time=1d",
            "--vacuum-files=2",
            "--rotate",
            "--flush",
            "--sync",
            "--sync=now",
            "restart",
            "start",
            "stop",
            "kill",
            "rm",
            "exec",
            "sudo",
            "-s",
            "unit=restart",
            "a,rm,b",
            "docker rm",
        ] {
            assert!(forbidden_in(bad).is_some(), "{bad} should be flagged");
        }
        for fine in [
            "--since",
            "--since=-1h",
            "--since=15m",
            "stats",
            "--no-stream",
            "status",
            "show",
            "list",
            "logs",
            "ps",
            "inspect",
            "-n",
            "-u",
            "--user",
            "ExecMainStatus",
            "Out of memory|oom-kill",
            "--timestamps",
            "--tail",
            "--list-boots",
            "--boot=0",
        ] {
            assert!(forbidden_in(fine).is_none(), "{fine} should be allowed (flagged as {:?})", forbidden_in(fine));
        }
    }

    #[test]
    fn no_template_contains_a_forbidden_word() {
        let mut checked = 0;
        for (name, command) in all_commands() {
            for arg in command.args {
                if let Arg::Lit(text) = arg {
                    assert!(forbidden_in(text).is_none(), "probe {name}: {} {text:?} contains the forbidden {:?}", command.program.name(), forbidden_in(text));
                    checked += 1;
                }
            }
            assert!(forbidden_in(command.program.name()).is_none(), "{}", command.program.name());
        }
        assert!(checked > 100, "only {checked} literals checked");
        // Auch kein Programmname und kein Pfad ist eines der Wörter
        for program in Program::ALL {
            assert!(forbidden_in(program.name()).is_none(), "{}", program.name());
            assert!(!program.path().contains("sudo"), "{}", program.path());
        }
    }

    /// Je Programm alle festen Elemente, die in irgendeiner Vorlage vorkommen dürfen. Alles andere (zum Beispiel ein
    /// Unterbefehl wie `docker rm`) lässt den Test scheitern, auch wenn es nicht auf der Liste verbotener Wörter steht.
    fn allowed_literals(program: Program) -> Vec<&'static str> {
        match program {
            Program::Journalctl => {
                vec!["--list-boots", "--no-pager", "-o", "json", "short-iso", "-n", "50", "-k", "-p", "warning", "-u", "--grep", "Out of memory|oom-kill"]
            }
            Program::Systemctl => {
                vec![
                    "--failed",
                    "--no-pager",
                    "-o",
                    "json",
                    "--user",
                    "status",
                    "-n",
                    "0",
                    "--",
                    "show",
                    "-p",
                    UNIT_SHOW_PROPERTIES,
                    "list-timers",
                    "list-units",
                    "--all",
                ]
            }
            Program::Coredumpctl => vec!["list", "--no-pager", "-q", "--json=short"],
            Program::Docker => vec![
                "ps",
                "-a",
                "--no-trunc",
                "--format",
                "json",
                "inspect",
                "--type=container",
                DOCKER_STATE_FORMAT,
                "--",
                "logs",
                "--tail",
                "--timestamps",
                "system",
                "df",
                "stats",
                "--no-stream",
            ],
            Program::Ip => vec!["-j", "addr", "route"],
            Program::Resolvectl => vec!["status", "--no-pager"],
            Program::Ss => vec!["-Htulpn"],
            Program::Tailscale => vec!["status", "--json"],
            Program::Findmnt => vec!["-J", "-t", "cifs,nfs,nfs4,fuse.sshfs"],
            Program::Ping => vec!["-c", "3", "-W", "1", "--"],
            Program::Df => vec!["-h", "-l", "-x", "tmpfs", "devtmpfs", "overlay"],
            Program::Lsblk => vec!["-J", "-o", "NAME,SIZE,TYPE,MOUNTPOINTS,FSTYPE"],
            Program::Sensors => vec!["-j"],
            Program::NvidiaSmi => vec![GPU_QUERY, "--format=csv"],
            Program::Pacman => vec!["-Q", "linux", "nvidia-open-dkms", "nvidia-utils", "mesa", "docker", "systemd"],
        }
    }

    #[test]
    fn every_literal_is_on_the_allowlist_of_its_program() {
        let mut used: std::collections::BTreeMap<&str, BTreeSet<&str>> = Default::default();
        for (name, command) in all_commands() {
            let allowed = allowed_literals(command.program);
            for arg in command.args {
                if let Arg::Lit(text) = arg {
                    assert!(allowed.contains(text), "probe {name}: {} {text:?} is not on the allowlist for {}", command.program.name(), command.program.name());
                    used.entry(command.program.name()).or_default().insert(text);
                }
            }
        }
        // Und jedes Programm der Liste wird auch benutzt (keine Altlast)
        for program in Program::ALL {
            assert!(used.contains_key(program.name()), "{} is not used by any probe", program.name());
        }
    }

    #[test]
    fn systemctl_show_properties_never_include_environment_or_command_lines() {
        const ALLOWED: &[&str] = &[
            "Id",
            "Description",
            "LoadState",
            "ActiveState",
            "SubState",
            "Result",
            "ExecMainCode",
            "ExecMainStatus",
            "NRestarts",
            "ActiveEnterTimestamp",
            "ActiveExitTimestamp",
            "InactiveEnterTimestamp",
            "InactiveExitTimestamp",
            "StateChangeTimestamp",
            "ExecMainStartTimestamp",
            "ExecMainExitTimestamp",
            "FragmentPath",
            "MemoryCurrent",
        ];
        let listed: Vec<&str> = UNIT_SHOW_PROPERTIES.split(',').collect();
        assert!(!listed.is_empty());
        for p in &listed {
            assert!(ALLOWED.contains(p), "property {p} is not allowed");
        }
        let unique: BTreeSet<&&str> = listed.iter().collect();
        assert_eq!(unique.len(), listed.len(), "duplicate properties");
        let lower = UNIT_SHOW_PROPERTIES.to_lowercase();
        for forbidden in ["environment", "execstart", "execstop", "execreload", "execcondition", "credential", "password", "secret"] {
            assert!(!lower.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn docker_state_template_reads_no_env_labels_or_commands() {
        for forbidden in [
            ".Env",
            ".Labels",
            ".Cmd",
            ".Entrypoint",
            ".Args",
            ".HostConfig",
            ".NetworkSettings",
            ".Healthcheck",
            "Health.Log",
            ".Config}",
            ".Config.Env",
            ".Config.Labels",
            ".Config.Cmd",
            ".Path",
            ".Driver",
        ] {
            assert!(!DOCKER_STATE_FORMAT.contains(forbidden), "template mentions {forbidden}");
        }
        // Aus `.Config` wird genau ein Feld gelesen: das Image
        let config_uses: Vec<&str> = DOCKER_STATE_FORMAT.match_indices(".Config").map(|(i, _)| &DOCKER_STATE_FORMAT[i..i + ".Config.Image".len()]).collect();
        assert_eq!(config_uses, [".Config.Image"]);
        // Die Vorlage selbst ist ausgeglichen
        assert_eq!(DOCKER_STATE_FORMAT.matches("{{").count(), DOCKER_STATE_FORMAT.matches("}}").count());
        assert!(DOCKER_STATE_FORMAT.starts_with('{') && DOCKER_STATE_FORMAT.ends_with('}'));
        // Die Mounts nennen Art, Quelle, Ziel und Schreibrecht, aber nie Optionen
        assert!(!DOCKER_STATE_FORMAT.contains("Options") && !DOCKER_STATE_FORMAT.contains("Mode"));
    }

    #[test]
    fn gpu_query_names_only_read_fields() {
        let fields: Vec<&str> = GPU_QUERY.strip_prefix("--query-gpu=").expect("--query-gpu=").split(',').collect();
        assert!(fields.len() >= 10);
        for f in &fields {
            assert!(
                f.starts_with("temperature.")
                    || f.starts_with("power.")
                    || f.starts_with("clocks.")
                    || f.starts_with("clocks_event_reasons.")
                    || f.starts_with("utilization.")
                    || f.starts_with("memory.")
                    || *f == "pstate",
                "unexpected GPU field {f}"
            );
        }
    }

    // ---- Form der Vorlagen ------------------------------------------------------------------------------------------

    #[test]
    fn programs_have_absolute_paths_under_usr_bin() {
        let mut names = BTreeSet::new();
        for p in Program::ALL {
            assert!(Path::new(p.path()).is_absolute(), "{}", p.path());
            assert_eq!(Path::new(p.path()).parent(), Some(Path::new("/usr/bin")), "{}", p.path());
            assert_eq!(Path::new(p.path()).file_name().unwrap().to_str().unwrap(), p.name());
            assert!(names.insert(p.name()), "duplicate {}", p.name());
            assert!(!p.name().contains(['/', ' ']));
        }
    }

    #[test]
    fn literals_are_never_empty_and_never_pass_through_a_shell() {
        for (name, command) in all_commands() {
            assert!(!command.args.is_empty(), "{name}");
            for arg in command.args {
                if let Arg::Lit(text) = arg {
                    assert!(!text.is_empty(), "probe {name}: empty literal");
                    assert!(!text.contains(['\n', '\0', '\r']), "probe {name}: {text:?}");
                }
            }
        }
    }

    #[test]
    fn identifiers_sit_behind_dash_dash_or_an_option_that_takes_them() {
        for (name, command) in all_commands() {
            for (i, arg) in command.args.iter().enumerate() {
                if matches!(arg, Arg::Unit | Arg::ContainerId | Arg::PingTarget) {
                    let before = i.checked_sub(1).map(|j| command.args[j]);
                    assert!(
                        before == Some(Arg::Lit("--")) || before == Some(Arg::Lit("-u")),
                        "probe {name}: an identifier placeholder must follow `--` or `-u`, found {before:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn placeholders_match_the_parameters_of_the_probe() {
        for spec in catalog::all() {
            let declared: BTreeSet<&str> = spec.params.iter().map(|p| p.name).collect();
            match &spec.exec {
                Exec::Commands(commands) => {
                    let used: BTreeSet<&str> = commands.iter().flat_map(|c| c.args.iter()).filter_map(|a| a.param()).collect();
                    assert_eq!(used, declared, "placeholders and parameters of {} differ", spec.name);
                    assert!(!commands.is_empty(), "{}", spec.name);
                }
                Exec::Read(paths) => {
                    assert!(!paths.is_empty(), "{}", spec.name);
                }
            }
        }
    }

    #[test]
    fn read_probes_open_only_fixed_places() {
        let mut count = 0;
        for spec in catalog::all() {
            let Exec::Read(paths) = &spec.exec else { continue };
            for path in *paths {
                count += 1;
                assert!(!path.contains(".."), "{path}");
                assert!(!path.contains("//"), "{path}");
                let allowed = path.starts_with("/proc/")
                    || path.starts_with("/sys/")
                    || path.starts_with("/var/lib/systemd/pstore/")
                    || *path == "/var/log/pacman.log"
                    || path.starts_with("{temp_log_dir}/temps-");
                assert!(allowed, "probe {}: {path} is not under a fixed place", spec.name);
            }
        }
        assert!(count >= 12, "{count}");
    }

    #[test]
    fn the_kinds_of_exec_match_the_plan() {
        let reads: BTreeSet<&str> = catalog::all().iter().filter(|s| matches!(s.exec, Exec::Read(_))).map(|s| s.name).collect();
        assert_eq!(reads, BTreeSet::from(["pstore", "temps_tail", "kernel_info", "pacman_log", "mem", "load", "top_procs", "nvme_temp"]));
        // Mehrere Befehle: nur dort, wo der Plan zwei Aufrufe nennt
        let multi: BTreeSet<&str> = catalog::all().iter().filter(|s| matches!(&s.exec, Exec::Commands(c) if c.len() > 1)).map(|s| s.name).collect();
        assert_eq!(multi, BTreeSet::from(["failed_units", "net_links", "disks"]));
    }

    // ---- Einsetzen ---------------------------------------------------------------------------------------------------

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Eine volle Container-ID (64 Zeichen), wie `docker ps -a --no-trunc` sie liefert.
    const CID: &str = "445f5bd9a1f3c12300f76ef9a5d61728732ec2297c8d655f031bae3dfb3ebcf3";

    fn bound() -> Bindings {
        Bindings {
            unit: Some(ResolvedUnit { name: "docker.service".into(), user: false }),
            container_id: Some(CID.into()),
            ping_target: Some("192.0.2.1".into()),
            boot_age_secs: Some(3600),
        }
    }

    fn expanded(json: &str, b: &Bindings) -> Vec<Argv> {
        expand_probe(&probe(json), b).unwrap_or_else(|e| panic!("{json}: {e}"))
    }

    #[test]
    fn expansion_gives_the_planned_argv() {
        let b = bound();
        let one = |json: &str| -> (Program, Vec<String>) {
            let mut v = expanded(json, &b);
            assert_eq!(v.len(), 1, "{json}");
            let a = v.remove(0);
            (a.program, a.args)
        };
        assert_eq!(one(r#"{"probe":"boots"}"#), (Program::Journalctl, strings(&["--list-boots", "--no-pager", "-o", "json"])));
        assert_eq!(
            one(r#"{"probe":"boot_end","boot":-1,"lines":200}"#),
            (Program::Journalctl, strings(&["--boot=-1", "-n", "200", "-o", "short-iso", "--no-pager"]))
        );
        assert_eq!(
            one(r#"{"probe":"kernel_errors","boot":0,"lines":50}"#),
            (Program::Journalctl, strings(&["-k", "--boot=0", "-p", "warning", "-o", "short-iso", "--no-pager", "-n", "50"]))
        );
        assert_eq!(
            one(r#"{"probe":"unit_journal","unit":"docker","since":"1h","lines":200}"#),
            (Program::Journalctl, strings(&["-u", "docker.service", "--since=-1h", "-n", "200", "-o", "short-iso", "--no-pager"]))
        );
        assert_eq!(
            one(r#"{"probe":"unit_journal","unit":"docker","since":"boot","lines":50}"#),
            (Program::Journalctl, strings(&["-u", "docker.service", "--boot=0", "-n", "50", "-o", "short-iso", "--no-pager"]))
        );
        assert_eq!(
            one(r#"{"probe":"unit_status","unit":"docker"}"#),
            (Program::Systemctl, strings(&["status", "--no-pager", "-n", "0", "--", "docker.service"]))
        );
        let (program, args) = one(r#"{"probe":"unit_show","unit":"docker"}"#);
        assert_eq!(program, Program::Systemctl);
        assert_eq!(args, ["show", "--no-pager", "-p", UNIT_SHOW_PROPERTIES, "--", "docker.service"]);
        assert_eq!(one(r#"{"probe":"timers"}"#), (Program::Systemctl, strings(&["list-timers", "--all", "--no-pager", "-o", "json"])));
        assert_eq!(one(r#"{"probe":"coredumps","since":"6h"}"#), (Program::Coredumpctl, strings(&["list", "--since=-6h", "--no-pager", "-q", "--json=short"])));
        assert_eq!(
            one(r#"{"probe":"coredumps","since":"boot"}"#),
            (Program::Coredumpctl, strings(&["list", "--since=-3600s", "--no-pager", "-q", "--json=short"]))
        );
        assert_eq!(one(r#"{"probe":"docker_ps"}"#), (Program::Docker, strings(&["ps", "-a", "--format", "json"])));
        let (program, args) = one(r#"{"probe":"docker_state","container":"web"}"#);
        assert_eq!(program, Program::Docker);
        assert_eq!(args, ["inspect", "--type=container", "--format", DOCKER_STATE_FORMAT, "--", CID]);
        assert_eq!(
            one(r#"{"probe":"docker_logs","container":"web","since":"15m","lines":500}"#),
            (Program::Docker, strings(&["logs", "--tail", "500", "--since=15m", "--timestamps", "--", CID]))
        );
        assert_eq!(
            one(r#"{"probe":"docker_logs","container":"web","since":"boot","lines":50}"#),
            (Program::Docker, strings(&["logs", "--tail", "50", "--since=3600s", "--timestamps", "--", CID]))
        );
        assert_eq!(one(r#"{"probe":"docker_df"}"#), (Program::Docker, strings(&["system", "df"])));
        assert_eq!(one(r#"{"probe":"docker_stats"}"#), (Program::Docker, strings(&["stats", "--no-stream", "--format", "json"])));
        assert_eq!(one(r#"{"probe":"net_reach","target":"nas"}"#), (Program::Ping, strings(&["-c", "3", "-W", "1", "--", "192.0.2.1"])));
        assert_eq!(
            one(r#"{"probe":"oom","boot":-1}"#),
            (Program::Journalctl, strings(&["-k", "--boot=-1", "--grep", "Out of memory|oom-kill", "-o", "short-iso", "--no-pager", "-n", "50"]))
        );
        assert_eq!(one(r#"{"probe":"driver_versions"}"#).0, Program::Pacman);
        assert_eq!(one(r#"{"probe":"driver_versions"}"#).1[0], "-Q");
        assert_eq!(one(r#"{"probe":"sensors"}"#), (Program::Sensors, strings(&["-j"])));
        assert_eq!(one(r#"{"probe":"net_dns"}"#), (Program::Resolvectl, strings(&["status", "--no-pager"])));
        assert_eq!(one(r#"{"probe":"net_listen"}"#), (Program::Ss, strings(&["-Htulpn"])));
        assert_eq!(one(r#"{"probe":"net_tailscale"}"#), (Program::Tailscale, strings(&["status", "--json"])));
        assert_eq!(one(r#"{"probe":"net_mounts"}"#), (Program::Findmnt, strings(&["-J", "-t", "cifs,nfs,nfs4,fuse.sshfs"])));
        let (program, args) = one(r#"{"probe":"gpu"}"#);
        assert_eq!(program, Program::NvidiaSmi);
        assert_eq!(args, [GPU_QUERY, "--format=csv"]);
    }

    #[test]
    fn probes_with_several_commands_expand_in_order() {
        let b = Bindings::default();
        let failed = expanded(r#"{"probe":"failed_units"}"#, &b);
        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0].args, ["--failed", "--no-pager", "-o", "json"]);
        assert_eq!(failed[1].args, ["--user", "--failed", "--no-pager", "-o", "json"]);
        let net = expanded(r#"{"probe":"net_links"}"#, &b);
        assert_eq!((net[0].program, &net[0].args), (Program::Ip, &strings(&["-j", "addr"])));
        assert_eq!((net[1].program, &net[1].args), (Program::Ip, &strings(&["-j", "route"])));
        let disks = expanded(r#"{"probe":"disks"}"#, &b);
        assert_eq!(disks[0].program, Program::Df);
        assert_eq!(disks[0].args, ["-h", "-l", "-x", "tmpfs", "-x", "devtmpfs", "-x", "overlay"]);
        assert_eq!(disks[1].program, Program::Lsblk);
    }

    #[test]
    fn user_units_get_the_user_flag_in_front() {
        let mut b = bound();
        b.unit = Some(ResolvedUnit { name: "pipewire.service".into(), user: true });
        assert_eq!(
            expanded(r#"{"probe":"unit_status","unit":"pipewire"}"#, &b)[0].args,
            ["--user", "status", "--no-pager", "-n", "0", "--", "pipewire.service"]
        );
        assert_eq!(
            expanded(r#"{"probe":"unit_journal","unit":"pipewire","since":"24h","lines":50}"#, &b)[0].args,
            ["--user", "-u", "pipewire.service", "--since=-24h", "-n", "50", "-o", "short-iso", "--no-pager"]
        );
    }

    #[test]
    fn expansion_rejects_missing_and_bad_values() {
        let unit = probe(r#"{"probe":"unit_status","unit":"docker"}"#);
        let mut b = Bindings::default();
        assert_eq!(expand_probe(&unit, &b), Err(ExpandError::MissingBinding("unit")));
        for name in ["", "-u", "--user", "-.mount", "a b", "a/b", "a;b", "a\nb", ".x", "1234", "docker", "docker.bak", &format!("{}.service", "a".repeat(256))]
        {
            b.unit = Some(ResolvedUnit { name: name.into(), user: false });
            assert!(matches!(expand_probe(&unit, &b), Err(ExpandError::BadBinding { what: "unit name", .. })), "{name:?}");
        }
        b.unit = Some(ResolvedUnit { name: format!("{}.service", "a".repeat(247)), user: false });
        assert!(expand_probe(&unit, &b).is_ok());

        let logs = probe(r#"{"probe":"docker_logs","container":"web","since":"1h","lines":50}"#);
        let mut b = Bindings::default();
        assert_eq!(expand_probe(&logs, &b), Err(ExpandError::MissingBinding("container_id")));
        for id in ["", "abc", "445f5bd9a1f3", &CID[..63], &CID.to_uppercase(), "--all--all--", "web-web-web1", &format!("{} -f", &CID[..61]), &"a".repeat(65)] {
            b.container_id = Some(id.into());
            assert!(matches!(expand_probe(&logs, &b), Err(ExpandError::BadBinding { what: "container ID", .. })), "{id:?}");
        }
        b.container_id = Some("a".repeat(64));
        assert!(expand_probe(&logs, &b).is_ok());
        b.container_id = Some(CID.into());
        assert!(expand_probe(&logs, &b).is_ok());

        let reach = probe(r#"{"probe":"net_reach","target":"nas"}"#);
        let mut b = Bindings::default();
        assert_eq!(expand_probe(&reach, &b), Err(ExpandError::MissingBinding("ping_target")));
        for target in ["", "-c", "-f", "a b", "a;b", "$(id)", "a/b", &"a".repeat(254)] {
            b.ping_target = Some(target.into());
            assert!(matches!(expand_probe(&reach, &b), Err(ExpandError::BadBinding { what: "ping target", .. })), "{target:?}");
        }
        for target in ["192.0.2.1", "nas.example.org", "2001:db8::1", "gateway"] {
            b.ping_target = Some(target.into());
            assert!(expand_probe(&reach, &b).is_ok(), "{target}");
        }

        // `since: boot` bei Docker und coredumpctl braucht das Alter des Boots
        let boot = probe(r#"{"probe":"coredumps","since":"boot"}"#);
        assert_eq!(expand_probe(&boot, &Bindings::default()), Err(ExpandError::MissingBinding("boot_age_secs")));
        // …bei festem Fenster nicht
        assert!(expand_probe(&probe(r#"{"probe":"coredumps","since":"1h"}"#), &Bindings::default()).is_ok());
    }

    #[test]
    fn no_expanded_element_starts_with_a_dash_unless_it_is_a_literal_or_a_flag_the_template_names() {
        // Aus Parametern entsteht nie ein Element, das mit `-` beginnt, außer den benannten Optionen `--boot=…`,
        // `--since=…`, `--user` (alle mit festem Anfang). Ein Wert, der mit `-` beginnt, kommt nie als eigenes Element.
        let b = bound();
        for probe in full_probes() {
            let Ok(commands) = expand_probe(&probe, &b) else { continue };
            let Exec::Commands(templates) = probe.spec().exec else { continue };
            for (argv, template) in commands.iter().zip(templates) {
                let mut rendered = argv.args.iter();
                for arg in template.args {
                    match arg {
                        Arg::UserFlag if !b.unit.as_ref().unwrap().user => continue,
                        _ => {}
                    }
                    let text = rendered.next().expect("one element per placeholder");
                    match arg {
                        Arg::Lit(lit) => assert_eq!(text, lit),
                        Arg::Unit | Arg::ContainerId | Arg::PingTarget | Arg::Lines => assert!(!text.starts_with('-'), "{}: {text}", probe.name()),
                        Arg::Boot => assert!(text.starts_with("--boot=")),
                        Arg::JournalSince => assert!(text.starts_with("--since=-") || text.starts_with("--boot=")),
                        Arg::DockerSince => assert!(text.starts_with("--since=") && !text.starts_with("--since=-")),
                        Arg::CoreSince => assert!(text.starts_with("--since=-")),
                        Arg::UserFlag => assert_eq!(text, "--user"),
                    }
                }
                assert!(rendered.next().is_none());
            }
        }
    }

    #[test]
    fn read_probes_have_no_command_to_expand() {
        for probe in full_probes() {
            if let Exec::Read(_) = probe.spec().exec {
                assert_eq!(expand_probe(&probe, &bound()), Err(ExpandError::NoCommands), "{}", probe.name());
            }
        }
    }

    #[test]
    fn every_command_probe_expands_with_sample_bindings() {
        let b = bound();
        let mut count = 0;
        for probe in full_probes() {
            if let Exec::Commands(templates) = probe.spec().exec {
                let argv = expand_probe(&probe, &b).unwrap_or_else(|e| panic!("{}: {e}", probe.name()));
                assert_eq!(argv.len(), templates.len());
                for a in argv {
                    assert!(a.args.iter().all(|s| !s.is_empty()), "{}", probe.name());
                    count += 1;
                }
            }
        }
        assert!(count >= 25, "{count}");
    }

    #[test]
    fn error_messages_are_readable() {
        assert!(ExpandError::NoCommands.to_string().contains("reads files"));
        assert!(ExpandError::MissingBinding("unit").to_string().contains("\"unit\""));
        assert!(ExpandError::MissingParam("lines").to_string().contains("\"lines\""));
        assert!(bad("unit name", "must not be empty").to_string().contains("unit name must not be empty"));
    }
}
