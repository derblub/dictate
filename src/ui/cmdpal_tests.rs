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

//! Tests des Automaten der Befehlspalette mit Antworten in der Form des Daemons (`src/palette/service.rs`); Fälle aus
//! der Lua-Vorlage (`c552237:docs/w5-vorlagen/cmdpal-spec.md`, Zeilen `pal:`).

use super::*;
use crate::ui::gate::{DEADTIME, IDLE};
use serde_json::json;
use std::time::Duration;

fn no_mods() -> Mods {
    Mods::default()
}

fn ctrl() -> Mods {
    Mods { ctrl: true, ..Mods::default() }
}

fn window(id: u64, display: &str) -> Value {
    json!({"intent": {"intent": "focus_window", "query": "x"}, "target": {"kind": "window", "id": id}, "display": display,
           "detail": "app", "score": 1.0, "class": "run", "target_id": format!("w{id}")})
}

fn reboot() -> Value {
    json!({"intent": {"intent": "reboot"}, "target": {"kind": "none"}, "display": "Reboot", "detail": null, "score": 1.0,
           "class": "confirm", "target_id": "reboot"})
}

fn answer(cands: Vec<Value>, confident: bool) -> Value {
    json!({"ok": true, "candidates": cands, "confident": confident, "web_search_url": "https://search.example/?q={query}"})
}

/// Lokale Anfragen einer Folge: (seq, Eingabe)
fn resolves(out: &[Effect]) -> Vec<(u64, String)> {
    out.iter()
        .filter_map(|e| match e {
            Effect::Resolve { seq, input, .. } => Some((*seq, input.clone())),
            _ => None,
        })
        .collect()
}

fn asks(out: &[Effect]) -> Vec<(u64, String)> {
    out.iter()
        .filter_map(|e| match e {
            Effect::Ask { seq, input, .. } => Some((*seq, input.clone())),
            _ => None,
        })
        .collect()
}

fn executed(out: &[Effect]) -> Option<(String, bool)> {
    out.iter().find_map(|e| match e {
        Effect::Execute { cand, confirmed, .. } => Some((cand.display.clone(), *confirmed)),
        _ => None,
    })
}

fn last_seq(out: &[Effect]) -> u64 {
    resolves(out).last().unwrap_or_else(|| panic!("no resolve in {out:?}")).0
}

/// Offen, nach der Totzeit, mit der Antwort auf die leere Eingabe
struct T {
    p: CmdPal,
    now: Instant,
}

impl T {
    fn open() -> T {
        let now = Instant::now();
        let mut p = CmdPal::new();
        let out = p.open(now);
        assert_eq!(&out[..2], &[Effect::Show, Effect::Render]);
        assert_eq!(resolves(&out), vec![(1, String::new())]);
        let mut t = T { p, now };
        let g = t.p.gen();
        t.p.resolve_reply(g, 1, Ok(&answer(vec![window(1, "Firefox")], false)), t.now);
        t.later(DEADTIME);
        t
    }

    /// Wieder öffnen (nach dem Ausführen), die leere Eingabe beantworten, Totzeit abwarten
    fn reopen(&mut self) {
        let seq = last_seq(&self.p.open(self.now));
        self.reply(seq, answer(vec![], true));
        self.later(DEADTIME);
    }

    fn later(&mut self, d: Duration) {
        self.now += d;
    }

    fn type_text(&mut self, s: &str) -> Vec<Effect> {
        self.p.text_changed(s, self.now)
    }

    fn reply(&mut self, seq: u64, v: Value) -> Vec<Effect> {
        let g = self.p.gen();
        self.p.resolve_reply(g, seq, Ok(&v), self.now)
    }

    fn key(&mut self, k: &str) -> Vec<Effect> {
        let out = self.p.key_press(k, no_mods(), self.now);
        self.p.key_release(k, self.now + Duration::from_millis(30));
        self.later(Duration::from_millis(120));
        out
    }

    fn ctrl_key(&mut self, k: &str) -> Vec<Effect> {
        let out = self.p.key_press(k, ctrl(), self.now);
        self.p.key_release(k, self.now + Duration::from_millis(30));
        self.later(Duration::from_millis(120));
        out
    }

    /// Eingabe tippen und die Antwort darauf geben
    fn resolved(&mut self, s: &str, v: Value) -> Vec<Effect> {
        let out = self.type_text(s);
        let seq = last_seq(&out);
        self.reply(seq, v)
    }
}

#[test]
fn open_asks_for_the_empty_input_and_shows_the_answer() {
    let t = T::open();
    assert!(t.p.is_open());
    assert_eq!(t.p.cands.len(), 1);
    assert_eq!(t.p.sel, Some(0));
    assert_eq!(t.p.shown_for.as_deref(), Some(""));
    // ohne Eingabe: kein Fragen der KI
    assert!(!t.p.enter_asks());
}

