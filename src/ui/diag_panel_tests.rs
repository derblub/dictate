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

//! Tests des Diagnose-Automaten mit Antworten in der Form des Daemons (`src/diagnose/service.rs`); Fälle aus der früheren
//! Lua-Vorlage (`diagnose.lua`, Zeilen in PLAN-wayland.md „Stand W4“).

use super::*;
use crate::ui::gate::{DEADTIME, IDLE};
use serde_json::json;

fn no_mods() -> Mods {
    Mods::default()
}

/// Alle Aufrufe einer Folge
fn calls(out: &[Effect]) -> Vec<(u64, Call)> {
    out.iter()
        .filter_map(|e| match e {
            Effect::Call { seq, call, .. } => Some((*seq, call.clone())),
            _ => None,
        })
        .collect()
}

fn last_call(out: &[Effect]) -> (u64, Call) {
    calls(out).pop().unwrap_or_else(|| panic!("no call in {out:?}"))
}

fn profiles_json() -> Value {
    json!({"ok": true, "profiles": [
        {"name": "now", "label": "Right now", "description": "What is wrong now", "needs_unit": false},
        {"name": "unit", "label": "A failed unit", "description": "One unit", "needs_unit": true},
        {"name": "docker", "label": "Docker", "description": "Containers", "needs_unit": false},
    ]})
}

fn plan_json() -> Value {
    json!({"ok": true, "items": [
        {"index": 0, "label": "kernel_info", "probe": {"probe": "kernel_info"}, "selected": true, "network": false},
        {"index": 1, "label": "journal", "probe": {"probe": "journal", "since": "1h"}, "selected": true, "network": false},
        {"index": 2, "label": "docker_df", "probe": {"probe": "docker_df"}, "selected": false, "network": false},
    ]})
}

fn preview_json(preview: u64, rounds: u64) -> Value {
    let mut sections = vec![
        json!({"id": "S1", "included": true, "header": "=== S1 · kernel_info · 2 lines", "round": 1, "placeholders": {"host": 1}}),
        json!({"id": "S2", "included": true, "header": "=== S2 · journal · 3 lines\nresolved: unit user-1.service", "round": 1, "placeholders": {"user": 1}}),
    ];
    let mut text = String::from(
        "Diagnostic report\nPersonal values are replaced by stable placeholders (user-N, host-N).\n\n=== S1 · kernel_info · 2 lines\n1| Linux host-1 7.3\n2| user-1000.slice ok\n\n=== S2 · journal · 3 lines\nresolved: unit user-1.service\n1| [REDACTED:password] by user-1\n2| user-2 is not a placeholder here\n3| done\n",
    );
    if rounds > 1 {
        sections.push(json!({"id": "S3", "included": true, "header": "=== S3 · mem · 1 line · round 2", "round": 2, "placeholders": {}}));
        text.push_str("\n=== S3 · mem · 1 line · round 2\n1| MemFree: 1 GiB\n");
    }
    json!({"ok": true, "id": "7", "preview": preview, "rounds": rounds, "max_rounds": 3, "text": text, "bytes": text.len(), "budget": 49152,
        "sections": sections, "redactions": 1, "placeholders": 2, "pseudonymize": true, "max_symptom_chars": 300,
        "target": {"host": "api.anthropic.com", "model": "claude-sonnet-5-5", "cloud": true, "ready": true}})
}

fn answer_json(rounds: u64) -> Value {
    json!({"ok": true, "id": "7", "preview": 1, "rounds": rounds, "max_rounds": 3, "model": "claude-sonnet-5-5",
    "answer": {
        "findings": [
            {"cause": "The disk of user-1 is full", "confidence": "high", "evidence": [{"section": "S2", "from": 1, "to": 2}, {"section": "S9", "from": 1, "to": 1}]},
            {"cause": "Nothing else", "confidence": "bogus", "evidence": []},
        ],
        "checks": [
            {"probe": {"probe": "mem"}, "reason": "memory", "network": false},
            {"probe": {"probe": "net_reach", "target": "nas"}, "reason": "reach the nas", "network": true},
        ],
        "notes": "Free space for user-1, run `du -sh /home/user-1`.",
    },
    "restore": {
        "findings": [[{"start": 12, "end": 18, "placeholder": "user-1", "value": "alice"}], []],
        "notes": [{"start": 15, "end": 21, "placeholder": "user-1", "value": "alice"}],
    }})
}

/// Öffnen bis zur Auswahl der Profile; liefert den Zeitpunkt nach der Totzeit
fn opened(p: &mut DiagPanel, t0: Instant) -> Instant {
    let out = p.toggle(t0);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Plan { profile: None, unit: None, container: None });
    assert_eq!(p.view, View::Loading);
    assert!(!out.contains(&Effect::Show), "Loading erscheint erst nach SHOW_DELAY");
    let out = p.reply(p.gen(), seq, Ok(&mut profiles_json()), t0);
    assert!(out.contains(&Effect::Show) && out.contains(&Effect::Render), "{out:?}");
    assert_eq!(p.view, View::Profiles);
    assert_eq!(p.active.as_deref(), Some("profile:1"));
    t0 + DEADTIME
}

