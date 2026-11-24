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

//! Dienst der Diagnose im Daemon (Aufgabe 7 in `PLAN-diagnose.md`): die Sitzung, ihr Ablauf und die Befehle `diag_*`.
//! Das Protokoll steht in `PLAN-diagnose.md` („Stand je Aufgabe“ › Aufgabe 7) und an [`crate::protocol::Request`].
//!
//! - **Auf dem Verbindungs-Thread.** Sammeln und Prüfen (bis zu 15 s) laufen auf dem Thread der Verbindung, die sie
//!   angefordert hat; der Kern liefert nur die Einstellungen. Fortschritt geht als Zwischenzeilen
//!   (`{"progress":{…}}`) vor der Antwort auf dieselbe Verbindung. Legt der Client auf (Panel zu), setzt der
//!   Daemon das Abbruch-Flag (`HangupWatch`), ebenso ein gescheitertes Schreiben einer Zwischenzeile; die Probe endet in
//!   unter 200 ms, und es bleibt nichts zurück (ein abgebrochenes Sammeln legt keine Sitzung an, ein abgebrochenes Prüfen
//!   ändert sie nicht).
//! - **Eine Sitzung** ([`Diagnose`]): `diag_collect` ersetzt die vorige vollständig (ein laufender Vorgang wird
//!   abgebrochen). Die Sitzung lebt höchstens [`SESSION_TTL`] (30 min ab dem Sammeln, nicht ab der letzten Benutzung)
//!   oder bis `diag_clear`; ein eigener Thread ([`DiagSweeper`]) löscht sie zur Frist, auch ohne weitere Anfrage. Ids
//!   zählen hoch, eine alte Id trifft nie die neue Sitzung.
//! - **Ein Vorgang je Sitzung**, und eine Sammlung je Prozess (`CollectLock`): ein zweites Sammeln oder Prüfen wird
//!   abgewiesen, außer `diag_collect` hat gerade einen Vorgang der ersetzten Sitzung abgebrochen; dann wartet es kurz
//!   ([`REPLACE_WAIT`]), bis dessen Probe beendet ist.
//! - **Vorschau = Senden.** Jede Änderung am Bericht (Sammeln, Prüfen, Abwahl) zählt die Nummer der Vorschau hoch;
//!   `diag_send` nennt die Nummer der Vorschau, die der User gesehen hat, und wird abgewiesen, wenn sie nicht mehr gilt.
//!   Ebenso, wenn der KI-Server seit dieser Vorschau ein anderer ist („AI server changed“, wie `act_run`): die Vorschau
//!   zeigt Server und Modell, und nur, was sie gezeigt hat, darf gesendet werden.
//! - **Runden mit dem Modell** (Aufgabe 6, [`super::ai`]): `diag_send` schickt genau den Text der Vorschau an das Modell
//!   (Modell `diagnose.model_for`, Frist `diagnose.timeout_ms`, Abbruch wie beim Sammeln) und antwortet mit der geprüften
//!   Antwort. Höchstens `diagnose.rounds` Antworten je Sitzung; dieselbe Vorschau wird nicht zweimal gesendet. Prüfbefehle
//!   (`diag_check`) nimmt der Dienst nur, wenn die letzte Antwort sie vorgeschlagen hat (gleiche Probe, gleiche
//!   Parameter), und nur einmal je Antwort: danach folgt die nächste Runde. Ein gescheitertes oder abgebrochenes Senden
//!   ändert die Sitzung nicht.
//! - **Symptom des Users** (PLAN-wayland.md 6e Punkt 7): `diag_preview` mit `symptom` setzt bzw. entfernt (leer) den
//!   Text „What is wrong?“ aus dem Panel; er steht gefiltert als gekennzeichnete Zeile im Kopf des Berichts
//!   ([`super::report::SYMPTOM_LABEL`]), ist damit Teil von Vorschau, Copy und Gesendetem, zählt die Nummer der Vorschau
//!   hoch und lässt den System-Prompt die Zeile als Daten nennen. Höchstens [`super::report::MAX_SYMPTOM_CHARS`] Zeichen;
//!   im Log nur „symptom set/removed“ mit Zahlen.
//! - **Schalter** `[diagnose] enabled` (Vorgabe aus): aus = jeder Befehl wird abgewiesen, eine vorhandene Sitzung gelöscht.
//! - **Nichts dauerhaft, nichts im Log.** Bericht und Ausgaben leben nur in der Sitzung im Speicher. Im Log stehen nur
//!   Profil, Probe-IDs, Status, Dauer, Bytes und die Zahl der Schwärzungen und Platzhalter, nie Inhalte, Parameter,
//!   Meldungen der Proben oder Namen (auch nicht mit `DICTATE_DEBUG`).
//! - **Rückübersetzung der Platzhalter** (Vorgabe I1, [`super::restore`]): die Antwort auf `diag_send` trägt neben
//!   `answer` die Spannen `restore` (Platzhalter → Klartext, nur in `findings[].cause` und `notes`, nie in `checks`, Code,
//!   Links oder URLs). Der Klartext geht nur an den Client dieser Antwort (das Panel), nie an das Modell, nie ins Log,
//!   nie in `diag_copy`.
//! - **Wer darf:** wie jeder Befehl am Steuer-Socket (0600 in einem Ordner 0700 unter `$XDG_RUNTIME_DIR`) jeder Prozess
//!   mit derselben uid, auch für `diag_send` und `diag_check` mit Netzverkehr (Review I7). Andere Benutzer kommen nicht
//!   heran. Eine eigene Bestätigung im Daemon gibt es nicht: ein Prozess derselben uid kann ohnehin den Daemon, die
//!   Config und das Panel selbst ändern; die Bestätigung durch den User leistet das Panel (Leertaste und Enter).

use super::ai::{ask_diagnosis, DiagAiError, DiagRequest, SectionLines};
use super::boot_hint::{already_checked, decide, read_boot_id, BootHintState, Why};
use super::identity::system_identity;
use super::probe::{parse_value, ContainerRef, Probe, UnitRef};
use super::probes::ProbeContext;
use super::profile::{Plan, Profile, ProfileArgs, Step};
use super::pseudo::Identity;
use super::report::{
    check_boots_with, gather_steps, list_failed_units_with, CollectError, CollectLock, CollectParams, Gathered, Progress, ReportBudget, Session,
    COLLECT_BUDGET, MAX_SECTIONS,
};
use crate::actions::store::TargetKey;
use crate::config::{Config, DiagnoseCfg};
use crate::pipeline::ai::CancelFlag;
use crate::protocol::Reply;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Höchste Lebensdauer einer Sitzung ab dem Sammeln (`PLAN-diagnose.md`, Aufgabe 7).
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// So lange wartet `diag_collect` auf die Sperre, wenn es gerade einen Vorgang der ersetzten Sitzung abgebrochen hat
/// (Ende der laufenden Probe in unter 200 ms plus Überlauf des Ausführers).
pub const REPLACE_WAIT: Duration = Duration::from_secs(2);
/// Höchstzahl Prüfbefehle je `diag_check`.
pub const MAX_CHECKS: usize = 16;