#[test]
fn one_request_in_flight_plus_the_newest_waiting() {
    let mut t = T::open();
    let a = t.type_text("f");
    assert_eq!(resolves(&a), vec![(2, "f".into())]);
    assert!(resolves(&t.type_text("fi")).is_empty());
    assert!(resolves(&t.type_text("fir")).is_empty());
    assert_eq!(t.p.counters.skipped, 1, "fi was replaced by fir while waiting");
    // die Antwort auf „f“ ist veraltet: verworfen, die wartende geht los
    let out = t.reply(2, answer(vec![window(2, "Old")], true));
    assert_eq!(resolves(&out), vec![(4, "fir".into())]);
    assert_eq!(t.p.counters.discarded, 1);
    assert_eq!(t.p.cands[0].display, "Firefox", "the stale answer changed nothing");
    t.reply(4, answer(vec![window(3, "Firefox Nightly")], true));
    assert_eq!(t.p.cands[0].display, "Firefox Nightly");
    assert_eq!(t.p.shown_for.as_deref(), Some("fir"));
    // eine Antwort, die nicht unterwegs ist, zählt nicht
    assert!(t.reply(4, answer(vec![], true)).is_empty());
}

#[test]
fn answers_of_an_older_opening_are_dropped() {
    let mut t = T::open();
    let out = t.type_text("x");
    let (seq, old) = (last_seq(&out), t.p.gen());
    let closed = t.p.close();
    assert!(closed.contains(&Effect::Hangup { lane: Lane::Resolve, seq }), "closing hangs up the running request: {closed:?}");
    assert_eq!(closed.last(), Some(&Effect::Hide));
    assert!(t.p.input.is_empty());
    t.p.open(t.now);
    assert!(t.p.resolve_reply(old, seq, Ok(&answer(vec![window(9, "Late")], true)), t.now).is_empty());
    assert!(t.p.cands.is_empty());
}

#[test]
fn at_most_twelve_valid_candidates() {
    let mut t = T::open();
    let mut list: Vec<Value> = (1..=20).map(|i| window(i, &format!("W{i}"))).collect();
    list.insert(0, json!({"intent": {"intent": "sh"}, "target": {"kind": "none"}, "display": "bad", "class": "run", "target_id": "x"}));
    t.resolved("w", answer(list, true));
    assert_eq!(t.p.cands.len(), MAX_CANDIDATES);
    assert_eq!(t.p.cands[0].display, "W1");
    assert_eq!(t.p.counters.invalid, 1);
}

#[test]
fn enter_asks_the_ai_unless_the_list_is_confident_or_picked() {
    let mut t = T::open();
    t.resolved("brwoser", answer(vec![window(1, "Firefox")], false));
    assert!(t.p.enter_asks());
    let out = t.key("Return");
    assert_eq!(asks(&out), vec![(1, "brwoser".into())]);
    assert!(executed(&out).is_none());
    // sicherer Treffer: Return führt aus
    let mut t = T::open();
    t.resolved("firefox", answer(vec![window(1, "Firefox"), window(2, "Other")], true));
    assert!(!t.p.enter_asks());
    let out = t.key("Return");
    assert_eq!(executed(&out), Some(("Firefox".into(), false)));
    assert!(!t.p.is_open(), "executing closes first");
    let hide = out.iter().position(|e| *e == Effect::Hide).unwrap();
    let exec = out.iter().position(|e| matches!(e, Effect::Execute { .. })).unwrap();
    assert!(hide < exec);
    // selbst gewählt (Pfeil): Return führt aus, auch ohne sicheren Treffer
    let mut t = T::open();
    t.resolved("fire", answer(vec![window(1, "Firefox"), window(2, "Firewall")], false));
    t.key("Down");
    assert_eq!(t.p.sel, Some(1));
    assert_eq!(executed(&t.key("Return")), Some(("Firewall".into(), false)));
    // keine Kandidaten: Return fragt die KI
    let mut t = T::open();
    t.resolved("something odd", answer(vec![], true));
    assert_eq!(asks(&t.key("Return")).len(), 1);
}

#[test]
fn return_before_the_answer_waits_for_it() {
    let mut t = T::open();
    let out = t.type_text("firefox");
    let seq = last_seq(&out);
    assert!(t.key("Return").is_empty(), "nothing yet: the choice waits");
    let out = t.reply(seq, answer(vec![window(1, "Firefox")], true));
    assert_eq!(executed(&out), Some(("Firefox".into(), false)));
    // dasselbe ohne sicheren Treffer: die KI wird gefragt
    let mut t = T::open();
    let seq = last_seq(&t.type_text("brwsr"));
    t.key("Return");
    let out = t.reply(seq, answer(vec![window(1, "Firefox")], false));
    assert_eq!(asks(&out).len(), 1);
    // Tippen danach vergisst die Wahl
    let mut t = T::open();
    let seq = last_seq(&t.type_text("fi"));
    t.key("Return");
    t.type_text("fir");
    let out = t.reply(seq, answer(vec![window(1, "Firefox")], true));
    assert!(executed(&out).is_none());
}

