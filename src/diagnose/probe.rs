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

//! Proben (Probes) der Linux-Diagnose und ihre Validierung.
//!
//! Eine [`Probe`] ist ein Eintrag aus dem festen Katalog ([`super::catalog`]) mit engen, typisierten Parametern, z. B.
//! `{"probe":"unit_journal","unit":"docker.service","since":"1h","lines":200}`. Weder das Modell noch der User tippt
//! einen Befehl: Sammeln und Prüfbefehle der KI benutzen denselben Katalog, und das Modell kann nur Einträge daraus
//! vorschlagen. Die Parameter sind Aufzählungen (Zeitfenster, Zeilen, Boot-Versatz, Fenster) oder *unaufgelöste
//! Suchbegriffe* ([`UnitRef`], [`ContainerRef`], [`PingTarget`]); die Auflösung gegen die echten Listen (Aufgabe 3) liefert
//! erst die Werte, die in ein argv kommen ([`super::argv`]).
//!
//! **Eine Quelle für die Regeln:** Typen und Grenzen stehen in [`super::catalog`] (je Parameter eine
//! [`super::catalog::ParamKind`]); die Namen und Werte der Aufzählungen stammen aus den Literalen der Makros
//! `name_enum!` und `int_enum!` hier (`NAMES`, `VALUES`) und gehen von dort in das Schema. Daraus entstehen das
//! JSON-Schema für das Modell *und* die Prüfung ([`parse_validated`], [`Probe::validate`]); beide können deshalb nicht
//! auseinanderlaufen. Die Typen in diesem Modul tragen nur die Daten; Tests vergleichen ihre Felder mit dem Katalog.
//!
//! **Jede Probe ist nur lesend.** Die Begründung („Read-only, weil …“) steht als Doku-Kommentar an der Variante von
//! [`Probe`]; ein Test lässt den Build scheitern, wenn eine fehlt. Die Rust-Validierung ist die einzige Schranke:
//! unbekannte Probe, unbekanntes Feld, Wert außerhalb des Bereichs und fehlendes Pflichtfeld sind Fehler, nichts wird
//! stillschweigend korrigiert (außer dem Trimmen von Suchbegriffen).

use super::catalog::{self, ProbeSpec, Term};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::fmt;

/// Längste angenommene Probe als JSON (Bytes); eine Probe ist ein paar Dutzend Bytes groß.
pub const MAX_JSON_BYTES: usize = 4 * 1024;

/// Deklariert ein Enum, dessen Namen auf dem Draht (serde), im Schema und im Katalogtext aus *einem* Literal je Wert
/// stammen. `ALL` und `NAMES` stehen in derselben Reihenfolge wie die Werte.
macro_rules! name_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $var:ident = $s:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $s)] $var,)+
        }

        impl $name {
            /// Alle Werte in der Reihenfolge der Deklaration.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// Die Namen auf dem Draht in derselben Reihenfolge.
            pub const NAMES: &'static [&'static str] = &[$($s),+];

            /// Der Name auf dem Draht.
            pub const fn name(self) -> &'static str {
                match self {
                    $($name::$var => $s,)+
                }
            }
        }
    };
}

/// Wie `name_enum!`, nur mit Ganzzahlen auf dem Draht (`VALUES`); jede andere Zahl lehnt das Lesen ab.
macro_rules! int_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $var:ident = $n:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $var,)+
        }

        impl $name {
            /// Alle Werte in der Reihenfolge der Deklaration.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// Die Zahlen auf dem Draht in derselben Reihenfolge.
            pub const VALUES: &'static [i64] = &[$($n),+];

            /// Die Zahl auf dem Draht.
            pub const fn value(self) -> i64 {
                match self {
                    $($name::$var => $n,)+
                }
            }

            /// Der Wert zu einer Zahl (`None`, wenn sie nicht zur Aufzählung gehört).
            pub fn from_value(n: i64) -> Option<$name> {
                match n {
                    $($n => Some($name::$var),)+
                    _ => None,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_i64(self.value())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let n = i64::deserialize(d)?;
                $name::from_value(n).ok_or_else(|| {
                    let values: Vec<String> = $name::VALUES.iter().map(i64::to_string).collect();
                    de::Error::custom(format!("{n} is not one of: {}", values.join(", ")))
                })
            }
        }
    };
}

name_enum! {
    /// Zeitfenster zurück von jetzt (`unit_journal`, `docker_logs`, `coredumps`); `boot` = seit dem Start des laufenden Boots.
    Since {
        Min15 = "15m",
        Hour1 = "1h",
        Hours6 = "6h",
        Hours24 = "24h",
        Boot = "boot",
    }
}

impl Since {
    /// Dauer als Text („15m“, „1h“, „6h“, „24h“), für `boot` `None` (der Beginn des Boots ist kein fester Abstand).
    pub const fn duration(self) -> Option<&'static str> {
        match self {
            Since::Min15 => Some("15m"),
            Since::Hour1 => Some("1h"),
            Since::Hours6 => Some("6h"),
            Since::Hours24 => Some("24h"),
            Since::Boot => None,
        }
    }
}

int_enum! {
    /// Anzahl der Zeilen vom Ende (`boot_end`, `kernel_errors`, `unit_journal`, `docker_logs`).
    Lines {
        N50 = 50,
        N200 = 200,
        N500 = 500,
    }
}

