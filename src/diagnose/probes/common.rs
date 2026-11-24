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

//! Gemeinsame Bausteine der Proben: Parameter auflösen und in argv einsetzen, Befehle ausführen, Journaltext
//! verdichten.

use super::collect::{Cmd, ProbeResult, Run};
use super::output::{Part, Status};
use super::resolve;
use super::text::{collapse_repeats, cut_to_bytes, human_bytes, LineFormat};
use crate::diagnose::argv::{expand_probe, Argv, Bindings};
use crate::diagnose::probe::{Probe, Since};
use crate::diagnose::run::Keep;

/// Löst alle Parameter der Probe gegen die echten Listen auf und liefert die Werte für die argv-Vorlagen. Was
/// aufgelöst wurde, steht in `run.out.resolved`. Ein Parameter, der auf nichts oder auf mehreres passt, ist
/// [`Status::Unavailable`] mit einer Meldung für das Modell.
pub fn bindings(run: &mut Run) -> Result<Bindings, Status> {
    let probe = run.probe;
    let mut b = Bindings::default();
    if let Some(offset) = probe.boot() {
        // Der Boot muss im Journal stehen (sonst scheitert `--boot=-5` mit einer Meldung, die niemand lesen soll)
        let window = resolve::boot_window(run, offset)?;
        run.out.resolved.push(resolve::describe_boot(&run.ctx.clock, i64::from(offset.offset()), &window));
    }
    if let Some(term) = probe.unit() {
        let unit = resolve::unit(run, term)?;
        run.out.resolved.push(resolve::describe_unit(term, &unit));
        b.unit = Some(unit);
    }
    if let Some(term) = probe.container() {
        let container = resolve::container(run, term)?;
        run.out.resolved.push(resolve::describe_container(term, &container));
        b.container_id = Some(container.id);
    }
    if let Some(term) = probe.target() {
        let address = resolve::ping_target(run.ctx, term)?;
        run.out.resolved.push(format!("target {} → {address}", term.as_str()));
        b.ping_target = Some(address);
    }
    // Nur `docker logs` und `coredumpctl` rechnen „seit dem Boot“ in Sekunden; `journalctl` kennt `--boot=0`
    if probe.since() == Some(Since::Boot) && matches!(probe, Probe::DockerLogs { .. } | Probe::Coredumps { .. }) {
        b.boot_age_secs = Some(resolve::boot_age_secs(run)?);
    }
    Ok(b)
}

/// Die fertigen Befehle der Probe aus dem Katalog.
pub fn commands(run: &mut Run) -> Result<Vec<Argv>, Status> {
    let b = bindings(run)?;
    expand_probe(run.probe, &b).map_err(|e| Status::Failed(format!("internal error: {e}")))
}

/// Führt den einzigen Befehl der Probe aus. `raw` ist die Grenze für stdout, `keep` was bei einem Überlauf bleibt.
pub fn single(run: &mut Run, raw: usize, keep: Keep) -> Result<Cmd, Status> {
    let argv = commands(run)?.into_iter().next().ok_or_else(|| Status::Failed("internal error: the probe has no command".into()))?;
    let cmd = run.exec(&argv, raw, keep);
    match cmd.problem() {
        Some(status) => Err(status),
        None => Ok(cmd),
    }
}

