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

//! Katalog der Proben: die *eine* Quelle für Gruppe, Beschreibung, Parameter, Grenze, Frist und argv-Vorlage jeder
//! Probe (Katalog v1 in `PLAN-diagnose.md`).
//!
//! Aus dieser Tabelle ([`CATALOG`]) entstehen
//! - das JSON-Schema je Probe ([`ProbeSpec::schema`], [`ProbeSpec::tool_input_schema`], [`combined_schema`]) für
//!   Anthropic Tool-Use (`input_schema`), OpenAI `response_format` und Ollama `format`,
//! - der kompakte Katalogtext für den System-Prompt ([`catalog_text`]),
//! - die Prüfung der Parameter ([`ProbeSpec::check`]), die [`super::probe::parse_validated`] und
//!   [`super::probe::Probe::validate`] benutzen, und
//! - die Vorlagen der Befehle ([`ProbeSpec::exec`], siehe [`super::argv`]) für Ausführer und Tests.
//!
//! Das Schema ist ein Hinweis an das Modell und eine Teilmenge von JSON-Schema (`type`, `enum`, `minimum`, `maximum`,
//! `minLength`, `maxLength`, `properties`, `required`, `additionalProperties`, `anyOf`, `description`), damit jeder
//! Anbieter es versteht; ein Test lässt jede Probe gegen dieses Schema *und* die Prüfung laufen. Einzige Schranke
//! bleibt die Prüfung in Rust: sie ist bei den Suchbegriffen strenger (Zeichen, erstes Zeichen), was ein Schema in
//! dieser Teilmenge nicht ausdrückt; die Parameterbeschreibungen nennen es.
//!
//! **Nur lesende Proben.** Die Begründung je Probe steht als Doku-Kommentar an [`super::probe::Probe`]. Grenze
//! ([`ProbeSpec::limit_bytes`]) und Frist ([`ProbeSpec::deadline`]) setzt der Ausführer (Aufgabe 2) durch: die Grenze
//! gilt für jeden Befehl der Probe und für die Summe, gemessen vor Kürzung und Schwärzung; die Frist (höchstens
//! [`MAX_DEADLINE`]) für die ganze Probe.

use super::argv::{Arg, Command, Exec, Program};
use super::probe::{BootOffset, Days, Lines, ProbeError, Since, WindowMin};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Höchste Frist einer Probe (`PLAN-diagnose.md`: Vorgabe 3 s, höchstens 10 s).
pub const MAX_DEADLINE: Duration = Duration::from_secs(10);

const KIB: usize = 1024;

/// Gruppe im Katalogtext und im Auswahlfenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Boot,
    Units,
    Docker,
    Network,
    Resources,
    Heat,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Boot, Group::Units, Group::Docker, Group::Network, Group::Resources, Group::Heat];

    /// Englische Überschrift.
    pub fn label(self) -> &'static str {
        match self {
            Group::Boot => "Boot",
            Group::Units => "Units",
            Group::Docker => "Docker",
            Group::Network => "Network",
            Group::Resources => "Resources",
            Group::Heat => "Heat",
        }
    }
}

/// Die Typen, auf die der Name einer Unit enden darf (`systemd.unit(5)`).
pub const UNIT_TYPES: &[&str] = &["service", "socket", "target", "timer", "mount", "automount", "swap", "path", "slice", "scope", "device"];

/// Art eines unaufgelösten Suchbegriffs: legt Zeichen und Länge fest. Ein Suchbegriff ist nie ein Argument; die
/// Auflösung gegen die echten Listen (Aufgabe 3) liefert den Wert, der ins argv kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    /// systemd-Unit: bis 128 Zeichen aus Buchstaben, Ziffern und `: _ . @ \ -`. Die Unit `-.mount` (Wurzel) lässt sich
    /// so nicht suchen, weil das erste Zeichen ein Buchstabe oder eine Ziffer sein muss.
    Unit,
    /// Docker-Container (Name oder Anfang der ID): bis 128 Zeichen aus Buchstaben, Ziffern und `_ . -`.
    Container,
    /// Name eines Ping-Ziels aus der Config-Liste: bis 64 Zeichen aus Buchstaben, Ziffern und `_ . : -`.
    Target,
}

impl Term {
    /// Längster Suchbegriff (Zeichen).
    pub const fn max_len(self) -> usize {
        match self {
            Term::Unit | Term::Container => 128,
            Term::Target => 64,
        }
    }

    /// Längster *aufgelöster* Wert (Zeichen): ein Unit-Name darf bis 255 lang sein (systemd), eine Container-ID hat 64
    /// Zeichen, ein Hostname bis 253.
    pub const fn max_resolved_len(self) -> usize {
        match self {
            Term::Unit => 255,
            Term::Container => 64,
            Term::Target => 253,
        }
    }

    fn allows(self, c: char) -> bool {
        c.is_ascii_alphanumeric()
            || match self {
                Term::Unit => matches!(c, ':' | '_' | '.' | '@' | '\\' | '-'),
                Term::Container => matches!(c, '_' | '.' | '-'),
                Term::Target => matches!(c, '_' | '.' | ':' | '-'),
            }
    }

    /// Beschreibung der erlaubten Zeichen für Meldungen und Schema.
    fn charset(self) -> &'static str {
        match self {
            Term::Unit => "letters, digits and : _ . @ \\ -",
            Term::Container => "letters, digits and _ . -",
            Term::Target => "letters, digits and _ . : -",
        }
    }

    fn check_with(self, s: &str, max: usize) -> Result<(), String> {
        let len = s.chars().count();
        if len == 0 {
            return Err("must not be empty".into());
        }
        if len > max {
            return Err(format!("must be at most {max} characters, got {len}"));
        }
        if !s.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err("must start with a letter or digit".into());
        }
        if !s.chars().all(|c| self.allows(c)) {
            return Err(format!("may only contain {}", self.charset()));
        }
        Ok(())
    }

    /// Ist der Suchbegriff gültig? Die Meldung ist die Fortsetzung von „field "x" of probe y …“.
    pub fn check(self, s: &str) -> Result<(), String> {
        self.check_with(s, self.max_len())
    }

    /// Ist ein *aufgelöster* Wert (Name aus einer echten Liste, Ziel aus der Config) zulässig? Dieselben Zeichen,
    /// nur die Länge darf größer sein. Ein Unit-Name muss dazu die volle Form `<Name>.<Typ>` haben (Typ aus
    /// [`UNIT_TYPES`], vor dem Punkt mindestens ein Zeichen): eine reine Zahl deutet `systemctl status` als PID, und
    /// `journalctl -u foo` hängt `.service` an; beides darf ein aufgelöster Name nie auslösen.
    pub fn check_resolved(self, s: &str) -> Result<(), String> {
        self.check_with(s, self.max_resolved_len())?;
        if self == Term::Unit {
            match s.rsplit_once('.') {
                Some((stem, kind)) if !stem.is_empty() && UNIT_TYPES.contains(&kind) => {}
                _ => return Err(format!("must be a full unit name ending in one of .{}", UNIT_TYPES.join(" ."))),
            }
        }
        Ok(())
    }
}

