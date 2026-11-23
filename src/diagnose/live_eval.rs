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

//! Messschritt der Aufgabe 10 (Baustein 4): acht **synthetische** Fälle nach echten Mustern, je ein Bericht über den Weg
//! des Produkts (Proben gegen Fakes und Fixtures → Filter → [`Session`] → `outgoing()`) und eine Runde mit Claude über
//! [`ask_diagnosis`] mit dem echten Prompt und Schema. Ergebnis und Bewertung: `diagnose-ai.md`.
//!
//! - `cases_build_their_reports_through_the_product_path` läuft mit der Unit-Suite (ohne Netz): jeder Fall liefert einen
//!   Bericht, der die Belege enthält, auf die das Bewertungskriterium baut. So veralten die Fixtures nicht still.
//! - `live_diagnosis_against_claude` ist `#[ignore]` und läuft **nie** in `scripts/check.sh`; er kostet echte Aufrufe und
//!   startet nur mit `DICTATE_LIVE_DIAG=1` (sonst bricht er ab, auch mit `--ignored`):
//!
//!   ```text
//!   DICTATE_LIVE_DIAG=1 [DICTATE_DIAG_MODELS=claude-sonnet-5-5,claude-haiku-5-5] [DICTATE_DIAG_CASES=1,4] \
//!     cargo test --release --lib diagnose::live_eval::live_diagnosis_against_claude -- --ignored --exact --nocapture
//!   ```
//!
//!   `DICTATE_LIVE_DIAG=dry` schreibt nur die Berichte (kein Aufruf). Rohantworten mit Bericht, Belegauszügen, Tokens und
//!   Dauer landen unter `DICTATE_DIAG_OUT` (Vorgabe `$DICTATE_TEST_ROOT/logs/diagnose-ai/<Zeit>/`, Testordner aus
//!   `tests/test_root/mod.rs`, auf blub `/home/data/stt-test`), nie im Repo. Der
//!   Schlüssel wird nur über `ai.api_key_file` gelesen (`~/.config/anthropic/api-key`).
//!
//! Alle Daten sind erfunden (Rechner `blub` und Benutzer `alice` wie in den übrigen Fixtures); keine Zeile stammt aus einem
//! echten Log. Jede Probe eines Profils hat ein Fake oder eine Datei, damit der Bericht wie auf einem echten Rechner
//! aussieht (keine „not installed“-Abschnitte).

// Testordner (DICTATE_TEST_ROOT): dieselbe Regel wie die Integrationstests und scripts/test-root.sh
#[path = "../../tests/test_root/mod.rs"]
mod test_root;

use super::ai::{ask_diagnosis, DiagRequest, SectionLines};
use super::argv::Program;
use super::probe::UnitRef;
use super::probes::testkit::{arm, at, boots_json, journal_line, World};
use super::profile::{Plan, Profile, ProfileArgs};
use super::pseudo::Identity;
use super::report::{CollectLock, CollectParams, Report, ReportBudget, Session};
use crate::config::{AiCfg, DiagnoseCfg, PingTargetEntry};
use crate::pipeline::ai::{thinking_for, tool_caps_for};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

/// Ein Fall: Profil wie im Panel, Fixtures, Bewertungskriterium (vorher festgelegt, steht so in `diagnose-ai.md`).
struct Case {
    id: u32,
    name: &'static str,
    profile: Profile,
    args: fn() -> ProfileArgs,
    setup: fn(&World),
    /// Was im gesendeten Bericht stehen muss, damit der Fall lösbar ist
    report_must: &'static [&'static str],
    /// Kriterium „getroffen“ (ja), als Text
    hit: &'static str,
    /// Kriterium „teilweise“, als Text
    partial: &'static str,
    /// Hilfe für die Bewertung: je Gruppe muss eines der Wörter in den Ursachen stehen (alle Gruppen = Vorschlag „ja“,
    /// eine = „teilweise“). Die Bewertung in `diagnose-ai.md` ist von Hand, nach `hit`/`partial`.
    keys: &'static [&'static [&'static str]],
}

