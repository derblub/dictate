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

//! Proben der Gruppe Resources: `mem`, `load`, `disks`, `top_procs`, `oom`. Was Rust selbst liest (`/proc`), liest es mit
//! festen Pfaden und kleinen Grenzen; `top_procs` liest nie `cmdline` (Argumente tragen Geheimnisse), nur den Namen des
//! Programms.

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{commands, journal_probe, table};
use super::json;
use super::output::{Part, Status};
use super::text::{clean, cut_to_bytes, human_bytes};
use crate::diagnose::run::Keep;
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::time::Instant;

/// Größe, bis zu der eine Datei in `/proc` gelesen wird.
const PROC_READ: usize = 64 * 1024;

fn read_text(run: &Run, path: &str, max: usize) -> io::Result<String> {
    let full = run.ctx.paths.under_root(path);
    run.fs.read(&full, max).map(|r| String::from_utf8_lossy(&r.bytes).into_owned())
}

// ---- mem -------------------------------------------------------------------------------------------------------------

/// Werte aus `/proc/meminfo` in KiB.
fn meminfo_values(text: &str) -> HashMap<&str, u64> {
    text.lines()
        .filter_map(|l| {
            let (key, rest) = l.split_once(':')?;
            let n = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            Some((key.trim(), n))
        })
        .collect()
}

/// Die Zeilen von `mem`: wichtige Werte, Belegung und Swap.
pub fn meminfo_text(meminfo: &str) -> String {
    const KEYS: &[&str] = &[
        "MemTotal",
        "MemAvailable",
        "MemFree",
        "Buffers",
        "Cached",
        "AnonPages",
        "Shmem",
        "Dirty",
        "Writeback",
        "Slab",
        "SReclaimable",
        "SUnreclaim",
        "SwapTotal",
        "SwapFree",
        "CommitLimit",
        "Committed_AS",
    ];
    let values = meminfo_values(meminfo);
    let mut out = String::new();
    for key in KEYS {
        if let Some(kib) = values.get(key) {
            out.push_str(&format!("{key}: {}\n", human_bytes(kib * 1024)));
        }
    }
    if let (Some(total), Some(avail)) = (values.get("MemTotal"), values.get("MemAvailable")) {
        if *total > 0 {
            let used = total.saturating_sub(*avail);
            out.push_str(&format!("in use (total - available): {} = {:.0}%\n", human_bytes(used * 1024), used as f64 * 100.0 / *total as f64));
        }
    }
    if let (Some(total), Some(free)) = (values.get("SwapTotal"), values.get("SwapFree")) {
        if *total > 0 {
            let used = total.saturating_sub(*free);
            out.push_str(&format!("swap in use: {} = {:.0}%\n", human_bytes(used * 1024), used as f64 * 100.0 / *total as f64));
        }
    }
    out
}

/// Zeilen von `/proc/pressure/*` ohne die Gesamtzeit: `memory some avg10=0.00 avg60=0.00 avg300=0.00`.
pub fn pressure_text(name: &str, text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().filter(|f| !f.starts_with("total=")).collect();
        if fields.len() >= 2 {
            out.push_str(&format!("{name} pressure {}\n", fields.join(" ")));
        }
    }
    out
}

pub fn mem(run: &mut Run) -> ProbeResult {
    run.check()?;
    let meminfo = read_text(run, "/proc/meminfo", PROC_READ).map_err(|e| Status::Failed(format!("cannot read /proc/meminfo: {e}")))?;
    let mut text = meminfo_text(&meminfo);
    if text.is_empty() {
        return Err(Status::Failed("/proc/meminfo has no known values".into()));
    }
    let mut missing = Vec::new();
    for name in ["memory", "cpu", "io"] {
        match read_text(run, &format!("/proc/pressure/{name}"), 4096) {
            Ok(t) => text.push_str(&pressure_text(name, &t)),
            Err(_) => missing.push(name),
        }
    }
    if missing.len() == 3 {
        run.out.note("no pressure information (the kernel has no PSI or it is disabled)");
    }
    run.out.push(Part::new("", text));
    Ok(())
}

// ---- load ------------------------------------------------------------------------------------------------------------

/// Zähler einer `cpu`-Zeile von `/proc/stat`: user nice system idle iowait irq softirq steal.
type Ticks = [u64; 8];