/// Typ und Grenzen eines Parameters; erzeugt Schema, Katalogtext und Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// Genau einer der Texte (Zeitfenster).
    Choice(&'static [&'static str]),
    /// Genau eine der Zahlen (Zeilen, Fenster).
    IntChoice(&'static [i64]),
    /// Ganzzahl von `min` bis `max` (Boot-Versatz).
    IntRange { min: i64, max: i64 },
    /// Unaufgelöster Suchbegriff.
    Term(Term),
}

/// Ein Parameter einer Probe. Alle Parameter sind Pflicht: es gibt keine stillen Vorgaben.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub kind: ParamKind,
    /// Englische Beschreibung für das Modell (Schema).
    pub doc: &'static str,
}

const fn param(name: &'static str, kind: ParamKind, doc: &'static str) -> Param {
    Param { name, kind, doc }
}

/// Eine Probe im Katalog.
#[derive(Debug, Clone, Copy)]
pub struct ProbeSpec {
    /// Wert von `probe` (snake_case).
    pub name: &'static str,
    pub group: Group,
    /// Englische Kurzbeschreibung für den Katalogtext und das Schema.
    pub description: &'static str,
    pub params: &'static [Param],
    /// Höchste Ausgabe in Bytes vor Kürzung und Schwärzung.
    pub limit_bytes: usize,
    /// Frist für die ganze Probe (höchstens [`MAX_DEADLINE`]).
    pub deadline: Duration,
    /// Befehle oder gelesene Dateien.
    pub exec: Exec,
    /// Erzeugt Netzverkehr (nur `net_reach`): läuft nie in einem Profil, nur nach Bestätigung des Users.
    pub network_traffic: bool,
}

const fn entry(
    name: &'static str,
    group: Group,
    description: &'static str,
    params: &'static [Param],
    limit_kib: usize,
    deadline_secs: u64,
    exec: Exec,
) -> ProbeSpec {
    ProbeSpec { name, group, description, params, limit_bytes: limit_kib * KIB, deadline: Duration::from_secs(deadline_secs), exec, network_traffic: false }
}

impl ProbeSpec {
    const fn with_network_traffic(mut self) -> ProbeSpec {
        self.network_traffic = true;
        self
    }
}

const fn cmd(program: Program, args: &'static [Arg]) -> Command {
    Command { program, args }
}

use Arg::Lit as L;

// ---- Parameter ------------------------------------------------------------------------------------------------

const BOOT: Param = param(
    "boot",
    ParamKind::IntRange { min: BootOffset::MIN, max: BootOffset::MAX },
    "Boot offset like journalctl: 0 = current boot, -1 = previous boot, down to -9",
);
const LINES: Param = param("lines", ParamKind::IntChoice(Lines::VALUES), "Number of lines from the end");
const SINCE: Param = param("since", ParamKind::Choice(Since::NAMES), "Time window back from now; boot = since the current boot started");
const UNIT: Param = param(
    "unit",
    ParamKind::Term(Term::Unit),
    "Unit name or part of it, e.g. docker or docker.service (letters, digits and : _ . @ \\ - only, starting with a letter or digit); no path, no option; matched locally against the units that exist",
);
const CONTAINER: Param = param(
    "container",
    ParamKind::Term(Term::Container),
    "Container name or start of its ID (letters, digits and _ . - only, starting with a letter or digit); matched locally against the containers that exist",
);
const TARGET: Param = param(
    "target",
    ParamKind::Term(Term::Target),
    "Name of a ping target from the configured list, e.g. gateway or nas (letters, digits and _ . : - only, starting with a letter or digit); any other target is refused",
);
const WINDOW_MIN: Param = param("window_min", ParamKind::IntChoice(WindowMin::VALUES), "Minutes around the end of the boot");
const DAYS: Param = param("days", ParamKind::IntChoice(Days::VALUES), "Days before the end of the boot");

// ---- Feste Vorlagen -------------------------------------------------------------------------------------------

/// Eigenschaften für `systemctl show -p`: Ergebnis, Exit-Status, Neustarts, Zeiten. Nie `Environment*` und nie
/// `Exec*`-Kommandozeilen (Geheimnisse); ein Test prüft die Liste.
pub const UNIT_SHOW_PROPERTIES: &str = "Id,Description,LoadState,ActiveState,SubState,Result,ExecMainCode,ExecMainStatus,NRestarts,ActiveEnterTimestamp,ActiveExitTimestamp,InactiveEnterTimestamp,InactiveExitTimestamp,StateChangeTimestamp,ExecMainStartTimestamp,ExecMainExitTimestamp,FragmentPath,MemoryCurrent";

/// Vorlage für `docker inspect --format`: ein JSON-Objekt mit Zustand, Neustarts, Health, Image und Mounts (ohne
/// Optionen). Liest nie `Env`, `Labels`, `Cmd` oder `Config` außer `Config.Image`; ein Test prüft das.
pub const DOCKER_STATE_FORMAT: &str = r#"{"status":{{json .State.Status}},"running":{{json .State.Running}},"restarting":{{json .State.Restarting}},"oom_killed":{{json .State.OOMKilled}},"exit_code":{{json .State.ExitCode}},"error":{{json .State.Error}},"started_at":{{json .State.StartedAt}},"finished_at":{{json .State.FinishedAt}},"health":{{if .State.Health}}{{json .State.Health.Status}}{{else}}null{{end}},"health_failing_streak":{{if .State.Health}}{{json .State.Health.FailingStreak}}{{else}}null{{end}},"restart_count":{{json .RestartCount}},"image":{{json .Config.Image}},"mounts":[{{range $i, $m := .Mounts}}{{if $i}},{{end}}{"type":{{json $m.Type}},"source":{{json $m.Source}},"destination":{{json $m.Destination}},"rw":{{json $m.RW}}}{{end}}]}"#;