impl Lines {
    /// Die Zahl als `u32`.
    pub const fn count(self) -> u32 {
        self.value() as u32
    }
}

int_enum! {
    /// Fenster in Minuten um das Ende eines Boots (`temps_tail`).
    WindowMin {
        M5 = 5,
        M15 = 15,
        M60 = 60,
    }
}

int_enum! {
    /// Fenster in Tagen vor dem Ende eines Boots (`pacman_log`).
    Days {
        D1 = 1,
        D3 = 3,
        D7 = 7,
    }
}

/// Boot-Versatz wie bei `journalctl --boot`: 0 = laufender Boot, -1 = der davor, bis -9. Ob es den Boot gibt, prüft
/// erst die Auflösung gegen `journalctl --list-boots` (Aufgabe 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BootOffset(i8);

impl BootOffset {
    /// Kleinster Versatz (der älteste Boot).
    pub const MIN: i64 = -9;
    /// Größter Versatz (der laufende Boot).
    pub const MAX: i64 = 0;
    /// Der laufende Boot.
    pub const CURRENT: BootOffset = BootOffset(0);
    /// Der Boot davor.
    pub const PREVIOUS: BootOffset = BootOffset(-1);

    /// Der Versatz zu einer Zahl (`None` außerhalb von -9 bis 0).
    pub fn new(n: i64) -> Option<BootOffset> {
        (Self::MIN..=Self::MAX).contains(&n).then_some(BootOffset(n as i8))
    }

    /// Die Zahl (0 bis -9).
    pub const fn offset(self) -> i8 {
        self.0
    }
}

impl Serialize for BootOffset {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i64(i64::from(self.0))
    }
}

impl<'de> Deserialize<'de> for BootOffset {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let n = i64::deserialize(d)?;
        BootOffset::new(n).ok_or_else(|| de::Error::custom(format!("{n} is not a boot offset between {} and {}", BootOffset::MIN, BootOffset::MAX)))
    }
}