/// … bis zur Vorschau (Profil „now“, alles gesammelt)
fn previewed(p: &mut DiagPanel, t0: Instant) -> Instant {
    let t = opened(p, t0);
    let out = p.key_press("1", no_mods(), t);
    p.key_release("1", t);
    assert_eq!(p.view, View::Items);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Plan { profile: Some("now".into()), unit: None, container: None });
    p.reply(p.gen(), seq, Ok(&mut plan_json()), t);
    assert_eq!(p.selected_count(), 2);
    let t = t + DEADTIME;
    let out = p.key_press("Return", no_mods(), t);
    p.key_release("Return", t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Collect { profile: "now".into(), unit: None, container: None, skip: vec![2] });
    assert_eq!(p.view, View::Progress);
    p.progress_line(p.gen(), seq, &json!({"progress": {"id": "7", "phase": "start", "index": 1, "total": 2, "probe": "kernel_info", "params": ""}}));
    p.progress_line(
        p.gen(),
        seq,
        &json!({"progress": {"id": "7", "phase": "end", "index": 1, "total": 2, "probe": "kernel_info", "status": "ok", "bytes": 10, "ms": 3}}),
    );
    assert_eq!(p.progress.as_ref().map(|pr| (pr.total, pr.done(), pr.lines)), Some((2, 1, 2)));
    assert_eq!(p.id.as_deref(), Some("7"));
    p.reply(p.gen(), seq, Ok(&mut preview_json(1, 1)), t);
    assert_eq!(p.view, View::Preview);
    assert_eq!(p.active.as_deref(), Some("send"));
    assert_eq!(p.sec.as_deref(), Some("S1"));
    t + DEADTIME
}

/// … bis zur Antwort
fn answered(p: &mut DiagPanel, t0: Instant) -> Instant {
    let t = previewed(p, t0);
    let out = p.key_press("Return", no_mods(), t);
    p.key_release("Return", t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Send { id: "7".into(), preview: 1 });
    assert_eq!(p.view, View::Send);
    p.reply(p.gen(), seq, Ok(&mut answer_json(1)), t);
    assert_eq!(p.view, View::Answer);
    t + DEADTIME
}

#[test]
fn whole_flow_profiles_items_collect_preview_send_answer() {
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    let a = p.answer.as_ref().unwrap();
    // Belege nur auf Abschnitte dieses Berichts, unbekannte Sicherheit wird „low“
    assert_eq!(a.findings.len(), 2);
    assert_eq!(a.findings[0].evidence, [Evidence { section: "S2".into(), from: 1, to: 2 }]);
    assert_eq!(a.findings[1].confidence, "low");
    assert_eq!(a.checks[1].label, "net_reach · target=nas");
    assert!(a.has_values());
    assert_eq!(p.answer_model.as_deref(), Some("claude-sonnet-5-5"));
    // zuerst der erste Beleg hervorgehoben, Enter springt in die Vorschau mit markierten Zeilen
    assert_eq!(p.active.as_deref(), Some("ev:1:1"));
    let out = p.key_press("Return", no_mods(), t);
    p.key_release("Return", t);
    assert!(out.contains(&Effect::Render));
    assert_eq!(p.view, View::Preview);
    assert_eq!(p.evidence, Some(Evidence { section: "S2".into(), from: 1, to: 2 }));
    assert_eq!(p.take_scroll(), Some(ScrollTo { section: "S2".into(), line: Some(1) }));
    // derselbe Bericht: kein zweites Senden, ⌫ zurück zur Antwort auf denselben Beleg
    assert!(p.reviewing() && !p.can_send());
    assert!(!p.items().iter().any(|i| i.id == "send"));
    assert_eq!(p.active.as_deref(), Some("back"));
    let t = t + DEADTIME;
    p.key_press("BackSpace", no_mods(), t);
    p.key_release("BackSpace", t);
    assert_eq!(p.view, View::Answer);
    assert_eq!(p.active.as_deref(), Some("ev:1:1"));
    assert_eq!(p.evidence, None);
}

#[test]
fn checks_are_picked_with_space_and_run_with_enter_network_only_when_chosen() {
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    // ↓ über die Belege zu den Prüfbefehlen
    p.key_press("Down", no_mods(), t);
    assert_eq!(p.active.as_deref(), Some("check:1"));
    p.key_press("space", no_mods(), t);
    p.key_release("space", t);
    assert_eq!(p.picked.iter().copied().collect::<Vec<_>>(), [1]);
    assert_eq!(p.items().iter().find(|i| i.id == "run").map(|i| (i.label.clone(), i.enabled)), Some(("Run 1 check".into(), true)));
    // Enter auf einem Prüfbefehl führt die gewählten aus (ohne Netz)
    let t2 = t + Duration::from_millis(200);
    let out = p.key_press("Return", no_mods(), t2);
    p.key_release("Return", t2);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Check { id: "7".into(), checks: vec![json!({"probe": "mem"})], network: false });
    assert_eq!(p.view, View::Progress);
    assert!(p.check_round && p.picked.is_empty());
    // Ergebnis der Runde: Vorschau steht auf dem neuen Abschnitt
    p.reply(p.gen(), seq, Ok(&mut preview_json(2, 2)), t2);
    assert_eq!(p.view, View::Preview);
    assert_eq!(p.new_ids, ["S3"]);
    assert_eq!(p.sec.as_deref(), Some("S3"));
    assert_eq!(p.take_scroll(), Some(ScrollTo { section: "S3".into(), line: None }));
    // neue Vorschau: Senden wieder möglich
    assert!(!p.reviewing() && p.can_send());

    // mit dem Netz-Prüfbefehl: network = true
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    assert!(p.activate("check:2", t).contains(&Effect::Render));
    let out = p.activate("run", t);
    assert_eq!(last_call(&out).1, Call::Check { id: "7".into(), checks: vec![json!({"probe": "net_reach", "target": "nas"})], network: true });
}

