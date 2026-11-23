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

//! Profile der Diagnose (Aufgabe 5, `PLAN-diagnose.md` „Katalog v1“): ein Profil ist ein fester Satz von Proben, den der
//! User im Panel wählt (die Palette kann es vorbelegen). Ein [`Plan`] ist die Probenliste eines Profils samt Auswahl: jeder
//! Eintrag hat einen Haken ([`PlanItem::selected`]), den der User *vor* dem Sammeln setzt oder löst; abgewählte Einträge
//! laufen gar nicht erst (sie kosten nichts), im Bericht stehen nur die gesammelten Abschnitte.
//!
//! | Profil | Proben |
//! |---|---|
//! | `last_crash` | `boots`, `boot_end -1`, `kernel_errors -1`, `pstore -1`, `temps_tail -1`, `pacman_log -1`, `driver_versions`, `kernel_info` |
//! | `now` | `failed_units`, `kernel_errors 0`, `mem`, `load`, `disks`, `sensors`, `gpu` |
//! | `unit` | `unit_status`, `unit_show`, `unit_journal` für eine gewählte (fehlgeschlagene) Unit |
//! | `docker` | `docker_ps`, `docker_df` (**standardmäßig abgewählt**), dazu `docker_state` und `docker_logs` je Container, der nicht läuft |
//! | `network` | `net_links`, `net_dns`, `net_listen`, `net_tailscale`, `net_mounts` (nie `net_reach`: das erzeugt Netzverkehr) |
//! | `heat` | `sensors`, `gpu`, `nvme_temp`, `temps_tail 0`, `load`, `top_procs` |
//!
//! Die Reihenfolge ist die des Berichts. Parameter, die der Plan offen ließ: Journale holen 200 Zeilen (`lines`), `temps_tail`
//! 15 Minuten um das Ende des Boots, `pacman_log` 3 Tage davor, `unit_journal` das Ende des laufenden Boots, `docker_logs`
//! 200 Zeilen aus den letzten 24 Stunden. Alles sind Werte der festen Aufzählungen des Katalogs.
//!
//! **`docker_df` (Review I4).** `docker system df` läuft in `dockerd` als root, ungedrosselt (`nice` und `ioprio` gelten nur
//! für den Client) und geht über alle Layer; auf einem Rechner mit vielen Images ist das genau die Last, die die Diagnose nicht
//! erzeugen soll. Der Eintrag steht im Profil, damit der User ihn kennt und anwählen kann, ist aber abgewählt und läuft nur
//! auf ausdrücklichen Wunsch.
//!
//! **Docker: Container, die nicht laufen.** `docker_state` und `docker_logs` brauchen einen Container; welcher, steht erst
//! nach `docker_ps` fest. Darum gibt es den Schritt [`Step::ProblemContainers`]: er wartet auf die Liste von `docker_ps`
//! ([`crate::diagnose::probes::Listed::containers`], aus derselben Ausgabe wie der Abschnitt im Bericht) und hängt je
//! Container, dessen `State` nicht `running` ist, `docker_state` und `docker_logs` an. Geordnet nach Schwere (neu startend,
//! tot, mit Fehler beendet, angehalten, erzeugt, mit Code 0 beendet), höchstens [`MAX_PROBLEM_CONTAINERS`]; was
//! darüber hinaus fehlt, vermerkt der Abschnitt von `docker_ps` in einem Hinweis. Die Namen aus der Liste sind fremder Text:
//! sie werden als [`ContainerRef`] geprüft und wie jeder Suchbegriff gegen die echten Listen aufgelöst (eingesetzt wird die
//! ID). Wer `docker_ps` abwählt, wählt damit auch diesen Schritt ab, und umgekehrt ([`Plan::set_selected`]).

use super::probe::{BootOffset, ContainerRef, Days, Lines, Probe, Since, UnitRef, WindowMin};
use super::probes::{ListedContainer, ProbeOutput};
use crate::config::{DiagnoseProfileInfo, DIAGNOSE_PROFILES};
use serde::{Deserialize, Serialize};

/// So viele Container, die nicht laufen, bekommen `docker_state` und `docker_logs` (je Container zwei Proben, jede mit bis
/// zu 64 KiB Grenze und eigener Frist; mehr würden das Budget und die Last sprengen).
pub const MAX_PROBLEM_CONTAINERS: usize = 4;

/// Ein Profil des Panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    LastCrash,
    Now,
    Unit,
    Docker,
    Network,
    Heat,
}