/// Deklariert einen unaufgelösten Suchbegriff: ein String, dessen Regeln [`Term::check`] festlegt. Das Feld ist
/// privat; es gibt nur gültige Werte.
macro_rules! term_type {
    ($(#[$meta:meta])* $name:ident, $term:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Der Begriff, wenn er die Regeln erfüllt (die Meldung ist die Fortsetzung von „field … “).
            pub fn new(s: &str) -> Result<$name, String> {
                $term.check(s)?;
                Ok($name(s.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                $name::new(&s).map_err(de::Error::custom)
            }
        }
    };
}

term_type! {
    /// Suchbegriff für eine systemd-Unit („docker“, „docker.service“); Zeichen und Länge siehe [`Term::Unit`]. Gilt
    /// erst nach der Auflösung gegen `systemctl list-units --all` (System und User), nie als Argument.
    UnitRef, Term::Unit
}

term_type! {
    /// Suchbegriff für einen Docker-Container (Name oder Anfang der ID); Zeichen und Länge siehe [`Term::Container`].
    /// Gilt erst nach der Auflösung gegen `docker ps -a`; eingesetzt wird die ID, nie dieser Text.
    ContainerRef, Term::Container
}

term_type! {
    /// Name eines Ping-Ziels aus der Config-Liste („gateway“, „nas“, „1.1.1.1“); Zeichen und Länge siehe
    /// [`Term::Target`]. Gilt erst nach der Auflösung gegen diese Liste; das Modell erfindet kein Ziel.
    PingTarget, Term::Target
}

/// Eine Probe aus dem Katalog v1 (`PLAN-diagnose.md`). Auf dem Draht ein JSON-Objekt mit `probe` als Name, z. B.
/// `{"probe":"boot_end","boot":-1,"lines":200}`. Unbekannte Felder werden abgelehnt, alle Felder sind Pflicht.
///
/// Jede Variante trägt ihre Begründung „Read-only, weil …“ (der Test `every_probe_documents_why_it_is_read_only`
/// prüft das am Quelltext). Befehle und Grenzen je Probe stehen im Katalog ([`super::catalog::CATALOG`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "probe", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    // ---- Boot
    /// Liste der letzten Boots mit Zeiten und Ende (sauber oder unsauber).
    ///
    /// Read-only, weil `journalctl --list-boots` nur die Journaldateien liest und eine Liste ausgibt; `--rotate`,
    /// `--flush`, `--sync` und `--vacuum*` stehen in keiner Vorlage, kein Parameter fließt ins argv.
    Boots {},
    /// Ende des Journals eines Boots.
    ///
    /// Read-only, weil `journalctl` mit `--boot=<n>`, `-n <Zeilen>`, `-o short-iso` und `--no-pager` nur liest; Boot und
    /// Zeilen kommen aus festen Aufzählungen, der Boot wird vorher gegen `--list-boots` geprüft.
    BootEnd { boot: BootOffset, lines: Lines },
    /// Kernelmeldungen ab Warnstufe eines Boots (Xid, AER, MCE, NVMe, Oops, Hung Task, cifs).
    ///
    /// Read-only, weil `journalctl -k -p warning` nur liest, mit festem Filter und begrenzter Zeilenzahl; der Journalzugriff
    /// kommt über die Gruppe `log`, nicht über `dmesg` oder root.
    KernelErrors { boot: BootOffset, lines: Lines },
    /// Kernel-Abstürze, die pstore nach einem Absturz gerettet hat (`dmesg-*`) zum Tag eines Boots.
    ///
    /// Read-only, weil Rust nur Dateien `dmesg*` unter `/var/lib/systemd/pstore/` schreibgeschützt öffnet und liest; es
    /// wird nichts gelöscht (anders als `systemd-pstore`, das archiviert und löscht) und kein Programm gestartet.
    Pstore { boot: BootOffset },
    /// Letzte Zeilen des temp-logger-Protokolls (CPU, NVMe, GPU) um das Ende eines Boots.
    ///
    /// Read-only, weil Rust nur `temps-<Datum>.csv` im festen Ordner der Config (`diagnose.temp_log_dir`) liest; der Name
    /// der Datei wird aus Datum gebildet, kein Pfad stammt vom Modell.
    TempsTail { boot: BootOffset, window_min: WindowMin },
    /// Kernelversion, Kernel-Kommandozeile (geschwärzt) und BIOS-Version.
    ///
    /// Read-only, weil Rust nur `/proc/version`, `/proc/cmdline` und `/sys/class/dmi/id/bios_version` liest; die
    /// Kommandozeile kann Schlüssel enthalten und wird geschwärzt.
    KernelInfo {},
    /// Pakete, die vor dem Ende eines Boots aktualisiert, installiert oder entfernt wurden.
    ///
    /// Read-only, weil Rust nur `/var/log/pacman.log` liest und nur die `[ALPM]`-Zeilen `upgraded`, `installed`,
    /// `removed`, `downgraded` und `reinstalled` ausgibt (ein Downgrade des Kernels gehört dazu); `pacman` selbst läuft
    /// dafür nicht.
    PacmanLog { boot: BootOffset, days: Days },
    /// Versionen von Kernel, NVIDIA-Treiber, Mesa, Docker und systemd.
    ///
    /// Read-only, weil `pacman -Q` die lokale Paketdatenbank abfragt, keine Rechte braucht und nichts ändert; die
    /// Paketliste ist fest, `-S`, `-R` und `-U` kommen in keiner Vorlage vor.
    DriverVersions {},
    // ---- Units
    /// Fehlgeschlagene Units des Systems und des User-Managers.
    ///
    /// Read-only, weil `systemctl --failed` nur listet (`-o json`, `--no-pager`); es gibt weder `reset-failed` noch
    /// `restart`.
    FailedUnits {},
    /// Zustand einer Unit ohne Logzeilen.
    ///
    /// Read-only, weil `systemctl status -n 0` den Zustand nur liest; die Unit wird lokal gegen `list-units --all`
    /// aufgelöst und steht hinter `--`, ein Name kann also keine Option sein.
    UnitStatus { unit: UnitRef },
    /// Ergebnis, Exit-Status, Neustartzähler und Zeiten einer Unit.
    ///
    /// Read-only, weil `systemctl show -p <feste Liste>` nur Eigenschaften abfragt; die Liste enthält weder
    /// `Environment` noch `ExecStart` (Geheimnisse in Kommandozeilen), ein Test prüft sie.
    UnitShow { unit: UnitRef },
    /// Journalzeilen einer Unit.
    ///
    /// Read-only, weil `journalctl -u <Unit> --since/--boot -n <Zeilen>` nur liest; Zeitfenster und Zeilen sind
    /// Aufzählungen, die Unit wird lokal aufgelöst, das Journal wird nie ganz gelesen.
    UnitJournal { unit: UnitRef, since: Since, lines: Lines },
    /// Alle systemd-Timer mit letztem und nächstem Lauf.
    ///
    /// Read-only, weil `systemctl list-timers --all` nur listet.
    Timers {},
    /// Liste der aufgezeichneten Coredumps.
    ///
    /// Read-only, weil `coredumpctl list` nur die Liste ausgibt; `info`, `dump` und `debug` (Speicherinhalt, Debugger)
    /// stehen in keiner Vorlage.
    Coredumps { since: Since },
    // ---- Docker
    /// Alle Container mit Zustand (ohne Labels und Kommandos).
    ///
    /// Read-only, weil `docker ps -a` nur listet; der Docker-Socket ist zwar root-gleich, aber die Vorlage kennt nur
    /// diese Abfrage, und Rust entfernt `Labels` und `Command` vor dem Bericht.
    DockerPs {},
    /// Zustand eines Containers: Status, Exit-Code, OOM, Neustarts, Health, Image, Mounts.
    ///
    /// Read-only, weil `docker inspect --type=container --format <feste Vorlage>` nur liest; die Vorlage nennt weder `Env`
    /// noch `Labels` noch `Cmd`, und der Container wird lokal aufgelöst (die volle 64-stellige ID aus
    /// `docker ps -a --no-trunc` wird eingesetzt, nie der Text des Modells).
    DockerState { container: ContainerRef },
    /// Logzeilen eines Containers (immer geschwärzt).
    ///
    /// Read-only, weil `docker logs --tail --since --timestamps` nur liest (kein `-f`); das höchste Geheimnisrisiko aller
    /// Proben, darum wird die Ausgabe immer geschwärzt.
    DockerLogs { container: ContainerRef, since: Since, lines: Lines },
    /// Platzverbrauch von Docker.
    ///
    /// Read-only, weil `docker system df` nur zusammenzählt; `prune` und `system df -v` stehen in keiner Vorlage.
    DockerDf {},
    /// Aktuelle CPU-, Speicher- und I/O-Last der laufenden Container.
    ///
    /// Read-only, weil `docker stats --no-stream` einmal liest und endet (ohne `--no-stream` liefe es ewig).
    DockerStats {},
    // ---- Network
    /// Netzwerkschnittstellen, Adressen und Routen.
    ///
    /// Read-only, weil `ip -j addr` und `ip -j route` nur anzeigen; `ip link set`, `addr add` und `route add` stehen in
    /// keiner Vorlage, Adressen werden pseudonymisiert.
    NetLinks {},
    /// DNS-Zustand je Schnittstelle.
    ///
    /// Read-only, weil `resolvectl status` nur anzeigt; `flush-caches` und `query` stehen in keiner Vorlage.
    NetDns {},
    /// Lauschende TCP- und UDP-Sockets mit Prozessnamen.
    ///
    /// Read-only, weil `ss -Htulpn` nur Sockets listet (ohne root nur eigene Prozessnamen); `ss -K` (Sockets beenden)
    /// kommt nicht vor.
    NetListen {},
    /// Zustand von Tailscale: Name, Online, Relay, Fehler.
    ///
    /// Read-only, weil `tailscale status --json` nur den lokalen Zustand vom Daemon abfragt; `up`, `down`, `set` und
    /// `logout` stehen in keiner Vorlage.
    NetTailscale {},
    /// Netzwerk-Mounts (CIFS, NFS, sshfs) ohne Zugangsdaten.
    ///
    /// Read-only, weil `findmnt -J` nur die Mount-Tabelle des Kernels liest und keine Dateisysteme anfasst (kein
    /// `statfs`, hängt also nicht an einem toten NAS); Optionen wie `credentials=` entfernt Rust.
    NetMounts {},
    /// Ping an ein Ziel aus der Config-Liste, dreimal.
    ///
    /// Read-only, weil `ping -c 3 -W 1` lokal nichts ändert; es ist die einzige Probe, die Netzverkehr erzeugt, und läuft
    /// darum nur nach Bestätigung des Users. Ziele stammen aus der Config, nie vom Modell.
    NetReach { target: PingTarget },
    // ---- Resources
    /// Speicher und Speicherdruck.
    ///
    /// Read-only, weil Rust nur `/proc/meminfo` und `/proc/pressure/*` liest.
    Mem {},
    /// Last und CPU-Auslastung.
    ///
    /// Read-only, weil Rust nur `/proc/loadavg` und zweimal `/proc/stat` im Abstand von 500 ms liest.
    Load {},
    /// Belegung lokaler Dateisysteme und Blockgeräte.
    ///
    /// Read-only, weil `df -h -l` und `lsblk -J` nur anzeigen; `-l` lässt Netz-Mounts aus, damit `df` nicht an einem
    /// toten NAS hängt.
    Disks {},
    /// Die zehn größten Prozesse nach Speicher und CPU (nur Programmname).
    ///
    /// Read-only, weil Rust nur `/proc/<pid>/stat` und `status` liest, nie `cmdline` (Geheimnisse in Argumenten).
    TopProcs {},
    /// Out-of-Memory-Kills in den Kernelmeldungen eines Boots.
    ///
    /// Read-only, weil `journalctl -k --grep` nur liest, mit festem Ausdruck (nie einem des Modells) und höchstens 50
    /// Zeilen.
    Oom { boot: BootOffset },
    // ---- Heat
    /// Hardware-Temperaturen, Lüfter und Spannungen.
    ///
    /// Read-only, weil `sensors -j` nur hwmon liest; `-s` (Grenzwerte setzen) und `sensors-detect` kommen nicht vor.
    Sensors {},
    /// NVIDIA-GPU: Temperatur, Leistung, Takt, Auslastung, Speicher, Drosselgründe.
    ///
    /// Read-only, weil `nvidia-smi --query-gpu=<feste Liste>` nur abfragt; `-pl`, `-pm`, `-lgc` und `-r` (Setzen und
    /// Zurücksetzen) stehen in keiner Vorlage.
    Gpu {},
    /// Temperaturen der NVMe-Laufwerke.
    ///
    /// Read-only, weil Rust nur die hwmon-Dateien `/sys/class/nvme/nvme*/device/hwmon*/temp*_input` liest; ohne
    /// `smartctl` und `nvme-cli` (die brauchen root).
    NvmeTemp {},
}

impl Probe {
    /// Name der Probe (Wert von `probe` auf dem Draht, Schlüssel im Katalog).
    pub fn name(&self) -> &'static str {
        match self {
            Probe::Boots {} => "boots",
            Probe::BootEnd { .. } => "boot_end",
            Probe::KernelErrors { .. } => "kernel_errors",
            Probe::Pstore { .. } => "pstore",
            Probe::TempsTail { .. } => "temps_tail",
            Probe::KernelInfo {} => "kernel_info",
            Probe::PacmanLog { .. } => "pacman_log",
            Probe::DriverVersions {} => "driver_versions",
            Probe::FailedUnits {} => "failed_units",
            Probe::UnitStatus { .. } => "unit_status",
            Probe::UnitShow { .. } => "unit_show",
            Probe::UnitJournal { .. } => "unit_journal",
            Probe::Timers {} => "timers",
            Probe::Coredumps { .. } => "coredumps",
            Probe::DockerPs {} => "docker_ps",
            Probe::DockerState { .. } => "docker_state",
            Probe::DockerLogs { .. } => "docker_logs",
            Probe::DockerDf {} => "docker_df",
            Probe::DockerStats {} => "docker_stats",
            Probe::NetLinks {} => "net_links",
            Probe::NetDns {} => "net_dns",
            Probe::NetListen {} => "net_listen",
            Probe::NetTailscale {} => "net_tailscale",
            Probe::NetMounts {} => "net_mounts",
            Probe::NetReach { .. } => "net_reach",
            Probe::Mem {} => "mem",
            Probe::Load {} => "load",
            Probe::Disks {} => "disks",
            Probe::TopProcs {} => "top_procs",
            Probe::Oom { .. } => "oom",
            Probe::Sensors {} => "sensors",
            Probe::Gpu {} => "gpu",
            Probe::NvmeTemp {} => "nvme_temp",
        }
    }

    /// Eintrag im Katalog. Jede Probe hat einen (Test `every_probe_has_a_catalog_entry`).
    pub fn spec(&self) -> &'static ProbeSpec {
        catalog::spec(self.name()).expect("every probe has a catalog entry (checked by a test)")
    }

    /// Boot-Versatz (`boot`), falls die Probe einen hat.
    pub fn boot(&self) -> Option<BootOffset> {
        match self {
            Probe::BootEnd { boot, .. }
            | Probe::KernelErrors { boot, .. }
            | Probe::Pstore { boot }
            | Probe::TempsTail { boot, .. }
            | Probe::PacmanLog { boot, .. }
            | Probe::Oom { boot } => Some(*boot),
            _ => None,
        }
    }

    /// Zeilenzahl (`lines`), falls die Probe sie hat.
    pub fn lines(&self) -> Option<Lines> {
        match self {
            Probe::BootEnd { lines, .. } | Probe::KernelErrors { lines, .. } | Probe::UnitJournal { lines, .. } | Probe::DockerLogs { lines, .. } => {
                Some(*lines)
            }
            _ => None,
        }
    }

    /// Zeitfenster (`since`), falls die Probe eines hat.
    pub fn since(&self) -> Option<Since> {
        match self {
            Probe::UnitJournal { since, .. } | Probe::Coredumps { since } | Probe::DockerLogs { since, .. } => Some(*since),
            _ => None,
        }
    }

    /// Fenster in Minuten (`window_min`), falls die Probe eines hat.
    pub fn window_min(&self) -> Option<WindowMin> {
        match self {
            Probe::TempsTail { window_min, .. } => Some(*window_min),
            _ => None,
        }
    }

    /// Fenster in Tagen (`days`), falls die Probe eines hat.
    pub fn days(&self) -> Option<Days> {
        match self {
            Probe::PacmanLog { days, .. } => Some(*days),
            _ => None,
        }
    }

    /// Suchbegriff der Unit (`unit`), falls die Probe einen hat.
    pub fn unit(&self) -> Option<&UnitRef> {
        match self {
            Probe::UnitStatus { unit } | Probe::UnitShow { unit } | Probe::UnitJournal { unit, .. } => Some(unit),
            _ => None,
        }
    }

    /// Suchbegriff des Containers (`container`), falls die Probe einen hat.
    pub fn container(&self) -> Option<&ContainerRef> {
        match self {
            Probe::DockerState { container } | Probe::DockerLogs { container, .. } => Some(container),
            _ => None,
        }
    }

    /// Name des Ping-Ziels (`target`), falls die Probe eines hat.
    pub fn target(&self) -> Option<&PingTarget> {
        match self {
            Probe::NetReach { target } => Some(target),
            _ => None,
        }
    }

    /// Prüft eine selbst gebaute Probe (z. B. aus einem Profil) gegen dieselben Regeln wie [`parse_validated`], nur
    /// ohne Trimmen. Die Typen erzwingen die Bereiche schon; das hier fängt ab, wenn Typ und Katalog auseinanderlaufen.
    pub fn validate(&self) -> Result<(), ProbeError> {
        let value = serde_json::to_value(self).map_err(|e| ProbeError::Malformed(e.to_string()))?;
        match value {
            Value::Object(map) => self.spec().check(&map),
            _ => Err(ProbeError::NotObject),
        }
    }
}

