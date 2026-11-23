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

//! Ausgabe einer Probe: Text plus Metadaten. Das ist die Schnittstelle zu den folgenden Aufgaben: der allgemeine
//! Geheimnis-Filter und das Pseudonymisieren (Aufgabe 4) laufen über [`Part::text`] unter Beachtung von [`Part::filter`],
//! der Bericht (Aufgabe 5) baut aus [`ProbeOutput::text`], den Zahlen und [`ProbeOutput::redactions`] den Kopf des
//! Abschnitts („probe · Parameter · Zeilen · Bytes · Schwärzungen“).

use crate::diagnose::pseudo::Category;
use std::time::Duration;

/// Wie eine Probe endete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Daten gesammelt.
    Ok,
    /// Die Probe lief, es gibt nichts zu berichten (keine fehlgeschlagene Unit, kein pstore-Eintrag, …). Der Text sagt
    /// es in einem Satz.
    Empty,
    /// Ein Teil fehlt (ein Befehl scheiterte oder lief in die Frist), der Rest ist im Text. Die Meldung sagt, was fehlt.
    Partial(String),
    /// Keine Daten: der Befehl scheiterte oder die Ausgabe war nicht lesbar. Die Meldung nennt den Grund.
    Failed(String),
    /// Die Frist der Probe (oder des Gesamtbudgets) ist abgelaufen, ohne dass Daten vorliegen.
    TimedOut,
    /// Abgebrochen.
    Cancelled,
    /// Die Probe ist hier nicht möglich: Programm oder Datei fehlt, der Suchbegriff passt auf nichts oder auf mehrere
    /// Dinge, der Boot existiert nicht, das Ziel steht nicht in der Config. Die Meldung sagt es dem Modell.
    Unavailable(String),
    /// Die Probe erzeugt Netzverkehr und braucht die Bestätigung des Users (`net_reach`).
    NeedsConfirmation,
    /// Die Probe ist gar nicht erst gestartet worden, weil das Gesamtbudget der Sammlung (15 s) für ihre Frist samt
    /// Überlauf nicht mehr reichte. Die Meldung sagt es dem Modell, damit es „keine Daten“ nicht als „kein Befund“ liest.
    Skipped(String),
}

impl Status {
    /// Kurzer Name für Log und Kopf (`ok`, `empty`, `partial`, `failed`, `timeout`, `cancelled`, `unavailable`,
    /// `needs_confirmation`, `skipped`).
    pub fn label(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Empty => "empty",
            Status::Partial(_) => "partial",
            Status::Failed(_) => "failed",
            Status::TimedOut => "timeout",
            Status::Cancelled => "cancelled",
            Status::Unavailable(_) => "unavailable",
            Status::NeedsConfirmation => "needs_confirmation",
            Status::Skipped(_) => "skipped",
        }
    }

    /// Liegen brauchbare Daten im Text?
    pub fn has_data(&self) -> bool {
        matches!(self, Status::Ok | Status::Partial(_))
    }

    /// Die Meldung zum Status, falls es eine gibt.
    pub fn message(&self) -> Option<&str> {
        match self {
            Status::Partial(m) | Status::Failed(m) | Status::Unavailable(m) | Status::Skipped(m) => Some(m),
            _ => None,
        }
    }
}

/// Welcher Filter auf einen Teil laufen soll, über den allgemeinen hinaus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Der allgemeine Filter (Aufgabe 4) auf jedem Text.
    Standard,
    /// Kernel-Kommandozeile: kann Schlüssel und Kennwörter in beliebigen `name=wert`-Parametern enthalten. Die Probe
    /// schwärzt schon Parameter, deren Name danach aussieht ([`Redaction`] `kernel_param`); der Filter soll den Teil
    /// zusätzlich streng behandeln (jeden Wert hinter `=`, den er sonst durchließe, nach den Regeln für Schlüssel).
    KernelCmdline,
}

/// Die Marke, hinter der der Kernel (Journal, pstore) die Kernel-Kommandozeile in einer Zeile zitieren. Der
/// allgemeine Filter behandelt den Rest jeder solchen Zeile in jedem Abschnitt streng (`filter::cmdline_lines`).
pub const KERNEL_CMDLINE_MARK: &str = "Kernel command line:";

/// Eine Schwärzung, die die Probe selbst vorgenommen hat (vor dem allgemeinen Filter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// Art: `docker_labels`, `docker_command`, `mount_option`, `command_arguments`, `kernel_param`.
    pub kind: &'static str,
    pub count: usize,
}

/// Ein Stück der Ausgabe: ein Befehl oder eine Datei, mit kurzer Überschrift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Überschrift („system“, „user“, „addr“, „cmdline“); leer bei einer Probe mit einem Stück.
    pub label: String,
    pub text: String,
    pub filter: Filter,
}

