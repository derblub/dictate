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

//! Automat des Panels der Befehlspalette (Port von `palette.lua`, PLAN-wayland.md 6g „Panel in dictate-ui“, Regeln mit
//! Lua-Zeilen in `c552237:docs/w5-vorlagen/cmdpal-spec.md`): rein, ohne GTK und ohne Uhr. Jede Eingabe trägt `now`, jede Folge
//! ist ein [`Effect`], den der Hauptthread von `dictate-ui` ausführt (Fenster, `pal_resolve`/`pal_ask` an den Daemon,
//! Ausführen über `ui::pal_exec`, Aufnahme abbrechen). Nicht verwechseln mit `ui::palette` (Farben von Noctalia).
//!
//! **Anfragen:** jede Änderung der Eingabe fragt den Daemon lokal (`pal_resolve`); unterwegs ist höchstens eine, dazu
//! höchstens eine wartende (die jüngste gewinnt, `seq`). Eine Antwort zählt nur, wenn sie zur letzten Anfrage gehört und
//! nichts wartet; `gen` steigt bei jedem Öffnen und Schließen (spätere Antworten einer alten Öffnung fallen weg). Die KI
//! (`pal_ask`) fragt Return, wenn die lokale Liste nicht sicher ist ([`CmdPal::enter_asks`]), oder Ctrl+Return immer;
//! ihre Vorschau bleibt bis zur nächsten Änderung der Eingabe stehen, jede Änderung, Esc und Schließen legen eine
//! laufende Frage auf. Return vor der Antwort auf die aktuelle Eingabe merkt die Wahl (`want`) und wählt danach.
//!
//! **Bestätigung:** Kandidaten der Klasse `confirm` (und jede Absicht mit Bestätigungspflicht laut Katalog, L18) brauchen
//! ein zweites Return bzw. den Knopf „Confirm“; ein zweiter Klick auf die Zeile bestätigt nicht (ein langsamer
//! Doppelklick auf „Power off“ schaltete sonst aus, Entscheidung 18). Esc, Tippen und eine andere Zeile heben sie auf. Ausgeführt wird erst
//! nach dem Schließen ([`Effect::Execute`] folgt auf [`Effect::Hide`]).
//!
//! **Abgang** (Entscheidung 17, nur `screenshot` `full` und `area`, [`Intent::vanishes_first`]): das Panel wird zuerst
//! unsichtbar ([`Effect::Vanish`]), ausgeführt wird, sobald die Hülle meldet, dass niri den durchsichtigen Frame hat
//! ([`CmdPal::vanished`]) oder nach [`VANISH_MAX`], geschlossen erst, wenn niri die Aktion annahm oder die Ausführung
//! endete ([`CmdPal::ran`]) oder nach [`RUN_MAX`]. In dieser Zeit wirkt keine Eingabe (Tasten samt Esc, Klicks, Hover,
//! Tippen, Antworten, Stimme; Lesart L39); Schließen (Sperre, Fokusverlust, Kürzel, Mod+Q) vor dem Ausführen führt nichts
//! aus und merkt nichts im Verlauf.
//!
//! **Stimme:** eine Aufnahme des Panels (Besitz entscheidet der Hauptthread beim Start, L23) zeigt sich statt der Liste;
//! ihr `final` ersetzt die Eingabe ([`one_space`], höchstens [`MAX_INPUT`] Zeichen) und wird nie ausgeführt. Esc bricht
//! die Aufnahme ab ([`Effect::VoiceCancel`]), ebenso das Schließen.
//!
//! **Totzeit** über [`Gate`] (L19): sie sperrt nur Return, Ctrl+Return und Klicks, nie Tippen, Pfeile, Tab oder Esc.
//! Leerlauf [`IDLE`] schließt (L1). Der Verlauf (höchstens [`HISTORY_MAX`] gewählte Eingaben) lebt nur im Speicher.
//!
//! Eingabe, Kandidaten (Fenstertitel, Programme) und Antworten nie loggen; beim Schließen werden sie überschrieben.