#[test]
fn no_checks_in_the_last_round() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    let out = p.key_press("Return", no_mods(), t);
    let (seq, _) = last_call(&out);
    p.reply(p.gen(), seq, Ok(&mut answer_json(3)), t);
    assert!(!p.rounds_left());
    assert!(p.items().iter().filter(|i| i.id.starts_with("check:")).all(|i| !i.enabled));
    assert!(p.activate("check:1", t + DEADTIME).is_empty());
    assert!(p.items().iter().any(|i| i.id == "run" && !i.enabled));
}

#[test]
fn stop_hangs_up_cancels_and_goes_back() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let t = opened(&mut p, t0);
    let out = p.activate("profile:1", t);
    let (seq, _) = last_call(&out);
    p.reply(p.gen(), seq, Ok(&mut plan_json()), t);
    let out = p.activate("collect", t + DEADTIME);
    let (collect, _) = last_call(&out);
    // vor der ersten Fortschrittszeile ist die Kennung unbekannt: nur auflegen
    let out = p.key_press("Escape", no_mods(), t + DEADTIME);
    assert_eq!(out.first(), Some(&Effect::Hangup { seq: collect }));
    assert!(calls(&out).is_empty(), "{out:?}");
    assert_eq!(p.view, View::Items);
    // spätes Ergebnis des abgebrochenen Sammelns zählt nicht
    assert!(p.reply(p.gen(), collect, Ok(&mut preview_json(1, 1)), t).is_empty());
    assert_eq!(p.view, View::Items);
    // mit Kennung: auflegen und `diag_cancel`
    let t2 = t + DEADTIME * 3;
    let out = p.activate("collect", t2);
    let (collect, _) = last_call(&out);
    p.progress_line(p.gen(), collect, &json!({"progress": {"id": "8", "phase": "start", "index": 1, "total": 2, "probe": "journal", "params": "since=1h"}}));
    let out = p.activate("stop", t2 + DEADTIME);
    assert_eq!(out[0], Effect::Hangup { seq: collect });
    assert_eq!(last_call(&out).1, Call::Cancel { id: "8".into() });
    assert_eq!(p.stats.hangups, 2);
}

#[test]
fn stop_while_sending_returns_to_the_report_and_a_foreign_cancel_is_an_error() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    let out = p.activate("send", t);
    let (seq, _) = last_call(&out);
    let out = p.key_press("Escape", no_mods(), t);
    assert_eq!(out[0], Effect::Hangup { seq });
    assert_eq!(p.view, View::Preview);
    assert!(p.reply(p.gen(), seq, Err(CallErr::Cancelled), t).is_empty());
    // abgebrochen von woanders (neue Sammlung, Ablauf): Fehler mit Rückweg zur Vorschau und Copy
    let out = p.activate("send", t + DEADTIME);
    let (seq, _) = last_call(&out);
    p.reply(p.gen(), seq, Err(CallErr::Cancelled), t);
    assert_eq!(p.view, View::Error);
    let e = p.err.clone().unwrap();
    assert_eq!((e.back, e.copy), (Some(View::Preview), true));
}

#[test]
fn sections_are_toggled_with_the_whole_deselection() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    p.key_press("Down", no_mods(), t);
    assert_eq!(p.sec.as_deref(), Some("S2"));
    assert_eq!(p.take_scroll(), Some(ScrollTo { section: "S2".into(), line: None }));
    p.key_press("Down", no_mods(), t);
    assert_eq!(p.sec.as_deref(), Some("S2"), "clamped at the last section");
    let out = p.key_press("space", no_mods(), t);
    p.key_release("space", t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Preview { id: "7".into(), deselected: Some(vec!["S2".into()]), symptom: None });
    // während die Vorschau unterwegs ist: kein zweites Umschalten, kein Senden
    assert!(!p.can_send());
    assert!(p.key_press("space", no_mods(), t + Duration::from_millis(200)).is_empty());
    let mut v = preview_json(2, 1);
    v["sections"][1]["included"] = json!(false);
    p.reply(p.gen(), seq, Ok(&mut v), t);
    assert!(p.can_send());
    // wieder aufnehmen: leere Abwahl; ein Klick auf den Kopf tut dasselbe
    let out = p.activate("sec:S2", t + DEADTIME);
    assert_eq!(last_call(&out).1, Call::Preview { id: "7".into(), deselected: Some(vec![]), symptom: None });
}

#[test]
fn copy_reports_only_the_count() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    let out = p.key_press("c", no_mods(), t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Copy { id: "7".into() });
    assert!(p.key_press("c", no_mods(), t + Duration::from_millis(400)).is_empty(), "one copy at a time");
    let out = p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "chars": 1234})), t);
    assert!(out.contains(&Effect::Notice("Copied the report (1234 characters)".into())), "{out:?}");
    assert_eq!(p.notice, Some(("Copied the report: 1234 characters.".into(), false)));
    assert_eq!(p.stats.copies, 1);
}