fn parse_stat(text: &str) -> Vec<(String, Ticks)> {
    text.lines()
        .filter(|l| l.starts_with("cpu"))
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let name = f.next()?.to_string();
            let mut ticks = [0u64; 8];
            for t in ticks.iter_mut() {
                *t = f.next()?.parse().ok()?;
            }
            Some((name, ticks))
        })
        .collect()
}

/// Verteilung der CPU-Zeit zwischen zwei Messungen in Prozent.
#[derive(Debug, Clone, PartialEq)]
pub struct CpuUsage {
    pub user: f64,
    pub system: f64,
    pub iowait: f64,
    pub steal: f64,
    pub idle: f64,
}

impl CpuUsage {
    pub fn busy(&self) -> f64 {
        100.0 - self.idle - self.iowait
    }
}

fn usage_between(a: &Ticks, b: &Ticks) -> Option<CpuUsage> {
    let d: Vec<u64> = a.iter().zip(b).map(|(x, y)| y.saturating_sub(*x)).collect();
    let total: u64 = d.iter().sum();
    if total == 0 {
        return None;
    }
    let pct = |n: u64| n as f64 * 100.0 / total as f64;
    Some(CpuUsage { user: pct(d[0] + d[1]), system: pct(d[2] + d[5] + d[6]), iowait: pct(d[4]), steal: pct(d[7]), idle: pct(d[3]) })
}

/// CPU-Nutzung zwischen zwei Lesungen von `/proc/stat`: insgesamt und je Thread (nach Last absteigend).
pub fn cpu_usage(first: &str, second: &str) -> Option<(CpuUsage, Vec<(String, f64)>)> {
    let a = parse_stat(first);
    let b = parse_stat(second);
    let total = usage_between(&a.iter().find(|(n, _)| n == "cpu")?.1, &b.iter().find(|(n, _)| n == "cpu")?.1)?;
    let mut cores: Vec<(String, f64)> = Vec::new();
    for (name, ta) in a.iter().filter(|(n, _)| n != "cpu") {
        if let Some((_, tb)) = b.iter().find(|(n, _)| n == name) {
            if let Some(u) = usage_between(ta, tb) {
                cores.push((name.clone(), u.busy()));
            }
        }
    }
    cores.sort_by(|x, y| y.1.total_cmp(&x.1));
    Some((total, cores))
}

pub fn load(run: &mut Run) -> ProbeResult {
    let loadavg = read_text(run, "/proc/loadavg", 1024).map_err(|e| Status::Failed(format!("cannot read /proc/loadavg: {e}")))?;
    let first = read_text(run, "/proc/stat", PROC_READ).map_err(|e| Status::Failed(format!("cannot read /proc/stat: {e}")))?;
    let started = Instant::now();
    run.sleep(run.ctx.sample_interval)?;
    let second = read_text(run, "/proc/stat", PROC_READ).map_err(|e| Status::Failed(format!("cannot read /proc/stat: {e}")))?;
    let secs = started.elapsed().as_secs_f64();
    let threads = parse_stat(&first).iter().filter(|(n, _)| n != "cpu").count();
    let mut text = String::new();
    let f: Vec<&str> = loadavg.split_whitespace().collect();
    if f.len() >= 4 {
        text.push_str(&format!("load average: {} {} {} (1, 5, 15 min); running/total tasks {}; {threads} CPU threads\n", f[0], f[1], f[2], f[3]));
    }
    match cpu_usage(&first, &second) {
        Some((u, cores)) => {
            text.push_str(&format!(
                "cpu over {secs:.1} s: {:.1}% busy (user {:.1}%, system {:.1}%, iowait {:.1}%, steal {:.1}%), idle {:.1}%\n",
                u.busy(),
                u.user,
                u.system,
                u.iowait,
                u.steal,
                u.idle
            ));
            let busiest: Vec<String> = cores.iter().take(3).map(|(n, p)| format!("{n} {p:.0}%")).collect();
            if !busiest.is_empty() {
                text.push_str(&format!("busiest threads: {}\n", busiest.join(", ")));
            }
        }
        None => text.push_str("cpu: no CPU time passed between the two samples\n"),
    }
    if text.is_empty() {
        return Err(Status::Failed("/proc/loadavg and /proc/stat have an unknown format".into()));
    }
    run.out.push(Part::new("", text));
    Ok(())
}

// ---- disks -----------------------------------------------------------------------------------------------------------

