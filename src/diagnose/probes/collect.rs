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

//! Ausführung einer Probe: Kontext, Frist und Budget, Befehle über den Ausführer ([`crate::diagnose::run`]), Auflösung
//! gegen die echten Listen und die Ausgabe ([`ProbeOutput`]).
//!
//! [`Collector`] ist der Einstieg für die folgenden Aufgaben (Bericht, Daemon): er hält den Kontext, den Abbruch und den
//! Zwischenspeicher der Auflösung (Unit-, Container- und Boot-Listen werden je Sammlung einmal geholt) und führt
//! [`Probe`]n nacheinander aus. [`Collector::collect`] liefert immer eine [`ProbeOutput`], nie einen Fehler: was schiefging,
//! steht im [`Status`] und im Text, den das Modell lesen soll.
//!
//! **Grenzen.** Der Katalog nennt je Probe eine Grenze für die *Ausgabe* ([`ProbeSpec::limit_bytes`], gilt für die Summe
//! aller Teile) und eine Frist für die ganze Probe. Befehle mit mehreren Teilen bekommen den Rest der Grenze zu gleichen
//! Teilen (der nächste Befehl erbt, was der vorige nicht brauchte). Programme, deren JSON in Rust zu einer kürzeren
//! Ausgabe verdichtet wird, dürfen mehr lesen als die Grenze ([`RAW_LIMIT`]): sonst wäre ein bei der Grenze
//! abgeschnittenes JSON nicht mehr lesbar. Nach dem Verdichten gilt die Grenze des Katalogs; eine letzte Prüfung in
//! [`Run::finish`] stellt sie für jede Probe sicher.

use super::fs::Fs;
use super::resolve::Cache;
use super::text::{cut_to_bytes, human_bytes};
use super::time::Clock;
use super::{output::*, ProbePaths};
use crate::diagnose::argv::{Argv, Program};
use crate::diagnose::catalog::ProbeSpec;
use crate::diagnose::probe::Probe;
use crate::diagnose::run::{self, Exit, Keep, ProbeEnv, Reason, RunLimits};
use crate::pipeline::ai::CancelFlag;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Wie viel ein Programm, dessen JSON Rust verdichtet, höchstens liefern darf (Bytes).
pub const RAW_LIMIT: usize = 256 * 1024;
/// Wie viel stderr eines Befehls behalten wird (Bytes): Fehlermeldungen, keine Daten.
pub const STDERR_LIMIT: usize = 4 * 1024;
/// Wie viele Zeilen stderr in einen Hinweis kommen.
const STDERR_LINES: usize = 3;

/// Ein Ziel aus der Config-Liste für `net_reach` (`diagnose.ping_targets`): Name für das Modell und Adresse zum Pingen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingTargetCfg {
    pub name: String,
    pub address: String,
}

/// Alles, was die Proben von außen brauchen. In Produktion [`ProbeContext::system`], in Tests nur über die Fixtures
/// (`testkit`), nie gegen das echte System.
#[derive(Debug, Clone)]
pub struct ProbeContext {
    pub env: ProbeEnv,
    pub paths: ProbePaths,
    pub clock: Clock,
    /// Erlaubte Ziele von `net_reach`; leer = keines.
    pub ping_targets: Vec<PingTargetCfg>,
    /// Abstand der zwei Messungen von `load` und `top_procs`.
    pub sample_interval: Duration,
}

/// Abstand der zwei Messungen in Produktion.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

impl ProbeContext {
    /// Produktion: Programme unter `/usr/bin`, Orte unter `/`, Systemuhr. Nicht in Tests (dort sind nur Fixtures
    /// möglich, damit keine Probe das echte System anfasst).
    #[cfg(not(test))]
    pub fn system(temp_log_dir: Option<std::path::PathBuf>, ping_targets: Vec<PingTargetCfg>) -> Result<ProbeContext, String> {
        Ok(ProbeContext {
            env: ProbeEnv::system(),
            paths: ProbePaths::system(temp_log_dir)?,
            clock: Clock::system(),
            ping_targets,
            sample_interval: SAMPLE_INTERVAL,
        })
    }
}

/// Optionen eines Laufs.
#[derive(Debug, Clone, Copy)]
pub struct CollectOptions {
    /// Verbleibendes Gesamtbudget der Sammlung; die Frist der Probe wird darauf gekürzt.
    pub budget: Duration,
    /// Der User hat Netzverkehr bestätigt (nur `net_reach`).
    pub network_confirmed: bool,
}

impl CollectOptions {
    pub fn new(budget: Duration) -> CollectOptions {
        CollectOptions { budget, network_confirmed: false }
    }

    pub fn confirmed(self) -> CollectOptions {
        CollectOptions { network_confirmed: true, ..self }
    }
}