#[test]
fn the_symptom_field_applies_with_enter_reverts_with_escape_and_comes_back_after_a_new_collection() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let t = previewed(&mut p, t0);
    // Tab führt ins Feld
    let out = p.key_press("Tab", no_mods(), t);
    assert!(out.contains(&Effect::Field(true)), "{out:?}");
    assert!(p.field.editing && !p.can_send());
    p.field_text("Wifi drops after resume".into(), t);
    // Esc verwirft
    let out = p.key_press("Escape", no_mods(), t);
    assert!(out.contains(&Effect::Field(false)) && calls(&out).is_empty(), "{out:?}");
    assert!(p.is_open() && p.field.text.is_empty());
    // Klick hinein, tippen, Enter übernimmt
    p.field_focused(t);
    p.field_text("Wifi drops after resume".into(), t);
    let t2 = t + DEADTIME;
    let out = p.key_press("Return", no_mods(), t2);
    p.key_release("Return", t2);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Preview { id: "7".into(), deselected: None, symptom: Some(SecretText::new("Wifi drops after resume".into())) });
    assert!(!p.field.editing && !p.can_send());
    let mut v = preview_json(2, 1);
    v["symptom"] = json!({"text": "Wifi drops after resume", "redactions": {}, "placeholders": {}});
    p.reply(p.gen(), seq, Ok(&mut v), t2);
    assert_eq!(p.field.applied, "Wifi drops after resume");
    assert_eq!(p.preview.as_ref().unwrap().symptom.as_ref().unwrap().text, "Wifi drops after resume");
    assert!(p.can_send());
    // derselbe Text noch einmal: kein Aufruf
    p.key_press("Tab", no_mods(), t2 + DEADTIME);
    let out = p.key_press("Return", no_mods(), t2 + DEADTIME * 2);
    assert!(calls(&out).is_empty(), "{out:?}");
    // neue Sammlung: das übernommene Symptom wird in der neuen Sitzung wieder gesetzt
    let mut p2 = DiagPanel::new();
    let t = previewed(&mut p2, t0);
    p2.activate("symptom", t);
    p2.field_text("Fan is loud".into(), t);
    let (seq, _) = last_call(&p2.key_press("Return", no_mods(), t));
    p2.reply(p2.gen(), seq, Ok(&mut preview_json(2, 1)), t);
    p2.view = View::Items;
    let out = p2.trigger("collect", true, t + DEADTIME * 3);
    let (seq, _) = last_call(&out);
    let out = p2.reply(p2.gen(), seq, Ok(&mut preview_json(1, 1)), t);
    assert_eq!(last_call(&out).1, Call::Preview { id: "7".into(), deselected: None, symptom: Some(SecretText::new("Fan is loud".into())) });
}

#[test]
fn a_click_beside_the_open_field_applies_it_and_does_nothing_else() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    p.activate("symptom", t);
    p.field_text("Screen freezes".into(), t);
    let out = p.activate("send", t);
    let (_, call) = last_call(&out);
    assert!(matches!(call, Call::Preview { symptom: Some(_), .. }), "{call:?}");
    assert_eq!(p.view, View::Preview, "not sent: the user sees the new preview first");
    assert_eq!(p.stats.sends, 0);
}

#[test]
fn a_refused_symptom_shows_a_notice_and_the_old_text() {
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, Instant::now());
    p.activate("symptom", t);
    p.field_text("x".repeat(400), t);
    let (seq, _) = last_call(&p.key_press("Return", no_mods(), t));
    p.reply(p.gen(), seq, Err(CallErr::Failed("The description is too long: 400 characters, at most 300".into())), t);
    assert_eq!(p.view, View::Preview);
    assert!(p.notice.as_ref().is_some_and(|(n, err)| *err && n.contains("too long")));
    assert!(p.field.text.is_empty() && p.field.applied.is_empty());
}

#[test]
fn unit_profile_takes_the_first_failed_unit_and_a_preset_searches() {
    let mut p = DiagPanel::new();
    let t = opened(&mut p, Instant::now());
    let out = p.key_press("2", no_mods(), t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Units);
    assert_eq!(p.view, View::Items);
    let out =
        p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "units": [{"unit": "backup.service", "user": false}, {"unit": "sync.service", "user": true}]})), t);
    assert_eq!(p.unit.as_deref(), Some("backup.service"));
    assert_eq!(last_call(&out).1, Call::Plan { profile: Some("unit".into()), unit: Some("backup.service".into()), container: None });
    // eine andere Unit wählen lädt den Plan neu
    let out = p.activate("unit:2", t + DEADTIME);
    assert_eq!(last_call(&out).1, Call::Plan { profile: Some("unit".into()), unit: Some("sync.service".into()), container: None });
    assert!(!p.can_collect(), "plan not loaded yet");

    // keine fehlgeschlagene Unit: nichts zu sammeln
    let mut p = DiagPanel::new();
    let t = opened(&mut p, Instant::now());
    let (seq, _) = last_call(&p.activate("profile:2", t));
    let out = p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "units": []})), t);
    assert!(calls(&out).is_empty());
    assert_eq!(p.units.as_ref().map(Vec::len), Some(0));
    assert!(!p.can_collect());

    // Vorbelegung aus der Palette: ein Treffer wird gewählt, kein Treffer wird Suchbegriff
    for (term, unit, searched) in [("BACKUP", "backup.service", false), ("nginx", "nginx", true)] {
        let mut p = DiagPanel::new();
        let t0 = Instant::now();
        let out = p.open(preset_of(None, Some(term), None), t0);
        let (seq, _) = last_call(&out);
        let out = p.reply(p.gen(), seq, Ok(&mut profiles_json()), t0);
        let (seq, call) = last_call(&out);
        assert_eq!(call, Call::Units);
        let out = p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "units": [{"unit": "backup.service", "user": false}]})), t0);
        assert_eq!(p.unit.as_deref(), Some(unit));
        assert_eq!(p.units.as_ref().unwrap()[0].searched, searched);
        assert!(matches!(last_call(&out).1, Call::Plan { unit: Some(u), .. } if u == unit));
    }
}