#[test]
fn confirm_needs_a_second_return_and_esc_or_typing_cancel_it() {
    let mut t = T::open();
    t.resolved("reboot", answer(vec![reboot()], true));
    let out = t.key("Return");
    assert_eq!(out, vec![Effect::Render]);
    assert_eq!(t.p.confirm, Some(0));
    // innerhalb der Totzeit: nichts
    assert!(t.key("Return").is_empty());
    t.later(DEADTIME);
    let out = t.key("Return");
    assert_eq!(executed(&out), Some(("Reboot".into(), true)));
    // Esc hebt auf, ein weiteres Esc schließt
    let mut t = T::open();
    t.resolved("reboot", answer(vec![reboot(), window(1, "Firefox")], true));
    t.key("Return");
    assert_eq!(t.key("Escape"), vec![Effect::Render]);
    assert_eq!(t.p.confirm, None);
    assert!(t.p.is_open());
    // Tippen hebt auf
    t.later(DEADTIME);
    t.key("Return");
    assert_eq!(t.p.confirm, Some(0));
    let out = t.type_text("reboo");
    assert!(out.contains(&Effect::Render));
    assert_eq!(t.p.confirm, None);
    // eine andere Zeile hebt auf
    let mut t = T::open();
    t.resolved("reboot", answer(vec![reboot(), window(1, "Firefox")], true));
    t.key("Return");
    t.key("Down");
    assert_eq!((t.p.confirm, t.p.sel), (None, Some(1)));
    // Knopf „Confirm“ (nach der Totzeit) führt bestätigt aus
    let mut t = T::open();
    t.resolved("reboot", answer(vec![reboot()], true));
    t.key("Return");
    t.later(DEADTIME);
    assert_eq!(executed(&t.p.activate("confirm", t.now)), Some(("Reboot".into(), true)));
}

#[test]
fn a_run_candidate_whose_intent_needs_a_confirmation_asks_too() {
    let mut t = T::open();
    let mut c = reboot();
    c["class"] = json!("run");
    t.resolved("reboot", answer(vec![c], true));
    assert_eq!(t.key("Return"), vec![Effect::Render]);
    assert_eq!(t.p.confirm, Some(0));
}

#[test]
fn deadtime_after_opening_blocks_return_and_clicks_not_typing() {
    let now = Instant::now();
    let mut p = CmdPal::new();
    p.open(now);
    let g = p.gen();
    p.resolve_reply(g, 1, Ok(&answer(vec![window(1, "Firefox")], true)), now);
    assert!(p.key_press("Return", no_mods(), now + Duration::from_millis(50)).is_empty());
    assert!(p.activate("row:0", now + Duration::from_millis(60)).is_empty());
    assert!(!p.text_changed("f", now + Duration::from_millis(70)).is_empty(), "typing is never blocked");
}

#[test]
fn the_ai_preview_stays_until_the_input_changes() {
    let mut t = T::open();
    t.resolved("open my mail", answer(vec![], true));
    let out = t.key("Return");
    let (aseq, _) = asks(&out)[0];
    assert_eq!(t.p.footer(), vec![("Esc", "Cancel")]);
    // eine lokale Antwort während der KI ändert die Liste, die KI läuft weiter
    let ai = json!({"ok": true, "intent": {"intent": "launch_app"}, "candidates": [window(5, "Thunderbird")], "web_search_url": "https://s/?q={query}"});
    let g = t.p.gen();
    assert_eq!(t.p.ask_reply(g, aseq, Ok(&ai), t.now), vec![Effect::Render]);
    assert_eq!(t.p.preview.as_deref(), Some("open my mail"));
    assert_eq!(t.p.cands[0].display, "Thunderbird");
    // Return gleich nach der Vorschau: Totzeit
    assert!(t.key("Return").is_empty());
    t.later(DEADTIME);
    assert_eq!(executed(&t.key("Return")), Some(("Thunderbird".into(), false)));
    // Tippen verwirft die Vorschau und fragt wieder lokal
    let mut t = T::open();
    t.resolved("open my mail", answer(vec![], true));
    let aseq = asks(&t.key("Return"))[0].0;
    let g = t.p.gen();
    t.p.ask_reply(g, aseq, Ok(&ai), t.now);
    let out = t.type_text("open my mails");
    assert!(t.p.preview.is_none());
    assert!(out.contains(&Effect::Render));
    assert_eq!(resolves(&out).len(), 1);
}

#[test]
fn a_late_local_answer_does_not_replace_the_preview() {
    let mut t = T::open();
    let seq = last_seq(&t.type_text("mail"));
    let aseq = asks(&t.ctrl_key("Return"))[0].0;
    let g = t.p.gen();
    t.p.ask_reply(g, aseq, Ok(&json!({"ok": true, "candidates": [window(5, "Thunderbird")]})), t.now);
    assert!(t.reply(seq, answer(vec![window(1, "Local")], true)).is_empty());
    assert_eq!(t.p.cands[0].display, "Thunderbird");
}