impl Part {
    pub fn new(label: &str, text: impl Into<String>) -> Part {
        Part { label: label.to_string(), text: text.into(), filter: Filter::Standard }
    }

    pub fn with_filter(mut self, filter: Filter) -> Part {
        self.filter = filter;
        self
    }
}

/// Ein Container aus der Liste von `docker_ps`, so wie die Profile ihn brauchen (die Liste mit Zuständen steht im Text,
/// aber als Tabelle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedContainer {
    /// Erster Name ohne führenden Schrägstrich.
    pub name: String,
    /// `State` von Docker: `running`, `restarting`, `exited`, `dead`, `paused`, `created`, `removing`.
    pub state: String,
    /// `Status` von Docker („Exited (137) 3 hours ago“), nur für den Exit-Code.
    pub status: String,
}

/// Eine fehlgeschlagene Unit aus `failed_units`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedUnit {
    pub name: String,
    /// Gehört zum User-Manager.
    pub user: bool,
}

/// Wie der Boot vor dem laufenden endete, nach dem Ende seines Journals (`ended_cleanly`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviousBoot {
    /// Das Journal kennt keinen Boot davor (erster Boot, Journal bereinigt).
    None,
    /// Das Ende des Journals zeigt ein geordnetes Herunterfahren.
    Clean,
    /// Keine Spur eines Herunterfahrens: Absturz, Freeze oder Stromausfall.
    Unclean,
    /// Das Ende ließ sich nicht lesen.
    Unknown,
}

/// Der Boot und sein Vorgänger, wie `boots` sie sieht (für den Hinweis nach einem unsauberen Ende, Aufgabe 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedBoots {
    /// Kennung des laufenden Boots (Index 0 der Liste), `None`, wenn die Liste ihn nicht enthält.
    pub current_id: Option<String>,
    pub previous: PreviousBoot,
}

/// Was eine Probe strukturiert aufgelistet hat, für die Profile (Aufgabe 5): `docker_ps` nennt die Container, an denen
/// das Profil Docker `docker_state` und `docker_logs` anhängt; `failed_units` nennt die Units, aus denen der User für das
/// Profil Unit wählt. Nie im Text des Berichts. Namen sind fremder Text: wer sie benutzt, prüft sie als Suchbegriff
/// (`ContainerRef::new`, `UnitRef::new`) und löst sie wie jeden anderen Suchbegriff gegen die echten Listen auf.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listed {
    pub containers: Vec<ListedContainer>,
    pub failed_units: Vec<ListedUnit>,
    /// Aus `boots`: der laufende Boot und das Urteil über den davor.
    pub boots: Option<ListedBoots>,
}

/// Ergebnis einer Probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    /// Name der Probe (Schlüssel im Katalog).
    pub probe: &'static str,
    pub status: Status,
    /// Die Teile in der Reihenfolge der Befehle bzw. Dateien.
    pub parts: Vec<Part>,
    /// Hinweise zur Ausgabe („truncated …“, „… repeated …“, Fehlermeldungen des Programms), ein Satz je Eintrag.
    pub notes: Vec<String>,
    /// Was lokal aufgelöst wurde, ein Eintrag je Parameter („unit docker → docker.service (system)“,
    /// „boot -1 → 2026-11-04T08:12:01+0100 … 2026-11-04T09:46:55+0100“).
    pub resolved: Vec<String>,
    /// Schwärzungen der Probe selbst; der Bericht zeigt sie in der Vorschau.
    pub redactions: Vec<Redaction>,
    /// Namen, die die Probe strukturiert kennt (Rechner und Gegenstellen von `net_tailscale`, Server, Benutzer und
    /// Freigaben von `net_mounts`): das Pseudonymisieren lernt sie, statt den Text neu zu zerlegen. Nie loggen.
    pub names: Vec<(Category, String)>,
    /// Strukturierte Listen für die Profile (nicht im Text).
    pub listed: Listed,
    /// Bytes, die die Programme bzw. Dateien geliefert haben (vor jeder Kürzung).
    pub raw_bytes: u64,
    /// Wie lange die Probe gebraucht hat.
    pub duration: Duration,
}

impl ProbeOutput {
    pub fn new(probe: &'static str) -> ProbeOutput {
        ProbeOutput {
            probe,
            status: Status::Ok,
            parts: Vec::new(),
            notes: Vec::new(),
            resolved: Vec::new(),
            redactions: Vec::new(),
            names: Vec::new(),
            listed: Listed::default(),
            raw_bytes: 0,
            duration: Duration::ZERO,
        }
    }

    /// Eine Ausgabe ohne Daten mit diesem Status.
    pub fn without_data(probe: &'static str, status: Status) -> ProbeOutput {
        ProbeOutput { status, ..ProbeOutput::new(probe) }
    }