use crate::actions::store::wipe;
use crate::ui::diag_panel::CallErr;
use crate::ui::gate::{Gate, IDLE};
use crate::ui::pal_exec::{Candidate, Class, ExecPrefs};
use crate::ui::panel::Mods;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Höchstens so viele Kandidaten zeigt das Panel (`MAX_CANDIDATES` in Lua)
pub const MAX_CANDIDATES: usize = 12;
/// Höchstlänge der Eingabe in Zeichen (`palette::MAX_INPUT_CHARS` im Daemon)
pub const MAX_INPUT: usize = 1000;
/// Höchstzahl der gemerkten Eingaben
pub const HISTORY_MAX: usize = 50;
/// Abgang: so lange wartet das unsichtbare Panel höchstens auf die Meldung der Hülle, dann wird trotzdem ausgeführt
pub const VANISH_MAX: Duration = Duration::from_millis(300);
/// Abgang: so lange bleibt das unsichtbare Panel nach dem Ausführen höchstens offen (die niri-IPC hat 1,5 s je Schritt;
/// das Panel hält bis dahin den Tastaturfokus)
pub const RUN_MAX: Duration = Duration::from_secs(2);
/// Anfang der Meldung des Daemons, wenn die Palette aus ist (neutral, nicht rot)
pub const OFF_PREFIX: &str = "The command palette is off";
/// Meldung, wenn der Daemon fehlt (wie bei den anderen Panels)
pub const DAEMON_DOWN: &str = crate::ui::panel::DAEMON_DOWN;
const DAEMON_DOWN_BODY: &str = "Start it with: systemctl --user start dictate";
const RETRY_AI: &str = "Edit the input, or press Enter to try again.";

/// Ein Fehler im Panel: Titel, optional eine graue Zeile darunter; `neutral` = nicht rot (Palette aus)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub title: String,
    pub body: Option<String>,
    pub neutral: bool,
}

impl Problem {
    fn new(title: &str, body: Option<&str>) -> Problem {
        Problem { title: title.to_string(), body: body.map(str::to_string), neutral: false }
    }
}

/// Phase einer Aufnahme des Panels (Zustand des Daemons)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePhase {
    Loading,
    Listening,
    Finalizing,
}

/// Laufende Aufnahme des Panels
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub phase: VoicePhase,
    pub stable: String,
    pub tail: String,
    /// Pegel 0..1 (nur beim Zuhören)
    pub level: f32,
}

/// Laufende Frage an die KI
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asking {
    pub seq: u64,
    pub input: String,
    pub started: Instant,
}

/// Wie gewählt wurde (für das Log und `pal_chosen` gleich; Lua unterschied es nur in den Zählern)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Key,
    Mouse,
}

/// Arbeiter im Binary: lokale Anfragen und KI laufen getrennt (eine lokale Antwort darf während der KI kommen)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Resolve,
    Ask,
}

/// Folgen für den Hauptthread
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Fenster zeigen (Quelle Palette)
    Show,
    Hide,
    /// Inhalt neu aufbauen
    Render,
    /// Nur Hervorhebung und Fuß neu (Auswahl bewegt, Eingabe geändert): kein Neuaufbau
    Highlight,
    /// Text der Aufnahme bzw. Pegel neu (kein Neuaufbau)
    VoiceText,
    /// Eingabezeile programmatisch setzen (Verlauf, Stimme); meldet keine Änderung zurück
    SetInput(String),
    /// `pal_resolve` mit dem Kontext der Öffnung
    Resolve {
        gen: u64,
        seq: u64,
        input: String,
    },
    /// `pal_ask` auf eigener Verbindung
    Ask {
        gen: u64,
        seq: u64,
        input: String,
    },
    /// Laufende Anfrage auflegen (der Daemon bricht ab)
    Hangup {
        lane: Lane,
        seq: u64,
    },
    /// Nach dem Schließen ausführen (`ui::pal_exec::execute` in einem Arbeiter); im Abgang vor dem Schließen. `run`
    /// kommt mit dem Ergebnis zurück ([`CmdPal::ran`]); `late`: im Abgang ohne Meldung der Hülle nach [`VANISH_MAX`]
    Execute {
        cand: Box<Candidate>,
        confirmed: bool,
        prefs: ExecPrefs,
        via: Via,
        run: u64,
        late: bool,
    },
    /// Abgang: Panel unsichtbar machen und [`CmdPal::vanished`] mit `gen` melden, sobald niri den Frame hat
    Vanish {
        gen: u64,
    },
    /// Aufnahme des Panels abbrechen (Daemon `cancel`, ein spätes `final` wird verworfen)
    VoiceCancel,
}

/// Zähler für Tests und das Log (keine Inhalte)
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counters {
    pub sent: u64,
    pub skipped: u64,
    pub discarded: u64,
    pub invalid: u64,
    pub asks: u64,
    pub ask_cancels: u64,
    pub voice_inputs: u64,
}

/// Gewählter Kandidat, der auf das Ausführen wartet
#[derive(Debug)]
struct Run {
    cand: Candidate,
    confirmed: bool,
    prefs: ExecPrefs,
    via: Via,
    /// Eingabe für den Verlauf (gemerkt erst beim Ausführen)
    input: String,
}

impl Run {
    /// Verworfen ohne Ausführen: Eingabe und Anzeige überschreiben
    fn wipe(&mut self) {
        wipe(&mut self.input);
        wipe(&mut self.cand.display);
        wipe(&mut self.cand.target_id);
        if let Some(d) = self.cand.detail.as_mut() {
            wipe(d);
        }
    }
}

