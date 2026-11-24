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

//! Proben der Gruppe Boot: `boots`, `boot_end`, `kernel_errors`, `pstore`, `temps_tail`, `kernel_info`, `pacman_log`,
//! `driver_versions`. Der wichtigste Fall ist „Was war beim letzten Boot-Ende?“: die Freezes legen das ganze
//! System still, also zählt, was *danach* noch lesbar ist (Ende des Journals, pstore, die letzten Zeilen des
//! temp-loggers, Paketänderungen davor).

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{bindings, journal_probe, single, table};
use super::output::{Filter, ListedBoots, Part, PreviousBoot, Status};
use super::resolve::{self, BootEntry};
use super::text::{clean, collapse_repeats, cut_to_bytes, human_bytes, without_partial_first_line, LineFormat};
use super::time::{format_duration, parse_iso_prefix, Clock};
use crate::diagnose::argv::{expand_probe, Bindings};
use crate::diagnose::probe::{BootOffset, Lines, Probe};
use crate::diagnose::run::Keep;
use std::io;
use std::path::{Path, PathBuf};

/// Wie viele Boots `boots` zeigt (die jüngsten).
pub const BOOTS_SHOWN: usize = 10;
/// Wie viele Zeilen vom Ende des vorigen Boots `boots` für das Urteil „sauber oder nicht“ liest.
const VERDICT_LINES: Lines = Lines::N200;

// ---- boots ---------------------------------------------------------------------------------------------------------

/// Endete der Boot mit einem geordneten Herunterfahren? `journal_tail` ist das Ende seines Journals im Format
/// `journalctl -o short-iso`. Ein Absturz, ein Freeze oder ein Stromausfall hinterlässt keine dieser Spuren; ein Neustart
/// der Sitzung oder des Compositors im selben Boot ist kein Boot-Ende und gehört nicht hierher.
pub fn ended_cleanly(journal_tail: &str) -> bool {
    journal_tail.lines().any(|l| {
        l.contains("Journal stopped")
            || l.contains("systemd-shutdown[")
            || l.contains("systemd[1]: Shutting down.")
            || (l.contains("Reached target") && ["Shutdown", "Power-Off", "Reboot", "Halt", "Final Step"].iter().any(|t| l.contains(t)))
    })
}

fn describe_end(b: &BootEntry, clock: &Clock, running: bool) -> String {
    if running {
        "(running)".to_string()
    } else {
        clock.format(b.last)
    }
}