#[test]
fn typing_and_esc_hang_up_the_ai() {
    let mut t = T::open();
    t.resolved("x y", answer(vec![], true));
    let aseq = asks(&t.key("Return"))[0].0;
    let out = t.type_text("x y z");
    assert!(out.contains(&Effect::Hangup { lane: Lane::Ask, seq: aseq }));
    assert!(t.p.asking.is_none());
    // die späte Antwort zählt nicht
    let g = t.p.gen();
    assert!(t.p.ask_reply(g, aseq, Ok(&json!({"ok": true, "candidates": [window(5, "T")]})), t.now).is_empty());
    // Esc legt auf, ein zweites schließt
    let aseq = asks(&t.ctrl_key("Return"))[0].0;
    let out = t.key("Escape");
    assert!(out.contains(&Effect::Hangup { lane: Lane::Ask, seq: aseq }));
    assert!(t.p.is_open());
    assert_eq!(t.key("Escape").last(), Some(&Effect::Hide));
}

#[test]
fn return_while_asking_waits_and_ctrl_return_always_asks() {
    let mut t = T::open();
    t.resolved("firefox", answer(vec![window(1, "Firefox")], true));
    let out = t.ctrl_key("Return");
    assert_eq!(asks(&out).len(), 1, "Ctrl+Return asks even with a confident list");
    assert!(t.key("Return").is_empty());
    // eine zweite Frage ersetzt die erste
    let first = t.p.asking.as_ref().unwrap().seq;
    let out = t.ctrl_key("Return");
    assert!(out.contains(&Effect::Hangup { lane: Lane::Ask, seq: first }));
    assert_eq!(asks(&out).len(), 1);
}

#[test]
fn errors_of_the_ai_and_of_the_daemon() {
    let mut t = T::open();
    t.resolved("q", answer(vec![window(1, "Firefox")], false));
    let ask = |t: &mut T, res: Result<&Value, CallErr>| {
        let aseq = asks(&t.ctrl_key("Return"))[0].0;
        let g = t.p.gen();
        t.p.ask_reply(g, aseq, res, t.now);
        t.p.ask_err.clone().unwrap()
    };
    let e = ask(&mut t, Ok(&json!({"ok": true, "candidates": []})));
    assert_eq!(e.title, "The AI found no matching command");
    assert_eq!(t.p.cands.len(), 1, "the local list stays");
    let e = ask(&mut t, Err(CallErr::Timeout));
    assert_eq!((e.title.as_str(), e.body.as_deref()), ("No answer from the AI", Some(RETRY_AI)));
    let e = ask(&mut t, Err(CallErr::Failed("The command palette is off (Settings › Palette)".into())));
    assert!(e.neutral && e.body.is_none());
    let e = ask(&mut t, Err(CallErr::Failed("AI error: HTTP 401 (check API key)".into())));
    assert!(!e.neutral);
    let e = ask(&mut t, Err(CallErr::DaemonDown));
    assert_eq!(e.title, DAEMON_DOWN);
    // nach einem Fehler der KI fragt Return wieder (der Fehler steht nicht in `enter_asks`)
    assert!(t.p.enter_asks());
    // lokal: Fehler ersetzt die Liste, „aus“ ist neutral
    let mut t = T::open();
    let seq = last_seq(&t.type_text("x"));
    let g = t.p.gen();
    t.p.resolve_reply(g, seq, Err(CallErr::Failed("The command palette is off (Settings › Palette)".into())), t.now);
    let e = t.p.err.clone().unwrap();
    assert!(e.neutral);
    assert!(t.p.cands.is_empty());
    assert!(!t.p.enter_asks(), "no AI while an error stands");
    let seq = last_seq(&t.type_text("xy"));
    t.p.resolve_reply(g, seq, Err(CallErr::Timeout), t.now);
    assert_eq!(t.p.err.as_ref().unwrap().title, "No answer from dictate");
    let seq = last_seq(&t.type_text("xyz"));
    t.p.resolve_reply(g, seq, Ok(&json!({"ok": true})), t.now);
    assert_eq!(t.p.err.as_ref().unwrap().title, "dictate sent an invalid answer");
}

