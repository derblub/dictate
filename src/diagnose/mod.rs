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

//! Linux-Diagnose (Baustein 4, `PLAN-diagnose.md`): ausgewählte, nur lesende Infos über das System sammeln und die KI
//! Ursachen und weitere lesende Prüfungen vorschlagen lassen.
//!
//! Dieses Modul enthält bisher die Grundlage (Aufgabe 1): den Katalog der Proben ([`catalog`]: Gruppe, Parameter, Grenze,
//! Frist, Schema für Prompt und Tool-Use aus *einer* Quelle), die Proben selbst mit ihrer Validierung ([`probe`]) und die
//! festen argv-Vorlagen als Daten ([`argv`]), dazu den Ausführer ([`run`]: argv, Frist, Grenzen, Umgebung, Drossel) und die
//! Proben v1 ([`probes`]: Auflösung der Parameter gegen die echten Listen, Ausführen oder Lesen in Rust, Parser, Kürzen,
//! Schwärzung der Probe selbst, Ausgabe als Text plus Metadaten) und den Filter des Berichts ([`filter`]: probenspezifisch,
//! dann der Geheimnisfilter `secrets::redact`, dann Pseudonymisieren mit [`pseudo`]), die Profile ([`profile`]) und den Bericht ([`report`]: Sammeln mit Budget und Sperre,
//! Abschnitte, Größenbudget, Abwahl, Vorschau), dazu der Dienst im Daemon ([`service`]: Sitzung, Ablauf, Befehle `diag_*`),
//! die Identität für das Pseudonymisieren ([`identity`]) und der KI-Pfad ([`ai`]: Prompt aus
//! Katalog und Profil, strukturierte Antwort je Anbieter, Prüfung von Befunden, Belegen und Prüfbefehlen) samt der
//! Rückübersetzung der Platzhalter fürs Panel ([`restore`]: nur Fließtext, nie Code, Links, URLs oder Prüfbefehle).
//!
//! **Fester Katalog statt Befehle.** Weder das Modell noch der User tippt einen Befehl. Eine [`Probe`] ist ein Eintrag aus
//! dem Katalog mit typisierten Parametern (Aufzählungen und unaufgelöste Suchbegriffe), z. B.
//! `{"probe":"unit_journal","unit":"docker.service","since":"1h","lines":200}`. Das Modell kann nur Einträge daraus
//! vorschlagen; die Rust-Validierung ([`parse_validated`], [`Probe::validate`]) ist die einzige Schranke: unbekannte Probe,
//! unbekanntes Feld und Wert außerhalb des Bereichs sind Fehler. Parameter werden lokal gegen die echten Listen aufgelöst
//! und gelangen nie als freier Text in ein argv.
//!
//! **Nur lesend, jede Probe einzeln begründet** (Doku-Kommentar an [`probe::Probe`]). Ändernde Aktionen gibt es nicht.
//! Nur [`run`] startet Programme, und nur als [`Argv`] aus den Vorlagen (auch die Auflösung der Parameter: [`argv::resolution`]); die Tests prüfen die Vorlagen (kein verbotenes Wort, je Programm nur erlaubte Elemente,
//! nur feste Pfade).
//!
//! Diagnose-Inhalte (Berichte, Ausgaben der Proben, Antworten) werden nie geloggt, auch nicht mit `DICTATE_DEBUG`, und
//! kommen nie in `history.rs`.

pub mod ai;
pub mod argv;
pub mod boot_hint;
pub mod catalog;
pub mod filter;
pub mod identity;
pub mod probe;
pub mod probes;
pub mod profile;
pub mod pseudo;
pub mod report;
pub mod restore;
pub mod run;
pub mod service;

#[cfg(test)]
mod live_eval;
#[cfg(test)]
mod test_support;

pub use argv::{expand_probe, Arg, Argv, Bindings, Command, Exec, ExpandError, Program, ResolvedUnit};
pub use catalog::{catalog_text, combined_schema, flat_schema, Group, ParamKind, ProbeSpec, Term};
pub use filter::{filter_line, filter_output, filter_outputs, FilterCounts, Filtered};
pub use probe::{parse_validated, BootOffset, ContainerRef, Days, Lines, PingTarget, Probe, ProbeError, Since, UnitRef, WindowMin};
pub use probes::{
    CollectOptions, Collector, Filter, Listed, ListedContainer, ListedUnit, Part, PingTargetCfg, ProbeContext, ProbeOutput, ProbePaths, Redaction, Status,
};
pub use profile::{failed_unit_choices, problem_containers, Plan, PlanError, PlanItem, Profile, ProfileArgs, Step, MAX_PROBLEM_CONTAINERS};
pub use pseudo::{Category, Identity, Pseudonyms};
pub use report::{
    gather_plan, gather_probes, list_failed_units, CollectError, CollectGuard, CollectLock, CollectParams, Collected, CutPolicy, Gathered, Progress, Report,
    ReportBudget, SectionView, SelectError, Session, COLLECT_BUDGET, MAX_SECTIONS, MIN_PROBE_TIME,
};
pub use run::{run, stuck_processes, Captured, Exit, Keep, ProbeEnv, Reason, RunLimits, RunResult};