#[test]
fn back_from_items_forgets_the_choice() {
    let mut p = DiagPanel::new();
    let t = opened(&mut p, Instant::now());
    let (seq, _) = last_call(&p.activate("profile:1", t));
    p.reply(p.gen(), seq, Ok(&mut plan_json()), t);
    // Leertaste wählt ab, Enter sammelt mit der Abwahl
    p.key_press("space", no_mods(), t + DEADTIME);
    p.key_release("space", t + DEADTIME);
    assert_eq!(p.selected_count(), 1);
    let out = p.key_press("BackSpace", no_mods(), t + DEADTIME * 2);
    assert!(out.contains(&Effect::Render));
    assert_eq!(p.view, View::Profiles);
    assert!(p.plan.is_none() && p.profile.is_some());
}

#[test]
fn errors_have_guidance_and_a_way_back() {
    let e = diag_error(DAEMON_DOWN, Some(View::Profiles), false);
    assert_eq!(e.retry, Some("Try again"));
    let e = diag_error("Diagnosis is off (Settings › Diagnose)", Some(View::Items), false);
    assert!(e.neutral && e.back.is_none() && e.retry.is_none());
    let e = diag_error("Unknown or expired diagnosis, collect again", Some(View::Preview), true);
    assert_eq!((e.title.as_str(), e.retry, e.back, e.copy, e.expired), ("The diagnosis expired", Some("Start again"), None, false, true));
    // meldet ein Aufruf die Sitzung als unbekannt (Daemon neu gestartet), löscht das Panel Bericht und Antwort sofort
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    let out = p.key_press("c", no_mods(), t);
    let (seq, call) = last_call(&out);
    assert_eq!(call, Call::Copy { id: "7".into() });
    p.reply(p.gen(), seq, Err(CallErr::Failed("Unknown or expired diagnosis, collect again".into())), t);
    assert_eq!(p.view, View::Error);
    assert!(p.preview.is_none() && p.answer.is_none() && p.id.is_none() && p.expires_at.is_none());
    let out = p.key_press("Return", no_mods(), t + DEADTIME);
    assert_eq!(last_call(&out).1, Call::Plan { profile: None, unit: None, container: None });
    let e = diag_error("AI server changed; check the preview again", None, true);
    assert_eq!(e.back, Some(View::Preview));

    // Daemon fehlt beim Öffnen: Try again lädt die Profile neu
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let (seq, _) = last_call(&p.toggle(t0));
    let out = p.reply(p.gen(), seq, Err(CallErr::DaemonDown), t0);
    assert!(out.contains(&Effect::Show));
    assert_eq!(p.view, View::Error);
    assert_eq!(p.active.as_deref(), Some("retry"));
    let out = p.key_press("Return", no_mods(), t0 + DEADTIME);
    assert_eq!(last_call(&out).1, Call::Plan { profile: None, unit: None, container: None });
    assert_eq!(p.view, View::Loading);
}

#[test]
fn close_hangs_up_cancels_clears_and_wipes_and_late_replies_are_dropped() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let t = answered(&mut p, t0);
    let gen = p.gen();
    p.activate("check:1", t);
    let (check, _) = last_call(&p.activate("run", t));
    let out = p.close();
    assert_eq!(out[0], Effect::Hangup { seq: check });
    let c: Vec<Call> = calls(&out).into_iter().map(|(_, c)| c).collect();
    assert_eq!(c, [Call::Cancel { id: "7".into() }, Call::Clear]);
    assert_eq!(out.last(), Some(&Effect::Hide));
    assert_eq!(p.view, View::Closed);
    assert!(p.answer.is_none() && p.preview.is_none() && p.id.is_none() && p.field.text.is_empty());
    assert!(p.reply(gen, check, Ok(&mut preview_json(2, 2)), t).is_empty());
    assert!(p.close().is_empty(), "closing twice does nothing");
    // wieder öffnen: eine späte Antwort der alten Öffnung zählt nicht
    let out = p.toggle(t);
    let (seq, _) = last_call(&out);
    assert!(p.reply(gen, seq, Ok(&mut profiles_json()), t).is_empty());
    assert_eq!(p.view, View::Loading);
}

#[test]
fn the_panel_closes_when_the_daemon_session_expires_even_while_in_use() {
    let mut v = preview_json(1, 1);
    v["expires_in_s"] = json!(600);
    assert_eq!(PreviewInfo::from_json(&mut v).map(|pv| pv.expires_in), Some(Some(Duration::from_secs(600))));
    // ohne `expires_in_s` gilt die Lebensdauer des Daemons ab der Vorschau
    let t0 = Instant::now();
    let mut p = DiagPanel::new();
    let t = answered(&mut p, t0);
    let end = p.expires_at.expect("expiry");
    assert!(end <= t + SESSION_TTL && end + DEADTIME * 3 >= t + SESSION_TTL);
    // zuerst zählt der Leerlauf (früher), die Frist der Sitzung bleibt dahinter
    assert!(p.next_deadline().is_some_and(|d| d <= t + IDLE && d < end));
    // der User bleibt aktiv: der Leerlauf greift nie, die Frist der Sitzung schon
    let mut now = t;
    while now + Duration::from_secs(240) < end {
        now += Duration::from_secs(240);
        p.key_press("Down", no_mods(), now);
        p.key_release("Down", now);
        assert!(p.tick(now).is_empty());
        assert!(p.next_deadline() <= Some(end));
    }
    let out = p.tick(end);
    assert!(out.contains(&Effect::Hide));
    assert!(calls(&out).iter().any(|(_, c)| *c == Call::Clear));
    assert!(p.answer.is_none() && p.preview.is_none());
    assert_eq!(p.next_deadline(), None);
}