fn lsblk_text(doc: &Value) -> Option<String> {
    fn walk(devices: &[Value], depth: usize, out: &mut String) {
        for d in devices {
            let mounts: Vec<&str> = d.get("mountpoints").and_then(Value::as_array).map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            out.push_str(&format!(
                "{}{} {} {} {} {}\n",
                "  ".repeat(depth),
                json::str_of(d, "name").unwrap_or("?"),
                json::str_of(d, "size").unwrap_or("?"),
                json::str_of(d, "type").unwrap_or("?"),
                json::str_of(d, "fstype").unwrap_or("-"),
                if mounts.is_empty() { "-".to_string() } else { mounts.join(",") }
            ));
            if let Some(children) = d.get("children").and_then(Value::as_array) {
                walk(children, depth + 1, out);
            }
        }
    }
    let devices = doc.get("blockdevices")?.as_array()?;
    let mut out = String::from("name size type fstype mountpoints\n");
    walk(devices, 0, &mut out);
    Some(out)
}

pub fn disks(run: &mut Run) -> ProbeResult {
    let argvs = commands(run)?;
    let labels = ["df", "lsblk"];
    let mut failures = Vec::new();
    let mut used = 0usize;
    for (i, argv) in argvs.iter().enumerate() {
        let cap = run.share(used, i, argvs.len());
        let cmd = run.exec(argv, RAW_LIMIT.min(cap * 8), Keep::Head);
        match cmd.problem() {
            Some(Status::TimedOut) => return Err(Status::TimedOut),
            Some(Status::Cancelled) => return Err(Status::Cancelled),
            Some(other) => {
                failures.push(format!("{}: {}", labels[i], other.message().unwrap_or(other.label())));
                continue;
            }
            None => {}
        }
        let text = if i == 0 {
            // df meldet kaputte Mounts („Transport endpoint is not connected“) auf stderr und endet mit 1
            if let Some(note) = cmd.exit_note() {
                run.out.note(note);
            }
            cmd.text()
        } else {
            match json::document(&cmd.raw_text()).and_then(|d| lsblk_text(&d)) {
                Some(t) => t,
                None => {
                    run.out.note("the lsblk output was not valid JSON and is shown as it came");
                    cmd.text()
                }
            }
        };
        let cut = cut_to_bytes(text.trim_end(), cap.saturating_sub(64), Keep::Head);
        used += cut.text.len();
        if cut.omitted_lines > 0 {
            run.out.note(format!("{} more {} lines are not shown", cut.omitted_lines, labels[i]));
        }
        run.out.push(Part::new(labels[i], cut.text));
    }
    if run.out.parts.is_empty() {
        return Err(Status::Failed(failures.join("; ")));
    }
    if !failures.is_empty() {
        run.out.status = Status::Partial(failures.join("; "));
    }
    Ok(())
}

// ---- top_procs -------------------------------------------------------------------------------------------------------

/// Ein Prozess zur Zeit der ersten Messung.
#[derive(Debug, Clone, PartialEq)]
struct Proc {
    pid: u32,
    name: String,
    rss_kib: u64,
    uid: u32,
    /// Verbrauchte CPU-Zeit in Ticks (user + system).
    ticks: u64,
}

/// `(user + system)` in Ticks und den Namen aus `/proc/<pid>/stat`. Der Name steht in Klammern und darf selbst Klammern
/// und Leerzeichen enthalten, darum gilt die letzte schließende Klammer.
fn parse_proc_stat(text: &str) -> Option<(String, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let name = text[open + 1..close].to_string();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // Nach der Klammer: state (Feld 3) … utime (Feld 14) = Index 11, stime = Index 12
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    Some((name, utime + stime))
}

/// `VmRSS` (KiB) und die echte Benutzerkennung aus `/proc/<pid>/status`.
fn parse_proc_status(text: &str) -> (Option<String>, u64, u32) {
    let mut name = None;
    let mut rss = 0;
    let mut uid = 0;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("VmRSS:") {
            rss = v.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Uid:") {
            uid = v.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
    }
    (name, rss, uid)
}

/// CPU-Last eines Prozesses zwischen zwei Messungen in Prozent eines Kerns.
pub fn cpu_percent(ticks_before: u64, ticks_after: u64, seconds: f64, ticks_per_second: f64) -> f64 {
    if seconds <= 0.0 || ticks_per_second <= 0.0 {
        return 0.0;
    }
    ticks_after.saturating_sub(ticks_before) as f64 * 100.0 / (ticks_per_second * seconds)
}

/// So viele Prozesse je Liste.
const TOP_N: usize = 10;

