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

//! Ausführer der Proben (Aufgabe 2 in `PLAN-diagnose.md`): ein [`Argv`] aus dem Katalog starten, mit Frist, Grenzen,
//! geleerter Umgebung und Drossel, und das Ergebnis samt Grund liefern.
//!
//! **Start.** Nur per argv über `std::process::Command`, nie `sh -c`. Das Programm ist immer der absolute Pfad aus
//! [`Program`]; die [`ProbeEnv`] darf nur das *Verzeichnis* ersetzen (Tests mit Fake-Programmen in einem Temp-Ordner,
//! in Produktion `/usr/bin`), nie den Dateinamen. Umgebung geleert bis auf `PATH=/usr/bin`, `LC_ALL=C.UTF-8`,
//! `SYSTEMD_PAGER=`, `SYSTEMD_COLORS=0`, `NO_COLOR=1`, `HOME` und `XDG_RUNTIME_DIR` (für `systemctl --user`, nur wenn
//! absolut). stdin ist `/dev/null`, das Arbeitsverzeichnis `/` (das Kind hält kein Verzeichnis des Daemons fest),
//! stdout und stderr sind getrennte Pipes. Das Kind ist Leiter einer eigenen Prozessgruppe (`process_group(0)`).
//! Alle geerbten Deskriptoren ab 3 bekommen im Kind vor `exec` `FD_CLOEXEC` (`close_range(3, ~0, CLOSE_RANGE_CLOEXEC)`,
//! ohne den Syscall eine Schleife mit `fcntl`): C-Bibliotheken des Daemons (CUDA, Pulse, X) öffnen nicht alles mit
//! `O_CLOEXEC`, und ein geerbtes Schreibende einer Daemon-Pipe hielte dort das EOF auf. Mit dem Flag statt Schließen
//! bleibt die Fehler-Pipe von `std` bis zum `exec` gültig.
//!
//! **Docker** (nur für `/usr/bin/docker`, [`DOCKER_ENV`]): `DOCKER_HOST=unix:///var/run/docker.sock` (hat Vorrang vor
//! `DOCKER_CONTEXT` und dem `currentContext` der Config, geprüft mit Docker-CLI 29.9: `docker context show` liefert
//! `default`), `DOCKER_CONFIG` auf ein eigenes, leeres, nur lesbares Verzeichnis (`$XDG_RUNTIME_DIR/dictate/
//! diag-docker-config`, Modus 0500, vor jedem Start geprüft: kein Symlink, gehört uns, leer; sonst startet die Probe
//! nicht), `DOCKER_CLI_HOOKS=false` und `DOCKER_CLI_HINTS=false`. So liest die CLI weder `~/.docker/config.json` noch
//! Kontexte, Zugangsdaten, Plugin-Hooks oder `~/.docker/cli-plugins`: kein fremder Docker-Host (kein `ssh://`/`tcp://`,
//! kein Netz an `net_reach` vorbei), keine Programme außerhalb des Katalogs, nur der lokale Unix-Socket. Ohne
//! `XDG_RUNTIME_DIR` starten Docker-Proben nicht.
//!
//! **Drossel gegen Hitze** im Kind vor `exec` (`pre_exec`, nur async-signal-sichere Syscalls, keine Wrapper-Programme):
//! `setpriority` auf nice [`NICE`] (ist der Daemon schon netter, bleibt sein Wert; senken dürfte ein Benutzer ihn nicht)
//! und `ioprio_set` auf `IOPRIO_CLASS_IDLE`. Scheitert eines davon, startet die Probe nicht ([`Reason::SpawnError`]):
//! eine ungedrosselte Probe ist keine.
//!
//! **Frist.** Je Probe aus dem Katalog ([`RunLimits::for_spec`]), höchstens [`MAX_DEADLINE`], und mit
//! [`RunLimits::within`] auf eine verbleibende Frist des Aufrufers verkürzbar (Gesamtbudget der Sammlung, Aufgabe 5).
//! Danach SIGTERM an die Gruppe, nach [`TERM_GRACE`] SIGKILL an die Gruppe. Ein Abbruch über [`CancelFlag`] wird
//! spätestens nach [`TICK`] bemerkt und schickt SIGTERM, nach [`CANCEL_GRACE`] SIGKILL: das Ergebnis kommt so auch bei
//! einem Programm, das SIGTERM ignoriert, in unter 200 ms. Länger als die Frist braucht [`run`] höchstens
//! [`MAX_OVERRUN`].
//!
//! **Grenzen beim Lesen.** Beide Pipes sind nicht blockierend und werden mit `poll(2)` zugleich geleert, so blockiert
//! das Kind nie an einer vollen Pipe, auf die gerade niemand wartet. Jede hat eine eigene Byte-Grenze; was darüber
//! hinausgeht, wird gelesen und verworfen ([`Captured::truncated`], [`Captured::seen`]), nicht gesammelt. Mit
//! [`Keep::Head`] bleibt der Anfang; nach [`DISCARD_MAX`] verworfenen Bytes schließt der Ausführer seine Seite der
//! Pipe ([`Captured::closed_early`]), das Kind bekommt dann SIGPIPE bzw. `EPIPE` (Rust setzt SIGPIPE im Kind auf die
//! Vorgabe zurück) und endet, statt bis zur Frist Ausgabe zu erzeugen, die keiner liest. Mit [`Keep::Tail`] bleibt das
//! Ende (Journale: das Ende ist wichtiger); dafür liest der Ausführer bis zum Ende der Pipe bzw. bis zur Frist, weil
//! erst das letzte Byte zeigt, was das Ende ist, höchstens aber [`TAIL_READ_MAX`] Bytes je Pipe: dann schließt er sie
//! ebenso ([`Captured::closed_early`]), und das Behaltene ist das Ende des Gelesenen, nicht sicher das echte Ende
//! ([`Captured::note`] sagt das). Je Durchgang der Hauptschleife liest der Ausführer höchstens [`READS_PER_TICK`] ×
//! [`READ_CHUNK`] Bytes je Pipe und prüft dann Ende, Frist und Abbruch: ein Kind, das schneller schreibt, als gelesen
//! wird (die Pipe meldet nie `EAGAIN`), hält ihn so nicht in der Leseschleife fest.
//!
//! **Ende und Aufräumen.** Ob der Leiter geendet hat, fragt `waitid(…, WNOWAIT)` ab, ohne ihn abzuholen (zusätzlich
//! weckt ein pidfd das `poll`). Solange der Leiter nicht abgeholt ist, gehört die Gruppen-ID sicher uns (die Nummer
//! kann nicht neu vergeben werden): erst dann geht SIGKILL an die ganze Gruppe, damit keine Kinder der Probe
//! zurückbleiben, und erst danach holt `waitpid` den Leiter ab. Danach liest der Ausführer die Pipes höchstens
//! [`PIPE_LINGER`] zu Ende und schließt sie dann, auch wenn noch jemand die Schreibseite offen hält.
//!
//! **Annahme: niemand sonst im Prozess holt Kinder ab.** Die Gruppensignale sind nur sicher, solange der Leiter nicht
//! abgeholt ist, und das hängt daran, dass kein anderer Code im Daemon Kinder abholt: kein `waitpid(-1, …)`/`wait()`,
//! und SIGCHLD weder auf `SIG_IGN` noch mit `SA_NOCLDWAIT` (dann holt der Kernel den Leiter selbst ab, die Gruppen-ID
//! wäre zwischen `waitid` und `kill(-pid)` frei und könnte eine fremde Gruppe desselben Users treffen). Geprüft am
//! 05.11.2026: im Repo gibt es weder das eine noch das andere (`signal_hook` nur für SIGTERM/SIGINT im Daemon). Der
//! zweite Teil wird vor jedem Start zur Laufzeit geprüft (`sigaction(SIGCHLD, NULL, …)`): ist SIGCHLD so gesetzt,
//! startet keine Probe ([`Reason::SpawnError`]). Ein fremdes `waitpid(-1)` lässt sich nicht erkennen; wer eines
//! einbaut, muss dieses Modul anpassen.
//!
//! **Was garantiert ist und was nicht** (Kinder, die die Gruppe verlassen):
//! - Garantiert: jeder Prozess, der beim Ende des Leiters (oder bei Frist/Abbruch) noch in der Gruppe ist, bekommt
//!   SIGKILL; der Ausführer hängt nie an Pipes, die ein entkommener Enkel offen hält ([`PIPE_LINGER`]), und nie länger
//!   als [`MAX_OVERRUN`] über die Frist.
//! - Nicht garantiert: ein Nachfahre, der mit `setsid`/`setpgid` eine eigene Gruppe gründet, wird nicht beendet; er
//!   läuft weiter und bekommt beim Schreiben `EPIPE`. Sicher fassen ließe sich das nur mit einer eigenen cgroup je
//!   Probe (`systemd-run --user --scope` bzw. cgroup v2 mit `cgroup.kill`); die Programme des Katalogs sind lesende
//!   Werkzeuge, die sich nicht ablösen, darum steht das als offener Punkt im Plan und nicht hier.
//!
//! **Prozess im D-Zustand.** Ein `df` oder `ls` an einem toten CIFS-Mount kann im Kernel hängen und überlebt SIGKILL,
//! bis der Mount aufgibt. Nach SIGKILL wartet der Ausführer darum nur [`REAP_WAIT`] und liefert dann das Ergebnis
//! (`exit: None`, `reaped: false`). Den Prozess übernimmt *ein* gemeinsamer Abhol-Thread (lazy gestartet), der alle
//! übergebenen PIDs mit `waitpid(…, WNOHANG)` alle [`REAPER_POLL`] abfragt, solange welche offen sind, und sonst
//! blockierend auf neue wartet. Begründung: ein Thread je hängendem Prozess häufte Threads an, ein blockierendes
//! `waitpid` im Ausführer hielte die Sammlung auf, und gar nicht abholen ließe Zombies zurück. So gibt es höchstens
//! einen Thread und keinen Zombie, der länger als [`REAPER_POLL`] nach dem echten Ende bleibt; die Zahl der noch
//! hängenden Prozesse liefert [`stuck_processes`] (die Sammlung kann damit z. B. eine Probe am selben Mount auslassen).
//!
//! Ausgaben der Proben werden nie geloggt; im Log stehen höchstens Programmname und PID.

