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

//! Gemeinsame Hilfen der Tests im Modul `diagnose` (nur mit `cfg(test)`).

use super::probe::{BootOffset, ContainerRef, Days, Lines, PingTarget, Probe, Since, UnitRef, WindowMin};
use serde_json::Value;

/// Jede Probe einmal mit *allen* Feldern gesetzt, als Struct-Literal ohne `..`: kommt ein Feld im Typ hinzu,
/// übersetzt der Test nicht mehr. Die Reihenfolge ist die des Katalogs.
pub fn full_probes() -> Vec<Probe> {
    let unit = UnitRef::new("docker.service").unwrap();
    let container = ContainerRef::new("web").unwrap();
    let boot = BootOffset::PREVIOUS;
    vec![
        Probe::Boots {},
        Probe::BootEnd { boot, lines: Lines::N50 },
        Probe::KernelErrors { boot, lines: Lines::N50 },
        Probe::Pstore { boot },
        Probe::TempsTail { boot, window_min: WindowMin::M5 },
        Probe::KernelInfo {},
        Probe::PacmanLog { boot, days: Days::D1 },
        Probe::DriverVersions {},
        Probe::FailedUnits {},
        Probe::UnitStatus { unit: unit.clone() },
        Probe::UnitShow { unit: unit.clone() },
        Probe::UnitJournal { unit, since: Since::Boot, lines: Lines::N50 },
        Probe::Timers {},
        Probe::Coredumps { since: Since::Min15 },
        Probe::DockerPs {},
        Probe::DockerState { container: container.clone() },
        Probe::DockerLogs { container, since: Since::Hours6, lines: Lines::N50 },
        Probe::DockerDf {},
        Probe::DockerStats {},
        Probe::NetLinks {},
        Probe::NetDns {},
        Probe::NetListen {},
        Probe::NetTailscale {},
        Probe::NetMounts {},
        Probe::NetReach { target: PingTarget::new("nas").unwrap() },
        Probe::Mem {},
        Probe::Load {},
        Probe::Disks {},
        Probe::TopProcs {},
        Probe::Oom { boot },
        Probe::Sensors {},
        Probe::Gpu {},
        Probe::NvmeTemp {},
    ]
}