#[test]
fn deadtime_idle_and_show_delay() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    p.toggle(t0);
    assert_eq!(p.next_deadline(), Some(t0 + SHOW_DELAY));
    assert_eq!(p.tick(t0 + SHOW_DELAY), vec![Effect::Show, Effect::Render]);
    let mut p = DiagPanel::new();
    let t = opened(&mut p, t0);
    // Totzeit: Ziffer direkt nach dem Wechsel zählt nicht
    assert!(p.key_press("1", no_mods(), t0 + Duration::from_millis(10)).is_empty());
    assert!(p.activate("profile:1", t0 + Duration::from_millis(10)).is_empty());
    assert!(p.gate.ignored >= 2);
    p.key_release("1", t0 + Duration::from_millis(20));
    assert!(!calls(&p.key_press("1", no_mods(), t)).is_empty());
    // Leerlauf schließt, außer beim Sammeln und Senden
    assert_eq!(p.next_deadline(), Some(t + IDLE));
    assert!(p.tick(t + IDLE).contains(&Effect::Hide));
    let mut p = DiagPanel::new();
    let t = previewed(&mut p, t0);
    p.activate("send", t);
    assert_eq!(p.next_deadline(), None);
    assert!(p.tick(t + IDLE * 2).is_empty());
}

#[test]
fn values_toggle_only_with_restored_placeholders() {
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    assert!(!p.plain);
    p.key_press("p", no_mods(), t);
    assert!(p.plain);
    assert_eq!(p.items().iter().find(|i| i.id == "values").map(|i| i.label.clone()), Some("Placeholders".into()));
    // modifizierte Tasten sind keine Kürzel
    assert!(p.key_press("c", Mods { logo: true, ..Mods::default() }, t + DEADTIME).is_empty());
}

// ---- reine Funktionen ------------------------------------------------------------------------------------------------

#[test]
fn valid_spans_drop_everything_that_does_not_fit() {
    let text = "user-1 and host-2 met user-1";
    let mut list = json!([
        {"start": 0, "end": 6, "placeholder": "user-1", "value": "alice"},
        {"start": 3, "end": 9, "placeholder": "user-1", "value": "x"},        // überlappt
        {"start": 11, "end": 17, "placeholder": "host-2", "value": ""},       // ohne Wert
        {"start": 11, "end": 17, "placeholder": "host-3", "value": "box"},    // anderer Text
        {"start": 22, "end": 28, "placeholder": "user-1", "value": "alice"},
        {"start": 22, "end": 99, "placeholder": "user-1", "value": "alice"},  // außerhalb
        {"start": 0, "end": 6, "placeholder": "User-1", "value": "alice"},    // keine Form
    ]);
    let v = valid_spans(text, &mut list);
    assert_eq!(v.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), [(0, 6), (22, 28)]);
    assert_eq!(list[0]["value"], json!(null), "value taken out");
    // Schnitt mitten in einem Zeichen gilt nicht
    let mut l = json!([{"start": 1, "end": 7, "placeholder": "user-1", "value": "a"}]);
    assert!(valid_spans("äuser-1", &mut l).is_empty());
    assert!(valid_spans(text, &mut json!("no list")).is_empty());
}

#[test]
fn mark_line_respects_the_limits_and_word_boundaries() {
    let limits: HashMap<String, u64> = [("user".to_string(), 1), ("host".to_string(), 2)].into();
    let m = mark_line("user-1 at host-2 (user-1000.slice, xhost-1, user-2) [REDACTED:password] [REDACTED:]", &limits);
    let marked: Vec<(Mark, &str)> = m.into_iter().filter(|(k, _)| *k != Mark::Plain).collect();
    assert_eq!(marked, [(Mark::Placeholder, "user-1"), (Mark::Placeholder, "host-2"), (Mark::Redacted, "[REDACTED:password]")]);
    // ohne Grenzen nur Schwärzungen; der Text bleibt vollständig
    let line = "user-1 [REDACTED:api_key] ä";
    let m = mark_line(line, &HashMap::new());
    assert_eq!(m.iter().map(|(_, t)| *t).collect::<String>(), line);
    assert_eq!(m.iter().filter(|(k, _)| *k == Mark::Redacted).count(), 1);
    assert!(mark_line("", &limits).is_empty());
}

#[test]
fn split_report_knows_two_line_headers_only_from_the_daemon() {
    let mut v = preview_json(1, 1);
    let pv = PreviewInfo::from_json(&mut v).unwrap();
    let (pre, secs) = split_report(&pv.text, &pv.sections);
    assert_eq!(pre.len(), 2);
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[0].head.len(), 1);
    assert_eq!(secs[0].body, ["1| Linux host-1 7.3", "2| user-1000.slice ok"]);
    assert_eq!(secs[1].head, ["=== S2 · journal · 3 lines", "resolved: unit user-1.service"]);
    assert_eq!(secs[1].body.len(), 3);
    // ein Abschnitt ohne `resolved` im Kopf: die Zeile ist Inhalt
    let sections = [SectionInfo { id: "S1".into(), header: "=== S1 · x".into(), ..SectionInfo::default() }];
    let (_, secs) = split_report("=== S1 · x\nresolved: fake\n1| a", &sections);
    assert_eq!(secs[0].head.len(), 1);
    assert_eq!(secs[0].body[0], "resolved: fake");
    assert_eq!(line_number("12| text"), Some((12, "text")));
    assert_eq!(line_number("no number"), None);
    assert_eq!(section_start("=== S12 · x"), Some("S12"));
    assert_eq!(section_start("=== Sx · x"), None);
    assert_eq!(section_start(" === S1 · x"), None);
}