impl Profile {
    pub const ALL: [Profile; 6] = [Profile::LastCrash, Profile::Now, Profile::Unit, Profile::Docker, Profile::Network, Profile::Heat];

    /// Name auf dem Draht (`last_crash`, `now`, `unit`, `docker`, `network`, `heat`).
    pub fn name(self) -> &'static str {
        match self {
            Profile::LastCrash => "last_crash",
            Profile::Now => "now",
            Profile::Unit => "unit",
            Profile::Docker => "docker",
            Profile::Network => "network",
            Profile::Heat => "heat",
        }
    }

    /// Das Profil zu einem Namen auf dem Draht.
    pub fn from_name(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Eintrag in [`DIAGNOSE_PROFILES`] (Überschrift und Erklärung; die Tabelle steht in `config.rs`, weil das
    /// Einstellungsfenster `crate::diagnose` nicht einbindet). Die Reihenfolge der Tabelle ist die von [`Profile::ALL`]
    /// (ein Test hält sie gleich).
    fn info(self) -> &'static DiagnoseProfileInfo {
        &DIAGNOSE_PROFILES[self as usize]
    }

    /// Englische Überschrift für Panel und Bericht.
    pub fn label(self) -> &'static str {
        self.info().label
    }

    /// Englische Erklärung für das Panel.
    pub fn description(self) -> &'static str {
        self.info().description
    }

    /// Braucht das Profil eine Unit?
    pub fn needs_unit(self) -> bool {
        self == Profile::Unit
    }
}

/// Eingaben, die ein Profil braucht.
#[derive(Debug, Clone, Default)]
pub struct ProfileArgs {
    /// Die Unit für das Profil [`Profile::Unit`] (Suchbegriff; gelöst wird er erst beim Sammeln gegen die echten Listen).
    pub unit: Option<UnitRef>,
    /// Ein Container für das Profil [`Profile::Docker`] (Suchbegriff, z. B. aus der Palette: „warum ist dataroom kaputt“):
    /// `docker_state` und `docker_logs` für ihn, dafür ist der Schritt für die Container, die nicht laufen, abgewählt
    /// (der User fragt nach diesem einen; er kann ihn wieder anwählen). Andere Profile brauchen keinen Container.
    pub container: Option<ContainerRef>,
}

/// Warum ein Plan nicht entstand oder sich nicht ändern ließ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// Das Profil `unit` braucht eine Unit.
    NeedsUnit,
    /// Es gibt keinen Eintrag mit diesem Index.
    NoSuchItem(usize),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::NeedsUnit => write!(f, "the profile \"unit\" needs a unit"),
            PlanError::NoSuchItem(i) => write!(f, "the plan has no item {i}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Ein Schritt der Sammlung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Eine Probe des Katalogs.
    Probe(Probe),
    /// `docker_state` und `docker_logs` für die Container, die nach der Liste von `docker_ps` nicht laufen (siehe
    /// Modulkopf). Erst beim Sammeln bekannt, darum kein fester Satz von Proben.
    ProblemContainers,
}

impl Step {
    /// Kurze Bezeichnung für das Panel („docker_ps“, „docker_state + docker_logs: containers that are not running“).
    pub fn label(&self) -> String {
        match self {
            Step::Probe(p) => p.name().to_string(),
            Step::ProblemContainers => "docker_state + docker_logs: containers that are not running".to_string(),
        }
    }
}

/// Ein Eintrag des Plans mit Haken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    pub step: Step,
    /// Läuft beim Sammeln. Abgewählte Einträge laufen nicht und stehen nicht im Bericht.
    pub selected: bool,
}

/// Die Probenliste eines Profils mit Auswahl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    profile: Profile,
    items: Vec<PlanItem>,
}

fn on(p: Probe) -> PlanItem {
    PlanItem { step: Step::Probe(p), selected: true }
}

fn off(p: Probe) -> PlanItem {
    PlanItem { step: Step::Probe(p), selected: false }
}