pub fn boots(run: &mut Run) -> ProbeResult {
    let all = resolve::boots(run)?;
    if all.is_empty() {
        run.out.push(Part::new("", "The journal lists no boots."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let shown: Vec<&BootEntry> = all.iter().rev().take(BOOTS_SHOWN).rev().collect();
    let clock = run.ctx.clock;
    let mut rows = vec![vec!["idx".to_string(), "boot id".into(), "first entry".into(), "last entry".into(), "duration".into()]];
    for b in &shown {
        let running = b.index == 0;
        let end = if running { run.ctx.clock.now() } else { b.last };
        rows.push(vec![
            b.index.to_string(),
            b.boot_id.chars().take(8).collect(),
            clock.format(b.first),
            describe_end(b, &clock, running),
            format_duration(end - b.first),
        ]);
    }
    let mut text = super::common::table(&rows);
    if all.len() > shown.len() {
        run.out.note(format!("{} older boots are not shown", all.len() - shown.len()));
    }
    // Urteil für den Boot davor: das Ende seines Journals (mit der Vorlage von `boot_end`)
    let mut previous = PreviousBoot::None;
    if all.iter().any(|b| b.index == -1) {
        let verdict = Probe::BootEnd { boot: BootOffset::PREVIOUS, lines: VERDICT_LINES };
        let argv = expand_probe(&verdict, &Bindings::default()).map_err(|e| Status::Failed(format!("internal error: {e}")))?.remove(0);
        let cmd = run.exec(&argv, RAW_LIMIT, Keep::Tail);
        match cmd.problem() {
            Some(Status::TimedOut) => return Err(Status::TimedOut),
            Some(Status::Cancelled) => return Err(Status::Cancelled),
            Some(_) => {
                previous = PreviousBoot::Unknown;
                text.push_str("\nboot -1: the end of its journal could not be read, so it is unknown whether it ended cleanly\n")
            }
            None => {
                if ended_cleanly(&cmd.text()) {
                    previous = PreviousBoot::Clean;
                    text.push_str("\nboot -1 ended cleanly (a shutdown sequence is in the last journal lines)\n");
                } else {
                    previous = PreviousBoot::Unclean;
                    text.push_str(&format!(
                        "\nboot -1 ended UNCLEANLY: the last {} journal lines hold no shutdown sequence (typical for a crash, a freeze or a power loss)\n",
                        VERDICT_LINES.count()
                    ));
                }
            }
        }
    }
    run.out.listed.boots = Some(ListedBoots { current_id: all.iter().find(|b| b.index == 0).map(|b| b.boot_id.clone()), previous });
    run.out.push(Part::new("", text));
    Ok(())
}

// ---- boot_end, kernel_errors ----------------------------------------------------------------------------------------

pub fn boot_end(run: &mut Run) -> ProbeResult {
    journal_probe(run, "The journal of this boot has no entries.")
}

pub fn kernel_errors(run: &mut Run) -> ProbeResult {
    journal_probe(run, "No kernel messages at warning level or worse in this boot.")
}

// ---- pstore ----------------------------------------------------------------------------------------------------------

/// Ein Absturzabbild, das pstore gerettet hat: ein Ordner mit `dmesg*`-Dateien.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dump {
    /// Zeit des Abbilds (Name des Ordners von `systemd-pstore`: Sekunden seit der Epoche).
    epoch: i64,
    dir: PathBuf,
    files: Vec<PathBuf>,
}

/// Höchstens so viele Abbilder zeigt `pstore` (die jüngsten im Fenster).
const MAX_DUMPS: usize = 3;
/// Höchstens so viel liest `pstore` je Abbild.
const DUMP_READ_LIMIT: usize = 512 * 1024;
/// Spielraum vor dem Beginn des Boots: der erste Journaleintrag kommt nach dem Start des Kernels.
const START_SLACK: i64 = 2;

fn find_dumps(run: &Run) -> io::Result<Vec<Dump>> {
    let base = run.ctx.paths.under_root("/var/lib/systemd/pstore");
    let mut dumps = Vec::new();
    let top = run.fs.read_dir(&base)?;
    for entry in top {
        if entry.symlink {
            continue;
        }
        if entry.is_dir {
            let Ok(epoch) = entry.name.parse::<i64>() else { continue };
            collect_leaf_dumps(run, &entry.path, epoch, 0, &mut dumps);
        } else if entry.is_file && entry.name.starts_with("dmesg") {
            let epoch =
                run.fs.file_meta(&entry.path).ok().and_then(|(_, m)| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            dumps.push(Dump { epoch, dir: base.clone(), files: vec![entry.path] });
        }
    }
    dumps.sort_by(|a, b| (a.epoch, &a.dir).cmp(&(b.epoch, &b.dir)));
    Ok(dumps)
}

fn collect_leaf_dumps(run: &Run, dir: &std::path::Path, epoch: i64, depth: usize, out: &mut Vec<Dump>) {
    let Ok(entries) = run.fs.read_dir(dir) else { return };
    let files: Vec<PathBuf> = entries.iter().filter(|e| !e.symlink && e.is_file && e.name.starts_with("dmesg")).map(|e| e.path.clone()).collect();
    if !files.is_empty() {
        out.push(Dump { epoch, dir: dir.to_path_buf(), files });
    }
    if depth < 3 {
        for e in entries.iter().filter(|e| !e.symlink && e.is_dir) {
            collect_leaf_dumps(run, &e.path, epoch, depth + 1, out);
        }
    }
}

/// Setzt ein Abbild aus Teilen zusammen. `systemd-pstore` legt `dmesg.txt` daneben; sonst gilt die Reihenfolge der Teile
/// (`Panic#2 Part18` ist der älteste Teil, `Part1` das Ende des Logs): absteigend nach der Nummer.
fn assemble_parts(parts: Vec<(Option<u32>, String)>) -> String {
    let mut parts = parts;
    parts.sort_by_key(|p| std::cmp::Reverse(p.0));
    parts.into_iter().map(|(_, body)| body).collect::<Vec<_>>().join("\n")
}

/// Kopfzeile eines Teils (`Oops#1 Part3`, `Panic#2 Part18`): die Nummer des Teils.
fn part_number(first_line: &str) -> Option<u32> {
    let (_, tail) = first_line.trim().split_once('#')?;
    let (_, part) = tail.split_once(" Part")?;
    part.trim().parse().ok()
}

enum DumpText {
    Text { text: String, files: usize },
    Unreadable(String),
}

fn read_dump(run: &Run, dump: &Dump) -> DumpText {
    let txt = dump.files.iter().find(|f| f.file_name().is_some_and(|n| n == "dmesg.txt"));
    let result: io::Result<(String, usize)> = (|| {
        if let Some(f) = txt {
            let read = run.fs.read_tail(f, DUMP_READ_LIMIT)?;
            return Ok((clean(without_partial_first_line(&read.bytes, read.truncated)), dump.files.len()));
        }
        let mut parts = Vec::new();
        let mut total = 0usize;
        for f in &dump.files {
            let read = run.fs.read(f, DUMP_READ_LIMIT)?;
            total += read.bytes.len();
            let text = clean(&read.bytes);
            let (first, body) = text.split_once('\n').unwrap_or((&text, ""));
            match part_number(first) {
                Some(n) => parts.push((Some(n), body.to_string())),
                None => parts.push((None, text.clone())),
            }
            if total > DUMP_READ_LIMIT * 2 {
                break;
            }
        }
        Ok((assemble_parts(parts), dump.files.len()))
    })();
    match result {
        Ok((text, files)) => DumpText::Text { text, files },
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => DumpText::Unreadable("permission denied".into()),
        Err(e) => DumpText::Unreadable(e.to_string()),
    }
}

pub fn pstore(run: &mut Run) -> ProbeResult {
    let offset = run.probe.boot().expect("pstore has a boot");
    bindings(run)?;
    let window = resolve::boot_window(run, offset)?;
    let all = match find_dumps(run) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            run.out.push(Part::new("", "No pstore directory (/var/lib/systemd/pstore): the kernel has saved no crash dumps on this system."));
            run.out.status = Status::Empty;
            return Ok(());
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(Status::Unavailable("the pstore directory cannot be listed by this user".into())),
        Err(e) => return Err(Status::Failed(format!("cannot list the pstore directory: {e}"))),
    };
    run.check()?;
    // Ein Absturz in diesem Boot wird erst im nächsten gesichert: das Abbild trägt die Zeit des Absturzes, also liegt es
    // zwischen dem Beginn dieses Boots und dem Beginn des nächsten
    let from = window.start - START_SLACK;
    let to = window.next_start;
    let inside: Vec<&Dump> = all.iter().filter(|d| d.epoch >= from && d.epoch < to.max(from + 1)).collect();
    if inside.is_empty() {
        let others = all.len();
        let mut msg = format!("No pstore dump between {} and {}.", run.ctx.clock.format(window.start), run.ctx.clock.format(window.next_start));
        if others > 0 {
            msg.push_str(&format!(" ({others} dump(s) from other boots are kept.)"));
        }
        run.out.push(Part::new("", msg));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let clock = run.ctx.clock;
    let skipped = inside.len().saturating_sub(MAX_DUMPS);
    let chosen: Vec<&Dump> = inside.iter().rev().take(MAX_DUMPS).rev().copied().collect();
    if skipped > 0 {
        run.out.note(format!("{skipped} older dump(s) in this window are not shown"));
    }
    let limit = run.spec.limit_bytes.saturating_sub(512);
    let mut unreadable = 0usize;
    for (i, dump) in chosen.iter().enumerate() {
        run.check()?;
        let budget = limit / (chosen.len() - i).max(1);
        let name = dump.dir.strip_prefix(run.ctx.paths.under_root("/var/lib/systemd/pstore")).unwrap_or(&dump.dir).display().to_string();
        let label = format!("dump {} ({name})", clock.format(dump.epoch));
        match read_dump(run, dump) {
            DumpText::Text { text, files } => {
                let collapsed = collapse_repeats(&text, LineFormat::Plain);
                // Bei einem Absturz zählt das Ende des Logs (die Oops-Meldung samt Aufrufliste)
                let cut = cut_to_bytes(collapsed.text.trim_end(), budget, Keep::Tail);
                if cut.omitted_lines > 0 {
                    run.out.note(format!(
                        "{}: {} earlier lines were left out to stay within {}",
                        clock.format(dump.epoch),
                        cut.omitted_lines,
                        human_bytes(budget as u64)
                    ));
                }
                run.out.raw_bytes += text.len() as u64;
                let body = if cut.text.trim().is_empty() { format!("(empty dump, {files} file(s))") } else { cut.text };
                run.out.push(Part::new(&label, body));
            }
            DumpText::Unreadable(why) => {
                unreadable += 1;
                let size: u64 = dump.files.iter().filter_map(|f| run.fs.file_meta(f).ok()).map(|(len, _)| len).sum();
                run.out.push(Part::new(
                    &label,
                    format!("{} file(s), {} in total, cannot be read ({why}). The kernel saved a crash dump at this time. The files are normally readable by root only; dictate never uses sudo, so read them yourself.", dump.files.len(), human_bytes(size)),
                ));
            }
        }
    }
    if unreadable > 0 {
        run.out.status = if unreadable == chosen.len() {
            Status::Failed(format!("{unreadable} crash dump(s) exist but are not readable by this user"))
        } else {
            Status::Partial(format!("{unreadable} crash dump(s) are not readable by this user"))
        };
        if unreadable == chosen.len() {
            // Die Metadaten stehen trotzdem im Text: das Abbild gab es
            return Err(run.out.status.clone());
        }
    }
    Ok(())
}

// ---- temps_tail --------------------------------------------------------------------------------------------------------

/// Höchstens so viele Zeilen zeigt `temps_tail`.
const MAX_TEMP_ROWS: usize = 240;
/// Höchstens so viel einer Tagesdatei liest `temps_tail`.
const TEMP_FILE_LIMIT: usize = 2 * 1024 * 1024;

pub fn temps_tail(run: &mut Run) -> ProbeResult {
    // Ein Temperaturlogger ist kein Teil von dictate: ohne eingetragenen Ordner sagt die Probe es dem Modell
    let Some(dir) = run.ctx.paths.temp_log_dir().map(Path::to_path_buf) else {
        return Err(Status::Unavailable("no temperature log is set up (diagnose.temp_log_dir is empty)".into()));
    };
    let offset = run.probe.boot().expect("boot");
    let minutes = run.probe.window_min().expect("window").value();
    bindings(run)?;
    let window = resolve::boot_window(run, offset)?;
    // Fenster: die letzten `minutes` bis zum Ende des Boots; nach einem Absturz laufen die Zeilen des Loggers noch bis
    // zum Absturz weiter (das Journal endet bis zu 65 s früher), also bis zum Beginn des nächsten Boots
    let to = window.next_start;
    let from = window.end - minutes * 60;
    let clock = run.ctx.clock;
    let first_day = clock.civil(from);
    let last_day = clock.civil(to);
    let mut header: Option<String> = None;
    let mut rows: Vec<(i64, String)> = Vec::new();
    let mut files_seen = 0usize;
    let mut missing: Vec<String> = Vec::new();
    // Tage von `from` bis `to` (höchstens drei: ein Fenster ist höchstens eine Stunde plus die Zeit bis zum nächsten Boot)
    let mut day = super::time::days_from_civil(first_day.year, first_day.month, first_day.day);
    let last = super::time::days_from_civil(last_day.year, last_day.month, last_day.day);
    let mut guard = 0;
    while day <= last && guard < 3 {
        guard += 1;
        let (y, m, d) = super::time::civil_from_days(day);
        let name = format!("temps-{y:04}-{m:02}-{d:02}.csv");
        let path = dir.join(&name);
        match run.fs.read_tail(&path, TEMP_FILE_LIMIT) {
            Ok(read) => {
                files_seen += 1;
                run.out.raw_bytes += read.bytes.len() as u64;
                let text = String::from_utf8_lossy(&read.bytes).into_owned();
                for (i, line) in text.lines().enumerate() {
                    if line.starts_with("zeit,") || line.starts_with("time,") {
                        header.get_or_insert_with(|| line.to_string());
                        continue;
                    }
                    if i == 0 && read.truncated {
                        continue; // angeschnittene erste Zeile
                    }
                    let stamp = line.split(',').next().unwrap_or("");
                    if let Some(t) = clock.parse_local(stamp) {
                        if t >= from && t < to.max(from + 1) {
                            rows.push((t, line.to_string()));
                        }
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => missing.push(name),
            Err(e) => return Err(Status::Failed(format!("cannot read {name}: {e}"))),
        }
        run.check()?;
        day += 1;
    }
    if files_seen == 0 {
        return Err(Status::Unavailable(format!("no temperature log for this time: {} not found in the configured directory", missing.join(", "))));
    }
    if rows.is_empty() {
        run.out.push(Part::new("", format!("The temperature log has no rows between {} and {}.", clock.format(from), clock.format(to))));
        run.out.status = Status::Empty;
        return Ok(());
    }
    rows.sort_by_key(|(t, _)| *t);
    let total = rows.len();
    if total > MAX_TEMP_ROWS {
        rows.drain(..total - MAX_TEMP_ROWS);
    }
    let last_row = rows.last().map(|(t, _)| *t).expect("not empty");
    let mut text = String::new();
    if let Some(h) = &header {
        text.push_str(h);
        text.push('\n');
    }
    // Eine angeschnittene letzte Zeile (Absturz mitten im Schreiben) sagt es
    let columns = header.as_ref().map(|h| h.split(',').count());
    for (i, (_, line)) in rows.iter().enumerate() {
        text.push_str(line);
        if i + 1 == rows.len() && columns.is_some_and(|c| line.split(',').count() < c) {
            text.push_str("  (incomplete line)");
        }
        text.push('\n');
    }
    if offset.offset() < 0 {
        let after = last_row - window.end;
        text.push_str(&format!(
            "last row {} is {} {} the last journal entry of the boot ({})\n",
            clock.format(last_row),
            format_duration(after.abs()),
            if after >= 0 { "after" } else { "before" },
            clock.format(window.end)
        ));
    } else {
        text.push_str(&format!("last row {} is {} old\n", clock.format(last_row), format_duration(clock.now() - last_row)));
    }
    let cut = cut_to_bytes(text.trim_end(), run.spec.limit_bytes.saturating_sub(128), Keep::Tail);
    if total > MAX_TEMP_ROWS {
        run.out.note(format!("{} older rows are not shown", total - MAX_TEMP_ROWS));
    }
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} earlier lines were left out to stay within the limit", cut.omitted_lines));
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- kernel_info -----------------------------------------------------------------------------------------------------

/// Namensteile von Kernel-Parametern, die einen Schlüssel oder ein Kennwort vermuten lassen.
const SECRET_PARAM_HINTS: &[&str] = &["pass", "key", "secret", "token", "cred", "psk", "pwd", "auth"];
/// Parameter, die den Hinweis enthalten, aber keine Geheimnisse sind.
const HARMLESS_PARAMS: &[&str] = &["keymap", "keyboard", "keytable", "passthrough", "authorized"];

/// Sieht der Name eines Parameters (oder einer Variablen in `systemd.setenv=NAME=…`) nach einem Geheimnis aus?
pub fn secret_param_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_PARAM_HINTS.iter().any(|h| lower.contains(h)) && !HARMLESS_PARAMS.iter().any(|h| lower.contains(h))
}

/// Schwärzt die Werte von Kernel-Parametern, deren Name nach einem Geheimnis aussieht (`luks.key=…`, `password=…`);
/// liefert den Text und die Zahl der Schwärzungen.
pub fn redact_cmdline(cmdline: &str) -> (String, usize) {
    let mut count = 0;
    let words: Vec<String> = cmdline
        .split_whitespace()
        .map(|word| match word.split_once('=') {
            Some((name, value)) if secret_param_name(name) && !value.starts_with("[REDACTED:") => {
                count += 1;
                format!("{name}=[REDACTED:kernel_param]")
            }
            _ => word.to_string(),
        })
        .collect();
    (words.join(" "), count)
}

pub fn kernel_info(run: &mut Run) -> ProbeResult {
    let items: [(&str, &str, usize); 3] =
        [("version", "/proc/version", 1024), ("cmdline", "/proc/cmdline", 4096), ("bios", "/sys/class/dmi/id/bios_version", 256)];
    let mut missing = Vec::new();
    for (label, path, max) in items {
        run.check()?;
        let full = run.ctx.paths.under_root(path);
        match run.fs.read(&full, max) {
            Ok(read) => {
                let text = clean(&read.bytes).trim().to_string();
                if text.is_empty() {
                    continue;
                }
                if label == "cmdline" {
                    let (redacted, n) = redact_cmdline(&text);
                    run.out.redacted("kernel_param", n);
                    run.out.push(Part::new(label, redacted).with_filter(Filter::KernelCmdline));
                } else {
                    run.out.push(Part::new(label, text));
                }
            }
            Err(e) => missing.push(format!("{label}: {}", if e.kind() == io::ErrorKind::NotFound { "not available".to_string() } else { e.to_string() })),
        }
    }
    for m in missing {
        run.out.note(m);
    }
    if run.out.parts.is_empty() {
        return Err(Status::Failed("none of /proc/version, /proc/cmdline and the BIOS version could be read".into()));
    }
    Ok(())
}

// ---- pacman_log ------------------------------------------------------------------------------------------------------

/// Höchstens so viel vom Ende des Logs wird durchsucht.
const PACMAN_SCAN_LIMIT: u64 = 32 * 1024 * 1024;
/// Pakete, die bei einem Absturz zuerst interessieren (Kernel, Treiber, Grafik, Dienste).
const IMPORTANT_PACKAGES: &[&str] = &[
    "linux",
    "nvidia",
    "mesa",
    "systemd",
    "docker",
    "cuda",
    "xorg",
    "libdrm",
    "firmware",
    "wayland",
    "vulkan",
    "glibc",
    "gcc-libs",
    "mkinitcpio",
    "dkms",
    "grub",
    "cifs",
    "samba",
    "containerd",
];

/// Zeit einer Zeile von `pacman.log`: `[2026-11-03T22:15:01+0100]` oder die ältere Form `[2024-06-18 10:11]`.
fn pacman_time(line: &str, clock: &Clock) -> Option<(i64, usize)> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let inner = &rest[..end];
    let secs = match parse_iso_prefix(inner) {
        Some((s, _, _)) => s,
        None => clock.parse_local(&format!("{inner}:00")).or_else(|| clock.parse_local(inner))?,
    };
    Some((secs, end + 2))
}

/// Eine Paketänderung: `(Zeit, Text, wichtig)` aus einer Zeile von `pacman.log`, falls es eine ist.
fn pacman_change(line: &str, clock: &Clock) -> Option<(i64, String, bool)> {
    let (secs, skip) = pacman_time(line, clock)?;
    let body = line.get(skip..)?.trim_start().strip_prefix("[ALPM] ")?;
    let (action, rest) = body.split_once(' ')?;
    if !matches!(action, "upgraded" | "installed" | "removed" | "downgraded" | "reinstalled") {
        return None;
    }
    let package = rest.split_whitespace().next().unwrap_or("");
    let important = IMPORTANT_PACKAGES.iter().any(|p| package.starts_with(p));
    Some((secs, format!("{} {body}", clock.format(secs)), important))
}

/// Wählt aus `lines` (Zeit, Text, wichtig) so viele, wie in `budget` passen: alles, wenn es passt; sonst zuerst die
/// wichtigen Pakete, dann die neuesten übrigen. Die Reihenfolge bleibt chronologisch.
fn select_changes(lines: &[(i64, String, bool)], budget: usize) -> (Vec<&str>, usize) {
    let size = |l: &(i64, String, bool)| l.1.len() + 1;
    if lines.iter().map(size).sum::<usize>() <= budget {
        return (lines.iter().map(|l| l.1.as_str()).collect(), 0);
    }
    let mut keep = vec![false; lines.len()];
    let mut used = 0usize;
    for pass in [true, false] {
        for i in (0..lines.len()).rev() {
            if lines[i].2 == pass && !keep[i] && used + size(&lines[i]) <= budget {
                keep[i] = true;
                used += size(&lines[i]);
            }
        }
    }
    let chosen: Vec<&str> = lines.iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| l.1.as_str()).collect();
    let left_out = lines.len() - chosen.len();
    (chosen, left_out)
}

pub fn pacman_log(run: &mut Run) -> ProbeResult {
    let offset = run.probe.boot().expect("boot");
    let days = run.probe.days().expect("days").value();
    bindings(run)?;
    let window = resolve::boot_window(run, offset)?;
    let from = window.end - days * super::time::DAY;
    let to = window.end;
    let path = run.ctx.paths.under_root("/var/log/pacman.log");
    let mut file = match run.fs.open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(Status::Unavailable("there is no /var/log/pacman.log (this is not an Arch Linux system, or the log was removed)".into()))
        }
        Err(e) => return Err(Status::Failed(format!("cannot read /var/log/pacman.log: {e}"))),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(PACMAN_SCAN_LIMIT);
    {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(start)).map_err(|e| Status::Failed(format!("cannot read /var/log/pacman.log: {e}")))?;
    }
    let clock = run.ctx.clock;
    let mut changes: Vec<(i64, String, bool)> = Vec::new();
    let mut scanned = 0u64;
    {
        use std::io::{BufRead, BufReader};
        let mut reader = BufReader::new(file);
        let mut buf = Vec::new();
        let mut first = start > 0;
        let mut counter = 0usize;
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf).map_err(|e| Status::Failed(format!("cannot read /var/log/pacman.log: {e}")))?;
            if n == 0 {
                break;
            }
            scanned += n as u64;
            counter += 1;
            if counter.is_multiple_of(4096) {
                run.check()?;
            }
            if first {
                first = false; // angeschnittene erste Zeile
                continue;
            }
            let line = String::from_utf8_lossy(&buf);
            if let Some((t, text, important)) = pacman_change(line.trim_end(), &clock) {
                if t >= from && t <= to {
                    changes.push((t, text, important));
                }
            }
        }
    }
    run.out.raw_bytes += scanned;
    if start > 0 {
        run.out.note(format!("only the last {} of the log were searched", human_bytes(PACMAN_SCAN_LIMIT)));
    }
    if changes.is_empty() {
        run.out.push(Part::new("", format!("No package was installed, upgraded or removed between {} and {}.", clock.format(from), clock.format(to))));
        run.out.status = Status::Empty;
        return Ok(());
    }
    changes.sort_by_key(|c| c.0);
    let budget = run.spec.limit_bytes.saturating_sub(256);
    let (chosen, left_out) = select_changes(&changes, budget);
    if left_out > 0 {
        run.out.note(format!("{left_out} package changes of less interest were left out (kernel, driver and system packages are kept)"));
    }
    run.out.push(Part::new("", chosen.join("\n")));
    Ok(())
}

// ---- driver_versions -------------------------------------------------------------------------------------------------

pub fn driver_versions(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit, Keep::Head)?;
    let out = cmd.text();
    let errors = clean(&cmd.stderr);
    let wanted = ["linux", "nvidia-open-dkms", "nvidia-utils", "mesa", "docker", "systemd"];
    let mut rows = vec![vec!["package".to_string(), "version".into()]];
    for name in wanted {
        let version = out.lines().find_map(|l| l.strip_prefix(name).and_then(|rest| rest.strip_prefix(' ')).map(str::trim));
        match version {
            Some(v) => rows.push(vec![name.to_string(), v.to_string()]),
            None if errors.contains(&format!("'{name}'")) => rows.push(vec![name.to_string(), "not installed".to_string()]),
            None => rows.push(vec![name.to_string(), "unknown".to_string()]),
        }
    }
    run.out.push(Part::new("", table(&rows)));
    Ok(())
}