use super::argv::{Argv, Program};
use super::catalog::{ProbeSpec, MAX_DEADLINE};
use crate::pipeline::ai::CancelFlag;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Nice-Wert der Proben (`nice 10`).
pub const NICE: libc::c_int = 10;
/// Zeit zwischen SIGTERM und SIGKILL nach Ablauf der Frist.
pub const TERM_GRACE: Duration = Duration::from_millis(500);
/// Zeit zwischen SIGTERM und SIGKILL nach einem Abbruch (kurz: der User wartet, und Proben ändern nichts).
pub const CANCEL_GRACE: Duration = Duration::from_millis(50);
/// So lange wartet der Ausführer nach SIGKILL auf das Ende des Leiters, dann übernimmt der Abhol-Thread.
pub const REAP_WAIT: Duration = Duration::from_millis(100);
/// So lange liest der Ausführer nach dem Ende des Leiters noch Pipes, deren Schreibseite jemand offen hält.
pub const PIPE_LINGER: Duration = Duration::from_millis(100);
/// Längstes Warten in `poll`: so schnell wird ein Abbruch bemerkt.
pub const TICK: Duration = Duration::from_millis(10);
/// Mit [`Keep::Head`] höchstens so viele Bytes je Pipe über die Grenze hinaus lesen und verwerfen, dann schließen.
pub const DISCARD_MAX: u64 = 1024 * 1024;
/// So viel länger als die Frist braucht [`run`] höchstens (TERM, Gnadenfrist, KILL, Warten, Nachlesen, Ticks).
pub const MAX_OVERRUN: Duration = Duration::from_millis(500 + 100 + 100 + 3 * 10);
/// Abstand, in dem der Abhol-Thread hängende Prozesse abfragt.
pub const REAPER_POLL: Duration = Duration::from_millis(200);
/// Größe eines Lesevorgangs an einer Pipe.
pub const READ_CHUNK: usize = 16 * 1024;
/// Höchstens so viele Lesevorgänge je Pipe und Durchgang, dann prüft die Hauptschleife Ende, Frist und Abbruch.
pub const READS_PER_TICK: usize = 4;
/// Mit [`Keep::Tail`] höchstens so viele Bytes je Pipe lesen, dann schließen (Schutz gegen Dauerausgabe).
pub const TAIL_READ_MAX: u64 = 8 * 1024 * 1024;

/// Umgebung, die jede Probe sieht, außer `HOME` und `XDG_RUNTIME_DIR` (siehe [`ProbeEnv`]).
const FIXED_ENV: &[(&str, &str)] = &[("PATH", "/usr/bin"), ("LC_ALL", "C.UTF-8"), ("SYSTEMD_PAGER", ""), ("SYSTEMD_COLORS", "0"), ("NO_COLOR", "1")];

/// Zusätzliche Umgebung nur für [`Program::Docker`], außer `DOCKER_CONFIG` (siehe Modulkopf).
pub const DOCKER_ENV: &[(&str, &str)] = &[("DOCKER_HOST", "unix:///var/run/docker.sock"), ("DOCKER_CLI_HOOKS", "false"), ("DOCKER_CLI_HINTS", "false")];
/// Verzeichnis der Docker-Client-Config unter `XDG_RUNTIME_DIR` (leer, Modus 0500).
const DOCKER_CONFIG_SUBDIR: &str = "dictate/diag-docker-config";

const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_IDLE: libc::c_int = 3;
const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
/// `IOPRIO_PRIO_VALUE(IOPRIO_CLASS_IDLE, 0)` (`linux/ioprio.h`).
const IOPRIO_IDLE: libc::c_int = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;

/// Wo die Programme liegen und welche Werte `HOME` und `XDG_RUNTIME_DIR` haben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeEnv {
    bin_dir: PathBuf,
    home: Option<PathBuf>,
    runtime_dir: Option<PathBuf>,
}

impl ProbeEnv {
    /// Produktion: Programme unter `/usr/bin`, `HOME` und `XDG_RUNTIME_DIR` aus der Umgebung des Daemons (nur absolute
    /// Pfade; fehlt `HOME`, gilt das Home-Verzeichnis aus der Benutzerdatenbank).
    pub fn system() -> ProbeEnv {
        let abs = |v: Option<OsString>| v.map(PathBuf::from).filter(|p| p.is_absolute());
        ProbeEnv {
            bin_dir: PathBuf::from("/usr/bin"),
            home: abs(std::env::var_os("HOME")).or_else(|| dirs::home_dir().filter(|p| p.is_absolute())),
            runtime_dir: abs(std::env::var_os("XDG_RUNTIME_DIR")),
        }
    }

    /// Dieselbe Umgebung mit einem anderen Verzeichnis der Programme (Tests mit Fakes). Nur absolute Pfade.
    pub fn with_bin_dir(self, dir: impl Into<PathBuf>) -> Result<ProbeEnv, String> {
        let bin_dir = absolute(dir.into(), "program directory")?;
        Ok(ProbeEnv { bin_dir, ..self })
    }

    /// Anderes `HOME` (`None`: nicht setzen). Nur absolute Pfade.
    pub fn with_home(self, home: Option<PathBuf>) -> Result<ProbeEnv, String> {
        let home = home.map(|h| absolute(h, "HOME")).transpose()?;
        Ok(ProbeEnv { home, ..self })
    }

    /// Anderes `XDG_RUNTIME_DIR` (`None`: nicht setzen). Nur absolute Pfade.
    pub fn with_runtime_dir(self, dir: Option<PathBuf>) -> Result<ProbeEnv, String> {
        let runtime_dir = dir.map(|d| absolute(d, "XDG_RUNTIME_DIR")).transpose()?;
        Ok(ProbeEnv { runtime_dir, ..self })
    }

    /// Pfad des Programms: Verzeichnis dieser Umgebung plus fester Dateiname aus [`Program::name`].
    pub fn program_path(&self, program: Program) -> PathBuf {
        self.bin_dir.join(program.name())
    }

    /// Die vollständige Umgebung des Kindes für `program`.
    pub fn vars(&self, program: Program) -> Vec<(OsString, OsString)> {
        let mut v: Vec<(OsString, OsString)> = FIXED_ENV.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect();
        if let Some(h) = &self.home {
            v.push(("HOME".into(), h.clone().into_os_string()));
        }
        if let Some(r) = &self.runtime_dir {
            v.push(("XDG_RUNTIME_DIR".into(), r.clone().into_os_string()));
        }
        if program == Program::Docker {
            v.extend(DOCKER_ENV.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
            if let Some(c) = self.docker_config_dir() {
                v.push(("DOCKER_CONFIG".into(), c.into_os_string()));
            }
        }
        v
    }

    /// Eigenes Verzeichnis der Docker-Client-Config (`$XDG_RUNTIME_DIR/dictate/diag-docker-config`); `None` ohne
    /// `XDG_RUNTIME_DIR` (dann starten Docker-Proben nicht).
    pub fn docker_config_dir(&self) -> Option<PathBuf> {
        self.runtime_dir.as_ref().map(|r| r.join(DOCKER_CONFIG_SUBDIR))
    }
}

/// Legt das Config-Verzeichnis für Docker an bzw. prüft es: ein echtes Verzeichnis (kein Symlink), gehört diesem
/// Benutzer, Modus 0500 (wird gesetzt), leer. Sonst ein Fehler, und die Probe startet nicht.
fn prepare_docker_config(dir: Option<&Path>) -> io::Result<()> {
    let Some(dir) = dir else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set, so there is no private docker client configuration"));
    };
    if let Some(parent) = dir.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    }
    match std::fs::DirBuilder::new().mode(0o500).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let bad = |what: &str| io::Error::new(io::ErrorKind::PermissionDenied, format!("the docker client configuration {} {what}", dir.display()));
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(bad("is not a directory"));
    }
    // SAFETY: `geteuid` hat keine Vorbedingungen.
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(bad("belongs to another user"));
    }
    if std::fs::read_dir(dir)?.next().is_some() {
        return Err(bad("is not empty"));
    }
    if meta.mode() & 0o7777 != 0o500 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500))?;
    }
    Ok(())
}

fn absolute(p: PathBuf, what: &str) -> Result<PathBuf, String> {
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(format!("the {what} must be an absolute path"))
    }
}

/// Welcher Teil einer zu langen Ausgabe bleibt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Keep {
    /// Der Anfang (Vorgabe); nach [`DISCARD_MAX`] verworfenen Bytes wird die Pipe geschlossen.
    #[default]
    Head,
    /// Das Ende (Journale); die Pipe wird bis zum Ende bzw. bis zur Frist gelesen, höchstens [`TAIL_READ_MAX`] Bytes.
    Tail,
}