impl Plan {
    /// Der Plan eines Profils mit der Vorgabe der Auswahl.
    pub fn new(profile: Profile, args: &ProfileArgs) -> Result<Plan, PlanError> {
        let previous = BootOffset::PREVIOUS;
        let current = BootOffset::CURRENT;
        let items = match profile {
            Profile::LastCrash => vec![
                on(Probe::Boots {}),
                on(Probe::BootEnd { boot: previous, lines: Lines::N200 }),
                on(Probe::KernelErrors { boot: previous, lines: Lines::N200 }),
                on(Probe::Pstore { boot: previous }),
                on(Probe::TempsTail { boot: previous, window_min: WindowMin::M15 }),
                on(Probe::PacmanLog { boot: previous, days: Days::D3 }),
                on(Probe::DriverVersions {}),
                on(Probe::KernelInfo {}),
            ],
            Profile::Now => vec![
                on(Probe::FailedUnits {}),
                on(Probe::KernelErrors { boot: current, lines: Lines::N200 }),
                on(Probe::Mem {}),
                on(Probe::Load {}),
                on(Probe::Disks {}),
                on(Probe::Sensors {}),
                on(Probe::Gpu {}),
            ],
            Profile::Unit => {
                let unit = args.unit.clone().ok_or(PlanError::NeedsUnit)?;
                vec![
                    on(Probe::UnitStatus { unit: unit.clone() }),
                    on(Probe::UnitShow { unit: unit.clone() }),
                    on(Probe::UnitJournal { unit, since: Since::Boot, lines: Lines::N200 }),
                ]
            }
            Profile::Docker => {
                let mut items = vec![
                    on(Probe::DockerPs {}),
                    // `docker system df` arbeitet in dockerd, ungedrosselt: nur auf Wunsch (Review I4)
                    off(Probe::DockerDf {}),
                ];
                if let Some(c) = &args.container {
                    items.extend(container_probes(std::slice::from_ref(c)).into_iter().map(on));
                }
                items.push(PlanItem { step: Step::ProblemContainers, selected: args.container.is_none() });
                items
            }
            Profile::Network => {
                vec![on(Probe::NetLinks {}), on(Probe::NetDns {}), on(Probe::NetListen {}), on(Probe::NetTailscale {}), on(Probe::NetMounts {})]
            }
            Profile::Heat => vec![
                on(Probe::Sensors {}),
                on(Probe::Gpu {}),
                on(Probe::NvmeTemp {}),
                on(Probe::TempsTail { boot: current, window_min: WindowMin::M15 }),
                on(Probe::Load {}),
                on(Probe::TopProcs {}),
            ],
        };
        Ok(Plan { profile, items })
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn items(&self) -> &[PlanItem] {
        &self.items
    }

    /// Setzt oder löst den Haken eines Eintrags. `docker_ps` und der Schritt für die Container, die nicht laufen, gehören
    /// zusammen: der zweite braucht die Liste des ersten, also wählt man mit dem einen den anderen ab, und wer den zweiten
    /// anwählt, wählt `docker_ps` an.
    pub fn set_selected(&mut self, index: usize, selected: bool) -> Result<(), PlanError> {
        let item = self.items.get_mut(index).ok_or(PlanError::NoSuchItem(index))?;
        item.selected = selected;
        let linked = match &item.step {
            Step::ProblemContainers if selected => Some(true),
            Step::Probe(Probe::DockerPs {}) if !selected => Some(false),
            _ => None,
        };
        if let Some(value) = linked {
            for other in &mut self.items {
                let is_partner = matches!((&other.step, value), (Step::Probe(Probe::DockerPs {}), true) | (Step::ProblemContainers, false));
                if is_partner {
                    other.selected = value;
                }
            }
        }
        Ok(())
    }

    /// Die Schritte, die laufen (angewählt), in der Reihenfolge des Berichts.
    pub fn selected_steps(&self) -> Vec<Step> {
        self.items.iter().filter(|i| i.selected).map(|i| i.step.clone()).collect()
    }
}

// ---- Container, die nicht laufen ----------------------------------------------------------------------------------------

/// Schwere eines Zustands: kleiner = wichtiger. `None`: läuft, kein Fall für das Profil.
fn severity(c: &ListedContainer) -> Option<u8> {
    match c.state.as_str() {
        "running" => None,
        "restarting" => Some(0),
        "dead" => Some(1),
        "exited" => Some(if exit_code(&c.status) == Some(0) { 5 } else { 2 }),
        "paused" => Some(3),
        "created" => Some(4),
        _ => Some(6),
    }
}

/// Exit-Code aus dem `Status` von Docker („Exited (137) 3 hours ago“).
fn exit_code(status: &str) -> Option<i64> {
    let rest = status.strip_prefix("Exited (")?;
    rest.split(')').next()?.trim().parse().ok()
}

/// Die Container für `docker_state` und `docker_logs`: höchstens [`MAX_PROBLEM_CONTAINERS`] der Container, die nicht laufen,
/// nach Schwere und dann in der Reihenfolge der Liste; dazu, wie viele Container, die nicht laufen, fehlen. Ein Name, der
/// kein gültiger Suchbegriff ist, fällt weg (er ließe sich nicht auflösen) und zählt als fehlend.
pub fn problem_containers(containers: &[ListedContainer]) -> (Vec<ContainerRef>, usize) {
    let mut ranked: Vec<(u8, usize, &ListedContainer)> = containers.iter().enumerate().filter_map(|(i, c)| severity(c).map(|s| (s, i, c))).collect();
    ranked.sort_by_key(|(s, i, _)| (*s, *i));
    let total = ranked.len();
    let chosen: Vec<ContainerRef> = ranked.iter().filter_map(|(_, _, c)| ContainerRef::new(&c.name).ok()).take(MAX_PROBLEM_CONTAINERS).collect();
    (chosen.clone(), total - chosen.len())
}

/// Die Probe für `docker_state` und `docker_logs` je Container (in dieser Reihenfolge je Container).
pub fn container_probes(containers: &[ContainerRef]) -> Vec<Probe> {
    containers
        .iter()
        .flat_map(|c| [Probe::DockerState { container: c.clone() }, Probe::DockerLogs { container: c.clone(), since: Since::Hours24, lines: Lines::N200 }])
        .collect()
}

/// Hinweis für den Abschnitt von `docker_ps`: was der Schritt gesammelt hat.
pub fn problem_containers_note(collected: usize, left_out: usize) -> Option<String> {
    match (collected, left_out) {
        (0, 0) => Some("every container is running; state and logs were not collected".to_string()),
        (_, 0) => None,
        (0, n) => Some(format!("{n} containers are not running, but their names cannot be looked up; use docker_state or docker_logs with a name")),
        (c, n) => Some(format!("{n} more containers are not running; state and logs were collected for the {c} that need attention most, ask for docker_state or docker_logs with a name for the others")),
    }
}

/// Die Namen der fehlgeschlagenen Units aus der Ausgabe von `failed_units`, soweit sie sich als Suchbegriff eignen, mit
/// dem Manager (`user`). Daraus wählt der User die Unit für das Profil [`Profile::Unit`].
pub fn failed_unit_choices(output: &ProbeOutput) -> Vec<(UnitRef, bool)> {
    output.listed.failed_units.iter().filter_map(|u| UnitRef::new(&u.name).ok().map(|r| (r, u.user))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::probes::ListedUnit;

    fn names(plan: &Plan) -> Vec<String> {
        plan.items().iter().map(|i| i.step.label()).collect()
    }

    fn unit() -> ProfileArgs {
        ProfileArgs { unit: Some(UnitRef::new("docker").unwrap()), container: None }
    }

    #[test]
    fn last_crash_is_the_list_of_the_plan() {
        let plan = Plan::new(Profile::LastCrash, &ProfileArgs::default()).unwrap();
        assert_eq!(names(&plan), ["boots", "boot_end", "kernel_errors", "pstore", "temps_tail", "pacman_log", "driver_versions", "kernel_info"]);
        // alles zum vorigen Boot
        let probes: Vec<&Probe> = plan
            .items()
            .iter()
            .map(|i| match &i.step {
                Step::Probe(p) => p,
                Step::ProblemContainers => panic!("no container step"),
            })
            .collect();
        for p in &probes {
            if let Some(b) = p.boot() {
                assert_eq!(b, BootOffset::PREVIOUS, "{}", p.name());
            }
        }
        assert!(plan.items().iter().all(|i| i.selected));
    }

    #[test]
    fn now_heat_and_network_are_the_lists_of_the_plan() {
        let now = Plan::new(Profile::Now, &ProfileArgs::default()).unwrap();
        assert_eq!(names(&now), ["failed_units", "kernel_errors", "mem", "load", "disks", "sensors", "gpu"]);
        match &now.items()[1].step {
            Step::Probe(p) => assert_eq!(p.boot(), Some(BootOffset::CURRENT)),
            other => panic!("{other:?}"),
        }
        let heat = Plan::new(Profile::Heat, &ProfileArgs::default()).unwrap();
        assert_eq!(names(&heat), ["sensors", "gpu", "nvme_temp", "temps_tail", "load", "top_procs"]);
        match &heat.items()[3].step {
            Step::Probe(p) => assert_eq!(p.boot(), Some(BootOffset::CURRENT)),
            other => panic!("{other:?}"),
        }
        let net = Plan::new(Profile::Network, &ProfileArgs::default()).unwrap();
        assert_eq!(names(&net), ["net_links", "net_dns", "net_listen", "net_tailscale", "net_mounts"]);
    }

    #[test]
    fn no_profile_contains_a_probe_that_sends_network_traffic() {
        for profile in Profile::ALL {
            let plan = Plan::new(profile, &unit()).unwrap();
            for item in plan.items() {
                if let Step::Probe(p) = &item.step {
                    assert!(!p.spec().network_traffic, "{profile:?} has {}", p.name());
                }
            }
        }
    }

    #[test]
    fn every_probe_of_every_profile_is_valid() {
        for profile in Profile::ALL {
            let plan = Plan::new(profile, &unit()).unwrap();
            for item in plan.items() {
                if let Step::Probe(p) = &item.step {
                    p.validate().unwrap_or_else(|e| panic!("{profile:?} {}: {e}", p.name()));
                }
            }
        }
    }

    #[test]
    fn the_unit_profile_needs_a_unit_and_uses_it_three_times() {
        assert_eq!(Plan::new(Profile::Unit, &ProfileArgs::default()), Err(PlanError::NeedsUnit));
        let plan = Plan::new(Profile::Unit, &unit()).unwrap();
        assert_eq!(names(&plan), ["unit_status", "unit_show", "unit_journal"]);
        for item in plan.items() {
            let Step::Probe(p) = &item.step else { panic!() };
            assert_eq!(p.unit().map(UnitRef::as_str), Some("docker"));
        }
        assert!(Profile::Unit.needs_unit());
        assert!(!Profile::Now.needs_unit());
        // die anderen Profile ignorieren die Unit
        assert_eq!(Plan::new(Profile::Now, &unit()).unwrap(), Plan::new(Profile::Now, &ProfileArgs::default()).unwrap());
    }

    #[test]
    fn docker_df_is_in_the_docker_profile_but_not_selected() {
        let plan = Plan::new(Profile::Docker, &ProfileArgs::default()).unwrap();
        assert_eq!(names(&plan), ["docker_ps", "docker_df", "docker_state + docker_logs: containers that are not running"]);
        let df = plan.items().iter().position(|i| i.step == Step::Probe(Probe::DockerDf {})).expect("docker_df is in the profile");
        assert!(!plan.items()[df].selected);
        assert_eq!(plan.selected_steps(), [Step::Probe(Probe::DockerPs {}), Step::ProblemContainers]);
        // auf Wunsch angewählt, in der Reihenfolge des Profils
        let mut plan = plan;
        plan.set_selected(df, true).unwrap();
        assert_eq!(plan.selected_steps(), [Step::Probe(Probe::DockerPs {}), Step::Probe(Probe::DockerDf {}), Step::ProblemContainers]);
        // und kein anderes Profil hat docker_df
        for profile in Profile::ALL.into_iter().filter(|p| *p != Profile::Docker) {
            let other = Plan::new(profile, &unit()).unwrap();
            assert!(other.items().iter().all(|i| i.step != Step::Probe(Probe::DockerDf {})), "{profile:?}");
        }
    }

    #[test]
    fn docker_ps_and_the_container_step_are_selected_together() {
        let mut plan = Plan::new(Profile::Docker, &ProfileArgs::default()).unwrap();
        plan.set_selected(0, false).unwrap();
        assert!(!plan.items()[2].selected, "without the list there are no containers to look at");
        plan.set_selected(2, true).unwrap();
        assert!(plan.items()[0].selected, "the step needs the list");
        plan.set_selected(2, false).unwrap();
        assert!(plan.items()[0].selected, "deselecting the step keeps the list");
        assert_eq!(plan.set_selected(9, true), Err(PlanError::NoSuchItem(9)));
    }

    #[test]
    fn profile_names_round_trip() {
        for (i, p) in Profile::ALL.into_iter().enumerate() {
            // die Tabelle in `config.rs` (Einstellungsfenster) steht in der Reihenfolge von `Profile::ALL`
            assert_eq!((p as usize, DIAGNOSE_PROFILES[i].name), (i, p.name()));
            assert_eq!(Profile::from_name(p.name()), Some(p));
            assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.name()));
            assert!(!p.label().is_empty() && !p.description().is_empty());
        }
        assert_eq!(Profile::from_name("everything"), None);
    }