#[test]
fn limits_add_sections_and_the_symptom() {
    let mut v = preview_json(1, 1);
    v["symptom"] = json!({"text": "host-1 is slow", "placeholders": {"host": 1}});
    let pv = PreviewInfo::from_json(&mut v).unwrap();
    let l = limits(&pv);
    assert_eq!((l.get("host"), l.get("user")), (Some(&2), Some(&1)));
}

#[test]
fn unit_matches_and_presets() {
    let units: Vec<UnitInfo> =
        ["backup.service", "backup-check.timer", "sync.service"].iter().map(|u| UnitInfo { unit: u.to_string(), user: false, searched: false }).collect();
    assert_eq!(unit_matches(&units, "Backup"), ["backup.service"], "exact with .service");
    assert_eq!(unit_matches(&units, "backup-check.timer"), ["backup-check.timer"]);
    assert_eq!(unit_matches(&units, "back"), ["backup.service", "backup-check.timer"]);
    assert!(unit_matches(&units, "nginx").is_empty());
    assert_eq!(preset_of(None, Some("x"), None), Some(Preset { profile: "unit".into(), unit: Some("x".into()), container: None }));
    assert_eq!(preset_of(None, None, Some("db")), Some(Preset { profile: "docker".into(), unit: None, container: Some("db".into()) }));
    assert_eq!(preset_of(None, Some("x"), Some("db")), None, "both: none");
    assert_eq!(preset_of(Some("now"), Some("x"), None), Some(Preset { profile: "now".into(), unit: None, container: None }));
    assert_eq!(preset_of(Some("  "), None, None), None);
    assert_eq!(params_of(&json!({"probe": "j", "unit": "a", "lines": 5, "x": null, "y": [1]})), "lines=5 unit=a");
}

#[test]
fn parse_answer_takes_texts_and_keeps_only_known_sections() {
    let known: HashSet<String> = ["S1".to_string()].into();
    let mut a = json!({"findings": [{"cause": "c", "evidence": [{"section": "S1", "from": 0, "to": -3}, {"section": "S2", "from": 1}]}, {"no": "cause"}],
        "checks": [{"probe": {"nope": 1}}, {"probe": {"probe": "mem"}, "label": "Memory"}], "notes": 5});
    let ans = parse_answer(&mut a, &mut json!({}), &known).unwrap();
    assert_eq!(ans.findings.len(), 1);
    assert_eq!(ans.findings[0].evidence, [Evidence { section: "S1".into(), from: 1, to: 1 }]);
    assert_eq!(ans.checks.len(), 1);
    assert_eq!(ans.checks[0].label, "Memory");
    assert_eq!(ans.notes, "");
    assert!(parse_answer(&mut json!("x"), &mut json!({}), &known).is_none());
    // Spannen in falscher Form brechen nichts
    let mut a2 = json!({"findings": [{"cause": "user-1"}], "notes": "user-1"});
    let ans = parse_answer(&mut a2, &mut json!("bad"), &known).unwrap();
    assert!(!ans.has_values());
    let mut a3 = json!({"findings": [{"cause": "user-1"}], "notes": "user-1"});
    assert!(!parse_answer(&mut a3, &mut json!({"findings": "x", "notes": 3}), &known).unwrap().has_values());
    assert_eq!(Evidence { section: "S1".into(), from: 2, to: 4 }.text(), "S1 · lines 2–4");
    assert_eq!(Evidence { section: "S1".into(), from: 2, to: 2 }.text(), "S1 · line 2");
}