/// Meldung bei `enabled = false`.
pub const DISABLED: &str = "Diagnosis is off (Settings › Diagnose)";
/// Unbekannte, ersetzte, gelöschte oder abgelaufene Sitzung.
pub const UNKNOWN_SESSION: &str = "Unknown or expired diagnosis, collect again";
/// `diag_check` ohne Vorschlag des Modells.
pub const NOT_PROPOSED: &str = "was not proposed by the AI in its last answer";
/// `diag_preview` mit Abwahl, während gesendet oder geprüft wird.
pub const BUSY_REPORT: &str = "The report cannot change while it is being sent or checked";

/// Baut den Kontext der Proben aus der Config (Produktion: das echte System; Tests: Fixture-Ordner mit Fakes).
pub type ContextFactory = Arc<dyn Fn(&DiagnoseCfg) -> Result<ProbeContext, String> + Send + Sync>;
/// Liefert die Identität für das Pseudonymisieren (Produktion: [`system_identity`]; Tests: erfunden).
pub type IdentityFactory = Arc<dyn Fn(&DiagnoseCfg) -> Identity + Send + Sync>;

/// Was der Dienst von außen braucht. Produktion: [`DiagDeps::system`].
#[derive(Clone)]
pub struct DiagDeps {
    pub context: ContextFactory,
    pub identity: IdentityFactory,
    /// Sperre der Sammlungen; Produktion [`CollectLock::global`], Tests eine eigene je Daemon.
    pub lock: &'static CollectLock,
    /// Lebensdauer der Sitzung ([`SESSION_TTL`]; Tests kürzer).
    pub ttl: Duration,
    /// Datei, in der der Hinweis nach einem unsauberen Boot-Ende den geprüften Boot merkt (`diagnose.json` im
    /// State-Ordner). `None` = der Daemon setzt sie neben die Verlaufsdatei; Tests nehmen eine eigene.
    pub state: Option<PathBuf>,
}

impl DiagDeps {
    /// Produktion: Proben gegen das System, Identität aus dem System. In den Unit-Tests der Bibliothek gibt es keine
    /// Proben gegen das System (dort wie [`DiagDeps::unavailable`]).
    pub fn system() -> DiagDeps {
        #[cfg(not(test))]
        let context: ContextFactory = Arc::new(|cfg: &DiagnoseCfg| {
            let targets = cfg.ping_targets.iter().map(|t| super::PingTargetCfg { name: t.name.clone(), address: t.address.clone() }).collect();
            ProbeContext::system(cfg.temp_log_path(), targets)
        });
        #[cfg(test)]
        let context: ContextFactory = Arc::new(|_: &DiagnoseCfg| Err("no probes against the system in tests".to_string()));
        DiagDeps {
            context,
            identity: Arc::new(|cfg: &DiagnoseCfg| {
                let dir = cfg.temp_log_path();
                system_identity(dir.as_deref().as_slice())
            }),
            lock: CollectLock::global(),
            ttl: SESSION_TTL,
            state: None,
        }
    }