#[test]
fn history_recalls_chosen_inputs_and_keeps_the_draft() {
    let mut t = T::open();
    for name in ["alpha", "beta"] {
        t.resolved(name, answer(vec![window(1, name)], true));
        t.later(DEADTIME);
        assert!(executed(&t.key("Return")).is_some());
        t.reopen();
    }
    assert_eq!(t.p.history_len(), 2);
    t.resolved("dr", answer(vec![], true));
    // ↑ bei leerer Liste: Verlauf, neuester zuerst
    let out = t.key("Up");
    assert!(out.contains(&Effect::SetInput("beta".into())));
    assert_eq!(resolves(&out), vec![(t.p.seq, "beta".into())]);
    assert!(t.p.key_press("Up", no_mods(), t.now).contains(&Effect::SetInput("alpha".into())));
    t.later(Duration::from_millis(200));
    // ältester bleibt
    assert!(!t.key("Up").iter().any(|e| matches!(e, Effect::SetInput(_))));
    assert!(t.key("Down").contains(&Effect::SetInput("beta".into())));
    // über das neueste hinaus: der Entwurf kommt zurück
    assert!(t.key("Down").contains(&Effect::SetInput("dr".into())));
    // wiederholte Eingaben stehen nur einmal, höchstens 50
    for i in 0..60 {
        t.p.remember(format!("e{i}"));
    }
    t.p.remember("e59".into());
    assert_eq!(t.p.history_len(), HISTORY_MAX);
    assert_eq!(t.p.history.last().map(String::as_str), Some("e59"));
    assert_eq!(t.p.history[0], "e10");
    t.p.remember("   ".into());
    assert_eq!(t.p.history_len(), HISTORY_MAX);
}

#[test]
fn history_forgets_a_waiting_choice() {
    let mut t = T::open();
    t.resolved("alpha", answer(vec![window(1, "alpha")], true));
    t.later(DEADTIME);
    t.key("Return");
    t.reopen();
    let seq = last_seq(&t.type_text("b"));
    t.key("Return");
    // ↑ mit leerer Liste: Verlauf; die wartende Wahl gilt nicht für „alpha“ (L20)
    t.key("Up");
    assert_eq!(t.p.input, "alpha");
    t.reply(seq, answer(vec![], true));
    let s2 = t.p.seq;
    assert!(executed(&t.reply(s2, answer(vec![window(1, "alpha")], true))).is_none());
}

#[test]
fn up_and_down_move_the_selection_and_tab_wraps() {
    let mut t = T::open();
    t.resolved("w", answer(vec![window(1, "A"), window(2, "B"), window(3, "C")], false));
    assert_eq!(t.key("Down"), vec![Effect::Highlight]);
    assert_eq!(t.p.sel, Some(1));
    t.key("Up");
    assert_eq!(t.p.sel, Some(0));
    t.key("ISO_Left_Tab");
    assert_eq!(t.p.sel, Some(2), "shift+tab wraps to the end");
    t.key("Tab");
    assert_eq!(t.p.sel, Some(0));
    // Hover hebt hervor, setzt aber kein „picked“
    let mut t = T::open();
    t.resolved("w", answer(vec![window(1, "A"), window(2, "B")], false));
    assert_eq!(t.p.hover(Some("row:1")), vec![Effect::Highlight]);
    assert!(t.p.enter_asks());
}

#[test]
fn a_click_on_a_row_chooses_it() {
    let mut t = T::open();
    t.resolved("w", answer(vec![window(1, "A"), window(2, "B")], false));
    assert_eq!(executed(&t.p.activate("row:1", t.now)), Some(("B".into(), false)));
    let mut t = T::open();
    t.resolved("w", answer(vec![reboot()], false));
    assert_eq!(t.p.activate("row:0", t.now), vec![Effect::Render]);
    assert_eq!(t.p.confirm, Some(0));
    // ein zweiter Klick auf dieselbe Zeile (auch nach der Totzeit) bestätigt nicht, nur „Confirm“ (Entscheidung 18)
    t.later(DEADTIME);
    assert!(executed(&t.p.activate("row:0", t.now)).is_none());
    assert_eq!(t.p.confirm, Some(0), "the confirmation stays open");
    t.later(DEADTIME);
    assert_eq!(executed(&t.p.activate("confirm", t.now)), Some(("Reboot".into(), true)));
    let mut t = T::open();
    t.resolved("w", answer(vec![reboot()], false));
    t.p.activate("row:0", t.now);
    assert_eq!(t.p.activate("close", t.now).last(), Some(&Effect::Hide));
}

