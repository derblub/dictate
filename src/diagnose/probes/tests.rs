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

//! Fixture-Tests aller Proben: normal, leer, Fehler, riesig. Alle Daten sind erfunden; die Programme sind Fakes in einem
//! Temp-Ordner, die Dateien liegen in einem Fixture-Baum. Keine Probe läuft hier gegen das echte System.

use super::output::{Filter, ProbeOutput, Status};
use super::testkit::{arm, at, boots_json, journal_line, now, us, Arm, World};
use super::time::{parse_iso, DAY};
use crate::diagnose::argv::Program;
use std::fs;
use std::time::Duration;

const CID_WEB: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90112233445566778899aabbccddeeff00";
const CID_DB: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0ffeeddccbbaa99887766554433221100";

/// Der Text darf nie einen Hinweis auf einen verweigerten Dateizugriff enthalten (die Probe öffnete etwas Undeklariertes)
/// und bleibt in der Grenze des Katalogs.
fn assert_sane(out: &ProbeOutput) {
    let text = out.text();
    assert!(!text.contains("may not open"), "{}: {text}", out.probe);
    let spec = crate::diagnose::catalog::spec(out.probe).unwrap();
    assert!(text.len() <= spec.limit_bytes, "{}: {} bytes exceed the limit of {}", out.probe, text.len(), spec.limit_bytes);
    assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    assert!(!text.chars().any(|c| c.is_control() && c != '\n' && c != '\t'), "{}: control characters in the text", out.probe);
}

fn text_of(out: &ProbeOutput) -> String {
    assert_sane(out);
    out.text()
}

/// Eine eindeutige Buchstabenfolge zu `i` (Zahlen in Fixtures würden beim Zusammenfassen als gleiche Meldung zählen).
fn alpha(mut i: u32) -> String {
    let mut s = String::new();
    loop {
        s.push((b'a' + (i % 26) as u8) as char);
        i /= 26;
        if i == 0 {
            return s;
        }
    }
}

fn crash_tail() -> String {
    let mut s = String::new();
    for i in 0..30 {
        s.push_str(&journal_line(&format!("2026-11-04T09:46:{:02}", 20 + i / 2), "kernel", &format!("nvidia-modeset: heartbeat {}", alpha(i))));
        s.push('\n');
    }
    s.push_str(&journal_line("2026-11-04T09:46:55", "dockerd[812]", "container 665982ad0616 healthy"));
    s.push('\n');
    s
}

fn clean_tail() -> String {
    let mut s = crash_tail();
    s.push_str(&journal_line("2026-11-04T09:46:56", "systemd[1]", "Reached target Shutdown."));
    s.push('\n');
    s.push_str(&journal_line("2026-11-04T09:46:57", "systemd-journald[304]", "Journal stopped"));
    s.push('\n');
    s
}

fn journalctl_with_boots(w: &World, extra: Vec<Arm>) {
    let mut arms = vec![arm("--list-boots", boots_json())];
    arms.extend(extra);
    w.fake(Program::Journalctl, &arms);
}

// =================================================================================================================
// Boot

#[test]
fn boots_lists_the_boots_and_judges_the_previous_one() {
    let w = World::new("boots-normal");
    journalctl_with_boots(&w, vec![arm("--boot=-1", crash_tail())]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("idx  boot id"), "{text}");
    for needle in ["-2   2e6ab35c", "-1   7d04a9e4", "0    c9b1d2e3", "2026-11-04T08:12:01+0100", "2026-11-04T09:46:55+0100", "(running)"] {
        assert!(text.contains(needle), "{needle} missing in\n{text}");
    }
    assert!(text.contains("boot -1 ended UNCLEANLY"), "{text}");
    assert!(!text.contains("ended cleanly"));
    // Dauer des Absturz-Boots: 08:12:01 bis 09:46:55 = 1 h 34 min
    assert!(text.contains("94 min"), "{text}");
}