/// Führt Proben nacheinander aus. Eine Sammlung = ein `Collector` (der Zwischenspeicher der Auflösung gilt für sie).
pub struct Collector<'a> {
    ctx: &'a ProbeContext,
    cancel: &'a CancelFlag,
    cache: Cache,
}

impl<'a> Collector<'a> {
    pub fn new(ctx: &'a ProbeContext, cancel: &'a CancelFlag) -> Collector<'a> {
        Collector { ctx, cancel, cache: Cache::default() }
    }

    /// Führt `probe` aus. Die Frist ist die des Katalogs, gekürzt auf `opts.budget`.
    pub fn collect(&mut self, probe: &Probe, opts: CollectOptions) -> ProbeOutput {
        let spec = probe.spec();
        let start = Instant::now();
        if spec.network_traffic && !opts.network_confirmed {
            return ProbeOutput::without_data(spec.name, Status::NeedsConfirmation);
        }
        if self.cancel.load(Ordering::SeqCst) {
            return ProbeOutput::without_data(spec.name, Status::Cancelled);
        }
        let allowed = spec.deadline.min(opts.budget);
        if allowed.is_zero() {
            return ProbeOutput::without_data(spec.name, Status::TimedOut);
        }
        let mut run = Run {
            ctx: self.ctx,
            cache: &mut self.cache,
            spec,
            probe,
            cancel: self.cancel,
            start,
            deadline: start + allowed,
            out: ProbeOutput::new(spec.name),
            fs: Fs::for_spec(&self.ctx.paths, spec),
        };
        let result = super::dispatch(&mut run);
        run.finish(result)
    }
}

/// Der Zustand einer laufenden Probe.
pub struct Run<'a> {
    pub ctx: &'a ProbeContext,
    pub(super) cache: &'a mut Cache,
    pub spec: &'static ProbeSpec,
    pub probe: &'a Probe,
    pub cancel: &'a CancelFlag,
    pub start: Instant,
    pub deadline: Instant,
    pub out: ProbeOutput,
    pub fs: Fs,
}

/// Ergebnis einer Probe-Funktion: Ende mit diesem Status (die Teile in `out` bleiben erhalten).
pub type ProbeResult = Result<(), Status>;