fn no_args() -> ProfileArgs {
    ProfileArgs::default()
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: 1,
            name: "oops-nvidia-isr",
            profile: Profile::LastCrash,
            args: no_args,
            setup: case_oops,
            report_must: &["nvidia_isr", "Kernel panic - not syncing: Fatal exception in interrupt", "nvidia-open-dkms (615.77.01-1 -> 615.78.08-1)"],
            hit: "Kernel-Panik durch einen Oops (GPF) im Interrupt-Handler des NVIDIA-Treibers (nvidia_isr) als Ursache des Absturzes",
            partial: "Kernel-Panik/Oops erkannt, aber ohne den NVIDIA-Treiber, oder nur allgemein „GPU/Treiber“ ohne den Oops",
            keys: &[&["nvidia_isr", "nvidia"], &["panic", "oops", "general protection"]],
        },
        Case {
            id: 2,
            name: "silent-hang",
            profile: Profile::LastCrash,
            args: no_args,
            setup: case_silent_hang,
            report_must: &["boot -1", "09:46:40"],
            hit: "Harter Hänger oder Stromverlust ohne jede Spur im Kernel (Journal und temp-logger enden zugleich, pstore leer, \
                  Temperaturen normal); keine erfundene konkrete Ursache mit hoher Sicherheit",
            partial: "Unsauberes Ende erkannt, aber eine nicht belegte Ursache (Hitze, OOM, Treiber) als wahrscheinlich genannt",
            keys: &[&["hang", "freeze", "lockup", "froze", "power", "hard reset"], &["no ", "without", "nothing", "empty", "silent"]],
        },
        Case {
            id: 3,
            name: "oom-kill",
            profile: Profile::Now,
            args: no_args,
            setup: case_oom,
            report_must: &["Out of memory: Killed process", "(java)"],
            hit: "Der OOM-Killer hat den Prozess java beendet, weil RAM und Swap erschöpft waren",
            partial: "Speichermangel genannt, aber ohne den getöteten Prozess, oder OOM-Kill ohne den Speichermangel",
            keys: &[&["oom", "out of memory", "out-of-memory"], &["java"]],
        },
        Case {
            id: 4,
            name: "failed-unit-exit-code",
            profile: Profile::Unit,
            args: || ProfileArgs { unit: Some(UnitRef::new("backup").unwrap()), container: None },
            setup: case_failed_unit,
            // Aufgabe 10c: `pgdata` bleibt im Pfad, Zeitstempel und Pfad der Unit werden nicht geschwärzt
            report_must: &[
                "status=23",
                "Permission denied (13)",
                "(code 23)",
                "/home/user-1/projects/path-1/pgdata/",
                "ExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET",
                "FragmentPath=/etc/systemd/system/backup.service",
            ],
            hit: "backup.service scheitert, weil rsync mit Code 23 (Teilübertragung) endet, und zwar wegen Permission denied auf \
                  Dateien unter dem Home (die persönlichen Teile der Pfade sind im Bericht Platzhalter `path-N`)",
            partial: "Nur eines von beiden: Exit-Code 23/rsync oder die verweigerten Dateien",
            keys: &[&["23", "partial transfer", "not transferred"], &["permission"]],
        },
        Case {
            id: 5,
            name: "container-segfault-strlen",
            profile: Profile::Docker,
            args: no_args,
            setup: case_segfault,
            // Aufgabe 10c: der Kopf mit nur einem Parameter bleibt lesbar, der wiederholte Block ergibt eine Sammelzeile
            report_must: &[
                "strlen",
                "storage_open",
                "unknown key 'storage.path'",
                "Restarting (139)",
                " · docker_state · container=dataroom-worker-1 · ",
                "messages above omitted",
            ],
            hit: "Der Worker stürzt in einer Neustartschleife mit SIGSEGV in strlen (NULL-Zeiger) aus storage_open ab, weil die \
                  Config den Schlüssel storage.path nicht mehr liefert (von 2.4.0 als unbekannt ignoriert)",
            partial: "Absturzschleife mit Segfault (strlen/storage_open) erkannt, aber ohne den Zusammenhang mit der Config",
            keys: &[&["strlen", "segfault", "sigsegv", "segmentation", "139"], &["storage.path", "config", "unknown key"]],
        },
        Case {
            id: 6,
            name: "cifs-hang",
            profile: Profile::Now,
            args: no_args,
            setup: case_cifs,
            report_must: &["has not responded in 180 seconds", "blocked for more than 122 seconds", "[cifs]"],
            hit: "Der CIFS-Server (nas) antwortet nicht; Prozesse hängen im D-Zustand im CIFS-Code, daher die hohe Last ohne \
                  CPU-Verbrauch",
            partial: "CIFS-Ausfall oder hängende Prozesse genannt, aber ohne den Zusammenhang",
            keys: &[&["cifs", "smb", "nas"], &["blocked", "hung", "hang", "d state", "d-state", "uninterruptible"]],
        },
        Case {
            id: 7,
            name: "dns-catch-all-wg0",
            profile: Profile::Network,
            args: no_args,
            setup: case_dns,
            report_must: &["DNS Domain: ~.", "wg0"],
            hit: "Die Routing-Domain ~. auf wg0 schickt alle DNS-Anfragen an 10.8.0.1, wg0 ist aber unten und ohne Route; daher \
                  löst nichts auf",
            partial: "wg0/VPN-DNS als Verdacht genannt, aber ohne ~. oder ohne dass wg0 unten ist",
            keys: &[&["wg0", "10.8.0.1"], &["~.", "routing domain", "catch-all", "all queries", "all dns"]],
        },
        Case {
            id: 8,
            name: "disk-full",
            profile: Profile::Now,
            args: no_args,
            setup: case_disk_full,
            report_must: &["100% /", "error 28"],
            hit: "Das Root-Dateisystem ist voll (100 %, 0 frei); daraus folgen die Schreibfehler und die gescheiterten Units",
            partial: "Volle Platte genannt, aber die Fehler einer anderen Ursache zugeschrieben, oder nur die Folgen",
            keys: &[&["full", "no space", "100%", "100 %", "enospc", "error 28"], &["/", "root"]],
        },
    ]
}

// ---- gemeinsame Fixtures ------------------------------------------------------------------------------------------------

fn identity() -> Identity {
    Identity { user: "alice".into(), home: "/home/alice".into(), hostname: "blub".into(), ..Identity::default() }
}

const CSV_HEADER: &str = "zeit,cpu_pkg,cpu_core_max,cpu_dell,nvme,gpu,gpu_util,gpu_power_w,fan_cpu_rpm,fan_case_rpm,load1,mem_avail_mb\n";

/// Zeilen des temp-loggers alle `step` s von `from_s` bis `to_s` (Sekunden des Tages), normale Werte.
fn csv_rows(day: &str, from_s: i64, to_s: i64, step: i64) -> String {
    let mut s = String::new();
    let mut t = from_s;
    while t <= to_s {
        s.push_str(&format!(
            "{day} {:02}:{:02}:{:02},{},{},47,41,{},{},{},1115,591,{:.2},{}\n",
            t / 3600,
            t % 3600 / 60,
            t % 60,
            52 + (t % 7),
            55 + (t % 5),
            46 + (t % 3),
            8 + (t % 9),
            18 + (t % 4),
            0.4 + (t % 6) as f64 / 10.0,
            41000 + (t % 50) * 10
        ));
        t += step;
    }
    s
}

const SENSORS_JSON: &str = r#"{"coretemp-isa-0000":{"Adapter":"ISA adapter","Package id 0":{"temp1_input":49.0,"temp1_max":100.0,"temp1_crit":105.0},"Core 0":{"temp2_input":47.0},"Core 1":{"temp3_input":48.0}},"dell_smm-isa-00de":{"Adapter":"ISA adapter","Processor Fan":{"fan1_input":1080.0},"Case Fan":{"fan2_input":590.0}},"nvme-pci-0100":{"Adapter":"PCI adapter","Composite":{"temp1_input":41.85}}}"#;

const GPU_CSV: &str = "temperature.gpu, power.draw [W], power.limit [W], clocks.current.sm [MHz], clocks.current.memory [MHz], utilization.gpu [%], utilization.memory [%], memory.used [MiB], memory.total [MiB], pstate, clocks_event_reasons.active, clocks_event_reasons.hw_thermal_slowdown, clocks_event_reasons.sw_thermal_slowdown, clocks_event_reasons.hw_power_brake_slowdown, clocks_event_reasons.sw_power_cap\n44, 18.40 W, 170.00 W, 210 MHz, 405 MHz, 3 %, 2 %, 1210 MiB, 12288 MiB, P8, 0x0000000000000001, Not Active, Not Active, Not Active, Not Active\n";

const LSBLK_JSON: &str = r#"{"blockdevices":[{"name":"nvme0n1","size":"931.5G","type":"disk","mountpoints":[null],"fstype":null,"children":[{"name":"nvme0n1p1","size":"512M","type":"part","mountpoints":["/boot"],"fstype":"vfat"},{"name":"nvme0n1p4","size":"217G","type":"part","mountpoints":["/"],"fstype":"ext4"}]},{"name":"sda","size":"3.6T","type":"disk","mountpoints":[null],"fstype":null,"children":[{"name":"sda1","size":"3.6T","type":"part","mountpoints":["/home/data"],"fstype":"ext4"}]}]}"#;

const DF_OK: &str = "Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p4  217G   96G  110G  47% /\n/dev/nvme0n1p1  511M  148M  364M  29% /boot\n/dev/sda1       3.6T  1.9T  1.6T  55% /home/data\n";

const MEMINFO_OK: &str =
    "MemTotal:       65536000 kB\nMemFree:        30120000 kB\nMemAvailable:   48770000 kB\nSwapTotal:       8388604 kB\nSwapFree:        8388604 kB\n";

