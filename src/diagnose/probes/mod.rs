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

//! Die Proben v1 (Aufgabe 3 in `PLAN-diagnose.md`): je Probe des Katalogs die argv aus den Parametern, das Ausführen
//! (über [`crate::diagnose::run`]) oder Lesen in Rust (feste Pfade, Pfadbasis austauschbar), der Parser, das Kürzen auf
//! die Grenze und die probenspezifische Schwärzung.
//!
//! **Einstieg:** [`Collector`] (Kontext, Abbruch, Zwischenspeicher der Auflösung) führt eine [`crate::diagnose::Probe`]
//! mit [`Collector::collect`] aus und liefert immer eine [`ProbeOutput`]: Teile mit Text, Status, Hinweise, was lokal
//! aufgelöst wurde, die Schwärzungen der Probe selbst und die Zahlen für den Kopf des Abschnitts. Das ist die Schnittstelle
//! für den allgemeinen Geheimnis-Filter und das Pseudonymisieren (Aufgabe 4: arbeitet auf [`Part::text`] und beachtet
//! [`Part::filter`]) und für den Bericht (Aufgabe 5).
//!
//! **Auflösung** ([`resolve`]): Ein Unit-Name gilt nur, wenn er in `systemctl list-units --all` (System und User) steht;
//! ein Container nur, wenn `docker ps -a --no-trunc` ihn kennt, eingesetzt wird die volle 64-stellige ID, nie der
//! Suchbegriff; ein Boot nur, wenn `journalctl --list-boots` ihn hat; ein Ping-Ziel nur aus der Config-Liste
//! ([`PingTargetCfg`]); `since: boot` braucht das Alter des Boots aus `/proc/uptime`. Mehrdeutige oder erfundene Namen
//! enden als [`Status::Unavailable`] mit einer Meldung, die dem Modell die Kandidaten nennt.
//!
//! **Dateien** ([`fs`]): jede Lese-Probe öffnet nur, was ihre `Exec::Read`-Liste im Katalog nennt, mit `O_NOFOLLOW`, nur
//! reguläre Dateien und mit Grenze. Die Orte (`/proc`, `/sys`, pstore, `pacman.log`, Ordner des
//! temp-loggers) liegen unter einer austauschbaren [`ProbePaths`]; Tests zeigen auf Fixture-Ordner.
//!
//! **Grenzen und Kürzen.** Die Grenze im Katalog gilt für den Text der Probe (Summe der Teile, einschließlich der
//! Hinweise); eine letzte Prüfung in [`Run::finish`] stellt sie für jede Probe sicher. Was bei einem Überlauf bleibt,
//! wählt die Probe nach dem Wert der Daten:
//!
//! - `Keep::Tail` (das Ende bleibt, vorn wird in ganzen Zeilen gekürzt): `boot_end`, `kernel_errors`, `oom`,
//!   `unit_journal`, `docker_logs` (Journale: das Ende ist wichtiger) und die Listen, die von alt nach neu laufen
//!   (`coredumps`, die Boots der Auflösung). Wiederholte Zeilen werden vorher zusammengefasst.
//! - `Keep::Head` (der Anfang bleibt): alle übrigen Befehle (Listen, Tabellen, kurze Texte).
//! - Dateien, die Rust liest, werden von hinten gelesen (`pacman.log`, temp-logger, pstore: das Ende zählt) und
//!   im Bericht auf Zeilen gekürzt.
//!
//! **Summe mehrerer Befehle einer Probe** (`failed_units`, `net_links`, `disks`): jeder Befehl bekommt den Rest der Grenze
//! zu gleichen Teilen (`Run::share`: bei zwei Befehlen zuerst die Hälfte, der zweite erbt, was der erste nicht brauchte).
//! Programme, deren JSON Rust verdichtet, dürfen bis [`collect::RAW_LIMIT`] (256 KiB) liefern, sonst wäre ein bei der Grenze
//! abgeschnittenes JSON nicht mehr lesbar; die Grenze des Katalogs gilt für das Ergebnis. Flache Listen (Boots, Units,
//! Coredumps, Container) liest [`json::objects`] auch dann, wenn die Ausgabe angeschnitten ist. Die Frist der Probe gilt für
//! alles zusammen, einschließlich der Auflösung; ein Befehl kann sie um höchstens `run::MAX_OVERRUN` überschreiten.
//!
//! **Schwärzung der Probe selbst** (vor dem allgemeinen Filter, gezählt in [`ProbeOutput::redactions`]): `docker_ps`
//! entfernt `Labels` und `Command`; `net_mounts` entfernt Optionen mit Zugangsdaten; `unit_status` kürzt Prozesse im
//! `CGroup:`-Baum und `Process:`-Zeilen auf den Programmnamen; `kernel_info` ersetzt Werte von Kernel-Parametern, die
//! nach Schlüssel oder Kennwort heißen, und markiert die Kommandozeile mit [`Filter::KernelCmdline`] für den strengen
//! Filter (die Zeile „Kernel command line: …“ in Journal und pstore behandelt der allgemeine Filter streng);
//! `top_procs` liest nie `cmdline`; nie `Environment` (steht in keiner Vorlage).
//!
//! Inhalte der Proben werden nie geloggt, auch nicht mit `DICTATE_DEBUG`.