#[test]
fn voice_replaces_the_input_and_never_runs() {
    let mut t = T::open();
    t.resolved("fire", answer(vec![window(1, "Firefox")], true));
    let out = t.p.voice_state(Some(VoicePhase::Listening));
    assert_eq!(out, vec![Effect::Render]);
    // während der Aufnahme bewirkt nur Esc etwas
    assert!(t.key("Return").is_empty());
    assert!(t.key("Down").is_empty());
    assert_eq!(t.p.voice_partial("öffne\tden", "browser\n"), vec![Effect::VoiceText]);
    assert_eq!(t.p.voice.as_ref().unwrap().tail, "browser");
    t.p.voice_state(Some(VoicePhase::Finalizing));
    let out = t.p.voice_final("  Öffne   den\nBrowser. ", t.now);
    assert!(out.contains(&Effect::SetInput("Öffne den Browser.".into())), "{out:?}");
    assert_eq!(resolves(&out).len(), 1);
    assert!(executed(&out).is_none() && asks(&out).is_empty());
    assert_eq!(t.p.input, "Öffne den Browser.");
    assert!(t.p.voice.is_none());
    assert_eq!(t.p.counters.voice_inputs, 1);
    // Esc bricht die Aufnahme ab, die Eingabe bleibt
    t.p.voice_state(Some(VoicePhase::Listening));
    assert_eq!(t.key("Escape"), vec![Effect::VoiceCancel, Effect::Render]);
    assert!(t.p.is_open());
    assert_eq!(t.p.input, "Öffne den Browser.");
    // Schließen während der Aufnahme bricht sie ab; ein spätes final ändert nichts
    t.p.voice_state(Some(VoicePhase::Listening));
    assert!(t.p.close().contains(&Effect::VoiceCancel));
    assert!(t.p.voice_final("spät", t.now).is_empty());
    // Ende ohne final: die Liste kehrt zurück
    let mut t = T::open();
    t.p.voice_state(Some(VoicePhase::Loading));
    assert_eq!(t.p.voice_state(None), vec![Effect::Render]);
    assert!(t.p.voice.is_none());
}

#[test]
fn a_fresh_recording_hangs_up_the_ai() {
    let mut t = T::open();
    t.resolved("q", answer(vec![], true));
    let aseq = asks(&t.key("Return"))[0].0;
    let out = t.p.voice_state(Some(VoicePhase::Loading));
    assert!(out.contains(&Effect::Hangup { lane: Lane::Ask, seq: aseq }));
}

#[test]
fn idle_closes_after_five_minutes() {
    let mut t = T::open();
    let at = t.p.next_deadline().unwrap();
    assert!(at > t.now);
    assert!(t.p.tick(t.now).is_empty());
    let out = t.p.tick(t.now + IDLE);
    assert_eq!(out.last(), Some(&Effect::Hide));
    assert!(t.p.next_deadline().is_none());
}

#[test]
fn footer_names_the_next_step() {
    let mut t = T::open();
    assert_eq!(t.p.footer(), vec![("Enter", "Run"), ("Mod+E", "Hold to speak"), ("Esc", "Close")]);
    t.resolved("re", answer(vec![reboot(), window(1, "A")], true));
    assert_eq!(t.p.footer(), vec![("↑↓", "Select"), ("Enter", "Run…"), ("Ctrl+Enter", "Ask AI"), ("Mod+E", "Hold to speak"), ("Esc", "Close")]);
    t.resolved("rx", answer(vec![window(1, "A")], false));
    assert_eq!(t.p.footer()[0], ("Enter", "Ask AI"));
}

#[test]
fn one_space_reads_like_lua() {
    assert_eq!(one_space("  a \t b\r\n c  ", 100), "a b c");
    assert_eq!(one_space("x\u{2028}y\u{85}z\u{7f}w", 100), "x y z w");
    assert_eq!(one_space("äöü ß", 3), "äöü");
    assert_eq!(one_space("ab cd", 3), "ab");
    assert_eq!(one_space("\n\n", 10), "");
}

#[test]
fn input_longer_than_the_limit_is_cut() {
    let mut t = T::open();
    let long = "x".repeat(MAX_INPUT + 50);
    let out = t.type_text(&long);
    assert_eq!(resolves(&out)[0].1.chars().count(), MAX_INPUT);
}

// --- Abgang (Entscheidung 17: screenshot full/area erst unsichtbar, dann ausführen, dann schließen) ------------------

fn shot(mode: &str) -> Value {
    json!({"intent": {"intent": "screenshot", "mode": mode}, "target": {"kind": "none"}, "display": format!("Screenshot {mode}"),
           "detail": null, "score": 1.0, "class": "run", "target_id": format!("screenshot:{mode}")})
}

fn has_hide(out: &[Effect]) -> bool {
    out.contains(&Effect::Hide)
}

fn run_of(out: &[Effect]) -> Option<(u64, bool)> {
    out.iter().find_map(|e| match e {
        Effect::Execute { run, late, .. } => Some((*run, *late)),
        _ => None,
    })
}

/// Offen, `shot(mode)` gewählt: im Abgang, wartet auf die Hülle
fn leaving(mode: &str) -> T {
    let mut t = T::open();
    t.resolved("shot", answer(vec![shot(mode)], true));
    let g = t.p.gen();
    let out = t.key("Return");
    assert_eq!(out, vec![Effect::Vanish { gen: g }], "{mode}: only the panel turns invisible");
    assert!(t.p.is_open() && t.p.leaving());
    t
}