    pub fn push(&mut self, part: Part) {
        self.parts.push(part);
    }

    pub fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }

    /// Zählt eine Schwärzung der Probe (gleiche Arten werden addiert, Zahl 0 zählt nicht).
    pub fn redacted(&mut self, kind: &'static str, count: usize) {
        if count == 0 {
            return;
        }
        match self.redactions.iter_mut().find(|r| r.kind == kind) {
            Some(r) => r.count += count,
            None => self.redactions.push(Redaction { kind, count }),
        }
    }

    /// Der Text für den Bericht: die Teile (bei mehr als einem mit Überschrift `## label`), dann die Hinweise und, wenn
    /// keine Daten vorliegen, der Grund in einer Zeile. Das ist genau der Text, den Filter und Bericht weiterverarbeiten.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let labelled = self.parts.len() > 1;
        for part in &self.parts {
            if labelled || !part.label.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("## {}\n", part.label));
            }
            out.push_str(part.text.trim_end_matches('\n'));
            out.push('\n');
        }
        for note in &self.notes {
            out.push_str(&format!("[{note}]\n"));
        }
        match &self.status {
            Status::Ok => {}
            Status::Empty if out.is_empty() => out.push_str("(nothing to report)\n"),
            Status::Empty => {}
            Status::Partial(m) => out.push_str(&format!("(partial: {m})\n")),
            Status::Failed(m) => out.push_str(&format!("(failed: {m})\n")),
            Status::TimedOut => out.push_str("(timed out)\n"),
            Status::Cancelled => out.push_str("(cancelled)\n"),
            Status::Unavailable(m) => out.push_str(&format!("(unavailable: {m})\n")),
            Status::NeedsConfirmation => out.push_str("(needs the confirmation of the user: this probe sends network traffic)\n"),
            Status::Skipped(m) => out.push_str(&format!("(skipped: {m})\n")),
        }
        out.trim_end_matches('\n').to_string()
    }

    /// Zeilen des Berichtstextes.
    pub fn line_count(&self) -> usize {
        self.text().lines().count()
    }

    /// Bytes des Berichtstextes.
    pub fn byte_count(&self) -> usize {
        self.text().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_labels_notes_and_the_reason() {
        let mut o = ProbeOutput::new("failed_units");
        o.push(Part::new("system", "a.service failed\n"));
        o.push(Part::new("user", "none"));
        o.note("truncated: kept 10 of 99 bytes");
        assert_eq!(o.text(), "## system\na.service failed\n\n## user\nnone\n[truncated: kept 10 of 99 bytes]");
        assert_eq!(o.line_count(), 6);

        let mut single = ProbeOutput::new("mem");
        single.push(Part::new("", "MemTotal 1"));
        assert_eq!(single.text(), "MemTotal 1");

        let failed = ProbeOutput::without_data("docker_ps", Status::Failed("exit 1: Cannot connect".into()));
        assert_eq!(failed.text(), "(failed: exit 1: Cannot connect)");
        assert!(!failed.status.has_data());
        assert_eq!(failed.status.message(), Some("exit 1: Cannot connect"));
        assert_eq!(ProbeOutput::without_data("x", Status::TimedOut).text(), "(timed out)");
        assert_eq!(ProbeOutput::without_data("x", Status::Empty).text(), "(nothing to report)");
        assert!(ProbeOutput::without_data("x", Status::NeedsConfirmation).text().contains("confirmation"));
        let skipped = ProbeOutput::without_data("x", Status::Skipped("collection budget used up".into()));
        assert_eq!(skipped.text(), "(skipped: collection budget used up)");
        assert!(!skipped.status.has_data());
        assert_eq!(skipped.status.label(), "skipped");
        let mut partial = ProbeOutput::new("x");
        partial.status = Status::Partial("user manager not reachable".into());
        partial.push(Part::new("", "data"));
        assert_eq!(partial.text(), "data\n(partial: user manager not reachable)");
        assert!(partial.status.has_data());
    }

    #[test]
    fn redactions_add_up_by_kind() {
        let mut o = ProbeOutput::new("net_mounts");
        o.redacted("mount_option", 2);
        o.redacted("mount_option", 1);
        o.redacted("docker_labels", 0);
        assert_eq!(o.redactions, [Redaction { kind: "mount_option", count: 3 }]);
    }

    #[test]
    fn status_labels_are_distinct() {
        let all = [
            Status::Ok,
            Status::Empty,
            Status::Partial(String::new()),
            Status::Failed(String::new()),
            Status::TimedOut,
            Status::Cancelled,
            Status::Unavailable(String::new()),
            Status::NeedsConfirmation,
            Status::Skipped(String::new()),
        ];
        let labels: std::collections::BTreeSet<&str> = all.iter().map(Status::label).collect();
        assert_eq!(labels.len(), all.len());
    }
}
