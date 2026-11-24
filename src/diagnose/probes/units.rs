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

//! Proben der Gruppe Units: `failed_units`, `unit_status`, `unit_show`, `unit_journal`, `timers`, `coredumps`.

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{commands, journal_probe, single, table};
use super::json;
use super::output::{ListedUnit, Part, Status};
use super::text::{cut_to_bytes, human_bytes};
use super::time::format_duration;
use crate::diagnose::run::Keep;
use serde_json::Value;

/// Längste Beschreibung einer Unit im Bericht (Zeichen).
const DESCRIPTION_CHARS: usize = 100;

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

// ---- failed_units --------------------------------------------------------------------------------------------------

/// Die Tabelle fehlgeschlagener Units aus der JSON-Ausgabe von `systemctl --failed`.
fn failed_table(items: &[Value]) -> String {
    let mut rows = vec![vec!["unit".to_string(), "state".into(), "description".into()]];
    for o in items {
        let state = format!("{}/{}", json::str_of(o, "active").unwrap_or("?"), json::str_of(o, "sub").unwrap_or("?"));
        rows.push(vec![json::str_of(o, "unit").unwrap_or("?").to_string(), state, short(json::str_of(o, "description").unwrap_or(""), DESCRIPTION_CHARS)]);
    }
    table(&rows)
}

pub fn failed_units(run: &mut Run) -> ProbeResult {
    let argvs = commands(run)?;
    let labels = ["system", "user"];
    let mut failures: Vec<String> = Vec::new();
    let mut any_data = false;
    let mut used = 0usize;
    for (i, argv) in argvs.iter().enumerate() {
        let cap = run.share(used, i, argvs.len());
        let cmd = run.exec(argv, RAW_LIMIT, Keep::Head);
        match cmd.problem() {
            Some(Status::TimedOut) => return Err(Status::TimedOut),
            Some(Status::Cancelled) => return Err(Status::Cancelled),
            Some(other) => {
                failures.push(format!("{} manager: {}", labels[i], other.message().unwrap_or(other.label())));
                continue;
            }
            None => {}
        }
        let text = cmd.raw_text();
        let (items, incomplete) = json::objects(&text);
        if items.is_empty() && !text.trim().is_empty() && text.trim() != "[]" {
            failures.push(format!("{} manager: the list is not readable", labels[i]));
            continue;
        }
        any_data = true;
        // Für das Profil Unit (Aufgabe 5): die Namen, aus denen der User wählt
        for o in &items {
            if let Some(name) = json::str_of(o, "unit") {
                run.out.listed.failed_units.push(ListedUnit { name: name.to_string(), user: i == 1 });
            }
        }
        let body = if items.is_empty() { "No failed units.".to_string() } else { failed_table(&items) };
        let cut = cut_to_bytes(body.trim_end(), cap.saturating_sub(64), Keep::Head);
        used += cut.text.len();
        if incomplete {
            run.out.note(format!("the {} list was cut at the capture limit", labels[i]));
        }
        run.out.push(Part::new(labels[i], cut.text));
    }
    if !any_data {
        return Err(Status::Failed(failures.join("; ")));
    }
    if !failures.is_empty() {
        run.out.status = Status::Partial(failures.join("; "));
    } else if run.out.parts.iter().all(|p| p.text == "No failed units.") {
        run.out.status = Status::Empty;
    }
    Ok(())
}