impl Run<'_> {
    /// Abbruch oder abgelaufene Frist?
    pub fn stop(&self) -> Option<Status> {
        if self.cancel.load(Ordering::SeqCst) {
            Some(Status::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(Status::TimedOut)
        } else {
            None
        }
    }

    /// Prüft Abbruch und Frist für `?`.
    pub fn check(&self) -> ProbeResult {
        match self.stop() {
            Some(s) => Err(s),
            None => Ok(()),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Schläft `d`, bis zu Abbruch oder Frist (in kleinen Schritten, ein Abbruch wirkt nach höchstens 10 ms).
    pub fn sleep(&self, d: Duration) -> ProbeResult {
        let until = Instant::now() + d;
        loop {
            self.check()?;
            let now = Instant::now();
            if now >= until {
                return Ok(());
            }
            std::thread::sleep((until - now).min(Duration::from_millis(10)));
        }
    }

    /// Wie viele Bytes der Ausgabe `index` von `count` Teilen bekommt, wenn bisher `used` Bytes belegt sind.
    pub fn share(&self, used: usize, index: usize, count: usize) -> usize {
        self.spec.limit_bytes.saturating_sub(used) / count.saturating_sub(index).max(1)
    }

    /// Startet `argv` mit Frist und Grenzen. `cap` ist die Grenze für stdout, `keep` bestimmt, ob bei einem Überlauf der
    /// Anfang oder das Ende bleibt. Läuft schon nichts mehr (Abbruch, Frist), wird nichts gestartet.
    pub fn exec(&mut self, argv: &Argv, cap: usize, keep: Keep) -> Cmd {
        exec(self.ctx, argv, self.deadline, cap, cap.min(STDERR_LIMIT), keep, self.cancel, &mut self.out)
    }

    /// Wie [`Run::exec`], aber auch stderr bis `cap` (für `docker logs`: der stderr des Containers ist Teil des Logs).
    pub fn exec_both(&mut self, argv: &Argv, cap: usize, keep: Keep) -> Cmd {
        exec(self.ctx, argv, self.deadline, cap, cap, keep, self.cancel, &mut self.out)
    }

    /// Beendet die Probe: setzt den Status, prüft die Grenze des Katalogs und die Dauer.
    pub fn finish(mut self, result: ProbeResult) -> ProbeOutput {
        match result {
            Ok(()) => {
                if self.out.status == Status::Ok && self.out.parts.iter().all(|p| p.text.trim().is_empty()) {
                    self.out.status = Status::Empty;
                }
            }
            Err(status) => self.out.status = status,
        }
        // Namen und Beschreibungen aus Listen sind fremder Text: nie Steuerzeichen im Bericht
        for part in &mut self.out.parts {
            part.text = super::text::clean(part.text.as_bytes());
        }
        enforce_limit(&mut self.out, self.spec.limit_bytes);
        self.out.duration = self.start.elapsed();
        self.out
    }
}

/// Stellt sicher, dass der Berichtstext die Grenze nicht überschreitet: zu große Teile werden gekürzt (das Ende bleibt
/// bei Journalen nicht eigens erkannt; hier genügt der Anfang, die Probe selbst hat vorher schon gekürzt).
fn enforce_limit(out: &mut ProbeOutput, limit: usize) {
    for _ in 0..8 {
        let total = out.text().len();
        if total <= limit || out.parts.is_empty() {
            return;
        }
        let excess = total - limit;
        let (i, _) = out.parts.iter().enumerate().max_by_key(|(_, p)| p.text.len()).expect("not empty");
        let part = &mut out.parts[i];
        let target = part.text.len().saturating_sub(excess + 64);
        let cut = cut_to_bytes(&part.text, target.max(64), Keep::Tail);
        part.text = cut.text;
        if !out.notes.iter().any(|n| n.starts_with("limit:")) {
            out.notes.push(format!("limit: the output was cut to the {} limit of this probe", human_bytes(limit as u64)));
        }
    }
}

/// Ergebnis eines Befehls.
#[derive(Debug, Clone)]
pub struct Cmd {
    pub program: Program,
    pub reason: Reason,
    pub exit: Option<Exit>,
    /// Die behaltenen Bytes von stdout (nicht bereinigt: JSON bleibt, wie es ist).
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// stdout wurde an der Grenze abgeschnitten (bei [`Keep::Head`] fehlt das Ende, bei [`Keep::Tail`] der Anfang).
    pub truncated: bool,
    /// Bytes, die der Befehl auf stdout geliefert hat.
    pub seen: u64,
    /// Welches Ende bei einem Überlauf blieb.
    pub keep: Keep,
    /// Der Ausführer hat die Pipe vorzeitig geschlossen ([`crate::diagnose::run::Captured::closed_early`]): das Programm
    /// hat mehr geliefert, als gelesen wurde, und bei [`Keep::Tail`] ist das Behaltene nicht sicher das echte Ende.
    pub closed_early: bool,
    /// Hinweis des Ausführers zur Kürzung ([`crate::diagnose::run::Captured::note`]), wenn gekürzt wurde.
    pub capture_note: Option<String>,
}

impl Cmd {
    fn not_run(program: Program, reason: Reason) -> Cmd {
        Cmd {
            program,
            reason,
            exit: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            truncated: false,
            seen: 0,
            keep: Keep::Head,
            closed_early: false,
            capture_note: None,
        }
    }

    /// stdout als bereinigter Text (siehe [`super::text::clean`]). Wurde stdout abgeschnitten, fehlt die angeschnittene
    /// Zeile (bei [`Keep::Tail`] die erste, bei [`Keep::Head`] die letzte): ein Wert ohne seinen Schlüssel oder ein halbes
    /// Token wäre für den Filter nicht mehr zu erkennen (Security-Review N1).
    pub fn text(&self) -> String {
        let bytes = match self.keep {
            Keep::Tail => super::text::without_partial_first_line(&self.stdout, self.truncated),
            Keep::Head => super::text::without_partial_last_line(&self.stdout, self.truncated),
        };
        super::text::clean(bytes)
    }

    /// Hinweis für den Bericht, wenn stdout an der Grenze des Ausführers gekürzt wurde. `what` sagt, was fehlt (`"older
    /// lines are missing"`). Hat der Ausführer die Pipe vorzeitig geschlossen (`closed_early`), gilt sein genauer Hinweis
    /// („the output went on, so this is not its end“, bzw. „at least N bytes“): nach dem Schließen ist das Behaltene bei
    /// [`Keep::Tail`] das Ende des *Gelesenen*, und das Modell soll es nicht für das Ende des Journals halten.
    pub fn capture_cut_note(&self, limit_text: &str, what: &str) -> Option<String> {
        if !self.truncated {
            return None;
        }
        match (&self.capture_note, self.closed_early) {
            (Some(note), true) => Some(note.clone()),
            _ => Some(format!("the output was cut at the capture limit of {limit_text}: {what}")),
        }
    }

    /// stdout als UTF-8 ohne Bereinigung (für JSON).
    pub fn raw_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Die ersten Zeilen von stderr, bereinigt und gekürzt (für Hinweise).
    pub fn stderr_summary(&self) -> Option<String> {
        let text = super::text::clean(&self.stderr);
        let mut seen: Vec<&str> = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if !seen.contains(&line) {
                seen.push(line);
            }
        }
        if seen.is_empty() {
            return None;
        }
        let more = seen.len().saturating_sub(STDERR_LINES);
        let mut s = seen.iter().take(STDERR_LINES).map(|l| &l[..super::text::safe_head(l, 200)]).collect::<Vec<_>>().join("; ");
        if more > 0 {
            s.push_str(&format!(" (+{more} more lines)"));
        }
        Some(s)
    }

    /// Hat der Befehl keine brauchbaren Daten geliefert? Dann der Status dafür: nicht gestartet (fehlendes Programm ist
    /// `Unavailable`), Frist, Abbruch, Absturz, oder Fehler-Exit ohne Ausgabe. Ein Exit-Code ≠ 0 *mit* Ausgabe ist
    /// brauchbar (`df` an einem kaputten Mount, `systemctl status` einer inaktiven Unit, `pacman -Q` mit fehlendem
    /// Paket); [`Cmd::exit_note`] sagt es dann.
    pub fn problem(&self) -> Option<Status> {
        let name = self.program.name();
        match &self.reason {
            Reason::Timeout => return Some(Status::TimedOut),
            Reason::Cancelled => return Some(Status::Cancelled),
            Reason::SpawnError { kind: std::io::ErrorKind::NotFound, .. } => return Some(Status::Unavailable(format!("{name} is not installed"))),
            Reason::SpawnError { message, .. } => return Some(Status::Failed(format!("could not start {name}: {message}"))),
            Reason::Ok => {}
        }
        if self.stdout.is_empty() {
            let why = match self.exit {
                Some(Exit::Code(0)) | None => return None,
                Some(Exit::Code(c)) => format!("{name} exited with code {c}"),
                Some(Exit::Signal(s)) => format!("{name} was killed by signal {s}"),
            };
            return Some(Status::Failed(match self.stderr_summary() {
                Some(e) => format!("{why}: {e}"),
                None => why,
            }));
        }
        None
    }

    /// Hinweis zu einem Exit-Code ≠ 0 bei vorhandener Ausgabe, mit der Fehlermeldung des Programms.
    pub fn exit_note(&self) -> Option<String> {
        let name = self.program.name();
        let what = match self.exit {
            Some(Exit::Code(0)) | None => None,
            Some(Exit::Code(c)) => Some(format!("{name} exited with code {c}")),
            Some(Exit::Signal(s)) => Some(format!("{name} was killed by signal {s}")),
        };
        match (what, self.stderr_summary()) {
            (Some(w), Some(e)) => Some(format!("{w}: {e}")),
            (Some(w), None) => Some(w),
            (None, Some(e)) => Some(format!("{name} reported: {e}")),
            (None, None) => None,
        }
    }
}

/// Startet einen Befehl (auch für die Auflösung, die kein [`Run`] hat).
#[allow(clippy::too_many_arguments)]
pub(super) fn exec(
    ctx: &ProbeContext,
    argv: &Argv,
    deadline: Instant,
    cap: usize,
    stderr_cap: usize,
    keep: Keep,
    cancel: &CancelFlag,
    out: &mut ProbeOutput,
) -> Cmd {
    if cancel.load(Ordering::SeqCst) {
        return Cmd::not_run(argv.program, Reason::Cancelled);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Cmd::not_run(argv.program, Reason::Timeout);
    }
    let limits = RunLimits { deadline: remaining, stdout_max: cap, stderr_max: stderr_cap, keep };
    let result = run_retrying(ctx, argv, &limits, cancel);
    out.raw_bytes += result.stdout.seen;
    Cmd {
        program: argv.program,
        reason: result.reason,
        exit: result.exit,
        truncated: result.stdout.truncated,
        seen: result.stdout.seen,
        keep,
        closed_early: result.stdout.closed_early,
        capture_note: result.stdout.note(),
        stdout: result.stdout.bytes,
        stderr: result.stderr.bytes,
    }
}

/// In Tests wiederholt bei `ETXTBSY`: ein paralleler Test kann zwischen `fork` und `exec` den Schreib-Deskriptor eines
/// eben geschriebenen Fake-Skripts geerbt haben. In Produktion ein einziger Lauf.
#[cfg(test)]
fn run_retrying(ctx: &ProbeContext, argv: &Argv, limits: &RunLimits, cancel: &CancelFlag) -> run::RunResult {
    for _ in 0..200 {
        let r = run::run(&ctx.env, argv, limits, cancel);
        match &r.reason {
            Reason::SpawnError { kind: std::io::ErrorKind::ExecutableFileBusy, .. } => std::thread::sleep(Duration::from_millis(10)),
            _ => return r,
        }
    }
    panic!("fake program stayed busy");
}

#[cfg(not(test))]
fn run_retrying(ctx: &ProbeContext, argv: &Argv, limits: &RunLimits, cancel: &CancelFlag) -> run::RunResult {
    run::run(&ctx.env, argv, limits, cancel)
}