/// Abgang (Entscheidung 17)
#[derive(Debug)]
enum Leave {
    /// Panel unsichtbar gemacht, wartet auf die Meldung der Hülle
    Vanishing { since: Instant, run: Box<Run> },
    /// Ausgeführt, wartet auf niri bzw. das Ende der Ausführung
    Running { since: Instant, run: u64 },
}

#[derive(Debug, Default)]
pub struct CmdPal {
    open: bool,
    gen: u64,
    /// Aktuelle Eingabe (die Eingabezeile spiegelt sie)
    pub input: String,
    pub cands: Vec<Candidate>,
    /// Eingabe, zu der `cands` gehört (`None`: seit dem Öffnen noch keine Antwort)
    pub shown_for: Option<String>,
    /// Hervorgehobene Zeile (ab 0)
    pub sel: Option<usize>,
    /// Zeile, die auf das zweite Return wartet
    pub confirm: Option<usize>,
    /// Fehler der lokalen Anfrage: ersetzt die Liste
    pub err: Option<Problem>,
    /// Fehler der KI: steht über der Liste
    pub ask_err: Option<Problem>,
    seq: u64,
    /// Laufende lokale Anfrage: Nummer
    inflight: Option<u64>,
    /// Wartende lokale Anfrage: Nummer und Eingabe
    pending: Option<(u64, String)>,
    /// Eingabe der laufenden lokalen Anfrage
    inflight_input: String,
    confident: bool,
    /// Der User hat die Auswahl selbst bewegt (Pfeile, Tab; Hover nicht)
    picked: bool,
    ask_seq: u64,
    pub asking: Option<Asking>,
    /// Eingabe, zu der die KI-Vorschau gehört (die Liste ist dann die der KI)
    pub preview: Option<String>,
    want: Option<Via>,
    pub voice: Option<Voice>,
    history: Vec<String>,
    hist_idx: Option<usize>,
    hist_draft: Option<String>,
    /// Aus der letzten Antwort des Daemons (Suchadresse, Terminal, Befehl im Projekt)
    prefs: ExecPrefs,
    gate: Gate,
    /// Abgang läuft (Panel offen, aber unsichtbar)
    leave: Option<Leave>,
    /// Nummer der letzten Ausführung
    run_seq: u64,
    pub counters: Counters,
}