// ---- unit_status -----------------------------------------------------------------------------------------------------

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Programmname aus dem Anfang einer Kommandozeile (`/usr/bin/dockerd -H fd://` wird `dockerd`, `sshd: user [priv]`
/// wird `sshd`) und ob Argumente weggefallen sind.
pub fn program_of(command_line: &str) -> (String, bool) {
    let mut words = command_line.split_whitespace();
    let first = words.next().unwrap_or("");
    let name = basename(first.trim_matches('"').trim_end_matches(':')).to_string();
    (name, words.next().is_some())
}

/// Entfernt aus der Ausgabe von `systemctl status` alles, was Argumente von Prozessen zeigt: im `CGroup:`-Baum bleibt je
/// Prozess PID und Programmname, in `Process:`-Zeilen (`ExecStart=…`) Programmname und Ergebnis. Liefert den Text und
/// die Zahl der Zeilen, aus denen Argumente entfernt wurden.
pub fn sanitize_status(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut redacted = 0usize;
    let mut in_cgroup = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("CGroup:") {
            in_cgroup = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_cgroup {
            // Baumzeichen in Unicode oder ASCII (`|-`, `` `- ``, ohne UTF-8-Locale); jede eingerückte Zeile, die danach mit
            // einer PID beginnt, wird gekürzt
            let glyph = |c: char| matches!(c, '├' | '└' | '│' | '─' | '|' | '`' | '-');
            let is_tree = line.starts_with(char::is_whitespace) && (trimmed.starts_with(glyph) || trimmed.starts_with(|c: char| c.is_ascii_digit()));
            if !is_tree {
                in_cgroup = false;
            } else {
                let glyphs_end = line.find(|c: char| !(c.is_whitespace() || glyph(c))).unwrap_or(line.len());
                let (prefix, rest) = line.split_at(glyphs_end);
                let pid_len = rest.bytes().take_while(u8::is_ascii_digit).count();
                if pid_len > 0 && rest[pid_len..].starts_with(' ') {
                    let (name, had_args) = program_of(&rest[pid_len + 1..]);
                    redacted += usize::from(had_args);
                    out.push_str(&format!("{prefix}{} {name}\n", &rest[..pid_len]));
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("Process:") {
            // `Process: 1234 ExecStart=/usr/bin/foo --arg (code=exited, status=0/SUCCESS)`
            let indent = &line[..line.len() - trimmed.len()];
            let rest = rest.trim_start();
            if let Some((pid, exec)) = rest.split_once(' ') {
                if let Some((key, value)) = exec.split_once('=') {
                    let (command, result) = match value.rsplit_once(" (") {
                        Some((c, r)) if r.ends_with(')') => (c, format!(" ({r}")),
                        _ => (value, String::new()),
                    };
                    let (name, had_args) = program_of(command);
                    redacted += usize::from(had_args);
                    out.push_str(&format!("{indent}Process: {pid} {key}={name}{result}\n"));
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, redacted)
}

pub fn unit_status(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit.max(RAW_LIMIT / 4), Keep::Head)?;
    let text = cmd.text();
    let (clean_text, n) = sanitize_status(&text);
    run.out.redacted("command_arguments", n);
    let cut = cut_to_bytes(clean_text.trim_end(), limit.saturating_sub(128), Keep::Head);
    if cut.omitted_lines > 0 {
        run.out.note(format!("the status was cut to {}", human_bytes(limit as u64)));
    }
    if let Some(note) = cmd.exit_note() {
        run.out.note(note);
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- unit_show -------------------------------------------------------------------------------------------------------

pub fn unit_show(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit, Keep::Head)?;
    let text = cmd.text();
    // Leere Werte (`ActiveExitTimestamp=`) sagen nichts
    let lines: Vec<&str> = text.lines().filter(|l| l.split_once('=').is_some_and(|(_, v)| !v.trim().is_empty())).collect();
    if lines.is_empty() {
        run.out.push(Part::new("", "The unit has no properties set."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let cut = cut_to_bytes(&lines.join("\n"), limit.saturating_sub(64), Keep::Head);
    if let Some(note) = cmd.exit_note() {
        run.out.note(note);
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

pub fn unit_journal(run: &mut Run) -> ProbeResult {
    journal_probe(run, "The unit has no journal lines in this time window.")
}

// ---- timers ----------------------------------------------------------------------------------------------------------

pub fn timers(run: &mut Run) -> ProbeResult {
    let cmd = single(run, RAW_LIMIT, Keep::Head)?;
    let text = cmd.raw_text();
    let (items, incomplete) = json::objects(&text);
    if items.is_empty() {
        run.out.push(Part::new("", "There are no timers."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let clock = run.ctx.clock;
    let micros = |o: &Value, key: &str| json::int_of(o, key).filter(|v| *v > 0).map(|v| v / 1_000_000);
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by_key(|o| micros(o, "next").unwrap_or(i64::MAX));
    let mut rows = vec![vec!["timer".to_string(), "next".into(), "last".into(), "activates".into()]];
    for o in sorted {
        let next = match micros(o, "next") {
            Some(t) => format!("{} (in {})", clock.format(t), format_duration(t - clock.now())),
            None => "-".to_string(),
        };
        let last = match micros(o, "last") {
            Some(t) => format!("{} ({} ago)", clock.format(t), format_duration(clock.now() - t)),
            None => "-".to_string(),
        };
        rows.push(vec![json::str_of(o, "unit").unwrap_or("?").to_string(), next, last, json::str_of(o, "activates").unwrap_or("").to_string()]);
    }
    if incomplete {
        run.out.note("the list was cut at the capture limit");
    }
    let cut = cut_to_bytes(table(&rows).trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} more timers are not shown", cut.omitted_lines));
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- coredumps -------------------------------------------------------------------------------------------------------

/// Name eines Signals (Linux x86-64) für die Anzeige.
pub fn signal_name(sig: i64) -> String {
    let name = match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        15 => "SIGTERM",
        24 => "SIGXCPU",
        25 => "SIGXFSZ",
        31 => "SIGSYS",
        _ => return format!("signal {sig}"),
    };
    name.to_string()
}

/// Wie viele Zeilen der Liste `coredumps` zeigt (die jüngsten).
const COREDUMPS_SHOWN: usize = 20;
/// Wie viele Gruppen der Zusammenfassung.
const COREDUMP_GROUPS: usize = 10;

pub fn coredumps(run: &mut Run) -> ProbeResult {
    let argv = commands(run)?.remove(0);
    // Die Liste läuft von alt nach neu: bei einem Überlauf zählen die jüngsten
    let cmd = run.exec(&argv, RAW_LIMIT, Keep::Tail);
    // `coredumpctl list` ohne Treffer endet mit Exit-Code 1 und „No coredumps found.“
    if cmd.stdout.iter().all(|b| b.is_ascii_whitespace()) && cmd.stderr_summary().is_some_and(|e| e.contains("No coredumps found")) {
        run.out.push(Part::new("", "No core dumps in this time window."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    if let Some(status) = cmd.problem() {
        return Err(status);
    }
    let text = cmd.raw_text();
    let (mut items, incomplete) = json::objects(&text);
    if items.is_empty() {
        run.out.push(Part::new("", "No core dumps in this time window."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let clock = run.ctx.clock;
    items.sort_by_key(|o| json::int_of(o, "time").unwrap_or(0));
    let secs = |o: &Value| json::int_of(o, "time").unwrap_or(0) / 1_000_000;
    // Zusammenfassung nach Programm und Signal
    let mut groups: Vec<(String, i64, usize, i64, i64)> = Vec::new();
    for o in &items {
        let exe = json::str_of(o, "exe").unwrap_or("?").to_string();
        let sig = json::int_of(o, "sig").unwrap_or(0);
        let t = secs(o);
        match groups.iter_mut().find(|g| g.0 == exe && g.1 == sig) {
            Some(g) => {
                g.2 += 1;
                g.3 = g.3.min(t);
                g.4 = g.4.max(t);
            }
            None => groups.push((exe, sig, 1, t, t)),
        }
    }
    groups.sort_by(|a, b| b.2.cmp(&a.2).then(b.4.cmp(&a.4)));
    let mut summary = vec![vec!["count".to_string(), "signal".into(), "program".into(), "first".into(), "last".into()]];
    for (exe, sig, count, first, last) in groups.iter().take(COREDUMP_GROUPS) {
        summary.push(vec![count.to_string(), signal_name(*sig), exe.clone(), clock.format(*first), clock.format(*last)]);
    }
    let mut listing = vec![vec!["time".to_string(), "signal".into(), "pid".into(), "program".into(), "core".into(), "size".into()]];
    for o in items.iter().rev().take(COREDUMPS_SHOWN).rev() {
        listing.push(vec![
            clock.format(secs(o)),
            signal_name(json::int_of(o, "sig").unwrap_or(0)),
            json::int_of(o, "pid").map(|p| p.to_string()).unwrap_or_default(),
            json::str_of(o, "exe").unwrap_or("?").to_string(),
            json::str_of(o, "corefile").unwrap_or("").to_string(),
            json::int_of(o, "size").map(|s| human_bytes(s.max(0) as u64)).unwrap_or_default(),
        ]);
    }
    let mut body = format!("{} core dumps\n{}\nlatest:\n{}", items.len(), table(&summary), table(&listing));
    if groups.len() > COREDUMP_GROUPS {
        body.push_str(&format!("({} more programs not listed)\n", groups.len() - COREDUMP_GROUPS));
    }
    if incomplete {
        run.out.note("the list was cut at the capture limit");
    }
    let cut = cut_to_bytes(body.trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// Security-Review N7: der CGroup-Baum in ASCII (ohne UTF-8-Locale) wird ebenso gekürzt.
    #[test]
    fn cgroup_tree_in_ascii_is_shortened_too() {
        let text = "● x.service - X\n     CGroup: /system.slice/x.service\n             |-1234 /usr/bin/x --token abc\n             `-1235 /usr/bin/y -p geheim\n              1236 /usr/bin/z --k v\n\nNov 05 10:00:00 host x[1234]: started\n";
        let (out, n) = sanitize_status(text);
        assert_eq!(n, 3);
        assert!(out.contains("|-1234 x\n") && out.contains("`-1235 y\n") && out.contains(" 1236 z\n"), "{out}");
        assert!(out.contains("Nov 05 10:00:00 host x[1234]: started"), "{out}");
        assert!(!out.contains("abc") && !out.contains("geheim"), "{out}");
    }
}