#[test]
fn call_names_timeouts_and_requests() {
    assert_eq!(Call::Collect { profile: "now".into(), unit: None, container: None, skip: vec![] }.timeout(), COLLECT_TIMEOUT);
    assert_eq!(Call::Send { id: "1".into(), preview: 1 }.timeout(), SEND_TIMEOUT);
    assert_eq!(Call::Clear.timeout(), CALL_TIMEOUT);
    assert_eq!(Call::Plan { profile: Some("now".into()), unit: None, container: None }.name(), "plan_profile");
    let r = Call::Preview { id: "3".into(), deselected: None, symptom: Some(SecretText::new("Wifi".into())) }.request();
    assert_eq!(r.to_line(), r#"{"cmd":"diag_preview","id":"3","symptom":"Wifi"}"#);
    assert!(!format!("{r:?}").contains("Wifi"), "symptom never in Debug");
    assert_eq!(CallErr::from_reply(&json!({"ok": false, "cancelled": true})), CallErr::Cancelled);
    assert_eq!(CallErr::Failed("unit foo".into()).kind(), "refused");
}

#[test]
fn odd_replies_never_panic_and_change_nothing_unexpected() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let (seq, _) = last_call(&p.toggle(t0));
    p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "profiles": [1, "x", {"name": 5}, {"name": "now"}]})), t0);
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].label, "now");
    let (seq, _) = last_call(&p.activate("profile:1", t0 + DEADTIME));
    p.reply(p.gen(), seq, Ok(&mut json!({"ok": true, "items": [1, {"index": "x"}, {"index": 4, "probe": 3}]})), t0);
    assert_eq!(p.plan.as_ref().map(|v| v.iter().map(|i| (i.index, i.probe.is_none())).collect::<Vec<_>>()), Some(vec![(4, true)]));
    p.plan.as_mut().unwrap()[0].selected = true;
    let (seq, _) = last_call(&p.activate("collect", t0 + DEADTIME * 2));
    p.progress_line(p.gen(), seq, &json!({"progress": "x"}));
    p.progress_line(p.gen(), seq, &json!({"progress": {"phase": "end"}}));
    p.progress_line(p.gen(), seq + 1, &json!({"progress": {"phase": "start", "index": 1}}));
    assert_eq!(p.progress.as_ref().unwrap().rows.len(), 0);
    let mut v = json!({"ok": true, "text": "t", "sections": [1, {"id": 2}, {"id": "S1", "placeholders": [1]}], "target": 5, "symptom": "x"});
    p.reply(p.gen(), seq, Ok(&mut v), t0);
    assert_eq!(p.view, View::Preview);
    let pv = p.preview.as_ref().unwrap();
    assert_eq!((pv.sections.len(), pv.symptom.is_none(), pv.target.ready), (1, true, true));
    // eine Antwort ohne Text ist ein Fehler, kein Absturz
    let mut p2 = DiagPanel::new();
    let t = opened(&mut p2, t0);
    let (seq, _) = last_call(&p2.activate("profile:1", t));
    p2.reply(p2.gen(), seq, Ok(&mut json!({"ok": true, "items": "no"})), t);
    assert_eq!(p2.view, View::Error);
}

#[test]
fn debug_output_never_shows_contents() {
    let mut p = DiagPanel::new();
    let t = answered(&mut p, Instant::now());
    p.activate("symptom", t);
    p.field_text("markersymptom".into(), t);
    let dump = format!(
        "{p:?} {:?} {:?} {:?} {:?} {:?}",
        p.preview,
        p.answer,
        p.field,
        CallErr::Failed("markerunit".into()),
        Call::Collect { profile: "now".into(), unit: Some("markerunit".into()), container: None, skip: vec![] }
    );
    for marker in ["markersymptom", "markerunit", "alice", "user-1", "Linux", "disk"] {
        assert!(!dump.contains(marker), "{marker} in {dump}");
    }
}

/// Block B Punkt 0: Back aus den Einträgen und Schließen legen auch ein laufendes `diag_units` auf (die Aufrufe laufen der
/// Reihe nach; es hielte sonst `diag_cancel` und `diag_clear` bis zu 40 s auf)
#[test]
fn back_and_close_hang_up_a_running_unit_search() {
    let mut p = DiagPanel::new();
    let t = opened(&mut p, Instant::now());
    let out = p.activate("profile:2", t);
    let (units, call) = last_call(&out);
    assert_eq!(call, Call::Units);
    assert_eq!(p.view, View::Items);
    let out = p.key_press("BackSpace", no_mods(), t + DEADTIME);
    assert_eq!(out.first(), Some(&Effect::Hangup { seq: units }), "{out:?}");
    assert_eq!(p.view, View::Profiles);
    assert_eq!(p.calls_pending(), 0);
    // eine späte Antwort zählt nicht
    assert!(p.reply(p.gen(), units, Ok(&mut json!({"ok": true, "units": [{"unit": "cups.service", "user": false}]})), t).is_empty());
    assert_eq!(p.view, View::Profiles);
    // Schließen während der Suche: auflegen, dann erst `diag_clear` (in dieser Reihenfolge an den Thread)
    let t2 = t + DEADTIME * 3;
    let out = p.activate("profile:2", t2);
    let (units, _) = last_call(&out);
    let out = p.close();
    assert_eq!(out.first(), Some(&Effect::Hangup { seq: units }), "{out:?}");
    assert_eq!(calls(&out).last().map(|c| c.1.clone()), Some(Call::Clear));
    assert_eq!(p.stats.hangups, 2);
    assert!(!p.is_open());
}

#[test]
fn from_the_palette_chosen_comes_once_with_the_profiles() {
    let mut p = DiagPanel::new();
    let t0 = Instant::now();
    let preset = preset_of(Some("now"), None, None);
    let out = p.open_from_palette(preset, t0);
    assert!(!out.contains(&Effect::Chosen), "not before the daemon answered");
    let (seq, _) = last_call(&out);
    let out = p.reply(p.gen(), seq, Ok(&mut profiles_json()), t0);
    assert_eq!(out.iter().filter(|e| **e == Effect::Chosen).count(), 1, "{out:?}");
    assert_eq!(p.profile.as_deref(), Some("now"), "the preset picks the profile");
    // die Diagnose ist aus (Fehler statt Profile): keine Meldung
    let mut p = DiagPanel::new();
    let (seq, _) = last_call(&p.open_from_palette(None, t0));
    let out = p.reply(p.gen(), seq, Err(CallErr::Failed("Diagnosis is off".into())), t0);
    assert!(!out.contains(&Effect::Chosen));
    // Mod+Shift+I meldet nie etwas an die Palette, auch nicht nach einer Öffnung aus ihr
    let mut p = DiagPanel::new();
    p.open_from_palette(None, t0);
    p.close();
    let (seq, _) = last_call(&p.toggle(t0));
    assert!(!p.reply(p.gen(), seq, Ok(&mut profiles_json()), t0).contains(&Effect::Chosen));
}