/// Steuerzeichen und Leerraum zu einem Leerzeichen, vorn und hinten ohne, höchstens `max` Zeichen (`one_space`, pal:1066)
pub fn one_space(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in text.chars().take(max.saturating_mul(8)) {
        let blank = c.is_control() || c.is_whitespace() || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}');
        if blank {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    if out.chars().count() > max {
        out = out.chars().take(max).collect();
        out.truncate(out.trim_end().len());
    }
    out
}

fn has_text(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

impl CmdPal {
    pub fn new() -> CmdPal {
        CmdPal::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Abgang läuft: das Panel ist offen, aber unsichtbar und nimmt nichts mehr an (auch keine Aufnahme)
    pub fn leaving(&self) -> bool {
        self.leave.is_some()
    }

    /// Offen und bedienbar (nicht im Abgang)
    fn live(&self) -> bool {
        self.open && self.leave.is_none()
    }

    pub fn gen(&self) -> u64 {
        self.gen
    }

    /// Lokale Anfrage läuft oder wartet
    pub fn resolving(&self) -> bool {
        self.inflight.is_some() || self.pending.is_some()
    }

    /// Zahl der gemerkten Eingaben (für das Log)
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Öffnen (der Hauptthread hat Kontext und Fokus vorher gemerkt): leere Eingabe fragt Häufigkeit und Fenster
    pub fn open(&mut self, now: Instant) -> Vec<Effect> {
        if self.open {
            return vec![];
        }
        self.reset();
        self.open = true;
        self.gen += 1;
        self.gate.open(now);
        let mut out = vec![Effect::Show, Effect::Render];
        out.extend(self.request(String::new()));
        out
    }

    /// Schließen: laufende Anfragen auflegen, Aufnahme abbrechen, alles vergessen (der Verlauf bleibt)
    pub fn close(&mut self) -> Vec<Effect> {
        if !self.open {
            return vec![];
        }
        let mut out = vec![];
        if let Some(seq) = self.inflight {
            out.push(Effect::Hangup { lane: Lane::Resolve, seq });
        }
        out.extend(self.cancel_ask());
        if self.voice.is_some() {
            out.push(Effect::VoiceCancel);
        }
        if let Some(Leave::Vanishing { mut run, .. }) = self.leave.take() {
            // vor dem Ausführen geschlossen (Sperre, Fokusverlust, Kürzel): nichts ausführen, nichts merken
            run.wipe();
        }
        self.open = false;
        self.gen += 1;
        self.reset();
        self.gate.clear();
        out.push(Effect::Hide);
        out
    }

    fn reset(&mut self) {
        wipe(&mut self.input);
        wipe(&mut self.inflight_input);
        for c in &mut self.cands {
            wipe(&mut c.display);
            if let Some(d) = c.detail.as_mut() {
                wipe(d);
            }
        }
        self.cands.clear();
        self.shown_for = None;
        self.sel = None;
        self.confirm = None;
        self.err = None;
        self.ask_err = None;
        self.inflight = None;
        self.pending = None;
        self.confident = false;
        self.picked = false;
        self.asking = None;
        self.preview = None;
        self.want = None;
        self.voice = None;
        self.hist_idx = None;
        self.hist_draft = None;
        self.prefs = ExecPrefs::default();
    }

    // --- Anfragen -----------------------------------------------------------------------------------------------

    fn request(&mut self, input: String) -> Vec<Effect> {
        self.seq += 1;
        if self.inflight.is_some() {
            if self.pending.is_some() {
                self.counters.skipped += 1;
            }
            self.pending = Some((self.seq, input));
            return vec![];
        }
        self.send(self.seq, input)
    }

    fn send(&mut self, seq: u64, input: String) -> Vec<Effect> {
        self.counters.sent += 1;
        self.inflight = Some(seq);
        self.inflight_input = input.clone();
        vec![Effect::Resolve { gen: self.gen, seq, input }]
    }

    fn next_pending(&mut self) -> Vec<Effect> {
        match self.pending.take() {
            Some((seq, input)) => self.send(seq, input),
            None => vec![],
        }
    }

    /// Antwort auf `pal_resolve` (`Ok` = `ok: true`; Fehler des Daemons als [`CallErr::Failed`])
    pub fn resolve_reply(&mut self, gen: u64, seq: u64, res: Result<&Value, CallErr>, now: Instant) -> Vec<Effect> {
        if !self.live() || gen != self.gen || self.inflight != Some(seq) {
            return vec![];
        }
        self.inflight = None;
        let input = std::mem::take(&mut self.inflight_input);
        if self.pending.is_some() || seq != self.seq {
            self.counters.discarded += 1;
            return self.next_pending();
        }
        if matches!(res, Err(CallErr::Cancelled)) {
            return vec![];
        }
        self.apply(input, res, now)
    }

    fn apply(&mut self, input: String, res: Result<&Value, CallErr>, now: Instant) -> Vec<Effect> {
        if self.preview.is_some() {
            // die Vorschau der KI bleibt bis zur nächsten Änderung
            self.counters.discarded += 1;
            self.want = None;
            return vec![];
        }
        self.shown_for = Some(input);
        self.confirm = None;
        self.picked = false;
        self.cands.clear();
        self.sel = None;
        self.err = None;
        match res {
            Ok(v) => match self.candidates(v) {
                Some(c) => {
                    self.cands = c;
                    self.confident = v["confident"] == true;
                    self.prefs.update(v);
                    self.sel = (!self.cands.is_empty()).then_some(0);
                }
                None => self.err = Some(Problem::new("dictate sent an invalid answer", None)),
            },
            Err(CallErr::DaemonDown) => self.err = Some(Problem::new(DAEMON_DOWN, Some(DAEMON_DOWN_BODY))),
            Err(CallErr::Timeout) => self.err = Some(Problem::new("No answer from dictate", Some("Type to try again."))),
            Err(CallErr::Failed(m)) => {
                let neutral = m.starts_with(OFF_PREFIX);
                self.err = Some(Problem { title: m, body: None, neutral });
            }
            Err(CallErr::Cancelled) => {}
        }
        let want = self.want.take();
        let mut out = vec![Effect::Render];
        match want {
            Some(Via::Key) if self.enter_asks() => out.extend(self.start_ask(now)),
            Some(via) => {
                if let Some(i) = self.sel {
                    out.extend(self.choose(i, via, now));
                }
            }
            None => {}
        }
        out
    }

    /// Gültige Kandidaten der Antwort (höchstens [`MAX_CANDIDATES`]); `None`, wenn `candidates` fehlt
    fn candidates(&mut self, v: &Value) -> Option<Vec<Candidate>> {
        let list = v["candidates"].as_array()?;
        let mut out = vec![];
        for c in list {
            if out.len() >= MAX_CANDIDATES {
                break;
            }
            match Candidate::from_value(c) {
                Some(c) => out.push(c),
                None => self.counters.invalid += 1,
            }
        }
        Some(out)
    }

    /// Fragt Return die KI? (`enter_asks`, pal:350)
    pub fn enter_asks(&self) -> bool {
        has_text(&self.input)
            && self.preview.is_none()
            && self.asking.is_none()
            && self.err.is_none()
            && self.shown_for.as_deref() == Some(self.input.as_str())
            && (self.cands.is_empty() || (!self.confident && !self.picked))
    }

    // --- KI -------------------------------------------------------------------------------------------------------

    fn start_ask(&mut self, now: Instant) -> Vec<Effect> {
        if !self.open || !has_text(&self.input) {
            return vec![];
        }
        let mut out = self.cancel_ask();
        self.want = None;
        self.ask_seq += 1;
        self.counters.asks += 1;
        self.asking = Some(Asking { seq: self.ask_seq, input: self.input.clone(), started: now });
        self.preview = None;
        self.ask_err = None;
        self.confirm = None;
        out.push(Effect::Ask { gen: self.gen, seq: self.ask_seq, input: self.input.clone() });
        out.push(Effect::Render);
        out
    }

    fn cancel_ask(&mut self) -> Vec<Effect> {
        match self.asking.take() {
            Some(a) => {
                self.counters.ask_cancels += 1;
                vec![Effect::Hangup { lane: Lane::Ask, seq: a.seq }]
            }
            None => vec![],
        }
    }

    /// Antwort auf `pal_ask`
    pub fn ask_reply(&mut self, gen: u64, seq: u64, res: Result<&Value, CallErr>, now: Instant) -> Vec<Effect> {
        if !self.live() || gen != self.gen || self.asking.as_ref().map(|a| a.seq) != Some(seq) {
            return vec![];
        }
        let Some(a) = self.asking.take() else { return vec![] };
        self.ask_err = None;
        match res {
            Ok(v) => match self.candidates(v) {
                Some(c) if c.is_empty() => self.ask_err = Some(Problem::new("The AI found no matching command", Some("Rephrase the input, then press Enter."))),
                Some(c) => {
                    self.preview = Some(a.input.clone());
                    self.cands = c;
                    self.sel = Some(0);
                    self.shown_for = Some(a.input);
                    self.err = None;
                    self.picked = false;
                    self.confident = false;
                    self.confirm = None;
                    self.prefs.update(v);
                    // die Liste wechselt unter dem Finger: ein schnelles zweites Return führt nicht sofort aus
                    self.gate.changed(now);
                }
                None => self.ask_err = Some(Problem::new("dictate sent an invalid answer", Some(RETRY_AI))),
            },
            Err(CallErr::DaemonDown) => self.ask_err = Some(Problem::new(DAEMON_DOWN, Some(DAEMON_DOWN_BODY))),
            Err(CallErr::Timeout) => self.ask_err = Some(Problem::new("No answer from the AI", Some(RETRY_AI))),
            Err(CallErr::Failed(m)) => {
                let neutral = m.starts_with(OFF_PREFIX);
                self.ask_err = Some(Problem { body: (!neutral).then(|| RETRY_AI.to_string()), title: m, neutral });
            }
            Err(CallErr::Cancelled) => {}
        }
        vec![Effect::Render]
    }

    // --- Eingabe --------------------------------------------------------------------------------------------------

    /// Die Eingabezeile hat jetzt `text` (Tippen, Löschen, Einfügen; nie ein programmatisches Setzen)
    pub fn text_changed(&mut self, text: &str, now: Instant) -> Vec<Effect> {
        if !self.live() {
            return vec![];
        }
        self.gate.touch(now);
        let text: String = text.chars().take(MAX_INPUT).collect();
        if text == self.input {
            return vec![];
        }
        wipe(&mut self.input);
        self.input = text.clone();
        self.hist_idx = None;
        self.hist_draft = None;
        self.want = None;
        let mut out = self.cancel_ask();
        let was_ai = !out.is_empty() || self.preview.is_some() || self.ask_err.is_some();
        self.preview = None;
        self.ask_err = None;
        if self.confirm.is_some() || was_ai {
            self.confirm = None;
            out.push(Effect::Render);
        } else {
            out.push(Effect::Highlight);
        }
        out.extend(self.request(text));
        out
    }

    pub fn key_release(&mut self, key: &str, now: Instant) {
        self.gate.release(key, now);
    }

    /// Taste, die die Eingabezeile nicht selbst behandelt (Liste in der Ansicht: Esc, Return, Tab, Pfeile, Ctrl+J/M)
    pub fn key_press(&mut self, key: &str, mods: Mods, now: Instant) -> Vec<Effect> {
        self.gate.press(key, now);
        if !self.open {
            return vec![];
        }
        if self.leave.is_some() {
            self.gate.ignored += 1;
            return vec![];
        }
        self.gate.touch(now);
        if key == "Escape" {
            if self.confirm.is_some() {
                return self.cancel_confirm(now);
            }
            if self.voice.is_some() {
                self.voice = None;
                self.gate.changed(now);
                return vec![Effect::VoiceCancel, Effect::Render];
            }
            if self.asking.is_some() {
                let mut out = self.cancel_ask();
                self.gate.changed(now);
                out.push(Effect::Render);
                return out;
            }
            return self.close();
        }
        if self.voice.is_some() {
            self.gate.ignored += 1;
            return vec![];
        }
        let enter_key = key == "Return" || key == "KP_Enter";
        if enter_key && mods.ctrl {
            if self.gate.fire(key, now) && self.confirm.is_none() {
                return self.start_ask(now);
            }
            return vec![];
        }
        if enter_key || (mods.ctrl && matches!(key, "j" | "m" | "J" | "M")) {
            return self.enter(key, now);
        }
        if !mods.plain() {
            return vec![];
        }
        match key {
            "Tab" => self.move_sel(if mods.shift { -1 } else { 1 }, now),
            "ISO_Left_Tab" => self.move_sel(-1, now),
            "Up" => {
                if self.hist_idx.is_some() || self.sel.is_none_or(|s| s == 0) {
                    self.history_step(-1, now)
                } else {
                    self.move_sel(-1, now)
                }
            }
            "Down" => {
                if self.hist_idx.is_some() {
                    self.history_step(1, now)
                } else {
                    self.move_sel(1, now)
                }
            }
            _ => vec![],
        }
    }

    fn enter(&mut self, key: &str, now: Instant) -> Vec<Effect> {
        if !self.gate.fire(key, now) {
            return vec![];
        }
        if let Some(i) = self.confirm {
            return self.execute(i, Via::Key, now);
        }
        if self.asking.is_some() {
            // wartet auf die KI; Esc bricht ab
            self.gate.ignored += 1;
            return vec![];
        }
        if self.enter_asks() {
            return self.start_ask(now);
        }
        if self.shown_for.as_deref() != Some(self.input.as_str()) && self.resolving() {
            self.want = Some(Via::Key);
            return vec![];
        }
        match self.sel {
            Some(i) => self.choose(i, Via::Key, now),
            None => vec![],
        }
    }

    fn cancel_confirm(&mut self, now: Instant) -> Vec<Effect> {
        self.confirm = None;
        self.gate.changed(now);
        vec![Effect::Render]
    }

    fn move_sel(&mut self, dir: isize, now: Instant) -> Vec<Effect> {
        let n = self.cands.len();
        if n == 0 {
            return vec![];
        }
        self.hist_idx = None;
        self.hist_draft = None;
        self.picked = true;
        let i = match self.sel {
            Some(s) => (s as isize + dir).rem_euclid(n as isize) as usize,
            None if dir > 0 => 0,
            None => n - 1,
        };
        self.select_row(i, now)
    }

    fn select_row(&mut self, i: usize, now: Instant) -> Vec<Effect> {
        if self.sel == Some(i) {
            // `picked` kann sich geändert haben: der Fuß folgt
            return vec![Effect::Highlight];
        }
        self.sel = Some(i);
        if self.confirm.is_some() {
            self.confirm = None;
            self.gate.changed(now);
            return vec![Effect::Render];
        }
        vec![Effect::Highlight]
    }

    /// Wählt Zeile `i`: `confirm` erst bestätigen lassen, sonst ausführen (`choose`, pal:583)
    fn choose(&mut self, i: usize, via: Via, now: Instant) -> Vec<Effect> {
        if !self.open {
            return vec![];
        }
        if self.shown_for.as_deref() != Some(self.input.as_str()) && self.resolving() {
            self.want = Some(via);
            return vec![];
        }
        let Some(c) = self.cands.get(i) else { return vec![] };
        if via == Via::Mouse && self.confirm == Some(i) {
            // bestätigt wird mit der Maus nur über „Confirm“ (Modulkopf)
            return vec![];
        }
        if (c.class == Class::Confirm || c.intent.needs_confirm()) && self.confirm != Some(i) {
            self.confirm = Some(i);
            self.sel = Some(i);
            self.gate.changed(now);
            return vec![Effect::Render];
        }
        self.execute(i, via, now)
    }

    fn execute(&mut self, i: usize, via: Via, now: Instant) -> Vec<Effect> {
        let Some(cand) = self.cands.get(i).cloned() else { return vec![] };
        let confirmed = self.confirm == Some(i);
        let run = Run { cand, confirmed, prefs: self.prefs.clone(), via, input: self.input.clone() };
        if run.cand.intent.vanishes_first() {
            // Abgang: laufende Anfragen auflegen (ihre Antworten zählen nicht mehr), Panel unsichtbar, warten
            let mut out = vec![];
            if let Some(seq) = self.inflight.take() {
                out.push(Effect::Hangup { lane: Lane::Resolve, seq });
            }
            self.pending = None;
            out.extend(self.cancel_ask());
            self.want = None;
            self.leave = Some(Leave::Vanishing { since: now, run: Box::new(run) });
            out.push(Effect::Vanish { gen: self.gen });
            return out;
        }
        let mut out = self.close();
        out.push(self.start(run, false));
        out
    }

    /// Ausführen: Eingabe in den Verlauf, `run` zählt hoch
    fn start(&mut self, run: Run, late: bool) -> Effect {
        let Run { cand, confirmed, prefs, via, input } = run;
        self.remember(input);
        self.run_seq += 1;
        Effect::Execute { cand: Box::new(cand), confirmed, prefs, via, run: self.run_seq, late }
    }

    /// Abgang: die Hülle meldet, dass niri den unsichtbaren Frame hat (`gen` der Öffnung): jetzt ausführen
    pub fn vanished(&mut self, gen: u64, now: Instant) -> Vec<Effect> {
        if !self.open || gen != self.gen {
            return vec![];
        }
        self.go(now, false)
    }

    fn go(&mut self, now: Instant, late: bool) -> Vec<Effect> {
        if !matches!(self.leave, Some(Leave::Vanishing { .. })) {
            return vec![];
        }
        let Some(Leave::Vanishing { run, .. }) = self.leave.take() else { return vec![] };
        let e = self.start(*run, late);
        self.leave = Some(Leave::Running { since: now, run: self.run_seq });
        vec![e]
    }

    /// niri nahm die Ausführung `run` an bzw. sie endete: ein Abgang mit dieser Nummer schließt jetzt
    pub fn ran(&mut self, run: u64) -> Vec<Effect> {
        match self.leave {
            Some(Leave::Running { run: r, .. }) if r == run && self.open => self.close(),
            _ => vec![],
        }
    }

    fn remember(&mut self, text: String) {
        if !has_text(&text) {
            return;
        }
        self.history.retain(|h| *h != text);
        self.history.push(text);
        if self.history.len() > HISTORY_MAX {
            let extra = self.history.len() - HISTORY_MAX;
            for mut h in self.history.drain(..extra) {
                wipe(&mut h);
            }
        }
    }

    /// Verlauf: `dir` -1 älter, +1 neuer (`history_step`, pal:1295; vergisst auch eine wartende Wahl, L20)
    fn history_step(&mut self, dir: isize, now: Instant) -> Vec<Effect> {
        let n = self.history.len();
        if n == 0 {
            return vec![];
        }
        let idx = match self.hist_idx {
            None if dir > 0 => return vec![],
            None => {
                self.hist_draft = Some(self.input.clone());
                n - 1
            }
            Some(i) => (i as isize + dir).max(0) as usize,
        };
        let text = if idx >= n {
            self.hist_idx = None;
            self.hist_draft.take().unwrap_or_default()
        } else {
            self.hist_idx = Some(idx);
            self.history[idx].clone()
        };
        self.want = None;
        if text == self.input {
            return vec![];
        }
        self.gate.touch(now);
        wipe(&mut self.input);
        self.input = text.clone();
        self.confirm = None;
        let mut out = self.cancel_ask();
        self.preview = None;
        self.ask_err = None;
        out.push(Effect::SetInput(text.clone()));
        out.push(Effect::Render);
        out.extend(self.request(text));
        out
    }

    // --- Maus -----------------------------------------------------------------------------------------------------

    /// Klick auf ein Bedienelement: `row:N`, `confirm`, `cancel`, `close`
    pub fn activate(&mut self, id: &str, now: Instant) -> Vec<Effect> {
        if !self.live() {
            return vec![];
        }
        self.gate.touch(now);
        if id == "close" {
            return self.close();
        }
        if !self.gate.click(now) {
            return vec![];
        }
        match id {
            "confirm" => match self.confirm {
                Some(i) => self.execute(i, Via::Mouse, now),
                None => vec![],
            },
            "cancel" => self.cancel_confirm(now),
            _ => match id.strip_prefix("row:").and_then(|n| n.parse::<usize>().ok()) {
                Some(i) if self.voice.is_none() && self.asking.is_none() => self.choose(i, Via::Mouse, now),
                _ => vec![],
            },
        }
    }

    /// Maus über einem Element: eine Zeile wird hervorgehoben (setzt `picked` nicht; nicht während einer Bestätigung)
    pub fn hover(&mut self, id: Option<&str>) -> Vec<Effect> {
        let Some(i) = id.and_then(|id| id.strip_prefix("row:")).and_then(|n| n.parse::<usize>().ok()) else { return vec![] };
        if !self.live() || self.confirm.is_some() || i >= self.cands.len() || self.sel == Some(i) {
            return vec![];
        }
        self.sel = Some(i);
        vec![Effect::Highlight]
    }

    // --- Stimme ---------------------------------------------------------------------------------------------------

    /// Zustand einer Aufnahme des Panels; `None`: sie endete ohne `final`
    pub fn voice_state(&mut self, phase: Option<VoicePhase>) -> Vec<Effect> {
        if !self.live() {
            self.voice = None;
            return vec![];
        }
        match (phase, self.voice.as_mut()) {
            (None, None) => vec![],
            (None, Some(_)) => {
                self.voice = None;
                vec![Effect::Render]
            }
            (Some(p), Some(v)) if v.phase == p => vec![],
            (Some(p), Some(v)) => {
                v.phase = p;
                vec![Effect::Render]
            }
            (Some(p), None) => {
                let mut out = self.cancel_ask();
                self.confirm = None;
                self.want = None;
                self.voice = Some(Voice { phase: p, stable: String::new(), tail: String::new(), level: 0.0 });
                out.push(Effect::Render);
                out
            }
        }
    }

    /// Zwischenstand der Aufnahme
    pub fn voice_partial(&mut self, stable: &str, tail: &str) -> Vec<Effect> {
        match self.voice.as_mut() {
            Some(v) if self.leave.is_none() && self.open => {
                wipe(&mut v.stable);
                wipe(&mut v.tail);
                v.stable = one_space(stable, MAX_INPUT);
                v.tail = one_space(tail, MAX_INPUT);
                vec![Effect::VoiceText]
            }
            _ => vec![],
        }
    }

    /// Pegel in dBFS (wie im Overlay auf 0..1)
    pub fn voice_level(&mut self, level: f32) -> Vec<Effect> {
        match self.voice.as_mut() {
            Some(v) if self.leave.is_none() && self.open && v.phase == VoicePhase::Listening => {
                v.level = level.clamp(0.0, 1.0);
                vec![Effect::VoiceText]
            }
            _ => vec![],
        }
    }

    /// `final` der Aufnahme des Panels: ersetzt die Eingabe ganz, nie ausführen
    pub fn voice_final(&mut self, text: &str, now: Instant) -> Vec<Effect> {
        let had = self.voice.take().is_some();
        if !self.live() {
            return vec![];
        }
        let clean = one_space(text, MAX_INPUT);
        let mut out = vec![];
        if !clean.is_empty() {
            self.counters.voice_inputs += 1;
            out.push(Effect::SetInput(clean.clone()));
            out.extend(self.text_changed(&clean, now));
        }
        if had || !clean.is_empty() {
            out.retain(|e| *e != Effect::Highlight && *e != Effect::Render);
            out.push(Effect::Render);
        }
        out
    }

    // --- Zeit, Fokus ----------------------------------------------------------------------------------------------

    pub fn focus_lost(&mut self) -> Vec<Effect> {
        self.close()
    }

    /// Nächster Zeitpunkt für [`Self::tick`] (Leerlauf)
    pub fn next_deadline(&self) -> Option<Instant> {
        if !self.open {
            return None;
        }
        match &self.leave {
            Some(Leave::Vanishing { since, .. }) => Some(*since + VANISH_MAX),
            Some(Leave::Running { since, .. }) => Some(*since + RUN_MAX),
            None => self.gate.last_input().map(|t| t + IDLE),
        }
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        if !self.open {
            return vec![];
        }
        match &self.leave {
            Some(Leave::Vanishing { since, .. }) if now >= *since + VANISH_MAX => self.go(now, true),
            Some(Leave::Running { since, .. }) if now >= *since + RUN_MAX => self.close(),
            Some(_) => vec![],
            None if self.gate.idle(now) => self.close(),
            None => vec![],
        }
    }

    /// Sekunden seit Beginn der Frage an die KI (Anzeige)
    pub fn ask_secs(&self, now: Instant) -> Option<u64> {
        self.asking.as_ref().map(|a| now.saturating_duration_since(a.started).as_secs())
    }

    /// Verb für Return auf dem Kandidaten (`verb`, pal:1214)
    pub fn verb(c: Option<&Candidate>) -> &'static str {
        match c {
            None => "Run",
            Some(c) if c.intent.name() == "diagnose" => "Open",
            Some(c) if c.class == Class::Ai => "Ask",
            Some(c) if c.class == Class::Confirm || c.intent.needs_confirm() => "Run…",
            Some(_) => "Run",
        }
    }

    /// Tastenhinweise des Fußes (Taste, Text); ohne Bestätigung (die ersetzt den Fuß)
    pub fn footer(&self) -> Vec<(&'static str, &'static str)> {
        if self.voice.is_some() || self.asking.is_some() {
            return vec![("Esc", "Cancel")];
        }
        let mut out = vec![];
        if self.cands.len() > 1 {
            out.push(("↑↓", "Select"));
        }
        if self.enter_asks() {
            out.push(("Enter", "Ask AI"));
        } else {
            if !self.cands.is_empty() {
                out.push(("Enter", CmdPal::verb(self.sel.and_then(|i| self.cands.get(i)))));
            }
            if has_text(&self.input) {
                out.push(("Ctrl+Enter", "Ask AI"));
            }
        }
        out.push(("Mod+E", "Hold to speak"));
        out.push(("Esc", "Close"));
        out
    }
}

#[cfg(test)]
#[path = "cmdpal_tests.rs"]
mod tests;
