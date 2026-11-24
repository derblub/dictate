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

//! Vorlese-Testsatz Ende zu Ende (GPU, Pulse): jede Aufnahme aus `tests/speech/testset.tsv` läuft über eine virtuelle
//! Pulse-Quelle in einen eigenen Daemon (eigener Socket, eigene Konfiguration ohne KI-Modus), `start`/`stop` wie beim
//! Halten von Mod+e. Bewertet wird der fertige Text, so wie ihn `dictate-ui` ausgibt.
//! `cargo test --release --test speech_testset -- --ignored --nocapture --test-threads=1` (oder `scripts/check.sh --only speech`)
//!
//! - Aufnahmen: `$DICTATE_TESTSET_DIR` (Standard `$DICTATE_TEST_ROOT/audio/testset/<id>.wav`, Testordner aus
//!   `tests/test_root/mod.rs`, auf blub `/home/data/stt-test`; 16 kHz mono, aufgenommen
//!   mit `scripts/record-testset.sh`). Fehlen alle, wird übersprungen; fehlen einzelne, schlägt der Test fehl (die
//!   Gesamt-Fehlerrate wäre nicht vergleichbar).
//! - Schwellen und harte Prüfungen stehen in `tests/speech/thresholds.tsv` (dort auch, wie sie abgeleitet sind).
//! - Ergebnis je Satz als TSV nach `$DICTATE_SPEECH_OUT` (Standard `$DICTATE_TEST_ROOT/logs/speech/speech-<zeit>.tsv`).
//! - Bewertung: Mehrheit aus `DICTATE_SPEECH_RUNS` Läufen je Satz (ungerade, Standard 3; Regel und Begründung im Kopf
//!   von `tests/speech/thresholds.tsv`). Läufe in Runden über alle Sätze; ein Satz läuft nicht weiter, sobald seine
//!   Mehrheit feststeht (meist nach zwei Läufen). `DICTATE_SPEECH_RUNS=1` = ein Lauf, jeder muss bestehen (schnell, für
//!   Agenten); `DICTATE_SPEECH_ALL=1` spielt immer alle Läufe (Ausgangsmessungen). Modell nur einmal geladen.
//! - `DICTATE_SPEECH_IDS=t03,t13` nur diese Sätze (Diagnose).
//!
//! Fehlerrate (CER): Zeichen-Editierdistanz nach [`words`] (klein, ohne Satzzeichen und Leerraum, Ziffern als deutsche
//! Zahlwörter, Füllwörter wie „ähm“ entfernt) geteilt durch die Länge der Referenz. Referenz ist die Spalte
//! „erwartet“, bei leerer Erwartung der Vorlesetext ohne Regieanweisungen. Gesamt-CER = Summe der Distanzen durch
//! Summe der Referenzlängen.

mod daemon_common;
mod pulse_common;
mod test_root;

use dictate::config::Config;
use pulse_common::{wav_ms, E2e};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

const SET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/speech/testset.tsv");
const LIMITS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/speech/thresholds.tsv");
/// Pause zwischen `start` und Abspielbeginn bzw. von Dateiende bis `stop` (die Aufnahmen enthalten schon die kurze
/// Stille vor und nach dem Sprechen; die 300 ms decken die Latenz von paplay ab, wie in `daemon_pulse_real`).
const LEAD_MS: u64 = 150;
const STOP_AFTER_MS: u64 = 300;

struct Row {
    id: String,
    what: String,
    /// Vorlesetext ohne Regieanweisungen
    spoken: String,
    /// Erwarteter getippter Text (`\n` schon als Zeilenumbruch); `None` = nur Fehlerrate
    expected: Option<String>,
}

impl Row {
    fn reference(&self) -> &str {
        self.expected.as_deref().unwrap_or(&self.spoken)
    }
}

