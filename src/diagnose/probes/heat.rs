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

//! Proben der Gruppe Heat: `sensors`, `gpu`, `nvme_temp`. Diagnose darf keine Hitze erzeugen: `sensors` liest nur hwmon,
//! `nvidia-smi` fragt einmal ab, die NVMe-Temperaturen kommen aus sysfs (ohne `smartctl`, das root bräuchte).

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{single, table};
use super::json;
use super::output::{Part, Status};
use super::text::{clean, cut_to_bytes};
use crate::diagnose::run::Keep;
use serde_json::Value;
use std::io;

// ---- sensors ---------------------------------------------------------------------------------------------------------

fn unit_of(key: &str) -> &'static str {
    if key.starts_with("temp") {
        "°C"
    } else if key.starts_with("fan") {
        "RPM"
    } else if key.starts_with("power") {
        "W"
    } else if key.starts_with("curr") {
        "A"
    } else if key.starts_with("in") {
        "V"
    } else {
        ""
    }
}

fn number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 => format!("{f:.0}"),
        Some(f) => format!("{f:.1}"),
        None => v.to_string(),
    }
}

/// Texte der Chips: Name, Adapter und je Merkmal der Messwert mit Schwellen. Merkmale ohne lesbaren Wert (leeres Objekt)
/// werden gezählt. Liefert den Text und die Zahl der nicht lesbaren Merkmale.
pub fn sensors_text(doc: &Value) -> Option<(String, usize)> {
    let chips = doc.as_object()?;
    let mut out = String::new();
    let mut unreadable = 0usize;
    for (chip, body) in chips {
        let Some(features) = body.as_object() else { continue };
        let adapter = features.get("Adapter").and_then(Value::as_str).unwrap_or("");
        out.push_str(&format!("{chip}{}\n", if adapter.is_empty() { String::new() } else { format!(" ({adapter})") }));
        for (name, feature) in features {
            if name == "Adapter" {
                continue;
            }
            let Some(subs) = feature.as_object() else { continue };
            let Some((input_key, input)) = subs.iter().find(|(k, _)| k.ends_with("_input")) else {
                if subs.is_empty() {
                    unreadable += 1;
                }
                continue;
            };
            let mut line = format!("  {name}: {} {}", number(input), unit_of(input_key)).trim_end().to_string();
            let mut found: Vec<(&str, String)> = Vec::new();
            let mut alarm = false;
            for (k, v) in subs {
                let Some((_, suffix)) = k.split_once('_') else { continue };
                match suffix {
                    "max" | "min" | "crit" | "high" | "low" | "emergency" => found.push((suffix, number(v))),
                    s if s.ends_with("alarm") && v.as_f64().is_some_and(|f| f != 0.0) => alarm = true,
                    _ => {}
                }
            }
            // Immer in derselben Reihenfolge: untere Grenzen, obere, kritische
            let order = ["min", "low", "max", "high", "crit", "emergency"];
            found.sort_by_key(|(s, _)| order.iter().position(|o| o == s));
            let thresholds: Vec<String> = found.iter().map(|(s, v)| format!("{s} {v}")).collect();
            if !thresholds.is_empty() {
                line.push_str(&format!(" ({})", thresholds.join(", ")));
            }
            if alarm {
                line.push_str(" ALARM");
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    Some((out, unreadable))
}

pub fn sensors(run: &mut Run) -> ProbeResult {
    let cmd = single(run, RAW_LIMIT, Keep::Head)?;
    let raw = cmd.raw_text();
    let Some((text, unreadable)) = json::document(&raw).and_then(|d| sensors_text(&d)) else {
        return Err(Status::Failed("sensors returned something that is not the expected JSON".into()));
    };
    if unreadable > 0 {
        run.out.note(format!("{unreadable} sensors could not be read"));
    }
    if let Some(note) = cmd.exit_note() {
        run.out.note(note);
    }
    if text.trim().is_empty() {
        run.out.push(Part::new("", "sensors found no chips."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let cut = cut_to_bytes(text.trim_end(), run.spec.limit_bytes.saturating_sub(128), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- gpu -------------------------------------------------------------------------------------------------------------

/// Die Gründe, aus denen die GPU ihren Takt senkt (`clocks_event_reasons.active`, `nvml`).
pub fn throttle_reasons(mask: u64) -> Vec<&'static str> {
    const BITS: &[(u64, &str)] = &[
        (0x1, "GPU idle"),
        (0x2, "applications clocks setting"),
        (0x4, "software power cap"),
        (0x8, "hardware slowdown"),
        (0x10, "sync boost"),
        (0x20, "software thermal slowdown"),
        (0x40, "hardware thermal slowdown"),
        (0x80, "hardware power brake slowdown"),
        (0x100, "display clock setting"),
    ];
    BITS.iter().filter(|(bit, _)| mask & bit != 0).map(|(_, name)| *name).collect()
}

fn parse_hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim().strip_prefix("0x").unwrap_or(s.trim()), 16).ok()
}

/// Text aus der CSV-Ausgabe von `nvidia-smi --query-gpu=… --format=csv`: je GPU ein Block `Feld: Wert`.
pub fn gpu_text(csv: &str) -> Option<String> {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<&str> = lines.next()?.split(", ").map(str::trim).collect();
    if !header.first().is_some_and(|h| h.starts_with("temperature.gpu")) {
        return None;
    }
    let mut out = String::new();
    let mut n = 0;
    for row in lines {
        let values: Vec<&str> = row.split(", ").map(str::trim).collect();
        if values.len() != header.len() {
            return None;
        }
        out.push_str(&format!("GPU {n}:\n"));
        for (name, value) in header.iter().zip(&values) {
            if name.starts_with("clocks_event_reasons.") && *name != "clocks_event_reasons.active" {
                // Einzelne Gründe nur, wenn sie aktiv sind
                if *value != "Not Active" {
                    out.push_str(&format!("  {name}: {value}\n"));
                }
                continue;
            }
            out.push_str(&format!("  {name}: {value}\n"));
            if *name == "clocks_event_reasons.active" {
                if let Some(mask) = parse_hex(value) {
                    let reasons = throttle_reasons(mask);
                    out.push_str(&format!("  throttle reasons: {}\n", if reasons.is_empty() { "none".to_string() } else { reasons.join(", ") }));
                }
            }
        }
        n += 1;
    }
    (n > 0).then_some(out)
}

pub fn gpu(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let cmd = single(run, limit.max(RAW_LIMIT / 16), Keep::Head)?;
    let text = cmd.text();
    match gpu_text(&text) {
        Some(t) => {
            let cut = cut_to_bytes(t.trim_end(), limit.saturating_sub(64), Keep::Head);
            if let Some(note) = cmd.exit_note() {
                run.out.note(note);
            }
            run.out.push(Part::new("", cut.text));
            Ok(())
        }
        // `nvidia-smi` meldet „has failed because it couldn't communicate with the NVIDIA driver“ auf stdout
        None => {
            Err(Status::Failed(format!("nvidia-smi gave no GPU data: {}", text.lines().next().unwrap_or("empty output").chars().take(200).collect::<String>())))
        }
    }
}

// ---- nvme_temp -------------------------------------------------------------------------------------------------------

pub fn nvme_temp(run: &mut Run) -> ProbeResult {
    let base = run.ctx.paths.under_root("/sys/class/nvme");
    let drives = match run.fs.read_dir(&base) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            run.out.push(Part::new("", "No NVMe drives (no /sys/class/nvme)."));
            run.out.status = Status::Empty;
            return Ok(());
        }
        Err(e) => return Err(Status::Failed(format!("cannot list /sys/class/nvme: {e}"))),
    };
    let mut rows = vec![vec!["drive".to_string(), "sensor".into(), "temperature".into()]];
    for drive in drives.iter().filter(|d| d.name.starts_with("nvme")) {
        run.check()?;
        let device = drive.path.join("device");
        let Ok(hwmons) = run.fs.read_dir(&device) else { continue };
        for hwmon in hwmons.iter().filter(|h| h.name.starts_with("hwmon") && h.is_dir) {
            let Ok(files) = run.fs.read_dir(&hwmon.path) else { continue };
            for f in files.iter().filter(|f| f.name.starts_with("temp") && f.name.ends_with("_input")) {
                let Ok(read) = run.fs.read(&f.path, 64) else { continue };
                let text = clean(&read.bytes);
                if let Ok(milli) = text.trim().parse::<i64>() {
                    let sensor = f.name.trim_end_matches("_input").to_string();
                    rows.push(vec![drive.name.clone(), sensor, format!("{:.1} °C", milli as f64 / 1000.0)]);
                }
            }
        }
    }
    if rows.len() == 1 {
        run.out.push(Part::new("", "No NVMe temperature sensors could be read."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    run.out.push(Part::new("", table(&rows)));
    Ok(())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn throttle_masks_decode() {
        assert_eq!(throttle_reasons(0), Vec::<&str>::new());
        assert_eq!(throttle_reasons(0x1), ["GPU idle"]);
        assert_eq!(throttle_reasons(0x4 | 0x40), ["software power cap", "hardware thermal slowdown"]);
        assert_eq!(parse_hex("0x0000000000000004"), Some(4));
        assert_eq!(parse_hex("[N/A]"), None);
    }

    #[test]
    fn sensors_render_inputs_thresholds_and_alarms() {
        let doc: Value = serde_json::from_str(
            r#"{"coretemp-isa-0000":{"Adapter":"ISA adapter","Package id 0":{"temp1_input":56.0,"temp1_max":100.0,"temp1_crit":105.5,"temp1_crit_alarm":0.0}},
               "dell_smm-isa-00de":{"Adapter":"ISA adapter","Processor Fan":{"fan1_input":1115.0},"Hot":{"temp2_input":90.0,"temp2_alarm":1.0},"Broken":{}},
               "weird":"not an object"}"#,
        )
        .unwrap();
        let (text, unreadable) = sensors_text(&doc).unwrap();
        assert!(text.contains("coretemp-isa-0000 (ISA adapter)\n  Package id 0: 56 °C (max 100, crit 105.5)\n"), "{text}");
        assert!(text.contains("  Processor Fan: 1115 RPM\n"), "{text}");
        assert!(text.contains("  Hot: 90 °C ALARM\n"), "{text}");
        assert_eq!(unreadable, 1);
        assert!(sensors_text(&serde_json::json!([1])).is_none());
    }
}