    /// Ohne Proben: jede Sammlung scheitert mit einer Meldung (Daemon-Tests, die die Diagnose nicht prüfen).
    pub fn unavailable() -> DiagDeps {
        DiagDeps {
            context: Arc::new(|_: &DiagnoseCfg| Err("diagnosis is not available in this daemon".to_string())),
            identity: Arc::new(|_: &DiagnoseCfg| Identity::default()),
            lock: Box::leak(Box::new(CollectLock::new())),
            ttl: SESSION_TTL,
            state: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Ziel der KI

/// Server und Modell, wie die Vorschau sie zeigt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    key: TargetKey,
    host: String,
    model: String,
    /// Warum nicht gesendet werden kann (Endpoint ungültig, kein Modell); die Vorschau zeigt es
    problem: Option<String>,
}

impl Target {
    fn of(cfg: &Config) -> Target {
        let (protocol, base) = cfg.ai.resolved();
        let host = crate::config::parse_endpoint(&base).map(|e| e.host).unwrap_or_default();
        let model = cfg.diagnose.model_for(&cfg.ai).to_string();
        let problem = match crate::pipeline::ai::resolve(&cfg.ai) {
            Err(e) => Some(format!("AI error: {e}")),
            Ok(_) if model.is_empty() => Some(format!("No model: {} has no default model (Settings › Diagnose)", cfg.ai.server_name())),
            Ok(_) => None,
        };
        Target { key: TargetKey { protocol, base, cloud: cfg.ai.is_cloud() }, host, model, problem }
    }

    fn json(&self) -> Value {
        let mut v = json!({"host": self.host, "model": self.model, "cloud": self.key.cloud, "ready": self.problem.is_none()});
        if let Some(p) = &self.problem {
            v["problem"] = json!(p);
        }
        v
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Speicher der Sitzung

/// Ein laufender Vorgang (Sammeln oder Prüfen).
struct Op {
    seq: u64,
    cancel: CancelFlag,
}

struct Entry {
    id: u64,
    created: Instant,
    /// `None`, solange das erste Sammeln läuft
    session: Option<Session>,
    op: Option<Op>,
    /// Nummer der Vorschau: zählt bei jeder Änderung des Berichts hoch
    preview: u64,
    /// Das Ziel, das die letzte Vorschau gezeigt hat
    shown: Option<Target>,
    /// Antworten des Modells in dieser Sitzung (Runden)
    answers: u32,
    /// Nummer der zuletzt gesendeten Vorschau (dieselbe wird nicht zweimal gesendet)
    sent_preview: Option<u64>,
    /// Prüfbefehle der letzten Antwort; leer nach `diag_check` (eine Prüfung je Antwort)
    proposed: Vec<Probe>,
}

impl Entry {
    fn cancel_op(&mut self) -> bool {
        match self.op.take() {
            Some(op) => {
                op.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }
}

struct State {
    entry: Option<Entry>,
    next_id: u64,
    next_seq: u64,
    ttl: Duration,
}

impl State {
    /// Löscht eine abgelaufene Sitzung (bricht ihren Vorgang ab). `true`, wenn etwas gelöscht wurde.
    fn expire(&mut self, now: Instant) -> bool {
        if self.entry.as_ref().is_some_and(|e| now >= e.created + self.ttl) {
            self.clear();
            return true;
        }
        false
    }

    fn clear(&mut self) -> bool {
        match self.entry.take() {
            Some(mut e) => {
                e.cancel_op();
                true
            }
            None => false,
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.entry.as_ref().map(|e| e.created + self.ttl)
    }

    /// Die Sitzung mit dieser Id (nach dem Ablauf).
    fn get(&mut self, id: u64, now: Instant) -> Option<&mut Entry> {
        self.expire(now);
        self.entry.as_mut().filter(|e| e.id == id)
    }

    fn seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    stop: AtomicBool,
    /// Hält Lesen, Entscheiden und Merken des Boot-Hinweises zusammen (zwei Anfragen zugleich melden einen Boot nur einmal)
    boot_hint: Mutex<()>,
}

/// Der Dienst, geteilt zwischen den Verbindungs-Threads.
#[derive(Clone)]
pub struct Diagnose {
    shared: Arc<Shared>,
    deps: DiagDeps,
}

/// Ablauf-Thread; `Drop` beendet ihn und löscht die Sitzung.
pub struct DiagSweeper {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for DiagSweeper {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        drop(lock(&self.shared.state));
        self.shared.changed.notify_all();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        lock(&self.shared.state).clear();
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn sweep(shared: &Shared) {
    let mut g = lock(&shared.state);
    while !shared.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if g.expire(now) {
            info!("diag session expired");
        }
        g = match g.deadline() {
            None => shared.changed.wait(g).unwrap_or_else(|e| e.into_inner()),
            Some(d) => shared.changed.wait_timeout(g, d.saturating_duration_since(now)).unwrap_or_else(|e| e.into_inner()).0,
        };
    }
}

/// Ids sind Dezimalzahlen als String (`"7"`); alles andere ist unbekannt.
fn parse_id(id: &str) -> Option<u64> {
    id.trim().parse().ok()
}

fn cancelled() -> Reply {
    Reply::err("Cancelled").with("cancelled", json!(true))
}

fn unknown() -> Reply {
    Reply::err(UNKNOWN_SESSION)
}

/// Schreibt eine Zwischenzeile (ohne `\n`) vor der Antwort auf die Verbindung der Anfrage.
pub type LineSink<'a> = &'a mut dyn FnMut(&str) -> std::io::Result<()>;

/// Was `diag_collect` sammeln soll.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectRequest {
    pub profile: String,
    pub unit: Option<String>,
    pub container: Option<String>,
    /// Einträge des Plans (Index wie in `diag_plan`), die nicht laufen sollen
    pub skip: Vec<usize>,
}

impl Diagnose {
    /// Dienst mit Ablauf-Thread.
    pub fn start(deps: DiagDeps) -> std::io::Result<(Diagnose, DiagSweeper)> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { entry: None, next_id: 0, next_seq: 0, ttl: deps.ttl }),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
            boot_hint: Mutex::new(()),
        });
        let s2 = shared.clone();
        let handle = std::thread::Builder::new().name("dictate-diag-ttl".into()).spawn(move || sweep(&s2))?;
        Ok((Diagnose { shared: shared.clone(), deps }, DiagSweeper { shared, handle: Some(handle) }))
    }

    fn with<T>(&self, f: impl FnOnce(&mut State, Instant) -> T) -> T {
        let mut g = lock(&self.shared.state);
        let r = f(&mut g, Instant::now());
        drop(g);
        self.shared.changed.notify_all();
        r
    }

    /// Schalter aus: Sitzung löschen und abweisen.
    fn off(&self, cfg: &Config) -> Option<Reply> {
        if cfg.diagnose.enabled {
            return None;
        }
        if self.with(|s, _| s.clear()) {
            info!("diag session cleared: diagnosis is off");
        }
        Some(Reply::err(DISABLED))
    }

    /// `diag_plan`: ohne Profil die Liste der Profile, mit Profil dessen Einträge mit Vorgabe der Auswahl.
    pub fn plan(&self, cfg: &Config, profile: Option<&str>, unit: Option<&str>, container: Option<&str>) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let Some(name) = profile else {
            let list: Vec<Value> = Profile::ALL
                .iter()
                .map(|p| json!({"name": p.name(), "label": p.label(), "description": p.description(), "needs_unit": p.needs_unit()}))
                .collect();
            return Reply::data(json!({"profiles": list}));
        };
        match make_plan(name, unit, container) {
            Ok(plan) => Reply::data(plan_json(&plan)),
            Err(r) => r,
        }
    }

    /// `diag_units`: die fehlgeschlagenen Units zur Auswahl für das Profil `unit` (eine Probe, unter der Sperre).
    pub fn units(&self, cfg: &Config, cancel: &CancelFlag) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let ctx = match (self.deps.context)(&cfg.diagnose) {
            Ok(c) => c,
            Err(e) => return Reply::err(format!("Cannot collect: {e}")),
        };
        let started = Instant::now();
        match list_failed_units_with(self.deps.lock, &ctx, cancel, COLLECT_BUDGET) {
            Ok(units) => {
                info!("diag units: {} failed units in {} ms", units.len(), started.elapsed().as_millis());
                Reply::data(json!({"units": units.iter().map(|(u, user)| json!({"unit": u.as_str(), "user": user})).collect::<Vec<_>>()}))
            }
            Err(CollectError::Cancelled) => cancelled(),
            Err(e) => Reply::err(collect_error(e)),
        }
    }

    /// `diag_collect`: neue Sitzung, Plan sammeln (Fortschritt über `out`), Vorschau.
    pub fn collect(&self, cfg: &Config, req: &CollectRequest, cancel: CancelFlag, out: LineSink) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let mut plan = match make_plan(&req.profile, req.unit.as_deref(), req.container.as_deref()) {
            Ok(p) => p,
            Err(r) => return r,
        };
        for &i in &req.skip {
            if plan.set_selected(i, false).is_err() {
                return Reply::err(format!("The plan of \"{}\" has no item {i}", plan.profile().name()));
            }
        }
        let steps = plan.selected_steps();
        if steps.is_empty() {
            return Reply::err("Nothing selected to collect");
        }
        let ctx = match (self.deps.context)(&cfg.diagnose) {
            Ok(c) => c,
            Err(e) => return Reply::err(format!("Cannot collect: {e}")),
        };
        let budget = match ReportBudget::from_kib(cfg.diagnose.report_kib) {
            Ok(b) => b,
            Err(e) => return Reply::field_err(e, "diagnose.report_kib"),
        };
        // Neue Sitzung: die vorige verschwindet sofort, ihr Vorgang wird abgebrochen
        let (id, seq, replaced_op) = self.with(|s, now| {
            let replaced = s.entry.as_mut().is_some_and(Entry::cancel_op);
            s.clear();
            s.next_id += 1;
            let seq = s.seq();
            s.entry = Some(Entry {
                id: s.next_id,
                created: now,
                session: None,
                op: Some(Op { seq, cancel: cancel.clone() }),
                preview: 0,
                shown: None,
                answers: 0,
                sent_preview: None,
                proposed: Vec::new(),
            });
            (s.next_id, seq, replaced)
        });
        let profile = plan.profile();
        let started = Instant::now();
        let wait = if replaced_op { REPLACE_WAIT } else { Duration::ZERO };
        let gathered = self.gather(&ctx, &cancel, Some(profile), steps, &CollectParams::default(), MAX_SECTIONS, id, wait, out);
        let gathered = match gathered {
            Ok(g) => g,
            Err(e) => {
                // nichts bleibt zurück: die Sitzung ohne Bericht entfällt (eine neuere bleibt unberührt)
                self.with(|s, _| {
                    if s.entry.as_ref().is_some_and(|e| e.id == id && e.op.as_ref().is_some_and(|o| o.seq == seq)) {
                        s.entry = None;
                    }
                });
                info!("diag collect {}: {} after {} ms", profile.name(), collect_error(e), started.elapsed().as_millis());
                return match e {
                    CollectError::Cancelled => cancelled(),
                    e => Reply::err(collect_error(e)),
                };
            }
        };
        let identity = (self.deps.identity)(&cfg.diagnose);
        let mut session = Session::new(identity, cfg.diagnose.pseudonymize, budget);
        if let Err(e) = session.add(gathered) {
            self.with(|s, _| {
                if s.entry.as_ref().is_some_and(|e| e.id == id) {
                    s.entry = None;
                }
            });
            return Reply::err(collect_error(e));
        }
        let target = Target::of(cfg);
        let reply = self.with(|s, now| {
            let ttl = s.ttl;
            let e = s.get(id, now).filter(|e| e.op.as_ref().is_some_and(|o| o.seq == seq))?;
            e.op = None;
            e.preview += 1;
            e.shown = Some(target.clone());
            e.session = Some(session);
            Some(preview_json(e, &target, cfg, ttl, now))
        });
        match reply {
            Some(v) => {
                log_report("collect", Some(profile), &v, started.elapsed());
                Reply::data(v)
            }
            // währenddessen ersetzt, gelöscht oder abgelaufen
            None => cancelled(),
        }
    }