#[test]
fn boots_recognises_a_clean_shutdown() {
    let w = World::new("boots-clean");
    journalctl_with_boots(&w, vec![arm("--boot=-1", clean_tail())]);
    let text = text_of(&w.collect(r#"{"probe":"boots"}"#));
    assert!(text.contains("boot -1 ended cleanly"), "{text}");
}

#[test]
fn boots_says_so_when_the_verdict_cannot_be_read() {
    let w = World::new("boots-noverdict");
    journalctl_with_boots(&w, vec![arm("--boot=-1", "").err("Failed to open journal").exit(1)]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    let text = text_of(&out);
    assert!(text.contains("unknown whether it ended cleanly"), "{text}");
    assert!(text.contains("-1   7d04a9e4"));
}

#[test]
fn boots_with_only_the_current_boot_has_no_verdict() {
    let w = World::new("boots-one");
    let only =
        format!(r#"[{{"index":0,"boot_id":"c9b1d2e3f4a5465798a1b2c3d4e53333","first_entry":{},"last_entry":{}}}]"#, us(at(2026, 11, 4, 10, 5, 30)), us(now()));
    w.fake(Program::Journalctl, &[arm("--list-boots", only)]);
    let text = text_of(&w.collect(r#"{"probe":"boots"}"#));
    assert!(!text.contains("boot -1"), "{text}");
    assert!(text.contains("(running)"));
}

#[test]
fn boots_hands_the_current_boot_and_the_verdict_on_the_previous_one_to_the_boot_hint() {
    use super::output::{ListedBoots, PreviousBoot};
    let current = Some("c9b1d2e3f4a5465798a1b2c3d4e53333".to_string());
    // unsauber: Absturz-Ende des Journals von Boot -1
    let w = World::new("boots-listed-unclean");
    journalctl_with_boots(&w, vec![arm("--boot=-1", crash_tail())]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    assert_eq!(out.listed.boots, Some(ListedBoots { current_id: current.clone(), previous: PreviousBoot::Unclean }));
    // sauber
    let w = World::new("boots-listed-clean");
    journalctl_with_boots(&w, vec![arm("--boot=-1", clean_tail())]);
    assert_eq!(w.collect(r#"{"probe":"boots"}"#).listed.boots, Some(ListedBoots { current_id: current.clone(), previous: PreviousBoot::Clean }));
    // das Ende ließ sich nicht lesen: unbekannt, nie „unsauber“
    let w = World::new("boots-listed-unknown");
    journalctl_with_boots(&w, vec![arm("--boot=-1", "").err("Failed to open journal").exit(1)]);
    assert_eq!(w.collect(r#"{"probe":"boots"}"#).listed.boots, Some(ListedBoots { current_id: current.clone(), previous: PreviousBoot::Unknown }));
    // nur der laufende Boot: kein Vorgänger
    let w = World::new("boots-listed-one");
    let only =
        format!(r#"[{{"index":0,"boot_id":"c9b1d2e3f4a5465798a1b2c3d4e53333","first_entry":{},"last_entry":{}}}]"#, us(at(2026, 11, 4, 10, 5, 30)), us(now()));
    w.fake(Program::Journalctl, &[arm("--list-boots", only)]);
    assert_eq!(w.collect(r#"{"probe":"boots"}"#).listed.boots, Some(ListedBoots { current_id: current, previous: PreviousBoot::None }));
    // Überschrift und Kennung stehen nur in `listed`, nicht als eigener Text über die acht Zeichen der Tabelle hinaus
    let w = World::new("boots-listed-text");
    journalctl_with_boots(&w, vec![arm("--boot=-1", crash_tail())]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    assert!(!out.text().contains("c9b1d2e3f4a5465798a1b2c3d4e53333"), "the full ID never reaches the report text");
}

#[test]
fn boots_empty_error_and_giant() {
    let w = World::new("boots-empty");
    w.fake(Program::Journalctl, &[arm("--list-boots", "[]")]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    assert_eq!(out.status, Status::Empty);
    assert!(text_of(&out).contains("no boots"));

    let w = World::new("boots-error");
    let out = w.collect(r#"{"probe":"boots"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("journalctl is not installed")), "{:?}", out.status);

    let w = World::new("boots-giant");
    let mut list = String::from("[");
    for i in 0..400 {
        if i > 0 {
            list.push(',');
        }
        let idx = i - 399;
        list.push_str(&format!(
            r#"{{"index":{idx},"boot_id":"{:032x}","first_entry":{},"last_entry":{}}}"#,
            i,
            us(at(2026, 1, 1, 0, 0, 0) + i as i64 * 3600),
            us(at(2026, 1, 1, 0, 30, 0) + i as i64 * 3600)
        ));
    }
    list.push(']');
    w.fake(Program::Journalctl, &[arm("--list-boots", list), arm("--boot=-1", crash_tail())]);
    let out = w.collect(r#"{"probe":"boots"}"#);
    let text = text_of(&out);
    assert_eq!(text.lines().filter(|l| l.contains("00000000")).count(), 10, "{text}");
    assert!(out.notes.iter().any(|n| n.contains("390 older boots are not shown")), "{:?}", out.notes);
}

#[test]
fn boot_end_returns_the_journal_end_of_the_resolved_boot() {
    let w = World::new("bootend-normal");
    journalctl_with_boots(&w, vec![arm("--boot=-1 -n 200", crash_tail())]);
    let out = w.collect(r#"{"probe":"boot_end","boot":-1,"lines":200}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains(&format!("nvidia-modeset: heartbeat {}", alpha(29))), "{text}");
    assert_eq!(out.resolved.len(), 1);
    assert!(out.resolved[0].starts_with("boot -1 → 2026-11-04T08:12:01+0100 … 2026-11-04T09:46:55+0100"), "{:?}", out.resolved);
}

#[test]
fn boot_end_empty_error_missing_boot_and_cancel() {
    let w = World::new("bootend-empty");
    journalctl_with_boots(&w, vec![arm("--boot=-1", "-- No entries --\n")]);
    let out = w.collect(r#"{"probe":"boot_end","boot":-1,"lines":50}"#);
    assert_eq!(out.status, Status::Empty);
    assert!(text_of(&out).contains("no entries"));

    // Programm scheitert mit Meldung und ohne Ausgabe
    let w = World::new("bootend-error");
    journalctl_with_boots(&w, vec![arm("--boot=-1", "").err("Failed to open journal files: Permission denied").exit(1)]);
    let out = w.collect(r#"{"probe":"boot_end","boot":-1,"lines":50}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("code 1") && m.contains("Permission denied")), "{:?}", out.status);

    // Der Boot steht nicht in der Liste
    let w = World::new("bootend-missing");
    journalctl_with_boots(&w, vec![]);
    let out = w.collect(r#"{"probe":"boot_end","boot":-5,"lines":50}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("boot -5 does not exist") && m.contains("-2 to 0")), "{:?}", out.status);
    // und die Probe wurde nie gestartet (die Liste ist der einzige Aufruf)
    assert!(!out.text().contains("heartbeat"));

    // Abbruch vor dem Start
    let w = World::new("bootend-cancel");
    journalctl_with_boots(&w, vec![arm("--boot=-1", crash_tail())]);
    w.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    let out = w.collect(r#"{"probe":"boot_end","boot":-1,"lines":50}"#);
    assert_eq!(out.status, Status::Cancelled);
}

#[test]
fn boot_end_giant_output_keeps_the_end_and_folds_repeats() {
    let w = World::new("bootend-giant");
    let mut s = String::new();
    for i in 0..60_000u32 {
        s.push_str(&journal_line(
            &format!("2026-11-04T09:{:02}:{:02}", (i / 60) % 60, i % 60),
            &format!("worker[{}]", 1000 + i % 977),
            &format!("request {} handled by unit {}", alpha(i), alpha(i / 7)),
        ));
        s.push('\n');
    }
    s.push_str(&journal_line("2026-11-04T09:46:55", "kernel", "LAST LINE OF THE BOOT"));
    s.push('\n');
    assert!(s.len() > 4_000_000);
    journalctl_with_boots(&w, vec![arm("--boot=-1", s)]);
    let started = std::time::Instant::now();
    let out = w.collect(r#"{"probe":"boot_end","boot":-1,"lines":500}"#);
    let text = text_of(&out);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(text.contains("LAST LINE OF THE BOOT"), "the end is kept");
    assert!(text.contains("earlier lines were left out") || out.notes.iter().any(|n| n.contains("earlier lines")), "{:?}", out.notes);
    assert!(out.notes.iter().any(|n| n.contains("capture limit")), "{:?}", out.notes);
    assert!(out.raw_bytes > 4_000_000);
}

#[test]
fn kernel_errors_folds_a_series_of_segfaults() {
    let w = World::new("kerr");
    let mut s = String::new();
    s.push_str(&journal_line("2026-11-04T09:00:00", "kernel", "NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus"));
    s.push('\n');
    for i in 0..76u32 {
        s.push_str(&journal_line(
            &format!("2026-11-04T09:10:{:02}", i % 60),
            "kernel",
            &format!(
                "node[{}]: segfault at {:08x} ip 00007f{:06x} sp 00007ffe{:04x} error 4 in libc.so.6[7f{:04x}+28000]",
                4000 + i,
                0x10 + i,
                0x1000 * i,
                i * 16,
                i
            ),
        ));
        s.push('\n');
    }
    journalctl_with_boots(&w, vec![arm("-k --boot=0", s)]);
    let out = w.collect(r#"{"probe":"kernel_errors","boot":0,"lines":500}"#);
    let text = text_of(&out);
    assert!(text.contains("Xid"), "{text}");
    assert!(text.contains("… repeated 76×"), "{text}");
    assert_eq!(text.lines().filter(|l| l.contains("segfault")).count(), 1, "{text}");
    assert!(out.notes.iter().any(|n| n.contains("74 repeated lines were folded")), "{:?}", out.notes);
}

// ---- pstore -----------------------------------------------------------------------------------------------------

const DMESG: &str = "<6>[ 7497.083228]  kthread+0xe4/0x120\n<4>[ 7497.083230] Oops: general protection fault in nvidia_isr\n<0>[ 7497.083233] Kernel panic - not syncing: Fatal exception in interrupt\n";

fn crash_epoch() -> i64 {
    at(2026, 11, 4, 9, 47, 57)
}

fn pstore_world(tag: &str) -> World {
    let w = World::new(tag);
    journalctl_with_boots(&w, vec![]);
    let dir = format!("var/lib/systemd/pstore/{}/001", crash_epoch());
    w.write(&format!("{dir}/dmesg.txt"), DMESG);
    w.write(&format!("{dir}/dmesg-efi_pstore-179378207701001"), "Panic#1 Part1\n<0>[ 7497.083233] Kernel panic - not syncing: Fatal exception in interrupt\n");
    w
}

#[test]
fn pstore_returns_the_dump_that_belongs_to_the_boot() {
    let w = pstore_world("pstore-normal");
    let out = w.collect(r#"{"probe":"pstore","boot":-1}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("Oops: general protection fault in nvidia_isr"), "{text}");
    assert!(text.contains(&format!("dump 2026-11-04T09:47:57+0100 ({}/001)", crash_epoch())), "{text}");
    // dmesg.txt genügt, die Teile werden nicht noch einmal angehängt
    assert_eq!(text.matches("Kernel panic - not syncing").count(), 1, "{text}");
    // Das Abbild gehört zum Boot davor, nicht zu den anderen
    let other = w.collect(r#"{"probe":"pstore","boot":0}"#);
    assert_eq!(other.status, Status::Empty);
    assert!(text_of(&other).contains("No pstore dump between"), "{}", other.text());
    assert!(other.text().contains("1 dump(s) from other boots"), "{}", other.text());
    let older = w.collect(r#"{"probe":"pstore","boot":-2}"#);
    assert_eq!(older.status, Status::Empty);
}

#[test]
fn pstore_assembles_parts_without_dmesg_txt_oldest_first() {
    let w = World::new("pstore-parts");
    journalctl_with_boots(&w, vec![]);
    let dir = format!("var/lib/systemd/pstore/{}/002", crash_epoch());
    // Part1 ist das Ende des Logs, die höchste Nummer der Anfang
    w.write(&format!("{dir}/dmesg-efi_pstore-1"), "Panic#2 Part1\nline-3 end\n");
    w.write(&format!("{dir}/dmesg-efi_pstore-2"), "Panic#2 Part2\nline-2 middle\n");
    w.write(&format!("{dir}/dmesg-efi_pstore-3"), "Panic#2 Part3\nline-1 start\n");
    let text = text_of(&w.collect(r#"{"probe":"pstore","boot":-1}"#));
    let a = text.find("line-1 start").unwrap();
    let b = text.find("line-2 middle").unwrap();
    let c = text.find("line-3 end").unwrap();
    assert!(a < b && b < c, "{text}");
    assert!(!text.contains("Panic#2 Part"), "headers are dropped: {text}");
}

#[test]
fn pstore_unreadable_dump_is_reported_with_its_metadata() {
    if unsafe { libc::geteuid() } == 0 {
        return; // als root sind die Dateien lesbar
    }
    use std::os::unix::fs::PermissionsExt;
    let w = pstore_world("pstore-denied");
    let dir = w.root().join(format!("var/lib/systemd/pstore/{}/001", crash_epoch()));
    for f in fs::read_dir(&dir).unwrap().flatten() {
        fs::set_permissions(f.path(), fs::Permissions::from_mode(0o000)).unwrap();
    }
    let out = w.collect(r#"{"probe":"pstore","boot":-1}"#);
    let text = text_of(&out);
    assert!(matches!(out.status, Status::Failed(_)), "{:?}", out.status);
    assert!(text.contains("2 file(s)"), "{text}");
    assert!(text.contains("cannot be read (permission denied)"), "{text}");
    assert!(text.contains("never uses sudo"), "{text}");
    assert!(!text.contains("Oops"));
}

#[test]
fn pstore_without_directory_and_with_a_giant_dump() {
    let w = World::new("pstore-none");
    journalctl_with_boots(&w, vec![]);
    fs::remove_dir_all(w.root().join("var/lib/systemd/pstore")).unwrap();
    let out = w.collect(r#"{"probe":"pstore","boot":-1}"#);
    assert_eq!(out.status, Status::Empty);
    assert!(out.text().contains("No pstore directory"), "{}", out.text());

    let w = World::new("pstore-giant");
    journalctl_with_boots(&w, vec![]);
    let mut big = String::new();
    for i in 0..60_000u32 {
        big.push_str(&format!("<6>[{:9}.{:06}] some kernel line number {} with text\n", i / 1000, i % 1000, alpha(i)));
    }
    big.push_str("<0>[ 99999.000000] FINAL PANIC LINE\n");
    w.write(&format!("var/lib/systemd/pstore/{}/001/dmesg.txt", crash_epoch()), big);
    let out = w.collect(r#"{"probe":"pstore","boot":-1}"#);
    let text = text_of(&out);
    assert!(text.contains("FINAL PANIC LINE"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("earlier lines were left out")), "{:?}", out.notes);
}

#[test]
fn pstore_shows_only_the_newest_dumps_of_a_window() {
    let w = World::new("pstore-many");
    journalctl_with_boots(&w, vec![]);
    for k in 0..5 {
        w.write(&format!("var/lib/systemd/pstore/{}/001/dmesg.txt", crash_epoch() + k), format!("dump number {k}\n"));
    }
    let out = w.collect(r#"{"probe":"pstore","boot":-1}"#);
    let text = text_of(&out);
    assert!(text.contains("dump number 4") && text.contains("dump number 3") && text.contains("dump number 2"), "{text}");
    assert!(!text.contains("dump number 1"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("2 older dump(s)")), "{:?}", out.notes);
}

// ---- temps_tail -------------------------------------------------------------------------------------------------

fn csv_rows(day: &str, from_s: i64, to_s: i64, step: i64) -> String {
    let mut s = String::new();
    let mut t = from_s;
    while t <= to_s {
        s.push_str(&format!(
            "{day} {:02}:{:02}:{:02},{},{},47,56,62,39,27.19,1115,591,0.57,44322\n",
            t / 3600,
            t % 3600 / 60,
            t % 60,
            60 + (t % 7),
            60 + (t % 5)
        ));
        t += step;
    }
    s
}

const CSV_HEADER: &str = "zeit,cpu_pkg,cpu_core_max,cpu_dell,nvme,gpu,gpu_util,gpu_power_w,fan_cpu_rpm,fan_case_rpm,load1,mem_avail_mb\n";

#[test]
fn temps_tail_shows_the_last_rows_after_the_journal_ended() {
    let w = World::new("temps-normal");
    journalctl_with_boots(&w, vec![]);
    let mut csv = String::from(CSV_HEADER);
    // Alle 20 s bis 09:47:38 (der Logger lebt 43 s länger als das Journal)
    csv.push_str(&csv_rows("2026-11-04", 9 * 3600 + 47 * 60 + 38 - 2000, 9 * 3600 + 47 * 60 + 38, 20));
    w.write("templog/temps-2026-11-04.csv", csv);
    let out = w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.starts_with("zeit,cpu_pkg"), "{text}");
    assert!(text.contains("2026-11-04 09:47:38,"), "{text}");
    assert!(!text.contains("2026-11-04 09:41:38,"), "outside the 5 minute window: {text}");
    assert!(text.contains("is 43 s after the last journal entry of the boot (2026-11-04T09:46:55+0100)"), "{text}");
    // ca. 5 Minuten plus 43 s bei 20 s Abstand
    let rows = text.lines().filter(|l| l.starts_with("2026-11-04 ")).count();
    assert_eq!(rows, 18, "{rows} rows\n{text}");
}

#[test]
fn temps_tail_for_the_running_boot_reports_the_age_of_the_newest_row() {
    let w = World::new("temps-now");
    journalctl_with_boots(&w, vec![]);
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-05", 11 * 3600 + 30 * 60, 11 * 3600 + 59 * 60 + 40, 20));
    w.write("templog/temps-2026-11-05.csv", csv);
    let out = w.collect(r#"{"probe":"temps_tail","boot":0,"window_min":15}"#);
    let text = text_of(&out);
    assert!(text.contains("last row 2026-11-05T11:59:40+0100 is 20 s old"), "{text}");
    assert!(!text.contains("11:40:"), "{text}");
}

#[test]
fn temps_tail_marks_a_line_that_was_cut_by_the_crash() {
    let w = World::new("temps-cut");
    journalctl_with_boots(&w, vec![]);
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-04", 9 * 3600 + 46 * 60, 9 * 3600 + 47 * 60, 20));
    csv.push_str("2026-11-04 09:47:20,70,71,4");
    w.write("templog/temps-2026-11-04.csv", csv);
    let text = text_of(&w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#));
    assert!(text.contains("2026-11-04 09:47:20,70,71,4  (incomplete line)"), "{text}");
}

#[test]
fn temps_tail_empty_missing_and_giant() {
    // Datei da, aber keine Zeile im Fenster
    let w = World::new("temps-empty");
    journalctl_with_boots(&w, vec![]);
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-04", 3 * 3600, 3 * 3600 + 100, 20));
    w.write("templog/temps-2026-11-04.csv", csv);
    let out = w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#);
    assert_eq!(out.status, Status::Empty);
    assert!(out.text().contains("no rows between"), "{}", out.text());

    // Keine Datei
    let w = World::new("temps-missing");
    journalctl_with_boots(&w, vec![]);
    let out = w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("temps-2026-11-04.csv")), "{:?}", out.status);

    // Ein Tag mit Zeilen alle 2 s
    let w = World::new("temps-giant");
    journalctl_with_boots(&w, vec![]);
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-04", 0, 9 * 3600 + 47 * 60 + 38, 2));
    assert!(csv.len() > 1_000_000, "{}", csv.len());
    w.write("templog/temps-2026-11-04.csv", csv);
    let out = w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":60}"#);
    let text = text_of(&out);
    assert!(out.notes.iter().any(|n| n.contains("older rows are not shown")), "{:?}", out.notes);
    assert!(text.contains("2026-11-04 09:47:38,"), "{text}");
}

#[test]
fn temps_tail_ignores_other_files_in_the_directory() {
    let w = World::new("temps-other");
    journalctl_with_boots(&w, vec![]);
    w.write("templog/temps-2026-11-04.csv", format!("{CSV_HEADER}{}", csv_rows("2026-11-04", 9 * 3600 + 46 * 60, 9 * 3600 + 47 * 60, 20)));
    w.write("templog/notes.txt", "hunter2 secret");
    w.write("templog/temps-2026-11-04.csv.bak", "old");
    let text = text_of(&w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#));
    assert!(!text.contains("hunter2") && !text.contains("old"), "{text}");
}

#[test]
fn temps_tail_without_a_configured_logger_says_so_and_reads_nothing() {
    // `diagnose.temp_log_dir` leer (Vorgabe): eine Datei am üblichen Ort bleibt ungelesen
    let mut w = World::new("temps-none");
    journalctl_with_boots(&w, vec![]);
    w.write("templog/temps-2026-11-04.csv", format!("{CSV_HEADER}{}", csv_rows("2026-11-04", 9 * 3600 + 46 * 60, 9 * 3600 + 47 * 60, 20)));
    let root = w.root();
    w.ctx.paths = super::ProbePaths::rooted(root.clone(), root.join("home/u"), None).unwrap();
    let out = w.collect(r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("no temperature log is set up")), "{:?}", out.status);
    assert!(!out.text().contains("2026-11-04"), "{}", out.text());
    // die Lese-Liste der Probe erlaubt ohne Ordner nichts, auch nicht unter der Wurzel
    let spec = crate::diagnose::catalog::spec("temps_tail").unwrap();
    let fs = super::fs::Fs::for_spec(&w.ctx.paths, spec);
    assert!(!fs.allows_file(&root.join("templog/temps-2026-11-04.csv")));
    assert!(!fs.allows_file(&root.join("temps-2026-11-04.csv")));
}

// ---- kernel_info -----------------------------------------------------------------------------------------------------

#[test]
fn kernel_info_redacts_secret_looking_parameters_and_marks_the_command_line() {
    let w = World::new("kinfo-normal");
    w.write("proc/version", "Linux version 7.2.14-arch1-1 (builder@host) #1 SMP\n");
    w.write(
        "proc/cmdline",
        "BOOT_IMAGE=/vmlinuz-linux root=UUID=1111-2222 rw quiet luks.key=sup3rs3cret cryptkey=/dev/x:1:2 vconsole.keymap=de nvidia_drm.modeset=1\n",
    );
    w.write("sys/class/dmi/id/bios_version", "F7\n");
    let out = w.collect(r#"{"probe":"kernel_info"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains("## version") && text.contains("## cmdline") && text.contains("## bios"), "{text}");
    assert!(text.contains("Linux version 7.2.14-arch1-1"));
    assert!(text.contains("F7"));
    assert!(!text.contains("sup3rs3cret") && !text.contains("/dev/x:1:2"), "{text}");
    assert!(text.contains("luks.key=[REDACTED:kernel_param]") && text.contains("cryptkey=[REDACTED:kernel_param]"), "{text}");
    assert!(text.contains("vconsole.keymap=de"), "harmless parameters stay: {text}");
    assert!(text.contains("root=UUID=1111-2222"));
    assert_eq!(out.redactions.iter().map(|r| (r.kind, r.count)).collect::<Vec<_>>(), [("kernel_param", 2)]);
    // Die Kommandozeile geht streng in den allgemeinen Filter, die anderen Teile nicht
    let filters: Vec<(String, Filter)> = out.parts.iter().map(|p| (p.label.clone(), p.filter)).collect();
    assert_eq!(filters, [("version".to_string(), Filter::Standard), ("cmdline".to_string(), Filter::KernelCmdline), ("bios".to_string(), Filter::Standard)]);
}

#[test]
fn kernel_info_partial_and_failing() {
    let w = World::new("kinfo-partial");
    w.write("proc/version", "Linux version 7.2.14\n");
    let out = w.collect(r#"{"probe":"kernel_info"}"#);
    assert_eq!(out.status, Status::Ok);
    assert!(out.notes.iter().any(|n| n.starts_with("cmdline: not available")), "{:?}", out.notes);
    assert!(out.notes.iter().any(|n| n.starts_with("bios: not available")));

    let w = World::new("kinfo-none");
    let out = w.collect(r#"{"probe":"kernel_info"}"#);
    assert!(matches!(out.status, Status::Failed(_)), "{:?}", out.status);

    // riesig: die Kommandozeile wird gelesen, aber nur bis zur Grenze
    let w = World::new("kinfo-giant");
    w.write("proc/version", "x".repeat(50_000));
    w.write("proc/cmdline", "a=b ".repeat(10_000));
    let out = w.collect(r#"{"probe":"kernel_info"}"#);
    assert_sane(&out);
}

#[test]
fn cmdline_redaction_rules() {
    use super::boot::redact_cmdline;
    assert_eq!(redact_cmdline("root=/dev/sda1 quiet"), ("root=/dev/sda1 quiet".to_string(), 0));
    assert_eq!(
        redact_cmdline("password=x ro Token=y NOKEY=z key"),
        ("password=[REDACTED:kernel_param] ro Token=[REDACTED:kernel_param] NOKEY=[REDACTED:kernel_param] key".to_string(), 3)
    );
    assert_eq!(redact_cmdline("vconsole.keymap=de keyboard=us").1, 0);
    assert_eq!(redact_cmdline("  a   b  "), ("a b".to_string(), 0));
}

// ---- pacman_log ------------------------------------------------------------------------------------------------------

fn pacman_fixture() -> String {
    let mut s = String::new();
    s.push_str("[2026-10-20T08:00:00+0100] [ALPM] upgraded linux (7.2.10.arch1-1 -> 7.2.11.arch1-1)\n");
    s.push_str("[2026-11-03T22:15:00+0100] [PACMAN] Running 'pacman -Syu'\n");
    s.push_str("[2026-11-03T22:15:01+0100] [ALPM] transaction started\n");
    s.push_str("[2026-11-03T22:15:02+0100] [ALPM] upgraded linux (7.2.13.arch1-1 -> 7.2.14.arch1-1)\n");
    s.push_str("[2026-11-03T22:15:03+0100] [ALPM] upgraded nvidia-open-dkms (615.77.01-1 -> 615.78.08-1)\n");
    s.push_str("[2026-11-03T22:15:04+0100] [ALPM] installed foo-tools (1.0-1)\n");
    s.push_str("[2026-11-03T22:15:05+0100] [ALPM-SCRIPTLET] ran hooks\n");
    s.push_str("[2026-11-03T22:15:06+0100] [ALPM] removed oldpkg (0.9-1)\n");
    s.push_str("[2026-11-03T22:15:07+0100] [ALPM] downgraded mesa (1:26.2.7-1 -> 1:26.2.6-1)\n");
    s.push_str("[2026-11-04 08:00] [ALPM] upgraded after-the-window (1-1 -> 2-1)\n");
    s.push_str("[2026-11-04T10:30:00+0100] [ALPM] upgraded after-the-crash (1-1 -> 2-1)\n");
    s
}

#[test]
fn pacman_log_lists_the_package_changes_before_the_end_of_a_boot() {
    let w = World::new("pacman-normal");
    journalctl_with_boots(&w, vec![]);
    w.write("var/log/pacman.log", pacman_fixture());
    let out = w.collect(r#"{"probe":"pacman_log","boot":-1,"days":1}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("2026-11-03T22:15:02+0100 upgraded linux (7.2.13.arch1-1 -> 7.2.14.arch1-1)"), "{text}");
    assert!(
        text.contains("upgraded nvidia-open-dkms")
            && text.contains("installed foo-tools")
            && text.contains("removed oldpkg")
            && text.contains("downgraded mesa"),
        "{text}"
    );
    assert!(text.contains("after-the-window"), "the old date format (08:00) lies inside: {text}");
    assert!(!text.contains("after-the-crash"), "{text}");
    assert!(!text.contains("transaction started") && !text.contains("Running 'pacman") && !text.contains("hooks"), "{text}");
    assert!(!text.contains("7.2.11"), "older than the window: {text}");
    // Auch mit 7 Tagen (ab dem 28.10.) bleibt der Eintrag vom 20.10. draußen
    assert!(!w.collect(r#"{"probe":"pacman_log","boot":-1,"days":7}"#).text().contains("7.2.11"));
}

#[test]
fn pacman_log_empty_missing_and_giant() {
    let w = World::new("pacman-empty");
    journalctl_with_boots(&w, vec![]);
    w.write("var/log/pacman.log", "[2026-01-01T00:00:00+0100] [ALPM] upgraded old (1-1 -> 2-1)\n");
    let out = w.collect(r#"{"probe":"pacman_log","boot":-1,"days":1}"#);
    assert_eq!(out.status, Status::Empty);
    assert!(out.text().contains("No package was installed"), "{}", out.text());

    let w = World::new("pacman-missing");
    journalctl_with_boots(&w, vec![]);
    let out = w.collect(r#"{"probe":"pacman_log","boot":-1,"days":1}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("pacman.log")), "{:?}", out.status);

    let w = World::new("pacman-giant");
    journalctl_with_boots(&w, vec![]);
    let mut big = String::new();
    for i in 0..40_000u32 {
        big.push_str(&format!(
            "[2026-11-03T{:02}:{:02}:{:02}+0100] [ALPM] upgraded filler-package-number-{i} ({i}.0-1 -> {i}.1-1)\n",
            12 + i / 20_000 * 6,
            (i / 60) % 60,
            i % 60
        ));
    }
    big.push_str("[2026-11-04T09:00:00+0100] [ALPM] upgraded linux (1-1 -> 2-1)\n");
    assert!(big.len() > 2_000_000);
    w.write("var/log/pacman.log", big);
    let out = w.collect(r#"{"probe":"pacman_log","boot":-1,"days":3}"#);
    let text = text_of(&out);
    assert!(text.contains("upgraded linux (1-1 -> 2-1)"), "kernel changes are kept when the budget runs out: {text}");
    assert!(out.notes.iter().any(|n| n.contains("package changes of less interest were left out")), "{:?}", out.notes);
    assert!(out.raw_bytes > 2_000_000);
}

// ---- driver_versions -------------------------------------------------------------------------------------------------

#[test]
fn driver_versions_lists_packages_and_names_the_missing_ones() {
    let w = World::new("drivers");
    w.fake(
        Program::Pacman,
        &[arm("-Q linux", "linux 7.2.14.arch1-1\nnvidia-utils 615.78.08-1\nmesa 1:26.2.6-1\ndocker 1:29.9.2-1\nsystemd 262.1-1\n")
            .err("error: package 'nvidia-open-dkms' was not found\n")
            .exit(1)],
    );
    let out = w.collect(r#"{"probe":"driver_versions"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    for needle in ["linux", "7.2.14.arch1-1", "nvidia-open-dkms  not installed", "nvidia-utils", "615.78.08-1", "mesa", "1:26.2.6-1", "systemd", "262.1-1"] {
        assert!(text.contains(needle), "{needle} missing in\n{text}");
    }

    let w = World::new("drivers-error");
    let out = w.collect(r#"{"probe":"driver_versions"}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("pacman is not installed")), "{:?}", out.status);

    let w = World::new("drivers-fail");
    w.fake(Program::Pacman, &[arm("", "").err("error: could not open database\n").exit(1)]);
    let out = w.collect(r#"{"probe":"driver_versions"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("could not open database")), "{:?}", out.status);
}

// =================================================================================================================
// Units

const UNITS_SYSTEM: &str = r#"[{"unit":"docker.service","load":"loaded","active":"active","sub":"running","description":"Docker Application Container Engine"},
{"unit":"docker.socket","load":"loaded","active":"active","sub":"running","description":"Docker Socket"},
{"unit":"containerd.service","load":"loaded","active":"active","sub":"running","description":"containerd"},
{"unit":"getty@tty1.service","load":"loaded","active":"active","sub":"running","description":"Getty"},
{"unit":"-.mount","load":"loaded","active":"active","sub":"mounted","description":"Root"},
{"unit":"--user.service","load":"loaded","active":"active","sub":"running","description":"hostile"},
{"unit":"1234","load":"loaded","active":"active","sub":"running","description":"a pid"},
{"unit":"../../etc/x.service","load":"loaded","active":"active","sub":"running","description":"path"}]"#;
const UNITS_USER: &str = r#"[{"unit":"pipewire.service","load":"loaded","active":"active","sub":"running","description":"PipeWire"},
{"unit":"pipewire-pulse.service","load":"loaded","active":"active","sub":"running","description":"PipeWire PulseAudio"}]"#;

fn systemctl(w: &World, extra: Vec<Arm>) {
    let mut arms = vec![arm("--user list-units", UNITS_USER), arm("list-units", UNITS_SYSTEM)];
    arms.extend(extra);
    w.fake(Program::Systemctl, &arms);
}

#[test]
fn failed_units_system_and_user() {
    let w = World::new("failed-normal");
    w.fake(
        Program::Systemctl,
        &[
            arm("--user --failed", "[]"),
            arm("--failed", r#"[{"unit":"cups.service","load":"loaded","active":"failed","sub":"failed","description":"CUPS Scheduler"},{"unit":"nas.mount","load":"loaded","active":"failed","sub":"failed","description":"NAS"}]"#),
        ],
    );
    let out = w.collect(r#"{"probe":"failed_units"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("## system\nunit"), "{text}");
    assert!(text.contains("cups.service  failed/failed  CUPS Scheduler"), "{text}");
    assert!(text.contains("## user\nNo failed units."), "{text}");
}

#[test]
fn failed_units_empty_partial_failing_and_giant() {
    let w = World::new("failed-empty");
    w.fake(Program::Systemctl, &[arm("--failed", "[]")]);
    let out = w.collect(r#"{"probe":"failed_units"}"#);
    assert_eq!(out.status, Status::Empty);

    // User-Manager nicht erreichbar: die Units des Systems bleiben
    let w = World::new("failed-partial");
    w.fake(
        Program::Systemctl,
        &[
            arm("--user --failed", "").err("Failed to connect to bus: No medium found\n").exit(1),
            arm("--failed", r#"[{"unit":"cups.service","load":"loaded","active":"failed","sub":"failed","description":"CUPS"}]"#),
        ],
    );
    let out = w.collect(r#"{"probe":"failed_units"}"#);
    assert!(matches!(&out.status, Status::Partial(m) if m.contains("user manager") && m.contains("No medium found")), "{:?}", out.status);
    assert!(text_of(&out).contains("cups.service"));

    let w = World::new("failed-fail");
    w.fake(Program::Systemctl, &[arm("", "").err("Failed to connect to system bus\n").exit(1)]);
    let out = w.collect(r#"{"probe":"failed_units"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("system manager") && m.contains("user manager")), "{:?}", out.status);

    let w = World::new("failed-giant");
    let mut list = String::from("[");
    for i in 0..4000 {
        if i > 0 {
            list.push(',');
        }
        list.push_str(&format!(r#"{{"unit":"unit-number-{i}.service","load":"loaded","active":"failed","sub":"failed","description":"{}"}}"#, "d".repeat(80)));
    }
    list.push(']');
    assert!(list.len() > super::collect::RAW_LIMIT);
    w.fake(Program::Systemctl, &[arm("--user --failed", "[]"), arm("--failed", list)]);
    let out = w.collect(r#"{"probe":"failed_units"}"#);
    let text = text_of(&out);
    assert!(text.contains("unit-number-0.service"));
    assert!(out.notes.iter().any(|n| n.contains("cut at the capture limit")), "{:?}", out.notes);
}

const STATUS_TEXT: &str = "● docker.service - Docker Application Container Engine
     Loaded: loaded (/usr/lib/systemd/system/docker.service; enabled; preset: disabled)
     Active: active (running) since Wed 2026-11-04 10:05:40 CET; 1 day ago
TriggeredBy: ● docker.socket
       Docs: https://docs.docker.com
    Process: 811 ExecStartPre=/usr/bin/foo --password=hunter2 (code=exited, status=0/SUCCESS)
   Main PID: 812 (dockerd)
      Tasks: 41
     Memory: 1.2G
        CPU: 2min
     CGroup: /system.slice/docker.service
             ├─ 812 /usr/bin/dockerd -H fd:// --containerd=/run/containerd/containerd.sock --token=abc123
             └─1320 \"sshd: user [priv]\"
";

#[test]
fn unit_status_resolves_the_name_and_drops_process_arguments() {
    let w = World::new("status-normal");
    // Das Fake antwortet nur auf den voll aufgelösten Namen hinter `--`
    systemctl(&w, vec![arm("status --no-pager -n 0 -- docker.service", STATUS_TEXT)]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"docker"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert_eq!(out.resolved, ["unit docker → docker.service (system manager)"]);
    assert!(text.contains("Active: active (running)"), "{text}");
    assert!(text.contains("Process: 811 ExecStartPre=foo (code=exited, status=0/SUCCESS)"), "{text}");
    assert!(text.contains("├─ 812 dockerd") || text.contains("├─ 812 dockerd\n"), "{text}");
    assert!(!text.contains("hunter2") && !text.contains("abc123") && !text.contains("--containerd") && !text.contains("[priv]"), "{text}");
    assert!(out.redactions.iter().any(|r| r.kind == "command_arguments" && r.count == 3), "{:?}", out.redactions);
}

#[test]
fn unit_status_exit_codes_failures_and_invented_names() {
    // Inaktive Unit: Exit 3 mit Ausgabe ist ein Ergebnis
    let w = World::new("status-inactive");
    systemctl(&w, vec![arm("status --no-pager -n 0 -- docker.service", "● docker.service\n     Active: inactive (dead)\n").exit(3)]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"docker.service"}"#);
    assert_eq!(out.status, Status::Ok);
    assert!(out.notes.iter().any(|n| n.contains("exited with code 3")), "{:?}", out.notes);

    // Keine Ausgabe und Exit 4
    let w = World::new("status-gone");
    systemctl(&w, vec![arm("status", "").err("Unit docker.service could not be found.\n").exit(4)]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"docker"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("code 4") && m.contains("could not be found")), "{:?}", out.status);

    // Erfundener Name, mehrdeutiger Name: das Programm wird nie mit dem Begriff aufgerufen
    let w = World::new("status-invented");
    systemctl(&w, vec![arm("status", "SHOULD NOT RUN")]);
    for (term, needle) in [("nginx", "no unit matches"), ("pipe", "matches several units"), ("1234", "no unit matches"), ("user", "no unit matches")] {
        let out = w.collect(&format!(r#"{{"probe":"unit_status","unit":"{term}"}}"#));
        assert!(matches!(&out.status, Status::Unavailable(m) if m.contains(needle)), "{term}: {:?}", out.status);
        assert!(!out.text().contains("SHOULD NOT RUN"));
    }
    let out = w.collect(r#"{"probe":"unit_status","unit":"pipe"}"#);
    assert!(out.text().contains("pipewire.service") && out.text().contains("pipewire-pulse.service"), "{}", out.text());
}

#[test]
fn units_are_resolved_once_per_collection() {
    let w = World::new("status-cache");
    let counter = w.base().join("calls");
    let script = format!("echo x >> '{}'", counter.display());
    systemctl(&w, vec![]);
    // Zähler: das Fake schreibt bei jedem Aufruf eine Zeile; dafür ein eigenes Skript um das Fake
    let bin = w.base().join("bin/systemctl");
    let original = fs::read_to_string(&bin).unwrap();
    fs::write(&bin, original.replacen("args=\"$*\"\n", &format!("args=\"$*\"\n{script}\n"), 1)).unwrap();
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    for _ in 0..3 {
        w.collect_with(&mut collector, r#"{"probe":"unit_status","unit":"docker"}"#);
    }
    // Zwei Listen (System, User) einmal, dazu je ein `status`-Aufruf (hier ohne Antwort, Exit 99)
    let calls = fs::read_to_string(&counter).unwrap().lines().count();
    assert_eq!(calls, 2 + 3, "lists are fetched once, status three times");
}

#[test]
fn user_units_get_the_user_flag() {
    let w = World::new("status-user");
    systemctl(&w, vec![arm("--user status --no-pager -n 0 -- pipewire.service", "● pipewire.service\n   Active: active (running)\n")]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"pipewire.service"}"#);
    assert_eq!(out.status, Status::Ok, "{}", out.text());
    assert_eq!(out.resolved, ["unit pipewire.service → pipewire.service (user manager)"]);
}

#[test]
fn unit_lists_that_cannot_be_read_stop_the_probe() {
    let w = World::new("status-nolist");
    w.fake(Program::Systemctl, &[arm("list-units", "").err("Failed to connect to bus\n").exit(1)]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"docker"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("cannot look up the units of the system manager")), "{:?}", out.status);
    // Nur der User-Manager fehlt: die Units des Systems zählen
    let w = World::new("status-nouser");
    w.fake(Program::Systemctl, &[arm("--user list-units", "").err("no bus\n").exit(1), arm("list-units", UNITS_SYSTEM), arm("status", "ok\n")]);
    let out = w.collect(r#"{"probe":"unit_status","unit":"docker"}"#);
    assert_eq!(out.status, Status::Ok, "{}", out.text());
}

#[test]
fn unit_show_drops_empty_properties() {
    let w = World::new("show");
    systemctl(
        &w,
        vec![arm("show --no-pager -p", "Id=docker.service\nResult=success\nExecMainStatus=0\nNRestarts=3\nActiveExitTimestamp=\nMemoryCurrent=1234\n")],
    );
    let out = w.collect(r#"{"probe":"unit_show","unit":"docker"}"#);
    let text = text_of(&out);
    assert!(text.contains("NRestarts=3") && text.contains("Result=success"), "{text}");
    assert!(!text.contains("ActiveExitTimestamp"), "{text}");

    let w = World::new("show-empty");
    systemctl(&w, vec![arm("show", "A=\nB=\n")]);
    assert_eq!(w.collect(r#"{"probe":"unit_show","unit":"docker"}"#).status, Status::Empty);
}

#[test]
fn unit_journal_uses_the_resolved_unit_and_the_time_window() {
    let w = World::new("ujournal");
    let mut journal = String::new();
    for i in 0..5u32 {
        journal.push_str(&journal_line(&format!("2026-11-05T11:5{i}:00"), "dockerd[812]", &format!("msg {}", alpha(i))));
        journal.push('\n');
    }
    w.fake(Program::Systemctl, &[arm("--user list-units", UNITS_USER), arm("list-units", UNITS_SYSTEM)]);
    w.fake(
        Program::Journalctl,
        &[arm("--list-boots", boots_json()), arm("-u docker.service --since=-1h -n 200", journal), arm("--user -u pipewire.service --boot=0", "pw line\n")],
    );
    let out = w.collect(r#"{"probe":"unit_journal","unit":"docker","since":"1h","lines":200}"#);
    assert!(text_of(&out).contains(&format!("msg {}", alpha(4))), "{}", out.text());
    let out = w.collect(r#"{"probe":"unit_journal","unit":"pipewire","since":"boot","lines":50}"#);
    assert!(text_of(&out).contains("pw line"), "{}", out.text());

    let w2 = World::new("ujournal-empty");
    w2.fake(Program::Systemctl, &[arm("--user list-units", UNITS_USER), arm("list-units", UNITS_SYSTEM)]);
    w2.fake(Program::Journalctl, &[arm("", "-- No entries --\n")]);
    assert_eq!(w2.collect(r#"{"probe":"unit_journal","unit":"docker","since":"15m","lines":50}"#).status, Status::Empty);
}

#[test]
fn timers_are_sorted_by_next_run() {
    let w = World::new("timers");
    let t = |secs: i64| us(secs);
    let json = format!(
        r#"[{{"next":{},"left":0,"last":{},"passed":0,"unit":"later.timer","activates":"later.service"}},{{"next":{},"left":0,"last":0,"passed":0,"unit":"soon.timer","activates":"soon.service"}},{{"next":0,"left":0,"last":0,"passed":0,"unit":"never.timer","activates":"never.service"}}]"#,
        t(now() + 2 * DAY),
        t(now() - 3600),
        t(now() + 600)
    );
    w.fake(Program::Systemctl, &[arm("list-timers", json)]);
    let out = w.collect(r#"{"probe":"timers"}"#);
    let text = text_of(&out);
    let (soon, later, never) = (text.find("soon.timer").unwrap(), text.find("later.timer").unwrap(), text.find("never.timer").unwrap());
    assert!(soon < later && later < never, "{text}");
    assert!(text.contains("(in 10 min)"), "{text}");
    assert!(text.contains("(1 h 00 min ago)") || text.contains("(60 min ago)"), "{text}");

    let w = World::new("timers-empty");
    w.fake(Program::Systemctl, &[arm("list-timers", "[]")]);
    assert_eq!(w.collect(r#"{"probe":"timers"}"#).status, Status::Empty);
    let w = World::new("timers-error");
    w.fake(Program::Systemctl, &[arm("", "").err("boom\n").exit(1)]);
    assert!(matches!(w.collect(r#"{"probe":"timers"}"#).status, Status::Failed(_)));
}

fn coredump_json(n: usize) -> String {
    let mut s = String::from("[");
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            r#"{{"time":{},"pid":{},"uid":1000,"gid":1000,"sig":11,"corefile":"missing","exe":"/usr/bin/dataroom-worker","size":{}}}"#,
            us(now() - 3600 + i as i64),
            5000 + i,
            1_000_000 + i
        ));
    }
    s.push_str(&format!(r#",{{"time":{},"pid":77,"uid":0,"gid":0,"sig":6,"corefile":"present","exe":"/usr/bin/other","size":2048}}]"#, us(now() - 60)));
    s
}

#[test]
fn coredumps_summarise_a_series() {
    let w = World::new("cores");
    w.fake(Program::Coredumpctl, &[arm("list --since=-6h", coredump_json(76))]);
    let out = w.collect(r#"{"probe":"coredumps","since":"6h"}"#);
    let text = text_of(&out);
    assert!(text.contains("77 core dumps"), "{text}");
    assert!(text.contains("76     SIGSEGV  /usr/bin/dataroom-worker"), "{text}");
    assert!(text.contains("1      SIGABRT  /usr/bin/other"), "{text}");
    // Die Liste zeigt nur die jüngsten 20
    assert!(text.lines().filter(|l| l.contains("dataroom-worker") && l.contains("missing")).count() <= 20, "{text}");
}

#[test]
fn coredumps_since_boot_uses_the_uptime_and_the_empty_case() {
    let w = World::new("cores-boot");
    w.write("proc/uptime", "3661.52 9999.00\n");
    w.fake(Program::Coredumpctl, &[arm("--since=-3661s", "[]")]);
    let out = w.collect(r#"{"probe":"coredumps","since":"boot"}"#);
    assert_eq!(out.status, Status::Empty, "{}", out.text());

    let w = World::new("cores-none");
    w.fake(Program::Coredumpctl, &[arm("list", "").err("No coredumps found.\n").exit(1)]);
    let out = w.collect(r#"{"probe":"coredumps","since":"1h"}"#);
    assert_eq!(out.status, Status::Empty, "{:?}", out.status);

    // Ohne /proc/uptime geht `since: boot` nicht
    let w = World::new("cores-nouptime");
    w.fake(Program::Coredumpctl, &[arm("list", "[]")]);
    let out = w.collect(r#"{"probe":"coredumps","since":"boot"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("uptime")), "{:?}", out.status);

    let w = World::new("cores-error");
    let out = w.collect(r#"{"probe":"coredumps","since":"1h"}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("coredumpctl is not installed")));

    // riesig: mehr als die Grenze für die Rohausgabe
    let w = World::new("cores-giant");
    w.fake(Program::Coredumpctl, &[arm("list", coredump_json(3000))]);
    let out = w.collect(r#"{"probe":"coredumps","since":"24h"}"#);
    let text = text_of(&out);
    assert!(text.contains("SIGSEGV"), "{text}");
}

// =================================================================================================================
// Docker

fn docker_list() -> String {
    let web = format!(
        r#"{{"Command":"\"/bin/sh -c 'export DB_PASSWORD=hunter2'\"","CreatedAt":"2026-11-05 12:07:06 +0100 CET","ID":"{CID_WEB}","Image":"nginx:1.27","Labels":"com.example.token=abc123,author=me","Mounts":"vol","Names":"dataroom-web-1","Networks":"dataroom_default","Ports":"0.0.0.0:8080->80/tcp","RunningFor":"2 hours ago","State":"running","Status":"Up 2 hours (healthy)"}}"#
    );
    let db = format!(
        r#"{{"Command":"\"postgres\"","ID":"{CID_DB}","Image":"postgres:16","Labels":"","Names":"dataroom-db-1","Ports":"","State":"restarting","Status":"Restarting (139) 5 seconds ago"}}"#
    );
    let bad = r#"{"Command":"\"sleep\"","ID":"short","Image":"x","Labels":"k=v","Names":"badid","State":"exited","Status":"Exited"}"#;
    format!("{web}\n{db}\n{bad}\n")
}

fn docker_world(extra: Vec<Arm>) -> World {
    let w = World::new("docker");
    let mut arms = vec![arm("ps -a --no-trunc", docker_list()), arm("ps -a --format json", docker_list())];
    arms.extend(extra);
    w.fake(Program::Docker, &arms);
    w
}

#[test]
fn docker_ps_removes_labels_and_commands() {
    let w = docker_world(vec![]);
    let out = w.collect(r#"{"probe":"docker_ps"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains("3 containers: 1 exited, 1 restarting, 1 running"), "{text}");
    assert!(
        text.contains("dataroom-web-1") && text.contains("a1b2c3d4e5f6") && text.contains("Up 2 hours (healthy)") && text.contains("0.0.0.0:8080->80/tcp"),
        "{text}"
    );
    for secret in ["hunter2", "abc123", "DB_PASSWORD", "com.example", "sleep", "/bin/sh"] {
        assert!(!text.contains(secret), "{secret} leaked:\n{text}");
    }
    let r: Vec<(&str, usize)> = out.redactions.iter().map(|r| (r.kind, r.count)).collect();
    assert_eq!(r, [("docker_labels", 2), ("docker_command", 3)]);
}

#[test]
fn docker_ps_empty_error_and_giant() {
    let w = World::new("dps-empty");
    w.fake(Program::Docker, &[arm("ps", "")]);
    assert_eq!(w.collect(r#"{"probe":"docker_ps"}"#).status, Status::Empty);

    let w = World::new("dps-error");
    w.fake(Program::Docker, &[arm("ps", "").err("Cannot connect to the Docker daemon at unix:///var/run/docker.sock\n").exit(1)]);
    let out = w.collect(r#"{"probe":"docker_ps"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("Cannot connect to the Docker daemon")), "{:?}", out.status);

    let w = World::new("dps-giant");
    let mut big = String::new();
    for i in 0..3000 {
        big.push_str(&format!(
            r#"{{"Command":"x","ID":"{:064x}","Image":"img:{i}","Labels":"{}","Names":"c{i}","Ports":"","State":"running","Status":"Up"}}"#,
            i,
            "l=v,".repeat(50)
        ));
        big.push('\n');
    }
    w.fake(Program::Docker, &[arm("ps", big)]);
    let out = w.collect(r#"{"probe":"docker_ps"}"#);
    let text = text_of(&out);
    assert!(text.contains("c0 "), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("more lines are not shown") || n.contains("capture limit")), "{:?}", out.notes);
}

const INSPECT_JSON: &str = r#"{"status":"restarting","running":false,"restarting":true,"oom_killed":false,"exit_code":139,"error":"","started_at":"2026-11-05T09:00:00.1Z","finished_at":"2026-11-05T09:00:05.2Z","health":null,"health_failing_streak":null,"restart_count":14,"image":"postgres:16","mounts":[{"type":"bind","source":"/srv/pg","destination":"/var/lib/postgresql","rw":true},{"type":"volume","source":"/var/lib/docker/volumes/x/_data","destination":"/cfg","rw":false}]}"#;

#[test]
fn docker_state_uses_the_full_id_and_the_container_type() {
    // Das Fake antwortet nur, wenn die volle ID hinter `--` steht und `--type=container` davor
    let w = World::new("dstate");
    w.fake(Program::Docker, &[arm(&format!("-- {CID_DB}"), INSPECT_JSON), arm("ps -a --no-trunc", docker_list())]);
    let out = w.collect(r#"{"probe":"docker_state","container":"db"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert_eq!(out.resolved, [format!("container db → dataroom-db-1 ({})", &CID_DB[..12])]);
    assert!(text.contains("status: restarting (running no, restarting yes, OOM killed no)"), "{text}");
    assert!(text.contains("exit code: 139") && text.contains("restart count: 14") && text.contains("image: postgres:16"), "{text}");
    assert!(text.contains("bind /srv/pg → /var/lib/postgresql (rw)") && text.contains("volume /var/lib/docker/volumes/x/_data → /cfg (ro)"), "{text}");
    assert!(!text.contains("health:"), "{text}");
}

#[test]
fn docker_state_rejects_invented_ambiguous_and_short_ids() {
    let w = World::new("dstate-names");
    w.fake(Program::Docker, &[arm("ps -a --no-trunc", docker_list()), arm("inspect", "SHOULD NOT RUN")]);
    for (term, needle) in [("nope", "no container matches"), ("dataroom", "matches several containers"), ("--all", ""), ("a1b", "no container matches")] {
        if needle.is_empty() {
            assert!(crate::diagnose::probe::parse_validated(&format!(r#"{{"probe":"docker_state","container":"{term}"}}"#)).is_err());
            continue;
        }
        let out = w.collect(&format!(r#"{{"probe":"docker_state","container":"{term}"}}"#));
        assert!(matches!(&out.status, Status::Unavailable(m) if m.contains(needle)), "{term}: {:?}", out.status);
        assert!(!out.text().contains("SHOULD NOT RUN"));
    }
    // Anfang der ID (vier Zeichen) und Name treffen genau einen
    let w = World::new("dstate-prefix");
    w.fake(Program::Docker, &[arm(&format!("-- {CID_WEB}"), INSPECT_JSON), arm("ps -a --no-trunc", docker_list())]);
    for term in ["a1b2", "dataroom-web-1", "web"] {
        let out = w.collect(&format!(r#"{{"probe":"docker_state","container":"{term}"}}"#));
        assert_eq!(out.status, Status::Ok, "{term}: {:?} {}", out.status, out.text());
    }
}

#[test]
fn docker_state_bad_output_and_error() {
    let w = World::new("dstate-bad");
    w.fake(Program::Docker, &[arm("ps -a --no-trunc", docker_list()), arm("inspect", "not json")]);
    let out = w.collect(r#"{"probe":"docker_state","container":"web"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("not the expected JSON")), "{:?}", out.status);
    let w = World::new("dstate-err");
    w.fake(Program::Docker, &[arm("ps -a --no-trunc", docker_list()), arm("inspect", "").err("Error: No such container\n").exit(1)]);
    let out = w.collect(r#"{"probe":"docker_state","container":"web"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("No such container")), "{:?}", out.status);
    // Docker nicht erreichbar: schon die Liste scheitert, und das steht im Status
    let w = World::new("dstate-nodocker");
    w.fake(Program::Docker, &[arm("", "").err("Cannot connect to the Docker daemon\n").exit(1)]);
    let out = w.collect(r#"{"probe":"docker_state","container":"web"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("cannot look up the containers")), "{:?}", out.status);
}

#[test]
fn docker_logs_merge_both_streams_fold_repeats_and_keep_the_end() {
    let w = World::new("dlogs");
    let mut out_lines = String::new();
    let mut err_lines = String::new();
    for i in 0..40u32 {
        out_lines.push_str(&format!("2026-11-05T10:00:{:02}.000000000Z GET /health 200 ok\n", i % 60));
    }
    for i in 0..30u32 {
        err_lines.push_str(&format!("2026-11-05T10:01:{:02}.100000000Z worker[{}] segfault at {:08x} in strlen\n", i, 100 + i, 0x7f00 + i));
    }
    err_lines.push_str("2026-11-05T10:02:00.000000000Z FATAL: password=hunter2 refused\n");
    w.fake(Program::Docker, &[arm(&format!("-- {CID_WEB}"), out_lines).err(&err_lines), arm("ps -a --no-trunc", docker_list())]);
    let out = w.collect(r#"{"probe":"docker_logs","container":"web","since":"1h","lines":500}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("… repeated 40×"), "{text}");
    assert!(text.contains("… repeated 30×") && text.contains("numbers differ"), "{text}");
    // Reihenfolge nach Zeit: erst die GET-Zeilen, dann die Segfaults, dann der letzte Fehler
    let (a, b, c) = (text.find("GET /health").unwrap(), text.find("segfault").unwrap(), text.find("FATAL").unwrap());
    assert!(a < b && b < c, "{text}");
    // Die Probe schwärzt nichts selbst; Geheimnisse im Log entfernt erst der allgemeine Filter (Aufgabe 4)
    assert!(text.contains("hunter2"));
    assert!(out.redactions.is_empty());
}

/// Messschritt Diagnose (Aufgabe 10b): eine Absturzschleife mit Backtrace (Block aus vier Zeilen, zwölfmal) ergibt eine
/// Sammelzeile, nicht vier gleiche untereinander.
#[test]
fn docker_logs_fold_a_repeated_crash_block_into_one_line() {
    let w = World::new("dlogs-block");
    let mut lines = String::new();
    for r in 0..12u32 {
        for (i, l) in
            ["worker 2.4.0 starting (pid 1)", "storage: opening backend", "*** SIGSEGV (address 0x0) received by PID 1", "#0  0x00007f3a1c2b8f3d in strlen ()"]
                .iter()
                .enumerate()
        {
            lines.push_str(&format!("2026-11-05T10:{:02}:{:02}.000000000Z {l}\n", r, i));
        }
    }
    w.fake(Program::Docker, &[arm(&format!("-- {CID_WEB}"), lines), arm("ps -a --no-trunc", docker_list())]);
    let out = w.collect(r#"{"probe":"docker_logs","container":"web","since":"1h","lines":500}"#);
    let text = text_of(&out);
    assert_eq!(text.lines().filter(|l| l.contains("× more of the")).count(), 1, "{text}");
    assert!(text.contains("… 9× more of the 4 messages above omitted (similar lines, last at 2026-11-05T10:10:03.000000000Z)"), "{text}");
    assert_eq!(text.matches("SIGSEGV").count(), 3, "{text}");
}

#[test]
fn docker_logs_empty_error_since_boot_and_giant() {
    let w = World::new("dlogs-empty");
    w.fake(Program::Docker, &[arm(&format!("-- {CID_WEB}"), ""), arm("ps -a --no-trunc", docker_list())]);
    assert_eq!(w.collect(r#"{"probe":"docker_logs","container":"web","since":"15m","lines":50}"#).status, Status::Empty);

    let w = World::new("dlogs-err");
    w.fake(
        Program::Docker,
        &[
            arm(&format!("-- {CID_WEB}"), "").err("Error response from daemon: configured logging driver does not support reading\n").exit(1),
            arm("ps -a --no-trunc", docker_list()),
        ],
    );
    let out = w.collect(r#"{"probe":"docker_logs","container":"web","since":"15m","lines":50}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("logging driver")), "{:?}", out.status);

    // since: boot → Sekunden seit dem Start aus /proc/uptime
    let w = World::new("dlogs-boot");
    w.write("proc/uptime", "7200.9 1.0\n");
    w.fake(Program::Docker, &[arm("--since=7200s", "2026-11-05T10:00:00.000000000Z hello\n"), arm("ps -a --no-trunc", docker_list())]);
    let out = w.collect(r#"{"probe":"docker_logs","container":"web","since":"boot","lines":50}"#);
    assert!(text_of(&out).contains("hello"), "{:?} {}", out.status, out.text());

    let w = World::new("dlogs-giant");
    let mut big = String::new();
    for i in 0..80_000u32 {
        big.push_str(&format!("2026-11-05T10:{:02}:{:02}.{:09}Z line {} {}\n", (i / 60) % 60, i % 60, i, alpha(i), "x".repeat(40 + (i % 17) as usize)));
    }
    big.push_str("2026-11-05T11:59:59.000000000Z LAST LOG LINE\n");
    w.fake(Program::Docker, &[arm(&format!("-- {CID_WEB}"), big), arm("ps -a --no-trunc", docker_list())]);
    let out = w.collect(r#"{"probe":"docker_logs","container":"web","since":"24h","lines":500}"#);
    let text = text_of(&out);
    assert!(text.contains("LAST LOG LINE"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("capture limit")), "{:?}", out.notes);
}

#[test]
fn docker_df_and_stats() {
    let w = World::new("ddf");
    w.fake(
        Program::Docker,
        &[arm("system df", "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE\nImages          46        40        21.26GB   2.651GB (12%)\n")],
    );
    let text = text_of(&w.collect(r#"{"probe":"docker_df"}"#));
    assert!(text.contains("Images") && text.contains("2.651GB"), "{text}");

    let w = World::new("dstats");
    w.fake(
        Program::Docker,
        &[arm(
            "stats --no-stream",
            r#"{"BlockIO":"107MB / 815kB","CPUPerc":"0.28%","Container":"665982ad0616c1","ID":"665982ad0616","MemPerc":"0.29%","MemUsage":"188.4MiB / 62.69GiB","Name":"dataroom-seaweedfs-1","NetIO":"39.1kB / 126B","PIDs":"15"}
"#,
        )],
    );
    let text = text_of(&w.collect(r#"{"probe":"docker_stats"}"#));
    assert!(text.contains("dataroom-seaweedfs-1") && text.contains("188.4MiB / 62.69GiB") && text.contains("0.28%"), "{text}");

    let w = World::new("dstats-empty");
    w.fake(Program::Docker, &[arm("stats", "")]);
    assert_eq!(w.collect(r#"{"probe":"docker_stats"}"#).status, Status::Empty);
    let w = World::new("ddf-err");
    w.fake(Program::Docker, &[arm("system", "").err("Cannot connect\n").exit(1)]);
    assert!(matches!(w.collect(r#"{"probe":"docker_df"}"#).status, Status::Failed(_)));
}

// =================================================================================================================
// Network

const IP_ADDR: &str = r#"[{"ifindex":1,"ifname":"lo","flags":["LOOPBACK","UP","LOWER_UP"],"mtu":65536,"operstate":"UNKNOWN","link_type":"loopback","address":"00:00:00:00:00:00","addr_info":[{"family":"inet","local":"127.0.0.1","prefixlen":8,"scope":"host"}]},
{"ifindex":2,"ifname":"enp5s0","flags":["BROADCAST","MULTICAST","UP","LOWER_UP"],"mtu":1500,"operstate":"UP","link_type":"ether","address":"aa:bb:cc:dd:ee:ff","addr_info":[{"family":"inet","local":"192.0.2.5","prefixlen":24,"scope":"global"},{"family":"inet6","local":"fe80::1","prefixlen":64,"scope":"link"}]},
{"ifindex":7,"ifname":"veth1a2b","flags":["UP"],"mtu":1500,"operstate":"UP","address":"aa:bb:cc:00:00:01","addr_info":[]},
{"ifindex":8,"ifname":"veth3c4d","flags":["UP"],"mtu":1500,"operstate":"UP","address":"aa:bb:cc:00:00:02","addr_info":[]},
{"ifindex":9,"ifname":"veth5e6f","flags":[],"mtu":1500,"operstate":"DOWN","address":"aa:bb:cc:00:00:03","addr_info":[]}]"#;
const IP_ROUTE: &str = r#"[{"dst":"default","gateway":"192.0.2.1","dev":"enp5s0","protocol":"dhcp","metric":100,"flags":[]},{"dst":"192.0.2.0/24","dev":"enp5s0","protocol":"kernel","scope":"link","prefsrc":"192.0.2.5","flags":[]}]"#;

#[test]
fn net_links_summarises_interfaces_and_routes() {
    let w = World::new("links");
    w.fake(Program::Ip, &[arm("addr", IP_ADDR), arm("route", IP_ROUTE)]);
    let out = w.collect(r#"{"probe":"net_links"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains("## addresses") && text.contains("## routes"), "{text}");
    assert!(text.contains("2: enp5s0 UP mtu 1500 <UP,LOWER_UP> mac aa:bb:cc:dd:ee:ff inet 192.0.2.5/24 inet6 fe80::1/64 (link)"), "{text}");
    assert!(text.contains("1: lo UNKNOWN mtu 65536 <UP,LOWER_UP> inet 127.0.0.1/8 (host)"), "{text}");
    assert!(text.contains("veth*: 3 interfaces without address (1 DOWN, 2 UP)"), "{text}");
    assert!(!text.contains("veth1a2b"));
    assert!(text.contains("default via 192.0.2.1 dev enp5s0 proto dhcp metric 100"), "{text}");
    assert!(text.contains("192.0.2.0/24 dev enp5s0 proto kernel scope link src 192.0.2.5"), "{text}");
}

#[test]
fn net_links_partial_failing_and_not_json() {
    let w = World::new("links-partial");
    w.fake(Program::Ip, &[arm("addr", IP_ADDR), arm("route", "").err("RTNETLINK answers: Operation not permitted\n").exit(2)]);
    let out = w.collect(r#"{"probe":"net_links"}"#);
    assert!(matches!(&out.status, Status::Partial(m) if m.contains("routes")), "{:?}", out.status);

    let w = World::new("links-fail");
    let out = w.collect(r#"{"probe":"net_links"}"#);
    assert!(matches!(out.status, Status::Failed(_)), "{:?}", out.status);

    let w = World::new("links-text");
    w.fake(Program::Ip, &[arm("addr", "1: lo: <LOOPBACK> mtu 65536\n"), arm("route", "[]")]);
    let out = w.collect(r#"{"probe":"net_links"}"#);
    assert!(out.notes.iter().any(|n| n.contains("not valid JSON")), "{:?}", out.notes);
    assert!(text_of(&out).contains("1: lo:"));

    // riesig: 2000 Schnittstellen mit Adresse
    let w = World::new("links-giant");
    let mut big = String::from("[");
    for i in 0..2000 {
        if i > 0 {
            big.push(',');
        }
        big.push_str(&format!(r#"{{"ifindex":{i},"ifname":"if{i}","flags":["UP"],"mtu":1500,"operstate":"UP","address":"aa:bb:cc:00:{:02x}:{:02x}","addr_info":[{{"family":"inet","local":"10.0.{}.{}","prefixlen":24,"scope":"global"}}]}}"#, i / 256, i % 256, i / 256, i % 256));
    }
    big.push(']');
    w.fake(Program::Ip, &[arm("addr", big), arm("route", IP_ROUTE)]);
    let out = w.collect(r#"{"probe":"net_links"}"#);
    let text = text_of(&out);
    assert!(text.contains("## routes\ndefault via 192.0.2.1"), "the second command keeps its share: {text}");
}

#[test]
fn net_dns_and_reach_pass_the_text_through() {
    let w = World::new("dns");
    w.fake(Program::Resolvectl, &[arm("status", "Global\n       Protocols: +LLMNR\nLink 2 (enp5s0)\n    DNS Servers: 192.0.2.1\n")]);
    let text = text_of(&w.collect(r#"{"probe":"net_dns"}"#));
    assert!(text.contains("DNS Servers: 192.0.2.1"), "{text}");
    let w = World::new("dns-empty");
    w.fake(Program::Resolvectl, &[arm("status", "")]);
    assert_eq!(w.collect(r#"{"probe":"net_dns"}"#).status, Status::Empty);
}

#[test]
fn net_reach_needs_confirmation_and_a_configured_target() {
    let w = World::new("reach");
    w.fake(Program::Ping, &[arm("-c 3 -W 1 -- nas.example.org", "PING nas.example.org (192.0.2.10) 56(84) bytes of data.\n\n--- nas.example.org ping statistics ---\n3 packets transmitted, 0 received, 100% packet loss, time 2050ms\n").exit(1)]);
    // Ohne Bestätigung läuft nichts
    let probe = crate::diagnose::probe::parse_validated(r#"{"probe":"net_reach","target":"nas"}"#).unwrap();
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    let out = collector.collect(&probe, super::collect::CollectOptions::new(Duration::from_secs(10)));
    assert_eq!(out.status, Status::NeedsConfirmation);
    assert!(out.text().contains("confirmation"));
    assert!(out.parts.is_empty());
    // Mit Bestätigung: Exit 1 mit Ausgabe ist das Ergebnis „Paketverlust“
    let out = w.collect(r#"{"probe":"net_reach","target":"nas"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains("100% packet loss") && text.contains("ping exited with code 1"), "{text}");
    assert_eq!(out.resolved, ["target nas → nas.example.org"]);
    // Name ohne Rücksicht auf Groß-/Kleinschreibung, Adresse aus der Liste
    for term in ["NAS", "Nas"] {
        assert_eq!(w.collect(&format!(r#"{{"probe":"net_reach","target":"{term}"}}"#)).status, Status::Ok);
    }
    // Ein Ziel außerhalb der Liste wird nie gepingt
    let w2 = World::new("reach-foreign");
    w2.fake(Program::Ping, &[arm("", "pinged!")]);
    for term in ["example.org", "8.8.8.8", "gateway2"] {
        let out = w2.collect(&format!(r#"{{"probe":"net_reach","target":"{term}"}}"#));
        assert!(
            matches!(&out.status, Status::Unavailable(m) if m.contains("not a configured ping target") && m.contains("gateway")),
            "{term}: {:?}",
            out.status
        );
        assert!(!out.text().contains("pinged!"));
    }
    // Eine Config ohne Ziele
    let mut w3 = World::new("reach-none");
    w3.ctx.ping_targets.clear();
    w3.fake(Program::Ping, &[arm("", "pinged!")]);
    let out = w3.collect(r#"{"probe":"net_reach","target":"nas"}"#);
    assert!(matches!(&out.status, Status::Unavailable(m) if m.contains("no ping targets are configured")), "{:?}", out.status);
    // Eine Adresse der Liste mit unzulässigem Inhalt (Config-Fehler) geht nicht in ein argv
    let mut w4 = World::new("reach-bad");
    w4.ctx.ping_targets = vec![super::collect::PingTargetCfg { name: "evil".into(), address: "-f".into() }];
    w4.fake(Program::Ping, &[arm("", "pinged!")]);
    let out = w4.collect(r#"{"probe":"net_reach","target":"evil"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("not usable")), "{:?}", out.status);
    assert!(!out.text().contains("pinged!"));
}

#[test]
fn net_listen_groups_many_sockets_of_one_program() {
    let w = World::new("listen");
    let mut ss = String::new();
    ss.push_str("tcp LISTEN 0 4096 127.0.0.1:631 0.0.0.0:* \n");
    ss.push_str("tcp LISTEN 0 4096 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=800,fd=3))\n");
    for p in 0..20 {
        ss.push_str(&format!("udp UNCONN 0 0 0.0.0.0:{} 0.0.0.0:* users:((\"firefox\",pid=1958,fd={}))\n", 36000 + p, 200 + p));
    }
    w.fake(Program::Ss, &[arm("-Htulpn", ss)]);
    let out = w.collect(r#"{"probe":"net_listen"}"#);
    let text = text_of(&out);
    assert!(text.lines().any(|l| l.starts_with("tcp    LISTEN  0.0.0.0:22 ") && l.trim_end().ends_with("sshd")), "{text}");
    assert!(text.contains("127.0.0.1:631"), "{text}");
    assert!(text.contains("20 sockets, e.g. 0.0.0.0:36000, 0.0.0.0:36001, 0.0.0.0:36002"), "{text}");
    assert_eq!(text.matches("firefox").count(), 1, "{text}");
    assert!(!text.contains("pid="), "{text}");

    let w = World::new("listen-empty");
    w.fake(Program::Ss, &[arm("-H", "")]);
    assert_eq!(w.collect(r#"{"probe":"net_listen"}"#).status, Status::Empty);
}

fn tailscale_json() -> String {
    r#"{"Version":"1.80.0","TUN":true,"BackendState":"Running","AuthURL":"https://login.tailscale.com/a/secret-token","TailscaleIPs":["100.64.0.5"],
"Self":{"ID":"n1","PublicKey":"nodekey:aaaa","HostName":"blub","DNSName":"blub.tail1234.ts.net.","OS":"linux","TailscaleIPs":["100.64.0.5"],"Online":true,"Relay":"fra","Addrs":["203.0.113.7:41641"]},
"Health":["The Tailscale coordination server is unreachable"],
"Peer":{"nodekey:b1":{"HostName":"nas","DNSName":"nas.tail1234.ts.net.","Online":true,"Relay":"fra","TailscaleIPs":["100.64.0.9"],"LastSeen":"0001-01-01T00:00:00Z","ExitNode":false,"PublicKey":"nodekey:bbbb"},
"nodekey:b2":{"HostName":"phone","Online":false,"Relay":"","TailscaleIPs":["100.64.0.10"],"LastSeen":"2026-11-04T08:00:00Z","ExitNode":false}}}"#
        .to_string()
}

#[test]
fn net_tailscale_summarises_without_addresses_or_keys() {
    let w = World::new("ts");
    w.fake(Program::Tailscale, &[arm("status --json", tailscale_json())]);
    let out = w.collect(r#"{"probe":"net_tailscale"}"#);
    let text = text_of(&out);
    assert!(text.contains("backend: Running  version: 1.80.0"), "{text}");
    assert!(text.contains("health: The Tailscale coordination server is unreachable"), "{text}");
    assert!(text.contains("self: blub (blub.tail1234.ts.net) online=yes relay=fra os=linux"), "{text}");
    assert!(text.contains("peers: 2 (1 online)"), "{text}");
    assert!(text.contains("phone") && text.contains("2026-11-04T08:00:00Z"), "{text}");
    for leaked in ["100.64.", "nodekey", "secret-token", "203.0.113", "login.tailscale.com"] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
}

#[test]
fn net_tailscale_errors() {
    let w = World::new("ts-err");
    w.fake(Program::Tailscale, &[arm("", "").err("failed to connect to local tailscaled\n").exit(1)]);
    let out = w.collect(r#"{"probe":"net_tailscale"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("failed to connect")), "{:?}", out.status);
    let w = World::new("ts-bad");
    w.fake(Program::Tailscale, &[arm("", "Logged out.\n")]);
    let out = w.collect(r#"{"probe":"net_tailscale"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("not JSON")), "{:?}", out.status);
    // riesig: über der Grenze für die Rohausgabe lässt sich das JSON nicht mehr lesen
    let w = World::new("ts-giant");
    let mut peers = String::new();
    for i in 0..400 {
        if i > 0 {
            peers.push(',');
        }
        peers.push_str(&format!(r#""nodekey:{i}":{{"HostName":"host{i}","Online":true,"Relay":"fra","CapMap":"{}"}}"#, "c".repeat(900)));
    }
    let big = format!(r#"{{"Version":"1","BackendState":"Running","Peer":{{{peers}}}}}"#);
    assert!(big.len() > super::collect::RAW_LIMIT);
    w.fake(Program::Tailscale, &[arm("", big)]);
    let out = w.collect(r#"{"probe":"net_tailscale"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("was cut")), "{:?}", out.status);
    // Viele Gegenstellen unter der Grenze: gekürzt auf die Grenze des Katalogs
    let w = World::new("ts-many");
    let mut peers = String::new();
    for i in 0..300 {
        if i > 0 {
            peers.push(',');
        }
        peers.push_str(&format!(r#""nodekey:{i}":{{"HostName":"host{i}","Online":true,"Relay":"fra"}}"#));
    }
    w.fake(Program::Tailscale, &[arm("", format!(r#"{{"Version":"1","BackendState":"Running","Peer":{{{peers}}}}}"#))]);
    let out = w.collect(r#"{"probe":"net_tailscale"}"#);
    assert_eq!(out.status, Status::Ok);
    assert!(text_of(&out).contains("peers: 300 (300 online)"));
}

#[test]
fn net_mounts_strips_credentials() {
    let w = World::new("mounts");
    w.fake(
        Program::Findmnt,
        &[arm(
            "-J",
            r#"{"filesystems":[{"target":"/mnt/nas","source":"//192.0.2.10/plex","fstype":"cifs","options":"rw,relatime,vers=3.1.1,username=bob,password=hunter2,credentials=/etc/smb.cred,addr=192.0.2.10"},{"target":"/mnt/nfs","source":"nas:/export","fstype":"nfs4","options":"rw,vers=4.2"}]}"#,
        )],
    );
    let out = w.collect(r#"{"probe":"net_mounts"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok);
    assert!(text.contains("/mnt/nas  cifs  //192.0.2.10/plex  rw,relatime,vers=3.1.1,addr=192.0.2.10"), "{text}");
    assert!(text.lines().any(|l| l.starts_with("/mnt/nfs  nfs4  nas:/export") && l.ends_with("rw,vers=4.2")), "{text}");
    for leaked in ["hunter2", "bob", "smb.cred"] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
    assert_eq!(out.redactions.iter().map(|r| (r.kind, r.count)).collect::<Vec<_>>(), [("mount_option", 3)]);
}

#[test]
fn net_mounts_empty_and_errors() {
    // findmnt -t …: keine Treffer = leere Ausgabe, Exit 1
    let w = World::new("mounts-empty");
    w.fake(Program::Findmnt, &[arm("", "").exit(1)]);
    assert_eq!(w.collect(r#"{"probe":"net_mounts"}"#).status, Status::Empty);
    let w = World::new("mounts-list-empty");
    w.fake(Program::Findmnt, &[arm("", r#"{"filesystems":[]}"#)]);
    assert_eq!(w.collect(r#"{"probe":"net_mounts"}"#).status, Status::Empty);
    let w = World::new("mounts-bad");
    w.fake(Program::Findmnt, &[arm("", "garbage")]);
    assert!(matches!(w.collect(r#"{"probe":"net_mounts"}"#).status, Status::Failed(_)));
    let w = World::new("mounts-err");
    w.fake(Program::Findmnt, &[arm("", "").err("findmnt: failed to read mountinfo\n").exit(1)]);
    assert!(matches!(&w.collect(r#"{"probe":"net_mounts"}"#).status, Status::Failed(m) if m.contains("mountinfo")));
    // riesig: viele Mounts mit langen Optionen
    let w = World::new("mounts-giant");
    let mut fs = String::new();
    for i in 0..400 {
        if i > 0 {
            fs.push(',');
        }
        fs.push_str(&format!(r#"{{"target":"/mnt/m{i}","source":"//h/s{i}","fstype":"cifs","options":"rw,{},password=p{i}"}}"#, "opt=value,".repeat(10)));
    }
    w.fake(Program::Findmnt, &[arm("", format!(r#"{{"filesystems":[{fs}]}}"#))]);
    let out = w.collect(r#"{"probe":"net_mounts"}"#);
    let text = text_of(&out);
    assert!(!text.contains("password="), "{text}");
    assert!(out.redactions.iter().any(|r| r.kind == "mount_option" && r.count == 400), "{:?}", out.redactions);
}

// =================================================================================================================
// Resources

#[test]
fn mem_reads_meminfo_and_pressure() {
    let w = World::new("mem");
    w.write(
        "proc/meminfo",
        "MemTotal:       65536000 kB\nMemFree:         1000000 kB\nMemAvailable:   32768000 kB\nSwapTotal:       8000000 kB\nSwapFree:        4000000 kB\n",
    );
    w.write("proc/pressure/memory", "some avg10=1.50 avg60=0.40 avg300=0.10 total=123456\nfull avg10=0.50 avg60=0.10 avg300=0.00 total=1\n");
    w.write("proc/pressure/cpu", "some avg10=5.00 avg60=4.00 avg300=3.00 total=99\n");
    w.write("proc/pressure/io", "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    let out = w.collect(r#"{"probe":"mem"}"#);
    let text = text_of(&out);
    assert!(
        text.contains("MemTotal: 62.5 GiB") && text.contains("MemAvailable: 31.2 GiB") && text.contains("= 50%") && text.contains("swap in use: 3.8 GiB = 50%"),
        "{text}"
    );
    assert!(text.contains("memory pressure some avg10=1.50 avg60=0.40 avg300=0.10") && text.contains("cpu pressure some avg10=5.00"), "{text}");
    assert!(!text.contains("total="));
}

#[test]
fn mem_without_pressure_missing_and_giant() {
    let w = World::new("mem-nopsi");
    w.write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 500 kB\n");
    let out = w.collect(r#"{"probe":"mem"}"#);
    assert_eq!(out.status, Status::Ok);
    assert!(out.notes.iter().any(|n| n.contains("no pressure information")), "{:?}", out.notes);

    let w = World::new("mem-missing");
    assert!(matches!(&w.collect(r#"{"probe":"mem"}"#).status, Status::Failed(m) if m.contains("/proc/meminfo")));
    let w = World::new("mem-garbage");
    w.write("proc/meminfo", "garbage\n");
    assert!(matches!(w.collect(r#"{"probe":"mem"}"#).status, Status::Failed(_)));

    let w = World::new("mem-giant");
    w.write("proc/meminfo", format!("MemTotal: 1000 kB\n{}", "Junk: 1 kB\n".repeat(200_000)));
    assert_sane(&w.collect(r#"{"probe":"mem"}"#));
}

#[test]
fn load_reports_the_load_average_and_the_cpu_over_the_interval() {
    let w = World::new("load");
    w.write("proc/loadavg", "1.20 0.80 0.50 3/812 12345\n");
    w.write("proc/stat", "cpu  1000 0 500 8000 100 0 0 0 0 0\ncpu0 500 0 250 4000 50 0 0 0 0 0\ncpu1 500 0 250 4000 50 0 0 0 0 0\n");
    let out = w.collect(r#"{"probe":"load"}"#);
    let text = text_of(&out);
    assert!(text.contains("load average: 1.20 0.80 0.50 (1, 5, 15 min); running/total tasks 3/812; 2 CPU threads"), "{text}");
    // Die Fixture ändert sich nicht: keine verstrichene CPU-Zeit
    assert!(text.contains("no CPU time passed between the two samples"), "{text}");
}

#[test]
fn load_errors_and_cancel_during_the_sample() {
    let w = World::new("load-missing");
    assert!(matches!(w.collect(r#"{"probe":"load"}"#).status, Status::Failed(_)));
    let mut w = World::new("load-cancel");
    w.ctx.sample_interval = Duration::from_secs(5);
    w.write("proc/loadavg", "1 1 1 1/1 1\n");
    w.write("proc/stat", "cpu  1 1 1 1 1 1 1 1 1 1\n");
    let cancel = w.cancel.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let started = std::time::Instant::now();
    let out = w.collect(r#"{"probe":"load"}"#);
    t.join().unwrap();
    assert_eq!(out.status, Status::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
}

#[test]
fn disks_df_and_lsblk() {
    let w = World::new("disks");
    w.fake(
        Program::Df,
        &[arm("-h -l", "Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p4  217G   79G  128G  38% /\n")
            .err("df: /tmp/.mount_x: Transport endpoint is not connected\n")
            .exit(1)],
    );
    w.fake(Program::Lsblk, &[arm("-J", r#"{"blockdevices":[{"name":"nvme0n1","size":"931.5G","type":"disk","mountpoints":[null],"fstype":null,"children":[{"name":"nvme0n1p4","size":"217G","type":"part","mountpoints":["/"],"fstype":"ext4"}]}]}"#)]);
    let out = w.collect(r#"{"probe":"disks"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("## df\nFilesystem") && text.contains("## lsblk\nname size type fstype mountpoints"), "{text}");
    assert!(text.contains("  nvme0n1p4 217G part ext4 /"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("df exited with code 1") && n.contains("Transport endpoint is not connected")), "{:?}", out.notes);

    let w = World::new("disks-partial");
    w.fake(Program::Df, &[arm("", "Filesystem Size\n/ 1G\n")]);
    let out = w.collect(r#"{"probe":"disks"}"#);
    assert!(matches!(&out.status, Status::Partial(m) if m.contains("lsblk")), "{:?}", out.status);
    let w = World::new("disks-fail");
    assert!(matches!(w.collect(r#"{"probe":"disks"}"#).status, Status::Failed(_)));
}

fn proc_entry(w: &World, pid: u32, name: &str, rss_kib: u64, uid: u32, ticks: u64) {
    w.write(
        &format!("proc/{pid}/stat"),
        format!("{pid} ({name}) S 1 {pid} {pid} 0 -1 4194560 100 0 0 0 {} {} 0 0 20 0 5 0 12345 1000 200 18446744073709551615\n", ticks / 2, ticks - ticks / 2),
    );
    w.write(&format!("proc/{pid}/status"), format!("Name:\t{name}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\nVmRSS:\t{rss_kib} kB\n"));
    // Die Kommandozeile mit Geheimnis liegt da, wird aber nie gelesen
    w.write(&format!("proc/{pid}/cmdline"), format!("/usr/bin/{name}\0--password=hunter2\0"));
    w.write(&format!("proc/{pid}/environ"), "SECRET_TOKEN=abc\0");
}

#[test]
fn top_procs_lists_programs_by_memory_and_cpu_without_command_lines() {
    let w = World::new("top");
    proc_entry(&w, 100, "firefox", 8_000_000, 1000, 50_000);
    proc_entry(&w, 200, "dockerd", 400_000, 0, 120_000);
    proc_entry(&w, 300, "Web Content", 1_500_000, 1000, 9_000);
    w.write("proc/kthreadd_not_a_pid", "x");
    w.write("proc/meminfo", "MemTotal: 1 kB\n");
    // Ein Prozess, dessen Dateien fehlen (endete beim Lesen)
    fs::create_dir_all(w.root().join("proc/400")).unwrap();
    let out = w.collect(r#"{"probe":"top_procs"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    let mem_part = text.split("busiest by CPU").next().unwrap();
    let (a, b, c) = (mem_part.find("firefox").unwrap(), mem_part.find("Web Content").unwrap(), mem_part.find("dockerd").unwrap());
    assert!(a < b && b < c, "{text}");
    let cpu_part = text.split("busiest by CPU").nth(1).unwrap();
    assert!(cpu_part.find("dockerd").unwrap() < cpu_part.find("Web Content").unwrap(), "most CPU time first: {text}");
    assert!(text.contains("7.6 GiB"), "{text}");
    for leaked in ["hunter2", "SECRET_TOKEN", "/usr/bin/", "password"] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
    assert!(out.notes.iter().any(|n| n.contains("3 processes read; program names only")), "{:?}", out.notes);
}

#[test]
fn top_procs_without_processes_and_with_many() {
    let w = World::new("top-none");
    assert!(matches!(&w.collect(r#"{"probe":"top_procs"}"#).status, Status::Failed(m) if m.contains("no process")));
    let w = World::new("top-many");
    for pid in 1000..1800 {
        proc_entry(&w, pid, &format!("proc{pid}"), 1000 + u64::from(pid), 1000, u64::from(pid));
    }
    let out = w.collect(r#"{"probe":"top_procs"}"#);
    let text = text_of(&out);
    assert!(text.contains("proc1799"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("800 processes read")), "{:?}", out.notes);
}

#[test]
fn oom_reads_the_kernel_journal_and_treats_no_match_as_empty() {
    let w = World::new("oom");
    journalctl_with_boots(
        &w,
        vec![arm("-k --boot=-1 --grep", journal_line("2026-11-04T09:30:00", "kernel", "Out of memory: Killed process 4242 (java) total-vm:1234kB") + "\n")],
    );
    let out = w.collect(r#"{"probe":"oom","boot":-1}"#);
    assert!(text_of(&out).contains("Killed process 4242 (java)"), "{}", out.text());

    // journalctl --grep ohne Treffer: leer, Exit 1
    let w = World::new("oom-none");
    journalctl_with_boots(&w, vec![arm("--grep", "").exit(1)]);
    let out = w.collect(r#"{"probe":"oom","boot":0}"#);
    assert_eq!(out.status, Status::Empty, "{:?} {}", out.status, out.text());
    let w = World::new("oom-none2");
    journalctl_with_boots(&w, vec![arm("--grep", "-- No entries --\n").exit(1)]);
    assert_eq!(w.collect(r#"{"probe":"oom","boot":0}"#).status, Status::Empty);
}

// =================================================================================================================
// Heat

#[test]
fn sensors_render_values_and_thresholds() {
    let w = World::new("sensors");
    w.fake(
        Program::Sensors,
        &[arm(
            "-j",
            r#"{"coretemp-isa-0000":{"Adapter":"ISA adapter","Package id 0":{"temp1_input":78.0,"temp1_max":100.0,"temp1_crit":105.0},"Core 0":{"temp2_input":75.0}},"dell_smm-isa-00de":{"Adapter":"ISA adapter","Processor Fan":{"fan1_input":2110.0},"Broken":{}}}"#,
        )
        .err("ERROR: Can't get value of subfeature temp1_input: Can't read\n")],
    );
    let out = w.collect(r#"{"probe":"sensors"}"#);
    let text = text_of(&out);
    assert!(text.contains("Package id 0: 78 °C (max 100, crit 105)") && text.contains("Processor Fan: 2110 RPM"), "{text}");
    assert!(out.notes.iter().any(|n| n.contains("1 sensors could not be read")), "{:?}", out.notes);
    assert!(out.notes.iter().any(|n| n.contains("Can't get value of subfeature")), "{:?}", out.notes);
    let w = World::new("sensors-bad");
    w.fake(Program::Sensors, &[arm("", "No sensors found!\n").exit(1)]);
    assert!(matches!(w.collect(r#"{"probe":"sensors"}"#).status, Status::Failed(_)));
    let w = World::new("sensors-empty");
    w.fake(Program::Sensors, &[arm("", "{}")]);
    assert_eq!(w.collect(r#"{"probe":"sensors"}"#).status, Status::Empty);
}

const GPU_CSV: &str = "temperature.gpu, power.draw [W], power.limit [W], clocks.current.sm [MHz], clocks.current.memory [MHz], utilization.gpu [%], utilization.memory [%], memory.used [MiB], memory.total [MiB], pstate, clocks_event_reasons.active, clocks_event_reasons.hw_thermal_slowdown, clocks_event_reasons.sw_thermal_slowdown, clocks_event_reasons.hw_power_brake_slowdown, clocks_event_reasons.sw_power_cap\n53, 26.25 W, 170.00 W, 285 MHz, 5001 MHz, 39 %, 10 %, 1564 MiB, 12288 MiB, P3, 0x0000000000000005, Not Active, Not Active, Not Active, Active\n";

#[test]
fn gpu_decodes_the_throttle_reasons() {
    let w = World::new("gpu");
    w.fake(Program::NvidiaSmi, &[arm("--query-gpu=", GPU_CSV)]);
    let out = w.collect(r#"{"probe":"gpu"}"#);
    let text = text_of(&out);
    assert!(text.contains("GPU 0:\n  temperature.gpu: 53\n  power.draw [W]: 26.25 W"), "{text}");
    assert!(text.contains("throttle reasons: GPU idle, software power cap"), "{text}");
    assert!(text.contains("clocks_event_reasons.sw_power_cap: Active"), "{text}");
    assert!(!text.contains("hw_thermal_slowdown"), "inactive reasons are left out: {text}");

    // Mehrere GPUs
    let two = format!("{GPU_CSV}{}", GPU_CSV.lines().nth(1).unwrap());
    let w = World::new("gpu-two");
    w.fake(Program::NvidiaSmi, &[arm("", two)]);
    let text = text_of(&w.collect(r#"{"probe":"gpu"}"#));
    assert!(text.contains("GPU 0:") && text.contains("GPU 1:"), "{text}");
}

#[test]
fn gpu_driver_failure_is_reported_from_stdout() {
    let w = World::new("gpu-fail");
    w.fake(
        Program::NvidiaSmi,
        &[arm(
            "",
            "NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver. Make sure that the latest NVIDIA driver is installed and running.\n",
        )
        .exit(9)],
    );
    let out = w.collect(r#"{"probe":"gpu"}"#);
    assert!(matches!(&out.status, Status::Failed(m) if m.contains("NVIDIA-SMI has failed")), "{:?}", out.status);
    let w = World::new("gpu-missing");
    assert!(matches!(&w.collect(r#"{"probe":"gpu"}"#).status, Status::Unavailable(m) if m.contains("nvidia-smi is not installed")));
}

#[test]
fn nvme_temp_reads_sysfs_through_symlinked_class_entries() {
    let w = World::new("nvme");
    // /sys/class/nvme/nvme0 ist ein Symlink auf das Gerät, wie im echten sysfs
    w.write("sys/devices/pci0000:00/nvme/nvme0/device/hwmon1/temp1_input", "51850\n");
    w.write("sys/devices/pci0000:00/nvme/nvme0/device/hwmon1/temp2_input", "49000\n");
    w.write("sys/devices/pci0000:00/nvme/nvme0/device/hwmon1/temp1_label", "Composite\n");
    w.write("sys/devices/pci0000:00/nvme/nvme1/device/hwmon2/temp1_input", "bogus\n");
    fs::create_dir_all(w.root().join("sys/class/nvme")).unwrap();
    std::os::unix::fs::symlink(w.root().join("sys/devices/pci0000:00/nvme/nvme0"), w.root().join("sys/class/nvme/nvme0")).unwrap();
    std::os::unix::fs::symlink(w.root().join("sys/devices/pci0000:00/nvme/nvme1"), w.root().join("sys/class/nvme/nvme1")).unwrap();
    let out = w.collect(r#"{"probe":"nvme_temp"}"#);
    let text = text_of(&out);
    assert_eq!(out.status, Status::Ok, "{text}");
    assert!(text.contains("nvme0  temp1   51.9 °C") && text.contains("nvme0  temp2   49.0 °C"), "{text}");
    assert!(!text.contains("bogus") && !text.contains("Composite"), "{text}");

    let w = World::new("nvme-none");
    assert_eq!(w.collect(r#"{"probe":"nvme_temp"}"#).status, Status::Empty);
    fs::create_dir_all(w.root().join("sys/class/nvme")).unwrap();
    let out = w.collect(r#"{"probe":"nvme_temp"}"#);
    assert_eq!(out.status, Status::Empty);
}

// =================================================================================================================
// Querschnitt

/// Ein Fake für jedes Programm, das den Aufruf (Argument für Argument) in eine Datei schreibt und Unsinn antwortet.
fn recording_world(tag: &str) -> (World, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new(tag);
    let log = w.base().join("argv.log");
    for &p in Program::ALL {
        let path = w.base().join("bin").join(p.name());
        let script = format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"{}: $a\" >> '{}'; done\nprintf '%s' '[]'\nexit 0\n", p.name(), log.display());
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (w, log)
}

/// Die festen Elemente, die mit `-` beginnen dürfen: alle Literale der Vorlagen und die Anfänge der eingesetzten Optionen.
fn allowed_dash_elements() -> std::collections::BTreeSet<String> {
    use crate::diagnose::argv::{resolution, Arg, Exec};
    let mut set = std::collections::BTreeSet::new();
    let mut add = |c: &crate::diagnose::argv::Command| {
        for a in c.args {
            if let Arg::Lit(l) = a {
                if l.starts_with('-') {
                    set.insert(l.to_string());
                }
            }
        }
    };
    for spec in crate::diagnose::catalog::all() {
        if let Exec::Commands(cs) = spec.exec {
            cs.iter().for_each(&mut add);
        }
    }
    resolution::ALL.iter().for_each(|(_, c)| add(c));
    set
}

#[test]
fn no_argument_that_starts_with_a_dash_ever_comes_from_a_parameter() {
    use crate::diagnose::probe::Probe;
    use std::os::unix::fs::PermissionsExt;
    let (w, log) = recording_world("argv-dash");
    // Hostile Listen: Namen mit `-`, Zahlen, Pfade; nur `user.service` und `web` sind brauchbar
    let units = r#"[{"unit":"--user.service","load":"l","active":"a","sub":"s"},{"unit":"-.mount","load":"l","active":"a","sub":"s"},{"unit":"user.service","load":"l","active":"a","sub":"s"},{"unit":"1234","load":"l","active":"a","sub":"s"}]"#;
    let containers = format!("{{\"ID\":\"--all--all--all\",\"Names\":\"-v\"}}\n{{\"ID\":\"{CID_WEB}\",\"Names\":\"web\"}}\n");
    for (prog, list, marker) in [(Program::Systemctl, units.to_string(), "list-units"), (Program::Docker, containers, "ps -a")] {
        let listfile = w.base().join(format!("{}.list", prog.name()));
        fs::write(&listfile, list).unwrap();
        let path = w.base().join("bin").join(prog.name());
        let script = format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"{}: $a\" >> '{}'; done\ncase \"$*\" in *\"{marker}\"*) cat '{}';; *) printf '[]';; esac\nexit 0\n",
            prog.name(),
            log.display(),
            listfile.display()
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let probes: Vec<Probe> = [
        r#"{"probe":"unit_status","unit":"user"}"#,
        r#"{"probe":"unit_show","unit":"user.service"}"#,
        r#"{"probe":"docker_state","container":"web"}"#,
        r#"{"probe":"docker_logs","container":"web","since":"15m","lines":50}"#,
        r#"{"probe":"net_reach","target":"nas"}"#,
        r#"{"probe":"coredumps","since":"1h"}"#,
    ]
    .iter()
    .map(|j| crate::diagnose::probe::parse_validated(j).unwrap())
    .collect();
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    for p in &probes {
        collector.collect(p, super::collect::CollectOptions::new(Duration::from_secs(20)).confirmed());
    }
    let recorded = fs::read_to_string(&log).unwrap_or_default();
    assert!(recorded.lines().count() > 10, "{recorded}");
    let allowed = allowed_dash_elements();
    for line in recorded.lines() {
        let (_, arg) = line.split_once(": ").unwrap();
        if arg.starts_with('-') {
            let ok = allowed.contains(arg) || arg.starts_with("--boot=") || arg.starts_with("--since=");
            assert!(ok, "an argument that starts with a dash does not come from a template: {arg:?}\n{recorded}");
        }
        assert!(
            !arg.contains("--user.service") && arg != "-v" && arg != "1234" && arg != "-.mount",
            "a hostile name from a list reached an argv: {arg:?}\n{recorded}"
        );
    }
    // Eingesetzt wurde der aufgelöste Name bzw. die volle ID bzw. die Adresse aus der Config
    assert!(recorded.contains("systemctl: user.service"), "{recorded}");
    assert!(recorded.contains(&format!("docker: {CID_WEB}")), "{recorded}");
    assert!(recorded.contains("ping: nas.example.org"), "{recorded}");
}

#[test]
fn every_probe_survives_an_empty_world_and_stays_within_its_limit() {
    use crate::diagnose::test_support::full_probes;
    let w = World::new("empty-world");
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    for probe in full_probes() {
        let out = collector.collect(&probe, super::collect::CollectOptions::new(Duration::from_secs(10)).confirmed());
        assert_sane(&out);
        assert_eq!(out.probe, probe.name());
        assert!(!matches!(out.status, Status::Ok), "{}: nothing exists, yet the status is ok: {}", probe.name(), out.text());
        assert!(!out.text().is_empty(), "{}", probe.name());
    }
}

#[test]
fn every_command_probe_survives_garbage_output() {
    use crate::diagnose::test_support::full_probes;
    use std::os::unix::fs::PermissionsExt;
    let w = World::new("garbage");
    // Ein Fake für jedes Programm: 3 MB Unsinn mit Steuerzeichen, Escape-Folgen, NUL, ungültigem UTF-8 und riesigen Zeilen
    let mut junk: Vec<u8> = Vec::new();
    for i in 0..30_000u32 {
        junk.extend_from_slice(format!("\u{1b}[31mline {i}\u{1b}[0m \u{7}\0 \u{202e}x").as_bytes());
        junk.extend_from_slice(&[0xff, 0xfe, b'\r', b'\n']);
    }
    junk.extend(std::iter::repeat_n(b'z', 300_000));
    junk.push(b'\n');
    let file = w.base().join("junk");
    fs::write(&file, &junk).unwrap();
    for &p in Program::ALL {
        let path = w.base().join("bin").join(p.name());
        fs::write(&path, format!("#!/bin/sh\ncat '{}'\ncat '{}' >&2\nexit 0\n", file.display(), file.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    w.write("proc/uptime", "100.0 1.0\n");
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    let started = std::time::Instant::now();
    for probe in full_probes() {
        let out = collector.collect(&probe, super::collect::CollectOptions::new(Duration::from_secs(20)).confirmed());
        assert_sane(&out);
    }
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}

#[test]
fn deadlines_cancel_and_zero_budget() {
    // Ein hängendes Programm: die Frist der Probe beendet es, und die Probe meldet es
    let w = World::new("hang");
    w.fake(Program::Journalctl, &[arm("--list-boots", boots_json())]);
    use std::os::unix::fs::PermissionsExt;
    let hang = w.base().join("bin/journalctl");
    let original = fs::read_to_string(&hang).unwrap();
    fs::write(&hang, original.replace("case \"$args\" in\n", "case \"$args\" in\n  *\"--boot=-1\"*) exec sleep 30;;\n")).unwrap();
    fs::set_permissions(&hang, fs::Permissions::from_mode(0o755)).unwrap();
    let probe = crate::diagnose::probe::parse_validated(r#"{"probe":"boot_end","boot":-1,"lines":50}"#).unwrap();
    let mut collector = super::collect::Collector::new(&w.ctx, &w.cancel);
    let started = std::time::Instant::now();
    let out = collector.collect(&probe, super::collect::CollectOptions::new(Duration::from_millis(400)));
    assert_eq!(out.status, Status::TimedOut, "{:?}", out.status);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());

    // Kein Budget übrig: nichts startet
    let out = collector.collect(&probe, super::collect::CollectOptions::new(Duration::ZERO));
    assert_eq!(out.status, Status::TimedOut);
    assert!(out.parts.is_empty());
}

#[test]
fn rust_probes_read_only_what_the_catalog_declares() {
    // Jede Lese-Probe läuft gegen einen vollen Fixture-Baum; kein Zugriff außerhalb der Liste (der Zugriffsschutz meldete
    // „may not open“, und `assert_sane` prüft das), und die Zahl der geöffneten Pfade bleibt klein
    let w = World::new("declared");
    journalctl_with_boots(&w, vec![]);
    w.write("proc/version", "Linux version 7.2.14\n");
    w.write("proc/cmdline", "quiet\n");
    w.write("sys/class/dmi/id/bios_version", "F7\n");
    w.write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 500 kB\n");
    w.write("proc/loadavg", "1 1 1 1/1 1\n");
    w.write("proc/stat", "cpu  1 1 1 1 1 1 1 1 1 1\ncpu0 1 1 1 1 1 1 1 1 1 1\n");
    proc_entry(&w, 10, "init", 1000, 0, 5);
    w.write("var/log/pacman.log", pacman_fixture());
    w.write(&format!("var/lib/systemd/pstore/{}/001/dmesg.txt", crash_epoch()), DMESG);
    w.write("templog/temps-2026-11-04.csv", format!("{CSV_HEADER}{}", csv_rows("2026-11-04", 9 * 3600 + 46 * 60, 9 * 3600 + 47 * 60, 20)));
    w.write("sys/class/nvme/nvme0/device/hwmon1/temp1_input", "50000\n");
    // Dinge, die keine Probe anfassen darf
    w.write("etc/shadow", "root:$6$secret\n");
    w.write("home/u/.ssh/id_ed25519", "PRIVATE KEY");
    w.write("home/u/.local/share/xorg/Xorg.1.log", "XORG1-MARKER");
    let names = [
        r#"{"probe":"pstore","boot":-1}"#,
        r#"{"probe":"temps_tail","boot":-1,"window_min":5}"#,
        r#"{"probe":"kernel_info"}"#,
        r#"{"probe":"pacman_log","boot":-1,"days":1}"#,
        r#"{"probe":"mem"}"#,
        r#"{"probe":"load"}"#,
        r#"{"probe":"top_procs"}"#,
        r#"{"probe":"nvme_temp"}"#,
    ];
    for json in names {
        let out = w.collect(json);
        assert_sane(&out);
        let text = out.text();
        assert!(!text.contains("secret") && !text.contains("PRIVATE KEY") && !text.contains("XORG1-MARKER"), "{json}: {text}");
        assert!(out.status.has_data() || out.status == Status::Empty, "{json}: {:?} {text}", out.status);
    }
}

#[test]
fn timestamps_in_outputs_use_the_clock_of_the_context() {
    let w = World::new("clock");
    journalctl_with_boots(&w, vec![]);
    let out = w.collect(r#"{"probe":"boot_end","boot":0,"lines":50}"#);
    assert_eq!(out.resolved.len(), 1);
    let start = out.resolved[0].split(" → ").nth(1).unwrap().split(" … ").next().unwrap();
    assert_eq!(parse_iso(start), Some(at(2026, 11, 4, 10, 5, 30)));
}