    fn container(name: &str, state: &str, status: &str) -> ListedContainer {
        ListedContainer { name: name.into(), state: state.into(), status: status.into() }
    }

    #[test]
    fn problem_containers_are_the_ones_that_do_not_run_ordered_by_severity() {
        let list = vec![
            container("web", "running", "Up 2 hours"),
            container("batch", "exited", "Exited (0) 3 hours ago"),
            container("db", "exited", "Exited (137) 3 hours ago"),
            container("worker", "restarting", "Restarting (1) 5 seconds ago"),
            container("old", "dead", "Dead"),
            container("api", "running", "Up 5 minutes (unhealthy)"),
        ];
        let (chosen, left) = problem_containers(&list);
        let names: Vec<&str> = chosen.iter().map(ContainerRef::as_str).collect();
        assert_eq!(names, ["worker", "old", "db", "batch"]);
        assert_eq!(left, 0);
        let probes = container_probes(&chosen);
        assert_eq!(probes.len(), 8);
        assert_eq!(probes[0].name(), "docker_state");
        assert_eq!(probes[1].name(), "docker_logs");
        assert_eq!(probes[1].container().map(ContainerRef::as_str), Some("worker"));
        for p in &probes {
            p.validate().unwrap();
        }
    }

    #[test]
    fn problem_containers_are_capped_and_the_rest_is_counted() {
        let list: Vec<ListedContainer> = (0..7).map(|i| container(&format!("job-{i}"), "exited", "Exited (1) 1 hour ago")).collect();
        let (chosen, left) = problem_containers(&list);
        assert_eq!(chosen.len(), MAX_PROBLEM_CONTAINERS);
        assert_eq!(left, 7 - MAX_PROBLEM_CONTAINERS);
        // stabil: bei gleicher Schwere die Reihenfolge der Liste
        assert_eq!(chosen[0].as_str(), "job-0");
        let note = problem_containers_note(chosen.len(), left).unwrap();
        assert!(note.contains("3 more containers") && note.contains("4"), "{note}");
        assert_eq!(problem_containers_note(2, 0), None);
        assert!(problem_containers_note(0, 0).unwrap().contains("running"));
    }