    /// Sammelt unter der Sperre; Fortschritt als Zwischenzeilen. Wer gerade einen Vorgang abgebrochen hat, wartet bis
    /// `wait` auf die Sperre.
    #[allow(clippy::too_many_arguments)]
    fn gather(
        &self,
        ctx: &ProbeContext,
        cancel: &CancelFlag,
        profile: Option<Profile>,
        steps: Vec<Step>,
        params: &CollectParams,
        room: usize,
        id: u64,
        wait: Duration,
        out: LineSink,
    ) -> Result<Gathered, CollectError> {
        let until = Instant::now() + wait;
        let id_text = id.to_string();
        let mut progress = |p: &Progress| {
            let line = progress_line(&id_text, p);
            if let Progress::End { probe, status, bytes, duration, .. } = p {
                info!("diag probe {probe}: {status}, {bytes} bytes in {} ms", duration.as_millis());
            }
            // Client weg: abbrechen (der Rest wird nicht mehr gelesen)
            if out(&line).is_err() {
                cancel.store(true, Ordering::SeqCst);
            }
        };
        loop {
            match gather_steps(self.deps.lock, ctx, cancel, profile, steps.clone(), params, room, &mut progress) {
                Err(CollectError::Busy) if Instant::now() < until && !cancel.load(Ordering::SeqCst) => std::thread::sleep(Duration::from_millis(20)),
                r => return r,
            }
        }
    }