/// Entfernt Regieanweisungen `[..]` und zieht Leerraum zusammen.
fn without_directions(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_set(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("id\t") {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert!((3..=4).contains(&cols.len()), "testset.tsv: expected 3 or 4 columns: {line:?}");
        let expected = cols.get(3).map(|e| e.trim()).filter(|e| !e.is_empty()).map(|e| e.replace("\\n", "\n"));
        rows.push(Row { id: cols[0].into(), what: cols[1].into(), spoken: without_directions(cols[2]), expected });
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Check {
    /// keine harte Prüfung
    None,
    /// getippter Text (ohne Leerzeichen am Ende) gleich der Erwartung, samt Zeilenumbrüchen und Satzzeichen
    Exact,
    /// Satzzeichen und Zeilenumbrüche in derselben Reihenfolge wie in der Erwartung ([`marks`]); die Wörter bewertet
    /// die CER. Für Sprachbefehle: ein Hörfehler („Lieber“ statt „Liebe“) ist kein Fehler des Befehls.
    Marks,
    /// erstes und letztes Wort (nach [`words`]) wie in der Referenz
    Ends,
}

struct Limits {
    total: f64,
    rows: BTreeMap<String, (f64, Check)>,
}

fn parse_limits(text: &str) -> Limits {
    let mut total = None;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("id\t") {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 3, "thresholds.tsv: expected id, max_cer, check: {line:?}");
        let max: f64 = cols[1].parse().unwrap_or_else(|_| panic!("thresholds.tsv: bad max_cer {:?}", cols[1]));
        let check = match cols[2] {
            "-" => Check::None,
            "exact" => Check::Exact,
            "marks" => Check::Marks,
            "ends" => Check::Ends,
            other => panic!("thresholds.tsv: unknown check {other:?} (-, exact, marks, ends)"),
        };
        if cols[0] == "total" {
            total = Some(max);
        } else {
            rows.insert(cols[0].to_string(), (max, check));
        }
    }
    Limits { total: total.expect("thresholds.tsv: no total row"), rows }
}

const UNITS: [&str; 20] = [
    "null",
    "eins",
    "zwei",
    "drei",
    "vier",
    "fünf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "elf",
    "zwölf",
    "dreizehn",
    "vierzehn",
    "fünfzehn",
    "sechzehn",
    "siebzehn",
    "achtzehn",
    "neunzehn",
];
const TENS: [&str; 10] = ["", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig"];
const MONTHS: [&str; 12] = ["januar", "februar", "märz", "april", "mai", "juni", "juli", "august", "september", "oktober", "november", "dezember"];
const FILLERS: [&str; 7] = ["äh", "ähm", "äm", "öh", "öhm", "hm", "hmm"];

/// 1–99; `ein` statt `eins` vor einem weiteren Wortteil („einundzwanzig“, „eintausend“ gibt es hier nicht).
fn below_100(n: u64, standalone: bool) -> String {
    match n {
        0 => String::new(),
        1 if !standalone => "ein".into(),
        1..=19 => UNITS[n as usize].into(),
        _ if n.is_multiple_of(10) => TENS[(n / 10) as usize].into(),
        _ => format!("{}und{}", below_100(n % 10, false), TENS[(n / 10) as usize]),
    }
}

/// 1–999, „hundert“ statt „einhundert“ (so wird gesprochen).
fn below_1000(n: u64, standalone: bool) -> String {
    let (h, r) = (n / 100, n % 100);
    let head = match h {
        0 => String::new(),
        1 => "hundert".into(),
        _ => format!("{}hundert", below_100(h, false)),
    };
    head + &below_100(r, standalone)
}

/// Deutsche Grundzahl als ein Wort, 0 bis 999 999 („tausendzweihundertneunundvierzig“); größere bleiben Ziffern.
fn de_cardinal(n: u64) -> String {
    match n {
        0 => "null".into(),
        1..=999 => below_1000(n, true),
        1000..=999_999 => {
            let (t, r) = (n / 1000, n % 1000);
            let head = if t == 1 { "tausend".to_string() } else { below_1000(t, false) + "tausend" };
            head + &below_1000(r, true)
        }
        _ => n.to_string(),
    }
}

/// Ordnungszahl im Dativ wie nach „am“ („dreiundzwanzigsten“, „dritten“); andere Endungen kosten nur wenige Zeichen.
fn de_ordinal(n: u64) -> String {
    match n {
        1 => "ersten".into(),
        3 => "dritten".into(),
        7 => "siebten".into(),
        8 => "achten".into(),
        2..=19 => de_cardinal(n) + "ten",
        _ if n >= 100 && (1..20).contains(&(n % 100)) => de_cardinal(n - n % 100) + &de_ordinal(n % 100),
        _ => de_cardinal(n) + "sten",
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    W(String),
    N(String),
    Dot,
    Colon,
    Gap,
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<Tok>| {
        if cur.is_empty() {
            return;
        }
        let t = std::mem::take(cur);
        out.push(if t.chars().all(|c| c.is_ascii_digit()) { Tok::N(t) } else { Tok::W(t) });
    };
    for c in s.to_lowercase().chars() {
        let same_kind = cur.chars().next().is_none_or(|p| p.is_ascii_digit() == c.is_ascii_digit());
        if c.is_alphanumeric() {
            if !same_kind {
                flush(&mut cur, &mut out);
            }
            cur.push(c);
            continue;
        }
        flush(&mut cur, &mut out);
        out.push(match c {
            '.' => Tok::Dot,
            ':' => Tok::Colon,
            _ => Tok::Gap,
        });
    }
    flush(&mut cur, &mut out);
    out
}

/// Normalisierte Wörter: klein, ohne Satzzeichen, ohne Füllwörter, Ziffern als Zahlwörter. Erkennt „14.30 Uhr“ /
/// „14:30 Uhr“ (→ „vierzehn uhr dreißig“), Tausenderpunkte („1.249“) und Ordnungszahlen vor Monatsnamen
/// („23. Oktober“); jede andere Ziffernfolge ist eine eigene Grundzahl („1,2,3“ → „eins zwei drei“).
fn words(s: &str) -> Vec<String> {
    let t = tokens(s);
    let next_word = |mut j: usize| -> Option<&str> {
        while j < t.len() {
            match &t[j] {
                Tok::W(w) => return Some(w),
                Tok::Gap | Tok::Dot => j += 1,
                _ => return None,
            }
        }
        None
    };
    let num = |s: &str| -> String { s.parse::<u64>().map(de_cardinal).unwrap_or_else(|_| s.to_string()) };
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::W(w) => {
                if !FILLERS.contains(&w.as_str()) {
                    out.push(w.clone());
                }
                i += 1;
            }
            Tok::N(a) => {
                // Uhrzeit: H.MM / H:MM vor „Uhr“
                if let (Some(Tok::Dot | Tok::Colon), Some(Tok::N(m))) = (t.get(i + 1), t.get(i + 2)) {
                    if a.len() <= 2 && m.len() == 2 && next_word(i + 3) == Some("uhr") {
                        out.push(num(a));
                        out.push("uhr".into());
                        if m != "00" {
                            out.push(num(m));
                        }
                        i += 3;
                        while !matches!(t.get(i), Some(Tok::W(_)) | None) {
                            i += 1;
                        }
                        i += 1; // „uhr“
                        continue;
                    }
                }
                // Tausenderpunkte: 1.249, 12.000.000
                let mut j = i;
                let mut digits = a.clone();
                while a.len() <= 3 && matches!(t.get(j + 1), Some(Tok::Dot)) {
                    match t.get(j + 2) {
                        Some(Tok::N(g)) if g.len() == 3 => {
                            digits.push_str(g);
                            j += 2;
                        }
                        _ => break,
                    }
                }
                if j > i {
                    out.push(num(&digits));
                    i = j + 1;
                    continue;
                }
                // Ordnungszahl vor einem Monatsnamen: „23. Oktober“
                if matches!(t.get(i + 1), Some(Tok::Dot)) && next_word(i + 2).is_some_and(|w| MONTHS.contains(&w)) {
                    out.push(a.parse::<u64>().map(de_ordinal).unwrap_or_else(|_| a.clone()));
                    i += 2;
                    continue;
                }
                out.push(num(a));
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut d: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = d[0];
        d[0] = i;
        for j in 1..=b.len() {
            let cur = d[j];
            d[j] = (prev + usize::from(a[i - 1] != b[j - 1])).min(d[j] + 1).min(d[j - 1] + 1);
            prev = cur;
        }
    }
    d[b.len()]
}

/// (Distanz, Länge der Referenz) über die normalisierten Zeichen ohne Leerraum.
fn char_errors(reference: &str, hypothesis: &str) -> (usize, usize) {
    let r: Vec<char> = words(reference).concat().chars().collect();
    let h: Vec<char> = words(hypothesis).concat().chars().collect();
    (levenshtein(&r, &h), r.len())
}

fn hard_check(check: Check, row: &Row, text: &str) -> Option<bool> {
    match check {
        Check::None => None,
        Check::Exact => Some(text.trim_end_matches(' ') == row.expected.as_deref().expect("exact check needs an expected text")),
        Check::Marks => Some(marks(text) == marks(row.expected.as_deref().expect("marks check needs an expected text"))),
        Check::Ends => {
            let (r, h) = (words(row.reference()), words(text));
            Some(r.first() == h.first() && r.last() == h.last())
        }
    }
}

/// Gerüst eines Textes: nur Satzzeichen und Zeilenumbrüche, in Reihenfolge.
fn marks(text: &str) -> String {
    text.chars().filter(|c| matches!(c, ',' | '.' | ';' | ':' | '?' | '!' | '\n')).collect()
}

/// Für die TSV-Ausgabe: Backslash, Tab und Zeilenumbruch maskieren.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn out_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DICTATE_SPEECH_OUT") {
        return PathBuf::from(p);
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    test_root::root().join("logs/speech").join(format!("speech-{secs}.tsv"))
}

#[test]
#[ignore]
fn testset_over_pulse() {
    let mut rows = parse_set(&std::fs::read_to_string(SET).unwrap());
    let limits = parse_limits(&std::fs::read_to_string(LIMITS).unwrap());
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let unknown: Vec<&String> = limits.rows.keys().filter(|k| !ids.contains(&k.as_str())).collect();
    let unlisted: Vec<&str> = ids.iter().copied().filter(|id| !limits.rows.contains_key(*id)).collect();
    assert!(unknown.is_empty() && unlisted.is_empty(), "thresholds.tsv does not match testset.tsv: unknown {unknown:?}, missing {unlisted:?}");
    for r in &rows {
        let check = limits.rows[&r.id].1;
        assert!(!matches!(check, Check::Exact | Check::Marks) || r.expected.is_some(), "{}: exact check without expected text", r.id);
    }

    // Nur ausgewählte Sätze (Diagnose); die Gesamt-CER ist dann nicht mit der Schwelle vergleichbar
    let only: Option<Vec<String>> = std::env::var("DICTATE_SPEECH_IDS").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    if let Some(only) = &only {
        let unknown: Vec<&String> = only.iter().filter(|id| !rows.iter().any(|r| &r.id == *id)).collect();
        assert!(unknown.is_empty(), "DICTATE_SPEECH_IDS: unknown {unknown:?}");
        rows.retain(|r| only.contains(&r.id));
    }
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let dir = std::env::var_os("DICTATE_TESTSET_DIR").map(PathBuf::from).unwrap_or_else(|| test_root::audio("testset"));
    let wav = |id: &str| dir.join(format!("{id}.wav"));
    let missing: Vec<&str> = ids.iter().copied().filter(|id| !wav(id).is_file()).collect();
    if missing.len() == ids.len() {
        println!("SKIP: no recordings in {} (record them with scripts/record-testset.sh)", dir.display());
        return;
    }
    assert!(missing.is_empty(), "recordings missing in {}: {missing:?} (scripts/record-testset.sh {})", dir.display(), missing.join(" "));
    let k: usize = std::env::var("DICTATE_SPEECH_RUNS").ok().map(|v| v.parse().expect("DICTATE_SPEECH_RUNS: number")).unwrap_or(RUNS);
    assert!(k % 2 == 1, "DICTATE_SPEECH_RUNS must be odd (majority vote), got {k}");
    let all = std::env::var_os("DICTATE_SPEECH_ALL").is_some();

    let mut cfg = Config::default();
    cfg.features.ai = false;
    cfg.features.history = false;
    let e = E2e::start(cfg);

    let out = out_path();
    let mut tsv = format!("# speech_testset runs={k} all={all} audio={} limits={LIMITS}\n", dir.display());
    tsv.push_str("id\trun\tcheck\tcer\tmax_cer\thard\trun_ok\treference\ttext\traw\n");
    let mut runs: Vec<Vec<Run>> = rows.iter().map(|_| Vec::new()).collect();
    let mut texts: Vec<Vec<String>> = rows.iter().map(|_| Vec::new()).collect();
    for round in 1..=k {
        for (i, r) in rows.iter().enumerate() {
            if !all && verdict(&runs[i], k) != Verdict::Open {
                continue;
            }
            let (max, check) = limits.rows[&r.id];
            let w = wav(&r.id);
            let d = e.dictate(&w, LEAD_MS, wav_ms(&w), STOP_AFTER_MS, &format!("{}-{round}", r.id));
            let (dist, len) = char_errors(r.reference(), &d.text);
            let run = Run { dist, len, max, hard: hard_check(check, r, &d.text) };
            let ok = run.cer_ok() && run.hard != Some(false);
            let hard_s = match run.hard {
                None => "-",
                Some(true) => "ok",
                Some(false) => "FAIL",
            };
            let ok_s = if ok { "ok" } else { "FAIL" };
            println!("{} #{round} {ok_s:4} cer {:.3} (max {max:.2}) hard {hard_s:4} {:<28} {:?}", r.id, run.cer(), r.what, d.text);
            writeln!(
                tsv,
                "{}\t{round}\t{check:?}\t{:.4}\t{max:.2}\t{hard_s}\t{ok_s}\t{}\t{}\t{}",
                r.id,
                run.cer(),
                esc(r.reference()),
                esc(&d.text),
                esc(&d.raw)
            )
            .unwrap();
            runs[i].push(run);
            texts[i].push(d.text);
        }
    }
    drop(e);

    tsv.push_str("# je Satz: Mehrheit aus den Läufen (Regel im Kopf von tests/speech/thresholds.tsv)\n");
    tsv.push_str("id\truns\tcer_median\tcer_max\tmax_cer\tcer_ok\thard_ok\tverdict\n");
    let mut failures = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let rs = &runs[i];
        let v = verdict(rs, k);
        assert_ne!(v, Verdict::Open, "{}: undecided after {} run(s)", r.id, rs.len());
        let hard_ok = match rs.iter().filter(|x| x.hard.is_some()).count() {
            0 => "-".to_string(),
            n => format!("{}/{n}", rs.iter().filter(|x| x.hard == Some(true)).count()),
        };
        let cer_ok = format!("{}/{}", rs.iter().filter(|x| x.cer_ok()).count(), rs.len());
        let (med, top) = (median_cer(rs), rs.iter().map(Run::cer).fold(0.0, f64::max));
        let v_s = if v == Verdict::Pass { "ok" } else { "FAIL" };
        writeln!(tsv, "{}\t{}\t{med:.4}\t{top:.4}\t{:.2}\t{cer_ok}\t{hard_ok}\t{v_s}", r.id, rs.len(), rs[0].max).unwrap();
        if v == Verdict::Fail {
            let detail: Vec<String> = rs.iter().zip(&texts[i]).map(|(x, t)| format!("cer {:.3} hard {:?} {t:?}", x.cer(), x.hard)).collect();
            failures.push(format!("{}: median CER {med:.3} (max {:.2}), CER ok {cer_ok}, hard ok {hard_ok}: {}", r.id, rs[0].max, detail.join(" | ")));
        }
    }
    // Gesamt-CER über die Runden, die jeder Satz durchlaufen hat (feste Auswahl, unabhängig vom Ausgang)
    let rounds = if all { k } else { k / 2 + 1 };
    let (dist_sum, len_sum) = runs.iter().flat_map(|rs| &rs[..rounds]).fold((0, 0), |(d, l), x| (d + x.dist, l + x.len));
    let total = dist_sum as f64 / len_sum.max(1) as f64;
    // Teilmenge (DICTATE_SPEECH_IDS): nur berichten, die Schwelle gilt für den ganzen Testsatz
    let total_ok = only.is_some() || total <= limits.total;
    writeln!(tsv, "total\t{rounds}\t{total:.4}\t-\t{:.2}\t-\t-\t{}", limits.total, if total_ok { "ok" } else { "FAIL" }).unwrap();
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(&out, tsv).unwrap();
    let n_runs: usize = runs.iter().map(Vec::len).sum();
    println!(
        "total CER {total:.4} (max {:.2}) over rounds 1-{rounds}, {} sentence(s), {n_runs} run(s), majority of {k}, results: {}",
        limits.total,
        rows.len(),
        out.display()
    );
    if !total_ok {
        failures.push(format!("total CER {total:.4} > {:.2}", limits.total));
    }
    assert!(failures.is_empty(), "{} failure(s):\n{}", failures.len(), failures.join("\n"));
}

/// Läufe je Satz im Standardfall (Mehrheit aus drei)
const RUNS: usize = 3;

/// Ein Lauf eines Satzes
#[derive(Clone, Copy, Debug)]
struct Run {
    dist: usize,
    len: usize,
    /// Schwelle des Satzes
    max: f64,
    hard: Option<bool>,
}

impl Run {
    fn cer(&self) -> f64 {
        self.dist as f64 / self.len.max(1) as f64
    }
    fn cer_ok(&self) -> bool {
        self.cer() <= self.max
    }
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Pass,
    Fail,
    /// noch nicht entschieden, weiterer Lauf nötig
    Open,
}

/// Mehrheit aus `k` Läufen (k ungerade), getrennt für „CER ≤ Schwelle“ und die harte Prüfung; ein Satz besteht,
/// wenn beide in der Mehrheit bestehen. Entschieden, sobald die Mehrheit feststeht: weitere Läufe änderten das
/// Ergebnis nicht mehr, deshalb ist das Abbrechen dann gleichwertig mit allen `k` Läufen.
/// „CER in der Mehrheit ≤ Schwelle“ ist dasselbe wie „Median-CER ≤ Schwelle“.
fn verdict(runs: &[Run], k: usize) -> Verdict {
    let need = k / 2 + 1;
    let cer_pass = runs.iter().filter(|r| r.cer_ok()).count();
    let hard_pass = runs.iter().filter(|r| r.hard != Some(false)).count();
    let (cer_fail, hard_fail) = (runs.len() - cer_pass, runs.len() - hard_pass);
    if cer_fail >= need || hard_fail >= need {
        Verdict::Fail
    } else if cer_pass >= need && hard_pass >= need {
        Verdict::Pass
    } else {
        Verdict::Open
    }
}

/// Median der CER (bei gerader Anzahl der Mittelwert der beiden mittleren)
fn median_cer(runs: &[Run]) -> f64 {
    let mut v: Vec<f64> = runs.iter().map(Run::cer).collect();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

// Bewertung ohne Audio (läuft im normalen `cargo test`)

#[test]
fn cardinals_and_ordinals() {
    let cases = [
        (1, "eins"),
        (9, "neun"),
        (14, "vierzehn"),
        (21, "einundzwanzig"),
        (30, "dreißig"),
        (101, "hunderteins"),
        (1249, "tausendzweihundertneunundvierzig"),
        (2026, "zweitausendsechsundzwanzig"),
        (21000, "einundzwanzigtausend"),
    ];
    for (n, w) in cases {
        assert_eq!(de_cardinal(n), w, "{n}");
    }
    assert_eq!(de_ordinal(23), "dreiundzwanzigsten");
    assert_eq!(de_ordinal(3), "dritten");
    assert_eq!(de_ordinal(12), "zwölften");
    assert_eq!(de_ordinal(101), "hundertersten");
}

#[test]
fn normalization_matches_spoken_form() {
    let spoken = "Am dreiundzwanzigsten Oktober zweitausendsechsundzwanzig um vierzehn Uhr dreißig kostet das Paket tausendzweihundertneunundvierzig Euro.";
    for typed in ["Am 23. Oktober 2026 um 14.30 Uhr kostet das Paket 1249 Euro.", "Am 23. Oktober 2026 um 14:30 Uhr kostet das Paket 1.249 Euro."] {
        assert_eq!(char_errors(spoken, typed).0, 0, "{typed}");
    }
    assert_eq!(words("Test 1,2,3"), ["test", "eins", "zwei", "drei"]);
    assert_eq!(words("Test eins, zwei, drei."), ["test", "eins", "zwei", "drei"]);
    assert_eq!(words("um 9 Uhr"), ["um", "neun", "uhr"]);
    assert_eq!(words("um 14.00 Uhr"), ["um", "vierzehn", "uhr"]);
    assert_eq!(words("Ich schicke dir das, ähm, morgen"), ["ich", "schicke", "dir", "das", "morgen"]);
    assert_eq!(words("Feature-Flag, Python-Skript"), ["feature", "flag", "python", "skript"]);
    assert_eq!(words("v1.2"), ["v", "eins", "zwei"]);
}

#[test]
fn cer_and_hard_checks() {
    assert_eq!(char_errors("Liebe Grüße,\nDaniel.", "Lieber Grüße, Daniel."), (1, 16));
    let row = |exp: &str| Row { id: "t".into(), what: String::new(), spoken: String::new(), expected: Some(exp.into()) };
    let t11 = row("Liebe Grüße,\nDaniel.");
    assert_eq!(hard_check(Check::Exact, &t11, "Liebe Grüße,\nDaniel. "), Some(true));
    assert_eq!(hard_check(Check::Exact, &t11, "Liebe Grüße, Daniel. "), Some(false));
    assert_eq!(hard_check(Check::Marks, &t11, "Lieber Grüße,\nDaniel. "), Some(true));
    assert_eq!(hard_check(Check::Marks, &t11, "Liebe Grüße\nDaniel. "), Some(false));
    assert_eq!(hard_check(Check::Marks, &t11, "Liebe Grüße, Daniel. "), Some(false));
    let t12 = row("Ist das schon alles?\n\nJa!");
    assert_eq!(hard_check(Check::Marks, &t12, "Ist das schon alles?\n\nja! "), Some(true));
    assert_eq!(hard_check(Check::Marks, &t12, "Ist das schon alles? Ja! "), Some(false));
    let t13 = row("Bitte speichere die Datei unter dem Namen Quartalsbericht.");
    assert_eq!(hard_check(Check::Ends, &t13, "bitte speichere die Datei unter Namen Quartalsbericht "), Some(true));
    assert_eq!(hard_check(Check::Ends, &t13, "Bitte speichere die Datei unter dem Namen."), Some(false));
    assert_eq!(hard_check(Check::None, &t13, ""), None);
}

#[test]
fn majority_verdict_stops_once_settled() {
    let run = |cer_ok: bool, hard: Option<bool>| Run { dist: if cer_ok { 0 } else { 10 }, len: 100, max: 0.05, hard };
    let (ok, bad) = (run(true, None), run(false, None));
    assert_eq!(verdict(&[], 3), Verdict::Open);
    assert_eq!(verdict(&[ok], 3), Verdict::Open);
    assert_eq!(verdict(&[ok, ok], 3), Verdict::Pass);
    assert_eq!(verdict(&[bad, ok], 3), Verdict::Open);
    assert_eq!(verdict(&[bad, ok, ok], 3), Verdict::Pass);
    assert_eq!(verdict(&[ok, bad, bad], 3), Verdict::Fail);
    assert_eq!(verdict(&[bad, bad], 3), Verdict::Fail);
    // k = 1: jeder Lauf entscheidet
    assert_eq!(verdict(&[ok], 1), Verdict::Pass);
    assert_eq!(verdict(&[bad], 1), Verdict::Fail);
    // CER und harte Prüfung brauchen je ihre Mehrheit, nicht im selben Lauf
    let (cer_only, hard_only) = (run(true, Some(false)), run(false, Some(true)));
    assert_eq!(verdict(&[cer_only, hard_only, ok], 3), Verdict::Pass);
    assert_eq!(verdict(&[cer_only, cer_only], 3), Verdict::Fail);
    assert_eq!(verdict(&[run(true, Some(true)), cer_only], 3), Verdict::Open);
    // Mehrheit ≤ Schwelle ist Median ≤ Schwelle
    let r = |dist| Run { dist, len: 100, max: 0.05, hard: None };
    assert_eq!(median_cer(&[r(9), r(0), r(3)]), 0.03);
    assert_eq!(median_cer(&[r(0), r(6)]), 0.03);
}

#[test]
fn testset_and_thresholds_agree() {
    let rows = parse_set(&std::fs::read_to_string(SET).unwrap());
    let limits = parse_limits(&std::fs::read_to_string(LIMITS).unwrap());
    assert!(rows.len() >= 10);
    for r in &rows {
        let (max, check) = limits.rows.get(&r.id).unwrap_or_else(|| panic!("{} missing in thresholds.tsv", r.id));
        assert!((0.0..=1.0).contains(max), "{}", r.id);
        assert!(!matches!(*check, Check::Exact | Check::Marks) || r.expected.is_some(), "{}", r.id);
    }
    assert_eq!(limits.rows.len(), rows.len());
    assert_eq!(rows.iter().find(|r| r.id == "t08").unwrap().spoken, "Das ist der erste Satz. Und das ist der zweite Satz nach der Pause.");
    assert_eq!(rows.iter().find(|r| r.id == "t12").unwrap().expected.as_deref(), Some("Ist das schon alles?\n\nJa!"));
}