/// Frist und Grenzen eines Laufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLimits {
    /// Frist ab dem Start; mehr als [`MAX_DEADLINE`] gilt nicht.
    pub deadline: Duration,
    pub stdout_max: usize,
    pub stderr_max: usize,
    pub keep: Keep,
}

impl RunLimits {
    /// Frist und Grenze der Probe aus dem Katalog; stderr ist ebenso begrenzt wie stdout. Die Grenze der *Summe* über
    /// mehrere Befehle einer Probe zieht der Aufrufer ab (Aufgabe 3/5).
    pub fn for_spec(spec: &ProbeSpec) -> RunLimits {
        RunLimits { deadline: spec.deadline.min(MAX_DEADLINE), stdout_max: spec.limit_bytes, stderr_max: spec.limit_bytes, keep: Keep::Head }
    }

    /// Dieselben Grenzen mit einer Frist von höchstens `remaining` (verbleibendes Gesamtbudget der Sammlung).
    pub fn within(self, remaining: Duration) -> RunLimits {
        RunLimits { deadline: self.deadline.min(remaining), ..self }
    }

    pub fn keep(self, keep: Keep) -> RunLimits {
        RunLimits { keep, ..self }
    }
}

/// Wie ein Lauf ausging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Das Programm hat von selbst geendet (der Exit-Code steht in [`RunResult::exit`], auch wenn er nicht 0 ist).
    Ok,
    /// Die Frist ist abgelaufen (auch: keine Frist mehr übrig, dann wurde nichts gestartet).
    Timeout,
    /// Abgebrochen über das [`CancelFlag`] (auch: schon vor dem Start, dann wurde nichts gestartet).
    Cancelled,
    /// Das Programm ließ sich nicht starten (fehlt, nicht ausführbar, Drossel nicht setzbar).
    SpawnError { kind: io::ErrorKind, message: String },
}

/// Wie der Prozess endete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Exit {
    pub fn success(self) -> bool {
        self == Exit::Code(0)
    }
}

/// Eine begrenzt gelesene Ausgabe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    /// Behaltene Bytes (höchstens die Grenze), nicht notwendig gültiges UTF-8.
    pub bytes: Vec<u8>,
    /// Gelesene Bytes insgesamt (behalten und verworfen).
    pub seen: u64,
    /// Es wurde etwas verworfen oder die Pipe vorzeitig geschlossen.
    pub truncated: bool,
    /// Der Ausführer hat die Pipe vor ihrem Ende geschlossen ([`Keep::Head`] nach [`DISCARD_MAX`] verworfenen,
    /// [`Keep::Tail`] nach [`TAIL_READ_MAX`] gelesenen Bytes): es gab mindestens [`Captured::seen`] Bytes, und bei
    /// `Tail` ist das Behaltene das Ende des Gelesenen, nicht sicher das Ende der Ausgabe.
    pub closed_early: bool,
    /// Welcher Teil behalten wurde.
    pub keep: Keep,
}

impl Captured {
    /// Hinweis für den Bericht, wenn gekürzt wurde.
    pub fn note(&self) -> Option<String> {
        if !self.truncated {
            return None;
        }
        if self.closed_early && self.keep == Keep::Tail {
            return Some(format!(
                "truncated: stopped reading after {} bytes and kept the last {} of them; the output went on, so this is not its end",
                self.seen,
                self.bytes.len()
            ));
        }
        let more = if self.closed_early { "at least " } else { "" };
        Some(format!("truncated: kept {} of {more}{} bytes", self.bytes.len(), self.seen))
    }
}

/// Ergebnis eines Laufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub program: Program,
    pub reason: Reason,
    /// `None`: nicht gestartet, oder nach SIGKILL nicht rechtzeitig geendet ([`RunResult::reaped`] ist dann `false`).
    pub exit: Option<Exit>,
    /// Der Prozess ist abgeholt (kein Zombie). `false` nur, wenn ihn der Abhol-Thread übernommen hat.
    pub reaped: bool,
    pub pid: Option<u32>,
    pub duration: Duration,
    pub stdout: Captured,
    pub stderr: Captured,
}

/// Zeiten und interne Grenzen eines Laufs. Nur Tests ändern sie: `assume_stuck` tut nach SIGKILL so, als ende der
/// Leiter nicht (D-Zustand lässt sich ohne Rechte nicht herstellen), damit der Weg über den Abhol-Thread prüfbar ist;
/// `fail_setup` lässt das Einrichten der Pipes nach dem Start scheitern; `tail_read_max` hebt [`TAIL_READ_MAX`] an,
/// damit sich Frist und Abbruch bei Dauerausgabe prüfen lassen.
#[derive(Debug, Clone, Copy)]
struct Timing {
    term_grace: Duration,
    cancel_grace: Duration,
    reap_wait: Duration,
    linger: Duration,
    tail_read_max: u64,
    assume_stuck: bool,
    fail_setup: bool,
}

const TIMING: Timing = Timing {
    term_grace: TERM_GRACE,
    cancel_grace: CANCEL_GRACE,
    reap_wait: REAP_WAIT,
    linger: PIPE_LINGER,
    tail_read_max: TAIL_READ_MAX,
    assume_stuck: false,
    fail_setup: false,
};

/// Startet `argv` in `env` mit den Grenzen `limits` und wartet auf das Ende, die Frist oder den Abbruch.
pub fn run(env: &ProbeEnv, argv: &Argv, limits: &RunLimits, cancel: &CancelFlag) -> RunResult {
    run_with(env, argv, limits, cancel, &TIMING)
}

fn run_with(env: &ProbeEnv, argv: &Argv, limits: &RunLimits, cancel: &CancelFlag, timing: &Timing) -> RunResult {
    let start = Instant::now();
    let mut result = RunResult {
        program: argv.program,
        reason: Reason::Ok,
        exit: None,
        reaped: true,
        pid: None,
        duration: Duration::ZERO,
        stdout: Captured::default(),
        stderr: Captured::default(),
    };
    if cancel.load(Ordering::SeqCst) {
        result.reason = Reason::Cancelled;
        return result;
    }
    let budget = limits.deadline.min(MAX_DEADLINE);
    if budget.is_zero() {
        result.reason = Reason::Timeout;
        return result;
    }
    let deadline = start + budget;

    let spawned = spawn(env, argv, timing);
    let (pid, out, err) = match spawned {
        Ok(s) => s,
        Err(SpawnFailure { error, pid, reaped }) => {
            result.reason = Reason::SpawnError { kind: error.kind(), message: error.to_string() };
            result.pid = pid.map(|p| p as u32);
            result.reaped = reaped;
            result.duration = start.elapsed();
            return result;
        }
    };
    result.pid = Some(pid as u32);
    let pidfd = pidfd_open(pid);
    let tail_max = timing.tail_read_max;
    let mut streams = [Stream::new(out, limits.stdout_max, limits.keep, tail_max), Stream::new(err, limits.stderr_max, limits.keep, tail_max)];

    let mut phase = Phase::Running;
    let exited = loop {
        let stuck = timing.assume_stuck && matches!(phase, Phase::Killed { .. });
        match child_state(pid) {
            ChildState::Running => {}
            _ if stuck => {}
            other => break other,
        }
        let now = Instant::now();
        let cancelled = cancel.load(Ordering::SeqCst);
        match phase {
            Phase::Running if cancelled => {
                result.reason = Reason::Cancelled;
                signal_group(pid, libc::SIGTERM);
                phase = Phase::Terminating { kill_at: now + timing.cancel_grace };
            }
            Phase::Running if now >= deadline => {
                result.reason = Reason::Timeout;
                signal_group(pid, libc::SIGTERM);
                phase = Phase::Terminating { kill_at: now + timing.term_grace };
            }
            Phase::Terminating { kill_at } if now >= kill_at => {
                signal_group(pid, libc::SIGKILL);
                phase = Phase::Killed { give_up_at: now + timing.reap_wait };
            }
            Phase::Terminating { kill_at } if cancelled => {
                // Abbruch während der Gnadenfrist nach Ablauf der Frist: nicht länger warten als nach einem Abbruch.
                phase = Phase::Terminating { kill_at: kill_at.min(now + timing.cancel_grace) };
            }
            Phase::Killed { give_up_at } if now >= give_up_at => {
                // Hängt im Kernel (D-Zustand): Ergebnis jetzt liefern, abholen übernimmt der Abhol-Thread.
                tracing::warn!("diagnose: {} (pid {pid}) did not end after SIGKILL, handing it to the reaper", argv.program.name());
                hand_to_reaper(pid);
                result.reaped = false;
                for s in &mut streams {
                    s.close();
                }
                result.duration = start.elapsed();
                [result.stdout, result.stderr] = streams.map(Stream::finish);
                return result;
            }
            _ => {}
        }
        let next = match phase {
            Phase::Running => deadline,
            Phase::Terminating { kill_at } => kill_at,
            Phase::Killed { give_up_at } => give_up_at,
        };
        let wait = next.saturating_duration_since(Instant::now()).min(TICK);
        pump(&mut streams, pidfd.as_ref(), wait);
    };

    // Der Leiter hat geendet und ist noch nicht abgeholt: die Gruppen-ID ist sicher unsere. Was noch in der Gruppe
    // läuft, endet jetzt; danach den Leiter abholen. (`Gone`: anderswo abgeholt, die ID könnte neu vergeben sein.)
    if exited == ChildState::Exited {
        signal_group(pid, libc::SIGKILL);
        result.exit = reap(pid);
    }
    drop(pidfd);
    let linger_until = Instant::now() + timing.linger;
    while streams.iter().any(Stream::open) {
        let left = linger_until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        pump(&mut streams, None, left.min(TICK));
    }
    for s in &mut streams {
        s.close();
    }
    result.duration = start.elapsed();
    [result.stdout, result.stderr] = streams.map(Stream::finish);
    result
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Running,
    Terminating { kill_at: Instant },
    Killed { give_up_at: Instant },
}