    /// `diag_preview`: Abwahl setzen (falls angegeben) und die Vorschau liefern; sie gilt ab jetzt für `diag_send`
    /// (Nummer und Ziel).
    /// `diag_preview`: die Vorschau, mit `deselected` als neue Abwahl und mit `symptom` als neues Symptom des Users (leer =
    /// keins; [`super::report::clean_symptom`], gefiltert wie der Bericht). Jede Änderung zählt die Nummer der Vorschau hoch.
    pub fn preview(&self, cfg: &Config, id: &str, deselected: Option<&[String]>, symptom: Option<&str>) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let Some(id) = parse_id(id) else { return unknown() };
        let target = Target::of(cfg);
        let r = self.with(|s, now| {
            let ttl = s.ttl;
            let Some(e) = s.get(id, now) else { return Err(unknown()) };
            let Some(session) = e.session.as_mut() else { return Err(Reply::err("The collection is still running")) };
            // während Senden oder Prüfen bleibt der Bericht, wie er ist: die Belege der Antwort gehören zu genau diesem Text
            // (Review L6); eine Vorschau ohne Änderung bleibt erlaubt
            if (deselected.is_some() || symptom.is_some()) && e.op.is_some() {
                return Err(Reply::err(BUSY_REPORT));
            }
            if let Some(text) = symptom {
                let before = session.outgoing();
                match session.set_symptom(Some(text)) {
                    Ok(_) => {}
                    Err(err) => return Err(Reply::err(err.to_string())),
                }
                if session.outgoing() != before {
                    e.preview += 1;
                    let v = session.report().symptom;
                    let (r, p) =
                        v.as_ref().map_or((0, 0), |v| (v.redactions.iter().map(|r| r.count).sum::<usize>(), v.pseudonyms.iter().map(|(_, n)| n).sum()));
                    info!("diag preview: symptom {}, {r} redactions, {p} placeholders", if v.is_some() { "set" } else { "removed" });
                }
            }
            if let Some(ids) = deselected {
                let before = session.outgoing();
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                if let Err(err) = session.set_deselected(&ids) {
                    return Err(Reply::err(format!("Cannot leave out: {err}")));
                }
                if session.outgoing() != before {
                    e.preview += 1;
                }
            }
            if e.shown.as_ref() != Some(&target) {
                e.preview += 1;
                e.shown = Some(target.clone());
            }
            Ok(preview_json(e, &target, cfg, ttl, now))
        });
        match r {
            Ok(v) => Reply::data(v),
            Err(r) => r,
        }
    }

    /// `diag_check`: gewählte Prüfbefehle (Proben aus dem Katalog) als weitere Runde sammeln, Vorschau.
    pub fn check(&self, cfg: &Config, id: &str, checks: &[Value], network: bool, cancel: CancelFlag, out: LineSink) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let Some(id) = parse_id(id) else { return unknown() };
        if checks.is_empty() {
            return Reply::err("No checks selected");
        }
        if checks.len() > MAX_CHECKS {
            return Reply::err(format!("Too many checks: {}, at most {MAX_CHECKS}", checks.len()));
        }
        let mut probes: Vec<Probe> = Vec::with_capacity(checks.len());
        for (i, c) in checks.iter().enumerate() {
            match parse_value(c.clone()) {
                // dieselbe Probe mit denselben Parametern zweimal gewählt: sie läuft einmal (Review I3)
                Ok(p) if probes.contains(&p) => {}
                Ok(p) => probes.push(p),
                Err(e) => return Reply::err(format!("Check {}: {e}", i + 1)),
            }
        }
        let ctx = match (self.deps.context)(&cfg.diagnose) {
            Ok(c) => c,
            Err(e) => return Reply::err(format!("Cannot collect: {e}")),
        };
        let max_rounds = cfg.diagnose.rounds;
        let begun = self.with(|s, now| {
            let seq = s.seq();
            let Some(e) = s.get(id, now) else { return Err(unknown()) };
            let Some(session) = e.session.as_ref() else { return Err(Reply::err("The collection is still running")) };
            if e.op.is_some() {
                return Err(Reply::err(collect_error(CollectError::Busy)));
            }
            if session.rounds() >= max_rounds {
                return Err(Reply::field_err(format!("This diagnosis has used all {max_rounds} rounds; collect again to start over"), "diagnose.rounds"));
            }
            // nur, was die letzte Antwort vorgeschlagen hat (das Modell nennt Proben, der User wählt daraus)
            if e.proposed.is_empty() {
                return Err(Reply::err(if e.answers == 0 {
                    "Send the report to the AI first; checks come from its answer"
                } else {
                    "No checks to run: the last answer proposed none, or they already ran"
                }));
            }
            if let Some(i) = probes.iter().position(|p| !e.proposed.contains(p)) {
                return Err(Reply::err(format!("Check {} {NOT_PROPOSED}", i + 1)));
            }
            let room = session.room();
            e.op = Some(Op { seq, cancel: cancel.clone() });
            Ok((seq, room))
        });
        let (seq, room) = match begun {
            Ok(b) => b,
            Err(r) => return r,
        };
        let started = Instant::now();
        let params = CollectParams { network_confirmed: network, ..CollectParams::default() };
        let steps = probes.into_iter().map(Step::Probe).collect();
        let gathered = self.gather(&ctx, &cancel, None, steps, &params, room, id, Duration::ZERO, out);
        let target = Target::of(cfg);
        let r = self.with(|s, now| {
            let ttl = s.ttl;
            let e = s.get(id, now).filter(|e| e.op.as_ref().is_some_and(|o| o.seq == seq));
            let Some(e) = e else { return Err(cancelled()) };
            e.op = None;
            let gathered = match gathered {
                Ok(g) => g,
                Err(CollectError::Cancelled) => return Err(cancelled()),
                Err(err) => return Err(Reply::err(collect_error(err))),
            };
            let session = e.session.as_mut().expect("eine Sitzung mit Vorgang hat einen Bericht");
            session.add(gathered).map_err(|err| Reply::err(collect_error(err)))?;
            // die Prüfungen dieser Antwort sind gelaufen; neue kommen erst mit der nächsten Antwort
            e.proposed.clear();
            e.preview += 1;
            e.shown = Some(target.clone());
            Ok(preview_json(e, &target, cfg, ttl, now))
        });
        match r {
            Ok(v) => {
                log_report("check", None, &v, started.elapsed());
                Reply::data(v)
            }
            Err(r) => {
                info!("diag check: {} after {} ms", r.error.as_deref().unwrap_or("error"), started.elapsed().as_millis());
                r
            }
        }
    }

    /// `diag_send`: prüft Sitzung, Vorschau, Ziel und Runden, schickt genau den Text der Vorschau an das Modell und
    /// antwortet mit der geprüften Antwort (siehe [`super::ai`]). Abbruch über `cancel` (Auflegen, `diag_cancel`, neue
    /// Sammlung, Ablauf); dann bleibt die Sitzung, wie sie war.
    pub fn send(&self, cfg: &Config, id: &str, preview: u64, cancel: CancelFlag) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let Some(id) = parse_id(id) else { return unknown() };
        let target = Target::of(cfg);
        let max_rounds = cfg.diagnose.rounds;
        let begun = self.with(|s, now| {
            let seq = s.seq();
            let Some(e) = s.get(id, now) else { return Err(unknown()) };
            let Some(session) = e.session.as_ref() else { return Err(Reply::err("The collection is still running")) };
            if e.op.is_some() {
                return Err(Reply::err(collect_error(CollectError::Busy)));
            }
            if e.preview != preview {
                return Err(Reply::err("The report changed since the preview; check the preview again"));
            }
            match &e.shown {
                Some(shown) if shown.key == target.key && shown.model == target.model => {}
                _ => return Err(Reply::err("AI server changed; check the preview again")),
            }
            if let Some(p) = &target.problem {
                return Err(Reply::err(p.clone()));
            }
            if e.answers >= max_rounds {
                return Err(Reply::field_err(format!("This diagnosis has used all {max_rounds} rounds; collect again to start over"), "diagnose.rounds"));
            }
            if e.sent_preview == Some(preview) {
                return Err(Reply::err("This report was already sent; run checks or change the report first"));
            }
            let report = session.report();
            let sections = report.sections.iter().map(|v| SectionLines { id: v.id.clone(), lines: if v.included { v.lines } else { 0 } }).collect();
            let profile = session.profile();
            let symptom = report.symptom.is_some();
            e.op = Some(Op { seq, cancel: cancel.clone() });
            Ok((seq, report.text, sections, profile, e.answers + 1, symptom))
        });
        let (seq, text, sections, profile, round, symptom) = match begun {
            Ok(b) => b,
            Err(r) => return r,
        };
        let req = DiagRequest { report: &text, sections, profile, round, max_rounds, symptom };
        let result = ask_diagnosis(&cfg.ai, &cfg.diagnose, &req, &cancel);
        let r = self.with(|s, now| {
            let e = s.get(id, now).filter(|e| e.op.as_ref().is_some_and(|o| o.seq == seq));
            let Some(e) = e else { return Err(cancelled()) };
            e.op = None;
            let outcome = match result {
                Ok(o) => o,
                Err(DiagAiError::Ai(crate::pipeline::ai::AiError::Cancelled)) => return Err(cancelled()),
                Err(err) => return Err(Reply::err(err.to_string())),
            };
            e.answers += 1;
            e.sent_preview = Some(preview);
            e.proposed = outcome.diagnosis.checks.iter().map(|c| c.probe.clone()).collect();
            let restore = e.session.as_ref().map(|session| restore_json(session, &outcome.diagnosis));
            Ok((outcome, e.answers, restore))
        });
        match r {
            Ok((o, answers, restore)) => {
                let d = &o.diagnosis;
                let tokens = |t: Option<u64>| t.map_or_else(|| "?".to_string(), |n| n.to_string());
                info!(
                    "diag send: model {}, round {answers}, {} findings, {} checks, dropped {} findings, {} evidence, {} checks ({} invalid, {} target, {} extra), tokens {} in {} out in {} ms",
                    o.model,
                    d.findings.len(),
                    d.checks.len(),
                    d.dropped.findings,
                    d.dropped.evidence,
                    d.dropped.checks(),
                    d.dropped.checks_invalid,
                    d.dropped.checks_target,
                    d.dropped.checks_extra,
                    tokens(o.usage.input),
                    tokens(o.usage.output),
                    o.elapsed.as_millis()
                );
                Reply::data(json!({
                    "id": id.to_string(),
                    "preview": preview,
                    "rounds": answers,
                    "max_rounds": max_rounds,
                    "model": o.model,
                    "answer": d.to_json(),
                    "restore": restore.unwrap_or_else(|| json!({"findings": [], "notes": []})),
                    "dropped": {"findings": d.dropped.findings, "evidence": d.dropped.evidence, "checks": d.dropped.checks()},
                }))
            }
            Err(r) => {
                info!("diag send: {}", if r.extra.get("cancelled") == Some(&json!(true)) { "cancelled" } else { "failed" });
                r
            }
        }
    }

    /// `diag_copy`: der Bericht, wie die Vorschau ihn zeigt (pseudonymisiert und geschwärzt), für CLIPBOARD; der Client
    /// schreibt ihn dorthin (bis W7 `dictate diag copy`, unter niri das Panel aus W4), der Daemon fasst die
    /// Zwischenablage nicht an.
    pub fn copy(&self, cfg: &Config, id: &str) -> Reply {
        if let Some(r) = self.off(cfg) {
            return r;
        }
        let Some(id) = parse_id(id) else { return unknown() };
        let r = self.with(|s, now| {
            let e = s.get(id, now)?;
            let session = e.session.as_ref()?;
            Some((session.outgoing(), e.preview))
        });
        match r {
            Some((text, preview)) => {
                info!("diag copy: {} bytes", text.len());
                Reply::data(json!({"text": text, "bytes": text.len(), "preview": preview}))
            }
            None => unknown(),
        }
    }

    /// `diag_boot_hint`: endete der Boot davor unsauber, und wurde das in diesem Boot noch nicht gemeldet? Antwort
    /// `{"show":bool,"reason":…}`. Ist die Kennung des Kernels (`/proc/sys/kernel/random/boot_id`) schon gemerkt, ist das
    /// die Antwort (`already_checked`, ohne Probe). Sonst wird nur mit der Probe `boots` geprüft (Liste der Boots und Ende
    /// des Journals von Boot −1), unter der Sperre der Sammlungen; der Bericht, die Sitzung und die KI kommen nicht ins Spiel. Der Boot wird in
    /// `diagnose.json` gemerkt (nur die Kennung), sobald das Urteil feststeht. Aus (`enabled` oder `boot_hint`) ist keine
    /// Fehlermeldung, sondern `show: false`, damit die Oberfläche ohne Diagnose still bleibt. Im Log nur `boot hint: shown` bzw.
    /// `boot hint: not needed`.
    pub fn boot_hint(&self, cfg: &Config, cancel: &CancelFlag) -> Reply {
        if !cfg.diagnose.enabled || !cfg.diagnose.boot_hint {
            return Reply::data(json!({"show": false, "reason": "off"}));
        }
        let Some(path) = self.deps.state.clone() else { return Reply::err("Cannot check: no state folder") };
        let ctx = match (self.deps.context)(&cfg.diagnose) {
            Ok(c) => c,
            Err(e) => return Reply::err(format!("Cannot check: {e}")),
        };
        let state = BootHintState::new(path);
        // Neustart der Oberfläche im selben Boot: die Kennung des Kernels genügt, das Journal wird nicht gelesen
        if let Some(kernel) = read_boot_id(&ctx.paths) {
            let _one_at_a_time = self.shared.boot_hint.lock().unwrap_or_else(|e| e.into_inner());
            if already_checked(Some(&kernel), state.load().as_deref()) {
                info!("boot hint: not needed");
                return Reply::data(json!({"show": false, "reason": Why::AlreadyChecked.as_str()}));
            }
        }
        let boots = match check_boots_with(self.deps.lock, &ctx, cancel, COLLECT_BUDGET) {
            Ok(b) => b,
            // gerade läuft eine Sammlung: nichts entschieden, nichts gemerkt
            Err(CollectError::Busy) => return Reply::data(json!({"show": false, "reason": "busy"})),
            Err(CollectError::Cancelled) => return cancelled(),
            Err(e) => return Reply::err(collect_error(e)),
        };
        let decision = {
            let _one_at_a_time = self.shared.boot_hint.lock().unwrap_or_else(|e| e.into_inner());
            let decision = decide(boots.as_ref(), state.load().as_deref());
            if let Some(id) = &decision.remember {
                if state.store(id).is_err() {
                    warn!("boot hint: the checked boot cannot be remembered");
                }
            }
            decision
        };
        match decision.why {
            Why::Unclean => info!("boot hint: shown"),
            Why::Clean | Why::NoPrevious | Why::AlreadyChecked => info!("boot hint: not needed"),
            Why::Unknown => {}
        }
        Reply::data(json!({"show": decision.show, "reason": decision.why.as_str()}))
    }

    /// `diag_cancel`: bricht den laufenden Vorgang der Sitzung ab (idempotent).
    pub fn cancel(&self, id: &str) -> Reply {
        if let Some(id) = parse_id(id) {
            if self.with(|s, now| s.get(id, now).is_some_and(Entry::cancel_op)) {
                info!("diag cancel");
            }
        }
        Reply::data(json!({}))
    }

    /// `diag_clear`: Sitzung löschen (bricht einen laufenden Vorgang ab).
    pub fn clear(&self) -> Reply {
        if self.with(|s, _| s.clear()) {
            info!("diag session cleared");
        }
        Reply::data(json!({}))
    }
}