pub fn top_procs(run: &mut Run) -> ProbeResult {
    let proc_dir = run.ctx.paths.under_root("/proc");
    let entries = run.fs.read_dir(&proc_dir).map_err(|e| Status::Failed(format!("cannot list /proc: {e}")))?;
    let pids: Vec<(u32, std::path::PathBuf)> =
        entries.iter().filter(|e| e.is_dir && !e.symlink).filter_map(|e| Some((e.name.parse::<u32>().ok()?, e.path.clone()))).collect();
    let mut procs: Vec<Proc> = Vec::new();
    for (n, (pid, dir)) in pids.iter().enumerate() {
        if n % 64 == 0 {
            run.check()?;
        }
        // Ein Prozess kann zwischen Auflisten und Lesen enden: dann fehlt er einfach
        let Ok(stat) = run.fs.read(&dir.join("stat"), 4096) else { continue };
        let Ok(status) = run.fs.read(&dir.join("status"), 8192) else { continue };
        let Some((comm, ticks)) = parse_proc_stat(&String::from_utf8_lossy(&stat.bytes)) else { continue };
        let (name, rss_kib, uid) = parse_proc_status(&String::from_utf8_lossy(&status.bytes));
        let name = clean(name.unwrap_or(comm).as_bytes());
        if name.trim().is_empty() {
            continue;
        }
        procs.push(Proc { pid: *pid, name, rss_kib, uid, ticks });
    }
    if procs.is_empty() {
        return Err(Status::Failed("no process in /proc could be read".into()));
    }
    // Zweite Messung der CPU-Zeit
    let started = Instant::now();
    run.sleep(run.ctx.sample_interval)?;
    let mut after: HashMap<u32, u64> = HashMap::new();
    for (n, p) in procs.iter().enumerate() {
        if n % 64 == 0 {
            run.check()?;
        }
        if let Ok(stat) = run.fs.read(&proc_dir.join(p.pid.to_string()).join("stat"), 4096) {
            if let Some((_, t)) = parse_proc_stat(&String::from_utf8_lossy(&stat.bytes)) {
                after.insert(p.pid, t);
            }
        }
    }
    let secs = started.elapsed().as_secs_f64();
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let cpu = |p: &Proc| after.get(&p.pid).map(|t| cpu_percent(p.ticks, *t, secs, hz)).unwrap_or(0.0);

    let mut by_rss: Vec<&Proc> = procs.iter().filter(|p| p.rss_kib > 0).collect();
    by_rss.sort_by(|a, b| b.rss_kib.cmp(&a.rss_kib).then(a.pid.cmp(&b.pid)));
    let mut by_cpu: Vec<&Proc> = procs.iter().collect();
    by_cpu.sort_by(|a, b| cpu(b).total_cmp(&cpu(a)).then(b.ticks.cmp(&a.ticks)).then(a.pid.cmp(&b.pid)));
    let render = |list: &[&Proc]| {
        let mut rows = vec![vec!["rss".to_string(), "cpu".into(), "cpu time".into(), "uid".into(), "pid".into(), "program".into()]];
        for p in list.iter().take(TOP_N) {
            rows.push(vec![
                human_bytes(p.rss_kib * 1024),
                format!("{:.0}%", cpu(p)),
                format!("{} s", p.ticks as f64 / hz),
                p.uid.to_string(),
                p.pid.to_string(),
                p.name.clone(),
            ]);
        }
        table(&rows)
    };
    let text = format!(
        "{} largest by memory:\n{}\n{} busiest by CPU over {secs:.1} s:\n{}",
        TOP_N.min(by_rss.len()),
        render(&by_rss),
        TOP_N.min(by_cpu.len()),
        render(&by_cpu)
    );
    let cut = cut_to_bytes(text.trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    run.out.note(format!("{} processes read; program names only, no command lines", procs.len()));
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- oom -------------------------------------------------------------------------------------------------------------

pub fn oom(run: &mut Run) -> ProbeResult {
    journal_probe(run, "No out-of-memory kills in the kernel messages of this boot.")
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    const STAT_A: &str = "cpu  1000 0 500 8000 100 0 0 0 0 0\ncpu0 500 0 250 4000 50 0 0 0 0 0\ncpu1 500 0 250 4000 50 0 0 0 0 0\nintr 1\n";
    const STAT_B: &str = "cpu  1100 0 540 8300 110 0 0 10 0 0\ncpu0 600 0 270 4050 55 0 0 5 0 0\ncpu1 500 0 270 4250 55 0 0 5 0 0\nintr 2\n";

    #[test]
    fn cpu_usage_between_two_samples() {
        let (total, cores) = cpu_usage(STAT_A, STAT_B).unwrap();
        // Deltas: user 100, system 40, idle 300, iowait 10, steal 10 → 460
        assert!((total.user - 100.0 * 100.0 / 460.0).abs() < 0.01, "{total:?}");
        assert!((total.idle - 300.0 * 100.0 / 460.0).abs() < 0.01);
        assert!((total.steal - 10.0 * 100.0 / 460.0).abs() < 0.01);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].0, "cpu0", "the busier thread comes first");
        assert!(cores[0].1 > cores[1].1);
        // Keine verstrichene Zeit: keine Aussage
        assert!(cpu_usage(STAT_A, STAT_A).is_none());
        assert!(cpu_usage("", STAT_A).is_none());
        assert!(cpu_usage("cpu  1 2 3", "cpu  1 2 3").is_none());
    }

    #[test]
    fn process_stat_names_may_contain_parentheses_and_spaces() {
        let line = "4242 (Web Content (2)) S 1 4242 4242 0 -1 4194560 100 0 0 0 150 50 0 0 20 0 5 0 12345 1000 200 18446744073709551615";
        assert_eq!(parse_proc_stat(line), Some(("Web Content (2)".to_string(), 200)));
        assert_eq!(parse_proc_stat("garbage"), None);
        assert_eq!(parse_proc_stat("1 (x) S 1"), None);
        let (name, rss, uid) = parse_proc_status("Name:\tfirefox\nUmask:\t0022\nUid:\t1000\t1000\t1000\t1000\nVmRSS:\t  123456 kB\n");
        assert_eq!((name.as_deref(), rss, uid), (Some("firefox"), 123_456, 1000));
        let (name, rss, _) = parse_proc_status("Name:\tkthreadd\n");
        assert_eq!((name.as_deref(), rss), (Some("kthreadd"), 0));
    }

    #[test]
    fn cpu_percent_of_a_process() {
        assert!((cpu_percent(100, 150, 0.5, 100.0) - 100.0).abs() < 1e-9, "50 ticks in 0.5 s at 100 Hz is one core");
        assert!((cpu_percent(100, 125, 0.5, 100.0) - 50.0).abs() < 1e-9);
        assert_eq!(cpu_percent(100, 90, 0.5, 100.0), 0.0);
        assert_eq!(cpu_percent(1, 2, 0.0, 100.0), 0.0);
    }

    #[test]
    fn meminfo_and_pressure_text() {
        let m = "MemTotal:       65536000 kB\nMemFree:         1000000 kB\nMemAvailable:   32768000 kB\nSwapTotal:       8000000 kB\nSwapFree:        8000000 kB\nDirty: 12 kB\nHugePages_Total: 0\n";
        let t = meminfo_text(m);
        assert!(t.contains("MemTotal: 62.5 GiB"), "{t}");
        assert!(t.contains("in use (total - available): 31.2 GiB = 50%"), "{t}");
        assert!(t.contains("swap in use: 0 B = 0%"), "{t}");
        assert!(!t.contains("HugePages"));
        assert_eq!(
            pressure_text("memory", "some avg10=0.50 avg60=0.10 avg300=0.00 total=12345\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n"),
            "memory pressure some avg10=0.50 avg60=0.10 avg300=0.00\nmemory pressure full avg10=0.00 avg60=0.00 avg300=0.00\n"
        );
        assert_eq!(meminfo_text(""), "");
    }

    #[test]
    fn lsblk_renders_the_tree() {
        let doc: Value = serde_json::from_str(r#"{"blockdevices":[{"name":"nvme0n1","size":"931.5G","type":"disk","mountpoints":[null],"fstype":null,"children":[{"name":"nvme0n1p1","size":"512M","type":"part","mountpoints":["/boot"],"fstype":"vfat"},{"name":"nvme0n1p2","size":"931G","type":"part","mountpoints":["/","/home"],"fstype":"ext4"}]}]}"#).unwrap();
        let t = lsblk_text(&doc).unwrap();
        assert_eq!(t, "name size type fstype mountpoints\nnvme0n1 931.5G disk - -\n  nvme0n1p1 512M part vfat /boot\n  nvme0n1p2 931G part ext4 /,/home\n");
        assert!(lsblk_text(&serde_json::json!({"x":1})).is_none());
    }
}
