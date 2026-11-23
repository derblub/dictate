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

//! Proben der Gruppe Docker: `docker_ps`, `docker_state`, `docker_logs`, `docker_df`, `docker_stats`.
//!
//! Der Docker-Socket ist root-gleich; darum kennt die Vorlage nur lesende Abfragen, und aus ihren Ausgaben entfernt Rust,
//! was Geheimnisse enthalten kann: `Labels` und `Command` der Liste (Kommandozeilen und Labels tragen oft Zugangsdaten),
//! und `docker inspect` liest ohnehin nur eine feste Vorlage ohne `Env`, `Labels` und `Cmd`.

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{commands, plain_text_probe, single, table};
use super::json;
use super::output::{ListedContainer, Part, Status};
use super::text::{clean, collapse_repeats, cut_to_bytes, human_bytes, LineFormat};
use crate::diagnose::run::Keep;
use serde_json::Value;

const PORTS_CHARS: usize = 60;

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

// ---- docker_ps -------------------------------------------------------------------------------------------------------

pub fn docker_ps(run: &mut Run) -> ProbeResult {
    let cmd = single(run, RAW_LIMIT, Keep::Head)?;
    let text = cmd.raw_text();
    let (items, incomplete) = json::objects(&text);
    if items.is_empty() {
        run.out.push(Part::new("", "There are no containers."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let mut labels = 0usize;
    let mut commands = 0usize;
    let mut states: std::collections::BTreeMap<String, usize> = Default::default();
    let mut rows = vec![vec!["name".to_string(), "id".into(), "state".into(), "status".into(), "image".into(), "ports".into()]];
    for o in &items {
        // Für die Profile (Aufgabe 5): erster Name, Zustand und Status; Docker schreibt Namen ohne Schrägstrich
        if let Some(name) = json::str_of(o, "Names").and_then(|n| n.split(',').map(|n| n.trim().trim_start_matches('/')).find(|n| !n.is_empty())) {
            run.out.listed.containers.push(ListedContainer {
                name: name.to_string(),
                state: json::str_of(o, "State").unwrap_or("").to_string(),
                status: json::str_of(o, "Status").unwrap_or("").to_string(),
            });
        }
        // Diese Felder werden nie gelesen, nur gezählt (für die Vorschau)
        labels += usize::from(json::str_of(o, "Labels").is_some_and(|l| !l.is_empty()));
        commands += usize::from(json::str_of(o, "Command").is_some_and(|c| !c.is_empty() && c != "\"\""));
        let state = json::str_of(o, "State").unwrap_or("?");
        *states.entry(state.to_string()).or_default() += 1;
        let id = json::str_of(o, "ID").unwrap_or("");
        rows.push(vec![
            json::str_of(o, "Names").unwrap_or("?").to_string(),
            id.chars().take(12).collect(),
            state.to_string(),
            json::str_of(o, "Status").unwrap_or("").to_string(),
            json::str_of(o, "Image").unwrap_or("").to_string(),
            short(json::str_of(o, "Ports").unwrap_or(""), PORTS_CHARS),
        ]);
    }
    run.out.redacted("docker_labels", labels);
    run.out.redacted("docker_command", commands);
    let summary = states.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
    let body = format!("{} containers: {summary}\n{}", items.len(), table(&rows));
    if incomplete {
        run.out.note("the list was cut at the capture limit");
    }
    let cut = cut_to_bytes(body.trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} more lines are not shown", cut.omitted_lines));
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- docker_state ----------------------------------------------------------------------------------------------------

fn flag(v: &Value, key: &str) -> &'static str {
    match v.get(key).and_then(Value::as_bool) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "?",
    }
}

fn field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn docker_state(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit.max(RAW_LIMIT / 8), Keep::Head)?;
    let text = cmd.raw_text();
    let Some(doc) = json::document(&text) else {
        return Err(Status::Failed("docker inspect returned something that is not the expected JSON".into()));
    };
    let mut out = String::new();
    out.push_str(&format!(
        "status: {} (running {}, restarting {}, OOM killed {})\n",
        field(&doc, "status"),
        flag(&doc, "running"),
        flag(&doc, "restarting"),
        flag(&doc, "oom_killed")
    ));
    out.push_str(&format!("exit code: {}\n", field(&doc, "exit_code")));
    let error = field(&doc, "error");
    if !error.is_empty() {
        out.push_str(&format!("error: {}\n", short(&clean(error.as_bytes()), 300)));
    }
    out.push_str(&format!("started: {}\nfinished: {}\n", field(&doc, "started_at"), field(&doc, "finished_at")));
    let health = field(&doc, "health");
    if !health.is_empty() {
        out.push_str(&format!("health: {health} (failing streak {})\n", field(&doc, "health_failing_streak")));
    }
    out.push_str(&format!("restart count: {}\nimage: {}\n", field(&doc, "restart_count"), field(&doc, "image")));
    match doc.get("mounts").and_then(Value::as_array) {
        Some(mounts) if !mounts.is_empty() => {
            out.push_str("mounts:\n");
            for m in mounts.iter().take(40) {
                let rw = if m.get("rw").and_then(Value::as_bool) == Some(false) { "ro" } else { "rw" };
                out.push_str(&format!("  {} {} → {} ({rw})\n", field(m, "type"), field(m, "source"), field(m, "destination")));
            }
            if mounts.len() > 40 {
                out.push_str(&format!("  … {} more mounts\n", mounts.len() - 40));
            }
        }
        _ => out.push_str("mounts: none\n"),
    }
    let cut = cut_to_bytes(out.trim_end(), limit.saturating_sub(64), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- docker_logs -----------------------------------------------------------------------------------------------------

/// Der Zeitstempel (Schlüssel zum Sortieren) am Anfang einer Zeile von `docker logs --timestamps`.
fn stamp(line: &str) -> Option<&str> {
    let end = line.find(' ')?;
    let head = &line[..end];
    super::time::parse_iso_prefix(head).map(|_| head)
}

/// Mischt stdout und stderr eines Containers nach Zeitstempel (die Streams des Containers kommen getrennt an; ohne
/// Mischen ginge die Reihenfolge verloren). Zeilen ohne Zeitstempel bleiben bei der Zeile davor.
fn merge_streams(out: &str, err: &str) -> String {
    let mut rows: Vec<(String, usize, String)> = Vec::new();
    for (stream, text) in [out, err].into_iter().enumerate() {
        let mut key = String::new();
        for line in text.lines() {
            if let Some(ts) = stamp(line) {
                key = ts.to_string();
            }
            rows.push((key.clone(), stream, line.to_string()));
        }
    }
    // Stabil: gleiche Zeit behält die Reihenfolge innerhalb des Streams; stdout vor stderr
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut merged = String::new();
    for (_, _, line) in rows {
        merged.push_str(&line);
        merged.push('\n');
    }
    merged
}

pub fn docker_logs(run: &mut Run) -> ProbeResult {
    let argv = commands(run)?.remove(0);
    let cmd = run.exec_both(&argv, RAW_LIMIT, Keep::Tail);
    // Endet `docker logs` mit Fehler, ist stderr dessen Meldung; bei Exit-Code 0 ist stderr der Log des Containers
    if let Some(status) = cmd.problem() {
        return Err(status);
    }
    let merged = merge_streams(&cmd.text(), &super::text::clean(&cmd.stderr));
    if merged.trim().is_empty() {
        run.out.push(Part::new("", "The container wrote no log lines in this time window."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let collapsed = collapse_repeats(&merged, LineFormat::Timestamped);
    let budget = run.spec.limit_bytes.saturating_sub(256);
    let cut = cut_to_bytes(collapsed.text.trim_end(), budget, Keep::Tail);
    if let Some(note) = cmd.capture_cut_note(&human_bytes(cmd.stdout.len() as u64), "older lines are missing") {
        run.out.note(note);
    }
    if collapsed.saved_lines > 0 {
        run.out.note(format!("{} repeated lines were folded into counts", collapsed.saved_lines));
    }
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} earlier lines were left out to stay within {}", cut.omitted_lines, human_bytes(budget as u64)));
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- docker_df, docker_stats -----------------------------------------------------------------------------------------

pub fn docker_df(run: &mut Run) -> ProbeResult {
    plain_text_probe(run, "Docker reports no disk usage.")
}

pub fn docker_stats(run: &mut Run) -> ProbeResult {
    let cmd = single(run, RAW_LIMIT, Keep::Head)?;
    let text = cmd.raw_text();
    let (items, incomplete) = json::objects(&text);
    if items.is_empty() {
        run.out.push(Part::new("", "No container is running."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let mut rows = vec![vec!["name".to_string(), "cpu".into(), "memory".into(), "mem %".into(), "net i/o".into(), "block i/o".into(), "pids".into()]];
    for o in &items {
        rows.push(vec![
            json::str_of(o, "Name").unwrap_or("?").to_string(),
            json::str_of(o, "CPUPerc").unwrap_or("").to_string(),
            json::str_of(o, "MemUsage").unwrap_or("").to_string(),
            json::str_of(o, "MemPerc").unwrap_or("").to_string(),
            json::str_of(o, "NetIO").unwrap_or("").to_string(),
            json::str_of(o, "BlockIO").unwrap_or("").to_string(),
            json::str_of(o, "PIDs").unwrap_or("").to_string(),
        ]);
    }
    if incomplete {
        run.out.note("the list was cut at the capture limit");
    }
    let cut = cut_to_bytes(table(&rows).trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn streams_merge_by_timestamp_and_keep_continuations() {
        let out = "2026-11-04T08:00:01.000000000Z out one\n2026-11-04T08:00:03.000000000Z out three\n";
        let err = "2026-11-04T08:00:02.000000000Z err two\ncontinuation of two\n2026-11-04T08:00:04.000000000Z err four\n";
        let merged = merge_streams(out, err);
        let lines: Vec<&str> = merged.lines().collect();
        assert_eq!(
            lines,
            [
                "2026-11-04T08:00:01.000000000Z out one",
                "2026-11-04T08:00:02.000000000Z err two",
                "continuation of two",
                "2026-11-04T08:00:03.000000000Z out three",
                "2026-11-04T08:00:04.000000000Z err four"
            ]
        );
        assert_eq!(merge_streams("", ""), "");
    }
}