/// Start gescheitert. `pid` ist gesetzt, wenn das Kind schon lief (Fehler beim Einrichten danach): es ist dann samt
/// Gruppe mit SIGKILL beendet und abgeholt (`reaped`) bzw. dem Abhol-Thread übergeben.
struct SpawnFailure {
    error: io::Error,
    pid: Option<libc::pid_t>,
    reaped: bool,
}

impl From<io::Error> for SpawnFailure {
    fn from(error: io::Error) -> SpawnFailure {
        SpawnFailure { error, pid: None, reaped: true }
    }
}

/// Startet das Kind; liefert PID und die Leseseiten von stdout und stderr (nicht blockierend).
fn spawn(env: &ProbeEnv, argv: &Argv, timing: &Timing) -> Result<(libc::pid_t, OwnedFd, OwnedFd), SpawnFailure> {
    if sigchld_reaped_by_kernel() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "SIGCHLD is ignored in this process (children are reaped by the kernel), so a probe could not be stopped safely",
        )
        .into());
    }
    if argv.program == Program::Docker {
        prepare_docker_config(env.docker_config_dir().as_deref())?;
    }
    let mut cmd = std::process::Command::new(env.program_path(argv.program));
    cmd.args(&argv.args)
        .env_clear()
        .envs(env.vars(argv.program))
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: `prepare_child` ruft nur `getpriority`, `setpriority`, `ioprio_set`, `close_range`, `getrlimit` und
    // `fcntl` (async-signal-sicher), liest `errno` und legt nichts an; das ist zwischen `fork` und `exec` erlaubt.
    unsafe {
        cmd.pre_exec(prepare_child);
    }
    let mut child = cmd.spawn()?;
    let pid = child.id() as libc::pid_t;
    let out = OwnedFd::from(child.stdout.take().expect("stdout is piped"));
    let err = OwnedFd::from(child.stderr.take().expect("stderr is piped"));
    // `Child` beendet und holt beim Drop nichts ab; Signale und Abholen erledigt dieses Modul über die PID.
    drop(child);
    let setup = if timing.fail_setup { Err(io::Error::other("simulated setup failure")) } else { set_nonblocking(&out).and_then(|()| set_nonblocking(&err)) };
    if let Err(error) = setup {
        // Das Kind läuft schon: nicht ohne Frist und ungeholt zurücklassen.
        drop((out, err));
        let reaped = kill_and_reap(pid, timing.reap_wait);
        return Err(SpawnFailure { error, pid: Some(pid), reaped });
    }
    Ok((pid, out, err))
}

/// SIGKILL an die Gruppe des noch nicht abgeholten Leiters `pid`, dann höchstens `wait` auf sein Ende warten und
/// abholen; endet er nicht (D-Zustand), übernimmt ihn der Abhol-Thread. Liefert, ob er abgeholt ist.
fn kill_and_reap(pid: libc::pid_t, wait: Duration) -> bool {
    signal_group(pid, libc::SIGKILL);
    let until = Instant::now() + wait;
    loop {
        match child_state(pid) {
            ChildState::Exited => {
                reap(pid);
                return true;
            }
            ChildState::Gone => return true,
            ChildState::Running if Instant::now() >= until => {
                hand_to_reaper(pid);
                return false;
            }
            ChildState::Running => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// Holt der Kernel Kinder selbst ab (SIGCHLD auf `SIG_IGN` oder `SA_NOCLDWAIT`)? Dann wären Gruppensignale unsicher
/// (siehe Modulkopf). Ist die Einstellung nicht lesbar, gilt das ebenfalls. Async-signal-sicher (nur `sigaction`).
fn sigchld_reaped_by_kernel() -> bool {
    // SAFETY: `old` ist ein gültiger, beschreibbarer `sigaction`; mit `act = NULL` ändert `sigaction` nichts.
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut old) != 0 {
            return true;
        }
        old.sa_sigaction == libc::SIG_IGN || old.sa_flags & libc::SA_NOCLDWAIT != 0
    }
}

/// Im Kind vor `exec`: Drossel und `FD_CLOEXEC` für alle geerbten Deskriptoren ab 3.
fn prepare_child() -> io::Result<()> {
    throttle()?;
    cloexec_inherited()
}

/// Setzt `FD_CLOEXEC` auf alle Deskriptoren ab 3 (stdio liegt da schon auf 0–2). `close_range` mit
/// `CLOSE_RANGE_CLOEXEC` (Linux ≥ 5.11) ist ein Syscall; kennt der Kernel ihn oder das Flag nicht, eine Schleife mit
/// `fcntl` bis zur Grenze `RLIMIT_NOFILE`.
fn cloexec_inherited() -> io::Result<()> {
    // SAFETY: Syscall ohne Zeiger.
    let r = unsafe { libc::syscall(libc::SYS_close_range, 3 as libc::c_uint, libc::c_uint::MAX, libc::CLOSE_RANGE_CLOEXEC) };
    if r == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ENOSYS) | Some(libc::EINVAL) => cloexec_by_loop(),
        _ => Err(e),
    }
}