/// Liest eine Probe aus JSON (Vorschlag des Modells oder Eingabe des Panels) und prüft sie vollständig: bekannte
/// Probe, keine unbekannten Felder, alle Pflichtfelder, Typen, Bereiche, Längen, Zeichen. Suchbegriffe werden vorher
/// getrimmt; der Rest wird nie stillschweigend korrigiert.
pub fn parse_validated(json: &str) -> Result<Probe, ProbeError> {
    if json.len() > MAX_JSON_BYTES {
        return Err(ProbeError::TooLong { bytes: json.len(), max: MAX_JSON_BYTES });
    }
    let value: Value = serde_json::from_str(json).map_err(|e| ProbeError::Json(e.to_string()))?;
    parse_value(value)
}

/// Wie [`parse_validated`] für ein schon gelesenes JSON-Objekt (etwa ein Eintrag aus `checks` einer Modellantwort).
pub fn parse_value(value: Value) -> Result<Probe, ProbeError> {
    let Value::Object(mut map) = value else {
        return Err(ProbeError::NotObject);
    };
    let name = match map.get("probe") {
        Some(Value::String(name)) => name.clone(),
        _ => return Err(ProbeError::MissingProbe),
    };
    let spec = catalog::spec(&name).ok_or(ProbeError::UnknownProbe(name))?;
    spec.normalize(&mut map);
    spec.check(&map)?;
    serde_json::from_value(Value::Object(map)).map_err(|e| ProbeError::Malformed(e.to_string()))
}