pub mod boot;
pub mod collect;
pub mod common;
pub mod docker;
pub mod fs;
pub mod heat;
pub mod json;
pub mod network;
pub mod output;
pub mod resolve;
pub mod resources;
pub mod text;
pub mod time;
pub mod units;

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;

pub use boot::ended_cleanly;
pub use collect::{CollectOptions, Collector, PingTargetCfg, ProbeContext, ProbeResult, Run, SAMPLE_INTERVAL};
pub use fs::{ProbePaths, DEFAULT_TEMP_LOG_DIR};
pub use output::{Filter, Listed, ListedBoots, ListedContainer, ListedUnit, Part, PreviousBoot, ProbeOutput, Redaction, Status, KERNEL_CMDLINE_MARK};
pub use time::Clock;

use crate::diagnose::probe::Probe;

/// Ruft die Funktion der Probe auf.
pub(crate) fn dispatch(run: &mut Run) -> ProbeResult {
    match run.probe {
        Probe::Boots {} => boot::boots(run),
        Probe::BootEnd { .. } => boot::boot_end(run),
        Probe::KernelErrors { .. } => boot::kernel_errors(run),
        Probe::Pstore { .. } => boot::pstore(run),
        Probe::TempsTail { .. } => boot::temps_tail(run),
        Probe::KernelInfo {} => boot::kernel_info(run),
        Probe::PacmanLog { .. } => boot::pacman_log(run),
        Probe::DriverVersions {} => boot::driver_versions(run),
        Probe::FailedUnits {} => units::failed_units(run),
        Probe::UnitStatus { .. } => units::unit_status(run),
        Probe::UnitShow { .. } => units::unit_show(run),
        Probe::UnitJournal { .. } => units::unit_journal(run),
        Probe::Timers {} => units::timers(run),
        Probe::Coredumps { .. } => units::coredumps(run),
        Probe::DockerPs {} => docker::docker_ps(run),
        Probe::DockerState { .. } => docker::docker_state(run),
        Probe::DockerLogs { .. } => docker::docker_logs(run),
        Probe::DockerDf {} => docker::docker_df(run),
        Probe::DockerStats {} => docker::docker_stats(run),
        Probe::NetLinks {} => network::net_links(run),
        Probe::NetDns {} => network::net_dns(run),
        Probe::NetListen {} => network::net_listen(run),
        Probe::NetTailscale {} => network::net_tailscale(run),
        Probe::NetMounts {} => network::net_mounts(run),
        Probe::NetReach { .. } => network::net_reach(run),
        Probe::Mem {} => resources::mem(run),
        Probe::Load {} => resources::load(run),
        Probe::Disks {} => resources::disks(run),
        Probe::TopProcs {} => resources::top_procs(run),
        Probe::Oom { .. } => resources::oom(run),
        Probe::Sensors {} => heat::sensors(run),
        Probe::Gpu {} => heat::gpu(run),
        Probe::NvmeTemp {} => heat::nvme_temp(run),
    }
}