/// Rückfall für [`cloexec_inherited`]: `fcntl(F_SETFD)` für jeden offenen Deskriptor von 3 bis unter die weiche Grenze
/// `RLIMIT_NOFILE` (darüber kann kein Deskriptor liegen, solange die Grenze nicht nachträglich gesenkt wurde).
fn cloexec_by_loop() -> io::Result<()> {
    // SAFETY: `lim` ist beschreibbar; `fcntl` auf eine beliebige Zahl liefert höchstens `EBADF`.
    unsafe {
        let mut lim: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) != 0 {
            return Err(io::Error::last_os_error());
        }
        let top = lim.rlim_cur.min(libc::c_int::MAX as libc::rlim_t) as libc::c_int;
        for fd in 3..top {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 && libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

/// Im Kind vor `exec`: nice [`NICE`] (oder netter, falls schon so) und I/O-Klasse idle.
fn throttle() -> io::Result<()> {
    // SAFETY: einfache Syscalls ohne Zeiger; `__errno_location` liefert den gültigen errno-Zeiger dieses Threads.
    unsafe {
        *libc::__errno_location() = 0;
        let current = libc::getpriority(libc::PRIO_PROCESS, 0);
        if current == -1 && *libc::__errno_location() != 0 {
            return Err(io::Error::last_os_error());
        }
        if current < NICE && libc::setpriority(libc::PRIO_PROCESS, 0, NICE) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_IDLE) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: `fd` ist ein offener Deskriptor dieses Prozesses.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// pidfd des Kindes (Linux ≥ 5.3): wird lesbar, sobald es endet, und weckt `poll` ohne Tick-Verzug. Fehlt er, merkt
/// der Ausführer das Ende spätestens nach [`TICK`].
fn pidfd_open(pid: libc::pid_t) -> Option<OwnedFd> {
    // SAFETY: Syscall ohne Zeiger; ein Ergebnis ≥ 0 ist ein neuer Deskriptor, der nur hier besessen wird.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn signal_group(pid: libc::pid_t, sig: libc::c_int) {
    // SAFETY: Syscall ohne Zeiger. Nur gerufen, solange der Leiter `pid` nicht abgeholt ist: die Gruppe ist unsere.
    unsafe {
        libc::kill(-pid, sig);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildState {
    Running,
    /// Geendet, noch nicht abgeholt.
    Exited,
    /// Nicht mehr unser Kind (anderswo abgeholt); kein Exit-Status.
    Gone,
}

fn child_state(pid: libc::pid_t) -> ChildState {
    loop {
        // SAFETY: `info` ist ein gültiger, beschreibbarer `siginfo_t`; mit WNOWAIT bleibt das Kind abholbar.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
        if r == 0 {
            // SAFETY: nach erfolgreichem `waitid` ist `si_pid` gesetzt (0, wenn noch niemand geendet hat).
            return if unsafe { info.si_pid() } == 0 { ChildState::Running } else { ChildState::Exited };
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return ChildState::Gone;
        }
    }
}

/// Holt den geendeten Leiter ab und liefert seinen Exit-Status.
fn reap(pid: libc::pid_t) -> Option<Exit> {
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: `status` ist beschreibbar; `pid` ist ein geendetes, noch nicht abgeholtes Kind.
        let r = unsafe { libc::waitpid(pid, &mut status, 0) };
        if r == pid {
            return if libc::WIFEXITED(status) {
                Some(Exit::Code(libc::WEXITSTATUS(status)))
            } else if libc::WIFSIGNALED(status) {
                Some(Exit::Signal(libc::WTERMSIG(status)))
            } else {
                None
            };
        }
        if r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return None;
    }
}

/// Eine Pipe mit ihrer Grenze.
struct Stream {
    fd: Option<OwnedFd>,
    cap: Captured,
    max: usize,
    keep: Keep,
    discarded: u64,
    /// Mit [`Keep::Tail`]: nach so vielen gelesenen Bytes schließen ([`TAIL_READ_MAX`]).
    tail_read_max: u64,
}

impl Stream {
    fn new(fd: OwnedFd, max: usize, keep: Keep, tail_read_max: u64) -> Stream {
        Stream { fd: Some(fd), cap: Captured { keep, ..Captured::default() }, max, keep, discarded: 0, tail_read_max }
    }

    fn open(&self) -> bool {
        self.fd.is_some()
    }

    fn close(&mut self) {
        self.fd = None;
    }

    /// Liest, bis die Pipe leer (`EAGAIN`), zu Ende oder zu ist, höchstens aber [`READS_PER_TICK`] Mal: danach
    /// kommt die Hauptschleife wieder an die Reihe (Frist, Abbruch), auch wenn das Kind schneller schreibt.
    fn drain(&mut self) {
        let mut buf = [0u8; READ_CHUNK];
        for _ in 0..READS_PER_TICK {
            let Some(fd) = &self.fd else { return };
            // SAFETY: `buf` ist beschreibbar und `buf.len()` Bytes lang; `fd` ist offen.
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                self.push(&buf[..n as usize]);
                continue;
            }
            if n < 0 {
                match io::Error::last_os_error().kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock => return,
                    _ => {}
                }
            }
            // Ende (0) oder Lesefehler: die Pipe ist für uns fertig.
            self.fd = None;
            return;
        }
    }

    fn push(&mut self, data: &[u8]) {
        self.cap.seen += data.len() as u64;
        match self.keep {
            Keep::Head => {
                let room = self.max - self.cap.bytes.len();
                let take = room.min(data.len());
                self.cap.bytes.extend_from_slice(&data[..take]);
                self.discarded += (data.len() - take) as u64;
                if self.discarded > DISCARD_MAX {
                    self.cap.closed_early = true;
                    self.fd = None;
                }
            }
            Keep::Tail => {
                self.cap.bytes.extend_from_slice(data);
                // Erst bei deutlichem Überhang vorn abschneiden, damit nicht jedes Lesen den Puffer verschiebt.
                if self.cap.bytes.len() > self.max + self.max.max(16 * 1024) {
                    let cut = self.cap.bytes.len() - self.max;
                    self.cap.bytes.drain(..cut);
                }
                if self.cap.seen >= self.tail_read_max {
                    self.cap.closed_early = true;
                    self.fd = None;
                }
            }
        }
    }

    fn finish(mut self) -> Captured {
        if self.cap.bytes.len() > self.max {
            let cut = self.cap.bytes.len() - self.max;
            self.cap.bytes.drain(..cut);
        }
        self.cap.truncated = self.cap.closed_early || self.cap.seen > self.cap.bytes.len() as u64;
        self.cap
    }
}

/// Wartet höchstens `wait` auf Daten an den offenen Pipes oder das Ende des Kindes (pidfd) und leert die Pipes.
fn pump(streams: &mut [Stream; 2], pidfd: Option<&OwnedFd>, wait: Duration) {
    let mut fds: Vec<libc::pollfd> = Vec::with_capacity(3);
    for s in streams.iter() {
        if let Some(fd) = &s.fd {
            fds.push(libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
    }
    if let Some(p) = pidfd {
        fds.push(libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 });
    }
    let ms = wait.as_millis().min(i32::MAX as u128) as libc::c_int;
    // SAFETY: `fds` ist ein gültiges, beschreibbares Feld mit `fds.len()` Einträgen (auch leer: dann nur Warten).
    unsafe {
        libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms);
    }
    // Nicht nur die gemeldeten: ein Lesen auf einer leeren, nicht blockierenden Pipe kostet nur ein EAGAIN.
    for s in streams.iter_mut() {
        s.drain();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Abhol-Thread

static REAPER: OnceLock<Mutex<Option<mpsc::Sender<libc::pid_t>>>> = OnceLock::new();
static STUCK: AtomicUsize = AtomicUsize::new(0);

/// Zahl der Prozesse, die nach SIGKILL noch nicht geendet haben (D-Zustand) und die der Abhol-Thread hält.
pub fn stuck_processes() -> usize {
    STUCK.load(Ordering::SeqCst)
}

fn hand_to_reaper(pid: libc::pid_t) {
    STUCK.fetch_add(1, Ordering::SeqCst);
    let lock = REAPER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("diag-reaper".into()).spawn(move || reaper_main(rx));
        Mutex::new(spawned.ok().map(|_| tx))
    });
    let sent = match lock.lock() {
        Ok(guard) => guard.as_ref().is_some_and(|tx| tx.send(pid).is_ok()),
        Err(_) => false,
    };
    if !sent {
        tracing::warn!("diagnose: cannot start the reaper thread; pid {pid} stays a zombie until dictate exits");
    }
}

fn reaper_main(rx: mpsc::Receiver<libc::pid_t>) {
    let mut pids: Vec<libc::pid_t> = Vec::new();
    loop {
        let next = if pids.is_empty() {
            match rx.recv() {
                Ok(p) => Some(p),
                Err(_) => return,
            }
        } else {
            match rx.recv_timeout(REAPER_POLL) {
                Ok(p) => Some(p),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => None,
            }
        };
        pids.extend(next);
        pids.retain(|&pid| {
            let mut status: libc::c_int = 0;
            // SAFETY: `status` ist beschreibbar; WNOHANG blockiert nie.
            let r = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            let interrupted = r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted;
            let keep = r == 0 || interrupted;
            if !keep {
                STUCK.fetch_sub(1, Ordering::SeqCst);
            }
            keep
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::catalog;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Eigener Temp-Ordner je Test, beim Drop gelöscht.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("dictate-diag-run-{}-{tag}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Legt ein Fake-Programm `program` (Shell-Skript) in einem neuen Ordner an und liefert Ordner und Umgebung.
    fn fake(program: Program, script: &str) -> (TempDir, ProbeEnv) {
        let dir = TempDir::new(program.name());
        let path = dir.path().join(program.name());
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let env = ProbeEnv::system()
            .with_bin_dir(dir.path())
            .unwrap()
            .with_home(Some(dir.path().join("home")))
            .unwrap()
            .with_runtime_dir(Some(dir.path().join("run")))
            .unwrap();
        (dir, env)
    }

    fn limits(deadline_ms: u64, max: usize) -> RunLimits {
        RunLimits { deadline: Duration::from_millis(deadline_ms), stdout_max: max, stderr_max: max, keep: Keep::Head }
    }

    fn argv(program: Program, args: &[&str]) -> Argv {
        Argv { program, args: args.iter().map(|s| s.to_string()).collect() }
    }

    fn flag() -> CancelFlag {
        Arc::new(AtomicBool::new(false))
    }

    /// `run_with`, aber wiederholt bei `ETXTBSY`: ein paralleler Test kann zwischen `fork` und `exec` den
    /// Schreib-Deskriptor eines eben geschriebenen Skripts geerbt haben (nur in Tests, nie mit `/usr/bin`).
    fn run_fake(env: &ProbeEnv, argv: &Argv, limits: &RunLimits, cancel: &CancelFlag, timing: &Timing) -> RunResult {
        for _ in 0..100 {
            let r = run_with(env, argv, limits, cancel, timing);
            match &r.reason {
                Reason::SpawnError { kind: io::ErrorKind::ExecutableFileBusy, .. } => std::thread::sleep(Duration::from_millis(10)),
                _ => return r,
            }
        }
        panic!("fake stayed busy");
    }

    /// Lebt `pid` noch (und ist kein Zombie)?
    fn alive(pid: u32) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                let state = stat.rsplit_once(')').map(|(_, rest)| rest.trim_start().chars().next()).unwrap_or(None);
                !matches!(state, Some('Z') | Some('X'))
            }
            Err(_) => false,
        }
    }

    fn wait_dead(pid: u32, within: Duration) -> bool {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if !alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        !alive(pid)
    }

    fn pid_from(out: &[u8]) -> u32 {
        String::from_utf8_lossy(out).trim().lines().next().unwrap().trim().parse().unwrap()
    }

    fn wait_file(path: &Path, within: Duration) -> String {
        let until = Instant::now() + within;
        loop {
            if let Ok(s) = fs::read_to_string(path) {
                if s.ends_with('\n') {
                    return s;
                }
            }
            assert!(Instant::now() < until, "{} not written", path.display());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn system_env_uses_the_catalog_paths_and_only_replaces_the_directory() {
        let sys = ProbeEnv::system();
        for &p in Program::ALL {
            assert_eq!(sys.program_path(p), Path::new(p.path()), "{}", p.name());
        }
        let fake = sys.clone().with_bin_dir("/tmp/fakes").unwrap();
        for &p in Program::ALL {
            let path = fake.program_path(p);
            assert_eq!(path.parent(), Some(Path::new("/tmp/fakes")));
            assert_eq!(path.file_name().unwrap(), p.name());
        }
        assert!(sys.clone().with_bin_dir("fakes").is_err());
        assert!(sys.clone().with_home(Some("home".into())).is_err());
        assert!(sys.with_runtime_dir(Some("run".into())).is_err());
    }

    #[test]
    fn limits_follow_the_catalog_and_the_remaining_budget() {
        for spec in catalog::all() {
            let l = RunLimits::for_spec(spec);
            assert_eq!(l.deadline, spec.deadline);
            assert!(l.deadline <= MAX_DEADLINE);
            assert_eq!((l.stdout_max, l.stderr_max, l.keep), (spec.limit_bytes, spec.limit_bytes, Keep::Head));
            assert_eq!(l.within(Duration::from_millis(250)).deadline, Duration::from_millis(250).min(spec.deadline));
            assert_eq!(l.within(Duration::from_secs(60)).deadline, spec.deadline);
            assert_eq!(l.keep(Keep::Tail).keep, Keep::Tail);
        }
    }

    #[test]
    fn exit_code_and_both_streams_are_reported() {
        let (_d, env) = fake(Program::Systemctl, "echo hello\necho oops >&2\nexit 3");
        let r = run_fake(&env, &argv(Program::Systemctl, &[]), &limits(3000, 1024), &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Ok);
        assert_eq!(r.exit, Some(Exit::Code(3)));
        assert!(r.reaped);
        assert_eq!(r.stdout.bytes, b"hello\n");
        assert_eq!(r.stderr.bytes, b"oops\n");
        assert!(!r.stdout.truncated && !r.stderr.truncated);
        assert_eq!(r.stdout.note(), None);
        assert!(r.duration < Duration::from_secs(2));
    }

    #[test]
    fn arguments_arrive_unchanged_without_a_shell() {
        let (_d, env) = fake(Program::Journalctl, "for a in \"$@\"; do printf '[%s]\\n' \"$a\"; done");
        let args = ["--boot=-1", "a b", "$(id)", "x;y", "*"];
        let r = run_fake(&env, &argv(Program::Journalctl, &args), &limits(3000, 1024), &flag(), &TIMING);
        assert_eq!(r.exit, Some(Exit::Code(0)));
        assert_eq!(String::from_utf8(r.stdout.bytes).unwrap(), "[--boot=-1]\n[a b]\n[$(id)]\n[x;y]\n[*]\n");
    }

    #[test]
    fn hanging_program_that_ignores_term_is_killed_with_its_group() {
        // TERM wird gemeldet und ignoriert; das Kind in derselben Gruppe soll mit enden.
        let script = "trap 'echo term >> \"$1\"' TERM\nsleep 30 &\necho $!\nwhile :; do sleep 0.05; done";
        let (d, env) = fake(Program::Docker, script);
        let mark = d.path().join("term");
        let deadline = 400;
        let r = run_fake(&env, &argv(Program::Docker, &[mark.to_str().unwrap()]), &limits(deadline, 1024), &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Timeout);
        assert_eq!(r.exit, Some(Exit::Signal(libc::SIGKILL)));
        assert!(r.reaped);
        let lo = Duration::from_millis(deadline) + TERM_GRACE - Duration::from_millis(20);
        assert!(r.duration >= lo && r.duration <= Duration::from_millis(deadline) + MAX_OVERRUN, "{:?}", r.duration);
        assert!(fs::read_to_string(&mark).unwrap().contains("term"), "SIGTERM came first");
        let child = pid_from(&r.stdout.bytes);
        assert!(wait_dead(child, Duration::from_secs(2)), "child in the group survived");
        assert!(!alive(r.pid.unwrap()));
    }

    #[test]
    fn remaining_budget_shortens_the_deadline() {
        let (_d, env) = fake(Program::Ss, "while :; do sleep 0.05; done");
        let l = limits(10_000, 1024).within(Duration::from_millis(300));
        let r = run_fake(&env, &argv(Program::Ss, &[]), &l, &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Timeout);
        assert_eq!(r.exit, Some(Exit::Signal(libc::SIGTERM)));
        assert!(r.duration < Duration::from_millis(300 + 250), "{:?}", r.duration);
    }

    #[test]
    fn large_output_is_limited_per_stream_while_reading_without_blocking() {
        // stderr zuerst und weit über den Pipe-Puffer (64 KiB): liest der Ausführer nur stdout, hängt das Fake.
        let script = "head -c 200000 /dev/zero >&2\nhead -c 300000 /dev/zero\necho end >&2\nexit 0";
        let (_d, env) = fake(Program::Lsblk, script);
        let l = RunLimits { deadline: Duration::from_secs(5), stdout_max: 1000, stderr_max: 500, keep: Keep::Head };
        let r = run_fake(&env, &argv(Program::Lsblk, &[]), &l, &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Ok);
        assert_eq!(r.exit, Some(Exit::Code(0)), "drained to the end, no SIGPIPE");
        assert_eq!((r.stdout.bytes.len(), r.stdout.seen), (1000, 300_000));
        assert_eq!((r.stderr.bytes.len(), r.stderr.seen), (500, 200_004));
        assert!(r.stdout.truncated && r.stderr.truncated);
        assert!(!r.stdout.closed_early && !r.stderr.closed_early);
        assert_eq!(r.stdout.note().unwrap(), "truncated: kept 1000 of 300000 bytes");
        assert!(r.duration < Duration::from_secs(2), "{:?}", r.duration);
    }

    #[test]
    fn endless_output_closes_the_pipes_early_instead_of_running_to_the_deadline() {
        // Der stderr-Schreiber läuft nachweislich, bevor der Leiter stdout flutet: er (eine endlose Shell-Schleife in der
        // Gruppe) schreibt zuerst 600 Zeilen (2400 Byte, über `stderr_max`) und legt erst dann die Markierung
        // `$1.ready` an; der Leiter wartet darauf und wird erst dann zu `yes out`. Ein nur abgezweigtes `yes err` war
        // dafür nicht verlässlich: der Leiter endet nach 1 MiB mit SIGPIPE in 3 bis 5 ms, und unter Last (parallele
        // Tests) war der Schreiber dann in einem von sechs Läufen noch nicht zum Zug gekommen und stderr leer.
        let script = "{ n=0; while :; do echo err; n=$((n+1)); [ $n = 600 ] && : > \"$1.ready\"; done; } >&2 &\n\
                      echo $! > \"$1\"\n\
                      while [ ! -e \"$1.ready\" ]; do sleep 0.01; done\n\
                      exec yes out";
        let (d, env) = fake(Program::Findmnt, script);
        let mark = d.path().join("writer");
        let l = RunLimits { deadline: Duration::from_secs(8), stdout_max: 4096, stderr_max: 2048, keep: Keep::Head };
        let r = run_fake(&env, &argv(Program::Findmnt, &[mark.to_str().unwrap()]), &l, &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Ok);
        assert_eq!(r.exit, Some(Exit::Signal(libc::SIGPIPE)));
        assert!(r.stdout.closed_early && r.stdout.truncated);
        assert_eq!(r.stdout.bytes.len(), 4096);
        assert!(r.stdout.bytes.starts_with(b"out\nout\n"));
        assert!(r.stdout.seen > DISCARD_MAX);
        assert!(r.stdout.note().unwrap().contains("at least"));
        // stderr: gleichzeitig überflutet, auf die Grenze gekürzt, der Anfang bleibt.
        assert!(r.stderr.truncated && r.stderr.seen >= 2400, "{:?}", (r.stderr.truncated, r.stderr.seen));
        assert_eq!(r.stderr.bytes.len(), 2048);
        assert!(r.stderr.bytes.starts_with(b"err\nerr\n"));
        assert!(r.duration < Duration::from_secs(3), "{:?}", r.duration);
        // Der endlose stderr-Schreiber gehört zur Gruppe und endet mit dem Leiter.
        let writer = pid_from(&fs::read(&mark).unwrap());
        assert!(wait_dead(writer, Duration::from_secs(2)), "stderr writer in the group survived");
    }

    #[test]
    fn tail_mode_keeps_the_end() {
        let (_d, env) = fake(Program::Journalctl, "seq 1 200000");
        let l = RunLimits { deadline: Duration::from_secs(5), stdout_max: 100, stderr_max: 100, keep: Keep::Tail };
        let r = run_fake(&env, &argv(Program::Journalctl, &[]), &l, &flag(), &TIMING);
        assert_eq!(r.exit, Some(Exit::Code(0)));
        assert_eq!(r.stdout.bytes.len(), 100);
        assert!(r.stdout.bytes.ends_with(b"199999\n200000\n"));
        assert!(r.stdout.truncated && !r.stdout.closed_early);
        assert!(r.stdout.seen > 1_000_000);
        assert!(!r.stderr.truncated);
    }

    #[test]
    fn children_in_the_group_end_with_the_leader() {
        let (_d, env) = fake(Program::Tailscale, "sleep 30 &\necho $!\nexit 0");
        let r = run_fake(&env, &argv(Program::Tailscale, &[]), &limits(3000, 1024), &flag(), &TIMING);
        assert_eq!((r.reason.clone(), r.exit), (Reason::Ok, Some(Exit::Code(0))));
        let child = pid_from(&r.stdout.bytes);
        assert!(wait_dead(child, Duration::from_secs(2)), "child in the group survived");
        // Das Kind hielt stdout offen; trotzdem kein Warten bis zur Frist.
        assert!(r.duration < Duration::from_secs(1), "{:?}", r.duration);
    }

    #[test]
    fn escaped_grandchild_does_not_hold_the_runner_on_its_pipes() {
        // `setsid` gründet Sitzung und Gruppe: dieser Enkel wird *nicht* beendet (dokumentiert), hält stdout und stderr
        // offen, und der Ausführer liefert trotzdem nach PIPE_LINGER.
        let (_d, env) = fake(Program::Resolvectl, "setsid sleep 30 &\necho $!\nsleep 0.1\nexit 0");
        let r = run_fake(&env, &argv(Program::Resolvectl, &[]), &limits(5000, 1024), &flag(), &TIMING);
        assert_eq!((r.reason.clone(), r.exit), (Reason::Ok, Some(Exit::Code(0))));
        assert!(r.duration < Duration::from_secs(1), "{:?}", r.duration);
        let escaped = pid_from(&r.stdout.bytes);
        let pgid = unsafe { libc::getpgid(escaped as libc::pid_t) };
        if alive(escaped) {
            assert_ne!(pgid, r.pid.unwrap() as libc::pid_t, "left the group");
            // Aufräumen nur per PID.
            unsafe {
                libc::kill(escaped as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    #[test]
    fn cancel_ends_a_term_ignoring_program_within_200_ms() {
        let (_d, env) = fake(Program::Coredumpctl, "trap '' TERM\nwhile :; do sleep 0.05; done");
        let cancel = flag();
        let setter = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                cancel.store(true, Ordering::SeqCst);
                Instant::now()
            })
        };
        let r = run_fake(&env, &argv(Program::Coredumpctl, &[]), &limits(8000, 1024), &cancel, &TIMING);
        let returned = Instant::now();
        let set_at = setter.join().unwrap();
        assert_eq!(r.reason, Reason::Cancelled);
        assert_eq!(r.exit, Some(Exit::Signal(libc::SIGKILL)));
        let reaction = returned.duration_since(set_at);
        assert!(reaction < Duration::from_millis(200), "{reaction:?}");
        assert!(!alive(r.pid.unwrap()));
    }

    #[test]
    fn cancelled_or_out_of_time_before_the_start_runs_nothing() {
        let (d, env) = fake(Program::Pacman, "echo ran > \"$1\"");
        let mark = d.path().join("ran");
        let a = argv(Program::Pacman, &[mark.to_str().unwrap()]);
        let cancel = flag();
        cancel.store(true, Ordering::SeqCst);
        let r = run_fake(&env, &a, &limits(3000, 64), &cancel, &TIMING);
        assert_eq!((r.reason, r.exit, r.pid), (Reason::Cancelled, None, None));
        let r = run_fake(&env, &a, &limits(3000, 64).within(Duration::ZERO), &flag(), &TIMING);
        assert_eq!((r.reason, r.exit, r.pid), (Reason::Timeout, None, None));
        assert!(!mark.exists());
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let dir = TempDir::new("empty");
        let env = ProbeEnv::system().with_bin_dir(dir.path()).unwrap();
        let r = run_fake(&env, &argv(Program::Df, &[]), &limits(3000, 64), &flag(), &TIMING);
        assert!(matches!(r.reason, Reason::SpawnError { kind: io::ErrorKind::NotFound, .. }), "{:?}", r.reason);
        assert_eq!((r.exit, r.pid), (None, None));
    }

    #[test]
    fn child_has_empty_env_null_stdin_own_group_root_cwd_and_is_throttled() {
        let (d, env) = fake(Program::Sensors, "echo $$ > \"$1\"\nwhile :; do sleep 0.05; done");
        let mark = d.path().join("pid");
        let cancel = flag();
        let runner = {
            let (env, cancel) = (env.clone(), cancel.clone());
            let a = argv(Program::Sensors, &[mark.to_str().unwrap()]);
            std::thread::spawn(move || run_fake(&env, &a, &limits(8000, 1024), &cancel, &TIMING))
        };
        let pid: libc::pid_t = wait_file(&mark, Duration::from_secs(5)).trim().parse().unwrap();

        // Umgebung: genau die feste Liste plus HOME und XDG_RUNTIME_DIR, sonst nichts.
        let environ = fs::read(format!("/proc/{pid}/environ")).unwrap();
        let mut got: Vec<String> = environ.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8(s.to_vec()).unwrap()).collect();
        got.sort();
        let mut want: Vec<String> = FIXED_ENV.iter().map(|(k, v)| format!("{k}={v}")).collect();
        want.push(format!("HOME={}", d.path().join("home").display()));
        want.push(format!("XDG_RUNTIME_DIR={}", d.path().join("run").display()));
        want.sort();
        assert_eq!(got, want);

        assert_eq!(fs::read_link(format!("/proc/{pid}/fd/0")).unwrap(), Path::new("/dev/null"));
        assert_eq!(fs::read_link(format!("/proc/{pid}/cwd")).unwrap(), Path::new("/"));
        assert_eq!(unsafe { libc::getpgid(pid) }, pid, "leader of its own group");

        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let fields: Vec<&str> = stat.rsplit_once(')').unwrap().1.split_whitespace().collect();
        let nice: i32 = fields[16].parse().unwrap(); // Feld 19 von proc_pid_stat(5); `fields[0]` ist Feld 3
        let own = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        assert_eq!(nice, own.max(NICE));
        let ioprio = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, pid) };
        assert_eq!(ioprio as libc::c_int >> IOPRIO_CLASS_SHIFT, IOPRIO_CLASS_IDLE, "ioprio {ioprio:#x}");

        cancel.store(true, Ordering::SeqCst);
        let r = runner.join().unwrap();
        assert_eq!(r.reason, Reason::Cancelled);
        assert_eq!(r.pid, Some(pid as u32));
    }

    #[test]
    fn process_that_outlives_sigkill_is_handed_to_the_reaper() {
        // D-Zustand lässt sich ohne Rechte nicht herstellen; `assume_stuck` nimmt denselben Weg, und der Abhol-Thread
        // muss den Zombie abholen.
        let (_d, env) = fake(Program::NvidiaSmi, "trap '' TERM\nwhile :; do sleep 0.05; done");
        let timing = Timing { assume_stuck: true, ..TIMING };
        let r = run_fake(&env, &argv(Program::NvidiaSmi, &[]), &limits(200, 64), &flag(), &timing);
        assert_eq!(r.reason, Reason::Timeout);
        assert!(!r.reaped);
        assert_eq!(r.exit, None);
        let pid = r.pid.unwrap();
        // Abgeholt heißt: kein Eintrag mehr unter /proc (als Zombie stünde er dort mit Zustand Z, bis wir ihn abholen).
        let until = Instant::now() + Duration::from_secs(3);
        while Path::new(&format!("/proc/{pid}")).exists() {
            assert!(Instant::now() < until, "reaper did not collect pid {pid}");
            std::thread::sleep(Duration::from_millis(20));
        }
        while stuck_processes() != 0 {
            assert!(Instant::now() < until, "stuck count not cleared");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(r.duration <= Duration::from_millis(200) + MAX_OVERRUN, "{:?}", r.duration);
    }

    #[test]
    fn drain_reads_at_most_one_tick_worth_and_returns_to_the_main_loop() {
        // Mehr in der Pipe, als ein Durchgang liest: `drain` muss nach READS_PER_TICK × READ_CHUNK zurückkehren, auch
        // wenn nie `EAGAIN` kommt (so prüft die Hauptschleife Frist und Abbruch auch bei Dauerausgabe).
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) }, 0);
        let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let size = unsafe { libc::fcntl(w.as_raw_fd(), libc::F_SETPIPE_SZ, 1024 * 1024) };
        assert!(size >= 256 * 1024, "pipe size {size}");
        let data = vec![b'x'; 256 * 1024];
        let n = unsafe { libc::write(w.as_raw_fd(), data.as_ptr().cast(), data.len()) };
        assert_eq!(n as usize, data.len());
        let per_tick = (READS_PER_TICK * READ_CHUNK) as u64;
        for keep in [Keep::Head, Keep::Tail] {
            let dup = r.try_clone().unwrap();
            let mut s = Stream::new(dup, 1024, keep, TAIL_READ_MAX);
            s.drain();
            assert_eq!(s.cap.seen, per_tick, "{keep:?}");
            assert!(s.open());
            s.drain();
            assert_eq!(s.cap.seen, 2 * per_tick, "{keep:?}");
        }
    }

    #[test]
    fn tail_mode_stops_reading_endless_output_at_the_read_cap() {
        let (_d, env) = fake(Program::Journalctl, "exec yes");
        let l = RunLimits { deadline: Duration::from_secs(8), stdout_max: 100, stderr_max: 100, keep: Keep::Tail };
        let r = run_fake(&env, &argv(Program::Journalctl, &[]), &l, &flag(), &TIMING);
        assert_eq!(r.reason, Reason::Ok);
        assert_eq!(r.exit, Some(Exit::Signal(libc::SIGPIPE)));
        assert!(r.stdout.closed_early && r.stdout.truncated);
        assert!(r.stdout.seen >= TAIL_READ_MAX && r.stdout.seen < TAIL_READ_MAX + READ_CHUNK as u64, "{}", r.stdout.seen);
        assert_eq!(r.stdout.bytes.len(), 100);
        assert!(r.stdout.bytes.ends_with(b"y\n"));
        assert!(r.stdout.note().unwrap().contains("not its end"), "{:?}", r.stdout.note());
        assert!(r.duration < Duration::from_secs(3), "{:?}", r.duration);
    }

    #[test]
    fn tail_mode_with_endless_output_keeps_deadline_and_cancel() {
        // Ohne Lesegrenze (nur hier) und mit mehreren Schreibern auf derselben Pipe: Frist und Abbruch gelten trotzdem.
        let timing = Timing { tail_read_max: u64::MAX, ..TIMING };
        let (_d, env) = fake(Program::Journalctl, "yes &\nyes &\nyes &\nexec yes");
        let a = argv(Program::Journalctl, &[]);
        let l = RunLimits { deadline: Duration::from_millis(300), stdout_max: 1024, stderr_max: 1024, keep: Keep::Tail };
        let r = run_fake(&env, &a, &l, &flag(), &timing);
        assert_eq!(r.reason, Reason::Timeout);
        assert!(r.duration <= Duration::from_millis(300) + MAX_OVERRUN, "{:?}", r.duration);
        assert!(!r.stdout.closed_early && r.stdout.seen > TAIL_READ_MAX / 8, "{}", r.stdout.seen);

        let cancel = flag();
        let setter = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                cancel.store(true, Ordering::SeqCst);
                Instant::now()
            })
        };
        let r = run_fake(&env, &a, &l.within(Duration::from_secs(8)), &cancel, &timing);
        let returned = Instant::now();
        let set_at = setter.join().unwrap();
        assert_eq!(r.reason, Reason::Cancelled);
        let reaction = returned.duration_since(set_at);
        assert!(reaction < Duration::from_millis(200), "{reaction:?}");
    }

    fn env_lines(out: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(out).lines().map(str::to_string).collect()
    }

    #[test]
    fn docker_gets_only_the_local_socket_and_an_empty_private_config() {
        let (d, env) = fake(Program::Docker, "tr '\\0' '\\n' < /proc/$$/environ\necho ---\nls -A \"$DOCKER_CONFIG\"\necho end");
        let r = run_fake(&env, &argv(Program::Docker, &["ps"]), &limits(3000, 4096), &flag(), &TIMING);
        assert_eq!((r.reason.clone(), r.exit), (Reason::Ok, Some(Exit::Code(0))), "{:?}", r.stderr);
        let lines = env_lines(&r.stdout.bytes);
        let split = lines.iter().position(|l| l == "---").unwrap();
        let mut got: Vec<String> = lines[..split].to_vec();
        got.sort();
        let config = d.path().join("run").join(DOCKER_CONFIG_SUBDIR);
        let mut want: Vec<String> = FIXED_ENV.iter().chain(DOCKER_ENV).map(|(k, v)| format!("{k}={v}")).collect();
        want.push(format!("HOME={}", d.path().join("home").display()));
        want.push(format!("XDG_RUNTIME_DIR={}", d.path().join("run").display()));
        want.push(format!("DOCKER_CONFIG={}", config.display()));
        want.sort();
        assert_eq!(got, want);
        assert!(want.contains(&"DOCKER_HOST=unix:///var/run/docker.sock".to_string()));
        assert!(want.contains(&"DOCKER_CLI_HOOKS=false".to_string()));
        assert_eq!(&lines[split + 1..], ["end"], "config directory is empty");
        let meta = fs::symlink_metadata(&config).unwrap();
        assert!(meta.is_dir());
        assert_eq!(meta.mode() & 0o7777, 0o500);

        // Andere Programme sehen nichts von Docker.
        let (_d2, env2) = fake(Program::Systemctl, "tr '\\0' '\\n' < /proc/$$/environ");
        let r = run_fake(&env2, &argv(Program::Systemctl, &[]), &limits(3000, 4096), &flag(), &TIMING);
        assert!(env_lines(&r.stdout.bytes).iter().all(|l| !l.starts_with("DOCKER_")), "{:?}", r.stdout);

        // Ohne XDG_RUNTIME_DIR kein Docker.
        let no_run = env.with_runtime_dir(None).unwrap();
        let r = run_fake(&no_run, &argv(Program::Docker, &["ps"]), &limits(3000, 64), &flag(), &TIMING);
        assert!(matches!(r.reason, Reason::SpawnError { kind: io::ErrorKind::NotFound, .. }), "{:?}", r.reason);
        assert_eq!(r.pid, None);
    }

    #[test]
    fn docker_config_that_is_not_empty_or_not_a_directory_blocks_the_start() {
        let (d, env) = fake(Program::Docker, "echo ran > \"$1\"");
        let mark = d.path().join("ran");
        let a = argv(Program::Docker, &[mark.to_str().unwrap()]);
        let config = d.path().join("run").join(DOCKER_CONFIG_SUBDIR);
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.json"), "{\"currentContext\":\"remote\"}").unwrap();
        let r = run_fake(&env, &a, &limits(3000, 64), &flag(), &TIMING);
        assert!(matches!(&r.reason, Reason::SpawnError { kind: io::ErrorKind::PermissionDenied, message } if message.contains("not empty")), "{:?}", r.reason);

        fs::remove_file(config.join("config.json")).unwrap();
        fs::remove_dir(&config).unwrap();
        let elsewhere = d.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &config).unwrap();
        let r = run_fake(&env, &a, &limits(3000, 64), &flag(), &TIMING);
        assert!(matches!(&r.reason, Reason::SpawnError { message, .. } if message.contains("not a directory")), "{:?}", r.reason);
        assert!(!mark.exists(), "the fake never ran");

        // Ein leeres Verzeichnis mit anderem Modus wird auf 0500 gesetzt und benutzt.
        fs::remove_file(&config).unwrap();
        fs::create_dir(&config).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        let r = run_fake(&env, &a, &limits(3000, 64), &flag(), &TIMING);
        assert_eq!(r.exit, Some(Exit::Code(0)), "{:?}", r.reason);
        assert_eq!(fs::metadata(&config).unwrap().mode() & 0o7777, 0o500);
    }

    /// Öffnet `/dev/null` ohne `O_CLOEXEC`, wie es eine C-Bibliothek im Daemon tun könnte.
    fn leak_fd() -> OwnedFd {
        let fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY) };
        assert!(fd > 2);
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
        unsafe { OwnedFd::from_raw_fd(fd) }
    }

    #[test]
    fn inherited_descriptors_without_cloexec_do_not_reach_the_probe() {
        let leaked = leak_fd();
        // `ls` sieht 0, 1, 2 und den eigenen Verzeichnis-Deskriptor (die kleinste freie Nummer, 3).
        let (_d, env) = fake(Program::Lsblk, "exec ls -1 /proc/self/fd");
        let r = run_fake(&env, &argv(Program::Lsblk, &[]), &limits(3000, 4096), &flag(), &TIMING);
        assert_eq!(r.exit, Some(Exit::Code(0)), "{:?}", r.stderr);
        let fds = env_lines(&r.stdout.bytes);
        assert_eq!(fds, ["0", "1", "2", "3"], "leaked fd {}", leaked.as_raw_fd());
    }

    /// Führt `f` in einem geforkten Kind aus (nur async-signal-sichere Syscalls in `f`) und liefert dessen Exit-Code.
    fn in_fork(f: fn() -> i32) -> i32 {
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            let code = f();
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status), "status {status:#x}");
        libc::WEXITSTATUS(status)
    }

    #[test]
    fn cloexec_fallback_loop_marks_inherited_descriptors() {
        let leaked = leak_fd();
        static FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
        FD.store(leaked.as_raw_fd(), Ordering::SeqCst);
        let code = in_fork(|| {
            if cloexec_by_loop().is_err() {
                return 2;
            }
            let flags = unsafe { libc::fcntl(FD.load(Ordering::SeqCst), libc::F_GETFD) };
            if flags >= 0 && flags & libc::FD_CLOEXEC != 0 {
                0
            } else {
                1
            }
        });
        assert_eq!(code, 0);
        assert_eq!(unsafe { libc::fcntl(leaked.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0, "parent unchanged");
    }

    #[test]
    fn ignored_sigchld_is_detected_and_blocks_probes() {
        assert!(!sigchld_reaped_by_kernel(), "the test process must not ignore SIGCHLD");
        // Im geforkten Kind, damit die parallelen Tests ihre Kinder weiter selbst abholen.
        let code = in_fork(|| unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = libc::SIG_IGN;
            libc::sigaction(libc::SIGCHLD, &act, std::ptr::null_mut());
            let ignored = sigchld_reaped_by_kernel();
            act.sa_sigaction = libc::SIG_DFL;
            act.sa_flags = libc::SA_NOCLDWAIT;
            libc::sigaction(libc::SIGCHLD, &act, std::ptr::null_mut());
            let nocldwait = sigchld_reaped_by_kernel();
            act.sa_flags = 0;
            libc::sigaction(libc::SIGCHLD, &act, std::ptr::null_mut());
            let default = sigchld_reaped_by_kernel();
            i32::from(ignored) | i32::from(nocldwait) << 1 | i32::from(default) << 2
        });
        assert_eq!(code, 0b011);
    }

    #[test]
    fn setup_failure_after_the_start_kills_and_reaps_the_child() {
        let (d, env) = fake(Program::Ip, "trap '' TERM\nsleep 30 &\necho $! > \"$1\"\nwhile :; do sleep 0.05; done");
        let mark = d.path().join("child");
        let timing = Timing { fail_setup: true, ..TIMING };
        let r = run_fake(&env, &argv(Program::Ip, &[mark.to_str().unwrap()]), &limits(5000, 64), &flag(), &timing);
        assert!(matches!(&r.reason, Reason::SpawnError { message, .. } if message.contains("simulated")), "{:?}", r.reason);
        assert!(r.reaped);
        let pid = r.pid.expect("the child had started");
        assert!(!Path::new(&format!("/proc/{pid}")).exists(), "leader {pid} is neither running nor a zombie");
        if let Ok(s) = fs::read_to_string(&mark) {
            assert!(wait_dead(s.trim().parse().unwrap(), Duration::from_secs(2)), "child in the group survived");
        }
        assert!(r.duration < Duration::from_secs(1), "{:?}", r.duration);
    }
}