/// Warum eine Probe abgelehnt wurde; die Meldung nennt Ursache und Feld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// Mehr als [`MAX_JSON_BYTES`].
    TooLong { bytes: usize, max: usize },
    /// Kein gültiges JSON.
    Json(String),
    /// Das JSON ist kein Objekt.
    NotObject,
    /// `probe` fehlt oder ist kein String.
    MissingProbe,
    /// Der Name steht nicht im Katalog.
    UnknownProbe(String),
    /// Ein Feld, das die Probe nicht kennt.
    UnknownField { probe: &'static str, field: String },
    /// Ein Pflichtfeld fehlt.
    MissingField { probe: &'static str, field: &'static str },
    /// Falscher Typ, Bereich, Länge oder Inhalt eines Feldes.
    InvalidField { probe: &'static str, field: &'static str, reason: String },
    /// Die Prüfung ließ etwas durch, was die Typen nicht lesen können; ein Fehler im Katalog, nicht in der Eingabe
    /// (die Beispieltests verhindern das).
    Malformed(String),
}

/// Höchstens so viele Zeichen eines fremden Namens in Meldungen, damit eine Modellantwort kein Log füllt.
const SHOWN_CHARS: usize = 40;

fn shown(s: &str) -> String {
    let cut: String = s.chars().take(SHOWN_CHARS).collect();
    if s.chars().count() > SHOWN_CHARS {
        format!("{cut:?}...")
    } else {
        format!("{cut:?}")
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::TooLong { bytes, max } => write!(f, "probe JSON is too long ({bytes} bytes, at most {max})"),
            ProbeError::Json(e) => write!(f, "probe is not valid JSON: {e}"),
            ProbeError::NotObject => write!(f, "probe must be a JSON object"),
            ProbeError::MissingProbe => write!(f, "missing \"probe\" (a string naming the probe)"),
            ProbeError::UnknownProbe(name) => write!(f, "unknown probe {}", shown(name)),
            ProbeError::UnknownField { probe, field } => write!(f, "unknown field {} for probe {probe}", shown(field)),
            ProbeError::MissingField { probe, field } => write!(f, "missing field \"{field}\" for probe {probe}"),
            ProbeError::InvalidField { probe, field, reason } => write!(f, "field \"{field}\" of probe {probe} {reason}"),
            ProbeError::Malformed(e) => write!(f, "probe does not match its type: {e}"),
        }
    }
}