fn psi(w: &World, mem_some10: &str, io_full10: &str) {
    w.write("proc/pressure/memory", format!("some avg10={mem_some10} avg60=0.10 avg300=0.05 total=123456\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=12\n"));
    w.write("proc/pressure/cpu", "some avg10=0.80 avg60=0.70 avg300=0.60 total=9912\n");
    w.write(
        "proc/pressure/io",
        format!("some avg10={io_full10} avg60=0.30 avg300=0.20 total=4401\nfull avg10={io_full10} avg60=0.20 avg300=0.10 total=3301\n"),
    );
}

/// Heat- und Ressourcenproben des Profils „Now“ (Last, Sensoren, GPU) mit ruhigen Werten.
fn calm_machine(w: &World, loadavg: &str) {
    w.write("proc/loadavg", loadavg);
    w.write("proc/stat", "cpu  120000 300 40000 2400000 9000 0 1200 0 0 0\ncpu0 15000 40 5000 300000 1100 0 150 0 0 0\n");
    w.fake(Program::Sensors, &[arm("-j", SENSORS_JSON)]);
    w.fake(Program::NvidiaSmi, &[arm("--query-gpu=", GPU_CSV)]);
    w.fake(Program::Lsblk, &[arm("-J", LSBLK_JSON)]);
}

fn no_failed_units(w: &World) {
    w.fake(Program::Systemctl, &[arm("--user --failed", "[]"), arm("--failed", "[]")]);
}