/// Felder für `nvidia-smi --query-gpu=`: Temperatur, Leistung, Takt, Auslastung, Speicher und Drosselgründe (die
/// Namen `clocks_event_reasons.*` kennt der Treiber auf blub; die älteren `clocks_throttle_reasons.*` sind Aliase).
pub const GPU_QUERY: &str = "--query-gpu=temperature.gpu,power.draw,power.limit,clocks.sm,clocks.mem,utilization.gpu,utilization.memory,memory.used,memory.total,pstate,clocks_event_reasons.active,clocks_event_reasons.hw_thermal_slowdown,clocks_event_reasons.sw_thermal_slowdown,clocks_event_reasons.hw_power_brake_slowdown,clocks_event_reasons.sw_power_cap";

/// Alle Proben in der Reihenfolge von `PLAN-diagnose.md` (Katalog v1, Spalte „Grenze“ in KiB).
pub static CATALOG: &[ProbeSpec] = &[
    // Boot
    entry(
        "boots",
        Group::Boot,
        "List of the last boots with first and last log time, and whether each boot ended cleanly.",
        &[],
        16,
        8,
        Exec::Commands(&[cmd(Program::Journalctl, &[L("--list-boots"), L("--no-pager"), L("-o"), L("json")])]),
    ),
    entry(
        "boot_end",
        Group::Boot,
        "The last journal lines of a boot; with boot -1 the end of the last session before the current one.",
        &[BOOT, LINES],
        64,
        5,
        Exec::Commands(&[cmd(Program::Journalctl, &[Arg::Boot, L("-n"), Arg::Lines, L("-o"), L("short-iso"), L("--no-pager")])]),
    ),
    entry(
        "kernel_errors",
        Group::Boot,
        "Kernel messages at warning level or worse of a boot: Xid, AER, MCE, NVMe, oops, hung tasks, cifs.",
        &[BOOT, LINES],
        64,
        5,
        Exec::Commands(&[cmd(
            Program::Journalctl,
            &[L("-k"), Arg::Boot, L("-p"), L("warning"), L("-o"), L("short-iso"), L("--no-pager"), L("-n"), Arg::Lines],
        )]),
    ),
    entry(
        "pstore",
        Group::Boot,
        "Kernel crash dumps that pstore saved across a crash (dmesg-*), for the day a boot ended.",
        &[BOOT],
        64,
        3,
        Exec::Read(&["/var/lib/systemd/pstore/**/dmesg*"]),
    ),
    entry(
        "temps_tail",
        Group::Boot,
        "The last lines of the temperature log (CPU, NVMe, GPU) around the end of a boot.",
        &[BOOT, WINDOW_MIN],
        32,
        3,
        Exec::Read(&["{temp_log_dir}/temps-*.csv"]),
    ),
    entry(
        "kernel_info",
        Group::Boot,
        "Kernel version, kernel command line (redacted) and BIOS version.",
        &[],
        4,
        3,
        Exec::Read(&["/proc/version", "/proc/cmdline", "/sys/class/dmi/id/bios_version"]),
    ),
    entry(
        "pacman_log",
        Group::Boot,
        "Packages upgraded, installed or removed shortly before the end of a boot (kernel and driver changes).",
        &[BOOT, DAYS],
        32,
        3,
        Exec::Read(&["/var/log/pacman.log"]),
    ),
    entry(
        "driver_versions",
        Group::Boot,
        "Installed versions of the kernel, NVIDIA driver, Mesa, Docker and systemd.",
        &[],
        4,
        3,
        Exec::Commands(&[cmd(Program::Pacman, &[L("-Q"), L("linux"), L("nvidia-open-dkms"), L("nvidia-utils"), L("mesa"), L("docker"), L("systemd")])]),
    ),
    // Units
    entry(
        "failed_units",
        Group::Units,
        "Failed systemd units of the system manager and of the user manager.",
        &[],
        16,
        5,
        Exec::Commands(&[
            cmd(Program::Systemctl, &[L("--failed"), L("--no-pager"), L("-o"), L("json")]),
            cmd(Program::Systemctl, &[L("--user"), L("--failed"), L("--no-pager"), L("-o"), L("json")]),
        ]),
    ),
    entry(
        "unit_status",
        Group::Units,
        "State of one systemd unit without log lines (see unit_journal for those).",
        &[UNIT],
        16,
        3,
        Exec::Commands(&[cmd(Program::Systemctl, &[Arg::UserFlag, L("status"), L("--no-pager"), L("-n"), L("0"), L("--"), Arg::Unit])]),
    ),
    entry(
        "unit_show",
        Group::Units,
        "Result, exit status, restart counter and timestamps of one systemd unit.",
        &[UNIT],
        8,
        3,
        Exec::Commands(&[cmd(Program::Systemctl, &[Arg::UserFlag, L("show"), L("--no-pager"), L("-p"), L(UNIT_SHOW_PROPERTIES), L("--"), Arg::Unit])]),
    ),
    entry(
        "unit_journal",
        Group::Units,
        "Journal lines of one systemd unit.",
        &[UNIT, SINCE, LINES],
        64,
        5,
        Exec::Commands(&[cmd(
            Program::Journalctl,
            &[Arg::UserFlag, L("-u"), Arg::Unit, Arg::JournalSince, L("-n"), Arg::Lines, L("-o"), L("short-iso"), L("--no-pager")],
        )]),
    ),
    entry(
        "timers",
        Group::Units,
        "All systemd timers with their last and next run.",
        &[],
        16,
        3,
        Exec::Commands(&[cmd(Program::Systemctl, &[L("list-timers"), L("--all"), L("--no-pager"), L("-o"), L("json")])]),
    ),
    entry(
        "coredumps",
        Group::Units,
        "List of recorded core dumps (the list only, never their contents).",
        &[SINCE],
        16,
        5,
        Exec::Commands(&[cmd(Program::Coredumpctl, &[L("list"), Arg::CoreSince, L("--no-pager"), L("-q"), L("--json=short")])]),
    ),
    // Docker
    entry(
        "docker_ps",
        Group::Docker,
        "All Docker containers with their state (without labels and commands).",
        &[],
        32,
        5,
        Exec::Commands(&[cmd(Program::Docker, &[L("ps"), L("-a"), L("--format"), L("json")])]),
    ),
    entry(
        "docker_state",
        Group::Docker,
        "State of one container: status, exit code, OOM kill, restarts, health, image and mounts.",
        &[CONTAINER],
        8,
        5,
        Exec::Commands(&[cmd(Program::Docker, &[L("inspect"), L("--type=container"), L("--format"), L(DOCKER_STATE_FORMAT), L("--"), Arg::ContainerId])]),
    ),
    entry(
        "docker_logs",
        Group::Docker,
        "Log lines of one container (always redacted).",
        &[CONTAINER, SINCE, LINES],
        64,
        5,
        Exec::Commands(&[cmd(Program::Docker, &[L("logs"), L("--tail"), Arg::Lines, Arg::DockerSince, L("--timestamps"), L("--"), Arg::ContainerId])]),
    ),
    entry("docker_df", Group::Docker, "Docker disk usage summary.", &[], 4, 5, Exec::Commands(&[cmd(Program::Docker, &[L("system"), L("df")])])),
    entry(
        "docker_stats",
        Group::Docker,
        "Current CPU, memory and I/O use of the running containers.",
        &[],
        16,
        8,
        Exec::Commands(&[cmd(Program::Docker, &[L("stats"), L("--no-stream"), L("--format"), L("json")])]),
    ),
    // Network
    entry(
        "net_links",
        Group::Network,
        "Network interfaces, addresses and routes.",
        &[],
        16,
        3,
        Exec::Commands(&[cmd(Program::Ip, &[L("-j"), L("addr")]), cmd(Program::Ip, &[L("-j"), L("route")])]),
    ),
    entry("net_dns", Group::Network, "DNS resolver state per link.", &[], 8, 3, Exec::Commands(&[cmd(Program::Resolvectl, &[L("status"), L("--no-pager")])])),
    entry(
        "net_listen",
        Group::Network,
        "Listening TCP and UDP sockets with the names of your own processes.",
        &[],
        16,
        3,
        Exec::Commands(&[cmd(Program::Ss, &[L("-Htulpn")])]),
    ),
    entry(
        "net_tailscale",
        Group::Network,
        "Tailscale connection state: name, online, relay and health messages.",
        &[],
        32,
        5,
        Exec::Commands(&[cmd(Program::Tailscale, &[L("status"), L("--json")])]),
    ),
    entry(
        "net_mounts",
        Group::Network,
        "Network file system mounts (CIFS, NFS, sshfs) without credentials.",
        &[],
        8,
        3,
        Exec::Commands(&[cmd(Program::Findmnt, &[L("-J"), L("-t"), L("cifs,nfs,nfs4,fuse.sshfs")])]),
    ),
    entry(
        "net_reach",
        Group::Network,
        "Ping a configured target three times; the only probe that sends network traffic, so it runs only after the user confirms.",
        &[TARGET],
        4,
        6,
        Exec::Commands(&[cmd(Program::Ping, &[L("-c"), L("3"), L("-W"), L("1"), L("--"), Arg::PingTarget])]),
    )
    .with_network_traffic(),
    // Resources
    entry(
        "mem",
        Group::Resources,
        "Memory use and memory pressure.",
        &[],
        4,
        3,
        Exec::Read(&["/proc/meminfo", "/proc/pressure/memory", "/proc/pressure/cpu", "/proc/pressure/io"]),
    ),
    entry("load", Group::Resources, "Load average and CPU use measured over half a second.", &[], 2, 3, Exec::Read(&["/proc/loadavg", "/proc/stat"])),
    entry(
        "disks",
        Group::Resources,
        "Usage of local file systems and the block devices.",
        &[],
        8,
        5,
        Exec::Commands(&[
            cmd(Program::Df, &[L("-h"), L("-l"), L("-x"), L("tmpfs"), L("-x"), L("devtmpfs"), L("-x"), L("overlay")]),
            cmd(Program::Lsblk, &[L("-J"), L("-o"), L("NAME,SIZE,TYPE,MOUNTPOINTS,FSTYPE")]),
        ]),
    ),
    entry(
        "top_procs",
        Group::Resources,
        "The ten largest processes by memory and by CPU (program names only).",
        &[],
        8,
        3,
        Exec::Read(&["/proc/*/stat", "/proc/*/status"]),
    ),
    entry(
        "oom",
        Group::Resources,
        "Out-of-memory kills in the kernel messages of a boot.",
        &[BOOT],
        16,
        8,
        Exec::Commands(&[cmd(
            Program::Journalctl,
            &[L("-k"), Arg::Boot, L("--grep"), L("Out of memory|oom-kill"), L("-o"), L("short-iso"), L("--no-pager"), L("-n"), L("50")],
        )]),
    ),
    // Heat
    entry(
        "sensors",
        Group::Heat,
        "Hardware temperatures, fan speeds and voltages (lm-sensors).",
        &[],
        16,
        3,
        Exec::Commands(&[cmd(Program::Sensors, &[L("-j")])]),
    ),
    entry(
        "gpu",
        Group::Heat,
        "NVIDIA GPU temperature, power, clocks, utilization, memory and throttle reasons.",
        &[],
        4,
        5,
        Exec::Commands(&[cmd(Program::NvidiaSmi, &[L(GPU_QUERY), L("--format=csv")])]),
    ),
    entry("nvme_temp", Group::Heat, "Temperatures of the NVMe drives.", &[], 2, 3, Exec::Read(&["/sys/class/nvme/nvme*/device/hwmon*/temp*_input"])),
];