fn collect_error(e: CollectError) -> String {
    match e {
        CollectError::Busy => "A collection is already running".to_string(),
        CollectError::Cancelled => "Cancelled".to_string(),
        CollectError::TooManySections => format!("The report already has the most sections ({MAX_SECTIONS}); collect again to start over"),
    }
}

fn make_plan(profile: &str, unit: Option<&str>, container: Option<&str>) -> Result<Plan, Reply> {
    let Some(p) = Profile::from_name(profile) else {
        let names: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
        return Err(Reply::err(format!("Unknown profile \"{}\" (known: {})", profile.chars().take(32).collect::<String>(), names.join(", "))));
    };
    let unit = match unit.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(UnitRef::new(u).map_err(|e| Reply::err(format!("unit {e}")))?),
        None => None,
    };
    let container = match container.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Some(ContainerRef::new(c).map_err(|e| Reply::err(format!("container {e}")))?),
        None => None,
    };
    Plan::new(p, &ProfileArgs { unit, container }).map_err(|e| {
        Reply::err(match e {
            super::profile::PlanError::NeedsUnit => "The profile \"unit\" needs a unit (see diag_units)".to_string(),
            e => e.to_string(),
        })
    })
}

fn plan_json(plan: &Plan) -> Value {
    let items: Vec<Value> = plan
        .items()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let (probe, network) = match &item.step {
                Step::Probe(p) => (serde_json::to_value(p).unwrap_or(Value::Null), p.spec().network_traffic),
                Step::ProblemContainers => (Value::Null, false),
            };
            json!({"index": i, "label": item.step.label(), "probe": probe, "selected": item.selected, "network": network})
        })
        .collect();
    let p = plan.profile();
    json!({"profile": p.name(), "label": p.label(), "description": p.description(), "items": items})
}