#[test]
fn screenshot_full_and_area_vanish_then_run_then_close() {
    for mode in ["full", "area"] {
        let mut t = leaving(mode);
        assert_eq!(t.p.history_len(), 0, "remembered only when it runs");
        let g = t.p.gen();
        let out = t.p.vanished(g, t.now);
        assert_eq!(executed(&out), Some((format!("Screenshot {mode}"), false)));
        assert_eq!(run_of(&out), Some((1, false)));
        assert!(!has_hide(&out), "{mode}: closed only after niri took it");
        assert!(t.p.is_open() && t.p.leaving());
        assert_eq!(t.p.history_len(), 1);
        assert_eq!(t.p.next_deadline(), Some(t.now + RUN_MAX));
        assert!(t.p.ran(7).is_empty(), "another run does not close it");
        let out = t.p.ran(1);
        assert!(has_hide(&out));
        assert!(!t.p.is_open() && !t.p.leaving());
        assert!(t.p.ran(1).is_empty(), "the second report (Executed after Accepted) does nothing");
    }
}

#[test]
fn screenshot_window_and_other_intents_close_first() {
    let mut t = T::open();
    t.resolved("shot", answer(vec![shot("window")], true));
    let out = t.key("Return");
    let hide = out.iter().position(|e| *e == Effect::Hide).unwrap();
    let exec = out.iter().position(|e| matches!(e, Effect::Execute { .. })).unwrap();
    assert!(hide < exec);
    assert!(!out.iter().any(|e| matches!(e, Effect::Vanish { .. })));
    assert!(!t.p.is_open() && !t.p.leaving());
    // eine spätere Meldung dieser Ausführung trifft kein offenes Panel
    t.reopen();
    assert!(t.p.ran(1).is_empty());
    assert!(t.p.is_open());
}

#[test]
fn nothing_reacts_while_leaving() {
    let mut t = leaving("full");
    let g = t.p.gen();
    for k in ["Return", "KP_Enter", "Escape", "Down", "Up", "Tab"] {
        assert!(t.key(k).is_empty(), "{k} while vanishing");
    }
    assert!(t.ctrl_key("Return").is_empty());
    assert!(t.ctrl_key("j").is_empty());
    assert!(t.p.activate("row:0", t.now).is_empty());
    assert!(t.p.activate("close", t.now).is_empty());
    assert!(t.p.hover(Some("row:0")).is_empty());
    assert!(t.type_text("other").is_empty());
    assert!(t.p.voice_state(Some(VoicePhase::Listening)).is_empty());
    assert!(t.p.voice.is_none());
    assert!(t.p.voice_final("speech", t.now).is_empty());
    assert!(t.p.resolve_reply(g, 1, Ok(&answer(vec![window(2, "B")], true)), t.now).is_empty());
    assert!(t.p.leaving());
    let out = t.p.vanished(g, t.now);
    assert_eq!(run_of(&out), Some((1, false)));
    // ausgeführt: weiter taub, nichts zweimal
    for k in ["Return", "Escape"] {
        assert!(t.key(k).is_empty(), "{k} while running");
    }
    assert!(t.p.activate("row:0", t.now).is_empty());
    assert!(t.p.vanished(g, t.now).is_empty(), "a second frame runs nothing");
    assert!(t.p.tick(t.now).is_empty());
    assert!(t.p.is_open());
}

#[test]
fn closing_before_the_frame_runs_nothing() {
    // Sperre, Kürzel, Mod+Q: `close`; Fokusverlust: `focus_lost`
    for how in ["close", "focus"] {
        let mut t = leaving("area");
        let g = t.p.gen();
        let out = if how == "close" { t.p.close() } else { t.p.focus_lost() };
        assert!(has_hide(&out) && executed(&out).is_none(), "{how}");
        assert!(!t.p.is_open() && !t.p.leaving());
        assert_eq!(t.p.history_len(), 0, "{how}: nothing ran, nothing remembered");
        assert!(t.p.vanished(g, t.now).is_empty(), "{how}: a late frame runs nothing");
        assert!(t.p.tick(t.now + VANISH_MAX).is_empty());
        // nächstes Öffnen: normal, ein alter Frame zählt nicht
        t.reopen();
        assert!(!t.p.leaving());
        assert!(t.p.vanished(g, t.now).is_empty());
        t.resolved("shot", answer(vec![shot("full")], true));
        let g2 = t.p.gen();
        assert_eq!(t.key("Return"), vec![Effect::Vanish { gen: g2 }]);
        assert!(t.p.vanished(g, t.now).is_empty(), "{how}: the frame of the earlier opening");
        assert_eq!(run_of(&t.p.vanished(g2, t.now)), Some((1, false)));
    }
}