/// Alle Proben des Katalogs.
pub fn all() -> &'static [ProbeSpec] {
    CATALOG
}

/// Eintrag zu einem Namen (`None` bei unbekannter Probe).
pub fn spec(name: &str) -> Option<&'static ProbeSpec> {
    CATALOG.iter().find(|s| s.name == name)
}

// ---------------------------------------------------------------------------------------------------------------
// Prüfung

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_int(v: &Value) -> Result<i64, String> {
    match v.as_i64() {
        Some(n) => Ok(n),
        None if v.is_number() => Err("must be an integer".into()),
        None => Err(format!("must be an integer, got {}", json_type(v))),
    }
}

impl ParamKind {
    /// Kurzform des Typs für den Katalogtext.
    pub fn type_label(&self) -> String {
        match self {
            ParamKind::Choice(values) => values.join("|"),
            ParamKind::IntChoice(values) => values.iter().map(i64::to_string).collect::<Vec<_>>().join("|"),
            ParamKind::IntRange { min, max } => format!("{min}..{max}"),
            ParamKind::Term(Term::Unit) => "unit-name".into(),
            ParamKind::Term(Term::Container) => "container-name".into(),
            ParamKind::Term(Term::Target) => "target-name".into(),
        }
    }

    /// Ist der Wert nach den Regeln dieses Typs gültig? Die Meldung ist die Fortsetzung von „field "x" of probe y …“.
    pub fn check(&self, v: &Value) -> Result<(), String> {
        match self {
            ParamKind::Choice(values) => match v {
                Value::String(s) if values.iter().any(|value| value == s) => Ok(()),
                Value::String(_) => Err(format!("must be one of: {}", values.join(", "))),
                _ => Err(format!("must be a string (one of: {}), got {}", values.join(", "), json_type(v))),
            },
            ParamKind::IntChoice(values) => {
                let n = check_int(v)?;
                if values.contains(&n) {
                    Ok(())
                } else {
                    Err(format!("must be one of: {}", values.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")))
                }
            }
            ParamKind::IntRange { min, max } => {
                let n = check_int(v)?;
                if (*min..=*max).contains(&n) {
                    Ok(())
                } else {
                    Err(format!("must be between {min} and {max}, got {n}"))
                }
            }
            ParamKind::Term(term) => match v {
                Value::String(s) => term.check(s),
                _ => Err(format!("must be a string, got {}", json_type(v))),
            },
        }
    }

    /// JSON-Schema des Wertes (Teilmenge, siehe Modulkopf).
    pub fn schema(&self) -> Value {
        match self {
            ParamKind::Choice(values) => json!({"type": "string", "enum": values}),
            ParamKind::IntChoice(values) => json!({"type": "integer", "enum": values}),
            ParamKind::IntRange { min, max } => json!({"type": "integer", "minimum": min, "maximum": max}),
            ParamKind::Term(term) => json!({"type": "string", "minLength": 1, "maxLength": term.max_len()}),
        }
    }

    /// Ob Zeichenketten dieses Typs vor der Prüfung von Leerraum am Rand befreit werden (Suchbegriffe); bei
    /// Aufzählungen nicht, dort ist nur der genaue Wert gültig.
    fn trims(&self) -> bool {
        matches!(self, ParamKind::Term(_))
    }
}

impl ProbeSpec {
    /// Der Parameter mit diesem Namen.
    pub fn param(&self, name: &str) -> Option<&'static Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Die Befehle der Probe (leer bei Proben, die in Rust lesen).
    pub fn commands(&self) -> &'static [Command] {
        match self.exec {
            Exec::Commands(commands) => commands,
            Exec::Read(_) => &[],
        }
    }

    /// Befreit Suchbegriffe von Leerraum am Rand; sonst bleibt alles, wie es ist.
    pub(crate) fn normalize(&self, map: &mut Map<String, Value>) {
        for p in self.params.iter().filter(|p| p.kind.trims()) {
            if let Some(Value::String(s)) = map.get_mut(p.name) {
                let trimmed = s.trim();
                if trimmed.len() != s.len() {
                    *s = trimmed.to_string();
                }
            }
        }
    }

    /// Prüft die Felder einer Probe (JSON-Objekt einschließlich `probe`): keine unbekannten Felder, alle Felder da,
    /// jeder Wert nach seinem Typ. Der Wert von `probe` ist schon aufgelöst.
    pub(crate) fn check(&self, map: &Map<String, Value>) -> Result<(), ProbeError> {
        for key in map.keys() {
            if key != "probe" && self.param(key).is_none() {
                return Err(ProbeError::UnknownField { probe: self.name, field: key.clone() });
            }
        }
        for p in self.params {
            match map.get(p.name) {
                None => return Err(ProbeError::MissingField { probe: self.name, field: p.name }),
                Some(v) => p.kind.check(v).map_err(|reason| ProbeError::InvalidField { probe: self.name, field: p.name, reason })?,
            }
        }
        Ok(())
    }

    fn object_schema(&self, with_probe: bool) -> Value {
        let mut properties = Map::new();
        let mut required: Vec<Value> = Vec::new();
        if with_probe {
            properties.insert("probe".into(), json!({"type": "string", "enum": [self.name]}));
            required.push(json!("probe"));
        }
        for p in self.params {
            let mut schema = p.kind.schema();
            if let Value::Object(o) = &mut schema {
                o.insert("description".into(), json!(p.doc));
            }
            properties.insert(p.name.into(), schema);
            required.push(json!(p.name));
        }
        let mut schema = Map::new();
        schema.insert("type".into(), json!("object"));
        schema.insert("description".into(), json!(self.description));
        schema.insert("properties".into(), Value::Object(properties));
        if !required.is_empty() {
            schema.insert("required".into(), Value::Array(required));
        }
        schema.insert("additionalProperties".into(), json!(false));
        Value::Object(schema)
    }

    /// Vollständiges Schema der Probe als Objekt mit dem Feld `probe` (für `response_format`, `format` und die Einträge
    /// von `checks` einer Modellantwort).
    pub fn schema(&self) -> Value {
        self.object_schema(true)
    }

    /// Schema nur der Parameter, ohne `probe` (für `input_schema` eines Tools; der Name des Tools ist die Probe).
    pub fn tool_input_schema(&self) -> Value {
        self.object_schema(false)
    }

    /// Eine Zeile des Katalogtextes: `name(param: typ, …): Beschreibung`.
    fn text_line(&self) -> String {
        let params: Vec<String> = self.params.iter().map(|p| format!("{}: {}", p.name, p.kind.type_label())).collect();
        let mut line = format!("- {}", self.name);
        if !params.is_empty() {
            line.push_str(&format!("({})", params.join(", ")));
        }
        line.push_str(": ");
        line.push_str(self.description);
        line
    }
}

/// Ein Schema für jede Probe: `{"anyOf": [<Schema je Probe>]}`; genau eine Alternative passt über `probe`.
pub fn combined_schema() -> Value {
    json!({"anyOf": CATALOG.iter().map(ProbeSpec::schema).collect::<Vec<_>>()})
}

/// Schema eines Prüfbefehls in einer Modellantwort: **ein** flaches Objekt mit `probe` (Aufzählung aller Proben) und
/// jedem Parameter des Katalogs als Pflichtfeld, das den Wert trägt oder `null` ist, wenn die Probe den Parameter nicht
/// hat. Welche Kombination gilt, prüft Rust ([`super::probe::parse_value`] nach Entfernen der `null`-Felder).
///
/// Warum nicht [`combined_schema`]: Mit `strict` übersetzt die API das Schema in eine Grammatik, und die Vereinigung der damals 34
/// Objekte lehnte sie ab („The compiled grammar is too large“, gemessen am 05.11.2026, `diagnose-ai.md`); ohne strict
/// lieferte Haiku 5.5 unvollständige Antworten und viele ungültige Prüfbefehle. Die flache Form hat 8 Felder „Wert oder
/// null“ (Grenze der API: 16 Vereinigungen) und kein optionales Feld. Ein Parameter mit verschiedenen Schemas je Probe
/// (bis W7 etwa `boot`: frei bei den Journalen, `0|-1` bei der Probe `xorg_log`) bekommt den gemeinsamen Typ; die Grenzen der einzelnen
/// Probe prüft Rust. Die Beschreibung nennt die Proben, die den Parameter haben.
pub fn flat_schema() -> Value {
    let mut order: Vec<&'static str> = Vec::new();
    let mut merged: std::collections::HashMap<&'static str, (Value, &'static str, Vec<&'static str>)> = std::collections::HashMap::new();
    for spec in CATALOG {
        for p in spec.params {
            let schema = p.kind.schema();
            match merged.get_mut(p.name) {
                None => {
                    order.push(p.name);
                    merged.insert(p.name, (schema, p.doc, vec![spec.name]));
                }
                Some((have, _, users)) => {
                    users.push(spec.name);
                    if *have != schema {
                        assert_eq!(have["type"], schema["type"], "parameter {} has different types in the catalog", p.name);
                        *have = json!({"type": schema["type"].clone()});
                    }
                }
            }
        }
    }
    let mut properties = Map::new();
    properties.insert("probe".into(), json!({"type": "string", "enum": CATALOG.iter().map(|s| s.name).collect::<Vec<_>>()}));
    for name in &order {
        let (schema, doc, users) = &merged[name];
        properties.insert(
            (*name).into(),
            json!({"anyOf": [schema, {"type": "null"}], "description": format!("{doc}. Only for {}; null for every other probe", users.join(", "))}),
        );
    }
    let required: Vec<Value> = properties.keys().map(|k| json!(k)).collect();
    json!({
        "type": "object",
        "description": "One probe of the catalog: its name and its parameters, null for the parameters it does not have",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// Kompakter Katalogtext für den System-Prompt: nach Gruppen, eine Zeile je Probe mit Parametern. Der Prompt drumherum
/// (Schutzrahmen, Anweisung zur Antwort) kommt mit dem KI-Pfad (Aufgabe 6).
pub fn catalog_text() -> String {
    let mut out = String::new();
    for group in Group::ALL {
        out.push_str(group.label());
        out.push_str(":\n");
        for s in CATALOG.iter().filter(|s| s.group == group) {
            out.push_str(&s.text_line());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::probe::{parse_validated, Probe};
    use crate::diagnose::test_support::{examples, full_probes, rejected, schema_accepts};
    use std::collections::BTreeSet;

    /// Die Proben des Plans (Katalog v1) in seiner Reihenfolge.
    const PLAN_NAMES: [&str; 33] = [
        "boots",
        "boot_end",
        "kernel_errors",
        "pstore",
        "temps_tail",
        "kernel_info",
        "pacman_log",
        "driver_versions",
        "failed_units",
        "unit_status",
        "unit_show",
        "unit_journal",
        "timers",
        "coredumps",
        "docker_ps",
        "docker_state",
        "docker_logs",
        "docker_df",
        "docker_stats",
        "net_links",
        "net_dns",
        "net_listen",
        "net_tailscale",
        "net_mounts",
        "net_reach",
        "mem",
        "load",
        "disks",
        "top_procs",
        "oom",
        "sensors",
        "gpu",
        "nvme_temp",
    ];

    #[test]
    fn the_catalog_has_exactly_the_probes_of_the_plan() {
        let names: Vec<&str> = CATALOG.iter().map(|s| s.name).collect();
        assert_eq!(names, PLAN_NAMES);
        let unique: BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "duplicate names");
        for name in names {
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name} is not snake_case");
        }
        // Reihenfolge der Beispiele und der vollständigen Proben ist die des Katalogs
        let full: Vec<&str> = full_probes().iter().map(Probe::name).collect();
        assert_eq!(full, PLAN_NAMES);
    }

    #[test]
    fn groups_limits_and_deadlines_follow_the_plan() {
        // (Name, Gruppe, Grenze in KiB) wie in der Tabelle „Katalog v1“
        let plan: [(&str, Group, usize); 33] = [
            ("boots", Group::Boot, 16),
            ("boot_end", Group::Boot, 64),
            ("kernel_errors", Group::Boot, 64),
            ("pstore", Group::Boot, 64),
            ("temps_tail", Group::Boot, 32),
            ("kernel_info", Group::Boot, 4),
            ("pacman_log", Group::Boot, 32),
            ("driver_versions", Group::Boot, 4),
            ("failed_units", Group::Units, 16),
            ("unit_status", Group::Units, 16),
            ("unit_show", Group::Units, 8),
            ("unit_journal", Group::Units, 64),
            ("timers", Group::Units, 16),
            ("coredumps", Group::Units, 16),
            ("docker_ps", Group::Docker, 32),
            ("docker_state", Group::Docker, 8),
            ("docker_logs", Group::Docker, 64),
            ("docker_df", Group::Docker, 4),
            ("docker_stats", Group::Docker, 16),
            ("net_links", Group::Network, 16),
            ("net_dns", Group::Network, 8),
            ("net_listen", Group::Network, 16),
            ("net_tailscale", Group::Network, 32),
            ("net_mounts", Group::Network, 8),
            ("net_reach", Group::Network, 4),
            ("mem", Group::Resources, 4),
            ("load", Group::Resources, 2),
            ("disks", Group::Resources, 8),
            ("top_procs", Group::Resources, 8),
            ("oom", Group::Resources, 16),
            ("sensors", Group::Heat, 16),
            ("gpu", Group::Heat, 4),
            ("nvme_temp", Group::Heat, 2),
        ];
        for (name, group, kib) in plan {
            let s = spec(name).unwrap();
            assert_eq!(s.group, group, "{name}");
            assert_eq!(s.limit_bytes, kib * 1024, "{name}");
        }
        for s in CATALOG {
            assert!(s.deadline >= Duration::from_secs(1) && s.deadline <= MAX_DEADLINE, "{}: {:?}", s.name, s.deadline);
            assert!(s.limit_bytes >= 1024 && s.limit_bytes <= 64 * 1024, "{}", s.name);
        }
        // Jede Gruppe hat Proben, und jede Gruppe aus der Liste kommt vor
        let used: std::collections::HashSet<Group> = CATALOG.iter().map(|s| s.group).collect();
        assert_eq!(used.len(), Group::ALL.len());
        let labels: BTreeSet<&str> = Group::ALL.iter().map(|g| g.label()).collect();
        assert_eq!(labels.len(), Group::ALL.len());
    }

    #[test]
    fn only_net_reach_sends_network_traffic() {
        let traffic: Vec<&str> = CATALOG.iter().filter(|s| s.network_traffic).map(|s| s.name).collect();
        assert_eq!(traffic, ["net_reach"]);
        // …und es ist auch der einzige Befehl, der ein Netzwerkziel anspricht
        for s in CATALOG {
            let pings = s.commands().iter().any(|c| c.program == Program::Ping);
            assert_eq!(pings, s.network_traffic, "{}", s.name);
        }
    }

    #[test]
    fn descriptions_and_parameter_docs_are_complete_english() {
        for s in CATALOG {
            assert!(!s.description.is_empty() && s.description.ends_with('.'), "{}", s.name);
            assert!(s.description.is_ascii(), "{}: visible texts are English", s.name);
            assert!(s.description.len() < 160, "{}", s.name);
            for p in s.params {
                assert!(!p.doc.is_empty() && p.doc.is_ascii(), "{}.{}", s.name, p.name);
            }
            let names: BTreeSet<&str> = s.params.iter().map(|p| p.name).collect();
            assert_eq!(names.len(), s.params.len(), "{}: duplicate parameter names", s.name);
        }
    }

    #[test]
    fn schema_fields_equal_struct_fields() {
        for probe in full_probes() {
            let spec = probe.spec();
            let Value::Object(map) = serde_json::to_value(&probe).unwrap() else { panic!("not an object") };
            let struct_fields: BTreeSet<&str> = map.keys().map(String::as_str).filter(|k| *k != "probe").collect();
            let spec_fields: BTreeSet<&str> = spec.params.iter().map(|p| p.name).collect();
            assert_eq!(struct_fields, spec_fields, "fields of {} differ between the type and the catalog", spec.name);
            // Und das erzeugte Schema nennt genau diese Felder
            let schema = spec.schema();
            let schema_fields: BTreeSet<&str> = schema["properties"].as_object().unwrap().keys().map(String::as_str).filter(|k| *k != "probe").collect();
            assert_eq!(schema_fields, spec_fields, "schema fields of {}", spec.name);
            let tool = spec.tool_input_schema();
            assert!(tool["properties"].as_object().unwrap().keys().all(|k| k != "probe"));
            // Alle Felder sind Pflicht
            let required: BTreeSet<&str> = schema["required"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
            let expected: BTreeSet<&str> = spec_fields.iter().copied().chain(["probe"]).collect();
            assert_eq!(required, expected, "required fields of {}", spec.name);
            assert_eq!(schema["additionalProperties"], json!(false));
            assert_eq!(schema["description"], json!(spec.description));
        }
    }

    #[test]
    fn schema_values_come_from_the_enums() {
        // Aufzählungen im Schema = Werte der Typen (eine Quelle)
        let unit_journal = spec("unit_journal").unwrap().schema();
        assert_eq!(unit_journal["properties"]["since"]["enum"], json!(Since::NAMES));
        assert_eq!(unit_journal["properties"]["lines"]["enum"], json!(Lines::VALUES));
        assert_eq!(unit_journal["properties"]["since"]["enum"], json!(["15m", "1h", "6h", "24h", "boot"]));
        assert_eq!(unit_journal["properties"]["lines"]["enum"], json!([50, 200, 500]));
        assert_eq!(unit_journal["properties"]["unit"], json!({"type": "string", "minLength": 1, "maxLength": 128, "description": UNIT.doc}));
        let boot_end = spec("boot_end").unwrap().schema();
        assert_eq!(boot_end["properties"]["boot"], json!({"type": "integer", "minimum": -9, "maximum": 0, "description": BOOT.doc}));
        assert_eq!(boot_end["properties"]["probe"], json!({"type": "string", "enum": ["boot_end"]}));
        assert_eq!(boot_end["required"], json!(["probe", "boot", "lines"]));
        assert_eq!(spec("temps_tail").unwrap().schema()["properties"]["window_min"]["enum"], json!(WindowMin::VALUES));
        assert_eq!(spec("pacman_log").unwrap().schema()["properties"]["days"]["enum"], json!(Days::VALUES));
        assert_eq!(spec("net_reach").unwrap().schema()["properties"]["target"]["maxLength"], json!(64));
        assert_eq!(spec("docker_state").unwrap().schema()["properties"]["container"]["maxLength"], json!(128));
        // Bereich des Boot-Versatzes = Grenzen des Typs
        assert_eq!((BootOffset::MIN, BootOffset::MAX), (-9, 0));
        // Probe ohne Parameter
        let boots = spec("boots").unwrap();
        assert_eq!(boots.schema()["required"], json!(["probe"]));
        assert_eq!(boots.tool_input_schema()["properties"], json!({}));
        assert!(boots.tool_input_schema().get("required").is_none());
    }

    #[test]
    fn examples_are_valid_for_schema_and_validation() {
        let mut seen = BTreeSet::new();
        for (json, expected) in examples() {
            let value: Value = serde_json::from_str(json).unwrap();
            let probe = parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(probe, expected, "{json}");
            assert!(schema_accepts(&probe.spec().schema(), &value), "schema of {} rejects {json}", probe.name());
            assert!(schema_accepts(&combined_schema(), &value), "combined schema rejects {json}");
            // Das Schema der Tool-Eingabe passt auf dieselben Parameter ohne `probe`
            let mut params = value.as_object().unwrap().clone();
            params.remove("probe");
            assert!(schema_accepts(&probe.spec().tool_input_schema(), &Value::Object(params)), "tool schema of {} rejects {json}", probe.name());
            // Und das Schema einer anderen Probe nimmt das Beispiel nicht an
            for other in CATALOG.iter().filter(|s| s.name != probe.name()) {
                assert!(!schema_accepts(&other.schema(), &value), "schema of {} accepts {json}", other.name);
            }
            seen.insert(probe.name());
        }
        let all: BTreeSet<&str> = CATALOG.iter().map(|s| s.name).collect();
        assert_eq!(seen, all, "every probe needs an example");
    }

    #[test]
    fn invalid_cases_are_rejected_by_the_schema_where_it_can_express_them() {
        for (json, _msg, schema_rejects) in rejected() {
            let value: Value = serde_json::from_str(&json).unwrap();
            if let Some(spec) = value.get("probe").and_then(Value::as_str).and_then(spec) {
                assert_eq!(!schema_accepts(&spec.schema(), &value), schema_rejects, "schema verdict for {json}");
            }
            if schema_rejects {
                assert!(!schema_accepts(&combined_schema(), &value), "combined schema accepts {json}");
            }
        }
    }

    #[test]
    fn boundaries_agree_between_schema_and_validation() {
        // Der letzte gültige Wert jeder Grenze: Schema und Prüfung nehmen ihn an; der erste ungültige steht in `rejected`.
        for (json, ok) in [
            (r#"{"probe":"boot_end","boot":0,"lines":50}"#, true),
            (r#"{"probe":"boot_end","boot":-9,"lines":500}"#, true),
            (r#"{"probe":"boot_end","boot":-10,"lines":500}"#, false),
            (r#"{"probe":"boot_end","boot":1,"lines":500}"#, false),
            (r#"{"probe":"temps_tail","boot":-1,"window_min":60}"#, true),
            (r#"{"probe":"pacman_log","boot":-1,"days":1}"#, true),
            (r#"{"probe":"coredumps","since":"boot"}"#, true),
        ] {
            let value: Value = serde_json::from_str(json).unwrap();
            assert_eq!(parse_validated(json).is_ok(), ok, "{json}");
            assert_eq!(schema_accepts(&combined_schema(), &value), ok, "{json}");
        }
        for term in ["unit_status", "docker_state"] {
            let key = if term == "unit_status" { "unit" } else { "container" };
            let at_limit = format!(r#"{{"probe":"{term}","{key}":"{}"}}"#, "a".repeat(128));
            assert!(parse_validated(&at_limit).is_ok(), "{term}");
            assert!(schema_accepts(&combined_schema(), &serde_json::from_str(&at_limit).unwrap()), "{term}");
        }
        let at_limit = format!(r#"{{"probe":"net_reach","target":"{}"}}"#, "a".repeat(64));
        assert!(parse_validated(&at_limit).is_ok());
        assert!(schema_accepts(&combined_schema(), &serde_json::from_str(&at_limit).unwrap()));
    }

    #[test]
    fn the_schema_does_not_express_everything_rust_checks() {
        // Zeichen und erstes Zeichen der Suchbegriffe prüft nur Rust (das Schema kennt hier kein `pattern`); die
        // Beschreibung des Parameters nennt die Regeln dem Modell.
        let value: Value = serde_json::from_str(r#"{"probe":"unit_status","unit":"--user"}"#).unwrap();
        assert!(schema_accepts(&spec("unit_status").unwrap().schema(), &value));
        assert!(parse_validated(r#"{"probe":"unit_status","unit":"--user"}"#).is_err());
        for p in [UNIT, CONTAINER, TARGET] {
            assert!(p.doc.contains("letters, digits"), "{}", p.name);
            assert!(p.doc.contains("starting with a letter or digit"), "{}", p.name);
        }
    }

    #[test]
    fn catalog_text_is_compact_and_complete() {
        let text = catalog_text();
        for s in CATALOG {
            let line = text
                .lines()
                .find(|l| l.starts_with(&format!("- {}", s.name)) && l[2 + s.name.len()..].starts_with(['(', ':']))
                .unwrap_or_else(|| panic!("{} missing in the catalog text", s.name));
            assert!(line.contains(s.description), "{line}");
            for p in s.params {
                assert!(line.contains(&format!("{}: ", p.name)), "{line}");
            }
        }
        for g in Group::ALL {
            assert!(text.contains(&format!("{}:\n", g.label())));
        }
        assert!(text.contains("- boots: List of the last boots"), "{text}");
        assert!(text.contains("- boot_end(boot: -9..0, lines: 50|200|500): "), "{text}");
        assert!(text.contains("- unit_journal(unit: unit-name, since: 15m|1h|6h|24h|boot, lines: 50|200|500): "), "{text}");
        assert!(text.contains("- temps_tail(boot: -9..0, window_min: 5|15|60): "), "{text}");
        assert!(text.contains("- pacman_log(boot: -9..0, days: 1|3|7): "), "{text}");
        assert!(!text.contains("xorg_log"), "{text}");
        assert!(text.contains("- docker_logs(container: container-name, since: 15m|1h|6h|24h|boot, lines: 50|200|500): "), "{text}");
        assert!(text.contains("- net_reach(target: target-name): "), "{text}");
        assert!(text.len() < 6000, "catalog text is {} bytes", text.len());
        assert_eq!(text.lines().filter(|l| l.starts_with("- ")).count(), CATALOG.len());
    }

    #[test]
    fn schema_shapes_for_providers() {
        let c = combined_schema();
        assert_eq!(c["anyOf"].as_array().unwrap().len(), CATALOG.len());
        // Das Schema lässt sich als JSON ausgeben und wieder lesen
        assert_eq!(serde_json::from_str::<Value>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        // Nur Schlüsselwörter der gemeinsamen Teilmenge (der Prüfer in `schema_accepts` bricht bei anderen ab) und
        // eine Größe, die in einen Prompt passt
        assert!(serde_json::to_string(&c).unwrap().len() < 24 * 1024, "combined schema is {} bytes", serde_json::to_string(&c).unwrap().len());
    }

    #[test]
    fn term_rules() {
        assert_eq!(Term::Unit.max_len(), 128);
        assert_eq!(Term::Unit.max_resolved_len(), 255);
        assert_eq!(Term::Container.max_resolved_len(), 64);
        assert_eq!(Term::Target.max_len(), 64);
        assert!(Term::Unit.check_resolved(&format!("{}.service", "a".repeat(247))).is_ok());
        assert!(Term::Unit.check_resolved(&format!("{}.service", "a".repeat(248))).is_err());
        assert!(Term::Unit.check(&"a".repeat(255)).is_err());
        // Ein aufgelöster Unit-Name hat die volle Form mit Typ: nie eine Zahl (PID) und nie ein Name ohne Suffix
        for ok in ["docker.service", "getty@tty1.service", "dev-disk-by\\x2duuid-1.device", "a.b.timer", "x.slice", "x.scope", "boot.mount"] {
            assert!(Term::Unit.check_resolved(ok).is_ok(), "{ok}");
        }
        for bad in ["1234", "foo", "docker", ".service", "a.", "a.unknown", "a.Service", "a.service.bak", "service", "a..b"] {
            assert!(Term::Unit.check_resolved(bad).is_err(), "{bad}");
        }
        // `check` zählt Zeichen, nicht Bytes: ein Umlaut ist ohnehin verboten
        assert!(Term::Unit.check("ä").is_err());
        for kind in [Term::Unit, Term::Container, Term::Target] {
            for bad in ["", "-x", " x", "x ", "x\ty", "x/y", "x;y", "x|y", "x`y", "x$y", "x'y", "x\"y", "x\0", "x*", "x?", "x\u{202e}"] {
                assert!(kind.check(bad).is_err(), "{kind:?} {bad:?}");
                assert!(kind.check_resolved(bad).is_err(), "{kind:?} {bad:?}");
            }
        }
        assert_eq!(ParamKind::Term(Term::Unit).type_label(), "unit-name");
        assert_eq!(ParamKind::IntRange { min: -9, max: 0 }.type_label(), "-9..0");
    }
}