impl std::error::Error for ProbeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::test_support::{examples, full_probes, rejected};
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn examples_parse_to_the_expected_probe_and_round_trip() {
        let mut seen = BTreeSet::new();
        for (json, expected) in examples() {
            let probe = parse_validated(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(probe, expected, "{json}");
            assert!(probe.validate().is_ok(), "{json}");
            // Auf dem Draht kommt dasselbe JSON heraus, das hineinging
            assert_eq!(serde_json::to_value(&probe).unwrap(), serde_json::from_str::<Value>(json).unwrap(), "{json}");
            // Direkt über serde (ohne Katalogprüfung) liest derselbe Typ dasselbe
            assert_eq!(serde_json::from_str::<Probe>(json).unwrap(), expected, "{json}");
            seen.insert(probe.name());
        }
        let all: BTreeSet<&str> = catalog::all().iter().map(|s| s.name).collect();
        assert_eq!(seen, all, "every probe needs an example");
    }

    #[test]
    fn every_probe_has_a_catalog_entry_and_name_is_the_wire_name() {
        let probes = full_probes();
        let names: BTreeSet<&str> = probes.iter().map(Probe::name).collect();
        assert_eq!(names.len(), probes.len(), "duplicate names");
        let catalog_names: BTreeSet<&str> = catalog::all().iter().map(|s| s.name).collect();
        assert_eq!(names, catalog_names);
        assert_eq!(catalog::all().len(), probes.len());
        for probe in &probes {
            assert_eq!(serde_json::to_value(probe).unwrap()["probe"], json!(probe.name()));
            assert_eq!(probe.spec().name, probe.name());
            assert!(probe.validate().is_ok(), "{}", probe.name());
        }
    }

    #[test]
    fn accessors_match_the_parameters_of_the_catalog() {
        for probe in full_probes() {
            let mut present = BTreeSet::new();
            let mut note = |name: &'static str, has: bool| {
                if has {
                    present.insert(name);
                }
            };
            note("boot", probe.boot().is_some());
            note("lines", probe.lines().is_some());
            note("since", probe.since().is_some());
            note("window_min", probe.window_min().is_some());
            note("days", probe.days().is_some());
            note("unit", probe.unit().is_some());
            note("container", probe.container().is_some());
            note("target", probe.target().is_some());
            let declared: BTreeSet<&str> = probe.spec().params.iter().map(|p| p.name).collect();
            assert_eq!(present, declared, "accessors of {}", probe.name());
        }
    }

    #[test]
    fn accessor_values() {
        let p = parse_validated(r#"{"probe":"unit_journal","unit":"x.service","since":"6h","lines":500}"#).unwrap();
        assert_eq!(p.unit().unwrap().as_str(), "x.service");
        assert_eq!(p.since(), Some(Since::Hours6));
        assert_eq!(p.lines(), Some(Lines::N500));
        assert_eq!(p.lines().unwrap().count(), 500);
        assert!(p.boot().is_none());
        let x = parse_validated(r#"{"probe":"pstore","boot":-1}"#).unwrap();
        assert_eq!(x.boot(), Some(BootOffset::PREVIOUS));
        assert_eq!(BootOffset::PREVIOUS.offset(), -1);
        assert_eq!(BootOffset::CURRENT.offset(), 0);
    }

    #[test]
    fn typed_enums_agree_with_their_wire_values() {
        // name_enum: serde und NAMES stammen aus demselben Literal
        for (i, since) in Since::ALL.iter().enumerate() {
            assert_eq!(Since::NAMES[i], since.name());
            assert_eq!(serde_json::to_value(since).unwrap(), json!(since.name()));
            assert_eq!(serde_json::from_value::<Since>(json!(since.name())).unwrap(), *since);
        }
        assert_eq!(Since::NAMES, ["15m", "1h", "6h", "24h", "boot"]);
        assert_eq!(Since::Boot.duration(), None);
        assert_eq!(Since::Hours24.duration(), Some("24h"));
        // int_enum: dasselbe für Zahlen
        fn check_ints<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug + Copy>(all: &[T], values: &[i64], value: impl Fn(T) -> i64) {
            assert_eq!(all.len(), values.len());
            for (v, n) in all.iter().zip(values) {
                assert_eq!(value(*v), *n);
                assert_eq!(serde_json::to_value(v).unwrap(), json!(n));
                assert_eq!(serde_json::from_value::<T>(json!(n)).unwrap(), *v);
            }
        }
        check_ints(Lines::ALL, Lines::VALUES, Lines::value);
        check_ints(WindowMin::ALL, WindowMin::VALUES, WindowMin::value);
        check_ints(Days::ALL, Days::VALUES, Days::value);
        assert_eq!(Lines::VALUES, [50, 200, 500]);
        assert_eq!(WindowMin::VALUES, [5, 15, 60]);
        assert_eq!(Days::VALUES, [1, 3, 7]);
        assert_eq!(Lines::from_value(100), None);
    }

    #[test]
    fn boot_offset_range() {
        for n in -9..=0 {
            let b = BootOffset::new(n).unwrap();
            assert_eq!(i64::from(b.offset()), n);
            assert_eq!(serde_json::from_value::<BootOffset>(json!(n)).unwrap(), b);
            assert_eq!(serde_json::to_value(b).unwrap(), json!(n));
        }
        for n in [-10, 1, i64::MIN, i64::MAX, 255, -256] {
            assert!(BootOffset::new(n).is_none(), "{n}");
            assert!(serde_json::from_value::<BootOffset>(json!(n)).is_err(), "{n}");
        }
        assert!(serde_json::from_value::<BootOffset>(json!(-1.5)).is_err());
        assert!(serde_json::from_value::<BootOffset>(json!("-1")).is_err());
        assert!(serde_json::from_value::<BootOffset>(json!(null)).is_err());
    }

    #[test]
    fn terms_accept_names_and_reject_everything_else() {
        for ok in ["docker", "docker.service", "getty@tty1.service", "user@1000.service", "dev-sda1.device", "a", "dev-disk-by\\x2duuid-abc.device"] {
            assert!(UnitRef::new(ok).is_ok(), "{ok}");
        }
        for bad in
            ["", " ", "-u", "--user", "-.mount", ".hidden", "a b", "a/b", "../etc", "a;b", "a$(b)", "a|b", "a\nb", "ä", "a\u{202e}b", " docker", "docker "]
        {
            assert!(UnitRef::new(bad).is_err(), "{bad:?}");
        }
        assert!(UnitRef::new(&"a".repeat(128)).is_ok());
        assert!(UnitRef::new(&"a".repeat(129)).is_err());
        for ok in ["web", "dataroom-web-1", "my_app.v2", "445f5bd9a1f3"] {
            assert!(ContainerRef::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-f", "--all", "a b", "a/b", "web:latest", "a@b", "a\tb", "_x", ".x"] {
            assert!(ContainerRef::new(bad).is_err(), "{bad:?}");
        }
        assert!(ContainerRef::new(&"a".repeat(128)).is_ok());
        assert!(ContainerRef::new(&"a".repeat(129)).is_err());
        for ok in ["gateway", "nas", "1.1.1.1", "dns-1", "fe80::1", "host.example.org"] {
            assert!(PingTarget::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-c", "--flood", "a b", "a/b", "a@b", "a;b", "::1", "$(id)", "a\n"] {
            assert!(PingTarget::new(bad).is_err(), "{bad:?}");
        }
        assert!(PingTarget::new(&"a".repeat(64)).is_ok());
        assert!(PingTarget::new(&"a".repeat(65)).is_err());
        // Auch der Weg über serde prüft (nicht nur `new`)
        assert!(serde_json::from_value::<UnitRef>(json!("-u")).is_err());
        assert!(serde_json::from_value::<UnitRef>(json!(5)).is_err());
        assert_eq!(serde_json::to_value(UnitRef::new("x.service").unwrap()).unwrap(), json!("x.service"));
    }

    #[test]
    fn terms_are_trimmed_by_parse_validated_but_not_by_serde() {
        let p = parse_validated(r#"{"probe":"unit_status","unit":"  docker.service\n"}"#).unwrap();
        assert_eq!(p.unit().unwrap().as_str(), "docker.service");
        assert!(serde_json::from_str::<Probe>(r#"{"probe":"unit_status","unit":" docker.service"}"#).is_err());
        let p = parse_validated(r#"{"probe":"net_reach","target":" nas "}"#).unwrap();
        assert_eq!(p.target().unwrap().as_str(), "nas");
        // Aufzählungen werden nie getrimmt oder in der Groß-/Kleinschreibung angepasst
        assert!(parse_validated(r#"{"probe":"coredumps","since":" 1h"}"#).is_err());
        assert!(parse_validated(r#"{"probe":"coredumps","since":"1H"}"#).is_err());
    }

    #[test]
    fn invalid_cases_are_rejected_with_a_message_naming_the_cause() {
        let cases = rejected();
        assert!(cases.len() >= 100, "{} cases", cases.len());
        // Jede Probe kommt in mindestens einem Fehlerfall vor
        for spec in catalog::all() {
            let tag = format!(r#""probe":"{}""#, spec.name);
            assert!(cases.iter().any(|(json, _, _)| json.contains(&tag)), "no invalid case for {}", spec.name);
        }
        for (json, msg, _schema_rejects) in cases {
            let err = match parse_validated(&json) {
                Ok(p) => panic!("{json} should be rejected, got {p:?}"),
                Err(e) => e.to_string(),
            };
            assert!(err.contains(msg), "{json}: message {err:?} does not contain {msg:?}");
        }
    }

    #[test]
    fn structural_errors() {
        assert!(matches!(parse_validated("not json"), Err(ProbeError::Json(_))));
        assert!(matches!(parse_validated("[]"), Err(ProbeError::NotObject)));
        assert!(matches!(parse_validated("\"boots\""), Err(ProbeError::NotObject)));
        assert!(matches!(parse_validated("{}"), Err(ProbeError::MissingProbe)));
        let big = format!(r#"{{"probe":"boots","pad":"{}"}}"#, "x".repeat(MAX_JSON_BYTES));
        assert!(matches!(parse_validated(&big), Err(ProbeError::TooLong { .. })));
        assert!(matches!(parse_value(json!(5)), Err(ProbeError::NotObject)));
        assert_eq!(parse_value(json!({"probe": "boots"})).unwrap(), Probe::Boots {});
        // Namen aus fremdem Mund erscheinen nur gekürzt in der Meldung
        let msg = parse_validated(&format!(r#"{{"probe":"{}"}}"#, "y".repeat(500))).unwrap_err().to_string();
        assert!(msg.len() < 120, "{msg}");
        let msg = parse_validated(&format!(r#"{{"probe":"boots","{}":1}}"#, "z".repeat(500))).unwrap_err().to_string();
        assert!(msg.len() < 120, "{msg}");
    }

    #[test]
    fn serde_alone_also_rejects_unknown_fields_and_bad_values() {
        // Auch ohne die Katalogprüfung (z. B. wenn jemand `serde_json::from_str::<Probe>` benutzt) bleibt der Typ streng
        for json in [
            r#"{"probe":"boots","x":1}"#,
            r#"{"probe":"boot_end","boot":-1,"lines":51}"#,
            r#"{"probe":"boot_end","boot":-10,"lines":50}"#,
            r#"{"probe":"coredumps","since":"2h"}"#,
            r#"{"probe":"unit_status","unit":"-x"}"#,
            r#"{"probe":"unknown"}"#,
            r#"{"boot":-1}"#,
        ] {
            assert!(serde_json::from_str::<Probe>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn every_probe_documents_why_it_is_read_only() {
        // Jede Variante von `Probe` trägt in ihrem Doku-Kommentar „Read-only, weil …“ (Plan: Begründung je Probe im Code)
        let source = include_str!("probe.rs");
        let start = source.find("pub enum Probe {").expect("enum Probe in the source");
        let body = &source[start..];
        let end = body.find("\n}\n").expect("end of enum Probe");
        let mut doc = String::new();
        let mut documented = Vec::new();
        for line in body[..end].lines().skip(1) {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("///") {
                doc.push_str(rest);
                doc.push(' ');
            } else if t.starts_with("#[") || t.starts_with("//") || t.is_empty() {
                continue;
            } else {
                // eine Variante: `Name {...},`
                let variant: String = t.chars().take_while(|c| c.is_alphanumeric()).collect();
                let lower = doc.to_lowercase();
                assert!(lower.contains("read-only, weil"), "variant {variant} has no \"Read-only, weil …\" in its doc comment: {doc:?}");
                let after = lower.split("read-only, weil").nth(1).unwrap();
                assert!(after.trim().len() > 30, "variant {variant}: the reason is too short: {after:?}");
                documented.push(variant);
                doc.clear();
            }
        }
        assert_eq!(documented.len(), catalog::all().len(), "{documented:?}");
    }
}