/// Ein kleiner Prüfer für genau die Teilmenge von JSON-Schema, die der Katalog erzeugt (dieselbe wie im Test der
/// Palette). Jedes andere Schlüsselwort lässt den Test scheitern, damit das Schema in der Teilmenge bleibt, die alle
/// Anbieter kennen.
pub fn schema_accepts(schema: &Value, v: &Value) -> bool {
    let s = schema.as_object().expect("schema is an object");
    for (key, rule) in s {
        let ok = match key.as_str() {
            "description" => true,
            "type" => match rule.as_str().unwrap() {
                "object" => v.is_object(),
                "string" => v.is_string(),
                "integer" => v.is_i64() || v.is_u64(),
                "null" => v.is_null(),
                other => panic!("type {other} is outside the supported subset"),
            },
            "enum" => rule.as_array().unwrap().contains(v),
            "minimum" => v.as_i64().is_none_or(|n| n >= rule.as_i64().unwrap()),
            "maximum" => v.as_i64().is_none_or(|n| n <= rule.as_i64().unwrap()),
            "minLength" => v.as_str().is_none_or(|t| t.chars().count() as u64 >= rule.as_u64().unwrap()),
            "maxLength" => v.as_str().is_none_or(|t| t.chars().count() as u64 <= rule.as_u64().unwrap()),
            "properties" => v.as_object().is_none_or(|o| rule.as_object().unwrap().iter().all(|(k, sub)| o.get(k).is_none_or(|val| schema_accepts(sub, val)))),
            "required" => v.as_object().is_none_or(|o| rule.as_array().unwrap().iter().all(|k| o.contains_key(k.as_str().unwrap()))),
            "additionalProperties" => {
                assert_eq!(rule, &serde_json::json!(false), "only additionalProperties:false is supported");
                v.as_object().is_none_or(|o| o.keys().all(|k| s["properties"].as_object().unwrap().contains_key(k)))
            }
            "anyOf" => rule.as_array().unwrap().iter().any(|sub| schema_accepts(sub, v)),
            "oneOf" => rule.as_array().unwrap().iter().filter(|sub| schema_accepts(sub, v)).count() == 1,
            other => panic!("keyword {other} is outside the supported subset"),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Jede Probe mit einem gültigen Beispiel (JSON) und derselben Probe als Rust-Wert.
pub fn examples() -> Vec<(&'static str, Probe)> {
    let unit = |s: &str| UnitRef::new(s).unwrap();
    let container = |s: &str| ContainerRef::new(s).unwrap();
    vec![
        (r#"{"probe":"boots"}"#, Probe::Boots {}),
        (r#"{"probe":"boot_end","boot":-1,"lines":200}"#, Probe::BootEnd { boot: BootOffset::PREVIOUS, lines: Lines::N200 }),
        (r#"{"probe":"boot_end","boot":0,"lines":50}"#, Probe::BootEnd { boot: BootOffset::CURRENT, lines: Lines::N50 }),
        (r#"{"probe":"kernel_errors","boot":-9,"lines":500}"#, Probe::KernelErrors { boot: BootOffset::new(-9).unwrap(), lines: Lines::N500 }),
        (r#"{"probe":"pstore","boot":-1}"#, Probe::Pstore { boot: BootOffset::PREVIOUS }),
        (r#"{"probe":"temps_tail","boot":-1,"window_min":15}"#, Probe::TempsTail { boot: BootOffset::PREVIOUS, window_min: WindowMin::M15 }),
        (r#"{"probe":"kernel_info"}"#, Probe::KernelInfo {}),
        (r#"{"probe":"pacman_log","boot":0,"days":7}"#, Probe::PacmanLog { boot: BootOffset::CURRENT, days: Days::D7 }),
        (r#"{"probe":"driver_versions"}"#, Probe::DriverVersions {}),
        (r#"{"probe":"failed_units"}"#, Probe::FailedUnits {}),
        (r#"{"probe":"unit_status","unit":"docker.service"}"#, Probe::UnitStatus { unit: unit("docker.service") }),
        (r#"{"probe":"unit_show","unit":"docker"}"#, Probe::UnitShow { unit: unit("docker") }),
        (
            r#"{"probe":"unit_journal","unit":"docker.service","since":"1h","lines":200}"#,
            Probe::UnitJournal { unit: unit("docker.service"), since: Since::Hour1, lines: Lines::N200 },
        ),
        (
            r#"{"probe":"unit_journal","unit":"getty@tty1.service","since":"boot","lines":50}"#,
            Probe::UnitJournal { unit: unit("getty@tty1.service"), since: Since::Boot, lines: Lines::N50 },
        ),
        (r#"{"probe":"timers"}"#, Probe::Timers {}),
        (r#"{"probe":"coredumps","since":"24h"}"#, Probe::Coredumps { since: Since::Hours24 }),
        (r#"{"probe":"docker_ps"}"#, Probe::DockerPs {}),
        (r#"{"probe":"docker_state","container":"dataroom-web-1"}"#, Probe::DockerState { container: container("dataroom-web-1") }),
        (
            r#"{"probe":"docker_logs","container":"dataroom-web-1","since":"15m","lines":500}"#,
            Probe::DockerLogs { container: container("dataroom-web-1"), since: Since::Min15, lines: Lines::N500 },
        ),
        (r#"{"probe":"docker_df"}"#, Probe::DockerDf {}),
        (r#"{"probe":"docker_stats"}"#, Probe::DockerStats {}),
        (r#"{"probe":"net_links"}"#, Probe::NetLinks {}),
        (r#"{"probe":"net_dns"}"#, Probe::NetDns {}),
        (r#"{"probe":"net_listen"}"#, Probe::NetListen {}),
        (r#"{"probe":"net_tailscale"}"#, Probe::NetTailscale {}),
        (r#"{"probe":"net_mounts"}"#, Probe::NetMounts {}),
        (r#"{"probe":"net_reach","target":"gateway"}"#, Probe::NetReach { target: PingTarget::new("gateway").unwrap() }),
        (r#"{"probe":"net_reach","target":"1.1.1.1"}"#, Probe::NetReach { target: PingTarget::new("1.1.1.1").unwrap() }),
        (r#"{"probe":"mem"}"#, Probe::Mem {}),
        (r#"{"probe":"load"}"#, Probe::Load {}),
        (r#"{"probe":"disks"}"#, Probe::Disks {}),
        (r#"{"probe":"top_procs"}"#, Probe::TopProcs {}),
        (r#"{"probe":"oom","boot":-2}"#, Probe::Oom { boot: BootOffset::new(-2).unwrap() }),
        (r#"{"probe":"sensors"}"#, Probe::Sensors {}),
        (r#"{"probe":"gpu"}"#, Probe::Gpu {}),
        (r#"{"probe":"nvme_temp"}"#, Probe::NvmeTemp {}),
    ]
}

/// Fehlerfälle: (JSON, Teil der Fehlermeldung, ob auch das Schema den Fall ablehnt). Was das Schema in seiner Teilmenge
/// nicht ausdrückt (Zeichen und erstes Zeichen der Suchbegriffe, Leerraum), lehnt nur Rust ab.
pub fn rejected() -> Vec<(String, &'static str, bool)> {
    let long = |n: usize| "x".repeat(n);
    let mut cases: Vec<(String, &'static str, bool)> = Vec::new();
    let mut add = |json: String, msg: &'static str, schema: bool| cases.push((json, msg, schema));
    // Probe und Felder
    add(r#"{"probe":"restart_unit","unit":"docker.service"}"#.into(), "unknown probe", true);
    add(r#"{"probe":"docker_rm","container":"web"}"#.into(), "unknown probe", true);
    add(r#"{"probe":"shell","cmd":"id"}"#.into(), "unknown probe", true);
    add(r#"{"probe":""}"#.into(), "unknown probe", true);
    add(r#"{"probe":"BOOTS"}"#.into(), "unknown probe", true);
    add(r#"{"probe":"boots","extra":1}"#.into(), "unknown field", true);
    add(r#"{"probe":"mem","lines":50}"#.into(), "unknown field", true);
    add(r#"{"probe":"boot_end","boot":-1,"lines":50,"since":"1h"}"#.into(), "unknown field", true);
    add(r#"{"probe":"unit_status","unit":"a.service","scope":"user"}"#.into(), "unknown field", true);
    add(r#"{"probe":"net_reach","target":"nas","count":100}"#.into(), "unknown field", true);
    add(r#"{"boot":-1,"lines":50}"#.into(), "missing \"probe\"", true);
    add(r#"{"probe":5}"#.into(), "missing \"probe\"", true);
    add(r#"{"probe":null}"#.into(), "missing \"probe\"", true);
    add(r#"{"probe":"boot_end","boot":-1}"#.into(), "missing field", true);
    add(r#"{"probe":"boot_end","lines":50}"#.into(), "missing field", true);
    add(r#"{"probe":"unit_journal","unit":"a.service","since":"1h"}"#.into(), "missing field", true);
    add(r#"{"probe":"unit_status"}"#.into(), "missing field", true);
    add(r#"{"probe":"docker_logs","container":"web","lines":50}"#.into(), "missing field", true);
    add(r#"{"probe":"net_reach"}"#.into(), "missing field", true);
    add(r#"{"probe":"temps_tail","boot":-1}"#.into(), "missing field", true);
    add(r#"{"probe":"pacman_log","days":3}"#.into(), "missing field", true);
    // Boot-Versatz
    add(r#"{"probe":"boot_end","boot":1,"lines":50}"#.into(), "between -9 and 0", true);
    add(r#"{"probe":"boot_end","boot":-10,"lines":50}"#.into(), "between -9 and 0", true);
    add(r#"{"probe":"kernel_errors","boot":-100,"lines":50}"#.into(), "between -9 and 0", true);
    add(r#"{"probe":"oom","boot":9}"#.into(), "between -9 and 0", true);
    add(r#"{"probe":"pstore","boot":-1.5}"#.into(), "must be an integer", true);
    add(r#"{"probe":"pstore","boot":"-1"}"#.into(), "must be an integer", true);
    add(r#"{"probe":"pstore","boot":null}"#.into(), "must be an integer", true);
    add(r#"{"probe":"pstore","boot":true}"#.into(), "must be an integer", true);
    add(r#"{"probe":"oom","boot":"all"}"#.into(), "must be an integer", true);
    // Zeilen
    add(r#"{"probe":"boot_end","boot":0,"lines":100}"#.into(), "must be one of: 50, 200, 500", true);
    add(r#"{"probe":"boot_end","boot":0,"lines":0}"#.into(), "must be one of: 50, 200, 500", true);
    add(r#"{"probe":"boot_end","boot":0,"lines":-1}"#.into(), "must be one of: 50, 200, 500", true);
    add(r#"{"probe":"boot_end","boot":0,"lines":100000}"#.into(), "must be one of: 50, 200, 500", true);
    add(r#"{"probe":"boot_end","boot":0,"lines":"50"}"#.into(), "must be an integer", true);
    add(r#"{"probe":"kernel_errors","boot":0,"lines":200.5}"#.into(), "must be an integer", true);
    add(r#"{"probe":"docker_logs","container":"web","since":"1h","lines":501}"#.into(), "must be one of: 50, 200, 500", true);
    // Zeitfenster
    add(r#"{"probe":"coredumps","since":"7d"}"#.into(), "must be one of: 15m, 1h, 6h, 24h, boot", true);
    add(r#"{"probe":"coredumps","since":"yesterday"}"#.into(), "must be one of: 15m, 1h, 6h, 24h, boot", true);
    add(r#"{"probe":"coredumps","since":"-1h"}"#.into(), "must be one of: 15m, 1h, 6h, 24h, boot", true);
    add(r#"{"probe":"coredumps","since":""}"#.into(), "must be one of: 15m, 1h, 6h, 24h, boot", true);
    add(r#"{"probe":"coredumps","since":3600}"#.into(), "must be a string", true);
    add(r#"{"probe":"unit_journal","unit":"a.service","since":"all","lines":50}"#.into(), "must be one of", true);
    add(r#"{"probe":"docker_logs","container":"web","since":"2h","lines":50}"#.into(), "must be one of", true);
    // Fenster der Rust-Proben
    add(r#"{"probe":"temps_tail","boot":0,"window_min":10}"#.into(), "must be one of: 5, 15, 60", true);
    add(r#"{"probe":"temps_tail","boot":0,"window_min":0}"#.into(), "must be one of: 5, 15, 60", true);
    add(r#"{"probe":"temps_tail","boot":0,"window_min":"15"}"#.into(), "must be an integer", true);
    add(r#"{"probe":"pacman_log","boot":0,"days":2}"#.into(), "must be one of: 1, 3, 7", true);
    add(r#"{"probe":"pacman_log","boot":0,"days":30}"#.into(), "must be one of: 1, 3, 7", true);
    // Suchbegriffe
    add(r#"{"probe":"unit_status","unit":""}"#.into(), "must not be empty", true);
    add(r#"{"probe":"unit_status","unit":"   "}"#.into(), "must not be empty", false);
    add(r#"{"probe":"unit_status","unit":5}"#.into(), "must be a string", true);
    add(r#"{"probe":"unit_status","unit":["a"]}"#.into(), "must be a string", true);
    add(r#"{"probe":"unit_status","unit":null}"#.into(), "must be a string", true);
    add(r#"{"probe":"unit_status","unit":"--user"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"unit_status","unit":"-u"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"unit_show","unit":"-.mount"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"unit_show","unit":"/etc/systemd/system/x.service"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"unit_journal","unit":"a b","since":"1h","lines":50}"#.into(), "may only contain", false);
    add(r#"{"probe":"unit_journal","unit":"a;rm -rf /","since":"1h","lines":50}"#.into(), "may only contain", false);
    add(r#"{"probe":"unit_journal","unit":"a\nb","since":"1h","lines":50}"#.into(), "may only contain", false);
    add(r#"{"probe":"unit_journal","unit":"ünit","since":"1h","lines":50}"#.into(), "must start with a letter or digit", false);
    add(format!(r#"{{"probe":"unit_status","unit":"{}"}}"#, long(129)), "at most 128", true);
    add(r#"{"probe":"docker_state","container":"--all"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"docker_state","container":"web:latest"}"#.into(), "may only contain", false);
    add(r#"{"probe":"docker_logs","container":"a b","since":"1h","lines":50}"#.into(), "may only contain", false);
    add(r#"{"probe":"docker_logs","container":"","since":"1h","lines":50}"#.into(), "must not be empty", true);
    add(format!(r#"{{"probe":"docker_state","container":"{}"}}"#, long(129)), "at most 128", true);
    add(r#"{"probe":"net_reach","target":"-c 1000000"}"#.into(), "must start with a letter or digit", false);
    add(r#"{"probe":"net_reach","target":"8.8.8.8 -f"}"#.into(), "may only contain", false);
    add(r#"{"probe":"net_reach","target":"http://example.com"}"#.into(), "may only contain", false);
    add(r#"{"probe":"net_reach","target":""}"#.into(), "must not be empty", true);
    add(format!(r#"{{"probe":"net_reach","target":"{}"}}"#, long(65)), "at most 64", true);
    // Jede Probe lehnt ein fremdes Feld ab; jede mit Parametern verlangt sie alle
    for spec in crate::diagnose::catalog::all() {
        if spec.params.is_empty() {
            add(format!(r#"{{"probe":"{}","extra":1}}"#, spec.name), "unknown field", true);
        } else {
            add(format!(r#"{{"probe":"{}"}}"#, spec.name), "missing field", true);
        }
    }
    cases
}