#[test]
fn leaving_runs_after_the_deadline_and_closes_after_the_second() {
    let mut t = leaving("full");
    let start = t.now - Duration::from_millis(120);
    assert_eq!(t.p.next_deadline(), Some(start + VANISH_MAX));
    assert!(t.p.tick(start + VANISH_MAX - Duration::from_millis(1)).is_empty());
    let at = start + VANISH_MAX;
    let out = t.p.tick(at);
    assert_eq!(run_of(&out), Some((1, true)), "runs without the frame");
    let g = t.p.gen();
    assert!(t.p.vanished(g, at).is_empty(), "a late frame runs nothing again");
    assert_eq!(t.p.next_deadline(), Some(at + RUN_MAX));
    assert!(t.p.tick(at + RUN_MAX - Duration::from_millis(1)).is_empty());
    assert!(has_hide(&t.p.tick(at + RUN_MAX)), "niri never answered: closed anyway");
    assert!(t.p.ran(1).is_empty());
}

#[test]
fn an_earlier_run_does_not_close_a_later_leave() {
    // erst ein Fenster (Ausführung 1 nach dem Schließen), dann ein Screenshot (2) im Abgang
    let mut t = T::open();
    t.resolved("firefox", answer(vec![window(1, "Firefox")], true));
    assert_eq!(run_of(&t.key("Return")), Some((1, false)));
    t.reopen();
    t.resolved("shot", answer(vec![shot("full")], true));
    let g = t.p.gen();
    t.key("Return");
    assert!(t.p.ran(1).is_empty(), "the late result of run 1 while vanishing");
    assert_eq!(run_of(&t.p.vanished(g, t.now)), Some((2, false)));
    assert!(t.p.ran(1).is_empty(), "the late result of run 1 while running");
    assert!(t.p.is_open());
    assert!(has_hide(&t.p.ran(2)));
}

#[test]
fn leaving_by_mouse_and_after_the_answer_also_vanishes() {
    let mut t = T::open();
    t.resolved("shot", answer(vec![shot("area")], true));
    let g = t.p.gen();
    assert_eq!(t.p.activate("row:0", t.now), vec![Effect::Vanish { gen: g }]);
    // Return vor der Antwort: die Wahl wartet, die Antwort führt in den Abgang
    let mut t = T::open();
    let seq = last_seq(&t.type_text("shot"));
    assert!(t.key("Return").is_empty());
    let g = t.p.gen();
    let out = t.reply(seq, answer(vec![shot("full")], true));
    assert_eq!(out.last(), Some(&Effect::Vanish { gen: g }), "{out:?}");
    assert!(!has_hide(&out) && executed(&out).is_none());
    assert!(t.p.leaving());
}

#[test]
fn the_daemon_settings_travel_with_the_run_and_reset_on_close() {
    let prefs_of = |out: &[Effect]| {
        out.iter().find_map(|e| match e {
            Effect::Execute { prefs, .. } => Some(prefs.clone()),
            _ => None,
        })
    };
    // lokale Antwort: Suchadresse, Terminal und Befehl im Projekt gehen mit der Ausführung
    let mut t = T::open();
    let mut a = answer(vec![window(1, "Firefox")], true);
    a["terminal"] = json!(["kitty"]);
    a["project_command"] = json!(["claude", "--continue"]);
    a["clipboard_command"] = json!(["copyq", "toggle"]);
    a["emoji_command"] = json!(["bemoji"]);
    t.resolved("firefox", a);
    let want = ExecPrefs {
        web_search_url: "https://search.example/?q={query}".into(),
        terminal: vec!["kitty".into()],
        project_command: vec!["claude".into(), "--continue".into()],
        clipboard_command: vec!["copyq".into(), "toggle".into()],
        emoji_command: vec!["bemoji".into()],
    };
    assert_eq!(prefs_of(&t.key("Return")), Some(want.clone()));
    // neu geöffnet: nichts vom vorigen Mal, bis der Daemon antwortet
    t.p.open(t.now);
    assert_eq!(t.p.prefs, ExecPrefs::default());

    // Antwort der KI ebenso; eine falsch geformte Liste ändert nichts
    let mut t = T::open();
    let mut a = answer(vec![], true);
    a["terminal"] = json!(["foot"]);
    t.resolved("open my mail", a);
    let aseq = asks(&t.key("Return"))[0].0;
    let ai = json!({"ok": true, "intent": {"intent": "launch_app"}, "candidates": [window(5, "Thunderbird")],
                    "web_search_url": "https://s/?q={query}", "terminal": ["kitty", 3], "project_command": ["claude"],
                    "clipboard_command": [], "emoji_command": [null]});
    let g = t.p.gen();
    t.p.ask_reply(g, aseq, Ok(&ai), t.now);
    t.later(DEADTIME);
    let got = prefs_of(&t.key("Return")).expect("executed");
    assert_eq!(got.web_search_url, "https://s/?q={query}");
    assert_eq!(got.terminal, ["foot"], "a list with a number is ignored");
    assert_eq!(got.project_command, ["claude"]);
    assert!(got.clipboard_command.is_empty(), "an empty list sets automatic");
    assert!(got.emoji_command.is_empty(), "a list with null is ignored");
}