/// Zwischenzeile des Fortschritts. Parameter und Meldungen sind für das Panel des Users, nie fürs Log.
fn progress_line(id: &str, p: &Progress) -> String {
    let v = match p {
        Progress::Start { index, total, probe, params } => {
            json!({"progress": {"id": id, "phase": "start", "index": index, "total": total, "probe": probe, "params": params}})
        }
        Progress::End { index, total, probe, status, message, bytes, duration } => json!({"progress": {
            "id": id, "phase": "end", "index": index, "total": total, "probe": probe, "status": status,
            "message": message, "bytes": bytes, "ms": duration.as_millis() as u64,
        }}),
    };
    v.to_string()
}

fn counts_json<'a>(pairs: impl Iterator<Item = (&'a str, usize)>) -> Value {
    let mut m = Map::new();
    for (k, n) in pairs {
        let old = m.get(k).and_then(Value::as_u64).unwrap_or(0);
        m.insert(k.to_string(), json!(old + n as u64));
    }
    Value::Object(m)
}

/// Die Vorschau als Antwort (siehe Protokoll in `PLAN-diagnose.md`).
fn preview_json(e: &Entry, target: &Target, cfg: &Config, ttl: Duration, now: Instant) -> Value {
    let session = e.session.as_ref().expect("Vorschau nur mit Bericht");
    let report = session.report();
    let (mut redactions, mut placeholders) = (0usize, 0usize);
    let sections: Vec<Value> = report
        .sections
        .iter()
        .map(|s| {
            redactions += s.redactions.iter().map(|r| r.count).sum::<usize>();
            placeholders += s.pseudonyms.iter().map(|(_, n)| n).sum::<usize>();
            json!({
                "id": s.id, "probe": s.probe, "params": s.params, "status": s.status, "included": s.included,
                "lines": s.lines, "bytes": s.bytes, "total_lines": s.total_lines, "cut": s.cut,
                "redactions": counts_json(s.redactions.iter().map(|r| (r.kind, r.count))),
                "placeholders": counts_json(s.pseudonyms.iter().map(|(c, n)| (c.prefix(), *n))),
                "header": s.header,
                "round": s.round,
            })
        })
        .collect();
    let expires = (e.created + ttl).saturating_duration_since(now).as_secs();
    // das Symptom steht im Kopf des Textes; hier nur noch einmal für die Markierung im Panel, mit seinen Zählungen
    let symptom = report.symptom.as_ref().map(|v| {
        redactions += v.redactions.iter().map(|r| r.count).sum::<usize>();
        placeholders += v.pseudonyms.iter().map(|(_, n)| n).sum::<usize>();
        json!({
            "text": v.text,
            "redactions": counts_json(v.redactions.iter().map(|r| (r.kind, r.count))),
            "placeholders": counts_json(v.pseudonyms.iter().map(|(c, n)| (c.prefix(), *n))),
        })
    });
    json!({
        "id": e.id.to_string(),
        "preview": e.preview,
        "profile": session.profile().map(Profile::name),
        "rounds": session.rounds(),
        "max_rounds": cfg.diagnose.rounds,
        "text": report.text,
        "bytes": report.text.len(),
        "budget": report.budget,
        "sections": sections,
        "symptom": symptom,
        "max_symptom_chars": super::report::MAX_SYMPTOM_CHARS,
        "redactions": redactions,
        "placeholders": placeholders,
        // der Schalter der Sitzung, nicht der jetzigen Config: er galt beim Sammeln (Review L4)
        "pseudonymize": session.pseudonymize(),
        "target": target.json(),
        "expires_in_s": expires,
    })
}

/// Spannen der Rückübersetzung zur Antwort ([`super::restore`]): je Befund die der Ursache, dazu die der Notizen. Nie
/// für Prüfbefehle. Leer, wenn die Sitzung nicht pseudonymisiert.
fn restore_json(session: &Session, d: &super::ai::Diagnosis) -> Value {
    use super::restore::spans_json;
    let findings: Vec<Value> = d.findings.iter().map(|f| spans_json(&session.restore_spans(&f.cause, false))).collect();
    json!({"findings": findings, "notes": spans_json(&session.restore_spans(&d.notes, true))})
}