/// Journaltext der Probe als Teil: bereinigt, wiederholte Zeilen zusammengefasst, auf `budget` Bytes gekürzt (vorn, das
/// Ende bleibt). Hinweise (Kürzungen, Fehlermeldung des Programms) landen in `run.out.notes`. `None`, wenn das Journal
/// nichts enthielt.
pub fn log_part(run: &mut Run, cmd: &Cmd, format: LineFormat, label: &str, budget: usize) -> Option<Part> {
    let text = cmd.text();
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "-- No entries --" {
        if let Some(note) = cmd.exit_note() {
            run.out.note(note);
        }
        return None;
    }
    let collapsed = collapse_repeats(&text, format);
    let cut = cut_to_bytes(&collapsed.text, budget, Keep::Tail);
    if let Some(note) = cmd.capture_cut_note(&human_bytes(cmd.stdout.len() as u64), "older lines are missing") {
        run.out.note(note);
    }
    if collapsed.saved_lines > 0 {
        run.out.note(format!("{} repeated lines were folded into counts", collapsed.saved_lines));
    }
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} earlier lines were left out to stay within {}", cut.omitted_lines, human_bytes(budget as u64)));
    }
    if let Some(note) = cmd.exit_note() {
        run.out.note(note);
    }
    Some(Part::new(label, cut.text))
}

/// Hat der Befehl nichts gefunden? Leere Ausgabe, Exit-Code 1 und keine Meldung (so enden `journalctl --grep` und
/// `findmnt -t` ohne Treffer).
pub fn is_silent_no_match(cmd: &Cmd) -> bool {
    use crate::diagnose::run::Exit;
    cmd.stdout.iter().all(|b| b.is_ascii_whitespace()) && cmd.exit == Some(Exit::Code(1)) && cmd.stderr.iter().all(|b| b.is_ascii_whitespace())
}

/// Eine Probe, die nur ein Journal liest (`boot_end`, `kernel_errors`, `oom`, `unit_journal`).
pub fn journal_probe(run: &mut Run, empty: &str) -> ProbeResult {
    let argv = commands(run)?.into_iter().next().ok_or_else(|| Status::Failed("internal error: the probe has no command".into()))?;
    let cmd = run.exec(&argv, super::collect::RAW_LIMIT, Keep::Tail);
    // `journalctl --grep` ohne Treffer: keine Ausgabe, Exit-Code 1, keine Meldung
    if is_silent_no_match(&cmd) {
        run.out.push(Part::new("", empty));
        run.out.status = Status::Empty;
        return Ok(());
    }
    if let Some(status) = cmd.problem() {
        return Err(status);
    }
    let budget = run.spec.limit_bytes.saturating_sub(256);
    match log_part(run, &cmd, LineFormat::Journal, "", budget) {
        Some(part) => run.out.push(part),
        None => {
            run.out.push(Part::new("", empty));
            run.out.status = Status::Empty;
        }
    }
    Ok(())
}

/// Eine Probe mit einem Befehl, dessen Text unverändert (bereinigt, gekürzt) in den Bericht kommt (`docker_df`,
/// `net_dns`, `net_reach`, `unit_status` nach eigener Bereinigung).
pub fn plain_text_probe(run: &mut Run, empty: &str) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit, Keep::Head)?;
    let text = cmd.text();
    if text.trim().is_empty() {
        run.out.push(Part::new("", empty));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let cut = cut_to_bytes(text.trim_end(), limit.saturating_sub(256), Keep::Head);
    if cut.omitted_lines > 0 || cmd.truncated {
        run.out.note(format!("the output was cut to {}", human_bytes(limit as u64)));
    }
    if let (true, Some(note)) = (cmd.closed_early, &cmd.capture_note) {
        run.out.note(note.clone());
    }
    if let Some(note) = cmd.exit_note() {
        run.out.note(note);
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

/// Die Kopfzeile einer Tabelle und ihre Zeilen, spaltenweise ausgerichtet (Leerzeichen, keine Tabs).
pub fn table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            line.push_str(cell);
            if i + 1 < row.len() {
                for _ in cell.chars().count()..widths[i] + 2 {
                    line.push(' ');
                }
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_align_columns_without_trailing_blanks() {
        let rows = vec![vec!["name".to_string(), "state".to_string(), "x".to_string()], vec!["a-long-name".to_string(), "up".to_string(), String::new()]];
        let t = table(&rows);
        assert_eq!(t, "name         state  x\na-long-name  up\n");
        assert_eq!(table(&[]), "");
    }
}