    #[test]
    fn names_that_cannot_be_looked_up_are_not_chosen() {
        // Docker erlaubt sie nicht, aber die Liste ist fremder Text
        let list =
            vec![container("--all", "exited", "Exited (1) 1 hour ago"), container("fine", "exited", "Exited (1) 1 hour ago"), container("a b", "dead", "Dead")];
        let (chosen, left) = problem_containers(&list);
        assert_eq!(chosen.iter().map(ContainerRef::as_str).collect::<Vec<_>>(), ["fine"]);
        assert_eq!(left, 2);
    }

    #[test]
    fn failed_unit_choices_keep_only_names_that_can_be_searched() {
        let mut out = ProbeOutput::new("failed_units");
        out.listed.failed_units = vec![
            ListedUnit { name: "docker.service".into(), user: false },
            ListedUnit { name: "-.mount".into(), user: false },
            ListedUnit { name: "dictate.service".into(), user: true },
        ];
        let choices = failed_unit_choices(&out);
        assert_eq!(choices.iter().map(|(u, user)| (u.as_str(), *user)).collect::<Vec<_>>(), [("docker.service", false), ("dictate.service", true)]);
    }

    #[test]
    fn docker_profile_with_a_container_collects_it_and_leaves_the_others_to_the_user() {
        let args = ProfileArgs { unit: None, container: Some(ContainerRef::new("dataroom").unwrap()) };
        let plan = Plan::new(Profile::Docker, &args).unwrap();
        let items: Vec<(String, bool)> = plan.items().iter().map(|i| (i.step.label(), i.selected)).collect();
        assert_eq!(
            items,
            [
                ("docker_ps".to_string(), true),
                ("docker_df".to_string(), false),
                ("docker_state".to_string(), true),
                ("docker_logs".to_string(), true),
                (Step::ProblemContainers.label(), false),
            ]
        );
        let steps = plan.selected_steps();
        assert!(matches!(&steps[1], Step::Probe(Probe::DockerState { container }) if container.as_str() == "dataroom"), "{steps:?}");
        // ohne Container wie bisher; andere Profile übergehen ihn
        assert_eq!(Plan::new(Profile::Docker, &ProfileArgs::default()).unwrap().items().len(), 3);
        assert_eq!(Plan::new(Profile::Heat, &args).unwrap(), Plan::new(Profile::Heat, &ProfileArgs::default()).unwrap());
    }
}