/// Logzeile nach Sammeln oder Prüfen: nur Zahlen.
fn log_report(what: &str, profile: Option<Profile>, v: &Value, elapsed: Duration) {
    let sections = v["sections"].as_array().map_or(0, Vec::len);
    info!(
        "diag {what}{}: round {}, {sections} sections, report {} bytes, {} redactions, {} placeholders in {} ms",
        profile.map(|p| format!(" {}", p.name())).unwrap_or_default(),
        v["rounds"],
        v["bytes"],
        v["redactions"],
        v["placeholders"],
        elapsed.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_limits_match_the_config() {
        // config.rs steht auch in Clients ohne `crate::diagnose`: dieselben Zahlen an beiden Stellen
        use crate::config::{DIAGNOSE_DEFAULT_REPORT_KIB, DIAGNOSE_REPORT_KIB_RANGE};
        assert_eq!(
            (ReportBudget::MIN_KIB, ReportBudget::MAX_KIB, ReportBudget::DEFAULT_KIB),
            (DIAGNOSE_REPORT_KIB_RANGE.0, DIAGNOSE_REPORT_KIB_RANGE.1, DIAGNOSE_DEFAULT_REPORT_KIB)
        );
        assert_eq!(super::super::probes::DEFAULT_TEMP_LOG_DIR, crate::config::DIAGNOSE_DEFAULT_TEMP_LOG_DIR);
    }

    #[test]
    fn ping_target_names_follow_the_rule_of_the_probe() {
        // Ein Name, den die Config annimmt, muss als Suchbegriff der Probe gelten (sonst ließe er sich nie pingen), und umgekehrt
        use crate::config::PingTargetEntry;
        let long = "a".repeat(64);
        let too_long = "a".repeat(65);
        for name in ["gateway", "nas", "1.1.1.1", "v6:host", "a_b-c.d", long.as_str(), too_long.as_str(), "", "-x", ".x", "a b", "a/b", "ä", "a@b"] {
            assert_eq!(PingTargetEntry::check_name(name).is_ok(), super::super::PingTarget::new(name).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn collect_reply_timeout_of_the_client_covers_the_budget() {
        assert!(crate::protocol::DIAG_COLLECT_REPLY_TIMEOUT >= COLLECT_BUDGET + REPLACE_WAIT + Duration::from_secs(5));
    }

    fn cfg() -> Config {
        let mut c = Config::default();
        c.diagnose.enabled = true;
        c
    }

    #[test]
    fn plans_and_errors_without_probes() {
        let (d, _sweeper) = Diagnose::start(DiagDeps::unavailable()).unwrap();
        let c = cfg();
        let r = d.plan(&c, None, None, None);
        assert!(r.ok && r.extra["profiles"].as_array().unwrap().len() == 6, "{r:?}");
        let r = d.plan(&c, Some("docker"), None, Some("dataroom"));
        let items = r.extra["items"].as_array().unwrap();
        assert_eq!(items[2]["probe"]["probe"], "docker_state");
        assert_eq!(items[2]["probe"]["container"], "dataroom");
        assert_eq!(d.plan(&c, Some("unit"), None, None).error.unwrap(), "The profile \"unit\" needs a unit (see diag_units)");
        assert!(d.plan(&c, Some("bogus"), None, None).error.unwrap().starts_with("Unknown profile \"bogus\""));
        assert!(d.plan(&c, Some("unit"), Some("-x"), None).error.unwrap().starts_with("unit "));
        // ohne Kontext: Fehler, und keine Sitzung bleibt
        let mut sink = |_: &str| Ok(());
        let r = d.collect(&c, &CollectRequest { profile: "now".into(), ..Default::default() }, Arc::new(AtomicBool::new(false)), &mut sink);
        assert_eq!(r.error.unwrap(), "Cannot collect: diagnosis is not available in this daemon");
        let r = d.collect(
            &c,
            &CollectRequest { profile: "now".into(), skip: vec![0, 1, 2, 3, 4, 5, 6], ..Default::default() },
            Arc::new(AtomicBool::new(false)),
            &mut sink,
        );
        assert_eq!(r.error.unwrap(), "Nothing selected to collect");
        let r = d.collect(&c, &CollectRequest { profile: "now".into(), skip: vec![7], ..Default::default() }, Arc::new(AtomicBool::new(false)), &mut sink);
        assert_eq!(r.error.unwrap(), "The plan of \"now\" has no item 7");
        assert_eq!(d.copy(&c, "1").error.unwrap(), UNKNOWN_SESSION);
        assert_eq!(d.preview(&c, "x", None, None).error.unwrap(), UNKNOWN_SESSION);
        // aus: alles abgewiesen
        let off = Config::default();
        assert_eq!(d.plan(&off, None, None, None).error.unwrap(), DISABLED);
        assert_eq!(d.send(&off, "1", 1, Arc::new(AtomicBool::new(false))).error.unwrap(), DISABLED);
        assert!(d.cancel("1").ok && d.clear().ok);
    }

    #[test]
    fn the_report_cannot_change_while_it_is_sent_or_checked() {
        // Review L6: Abwahl während eines Vorgangs (Senden, Prüfen) abgewiesen, eine Vorschau ohne Änderung erlaubt
        let (d, _sweeper) = Diagnose::start(DiagDeps::unavailable()).unwrap();
        let c = cfg();
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        d.with(|s, now| {
            s.next_id = 4;
            s.entry = Some(Entry {
                id: 4,
                created: now,
                session: Some(Session::new(Identity::default(), true, ReportBudget::from_kib(48).unwrap())),
                op: Some(Op { seq: 1, cancel: cancel.clone() }),
                preview: 1,
                shown: Some(Target::of(&c)),
                answers: 0,
                sent_preview: None,
                proposed: Vec::new(),
            });
        });
        let r = d.preview(&c, "4", Some(&[]), None);
        assert_eq!(r.error.as_deref(), Some(BUSY_REPORT), "{r:?}");
        let r = d.preview(&c, "4", None, None);
        assert!(r.ok, "{r:?}");
        assert_eq!(r.extra["preview"], 1, "nichts geändert");
        // Review L4: die Vorschau meldet den Schalter der Sitzung, nicht den der jetzigen Config
        let mut off = cfg();
        off.diagnose.pseudonymize = false;
        assert_eq!(d.preview(&off, "4", None, None).extra["pseudonymize"], true);
        // nach dem Vorgang geht die Abwahl wieder
        d.with(|s, _| s.entry.as_mut().unwrap().op = None);
        assert!(d.preview(&c, "4", Some(&[]), None).ok);
    }
}