/// Alltägliche Zeilen eines Desktops (für das Ende eines Boots), alle `step` s von `from` (hh, mm, ss) an, `n` Zeilen.
fn desktop_lines(day: &str, from: (i64, i64, i64), n: i64, step: i64) -> Vec<String> {
    let idents = [
        ("dockerd[812]", "container 665982ad0616 healthy"),
        ("picom[1688]", "paint: skipped frame, window list changed"),
        ("kernel", "usb 3-2: reset SuperSpeed USB device number 3 using xhci_hcd"),
        ("systemd[1302]", "Started Firefox web browser (session)."),
        ("NetworkManager[640]", "<info>  [dhcp4] (enp5s0): state changed new lease"),
        ("pipewire[1544]", "spa.alsa: front:0p: (12 suppressed) xrun"),
        ("dictate[1710]", "daemon: idle"),
        ("tailscaled[901]", "netcheck: report: udp=true v6=false"),
    ];
    let start = from.0 * 3600 + from.1 * 60 + from.2;
    (0..n)
        .map(|i| {
            let t = start + i * step;
            let (ident, msg) = idents[(i as usize * 5 + 3) % idents.len()];
            journal_line(&format!("{day}T{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60), ident, msg)
        })
        .collect()
}

fn lines(v: &[String]) -> String {
    let mut s = v.join("\n");
    s.push('\n');
    s
}

/// Das Journal von Boot −1 endet ohne Shutdown um 09:46:55 (wie `boots_json`).
fn crashed_boot_tail() -> String {
    let mut v = desktop_lines("2026-11-04", (9, 40, 0), 40, 10);
    v.push(journal_line("2026-11-04T09:46:55", "dockerd[812]", "container 665982ad0616 healthy"));
    lines(&v)
}

fn noise_kernel_warnings(day: &str) -> Vec<String> {
    vec![
        journal_line(&format!("{day}T08:12:02"), "kernel", "x86/cpu: SGX disabled or unsupported by BIOS."),
        journal_line(
            &format!("{day}T08:12:03"),
            "kernel",
            "ACPI BIOS Error (bug): Could not resolve symbol [\\_SB.PC00.PEG1.PEGP], AE_NOT_FOUND (20250404/dswload2-162)",
        ),
        journal_line(&format!("{day}T08:12:04"), "kernel", "nvidia: loading out-of-tree module taints kernel."),
        journal_line(&format!("{day}T08:12:04"), "kernel", "nvidia: module license taints kernel."),
        journal_line(&format!("{day}T08:12:05"), "kernel", "NVRM: loading NVIDIA UNIX Open Kernel Module for x86_64  615.78.08  Release Build"),
        journal_line(&format!("{day}T08:12:09"), "kernel", "usb 1-4: device descriptor read/64, error -71"),
    ]
}

/// Dateien und Programme, die das Profil „Last crash“ außer Journal und pstore liest.
fn last_crash_rest(w: &World, pacman_log: &str) {
    w.write("var/log/pacman.log", pacman_log);
    w.write("proc/version", "Linux version 7.2.14-arch1-1 (linux@archlinux) (gcc (GCC) 16.2.1 20261020, GNU ld (GNU Binutils) 2.46) #1 SMP PREEMPT_DYNAMIC\n");
    w.write("proc/cmdline", "BOOT_IMAGE=/vmlinuz-linux root=UUID=6f1c2a9e-0b7d-4d1e-9a51-3c2b7e8f4d10 rw nvidia-drm.modeset=1 loglevel=3 quiet\n");
    w.write("sys/class/dmi/id/bios_version", "1.21.0\n");
    w.fake(
        Program::Pacman,
        &[arm(
            "-Q linux",
            "linux 7.2.14.arch1-1\nnvidia-open-dkms 615.78.08-1\nnvidia-utils 615.78.08-1\nmesa 1:26.2.7-1\ndocker 1:29.9.2-1\nsystemd 262.1-1\n",
        )],
    );
}

const PACMAN_NVIDIA_UPDATE: &str = "[2026-10-28T19:02:11+0100] [ALPM] upgraded firefox (144.0-1 -> 144.0.2-1)\n\
[2026-11-03T22:15:00+0100] [PACMAN] Running 'pacman -Syu'\n\
[2026-11-03T22:15:01+0100] [ALPM] transaction started\n\
[2026-11-03T22:15:02+0100] [ALPM] upgraded linux (7.2.13.arch1-1 -> 7.2.14.arch1-1)\n\
[2026-11-03T22:15:03+0100] [ALPM] upgraded nvidia-open-dkms (615.77.01-1 -> 615.78.08-1)\n\
[2026-11-03T22:15:04+0100] [ALPM] upgraded nvidia-utils (615.77.01-1 -> 615.78.08-1)\n\
[2026-11-03T22:15:05+0100] [ALPM-SCRIPTLET] ==> dkms install --no-depmod nvidia/615.78.08 -k 7.2.14-arch1-1\n\
[2026-11-03T22:15:41+0100] [ALPM] transaction completed\n";

const PACMAN_QUIET: &str = "[2026-10-28T19:02:11+0100] [ALPM] upgraded firefox (144.0-1 -> 144.0.2-1)\n\
[2026-10-28T19:02:12+0100] [ALPM] upgraded python-requests (2.33.0-1 -> 2.33.1-1)\n";

// ---- Fall 1: Oops in nvidia_isr ---------------------------------------------------------------------------------------

fn case_oops(w: &World) {
    let mut kernel = noise_kernel_warnings("2026-11-04");
    kernel.push(journal_line("2026-11-04T09:12:40", "kernel", "NVRM: GPU at PCI:0000:01:00: GPU-6c1f0a4e-2b9d-4c71-8e3a-5d0b7f9a1c22"));
    w.fake(Program::Journalctl, &[arm("-k --boot=-1", lines(&kernel)), arm("--boot=-1 -n 200", crashed_boot_tail()), arm("--list-boots", boots_json())]);
    let dmesg = "<4>[ 5755.083100] general protection fault, probably for non-canonical address 0xdead000000000122: 0000 [#1] PREEMPT SMP NOPTI\n\
<4>[ 5755.083110] CPU: 5 UID: 0 PID: 0 Comm: swapper/5 Tainted: P           OE      7.2.14-arch1-1 #1\n\
<4>[ 5755.083112] Tainted: [P]=PROPRIETARY_MODULE, [O]=OOT_MODULE, [E]=UNSIGNED_MODULE\n\
<4>[ 5755.083115] Hardware name: Dell Inc. XPS 8960/0KVM4R, BIOS 1.21.0 08/12/2026\n\
<4>[ 5755.083120] RIP: 0010:nvidia_isr+0x5c/0x1a0 [nvidia]\n\
<4>[ 5755.083122] Code: 48 8b 47 08 48 85 c0 74 3a 48 8b 50 10 <48> 8b 02 a8 01 75 2c\n\
<4>[ 5755.083130] RSP: 0018:ffffb3a4c0d48f30 EFLAGS: 00010086\n\
<4>[ 5755.083200] Call Trace:\n\
<4>[ 5755.083201]  <IRQ>\n\
<4>[ 5755.083203]  ? die_addr+0x37/0x90\n\
<4>[ 5755.083204]  ? exc_general_protection+0x1d8/0x430\n\
<4>[ 5755.083205]  __handle_irq_event_percpu+0x4a/0x180\n\
<4>[ 5755.083207]  handle_irq_event+0x39/0x80\n\
<4>[ 5755.083209]  handle_edge_irq+0x8c/0x230\n\
<4>[ 5755.083211]  __common_interrupt+0x4a/0xf0\n\
<4>[ 5755.083213]  common_interrupt+0x7f/0xa0\n\
<4>[ 5755.083215]  </IRQ>\n\
<4>[ 5755.083216]  <TASK>\n\
<4>[ 5755.083217]  asm_common_interrupt+0x26/0x40\n\
<4>[ 5755.083218]  cpuidle_enter_state+0xc1/0x430\n\
<4>[ 5755.083219]  </TASK>\n\
<4>[ 5755.083220] Modules linked in: nvidia_drm(POE) nvidia_modeset(POE) nvidia_uvm(POE) nvidia(POE) snd_hda_intel uvcvideo cifs xt_conntrack\n\
<4>[ 5755.083230] ---[ end trace 0000000000000000 ]---\n\
<4>[ 5755.083231] RIP: 0010:nvidia_isr+0x5c/0x1a0 [nvidia]\n\
<0>[ 5755.083233] Kernel panic - not syncing: Fatal exception in interrupt\n\
<0>[ 5755.083240] Kernel Offset: 0x2a000000 from 0xffffffff81000000 (relocation range: 0xffffffff80000000-0xffffffffbfffffff)\n";
    w.write(&format!("var/lib/systemd/pstore/{}/001/dmesg.txt", at(2026, 11, 4, 9, 47, 57)), dmesg);
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-04", 9 * 3600 + 30 * 60, 9 * 3600 + 47 * 60 + 50, 20));
    w.write("templog/temps-2026-11-04.csv", csv);
    last_crash_rest(w, PACMAN_NVIDIA_UPDATE);
}

// ---- Fall 2: stiller Hänger ---------------------------------------------------------------------------------------------

fn case_silent_hang(w: &World) {
    let kernel = noise_kernel_warnings("2026-11-04");
    w.fake(Program::Journalctl, &[arm("-k --boot=-1", lines(&kernel)), arm("--boot=-1 -n 200", crashed_boot_tail()), arm("--list-boots", boots_json())]);
    // pstore: Ordner da, aber leer (World legt ihn an). Der temp-logger schreibt bis 09:46:50 und dann nichts mehr
    // (letzte Zeile 09:46:40 bei 20 s Takt; bis W7 nannte auch der Xorg-Log 09:46:50)
    let mut csv = String::from(CSV_HEADER);
    csv.push_str(&csv_rows("2026-11-04", 9 * 3600 + 30 * 60, 9 * 3600 + 46 * 60 + 50, 20));
    w.write("templog/temps-2026-11-04.csv", csv);
    last_crash_rest(w, PACMAN_QUIET);
}

// ---- Fall 3: OOM-Kill ---------------------------------------------------------------------------------------------------

fn case_oom(w: &World) {
    let d = "2026-11-05";
    let mut k = noise_kernel_warnings(d);
    for (t, m) in [
        ("11:41:02", "java invoked oom-killer: gfp_mask=0x140cca(GFP_HIGHUSER_MOVABLE|__GFP_COMP), order=0, oom_score_adj=0"),
        ("11:41:02", "CPU: 3 UID: 1000 PID: 48230 Comm: java Tainted: P           OE      7.2.14-arch1-1 #1"),
        ("11:41:02", "Mem-Info:"),
        ("11:41:02", "active_anon:15690112 inactive_anon:204800 isolated_anon:0 active_file:1201 inactive_file:988 unevictable:0"),
        ("11:41:02", "Free swap  = 0kB"),
        ("11:41:02", "Total swap = 8388604kB"),
        ("11:41:02", "oom-kill:constraint=CONSTRAINT_NONE,nodemask=(null),cpuset=user.slice,mems_allowed=0,global_oom,task_memcg=/user.slice/user-1000.slice/user@1000.service/app.slice/app-idea.scope,task=java,pid=48211,uid=1000"),
        ("11:41:02", "Out of memory: Killed process 48211 (java) total-vm:38912044kB, anon-rss:30123456kB, file-rss:1024kB, shmem-rss:0kB, UID:1000 pgtables:61200kB oom_score_adj:0"),
        ("11:41:03", "oom_reaper: reaped process 48211 (java), now anon-rss:0kB, file-rss:0kB, shmem-rss:0kB"),
    ] {
        k.push(journal_line(&format!("{d}T{t}"), "kernel", m));
    }
    w.fake(Program::Journalctl, &[arm("-k --boot=0", lines(&k)), arm("--list-boots", boots_json())]);
    no_failed_units(w);
    w.write(
        "proc/meminfo",
        "MemTotal:       65536000 kB\nMemFree:        24120000 kB\nMemAvailable:   33770000 kB\nSwapTotal:       8388604 kB\nSwapFree:         122880 kB\n",
    );
    psi(w, "12.40", "3.10");
    calm_machine(w, "2.10 4.80 3.95 2/1188 50211\n");
    w.fake(Program::Df, &[arm("-h -l", DF_OK)]);
}

// ---- Fall 4: Unit mit Exit-Code -------------------------------------------------------------------------------------------

const UNITS_SYSTEM: &str = r#"[{"unit":"backup.service","load":"loaded","active":"failed","sub":"failed","description":"Nightly rsync backup to NAS"},
{"unit":"backup.timer","load":"loaded","active":"active","sub":"waiting","description":"Nightly backup"},
{"unit":"docker.service","load":"loaded","active":"active","sub":"running","description":"Docker Application Container Engine"},
{"unit":"sshd.service","load":"loaded","active":"active","sub":"running","description":"OpenSSH Daemon"}]"#;
const UNITS_USER: &str = r#"[{"unit":"pipewire.service","load":"loaded","active":"active","sub":"running","description":"PipeWire"}]"#;

fn case_failed_unit(w: &World) {
    let status = "× backup.service - Nightly rsync backup to NAS\n     Loaded: loaded (/etc/systemd/system/backup.service; static)\n     Active: failed (Result: exit-code) since Thu 2026-11-05 03:00:41 CET; 8h ago\nTriggeredBy: ● backup.timer\n    Process: 51234 ExecStart=/usr/local/bin/backup.sh (code=exited, status=23)\n   Main PID: 51234 (code=exited, status=23)\n        CPU: 12.402s\n";
    let show = "Id=backup.service\nDescription=Nightly rsync backup to NAS\nLoadState=loaded\nActiveState=failed\nSubState=failed\nResult=exit-code\nExecMainCode=1\nExecMainStatus=23\nNRestarts=0\nActiveEnterTimestamp=Thu 2026-11-05 03:00:00 CET\nActiveExitTimestamp=Thu 2026-11-05 03:00:41 CET\nInactiveEnterTimestamp=Thu 2026-11-05 03:00:41 CET\nStateChangeTimestamp=Thu 2026-11-05 03:00:41 CET\nExecMainStartTimestamp=Thu 2026-11-05 03:00:00 CET\nExecMainExitTimestamp=Thu 2026-11-05 03:00:41 CET\nFragmentPath=/etc/systemd/system/backup.service\nMemoryCurrent=\n";
    w.fake(
        Program::Systemctl,
        &[
            arm("--user list-units", UNITS_USER),
            arm("list-units", UNITS_SYSTEM),
            arm("status --no-pager -n 0 -- backup.service", status).exit(3),
            arm("show --no-pager -p", show),
        ],
    );
    let d = "2026-11-05";
    let mut j = vec![
        journal_line(&format!("{d}T03:00:00"), "systemd[1]", "Starting Nightly rsync backup to NAS..."),
        journal_line(&format!("{d}T03:00:00"), "backup.sh[51234]", "backup: syncing /home/alice to /mnt/nas/backup/blub"),
    ];
    for (i, f) in ["base/16384/2619", "base/16384/2840", "global/pg_control", "pg_wal/000000010000000000000042", "postmaster.pid"].iter().enumerate() {
        j.push(journal_line(
            &format!("{d}T03:00:{:02}", 30 + i),
            "backup.sh[51234]",
            &format!("rsync: [sender] send_files failed to open \"/home/alice/projects/dataroom/pgdata/{f}\": Permission denied (13)"),
        ));
    }
    j.push(journal_line(
        &format!("{d}T03:00:36"),
        "backup.sh[51234]",
        "rsync: [sender] opendir \"/home/alice/projects/dataroom/pgdata/pg_stat_tmp\" failed: Permission denied (13)",
    ));
    j.push(journal_line(&format!("{d}T03:00:41"), "backup.sh[51234]", "sent 1.92G bytes  received 48.21K bytes  47.03M bytes/sec"));
    j.push(journal_line(
        &format!("{d}T03:00:41"),
        "backup.sh[51234]",
        "rsync error: some files/attrs were not transferred (see previous errors) (code 23) at main.c(1338) [sender=3.4.1]",
    ));
    j.push(journal_line(&format!("{d}T03:00:41"), "systemd[1]", "backup.service: Main process exited, code=exited, status=23/n/a"));
    j.push(journal_line(&format!("{d}T03:00:41"), "systemd[1]", "backup.service: Failed with result 'exit-code'."));
    j.push(journal_line(&format!("{d}T03:00:41"), "systemd[1]", "Failed to start Nightly rsync backup to NAS."));
    w.fake(Program::Journalctl, &[arm("-u backup.service", lines(&j)), arm("--list-boots", boots_json())]);
}

// ---- Fall 5: Container mit Segfault-Serie in strlen -----------------------------------------------------------------------

const CID_WEB: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90112233445566778899aabbccddeeff00";
const CID_WORKER: &str = "5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d";
const CID_DB: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0ffeeddccbbaa99887766554433221100";

fn case_segfault(w: &World) {
    let web = format!(
        r#"{{"Command":"\"nginx -g 'daemon off;'\"","CreatedAt":"2026-11-02 10:07:06 +0100 CET","ID":"{CID_WEB}","Image":"nginx:1.27","Labels":"com.docker.compose.project=dataroom","Names":"dataroom-web-1","Networks":"dataroom_default","Ports":"0.0.0.0:8080->80/tcp","RunningFor":"3 days ago","State":"running","Status":"Up 3 days (healthy)"}}"#
    );
    let worker = format!(
        r#"{{"Command":"\"/usr/local/bin/worker\"","CreatedAt":"2026-11-05 09:58:12 +0100 CET","ID":"{CID_WORKER}","Image":"dataroom/worker:2.4.0","Labels":"com.docker.compose.project=dataroom","Names":"dataroom-worker-1","Networks":"dataroom_default","Ports":"","RunningFor":"About an hour ago","State":"restarting","Status":"Restarting (139) 12 seconds ago"}}"#
    );
    let db = format!(
        r#"{{"Command":"\"docker-entrypoint.sh postgres\"","CreatedAt":"2026-11-02 10:07:05 +0100 CET","ID":"{CID_DB}","Image":"postgres:16","Labels":"com.docker.compose.project=dataroom","Names":"dataroom-db-1","Networks":"dataroom_default","Ports":"5432/tcp","RunningFor":"3 days ago","State":"running","Status":"Up 3 days (healthy)"}}"#
    );
    let list = format!("{web}\n{worker}\n{db}\n");
    let inspect = r#"{"status":"restarting","running":false,"restarting":true,"oom_killed":false,"exit_code":139,"error":"","started_at":"2026-11-05T10:59:41.204118Z","finished_at":"2026-11-05T10:59:42.011342Z","health":null,"health_failing_streak":null,"restart_count":57,"image":"dataroom/worker:2.4.0","mounts":[{"type":"bind","source":"/srv/dataroom/worker.toml","destination":"/etc/worker/worker.toml","rw":false},{"type":"volume","source":"/var/lib/docker/volumes/dataroom_files/_data","destination":"/data","rw":true}]}"#;
    let mut logs = String::new();
    for cycle in 0..12u32 {
        let base = 10 * 3600 + 58 * 60 + 12 + i64::from(cycle) * 7;
        let ts = |off: i64| {
            let t = base + off;
            format!("2026-11-05T{:02}:{:02}:{:02}.{:06}000Z", t / 3600 - 1, t % 3600 / 60, t % 60, 104_118 + cycle * 3)
        };
        for (off, l) in [
            (0, "worker 2.4.0 starting (pid 1)".to_string()),
            (0, "loading config /etc/worker/worker.toml".to_string()),
            (0, "warning: unknown key 'storage.path' in /etc/worker/worker.toml (ignored)".to_string()),
            (0, "queue: connected to redis://queue:6379/0".to_string()),
            (0, "storage: opening backend".to_string()),
            (1, "*** SIGSEGV (address 0x0) received by PID 1".to_string()),
            (1, "#0  0x00007f3a1c2b8f3d in strlen () from /lib/x86_64-linux-gnu/libc.so.6".to_string()),
            (1, "#1  0x000055d0c1e4a2b1 in storage_open (cfg=0x55d0c3a0b2a0) at src/storage.c:88".to_string()),
            (1, "#2  0x000055d0c1e41a07 in main (argc=1, argv=0x7ffd2b6e1f58) at src/main.c:142".to_string()),
        ] {
            let _ = writeln!(logs, "{} {l}", ts(off));
        }
    }
    w.fake(
        Program::Docker,
        &[arm("ps -a --no-trunc", &list), arm("ps -a --format json", &list), arm("inspect --type=container", inspect), arm("logs --tail", logs)],
    );
    w.write("proc/uptime", "92430.11 1100000.00\n");
}

// ---- Fall 6: CIFS-Hänger ---------------------------------------------------------------------------------------------------

fn case_cifs(w: &World) {
    let d = "2026-11-05";
    let mut k = noise_kernel_warnings(d);
    let hung = |k: &mut Vec<String>, t: &str, task: &str, pid: u32| {
        for m in [
            format!("INFO: task {task}:{pid} blocked for more than 122 seconds."),
            "      Tainted: P           OE      7.2.14-arch1-1 #1".to_string(),
            "\"echo 0 > /proc/sys/kernel/hung_task_timeout_secs\" disables this message.".to_string(),
            format!("task:{task} state:D stack:0 pid:{pid} tgid:{pid} ppid:1702 flags:0x00000002"),
            "Call Trace:".to_string(),
            " <TASK>".to_string(),
            " __schedule+0x3c1/0x1280".to_string(),
            " schedule+0x27/0xf0".to_string(),
            " wait_for_response+0x10e/0x1c0 [cifs]".to_string(),
            " compound_send_recv+0x5e3/0xff0 [cifs]".to_string(),
            " smb2_query_path_info+0x1f2/0x5b0 [cifs]".to_string(),
            " cifs_get_inode_info+0x3f1/0x9a0 [cifs]".to_string(),
            " cifs_revalidate_dentry_attr+0x1d3/0x2e0 [cifs]".to_string(),
            " vfs_statx+0x9a/0x180".to_string(),
            " </TASK>".to_string(),
        ] {
            k.push(journal_line(&format!("{d}T{t}"), "kernel", &m));
        }
    };
    k.push(journal_line(&format!("{d}T10:12:40"), "kernel", "CIFS: VFS: \\\\nas has not responded in 180 seconds. Reconnecting..."));
    hung(&mut k, "10:14:33", "Thunar", 23011);
    k.push(journal_line(&format!("{d}T10:15:43"), "kernel", "CIFS: VFS: \\\\nas has not responded in 180 seconds. Reconnecting..."));
    hung(&mut k, "10:16:36", "rsync", 23340);
    hung(&mut k, "10:16:36", "kworker/u64:3", 22871);
    k.push(journal_line(&format!("{d}T10:18:46"), "kernel", "CIFS: VFS: \\\\nas has not responded in 180 seconds. Reconnecting..."));
    k.push(journal_line(&format!("{d}T10:20:12"), "kernel", "CIFS: VFS: cifs_setup_session: Send error in SessSetup = -112"));
    w.fake(Program::Journalctl, &[arm("-k --boot=0", lines(&k)), arm("--list-boots", boots_json())]);
    no_failed_units(w);
    w.write("proc/meminfo", MEMINFO_OK);
    psi(w, "0.00", "62.40");
    calm_machine(w, "9.85 9.40 7.12 2/1203 30211\n");
    w.fake(Program::Df, &[arm("-h -l", DF_OK)]);
}

// ---- Fall 7: DNS über eine tote Schnittstelle ----------------------------------------------------------------------------

fn case_dns(w: &World) {
    let addr = r#"[{"ifindex":1,"ifname":"lo","flags":["LOOPBACK","UP","LOWER_UP"],"mtu":65536,"operstate":"UNKNOWN","link_type":"loopback","address":"00:00:00:00:00:00","addr_info":[{"family":"inet","local":"127.0.0.1","prefixlen":8,"scope":"host"}]},
{"ifindex":2,"ifname":"enp5s0","flags":["BROADCAST","MULTICAST","UP","LOWER_UP"],"mtu":1500,"operstate":"UP","link_type":"ether","address":"aa:bb:cc:dd:ee:ff","addr_info":[{"family":"inet","local":"192.0.2.5","prefixlen":24,"scope":"global"},{"family":"inet6","local":"fe80::a8bb:ccff:fedd:eeff","prefixlen":64,"scope":"link"}]},
{"ifindex":4,"ifname":"docker0","flags":["NO-CARRIER","BROADCAST","MULTICAST","UP"],"mtu":1500,"operstate":"DOWN","link_type":"ether","address":"02:42:5c:1a:00:01","addr_info":[{"family":"inet","local":"172.17.0.1","prefixlen":16,"scope":"global"}]},
{"ifindex":5,"ifname":"tailscale0","flags":["POINTOPOINT","MULTICAST","NOARP","UP","LOWER_UP"],"mtu":1280,"operstate":"UNKNOWN","link_type":"none","addr_info":[{"family":"inet","local":"100.64.0.5","prefixlen":32,"scope":"global"}]},
{"ifindex":6,"ifname":"wg0","flags":["POINTOPOINT","NOARP"],"mtu":1420,"operstate":"DOWN","link_type":"none","addr_info":[]}]"#;
    let route = r#"[{"dst":"default","gateway":"192.0.2.1","dev":"enp5s0","protocol":"dhcp","prefsrc":"192.0.2.5","metric":100,"flags":[]},{"dst":"172.17.0.0/16","dev":"docker0","protocol":"kernel","scope":"link","prefsrc":"172.17.0.1","flags":["linkdown"]},{"dst":"192.0.2.0/24","dev":"enp5s0","protocol":"kernel","scope":"link","prefsrc":"192.0.2.5","metric":100,"flags":[]}]"#;
    w.fake(Program::Ip, &[arm("-j addr", addr), arm("-j route", route)]);
    let dns = "Global\n           Protocols: -LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n    resolv.conf mode: stub\n\n\
Link 2 (enp5s0)\n      Current Scopes: DNS\n           Protocols: +DefaultRoute -LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n  Current DNS Server: 192.0.2.1\n         DNS Servers: 192.0.2.1\n\n\
Link 4 (docker0)\n      Current Scopes: none\n           Protocols: -DefaultRoute -LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n\n\
Link 5 (tailscale0)\n      Current Scopes: none\n           Protocols: -DefaultRoute -LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n\n\
Link 6 (wg0)\n      Current Scopes: DNS\n           Protocols: -DefaultRoute -LLMNR -mDNS -DNSOverTLS DNSSEC=no/unsupported\n  Current DNS Server: 10.8.0.1\n         DNS Servers: 10.8.0.1\n          DNS Domain: ~.\n";
    w.fake(Program::Resolvectl, &[arm("status", dns)]);
    w.fake(
        Program::Ss,
        &[arm(
            "-Htulpn",
            "udp UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:* users:((\"systemd-resolve\",pid=598,fd=17))\nudp UNCONN 0 0 127.0.0.54:53 0.0.0.0:* users:((\"systemd-resolve\",pid=598,fd=19))\ntcp LISTEN 0 4096 127.0.0.53%lo:53 0.0.0.0:* users:((\"systemd-resolve\",pid=598,fd=18))\ntcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=800,fd=3))\ntcp LISTEN 0 4096 0.0.0.0:8080 0.0.0.0:* users:((\"docker-proxy\",pid=1501,fd=7))\n",
        )],
    );
    let ts = r#"{"Version":"1.80.0","TUN":true,"BackendState":"Running","TailscaleIPs":["100.64.0.5"],
"Self":{"ID":"n1","HostName":"blub","DNSName":"blub.tail1234.ts.net.","OS":"linux","TailscaleIPs":["100.64.0.5"],"Online":true,"Relay":"fra"},
"Health":[],
"Peer":{"nodekey:b1":{"HostName":"nas","DNSName":"nas.tail1234.ts.net.","Online":true,"Relay":"fra","TailscaleIPs":["100.64.0.9"],"ExitNode":false}}}"#;
    w.fake(Program::Tailscale, &[arm("status --json", ts)]);
    w.fake(Program::Findmnt, &[arm("", r#"{"filesystems":[]}"#)]);
}

// ---- Fall 8: volle Platte ---------------------------------------------------------------------------------------------------

fn case_disk_full(w: &World) {
    let d = "2026-11-05";
    let mut k = noise_kernel_warnings(d);
    for (t, m) in [
        ("10:42:17", "EXT4-fs (nvme0n1p4): Delayed block allocation failed for inode 3015472 at logical offset 0 with max blocks 2 with error 28"),
        ("10:42:17", "EXT4-fs (nvme0n1p4): This should not happen!! Data will be lost"),
        ("10:42:17", "EXT4-fs (nvme0n1p4): Total free blocks count 0"),
        ("10:42:17", "EXT4-fs (nvme0n1p4): Free/Dirty block details"),
        ("10:42:17", "EXT4-fs (nvme0n1p4): free_blocks=0"),
        ("10:42:17", "EXT4-fs (nvme0n1p4): dirty_blocks=4096"),
        ("11:03:55", "EXT4-fs (nvme0n1p4): Delayed block allocation failed for inode 3015488 at logical offset 12 with max blocks 8 with error 28"),
    ] {
        k.push(journal_line(&format!("{d}T{t}"), "kernel", m));
    }
    w.fake(Program::Journalctl, &[arm("-k --boot=0", lines(&k)), arm("--list-boots", boots_json())]);
    w.fake(
        Program::Systemctl,
        &[
            arm("--user --failed", r#"[{"unit":"dictate.service","load":"loaded","active":"failed","sub":"failed","description":"dictate daemon"}]"#),
            arm(
                "--failed",
                r#"[{"unit":"docker.service","load":"loaded","active":"failed","sub":"failed","description":"Docker Application Container Engine"},{"unit":"logrotate.service","load":"loaded","active":"failed","sub":"failed","description":"Rotate log files"}]"#,
            ),
        ],
    );
    w.write("proc/meminfo", MEMINFO_OK);
    psi(w, "0.00", "0.40");
    calm_machine(w, "0.60 0.72 0.80 1/1150 40211\n");
    w.fake(
        Program::Df,
        &[arm(
            "-h -l",
            "Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p4  217G  217G     0 100% /\n/dev/nvme0n1p1  511M  148M  364M  29% /boot\n/dev/sda1       3.6T  1.9T  1.6T  55% /home/data\n",
        )],
    );
}

// ---- Bericht über den Weg des Produkts ----------------------------------------------------------------------------------

struct Built {
    report: Report,
    text: String,
    sections: Vec<SectionLines>,
}

fn build(case: &Case) -> Built {
    let w = World::new(&format!("live-{}", case.name));
    (case.setup)(&w);
    let plan = Plan::new(case.profile, &(case.args)()).expect("the plan of the case");
    let mut s = Session::new(identity(), true, ReportBudget::from_kib(crate::config::DIAGNOSE_DEFAULT_REPORT_KIB).unwrap());
    s.collect_steps(&CollectLock::new(), &w.ctx, &w.cancel, Some(case.profile), plan.selected_steps(), &CollectParams::default(), &mut |_| {})
        .unwrap_or_else(|e| panic!("case {}: {e}", case.id));
    let report = s.report();
    let text = s.outgoing();
    // wie `service.rs` beim Senden
    let sections = report.sections.iter().map(|v| SectionLines { id: v.id.clone(), lines: if v.included { v.lines } else { 0 } }).collect();
    Built { report, text, sections }
}

/// Die Zeilen `from..=to` eines Abschnitts, wie das Panel sie als Beleg zeigt (die Nummer vor `| `, Zeile 1 = erste Zeile
/// hinter Kopf und `resolved:`).
fn evidence_lines(b: &Built, section: &str, from: usize, to: usize) -> String {
    let Some(view) = b.report.sections.iter().find(|v| v.id == section) else { return "<unknown section>".into() };
    let Some(at) = b.text.find(&view.header) else { return "<header not found>".into() };
    let body: Vec<&str> = b.text[at + view.header.len()..]
        .lines()
        .skip_while(|l| l.is_empty())
        .skip_while(|l| l.starts_with("resolved: "))
        .take_while(|l| !l.starts_with("=== S"))
        .collect();
    body.iter().skip(from.saturating_sub(1)).take(to + 1 - from.max(1)).copied().collect::<Vec<_>>().join("\n")
}

#[test]
fn cases_build_their_reports_through_the_product_path() {
    for case in cases() {
        let b = build(&case);
        for needle in case.report_must {
            assert!(b.text.contains(needle), "case {} ({}): {needle:?} missing in the report\n{}", case.id, case.name, b.text);
        }
        // Jede Probe des Profils hat ein Fake oder eine Datei: kein Abschnitt meldet ein fehlendes Programm oder einen Fehler
        for v in &b.report.sections {
            assert!(matches!(v.status, "ok" | "empty" | "partial"), "case {} ({}): {} is {}\n{}", case.id, case.name, v.probe, v.status, b.text);
        }
        assert!(
            !b.text.contains("alice") && !b.text.contains("blub ") && !b.text.contains("projects/dataroom"),
            "case {}: personal data left in the report",
            case.id
        );
        // keine gleichen Sammelzeilen untereinander (Aufgabe 10c)
        let lines: Vec<&str> = b.text.lines().collect();
        assert!(
            !lines.windows(2).any(|w| w[0].contains("× more of the") && w[1].contains("× more of the")),
            "case {}: summaries in a row\n{}",
            case.id,
            b.text
        );
        assert!(crate::actions::secrets::residue(&b.text).is_empty(), "case {}: secrets in the report", case.id);
    }
}

fn grade_hint(case: &Case, causes: &str) -> &'static str {
    let c = causes.to_lowercase();
    let hits = case.keys.iter().filter(|g| g.iter().any(|k| c.contains(&k.to_lowercase()))).count();
    match hits {
        n if n == case.keys.len() => "ja?",
        0 => "nein?",
        _ => "teilweise?",
    }
}

#[test]
#[ignore = "real Anthropic API calls; only with DICTATE_LIVE_DIAG=1, never in check.sh"]
fn live_diagnosis_against_claude() {
    let mode = std::env::var("DICTATE_LIVE_DIAG").unwrap_or_default();
    assert!(mode == "1" || mode == "dry", "set DICTATE_LIVE_DIAG=1 (real calls) or =dry (reports only)");
    let models: Vec<String> = std::env::var("DICTATE_DIAG_MODELS")
        .unwrap_or_else(|_| "claude-sonnet-5-5,claude-haiku-5-5".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let only: Option<Vec<u32>> = std::env::var("DICTATE_DIAG_CASES").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let out: PathBuf = std::env::var("DICTATE_DIAG_OUT").map(PathBuf::from).unwrap_or_else(|_| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        test_root::root().join("logs/diagnose-ai").join(t.to_string())
    });
    fs::create_dir_all(&out).unwrap();
    let ai = AiCfg {
        preset: "anthropic".into(),
        provider: "anthropic".into(),
        api_key_file: "~/.config/anthropic/api-key".into(),
        timeout_ms: 30_000,
        ..AiCfg::default()
    };
    let mut summary = String::from("case\tmodel\tgrade_hint\tfindings\tchecks\tdropped_findings\tdropped_evidence\tdropped_checks\tin\tout\tms\tcaps\terror\n");
    for case in cases() {
        if only.as_ref().is_some_and(|o| !o.contains(&case.id)) {
            continue;
        }
        let b = build(&case);
        fs::write(out.join(format!("{}-{}.report.txt", case.id, case.name)), &b.text).unwrap();
        if mode == "dry" {
            eprintln!("case {} {}: {} bytes, {} sections", case.id, case.name, b.text.len(), b.sections.len());
            continue;
        }
        for model in &models {
            let diag = DiagnoseCfg {
                enabled: true,
                model: model.clone(),
                ping_targets: vec![
                    PingTargetEntry { name: "gateway".into(), address: "192.0.2.1".into() },
                    PingTargetEntry { name: "nas".into(), address: "nas.example.org".into() },
                ],
                ..DiagnoseCfg::default()
            };
            let req =
                DiagRequest { report: &b.text, sections: b.sections.clone(), profile: Some(case.profile), round: 1, max_rounds: diag.rounds, symptom: false };
            let caps_before = (format!("{:?}", tool_caps_for(model)), format!("{:?}", thinking_for(model)));
            let started = Instant::now();
            let result = ask_diagnosis(&ai, &diag, &req, &Arc::new(AtomicBool::new(false)));
            let ms = started.elapsed().as_millis();
            let caps_after = (format!("{:?}", tool_caps_for(model)), format!("{:?}", thinking_for(model)));
            let caps = if caps_before == caps_after { "unchanged".to_string() } else { format!("{caps_before:?} -> {caps_after:?}") };
            let record = match &result {
                Ok(o) => {
                    let d = &o.diagnosis;
                    let evidence: Vec<Value> = d
                        .findings
                        .iter()
                        .map(|f| {
                            json!(f
                                .evidence
                                .iter()
                                .map(|e| json!({"section": e.section, "from": e.from, "to": e.to, "lines": evidence_lines(&b, &e.section, e.from, e.to)}))
                                .collect::<Vec<_>>())
                        })
                        .collect();
                    let causes: String = d.findings.iter().map(|f| f.cause.as_str()).collect::<Vec<_>>().join(" | ");
                    let hint = grade_hint(&case, &causes);
                    let _ = writeln!(
                        summary,
                        "{}\t{model}\t{hint}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{ms}\t{caps}\t",
                        case.id,
                        d.findings.len(),
                        d.checks.len(),
                        d.dropped.findings,
                        d.dropped.evidence,
                        d.dropped.checks(),
                        o.usage.input.unwrap_or(0),
                        o.usage.output.unwrap_or(0)
                    );
                    eprintln!("case {} {model}: {hint} {} findings, {} checks, dropped {:?}, {} ms", case.id, d.findings.len(), d.checks.len(), d.dropped, ms);
                    json!({"case": case.id, "name": case.name, "model": model, "ms": ms, "caps": caps,
                           "usage": {"input": o.usage.input, "output": o.usage.output},
                           "answer": d.to_json(), "evidence_lines": evidence,
                           "dropped": {"findings": d.dropped.findings, "evidence": d.dropped.evidence, "checks_invalid": d.dropped.checks_invalid,
                                       "checks_target": d.dropped.checks_target, "checks_extra": d.dropped.checks_extra},
                           "criterion": {"hit": case.hit, "partial": case.partial}})
                }
                Err(e) => {
                    // nur hier der Servertext (synthetische Daten, Schlüssel steht nie darin)
                    let detail = match e {
                        super::ai::DiagAiError::Ai(a) => a.to_string(),
                        other => other.to_string(),
                    };
                    let _ = writeln!(summary, "{}\t{model}\t-\t-\t-\t-\t-\t-\t-\t-\t{ms}\t{caps}\t{}", case.id, detail.replace(['\t', '\n'], " "));
                    eprintln!("case {} {model}: ERROR after {ms} ms: {detail}", case.id);
                    json!({"case": case.id, "model": model, "ms": ms, "caps": caps, "error": detail})
                }
            };
            fs::write(out.join(format!("{}-{}.{model}.json", case.id, case.name)), serde_json::to_string_pretty(&record).unwrap()).unwrap();
        }
    }
    fs::write(out.join("summary.tsv"), &summary).unwrap();
    eprintln!("results in {}", out.display());
}

#[test]
#[ignore = "writes the answer schema for a manual check against the API"]
fn dump_answer_schema() {
    let path = std::env::var("DICTATE_DIAG_SCHEMA_OUT").expect("DICTATE_DIAG_SCHEMA_OUT");
    fs::write(path, serde_json::to_string(super::ai::answer_schema()).unwrap()).unwrap();
}
